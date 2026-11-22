use std::fmt::Write as _;
use std::path::Path;

#[derive(Clone, Debug)]
pub struct SelfTestResult {
    pub passed: usize,
    pub total: usize,
    pub summary: String,
    pub markdown: String,
    pub issue_url: String,
}

pub fn url_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len() * 2);
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push_str("%20"),
            b'\n' => out.push_str("%0A"),
            _ => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

#[cfg(windows)]
fn check_dll(dir: &Path, name: &str) -> bool {
    use std::ffi::CString;
    extern "system" {
        fn LoadLibraryA(name: *const i8) -> usize;
    }
    let full = dir.join(name);
    if let Ok(cs) = CString::new(full.to_string_lossy().as_bytes()) {
        if unsafe { LoadLibraryA(cs.as_ptr()) } != 0 {
            return true;
        }
    }
    if let Ok(cs) = CString::new(name) {
        return unsafe { LoadLibraryA(cs.as_ptr()) } != 0;
    }
    false
}

#[cfg(not(windows))]
fn check_dll(_dir: &Path, _name: &str) -> bool {
    false
}

#[cfg(windows)]
fn check_pe_loader_minimal(shim_dir: &Path) -> Result<String, String> {
    let mut pe = vec![0u8; 0x2000];
    pe[0..2].copy_from_slice(b"MZ");
    pe[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    pe[0x80..0x84].copy_from_slice(b"PE\0\0");
    pe[0x84..0x86].copy_from_slice(&0x8664u16.to_le_bytes());
    pe[0x86..0x88].copy_from_slice(&1u16.to_le_bytes());
    pe[0x94..0x96].copy_from_slice(&0xF0u16.to_le_bytes());
    pe[0x98..0x9A].copy_from_slice(&0x020Bu16.to_le_bytes());
    pe[0xA8..0xAC].copy_from_slice(&0x1000u32.to_le_bytes());
    pe[0xB0..0xB8].copy_from_slice(&0x1_4000_0000u64.to_le_bytes());
    pe[0xD0..0xD4].copy_from_slice(&0x2000u32.to_le_bytes());
    pe[0xD4..0xD8].copy_from_slice(&0x200u32.to_le_bytes());
    pe[0x104..0x108].copy_from_slice(&16u32.to_le_bytes());
    pe[0x120..0x124].copy_from_slice(&0x1020u32.to_le_bytes());
    pe[0x124..0x128].copy_from_slice(&12u32.to_le_bytes());
    pe[0x188..0x18D].copy_from_slice(b".text");
    pe[0x190..0x194].copy_from_slice(&0x1000u32.to_le_bytes());
    pe[0x194..0x198].copy_from_slice(&0x1000u32.to_le_bytes());
    pe[0x198..0x19C].copy_from_slice(&0x1000u32.to_le_bytes());
    pe[0x19C..0x1A0].copy_from_slice(&0x1000u32.to_le_bytes());
    pe[0x1AC..0x1B0].copy_from_slice(&0x6000_0020u32.to_le_bytes());
    pe[0x1000..0x1006].copy_from_slice(&[0xB8, 42, 0, 0, 0, 0xC3]);
    pe[0x1020..0x1024].copy_from_slice(&0x1000u32.to_le_bytes());
    pe[0x1024..0x1028].copy_from_slice(&0x1006u32.to_le_bytes());
    pe[0x1028..0x102C].copy_from_slice(&0x1030u32.to_le_bytes());
    pe[0x1030] = 0x01;
    let (st, rc) = crate::pe_loader::load_and_run_pe64(&pe, shim_dir, true)?;
    if rc == 42 {
        Ok(format!("rc=42, pdata={}, base=0x{:x}", st.pdata_entries, st.mapped_base))
    } else {
        Err(format!("unexpected rc={rc}"))
    }
}

#[cfg(not(windows))]
fn check_pe_loader_minimal(_shim_dir: &Path) -> Result<String, String> {
    Err("Windows only".into())
}

pub fn run_self_test(shim_dir: &Path) -> SelfTestResult {
    let ver = env!("CARGO_PKG_VERSION");
    let arch = std::env::consts::ARCH;
    let os = std::env::consts::OS;
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0);
    let cpu = std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_else(|_| "unknown".into());

    let mut passed = 0usize;
    let mut total = 0usize;
    let mut lines = Vec::new();

    // 1. 9 Shim DLLs
    total += 1;
    let shims = [
        "xmem.dll",
        "kernelx.dll",
        "d3d12_xs.dll",
        "xg_xs.dll",
        "xgameruntime.dll",
        "dstorage.dll",
        "GameInput.dll",
        "winrt_xs.dll",
        "xaudio2_9.dll",
    ];
    let mut ok_shims = 0usize;
    let mut missing_shims = Vec::new();
    for s in shims {
        if check_dll(shim_dir, s) {
            ok_shims += 1;
        } else {
            missing_shims.push(s);
        }
    }
    if ok_shims == shims.len() {
        passed += 1;
        lines.push(format!("- [x] **Shim DLLs:** {ok_shims}/{} loaded", shims.len()));
    } else {
        lines.push(format!(
            "- [ ] **Shim DLLs:** {ok_shims}/{} loaded (missing: {})",
            shims.len(),
            missing_shims.join(", ")
        ));
    }

    // 2. XMem Physical Pool
    total += 1;
    let mut pool = crate::xmem::PhysicalPool::default();
    match pool.alloc(16 * 1024 * 1024) {
        Ok(addr) => {
            pool.free(addr);
            passed += 1;
            lines.push(format!("- [x] **XMem 16 MiB Pool:** OK (`0x{addr:x}`)"));
        }
        Err(e) => lines.push(format!("- [ ] **XMem 16 MiB Pool:** FAIL (`{e}`)")),
    }

    // 3. In-process PE64 loader
    total += 1;
    match check_pe_loader_minimal(shim_dir) {
        Ok(msg) => {
            passed += 1;
            lines.push(format!("- [x] **PE32+ Loader:** OK ({msg})"));
        }
        Err(e) => lines.push(format!("- [ ] **PE32+ Loader:** FAIL (`{e}`)")),
    }

    // 4. RDNA2 -> HLSL -> DXBC (d3dcompiler_47.dll)
    total += 1;
    let mut prog = Vec::new();
    prog.extend_from_slice(&((0x3Fu32 << 25) | (0u32 << 17) | (0x01u32 << 9) | 242u32).to_le_bytes());
    prog.extend_from_slice(&((0x3Eu32 << 26) | (12u32 << 4) | 0xFu32).to_le_bytes());
    prog.extend_from_slice(&0u32.to_le_bytes());
    prog.extend_from_slice(&0xBF81_0000u32.to_le_bytes());
    let (stage, hlsl) = d3d12_xs::translate_and_cache_rdna2(&prog, d3d12_xs::rdna2_hlsl::ShaderStage::Vertex);
    #[cfg(windows)]
    match d3d12_xs::compile_hlsl_to_dxbc(&hlsl, stage) {
        Ok(dxbc) => {
            passed += 1;
            lines.push(format!("- [x] **RDNA2 -> DXBC (`d3dcompiler_47.dll`):** OK ({} bytes)", dxbc.len()));
        }
        Err(e) => lines.push(format!("- [ ] **RDNA2 -> DXBC:** FAIL (`{e}`)")),
    }
    #[cfg(not(windows))]
    lines.push("- [ ] **RDNA2 -> DXBC:** skipped (non-Windows)".into());

    // 5. RDNA2 -> SM 6.6 DXIL (dxcompiler.dll)
    total += 1;
    #[cfg(windows)]
    match d3d12_xs::compile_hlsl_to_dxil(&hlsl, stage) {
        Ok(dxil) => {
            passed += 1;
            lines.push(format!("- [x] **RDNA2 -> SM 6.6 DXIL (`dxcompiler.dll`):** OK ({} bytes)", dxil.len()));
        }
        Err(e) => lines.push(format!("- [ ] **RDNA2 -> SM 6.6 DXIL (`dxcompiler.dll`):** FAIL (`{e}`)")),
    }
    #[cfg(not(windows))]
    lines.push("- [ ] **RDNA2 -> SM 6.6 DXIL:** skipped (non-Windows)".into());

    let status_word = if passed == total { "PASS" } else { "PARTIAL/FAIL" };
    let summary = format!("PercyScarlett v{ver} Self-Test: {passed}/{total} {status_word}");
    let markdown = format!(
        "### Environment\n- **Version:** `v{ver}`\n- **OS / Arch:** `{os}` / `{arch}` ({cores} threads)\n- **CPU:** `{cpu}`\n\n### Self-Test Results ({passed}/{total})\n{}\n\n### Notes / What happened\n_Add any extra details or screenshots below:_\n",
        lines.join("\n")
    );
    let title = format!("[Tester Report] v{ver} ({passed}/{total} {status_word}) on {os}/{arch}");
    let issue_url = format!(
        "https://github.com/rpatty-dev/PercyScarlett/issues/new?title={}&body={}",
        url_encode(&title),
        url_encode(&markdown)
    );

    SelfTestResult {
        passed,
        total,
        summary,
        markdown,
        issue_url,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_prefilled_github_issue_url() {
        let res = run_self_test(Path::new("."));
        assert_eq!(res.total, 5);
        assert!(res.passed >= 2);
        assert!(res.issue_url.starts_with("https://github.com/rpatty-dev/PercyScarlett/issues/new?title="));
        assert!(res.issue_url.contains("&body="));
        assert!(!res.issue_url.contains(' '));
    }
}
