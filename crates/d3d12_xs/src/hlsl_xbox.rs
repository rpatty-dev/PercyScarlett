//! Clean-room Xbox Series (GFX10.3) -> SM 6.6 HLSL translator with real root-signature,
//! descriptor-heap (`ResourceDescriptorHeap` / `SamplerDescriptorHeap`), vertex-input,
//! pixel-interpolation, and MRT-export wiring.
#![allow(dead_code)]

use std::fmt::Write as _;

use super::dxbc_xbox::{HwStage, ResKind, RootParamKind, SigParam, XboxProgram};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UdSource {
    Zero,
    Constant { root_slot: u8, dword_off: u32 },
    Table { root_slot: u8, is_sampler: bool },
    HeapBase,
}

#[derive(Debug, Clone)]
pub struct HostShaderMeta {
    pub stage: HwStage,
    pub hlsl: String,
    pub ud_dwords: u32,
    pub ud_sources: Vec<UdSource>,
    pub root_descs: Vec<u8>,
    pub static_samplers: Vec<[u32; 4]>,
    pub unknown_ops: usize,
}

#[derive(Clone, Debug)]
enum SgprProv {
    Unknown,
    Const(u32),
    RootBuf(u8),
    HeapPtr { is_sampler: bool, root_slot: u8, expr: String, byte_off: u32 },
    HeapDesc { is_sampler: bool, kind: ResKind, idx_expr: String },
}

