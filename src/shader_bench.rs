use std::fs;
use std::path::Path;
use std::time::Instant;

use d3d12_xs::rdna2_hlsl::{decode_rdna2, detect_stage, lift_to_hlsl_from_ops, ShaderStage};
use flate2::{Decompress, FlushDecompress, Status};

fn rd32(b: &[u8], off: usize) -> u32 {
    b.get(off..off + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .unwrap_or(0)
}

fn skip_scarlett_prologue(code: &[u8]) -> &[u8] {
    if code.len() >= 0x28 && rd32(code, 0x18) == 0xD580_0000 && rd32(code, 0x1c) == 0x03FE_0100 {
        &code[0x24..]
    } else {
        code
    }
}

fn load_ps09_raw(path: &Path) -> Result<Vec<u8>, String> {
    let data = fs::read(path).map_err(|e| e.to_string())?;
    if data.starts_with(b"PS\x00\x09") {
        return Ok(data);
    }
    let mut raw = Vec::new();
    let mut pos = 0usize;
    let mut found = false;
    while pos + 2 < data.len() {
        if data[pos] == 0x78 && matches!(data[pos + 1], 0x9c | 0xda | 0x01) {
            let mut dec = Decompress::new(true);
            let mut out = vec![0u8; 65536];
            if let Ok(Status::StreamEnd | Status::Ok) =
                dec.decompress(&data[pos..], &mut out, FlushDecompress::Finish)
            {
                let produced = dec.total_out() as usize;
                if produced >= 64 {
                    if !found && out[..produced].starts_with(b"PS\x00\x09") {
                        found = true;
                    }
                    if found {
                        raw.extend_from_slice(&out[..produced]);
                        pos += (dec.total_in() as usize).max(2);
                        continue;
                    }
                }
            }
        }
        pos += 1;
    }
    if !found {
        return Err("No PS\\x00\\x09 stream found".into());
    }
    Ok(raw)
}

pub fn run_shader_benchmark(path: &Path, limit: usize, compile_dxil: bool) {
    let t0 = Instant::now();
    let raw = match load_ps09_raw(path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Shader load error: {e}");
            return;
        }
    };
    if raw.len() < 0x80 || &raw[0..4] != b"PS\x00\x09" {
        eprintln!("Invalid PS09 header");
        return;
    }
    let c0 = rd32(&raw, 0x50) as usize;
    let c1 = rd32(&raw, 0x54) as usize;
    let c2 = rd32(&raw, 0x58) as usize;
    let c3 = rd32(&raw, 0x5c) as usize;
    let meta_size = rd32(&raw, 0x60) as usize;
    let off_c3 = 0x80 + c0 * 0x70 + c1 * 0x18 + c2 * 8;
    let off_meta = off_c3 + c3 * 8;
    let off_code = off_meta + meta_size;

    let total = if limit == 0 { c3 } else { limit.min(c3) };
    let lifted_ok = std::sync::atomic::AtomicUsize::new(0);
    let dxil_ok = std::sync::atomic::AtomicUsize::new(0);
    let dxil_fail = std::sync::atomic::AtomicUsize::new(0);
    let done = std::sync::atomic::AtomicUsize::new(0);
    let vs = std::sync::atomic::AtomicUsize::new(0);
    let ps = std::sync::atomic::AtomicUsize::new(0);
    let cs = std::sync::atomic::AtomicUsize::new(0);
    let ms = std::sync::atomic::AtomicUsize::new(0);
    let rt = std::sync::atomic::AtomicUsize::new(0);
    let first_err: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
    let unknown_hist: std::sync::Mutex<std::collections::HashMap<String, usize>> =
        std::sync::Mutex::new(std::collections::HashMap::new());
    let next_idx = std::sync::atomic::AtomicUsize::new(0);
    let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(8).min(16);

    std::thread::scope(|s| {
        for wid in 0..workers {
            std::thread::Builder::new()
                .name(format!("dxc_{wid}"))
                .stack_size(16 * 1024 * 1024)
                .spawn_scoped(s, || {
                use std::sync::atomic::Ordering::Relaxed;
                loop {
                    let i = next_idx.fetch_add(1, Relaxed);
                    if i >= total {
                        break;
                    }
                    let off = rd32(&raw, off_c3 + i * 8) as usize;
                    let sz = rd32(&raw, off_c3 + i * 8 + 4) as usize;
                    let abs_off = off_code.saturating_add(off);
                    if abs_off + sz <= raw.len() && sz > 0 {
                        let code = skip_scarlett_prologue(&raw[abs_off..abs_off + sz]);
                        let ops = decode_rdna2(code);
                        let stage = detect_stage(&ops, ShaderStage::Compute);
                        let all_known = !ops.is_empty() && ops.iter().all(|o| o.is_known);
                        let (_stage, hlsl, insns) = lift_to_hlsl_from_ops(&ops, stage);
                        if all_known && insns > 0 {
                            lifted_ok.fetch_add(1, Relaxed);
                        } else if !ops.is_empty() {
                            let mut hist = unknown_hist.lock().unwrap();
                            for u in ops.iter().filter(|o| !o.is_known) {
                                let key = u.mnemonic.split_whitespace().next().unwrap_or(&u.mnemonic).to_string();
                                *hist.entry(key).or_insert(0) += 1;
                            }
                            drop(hist);
                            let mut guard = first_err.lock().unwrap();
                            if guard.is_none() {
                                if let Some(u) = ops.iter().find(|o| !o.is_known) {
                                    *guard = Some(format!(
                                        "shader#{i} ({}): unknown RDNA2 insn {}",
                                        stage.as_str(),
                                        u.mnemonic
                                    ));
                                }
                            }
                        }
                        match stage {
                            ShaderStage::Vertex => { vs.fetch_add(1, Relaxed); }
                            ShaderStage::Pixel => { ps.fetch_add(1, Relaxed); }
                            ShaderStage::Compute => { cs.fetch_add(1, Relaxed); }
                            ShaderStage::Mesh => { ms.fetch_add(1, Relaxed); }
                            ShaderStage::RayTracing => { rt.fetch_add(1, Relaxed); }
                            ShaderStage::Geometry => {}
                        }
                        if compile_dxil {
                            #[cfg(windows)]
                            if all_known && insns > 0 {
                                match d3d12_xs::compile_hlsl_to_dxil(&hlsl, stage) {
                                    Ok(_) => { dxil_ok.fetch_add(1, Relaxed); }
                                    Err(e) => {
                                        dxil_fail.fetch_add(1, Relaxed);
                                        let mut guard = first_err.lock().unwrap();
                                        if guard.is_none() {
                                            *guard = Some(format!("shader#{i} ({}): {e}", stage.as_str()));
                                        }
                                    }
                                }
                            } else {
                                dxil_fail.fetch_add(1, Relaxed);
                            }
                        }
                    }
                    let cur = done.fetch_add(1, Relaxed) + 1;
                    if cur % 500 == 0 || cur == total {
                        eprintln!(
                            "  [{cur}/{total}] lifted={} dxil_ok={} dxil_fail={}",
                            lifted_ok.load(Relaxed),
                            dxil_ok.load(Relaxed),
                            dxil_fail.load(Relaxed)
                        );
                    }
                }
            })
            .expect("spawn dxc worker");
        }
    });

    let lifted_ok = lifted_ok.into_inner();
    let dxil_ok = dxil_ok.into_inner();
    let dxil_fail = dxil_fail.into_inner();
    let vs = vs.into_inner();
    let ps = ps.into_inner();
    let cs = cs.into_inner();
    let ms = ms.into_inner();
    let rt = rt.into_inner();
    let first_err = first_err.into_inner().unwrap();
    let mut top_unk: Vec<(String, usize)> = unknown_hist.into_inner().unwrap().into_iter().collect();
    top_unk.sort_by(|a, b| b.1.cmp(&a.1));

    let elapsed = t0.elapsed().as_secs_f32();
    println!(
        "PS09 Shaders: total={c3}, benchmarked={total}, strict_lifted={lifted_ok}/{total} ({:.1}%) in {elapsed:.2}s",
        (lifted_ok as f64 / total.max(1) as f64) * 100.0
    );
    println!("Stages: VS={vs} PS={ps} CS={cs} Mesh={ms} RT={rt}");
    if !top_unk.is_empty() {
        let summary = top_unk
            .iter()
            .take(15)
            .map(|(k, v)| format!("{k}:{v}"))
            .collect::<Vec<_>>()
            .join(" ");
        println!("Top unknown opcodes: {summary}");
    }
    if compile_dxil {
        println!(
            "DXIL SM 6.6 (dxcompiler.dll): ok={dxil_ok}/{total} ({:.1}%), fail={dxil_fail}",
            (dxil_ok as f64 / total.max(1) as f64) * 100.0
        );
        if let Some(err) = first_err {
            println!("First DXC error sample: {err}");
        }
    }
}