fn rd32(b: &[u8], off: usize) -> Option<u32> {
    b.get(off..off + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn fmt_src(src: u32, lit: Option<u32>, as_float: bool) -> String {
    match src {
        0..=105 => if as_float { format!("asfloat(s[{src}])") } else { format!("s[{src}]") },
        106 => if as_float { "asfloat(vcc)".into() } else { "vcc".into() },
        124 => "0u".into(),
        126 => "exec_lo".into(),
        127 => "exec_hi".into(),
        128..=192 => {
            let v = (src - 128) as i32;
            if as_float { format!("{v}.0") } else { format!("{v}u") }
        }
        193..=208 => {
            let v = -(src as i32 - 192);
            if as_float { format!("{v}.0") } else { format!("asuint({v})") }
        }
        240 => if as_float { "0.5".into() } else { "0x3F000000u".into() },
        241 => if as_float { "-0.5".into() } else { "0xBF000000u".into() },
        242 => if as_float { "1.0".into() } else { "0x3F800000u".into() },
        243 => if as_float { "-1.0".into() } else { "0xBF800000u".into() },
        244 => if as_float { "2.0".into() } else { "0x40000000u".into() },
        245 => if as_float { "-2.0".into() } else { "0xC0000000u".into() },
        246 => if as_float { "4.0".into() } else { "0x40800000u".into() },
        247 => if as_float { "-4.0".into() } else { "0xC0800000u".into() },
        248 => if as_float { "0.15915494".into() } else { "0x3E22F983u".into() },
        251 => if as_float { "asfloat(vcc)".into() } else { "vcc".into() },
        255 => {
            let imm = lit.unwrap_or(0);
            if as_float { format!("asfloat(0x{imm:08x}u)") } else { format!("0x{imm:08x}u") }
        }
        256..=511 => {
            let v = src - 256;
            if as_float { format!("asfloat(v[{v}])") } else { format!("v[{v}]") }
        }
        _ => "0u".into(),
    }
}

fn fmt_sdst(sdst: u32) -> String {
    match sdst {
        0..=105 => format!("s[{sdst}]"),
        106 | 107 => "vcc".into(),
        126 => "exec_lo".into(),
        127 => "exec_hi".into(),
        _ => "_s_sink".into(),
    }
}

fn eval_const_src(src: u32, lit: Option<u32>, prov: &[SgprProv; 106]) -> Option<u32> {
    match src {
        0..=105 => match prov[src as usize] { SgprProv::Const(c) => Some(c), _ => None },
        124 => Some(0),
        128..=192 => Some(src - 128),
        193..=208 => Some((-(src as i32 - 192)) as u32),
        240 => Some(0x3F00_0000),
        241 => Some(0xBF00_0000),
        242 => Some(0x3F80_0000),
        243 => Some(0xBF80_0000),
        244 => Some(0x4000_0000),
        245 => Some(0xC000_0000),
        246 => Some(0x4080_0000),
        247 => Some(0xC080_0000),
        248 => Some(0x3E22_F983),
        255 => lit,
        _ => None,
    }
}

fn sig_vec_type(comp_type: u32) -> &'static str {
    match comp_type {
        1 => "uint4",
        2 => "int4",
        _ => "float4",
    }
}

fn find_sig(sigs: &[SigParam], row: u8) -> Option<&SigParam> {
    sigs.iter().find(|s| s.register == row as u32)
}

fn lookup_res_kind(prog: &XboxProgram, root_slot: u8, byte_off: u32, default_sampler: bool) -> ResKind {
    let dw = byte_off / 4;
    if let Some(u) = prog.used_res.iter().find(|u| {
        u.root_slot == root_slot && u.root_kind == RootParamKind::Table && u.table_offset_dw == dw
    }) {
        return u.res_kind;
    }
    if let Some(u) = prog.used_res.iter().find(|u| {
        u.root_slot == root_slot && u.root_kind == RootParamKind::Table && u.table_offset_dw <= dw
    }) {
        return u.res_kind;
    }
    if default_sampler { ResKind::Sampler } else { ResKind::Srv }
}

fn is_sampler_table(prog: &XboxProgram, root_slot: u8) -> bool {
    let any_samp = prog.used_res.iter().any(|u| {
        u.root_slot == root_slot && u.root_kind == RootParamKind::Table && u.res_kind == ResKind::Sampler
    });
    let any_res = prog.used_res.iter().any(|u| {
        u.root_slot == root_slot && u.root_kind == RootParamKind::Table && u.res_kind != ResKind::Sampler
    });
    any_samp && !any_res
}

struct Insn {
    off: usize,
    stmt: String,
    is_endpgm: bool,
    branch_target: Option<usize>,
    branch_cond: Option<&'static str>,
}

/// Translates a parsed Xbox Series native VS or PS program into compilable SM 6.6 HLSL.
pub fn translate(prog: &XboxProgram) -> Result<HostShaderMeta, String> {
    if !matches!(prog.stage, HwStage::Vs | HwStage::Ps) {
        return Err(format!("hlsl_xbox supports VS/PS only, got {:?}", prog.stage));
    }

    // 1. Map user-data SGPRs & root descriptors from FSSM root_layout.
    let mut ud_sources: Vec<UdSource> = Vec::new();
    let mut root_descs: Vec<u8> = Vec::new();
    let mut prov: [SgprProv; 106] = std::array::from_fn(|_| SgprProv::Unknown);
    let mut init_stmts = String::new();

    for rl in &prog.root_layout {
        let ud = rl.first_user_data as usize;
        match rl.kind {
            RootParamKind::Constants => {
                let n = rl.user_sgprs.max(1) as usize;
                if ud + n > ud_sources.len() {
                    ud_sources.resize(ud + n, UdSource::Zero);
                }
                for k in 0..n {
                    let idx = ud + k;
                    if idx < 106 {
                        ud_sources[idx] = UdSource::Constant {
                            root_slot: rl.root_slot,
                            dword_off: rl.table_offset_dw + k as u32,
                        };
                        let _ = writeln!(init_stmts, "    s[{idx}] = g_ud_{idx};");
                    }
                }
            }
            RootParamKind::Table => {
                if ud + 1 > ud_sources.len() {
                    ud_sources.resize(ud + 1, UdSource::Zero);
                }
                let samp = is_sampler_table(prog, rl.root_slot);
                ud_sources[ud] = UdSource::Table { root_slot: rl.root_slot, is_sampler: samp };
                if ud < 106 {
                    let _ = writeln!(init_stmts, "    s[{ud}] = g_ud_{ud};");
                    prov[ud] = SgprProv::HeapPtr {
                        is_sampler: samp,
                        root_slot: rl.root_slot,
                        expr: format!("g_ud_{ud}"),
                        byte_off: 0,
                    };
                }
            }
            RootParamKind::ResourceHeap | RootParamKind::SamplerHeap => {
                let samp = rl.kind == RootParamKind::SamplerHeap;
                if ud + 1 > ud_sources.len() {
                    ud_sources.resize(ud + 1, UdSource::Zero);
                }
                ud_sources[ud] = UdSource::HeapBase;
                if ud < 106 {
                    let _ = writeln!(init_stmts, "    s[{ud}] = 0u;");
                    prov[ud] = SgprProv::HeapPtr {
                        is_sampler: samp,
                        root_slot: 0xFF,
                        expr: "0u".into(),
                        byte_off: 0,
                    };
                }
            }
            RootParamKind::Descriptor => {
                if !root_descs.contains(&rl.root_slot) {
                    root_descs.push(rl.root_slot);
                }
                if ud < 106 {
                    prov[ud] = SgprProv::RootBuf(rl.root_slot);
                }
            }
            RootParamKind::ImmediateSampler | RootParamKind::Other(_) => {}
        }
    }

    // 2. Decode & lower instructions with live SGPR provenance tracking.
    let bytes = &prog.code;
    let mut insns: Vec<Insn> = Vec::with_capacity(bytes.len() / 4);
    let mut static_samplers: Vec<[u32; 4]> = Vec::new();
    let mut unknown_ops = 0usize;
    let mut uses_cb = false;
    let mut uses_exec_kill = false;
    let mut off = 0usize;
    let mut furthest_branch = 0usize;

    let mut intern_sampler = |w: [u32; 4]| -> usize {
        if let Some(i) = static_samplers.iter().position(|&x| x == w) {
            i
        } else {
            static_samplers.push(w);
            static_samplers.len() - 1
        }
    };

    while off + 4 <= bytes.len() {
        let w0 = rd32(bytes, off).unwrap();
        if w0 == 0xEFCD_AB89 || w0 == 0x89AB_CDEF {
            break;
        }
        let w1 = rd32(bytes, off + 4).unwrap_or(0);
        let w2 = rd32(bytes, off + 8).unwrap_or(0);
        let mut len_words = 1usize;
        let mut is_endpgm = false;
        let mut branch_target: Option<usize> = None;
        let mut branch_cond: Option<&'static str> = None;
        let mut stmt = String::new();

        if (w0 & 0xFFFF_0000) == 0xBF81_0000 || (w0 & 0xFFFF_0000) == 0xBF9F_0000 {
            is_endpgm = true;
            stmt = "// s_endpgm".into();
        } else if (w0 & 0xFF80_0000) == 0xBF80_0000 {
            // SOPP
            let op = (w0 >> 16) & 0x7F;
            let imm = (w0 & 0xFFFF) as i16;
            match op {
                0x00 | 0x0C..=0x11 | 0x13..=0x15 | 0x20 | 0x21 | 0x23 | 0x24 | 0x26 | 0x27 => {}
                0x02..=0x09 => {
                    let target = (off as isize + 4 + (imm as isize) * 4).max(0) as usize;
                    branch_target = Some(target);
                    branch_cond = match op {
                        0x02 => None,
                        0x04 => Some("(scc == 0u)"),
                        0x05 => Some("(scc != 0u)"),
                        0x06 => Some("(vcc == 0u)"),
                        0x07 => Some("(vcc != 0u)"),
                        0x08 => Some("((exec_lo | exec_hi) == 0u)"),
                        0x09 => Some("((exec_lo | exec_hi) != 0u)"),
                        _ => None,
                    };
                }
                _ => { unknown_ops += 1; }
            }
        } else if (w0 >> 23) == 0x17D {
            // SOP1
            let op = (w0 >> 8) & 0xFF;
            let ssrc0 = w0 & 0xFF;
            let sdst = (w0 >> 16) & 0x7F;
            let lit = if ssrc0 == 255 { len_words = 2; Some(w1) } else { None };
            let a = fmt_src(ssrc0, lit, false);
            let dst = fmt_sdst(sdst);
            match op {
                0x03 => {
                    // s_mov_b32
                    if (sdst as usize) < 106 {
                        if let Some(c) = eval_const_src(ssrc0, lit, &prov) {
                            prov[sdst as usize] = SgprProv::Const(c);
                        } else if (ssrc0 as usize) < 106 {
                            prov[sdst as usize] = prov[ssrc0 as usize].clone();
                        } else {
                            prov[sdst as usize] = SgprProv::Unknown;
                        }
                    }
                    stmt = format!("{dst} = {a};");
                }
                0x04 => {
                    // s_mov_b64
                    if (sdst as usize) < 106 { prov[sdst as usize] = SgprProv::Unknown; }
                    if (sdst as usize) + 1 < 106 { prov[sdst as usize + 1] = SgprProv::Unknown; }
                    let dst_hi = fmt_sdst(sdst + 1);
                    stmt = format!("{dst} = {a}; {dst_hi} = 0u;");
                }
                0x05 => stmt = format!("if (scc != 0u) {{ {dst} = {a}; }}"),
                0x06 | 0x07 => stmt = format!("{dst} = ~{a}; scc = ({dst} != 0u) ? 1u : 0u;"),
                0x08 => stmt = format!("{dst} = ~{a};"),
                0x0A => { /* s_wqm_b64: per-pixel HLSL keeps active lane */ }
                0x09 => stmt = format!("{dst} = countbits({a}); scc = ({dst} != 0u) ? 1u : 0u;"),
                0x0B | 0x10 => stmt = format!("{dst} = countbits(exec_lo) + countbits(exec_hi); scc = ({dst} != 0u) ? 1u : 0u;"),
                0x0F | 0x13 | 0x14 => stmt = format!("{dst} = firstbitlow({a});"),
                0x17 | 0x18 | 0x1B => stmt = format!("{dst} = firstbithigh({a});"),
                0x1C => stmt = format!("{dst} = {dst} & ~(1u << ({a} & 31u));"),
                0x1D => stmt = format!("{dst} = {dst} | (1u << ({a} & 31u));"),
                0x1F => stmt = format!("{dst} = {}u; {} = 0u;", off + 4, fmt_sdst(sdst + 1)),
                0x21 => { /* s_swappc_b64: VS fetch shader call (inputs already in VGPRs) */ }
                0x24 | 0x25 => {
                    uses_exec_kill = true;
                    stmt = format!("{dst} = exec_lo; exec_lo &= {a}; scc = (exec_lo != 0u) ? 1u : 0u;");
                }
                0x26 | 0x27 => stmt = format!("{dst} = exec_lo; exec_lo |= {a}; scc = (exec_lo != 0u) ? 1u : 0u;"),
                0x28 | 0x29 => stmt = format!("{dst} = exec_lo; exec_lo ^= {a}; scc = (exec_lo != 0u) ? 1u : 0u;"),
                0x2A | 0x2B => {
                    uses_exec_kill = true;
                    stmt = format!("{dst} = exec_lo; exec_lo &= ~{a}; scc = (exec_lo != 0u) ? 1u : 0u;");
                }
                0x3A | 0x3B => {
                    uses_exec_kill = true;
                    stmt = format!("{dst} = exec_lo; exec_lo = exec_lo & ~{a}; scc = (exec_lo != 0u) ? 1u : 0u;");
                }
                0x3C | 0x3D => {
                    uses_exec_kill = true;
                    stmt = format!("{dst} = exec_lo; exec_lo = ~exec_lo & {a}; scc = (exec_lo != 0u) ? 1u : 0u;");
                }
                0x47 => stmt = format!("{dst} = asuint(abs(asint({a})));"),
                _ => { unknown_ops += 1; }
            }
        } else if (w0 >> 23) == 0x17E {
            // SOPC
            let op = (w0 >> 16) & 0x7F;
            let ssrc0 = w0 & 0xFF;
            let ssrc1 = (w0 >> 8) & 0xFF;
            let lit = if ssrc0 == 255 || ssrc1 == 255 { len_words = 2; Some(w1) } else { None };
            let a = fmt_src(ssrc0, lit, false);
            let b = fmt_src(ssrc1, lit, false);
            stmt = match op {
                0x00 => format!("scc = (asint({a}) == asint({b})) ? 1u : 0u;"),
                0x01 => format!("scc = (asint({a}) != asint({b})) ? 1u : 0u;"),
                0x02 => format!("scc = (asint({a}) > asint({b})) ? 1u : 0u;"),
                0x03 => format!("scc = (asint({a}) >= asint({b})) ? 1u : 0u;"),
                0x04 => format!("scc = (asint({a}) < asint({b})) ? 1u : 0u;"),
                0x05 => format!("scc = (asint({a}) <= asint({b})) ? 1u : 0u;"),
                0x06 => format!("scc = ({a} == {b}) ? 1u : 0u;"),
                0x07 => format!("scc = ({a} != {b}) ? 1u : 0u;"),
                0x08 => format!("scc = ({a} > {b}) ? 1u : 0u;"),
                0x09 => format!("scc = ({a} >= {b}) ? 1u : 0u;"),
                0x0A => format!("scc = ({a} < {b}) ? 1u : 0u;"),
                0x0B => format!("scc = ({a} <= {b}) ? 1u : 0u;"),
                0x0C => format!("scc = (({a} & (1u << ({b} & 31u))) == 0u) ? 1u : 0u;"),
                0x0D => format!("scc = (({a} & (1u << ({b} & 31u))) != 0u) ? 1u : 0u;"),
                0x0E..=0x13 => format!("scc = ({a} == {b}) ? 1u : 0u;"),
                _ => { unknown_ops += 1; format!("/* UNK op={op:#x} w0={w0:#010x} w1={w1:#010x} */") }
            };
        } else if (w0 >> 28) == 0xB {
            // SOPK
            let op = (w0 >> 23) & 0x1F;
            let sdst = (w0 >> 16) & 0x7F;
            let simm = (w0 & 0xFFFF) as i16 as i32;
            let uimm = w0 & 0xFFFF;
            if op == 0x14 { len_words = 2; }
            let dst = fmt_sdst(sdst);
            if op == 0x00 && (sdst as usize) < 106 {
                prov[sdst as usize] = SgprProv::Const(simm as u32);
            }
            stmt = match op {
                0x00 => format!("{dst} = asuint({simm});"),
                0x01 | 0x11 | 0x12 | 0x14 | 0x18..=0x1C => String::new(),
                0x02 => format!("if (scc != 0u) {{ {dst} = asuint({simm}); }}"),
                0x03 => format!("scc = (asint({dst}) == {simm}) ? 1u : 0u;"),
                0x04 => format!("scc = (asint({dst}) != {simm}) ? 1u : 0u;"),
                0x05 => format!("scc = (asint({dst}) > {simm}) ? 1u : 0u;"),
                0x06 => format!("scc = (asint({dst}) >= {simm}) ? 1u : 0u;"),
                0x07 => format!("scc = (asint({dst}) < {simm}) ? 1u : 0u;"),
                0x08 => format!("scc = (asint({dst}) <= {simm}) ? 1u : 0u;"),
                0x09 => format!("scc = ({dst} == {uimm}u) ? 1u : 0u;"),
                0x0A => format!("scc = ({dst} != {uimm}u) ? 1u : 0u;"),
                0x0B => format!("scc = ({dst} > {uimm}u) ? 1u : 0u;"),
                0x0C => format!("scc = ({dst} >= {uimm}u) ? 1u : 0u;"),
                0x0D => format!("scc = ({dst} < {uimm}u) ? 1u : 0u;"),
                0x0E => format!("scc = ({dst} <= {uimm}u) ? 1u : 0u;"),
                0x0F => format!("{dst} = asuint(asint({dst}) + {simm});"),
                0x10 => format!("{dst} = asuint(asint({dst}) * {simm});"),
                _ => { unknown_ops += 1; format!("/* UNK op={op:#x} w0={w0:#010x} w1={w1:#010x} */") }
            };
        } else if (w0 >> 26) == 0x3E {
            // EXP
            len_words = 2;
            let target = (w0 >> 4) & 0x3F;
            let compr = ((w0 >> 10) & 1) != 0;
            let done = ((w0 >> 11) & 1) != 0;
            let vs = [w1 & 0xFF, (w1 >> 8) & 0xFF, (w1 >> 16) & 0xFF, (w1 >> 24) & 0xFF];
            let val = if compr {
                format!(
                    "float4(f16tof32(v[{}] & 0xFFFFu), f16tof32(v[{}] >> 16), f16tof32(v[{}] & 0xFFFFu), f16tof32(v[{}] >> 16))",
                    vs[0], vs[0], vs[1], vs[1]
                )
            } else {
                format!("float4(asfloat(v[{}]), asfloat(v[{}]), asfloat(v[{}]), asfloat(v[{}]))", vs[0], vs[1], vs[2], vs[3])
            };
            match target {
                12..=15 => stmt = format!("exp_pos0 = {val};"),
                0..=7 => {
                    stmt = format!("exp_mrt[{target}] = {val};");
                    if done && uses_exec_kill && prog.stage == HwStage::Ps {
                        stmt.push_str(" if ((exec_lo & 1u) == 0u) discard;");
                    }
                }
                9 => {
                    // exp null (done=1 when pixel discarded)
                    if done && prog.stage == HwStage::Ps {
                        stmt = "discard;".into();
                    }
                }
                32..=63 => stmt = format!("exp_param[{}] = {val};", target - 32),
                _ => {}
            }
        } else if (w0 >> 26) == 0x3D {
            // SMEM (s_load_dword* / s_buffer_load_dword*)
            len_words = 2;
            let op = (w0 >> 18) & 0xFF;
            let sbase = ((w0 & 0x3F) * 2) as usize;
            let sdata = ((w0 >> 6) & 0x7F) as usize;
            let raw_off = w1 & 0x001F_FFFF;
            let imm_off = raw_off; // byte offset
            let soff = (w1 >> 25) & 0x7F;
            let soff_expr = if soff != 124 && soff != 125 && soff < 106 {
                format!("({imm_off}u + s[{soff}])")
            } else {
                format!("{imm_off}u")
            };
            let count = match op {
                0x00 | 0x08 => 1,
                0x01 | 0x09 => 2,
                0x02 | 0x0A => 4,
                0x03 | 0x0B => 8,
                0x04 | 0x0C => 16,
                0x20..=0x26 => 0,
                _ => { unknown_ops += 1; 0 }
            };
            if count > 0 && sbase < 106 {
                match prov[sbase].clone() {
                    SgprProv::HeapPtr { is_sampler, root_slot, expr, byte_off } => {
                        // Loading a 4-dword or 8-dword descriptor from the descriptor heap!
                        let total_off = byte_off.wrapping_add(imm_off);
                        let shift = if is_sampler { 4 } else { 5 };
                        let idx_expr = if soff != 124 && soff != 125 && soff < 106 {
                            format!("(({expr} + {total_off}u + s[{soff}]) >> {shift})")
                        } else if total_off == 0 {
                            format!("({expr} >> {shift})")
                        } else {
                            format!("(({expr} + {total_off}u) >> {shift})")
                        };
                        let kind = lookup_res_kind(prog, root_slot, total_off, is_sampler);
                        if sdata < 106 {
                            prov[sdata] = SgprProv::HeapDesc { is_sampler, kind, idx_expr };
                        }
                        stmt = format!("/* heap desc load s[{sdata}..{}] */", sdata + count - 1);
                    }
                    SgprProv::RootBuf(slot) => {
                        for k in 0..count {
                            let dst = fmt_sdst((sdata + k) as u32);
                            let _ = writeln!(stmt, "{dst} = g_rdesc_{slot}.Load({soff_expr} + {}u);", k * 4);
                            if sdata + k < 106 { prov[sdata + k] = SgprProv::Unknown; }
                        }
                    }
                    SgprProv::HeapDesc { is_sampler: false, kind: ResKind::Cbv, idx_expr } => {
                        uses_cb = true;
                        let _ = writeln!(stmt, "{{ ConstantBuffer<RawCB> _cb = ResourceDescriptorHeap[{idx_expr}];");
                        for k in 0..count {
                            let dst = fmt_sdst((sdata + k) as u32);
                            let o = format!("({soff_expr} + {}u)", k * 4);
                            let _ = writeln!(stmt, "  {dst} = _cb.d[{o} >> 4][({o} >> 2) & 3u];");
                            if sdata + k < 106 { prov[sdata + k] = SgprProv::Unknown; }
                        }
                        stmt.push_str("}");
                    }
                    SgprProv::HeapDesc { is_sampler: false, kind: _, idx_expr } => {
                        let _ = writeln!(stmt, "{{ ByteAddressBuffer _sb = ResourceDescriptorHeap[{idx_expr}];");
                        for k in 0..count {
                            let dst = fmt_sdst((sdata + k) as u32);
                            let _ = writeln!(stmt, "  {dst} = _sb.Load({soff_expr} + {}u);", k * 4);
                            if sdata + k < 106 { prov[sdata + k] = SgprProv::Unknown; }
                        }
                        stmt.push_str("}");
                    }
                    _ => {
                        // Untracked scalar load -> zero (safe fallback)
                        for k in 0..count {
                            let dst = fmt_sdst((sdata + k) as u32);
                            let _ = writeln!(stmt, "{dst} = 0u; /* untracked smem s[{sbase}] */");
                            if sdata + k < 106 { prov[sdata + k] = SgprProv::Unknown; }
                        }
                    }
                }
            }
        } else if (w0 >> 26) == 0x3C {
            // MIMG (image_sample* / image_load*)
            let nsize = ((w0 >> 1) & 3) as usize;
            len_words = 2 + nsize;
            let op = (w0 >> 18) & 0xFF;
            let dmask = (w0 >> 8) & 0xF;
            let dim = (w0 >> 3) & 0x7;
            let vaddr = w1 & 0xFF;
            let vdata = (w1 >> 8) & 0xFF;
            let srsrc = (((w1 >> 16) & 0x1F) * 4) as usize;
            let ssamp = (((w1 >> 21) & 0x1F) * 4) as usize;
            let addrs: [u32; 5] = if nsize > 0 {
                [vaddr, w2 & 0xFF, (w2 >> 8) & 0xFF, (w2 >> 16) & 0xFF, (w2 >> 24) & 0xFF]
            } else {
                [vaddr, (vaddr + 1) & 0xFF, (vaddr + 2) & 0xFF, (vaddr + 3) & 0xFF, (vaddr + 4) & 0xFF]
            };

            let tex_expr = match prov.get(srsrc) {
                Some(SgprProv::HeapDesc { idx_expr, .. }) => Some(idx_expr.clone()),
                _ => None,
            };

            let is_load = matches!(op, 0x00 | 0x01);
            let (smp_decl, smp_expr) = if is_load {
                (String::new(), String::new())
            } else if ssamp + 3 < 106 {
                if let (SgprProv::Const(c0), SgprProv::Const(c1), SgprProv::Const(c2), SgprProv::Const(c3)) =
                    (&prov[ssamp], &prov[ssamp + 1], &prov[ssamp + 2], &prov[ssamp + 3])
                {
                    let idx = intern_sampler([*c0, *c1, *c2, *c3]);
                    (String::new(), format!("g_ss_{idx}"))
                } else if let SgprProv::HeapDesc { idx_expr, .. } = &prov[ssamp] {
                    (format!("  SamplerState _s = SamplerDescriptorHeap[{idx_expr}];\n"), "_s".to_string())
                } else {
                    let idx = intern_sampler([0x0400, 0x00C0_0000, 0x18F0_0000, 0]);
                    (String::new(), format!("g_ss_{idx}"))
                }
            } else {
                let idx = intern_sampler([0x0400, 0x00C0_0000, 0x18F0_0000, 0]);
                (String::new(), format!("g_ss_{idx}"))
            };

            let (tex_ty, coord_f, coord_i) = match dim {
                0 => ("Texture1D<float4>", format!("asfloat(v[{}])", addrs[0]), format!("int2(v[{}], 0)", addrs[0])),
                2 => ("Texture3D<float4>", format!("float3(asfloat(v[{}]), asfloat(v[{}]), asfloat(v[{}]))", addrs[0], addrs[1], addrs[2]), format!("int4(v[{}], v[{}], v[{}], 0)", addrs[0], addrs[1], addrs[2])),
                3 => ("TextureCube<float4>", format!("float3(asfloat(v[{}]), asfloat(v[{}]), asfloat(v[{}]))", addrs[0], addrs[1], addrs[2]), format!("int3(0,0,0)")),
                5 => ("Texture2DArray<float4>", format!("float3(asfloat(v[{}]), asfloat(v[{}]), asfloat(v[{}]))", addrs[0], addrs[1], addrs[2]), format!("int4(v[{}], v[{}], v[{}], 0)", addrs[0], addrs[1], addrs[2])),
                _ => ("Texture2D<float4>", format!("float2(asfloat(v[{}]), asfloat(v[{}]))", addrs[0], addrs[1]), format!("int3(v[{}], v[{}], 0)", addrs[0], addrs[1])),
            };

            let lod_idx = match dim { 0 => addrs[1], 2 | 3 | 5 => addrs[3], _ => addrs[2] };

            let mut body = String::new();
            if let Some(ti) = tex_expr {
                let _ = writeln!(body, "{{ {tex_ty} _t = ResourceDescriptorHeap[{ti}];");
                if is_load {
                    let _ = writeln!(body, "  float4 _ts = _t.Load({coord_i});");
                } else {
                    if !smp_decl.is_empty() {
                        body.push_str(&smp_decl);
                    }
                    let call = match op {
                        0x20 => format!("_t.Sample({smp_expr}, {coord_f})"),
                        0x24 => format!("_t.SampleBias({smp_expr}, {coord_f}, asfloat(v[{lod_idx}]))"),
                        0x25 => format!("_t.SampleLevel({smp_expr}, {coord_f}, asfloat(v[{lod_idx}]))"),
                        0x27 => format!("_t.SampleLevel({smp_expr}, {coord_f}, 0.0)"),
                        _ => format!("_t.SampleLevel({smp_expr}, {coord_f}, 0.0)"),
                    };
                    let _ = writeln!(body, "  float4 _ts = {call};");
                }
                let mut out_k = 0u32;
                let comps = ["x", "y", "z", "w"];
                for ch in 0..4u32 {
                    if dmask & (1 << ch) != 0 {
                        let _ = writeln!(body, "  v[{}] = asuint(_ts.{});", (vdata + out_k) & 0xFF, comps[ch as usize]);
                        out_k += 1;
                    }
                }
                body.push('}');
            } else {
                let mut out_k = 0u32;
                for ch in 0..4u32 {
                    if dmask & (1 << ch) != 0 {
                        let _ = writeln!(body, "v[{}] = 0x3F800000u; /* untracked mimg */", (vdata + out_k) & 0xFF);
                        out_k += 1;
                    }
                }
            }
            stmt = body;
        } else if (w0 >> 26) == 0x38 || (w0 >> 26) == 0x3A {
            // MUBUF / MTBUF
            len_words = 2;
            let op = (w0 >> 18) & 0x7F;
            let off12 = w0 & 0xFFF;
            let offen = (w0 & (1 << 12)) != 0;
            let idxen = (w0 & (1 << 13)) != 0;
            let vaddr = w1 & 0xFF;
            let vdata = (w1 >> 8) & 0xFF;
            let srsrc = (((w1 >> 16) & 0x1F) * 4) as usize;
            let soff = (w1 >> 24) & 0xFF;
            let mut addr_parts: Vec<String> = Vec::new();
            if idxen && offen {
                addr_parts.push(format!("(v[{vaddr}] * 16u + v[{}])", (vaddr + 1) & 0xFF));
            } else if idxen {
                addr_parts.push(format!("(v[{vaddr}] * 16u)"));
            } else if offen {
                addr_parts.push(format!("v[{vaddr}]"));
            }
            if off12 != 0 { addr_parts.push(format!("{off12}u")); }
            if soff != 128 && soff != 124 { addr_parts.push(fmt_src(soff, None, false)); }
            let addr_expr = if addr_parts.is_empty() { "0u".into() } else { addr_parts.join(" + ") };

            // GFX10 MUBUF load sizes: 0x0c=dword, 0x0d=dwordx2, 0x0e=dwordx4, 0x0f=dwordx3.
            let n_dw = match op {
                0x0c => 1,
                0x0d => 2,
                0x0e => 4,
                0x0f => 3,
                0x00..=0x03 => (op + 1).min(4),
                _ => 1,
            };
            match prov.get(srsrc) {
                Some(SgprProv::RootBuf(slot)) => {
                    let _ = writeln!(stmt, "{{ uint _ba = ({addr_expr});");
                    for k in 0..n_dw {
                        let _ = writeln!(stmt, "  v[{}] = g_rdesc_{slot}.Load((_ba + {}u) & 0xFFFFFFFCu);", (vdata + k) & 0xFF, k * 4);
                    }
                    stmt.push('}');
                }
                Some(SgprProv::HeapDesc { is_sampler: false, kind: ResKind::Cbv, idx_expr }) => {
                    uses_cb = true;
                    let _ = writeln!(stmt, "{{ ConstantBuffer<RawCB> _cb = ResourceDescriptorHeap[{idx_expr}]; uint _ba = ({addr_expr});");
                    for k in 0..n_dw {
                        let _ = writeln!(stmt, "  v[{}] = _cb.d[(_ba + {}u) >> 4][((_ba + {}u) >> 2) & 3u];", (vdata + k) & 0xFF, k * 4, k * 4);
                    }
                    stmt.push('}');
                }
                Some(SgprProv::HeapDesc { is_sampler: false, idx_expr, .. }) => {
                    let _ = writeln!(stmt, "{{ ByteAddressBuffer _b = ResourceDescriptorHeap[{idx_expr}]; uint _ba = ({addr_expr});");
                    for k in 0..n_dw {
                        let _ = writeln!(stmt, "  v[{}] = _b.Load((_ba + {}u) & 0xFFFFFFFCu);", (vdata + k) & 0xFF, k * 4);
                    }
                    stmt.push('}');
                }
                _ => {
                    for k in 0..n_dw {
                        let _ = writeln!(stmt, "v[{}] = 0u; /* untracked buf_load s[{srsrc}] */", (vdata + k) & 0xFF);
                    }
                }
            }
        } else if (w0 >> 26) == 0x35 {
            // VOP3 (with abs/neg/clamp modifiers!)
            let op = (w0 >> 16) & 0x3FF;
            let vdst = w0 & 0xFF;
            let abs_bits = (w0 >> 8) & 0x7;
            let clamp_bit = ((w0 >> 15) & 1) != 0;
            let neg_bits = (w1 >> 29) & 0x7;
            let s0 = w1 & 0x1FF;
            let s1 = (w1 >> 9) & 0x1FF;
            let s2 = (w1 >> 18) & 0x1FF;
            let lit = if s0 == 255 || s1 == 255 || s2 == 255 { len_words = 3; Some(w2) } else { len_words = 2; None };
            let fmod = |src: u32, idx: u32| -> String {
                let mut e = fmt_src(src, lit, true);
                if abs_bits & (1 << idx) != 0 { e = format!("abs({e})"); }
                if neg_bits & (1 << idx) != 0 { e = format!("(-({e}))"); }
                e
            };
            let umod = |src: u32| fmt_src(src, lit, false);
            let wrap_f = |e: String| if clamp_bit { format!("saturate({e})") } else { e };

            stmt = match op {
                0x000..=0x0FF => {
                    let is_cmpx = (op & 0x10) != 0;
                    let sym = match op & 0x07 { 1 => "<", 2 => "==", 3 => "<=", 4 => ">", 5 => "!=", 6 => ">=", _ => "==" };
                    let cond = match op & 0x07 {
                        0 => "false".into(),
                        7 => "true".into(),
                        _ if op >= 0xC0 => format!("({} {sym} {})", umod(s0), umod(s1)),
                        _ if op >= 0x80 => format!("(asint({}) {sym} asint({}))", umod(s0), umod(s1)),
                        _ => format!("({} {sym} {})", fmod(s0, 0), fmod(s1, 1)),
                    };
                    if is_cmpx {
                        format!("exec_lo = exec_lo & ({cond} ? 1u : 0u);")
                    } else {
                        let dst = fmt_sdst(vdst);
                        format!("{dst} = {cond} ? 1u : 0u;")
                    }
                }
                0x100..=0x13F => {
                    let v2 = op - 0x100;
                    let (a_f, b_f) = (fmod(s0, 0), fmod(s1, 1));
                    let (a_u, b_u, c_u) = (umod(s0), umod(s1), umod(s2));
                    match v2 {
                        0x00 => format!("v[{vdst}] = (({c_u} & 1u) != 0u) ? {b_u} : {a_u};"),
                        0x03 => format!("v[{vdst}] = asuint({});", wrap_f(format!("({a_f} + {b_f})"))),
                        0x04 => format!("v[{vdst}] = asuint({});", wrap_f(format!("({a_f} - {b_f})"))),
                        0x05 => format!("v[{vdst}] = asuint({});", wrap_f(format!("({b_f} - {a_f})"))),
                        0x06 | 0x1F | 0x2B => format!("v[{vdst}] = asuint({});", wrap_f(format!("(asfloat(v[{vdst}]) + {a_f} * {b_f})"))),
                        0x07 | 0x08 => format!("v[{vdst}] = asuint({});", wrap_f(format!("({a_f} * {b_f})"))),
                        0x09..=0x0C => format!("v[{vdst}] = {a_u} * {b_u};"),
                        0x0F => format!("v[{vdst}] = asuint({});", wrap_f(format!("min({a_f}, {b_f})"))),
                        0x10 => format!("v[{vdst}] = asuint({});", wrap_f(format!("max({a_f}, {b_f})"))),
                        0x11 => format!("v[{vdst}] = asuint(min(asint({a_u}), asint({b_u})));"),
                        0x12 => format!("v[{vdst}] = asuint(max(asint({a_u}), asint({b_u})));"),
                        0x13 => format!("v[{vdst}] = min({a_u}, {b_u});"),
                        0x14 => format!("v[{vdst}] = max({a_u}, {b_u});"),
                        0x16 => format!("v[{vdst}] = {b_u} >> ({a_u} & 31u);"),
                        0x18 => format!("v[{vdst}] = asuint(asint({b_u}) >> ({a_u} & 31u));"),
                        0x1A => format!("v[{vdst}] = {b_u} << ({a_u} & 31u);"),
                        0x1B => format!("v[{vdst}] = {a_u} & {b_u};"),
                        0x1C => format!("v[{vdst}] = {a_u} | {b_u};"),
                        0x1D => format!("v[{vdst}] = {a_u} ^ {b_u};"),
                        0x1E => format!("v[{vdst}] = ~({a_u} ^ {b_u});"),
                        0x25 | 0x28 => format!("v[{vdst}] = {a_u} + {b_u};"),
                        0x26 | 0x29 => format!("v[{vdst}] = {a_u} - {b_u};"),
                        0x27 | 0x2A => format!("v[{vdst}] = {b_u} - {a_u};"),
                        0x2F => format!("v[{vdst}] = (f32tof16({a_f}) & 0xFFFFu) | ((f32tof16({b_f}) & 0xFFFFu) << 16);"),
                        _ => { unknown_ops += 1; String::new() }
                    }
                }
                0x140 | 0x141 | 0x14B | 0x16B | 0x17B => {
                    // v_mad_legacy_f32 / v_mad_f32 / v_fma_f32 (0x14B on GFX10!)
                    format!("v[{vdst}] = asuint({});", wrap_f(format!("({} * {} + {})", fmod(s0, 0), fmod(s1, 1), fmod(s2, 2))))
                }
                0x142 | 0x143 => format!("v[{vdst}] = {} * {} + {};", umod(s0), umod(s1), umod(s2)),
                0x144 | 0x145 | 0x148 | 0x149 => format!("v[{vdst}] = ({} >> ({} & 31u)) & ((1u << ({} & 31u)) - 1u);", umod(s0), umod(s1), umod(s2)),
                0x146 | 0x14A => format!("v[{vdst}] = ({} & {}) | (~{} & {});", umod(s0), umod(s1), umod(s0), umod(s2)),
                0x147 => format!("v[{vdst}] = (({} << (32u - ({} & 31u))) | ({} >> ({} & 31u)));", umod(s0), umod(s2), umod(s1), umod(s2)),
                0x151 => format!("v[{vdst}] = asuint({});", wrap_f(format!("min(min({}, {}), {})", fmod(s0, 0), fmod(s1, 1), fmod(s2, 2)))),
                0x152 => format!("v[{vdst}] = asuint(min(min(asint({}), asint({})), asint({})));", umod(s0), umod(s1), umod(s2)),
                0x153 => format!("v[{vdst}] = min(min({}, {}), {});", umod(s0), umod(s1), umod(s2)),
                0x154 => format!("v[{vdst}] = asuint({});", wrap_f(format!("max(max({}, {}), {})", fmod(s0, 0), fmod(s1, 1), fmod(s2, 2)))),
                0x155 => format!("v[{vdst}] = asuint(max(max(asint({}), asint({})), asint({})));", umod(s0), umod(s1), umod(s2)),
                0x156 => format!("v[{vdst}] = max(max({}, {}), {});", umod(s0), umod(s1), umod(s2)),
                0x157 => format!("v[{vdst}] = asuint({});", wrap_f(format!("clamp({}, min({}, {}), max({}, {}))", fmod(s0, 0), fmod(s1, 1), fmod(s2, 2), fmod(s1, 1), fmod(s2, 2)))),
                0x158 => format!("v[{vdst}] = asuint(clamp(asint({}), min(asint({}), asint({})), max(asint({}), asint({}))));", umod(s0), umod(s1), umod(s2), umod(s1), umod(s2)),
                0x159 => format!("v[{vdst}] = clamp({}, min({}, {}), max({}, {}));", umod(s0), umod(s1), umod(s2), umod(s1), umod(s2)),
                0x169 => format!("v[{vdst}] = {} * {};", umod(s0), umod(s1)),
                0x16A => format!("{{ uint _hi; umul({}, {}, _hi); v[{vdst}] = _hi; }}", umod(s0), umod(s1)),
                0x180..=0x1EF => {
                    let v1_op = op - 0x180;
                    let (a_f, a_u) = (fmod(s0, 0), umod(s0));
                    match v1_op {
                        0x00 => String::new(),
                        0x01 => format!("v[{vdst}] = {};", if abs_bits != 0 || neg_bits != 0 || clamp_bit { format!("asuint({})", wrap_f(a_f)) } else { a_u }),
                        0x05 => format!("v[{vdst}] = asuint((float)asint({a_u}));"),
                        0x06 => format!("v[{vdst}] = asuint((float){a_u});"),
                        0x07 => format!("v[{vdst}] = (uint)(int){a_f};"),
                        0x08 => format!("v[{vdst}] = asuint((int){a_f});"),
                        0x0A => format!("v[{vdst}] = f32tof16({a_f});"),
                        0x0B => format!("v[{vdst}] = asuint(f16tof32({a_u} & 0xFFFFu));"),
                        0x1F | 0x20 => format!("v[{vdst}] = asuint(frac({a_f}));"),
                        0x21 => format!("v[{vdst}] = asuint(trunc({a_f}));"),
                        0x22 => format!("v[{vdst}] = asuint(ceil({a_f}));"),
                        0x23 => format!("v[{vdst}] = asuint(round({a_f}));"),
                        0x24 => format!("v[{vdst}] = asuint(floor({a_f}));"),
                        0x25 => format!("v[{vdst}] = asuint(exp2({a_f}));"),
                        0x27 => format!("v[{vdst}] = asuint(log2(max({a_f}, 1e-12)));"),
                        0x2A => format!("v[{vdst}] = asuint(rcp({a_f}));"),
                        0x2E => format!("v[{vdst}] = asuint(rsqrt(max({a_f}, 1e-12)));"),
                        0x33 => format!("v[{vdst}] = asuint(sqrt(max({a_f}, 0.0)));"),
                        0x35 => format!("v[{vdst}] = asuint(sin({a_f} * 6.2831853));"),
                        0x36 => format!("v[{vdst}] = asuint(cos({a_f} * 6.2831853));"),
                        0x37 => format!("v[{vdst}] = ~{a_u};"),
                        _ => { unknown_ops += 1; String::new() }
                    }
                }
                0x342 | 0x371 => format!("v[{vdst}] = ({} & {}) | {};", umod(s0), umod(s1), umod(s2)),
                0x346 | 0x36F => format!("v[{vdst}] = ({} << ({} & 31u)) + {};", umod(s0), umod(s1), umod(s2)),
                0x347 => format!("v[{vdst}] = ({} + {}) << ({} & 31u);", umod(s0), umod(s1), umod(s2)),
                0x36D => format!("v[{vdst}] = {} + {} + {};", umod(s0), umod(s1), umod(s2)),
                0x372 => format!("v[{vdst}] = {} | {} | {};", umod(s0), umod(s1), umod(s2)),
                0x373 => format!("v[{vdst}] = ({} & 0xFFFFu) * ({} & 0xFFFFu) + {};", umod(s0), umod(s1), umod(s2)),
                _ => { unknown_ops += 1; format!("/* UNK op={op:#x} w0={w0:#010x} w1={w1:#010x} */") }
            };
        } else if (w0 >> 26) == 0x32 || (w0 >> 26) == 0x34 {
            // VINTRP
            if (w0 >> 26) == 0x34 { len_words = 2; }
            let op = (w0 >> 16) & 0x3;
            let vdst = (w0 >> 18) & 0xFF;
            let attr = (w0 >> 10) & 0x3F;
            let chan = (w0 >> 8) & 0x3;
            if op != 0 {
                // p2 or mov: assign final interpolated attribute component!
                if prog.ps_inputs.iter().any(|pi| pi.attr as u32 == attr) {
                    stmt = format!("v[{vdst}] = asuint(_in.i_a{attr}[{chan}]);");
                } else {
                    stmt = format!("v[{vdst}] = 0u; /* unmapped attr{attr}.{chan} */");
                }
            }
        } else if (w0 >> 25) == 0x3F {
            // VOP1
            let op = (w0 >> 9) & 0xFF;
            let src0 = w0 & 0x1FF;
            let vdst = (w0 >> 17) & 0xFF;
            let lit = if matches!(src0, 249 | 250 | 255) { len_words = 2; if src0 == 255 { Some(w1) } else { None } } else { None };
            let (a_f, a_u) = (fmt_src(src0, lit, true), fmt_src(src0, lit, false));
            stmt = match op {
                0x00 => String::new(),
                0x01 => format!("v[{vdst}] = {a_u};"),
                0x02 => format!("s[{vdst}] = {a_u};"), // v_readfirstlane_b32
                0x05 => format!("v[{vdst}] = asuint((float)asint({a_u}));"),
                0x06 => format!("v[{vdst}] = asuint((float){a_u});"),
                0x07 => format!("v[{vdst}] = (uint)(int){a_f};"),
                0x08 => format!("v[{vdst}] = asuint((int){a_f});"),
                0x0A => format!("v[{vdst}] = f32tof16({a_f});"),
                0x0B => format!("v[{vdst}] = asuint(f16tof32({a_u} & 0xFFFFu));"),
                0x12 => format!("v[{vdst}] = asuint((float)({a_u} & 0xFFu));"),
                0x13 => format!("v[{vdst}] = asuint((float)(({a_u} >> 8) & 0xFFu));"),
                0x14 => format!("v[{vdst}] = asuint((float)(({a_u} >> 16) & 0xFFu));"),
                0x15 => format!("v[{vdst}] = asuint((float)(({a_u} >> 24) & 0xFFu));"),
                0x1F | 0x20 => format!("v[{vdst}] = asuint(frac({a_f}));"),
                0x21 => format!("v[{vdst}] = asuint(trunc({a_f}));"),
                0x22 => format!("v[{vdst}] = asuint(ceil({a_f}));"),
                0x23 => format!("v[{vdst}] = asuint(round({a_f}));"),
                0x24 => format!("v[{vdst}] = asuint(floor({a_f}));"),
                0x25 => format!("v[{vdst}] = asuint(exp2({a_f}));"),
                0x27 => format!("v[{vdst}] = asuint(log2(max({a_f}, 1e-12)));"),
                0x2A => format!("v[{vdst}] = asuint(rcp({a_f}));"),
                0x2E => format!("v[{vdst}] = asuint(rsqrt(max({a_f}, 1e-12)));"),
                0x33 => format!("v[{vdst}] = asuint(sqrt(max({a_f}, 0.0)));"),
                0x35 => format!("v[{vdst}] = asuint(sin({a_f} * 6.2831853));"),
                0x36 => format!("v[{vdst}] = asuint(cos({a_f} * 6.2831853));"),
                0x37 => format!("v[{vdst}] = ~{a_u};"),
                _ => { unknown_ops += 1; format!("/* UNK op={op:#x} w0={w0:#010x} w1={w1:#010x} */") }
            };
        } else if (w0 >> 25) == 0x3E {
            // VOPC (v_cmp_* writes vcc; v_cmpx_* with (op & 0x10) != 0 writes exec_lo!)
            let op = (w0 >> 17) & 0xFF;
            let src0 = w0 & 0x1FF;
            let vsrc1 = (w0 >> 9) & 0xFF;
            let lit = if matches!(src0, 249 | 250 | 255) { len_words = 2; if src0 == 255 { Some(w1) } else { None } } else { None };
            let (a_f, b_f) = (fmt_src(src0, lit, true), format!("asfloat(v[{vsrc1}])"));
            let (a_u, b_u) = (fmt_src(src0, lit, false), format!("v[{vsrc1}]"));
            let sym = match op & 0x07 { 1 => "<", 2 => "==", 3 => "<=", 4 => ">", 5 => "!=", 6 => ">=", _ => "==" };
            let cond = match op & 0x07 {
                0 => "false".into(),
                7 => "true".into(),
                _ if op < 0x80 => format!("({a_f} {sym} {b_f})"),
                _ if op < 0xC0 => format!("(asint({a_u}) {sym} asint({b_u}))"),
                _ => format!("({a_u} {sym} {b_u})"),
            };
            if (op & 0x10) != 0 {
                stmt = format!("exec_lo = exec_lo & ({cond} ? 1u : 0u);");
            } else {
                stmt = format!("vcc = {cond} ? 1u : 0u;");
            }
        } else if (w0 >> 31) == 0 {
            // VOP2
            let op = (w0 >> 25) & 0x3F;
            let src0 = w0 & 0x1FF;
            let vsrc1 = (w0 >> 9) & 0xFF;
            let vdst = (w0 >> 17) & 0xFF;
            let always_lit = matches!(op, 0x20 | 0x21 | 0x2C | 0x2D | 0x37 | 0x38);
            let lit = if always_lit || matches!(src0, 249 | 250 | 255) { len_words = 2; Some(w1) } else { None };
            let (a_f, b_f) = (fmt_src(src0, lit, true), format!("asfloat(v[{vsrc1}])"));
            let (a_u, b_u) = (fmt_src(src0, lit, false), format!("v[{vsrc1}]"));
            let imm_f = format!("asfloat(0x{:08x}u)", w1);
            stmt = match op {
                0x00 | 0x01 => format!("v[{vdst}] = ((vcc & 1u) != 0u) ? {b_u} : {a_u};"), // v_cndmask_b32 (GFX10=0x01)
                0x03 => format!("v[{vdst}] = asuint({a_f} + {b_f});"),
                0x04 => format!("v[{vdst}] = asuint({a_f} - {b_f});"),
                0x05 => format!("v[{vdst}] = asuint({b_f} - {a_f});"),
                0x06 | 0x1F | 0x2A | 0x2B => format!("v[{vdst}] = asuint(asfloat(v[{vdst}]) + {a_f} * {b_f});"),
                0x07 | 0x08 => format!("v[{vdst}] = asuint({a_f} * {b_f});"),
                0x09..=0x0C => format!("v[{vdst}] = {a_u} * {b_u};"),
                0x0F => format!("v[{vdst}] = asuint(min({a_f}, {b_f}));"),
                0x10 => format!("v[{vdst}] = asuint(max({a_f}, {b_f}));"),
                0x11 => format!("v[{vdst}] = asuint(min(asint({a_u}), asint({b_u})));"),
                0x12 => format!("v[{vdst}] = asuint(max(asint({a_u}), asint({b_u})));"),
                0x13 => format!("v[{vdst}] = min({a_u}, {b_u});"),
                0x14 => format!("v[{vdst}] = max({a_u}, {b_u});"),
                0x16 => format!("v[{vdst}] = {b_u} >> ({a_u} & 31u);"),
                0x18 => format!("v[{vdst}] = asuint(asint({b_u}) >> ({a_u} & 31u));"),
                0x1A => format!("v[{vdst}] = {b_u} << ({a_u} & 31u);"),
                0x1B => format!("v[{vdst}] = {a_u} & {b_u};"),
                0x1C => format!("v[{vdst}] = {a_u} | {b_u};"),
                0x1D => format!("v[{vdst}] = {a_u} ^ {b_u};"),
                0x1E => format!("v[{vdst}] = ~({a_u} ^ {b_u});"),
                0x20 | 0x2C => format!("v[{vdst}] = asuint({a_f} * {imm_f} + {b_f});"),
                0x21 | 0x2D => format!("v[{vdst}] = asuint({a_f} * {b_f} + {imm_f});"),
                0x23 | 0x25 => format!("v[{vdst}] = {a_u} + {b_u};"),
                0x26 => format!("v[{vdst}] = {a_u} - {b_u};"),
                0x27 => format!("v[{vdst}] = {b_u} - {a_u};"),
                0x28 => format!("v[{vdst}] = {a_u} + {b_u} + (vcc & 1u);"),
                0x29 => format!("v[{vdst}] = {a_u} - {b_u} - (vcc & 1u);"),
                0x2F => format!("v[{vdst}] = (f32tof16({a_f}) & 0xFFFFu) | ((f32tof16({b_f}) & 0xFFFFu) << 16);"),
                _ => { unknown_ops += 1; format!("/* UNK op={op:#x} w0={w0:#010x} w1={w1:#010x} */") }
            };
        } else if (w0 >> 30) == 2 {
            // SOP2
            let op = (w0 >> 23) & 0x7F;
            let ssrc0 = w0 & 0xFF;
            let ssrc1 = (w0 >> 8) & 0xFF;
            let sdst = (w0 >> 16) & 0x7F;
            let lit = if ssrc0 == 255 || ssrc1 == 255 { len_words = 2; Some(w1) } else { None };
            let a = fmt_src(ssrc0, lit, false);
            let b = fmt_src(ssrc1, lit, false);
            let dst = fmt_sdst(sdst);
            if (sdst as usize) < 106 {
                // Propagate HeapPtr + const offset for s_add_u32
                if matches!(op, 0x00 | 0x02) {
                    if let (Some(SgprProv::HeapPtr { is_sampler, root_slot, expr, byte_off }), Some(c)) =
                        (prov.get(ssrc0 as usize).cloned(), eval_const_src(ssrc1, lit, &prov))
                    {
                        prov[sdst as usize] = SgprProv::HeapPtr { is_sampler, root_slot, expr, byte_off: byte_off.wrapping_add(c) };
                    } else {
                        prov[sdst as usize] = SgprProv::Unknown;
                    }
                } else {
                    prov[sdst as usize] = SgprProv::Unknown;
                }
            }
            stmt = match op {
                0x00 | 0x02 => format!("{dst} = {a} + {b}; scc = ({dst} < {a}) ? 1u : 0u;"),
                0x01 | 0x03 => format!("{dst} = {a} - {b}; scc = ({a} < {b}) ? 1u : 0u;"),
                0x04 => format!("{dst} = {a} + {b} + (scc & 1u);"),
                0x05 => format!("{dst} = {a} - {b} - (scc & 1u);"),
                0x06 => format!("{dst} = asuint(min(asint({a}), asint({b})));"),
                0x07 => format!("{dst} = min({a}, {b});"),
                0x08 => format!("{dst} = asuint(max(asint({a}), asint({b})));"),
                0x09 => format!("{dst} = max({a}, {b});"),
                0x0A | 0x0B => format!("{dst} = (scc != 0u) ? {a} : {b};"),
                0x0E | 0x0F => {
                    if sdst == 126 { uses_exec_kill = true; }
                    format!("{dst} = {a} & {b}; scc = ({dst} != 0u) ? 1u : 0u;")
                }
                0x10 | 0x11 => format!("{dst} = {a} | {b}; scc = ({dst} != 0u) ? 1u : 0u;"),
                0x12 | 0x13 => format!("{dst} = {a} ^ {b}; scc = ({dst} != 0u) ? 1u : 0u;"),
                0x14 | 0x15 => format!("{dst} = {a} & ~{b}; scc = ({dst} != 0u) ? 1u : 0u;"),
                0x1C | 0x1D => format!("{dst} = {a} << ({b} & 31u);"),
                0x1E | 0x1F => format!("{dst} = {a} >> ({b} & 31u);"),
                0x20..=0x22 => format!("{dst} = asuint(asint({a}) >> ({b} & 31u));"),
                0x2C => format!("{dst} = asuint(asint({a}) * asint({b}));"),
                _ => { unknown_ops += 1; format!("/* UNK op={op:#x} w0={w0:#010x} w1={w1:#010x} */") }
            };
        } else {
            unknown_ops += 1;
        }

        insns.push(Insn { off, stmt, is_endpgm, branch_target, branch_cond });
        off += len_words * 4;
        if let Some(t) = branch_target {
            furthest_branch = furthest_branch.max(t);
        }
        if is_endpgm && furthest_branch < off {
            break;
        }
    }

    // 3. Emit SM 6.6 HLSL header + resource declarations + entry point.
    let mut hlsl = String::with_capacity(4096);
    if !ud_sources.is_empty() {
        // Scalar fields guarantee tight 4-byte packing matching root-constant dword indices.
        hlsl.push_str("cbuffer UserData : register(b0, space0) {\n");
        for i in 0..ud_sources.len() {
            let _ = writeln!(hlsl, "    uint g_ud_{i};");
        }
        hlsl.push_str("};\n");
    }
    if uses_cb {
        hlsl.push_str("struct RawCB { uint4 d[4096]; };\n");
    }
    for &slot in &root_descs {
        let _ = writeln!(hlsl, "ByteAddressBuffer g_rdesc_{slot} : register(t{slot}, space1);");
    }
    for k in 0..static_samplers.len() {
        let _ = writeln!(hlsl, "SamplerState g_ss_{k} : register(s{k}, space2);");
    }
    hlsl.push('\n');

    let mut body = String::with_capacity(4096);
    body.push_str("    uint s[106]; uint v[64]; uint vcc = 0u; uint scc = 0u; uint exec_lo = 0xFFFFFFFFu; uint exec_hi = 0u; uint _s_sink = 0u;\n");
    body.push_str("    [unroll] for (uint _i = 0u; _i < 16u; ++_i) s[_i] = 0u;\n");
    body.push_str("    [unroll] for (uint _j = 0u; _j < 32u; ++_j) v[_j] = 0u;\n");
    body.push_str(&init_stmts);

    match prog.stage {
        HwStage::Vs => {
            body.push_str("    v[0] = _vid; v[3] = _iid;\n");
            for vi in &prog.vs_inputs {
                if let Some(sig) = find_sig(&prog.isg, vi.row) {
                    if sig.sys_val != 0 { continue; }
                    let is_float = sig.comp_type == 3 || sig.comp_type == 0;
                    for ch in 0..4u8 {
                        let vr = vi.vgpr as usize + ch as usize;
                        if vr >= 64 { break; }
                        if vi.mask & (1 << ch) != 0 {
                            if is_float {
                                let _ = writeln!(body, "    v[{vr}] = asuint(_in.v_r{}[{ch}]);", vi.row);
                            } else {
                                let _ = writeln!(body, "    v[{vr}] = (uint)_in.v_r{}[{ch}];", vi.row);
                            }
                        }
                    }
                }
            }
            body.push_str("    float4 exp_pos0 = float4(0.0, 0.0, 0.0, 1.0);\n");
            body.push_str("    float4 exp_param[16]; [unroll] for (uint _p = 0u; _p < 16u; ++_p) exp_param[_p] = float4(0,0,0,0);\n");
        }
        HwStage::Ps => {
            // Map SPI_PS_INPUT_ADDR bits to hardware PS input VGPRs.
            let mut vg = 0u32;
            let bit_sizes: [u32; 15] = [2, 2, 2, 3, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1];
            for (bit, &sz) in bit_sizes.iter().enumerate() {
                if (prog.spi_ps_input_addr & (1 << bit)) != 0 {
                    match bit {
                        8 => { let _ = writeln!(body, "    v[{vg}] = asuint(_in.pos.x);"); }
                        9 => { let _ = writeln!(body, "    v[{vg}] = asuint(_in.pos.y);"); }
                        10 => { let _ = writeln!(body, "    v[{vg}] = asuint(_in.pos.z);"); }
                        11 => { let _ = writeln!(body, "    v[{vg}] = asuint(_in.pos.w);"); }
                        _ => {}
                    }
                    vg += sz;
                }
            }
            body.push_str("    float4 exp_mrt[8]; [unroll] for (uint _m = 0u; _m < 8u; ++_m) exp_mrt[_m] = float4(0,0,0,1);\n");
        }
        _ => {}
    }

    let uses_exec_mask = insns.iter().any(|o| o.stmt.contains("exec_lo =") || o.stmt.contains("exec_lo &=") || o.stmt.contains("exec_lo |=") || o.stmt.contains("exec_lo ^="));
    let fmt_stmt = |s: &str| -> String {
        if uses_exec_mask && s.starts_with("v[") {
            format!("if ((exec_lo & 1u) != 0u) {{ {s} }}")
        } else {
            s.to_string()
        }
    };

    let has_branches = insns.iter().any(|o| o.branch_target.is_some());
    if !has_branches {
        for o in &insns {
            if !o.stmt.is_empty() {
                let s = fmt_stmt(&o.stmt);
                for line in s.lines() {
                    let _ = writeln!(body, "    {line}");
                }
            }
        }
    } else {
        body.push_str("    uint _pc = 0u; uint _iters = 0u;\n");
        body.push_str("    while (_pc != 0xFFFFFFFFu && _iters < 256u) {\n        _iters++;\n        switch (_pc) {\n");
        for (idx, o) in insns.iter().enumerate() {
            let _ = writeln!(body, "        case {}u: {{", o.off);
            if !o.stmt.is_empty() {
                let s = fmt_stmt(&o.stmt);
                for line in s.lines() {
                    let _ = writeln!(body, "            {line}");
                }
            }
            let next_pc = insns.get(idx + 1).map(|n| n.off as u32).unwrap_or(0xFFFF_FFFF);
            if o.is_endpgm {
                body.push_str("            _pc = 0xFFFFFFFFu; break;\n");
            } else if let Some(tgt) = o.branch_target {
                let valid = if insns.iter().any(|x| x.off == tgt) { tgt as u32 } else { next_pc };
                if let Some(cond) = o.branch_cond {
                    let _ = writeln!(body, "            _pc = {cond} ? {valid}u : {next_pc}u; break;");
                } else {
                    let _ = writeln!(body, "            _pc = {valid}u; break;");
                }
            } else {
                let _ = writeln!(body, "            _pc = {next_pc}u; break;");
            }
            body.push_str("        }\n");
        }
        body.push_str("        default: _pc = 0xFFFFFFFFu; break;\n        }\n    }\n");
    }

    match prog.stage {
        HwStage::Vs => {
            let mut has_vsin = false;
            hlsl.push_str("struct VSIn {\n");
            for s in &prog.isg {
                if s.sys_val != 0 { continue; }
                has_vsin = true;
                let _ = writeln!(hlsl, "    {} v_r{} : {}{};", sig_vec_type(s.comp_type), s.register, s.semantic, s.index);
            }
            hlsl.push_str("};\nstruct VSOut {\n    float4 pos : SV_Position;\n");
            for vo in &prog.vs_outputs {
                if let Some(s) = find_sig(&prog.osg, vo.row) {
                    if s.sys_val == 1 || s.semantic.eq_ignore_ascii_case("SV_Position") { continue; }
                    let _ = writeln!(hlsl, "    float4 o_r{} : {}{};", vo.row, s.semantic, s.index);
                }
            }
            hlsl.push_str("};\n");
            if has_vsin {
                hlsl.push_str("VSOut main(VSIn _in, uint _vid : SV_VertexID, uint _iid : SV_InstanceID) {\n");
            } else {
                hlsl.push_str("VSOut main(uint _vid : SV_VertexID, uint _iid : SV_InstanceID) {\n");
            }
            hlsl.push_str(&body);
            hlsl.push_str("    VSOut _out;\n    _out.pos = exp_pos0;\n");
            for vo in &prog.vs_outputs {
                if let Some(s) = find_sig(&prog.osg, vo.row) {
                    if s.sys_val == 1 || s.semantic.eq_ignore_ascii_case("SV_Position") { continue; }
                    let _ = writeln!(hlsl, "    _out.o_r{} = exp_param[{}];", vo.row, (vo.param as usize).min(15));
                }
            }
            hlsl.push_str("    return _out;\n}\n");
        }
        HwStage::Ps => {
            hlsl.push_str("struct PSIn {\n    float4 pos : SV_Position;\n");
            for pi in &prog.ps_inputs {
                if let Some(s) = find_sig(&prog.isg, pi.row) {
                    if s.sys_val == 1 || s.semantic.eq_ignore_ascii_case("SV_Position") { continue; }
                    let qual = if pi.flags & 1 != 0 {
                        "nointerpolation "
                    } else if pi.flags & 2 != 0 {
                        "noperspective "
                    } else {
                        ""
                    };
                    let _ = writeln!(hlsl, "    {qual}float4 i_a{} : {}{};", pi.attr, s.semantic, s.index);
                }
            }
            hlsl.push_str("};\nstruct PSOut {\n");
            let targets: Vec<u32> = prog.osg.iter()
                .filter(|s| s.semantic.eq_ignore_ascii_case("SV_Target"))
                .map(|s| s.index)
                .collect();
            let targets = if targets.is_empty() { vec![0] } else { targets };
            for &t in &targets {
                let _ = writeln!(hlsl, "    float4 t{t} : SV_Target{t};");
            }
            hlsl.push_str("};\nPSOut main(PSIn _in) {\n");
            hlsl.push_str(&body);
            hlsl.push_str("    PSOut _out;\n");
            for &t in &targets {
                let _ = writeln!(hlsl, "    _out.t{t} = exp_mrt[{}];", (t as usize).min(7));
            }
            hlsl.push_str("    return _out;\n}\n");
        }
        _ => unreachable!(),
    }

    Ok(HostShaderMeta {
        stage: prog.stage,
        hlsl,
        ud_dwords: ud_sources.len() as u32,
        ud_sources,
        root_descs,
        static_samplers,
        unknown_ops,
    })
}
