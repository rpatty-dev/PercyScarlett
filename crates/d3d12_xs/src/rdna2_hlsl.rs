//! Clean-room RDNA2 (GFX10.3) instruction decoder and instruction-level HLSL lifter.

use std::fmt::Write as _;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShaderStage {
    Vertex,
    Pixel,
    Compute,
    Mesh,
    Geometry,
    RayTracing,
}

impl ShaderStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Vertex => "vs",
            Self::Pixel => "ps",
            Self::Compute => "cs",
            Self::Mesh => "ms",
            Self::Geometry => "gs",
            Self::RayTracing => "rt",
        }
    }

    pub fn dxbc_target(self) -> &'static str {
        match self {
            Self::Vertex => "vs_5_0",
            Self::Pixel => "ps_5_0",
            Self::Compute => "cs_5_0",
            Self::Geometry => "gs_5_0",
            Self::Mesh | Self::RayTracing => "cs_5_0",
        }
    }

    pub fn dxil_target(self) -> &'static str {
        match self {
            Self::Vertex => "vs_6_6",
            Self::Pixel => "ps_6_6",
            Self::Compute => "cs_6_6",
            Self::Mesh => "ms_6_6",
            Self::Geometry => "gs_6_6",
            Self::RayTracing => "lib_6_6",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedOp {
    pub offset: usize,
    pub word0: u32,
    pub word1: u32,
    pub len_words: usize,
    pub mnemonic: String,
    pub hlsl_stmt: String,
    pub is_known: bool,
    pub is_endpgm: bool,
    pub is_export: bool,
    pub is_export_prim: bool,
    pub uses_lds: bool,
    pub uses_wave: bool,
    /// Relative branch target byte offset (for SOPP branch ops), if any.
    pub branch_target: Option<usize>,
    /// Condition expression in HLSL (`None` = unconditional branch).
    pub branch_cond: Option<&'static str>,
}

fn fmt_src(src: u32, lit: Option<u32>, as_float: bool) -> String {
    match src {
        0..=105 => {
            if as_float {
                format!("asfloat(s[{src}])")
            } else {
                format!("s[{src}]")
            }
        }
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
            if as_float {
                format!("asfloat(0x{imm:08x}u)")
            } else {
                format!("0x{imm:08x}u")
            }
        }
        256..=511 => {
            let v = src - 256;
            if as_float {
                format!("asfloat(v[{v}])")
            } else {
                format!("v[{v}]")
            }
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

fn read_u32(bytes: &[u8], off: usize) -> Option<u32> {
    bytes.get(off..off + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
}

/// Decodes an RDNA2 (GFX10.3 + Scarlett GFX10.1 extensions) instruction stream into per-instruction ops + HLSL statements.
pub fn decode_rdna2(bytes: &[u8]) -> Vec<DecodedOp> {
    let mut ops = Vec::with_capacity(bytes.len() / 4);
    let mut off = 0usize;
    while off + 4 <= bytes.len() {
        let w0 = read_u32(bytes, off).unwrap();
        if w0 == 0xEFCD_AB89 || w0 == 0x89AB_CDEF {
            break;
        }
        let w1 = read_u32(bytes, off + 4).unwrap_or(0);
        let mut len_words = 1usize;
        let mut is_known = true;
        let mut is_endpgm = false;
        let mut is_export = false;
        let mut is_export_prim = false;
        let mut uses_lds = false;
        let mut uses_wave = false;
        let mut branch_target: Option<usize> = None;
        let mut branch_cond: Option<&'static str> = None;
        let mut hlsl_stmt = String::new();

        let mnemonic = if (w0 & 0xFFFF_0000) == 0xBF81_0000 || (w0 & 0xFFFF_0000) == 0xBF9F_0000 {
            is_endpgm = true;
            let tag = if (w0 & 0xFFFF_0000) == 0xBF9F_0000 { "s_code_end" } else { "s_endpgm" };
            hlsl_stmt = format!("// {tag}");
            tag.to_string()
        } else if (w0 & 0xFF80_0000) == 0xBF80_0000 {
            // SOPP
            let op = (w0 >> 16) & 0x7F;
            let imm = (w0 & 0xFFFF) as i16;
            match op {
                0x00 => {
                    hlsl_stmt = "/* s_nop */".into();
                    "s_nop".into()
                }
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
                    hlsl_stmt = format!("/* branch op=0x{op:02x} -> @L_{target} */");
                    format!("s_branch_0x{op:02x} {imm}")
                }
                0x0A => {
                    hlsl_stmt = "GroupMemoryBarrierWithGroupSync();".into();
                    "s_barrier".into()
                }
                0x0C | 0x0D | 0x0E | 0x0F | 0x10 | 0x11 | 0x13 | 0x14 | 0x15 | 0x20 | 0x21 | 0x26 | 0x27 => {
                    hlsl_stmt = format!("/* sopp_0x{op:02x} 0x{:04x} */", w0 & 0xFFFF);
                    format!("sopp_0x{op:02x} 0x{:04x}", w0 & 0xFFFF)
                }
                _ => {
                    is_known = false;
                    hlsl_stmt = format!("/* UNKNOWN sopp_0x{op:02x} {imm} */");
                    format!("sopp_0x{op:02x} {imm}")
                }
            }
        } else if (w0 >> 23) == 0x17D {
            // SOP1
            let op = (w0 >> 8) & 0xFF;
            let ssrc0 = w0 & 0xFF;
            let sdst = (w0 >> 16) & 0x7F;
            let lit = if ssrc0 == 255 {
                len_words = 2;
                Some(w1)
            } else {
                None
            };
            let a = fmt_src(ssrc0, lit, false);
            let dst = fmt_sdst(sdst);
            hlsl_stmt = match op {
                0x03 => format!("{dst} = {a};"), // s_mov_b32
                0x04 => {
                    let dst_hi = fmt_sdst(sdst + 1);
                    format!("{dst} = {a}; {dst_hi} = 0u;") // s_mov_b64
                }
                0x05 => format!("if (scc != 0u) {{ {dst} = {a}; }}"), // s_cmov_b32
                0x06 | 0x07 => format!("{dst} = ~{a}; scc = ({dst} != 0u) ? 1u : 0u;"), // s_not_b32
                0x08 => format!("{dst} = ~{a};"), // s_not_b64
                0x09 | 0x0A => {
                    uses_wave = true;
                    format!("{dst} = countbits({a}); scc = ({dst} != 0u) ? 1u : 0u;") // s_bcnt1_i32_b32 / s_wqm
                }
                0x0B | 0x10 => {
                    uses_wave = true;
                    format!("{dst} = countbits(exec_lo) + countbits(exec_hi); scc = ({dst} != 0u) ? 1u : 0u;") // s_bcnt1_i32_b64
                }
                0x0F | 0x13 => format!("{dst} = firstbitlow({a});"), // s_ff1_i32_b32
                0x17 => format!("{dst} = firstbithigh({a});"),       // s_flbit_i32_b32
                0x1F => {
                    let dst_hi = fmt_sdst(sdst + 1);
                    format!("{dst} = {}u; {dst_hi} = 0u;", off + 4) // s_getpc_b64
                }
                0x24 | 0x25 => {
                    uses_wave = true;
                    format!("{dst} = exec_lo; exec_lo &= {a}; scc = (exec_lo != 0u) ? 1u : 0u;") // s_and_saveexec_b64 / b32
                }
                0x26 | 0x27 => {
                    uses_wave = true;
                    format!("{dst} = exec_lo; exec_lo |= {a}; scc = (exec_lo != 0u) ? 1u : 0u;") // s_or_saveexec_b64 / b32
                }
                0x28 | 0x29 => {
                    uses_wave = true;
                    format!("{dst} = exec_lo; exec_lo ^= {a}; scc = (exec_lo != 0u) ? 1u : 0u;") // s_xor_saveexec_b64 / b32
                }
                0x2A | 0x2B => {
                    uses_wave = true;
                    format!("{dst} = exec_lo; exec_lo &= ~{a}; scc = (exec_lo != 0u) ? 1u : 0u;") // s_andn2_saveexec_b64 / b32
                }
                0x3C | 0x3D => {
                    uses_wave = true;
                    format!("{dst} = exec_lo; exec_lo = ~exec_lo & {a}; scc = (exec_lo != 0u) ? 1u : 0u;") // s_andn1_saveexec_b64 / wrexec
                }
                0x47 => format!("{dst} = asuint(abs(asint({a})));"), // s_abs_i32
                _ => {
                    is_known = false;
                    format!("/* UNKNOWN sop1_0x{op:02x} */")
                }
            };
            format!("sop1_0x{op:02x} s{sdst}, s{ssrc0}")
        } else if (w0 >> 23) == 0x17E {
            // SOPC
            let op = (w0 >> 16) & 0x7F;
            let ssrc0 = w0 & 0xFF;
            let ssrc1 = (w0 >> 8) & 0xFF;
            let lit = if ssrc0 == 255 || ssrc1 == 255 {
                len_words = 2;
                Some(w1)
            } else {
                None
            };
            let a = fmt_src(ssrc0, lit, false);
            let b = fmt_src(ssrc1, lit, false);
            hlsl_stmt = match op {
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
                _ => {
                    is_known = false;
                    format!("/* UNKNOWN sopc_0x{op:02x} */")
                }
            };
            format!("sopc_0x{op:02x} s{ssrc0}, s{ssrc1}")
        } else if (w0 >> 28) == 0xB {
            // SOPK
            let op = (w0 >> 23) & 0x1F;
            let sdst = (w0 >> 16) & 0x7F;
            let simm = (w0 & 0xFFFF) as i16 as i32;
            let uimm = w0 & 0xFFFF;
            if op == 0x14 {
                len_words = 2; // s_setreg_imm32_b32
            }
            let dst = fmt_sdst(sdst);
            hlsl_stmt = match op {
                0x00 => format!("{dst} = asuint({simm});"), // s_movk_i32
                0x01 => "/* s_version */".into(),
                0x02 => format!("if (scc != 0u) {{ {dst} = asuint({simm}); }}"), // s_cmovk_i32
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
                0x0F => format!("{dst} = asuint(asint({dst}) + {simm});"), // s_addk_i32
                0x10 => format!("{dst} = asuint(asint({dst}) * {simm});"), // s_mulk_i32
                0x11 | 0x12 | 0x14 => format!("/* sopk_reg_0x{op:02x} */"),
                _ => {
                    is_known = false;
                    format!("/* UNKNOWN sopk_0x{op:02x} */")
                }
            };
            format!("sopk_0x{op:02x} s{sdst}, {simm}")
        } else if (w0 >> 26) == 0x3E {
            // EXP (64-bit), including compr=1 FP16-packed exports
            len_words = 2;
            is_export = true;
            let target = (w0 >> 4) & 0x3F;
            let en = w0 & 0xF;
            let compr = ((w0 >> 10) & 1) != 0;
            let vs = [w1 & 0xFF, (w1 >> 8) & 0xFF, (w1 >> 16) & 0xFF, (w1 >> 24) & 0xFF];
            if target == 20 {
                is_export_prim = true;
            }
            let val = if compr {
                format!(
                    "float4(f16tof32(v[{}] & 0xFFFFu), f16tof32(v[{}] >> 16), f16tof32(v[{}] & 0xFFFFu), f16tof32(v[{}] >> 16))",
                    vs[0], vs[0], vs[1], vs[1]
                )
            } else {
                format!(
                    "float4(asfloat(v[{}]), asfloat(v[{}]), asfloat(v[{}]), asfloat(v[{}]))",
                    vs[0], vs[1], vs[2], vs[3]
                )
            };
            match target {
                12..=15 => hlsl_stmt = format!("exp_pos0 = {val};"),
                0..=7 => hlsl_stmt = format!("exp_mrt[{target}] = {val};"),
                32..=63 => hlsl_stmt = format!("exp_param[{}] = {val};", target - 32),
                20 => hlsl_stmt = format!("/* exp_prim v[{},{},{}] */", vs[0], vs[1], vs[2]),
                9..=11 | 21 => hlsl_stmt = format!("/* exp_misc target={target} */"),
                _ => {
                    is_known = false;
                    hlsl_stmt = format!("/* UNKNOWN exp target={target} en=0x{en:x} */");
                }
            }
            format!("exp target={target} en=0x{en:x}")
        } else if (w0 >> 26) == 0x3D {
            // SMEM (64-bit)
            len_words = 2;
            let op = (w0 >> 18) & 0xFF;
            let sbase = (w0 & 0x3F) * 2;
            let sdata = (w0 >> 6) & 0x7F;
            let imm_off = (w1 & 0x001F_FFFF) / 4;
            let count = match op {
                0x00 | 0x08 => 1,
                0x01 | 0x09 => 2,
                0x02 | 0x0A => 4,
                0x03 | 0x0B => 8,
                0x04 | 0x0C => 16,
                0x20..=0x26 => 0, // s_dcache_inv / s_memtime
                _ => {
                    is_known = false;
                    1
                }
            };
            for i in 0..count {
                let dst = fmt_sdst(sdata + i);
                let _ = writeln!(
                    hlsl_stmt,
                    "{dst} = g_cbuf.Load({}u);",
                    (imm_off + i) * 4
                );
            }
            format!("s_load_x{count} s{sdata}, s[{sbase}:{}], 0x{:x}", sbase + 1, imm_off * 4)
        } else if (w0 >> 26) == 0x36 {
            // DS / LDS (64-bit)
            len_words = 2;
            uses_lds = true;
            let op = (w0 >> 17) & 0xFF;
            let addr = w1 & 0xFF;
            let data0 = (w1 >> 8) & 0xFF;
            let vdst = (w1 >> 24) & 0xFF;
            if op == 0x0D || op == 0x0E || op == 0x0F || op == 0x10 {
                hlsl_stmt = format!("g_lds[(v[{addr}] >> 2) & 4095u] = v[{data0}];");
                format!("ds_write_b32 v{addr}, v{data0}")
            } else {
                hlsl_stmt = format!("v[{vdst}] = g_lds[(v[{addr}] >> 2) & 4095u];");
                format!("ds_read_0x{op:02x} v{vdst}, v{addr}")
            }
        } else if (w0 >> 26) == 0x35 {
            // VOP3 (64-bit)
            len_words = 2;
            let op = (w0 >> 16) & 0x3FF;
            let vdst = w0 & 0xFF;
            let s0 = w1 & 0x1FF;
            let s1 = (w1 >> 9) & 0x1FF;
            let s2 = (w1 >> 18) & 0x1FF;
            match op {
                0x000..=0x0FF => {
                    let a_f = fmt_src(s0, None, true);
                    let b_f = fmt_src(s1, None, true);
                    let a_u = fmt_src(s0, None, false);
                    let b_u = fmt_src(s1, None, false);
                    let dst = fmt_sdst(vdst);
                    let cmp_op = op & 0x0F;
                    hlsl_stmt = if op >= 0xC0 {
                        let sym = match cmp_op { 1 => "<", 2 => "==", 3 => "<=", 4 => ">", 5 => "!=", 6 => ">=", _ => "==" };
                        format!("{dst} = ({a_u} {sym} {b_u}) ? 1u : 0u;")
                    } else if op >= 0x80 {
                        let sym = match cmp_op { 1 => "<", 2 => "==", 3 => "<=", 4 => ">", 5 => "!=", 6 => ">=", _ => "==" };
                        format!("{dst} = (asint({a_u}) {sym} asint({b_u})) ? 1u : 0u;")
                    } else {
                        let sym = match cmp_op { 1 => "<", 2 => "==", 3 => "<=", 4 => ">", 5 => "!=", 6 => ">=", _ => "==" };
                        format!("{dst} = ({a_f} {sym} {b_f}) ? 1u : 0u;")
                    };
                    format!("v_cmp_0x{op:03x} s{vdst}")
                }
                0x101 => {
                    hlsl_stmt = format!("v[{vdst}] = asuint(asfloat(v[{vdst}]) + {} * {});", fmt_src(s0, None, true), fmt_src(s1, None, true));
                    format!("v_dot2c_f32_f16 v{vdst}")
                }
                0x140 => {
                    // v_mad_legacy_f32
                    hlsl_stmt = format!(
                        "v[{vdst}] = asuint({} * {} + {});",
                        fmt_src(s0, None, true),
                        fmt_src(s1, None, true),
                        fmt_src(s2, None, true)
                    );
                    format!("v_mad_legacy_f32 v{vdst}")
                }
                0x141 | 0x16B | 0x17B => {
                    // v_mad_f32 (Scarlett) / v_fma_f32 / v_fmac_f32
                    hlsl_stmt = format!(
                        "v[{vdst}] = asuint({} * {} + {});",
                        fmt_src(s0, None, true),
                        fmt_src(s1, None, true),
                        fmt_src(s2, None, true)
                    );
                    format!("v_fma_f32 v{vdst}")
                }
                0x142 | 0x143 => {
                    // v_mad_u32_u24 / v_mad_i32_i24
                    hlsl_stmt = format!(
                        "v[{vdst}] = {} * {} + {};",
                        fmt_src(s0, None, false),
                        fmt_src(s1, None, false),
                        fmt_src(s2, None, false)
                    );
                    format!("v_mad_24 v{vdst}")
                }
                0x144 | 0x145 => {
                    // v_bfe_u32 / v_bfe_i32
                    hlsl_stmt = format!(
                        "v[{vdst}] = ({} >> ({} & 31u)) & ((1u << ({} & 31u)) - 1u);",
                        fmt_src(s0, None, false),
                        fmt_src(s1, None, false),
                        fmt_src(s2, None, false)
                    );
                    format!("v_bfe_u32 v{vdst}")
                }
                0x154..=0x158 => {
                    hlsl_stmt = format!(
                        "v[{vdst}] = ({} + {} + {});",
                        fmt_src(s0, None, false),
                        fmt_src(s1, None, false),
                        fmt_src(s2, None, false)
                    );
                    format!("v_vop3_0x{op:03x} v{vdst}")
                }
                0x300..=0x30F => {
                    hlsl_stmt = format!("v[{vdst}] = ({} & 0xFFFFu) | (({} & 0xFFFFu) << 16);", fmt_src(s0, None, false), fmt_src(s1, None, false));
                    format!("v_pack_0x{op:03x} v{vdst}")
                }
                0x342 => {
                    hlsl_stmt = format!("v[{vdst}] = ({} & {}) | {};", fmt_src(s0, None, false), fmt_src(s1, None, false), fmt_src(s2, None, false));
                    format!("v_and_or_b32 v{vdst}")
                }
                0x35A => {
                    hlsl_stmt = format!("v[{vdst}] = ({} << 16) | ({} & 0xFFFFu);", fmt_src(s0, None, false), fmt_src(s1, None, false));
                    format!("v_perm_b32 v{vdst}")
                }
                0x36F | 0x346 => {
                    hlsl_stmt = format!("v[{vdst}] = ({} << ({} & 31u)) + {};", fmt_src(s0, None, false), fmt_src(s1, None, false), fmt_src(s2, None, false));
                    format!("v_lshl_add_u32 v{vdst}")
                }
                0x377 | 0x378 => {
                    uses_wave = true;
                    hlsl_stmt = format!("v[{vdst}] = WaveReadLaneAt({}, {});", fmt_src(s0, None, false), fmt_src(s1, None, false));
                    format!("v_permlane_0x{op:03x} v{vdst}")
                }
                0x169 => {
                    // v_mul_lo_u32
                    hlsl_stmt = format!("v[{vdst}] = {} * {};", fmt_src(s0, None, false), fmt_src(s1, None, false));
                    format!("v_mul_lo_u32 v{vdst}")
                }
                0x361 => {
                    uses_wave = true;
                    hlsl_stmt = format!("/* v_writelane_b32 v{vdst} */");
                    format!("v_writelane_b32 v{vdst}")
                }
                0x36D => {
                    hlsl_stmt = format!(
                        "v[{vdst}] = {} + {} + {};",
                        fmt_src(s0, None, false),
                        fmt_src(s1, None, false),
                        fmt_src(s2, None, false)
                    );
                    format!("v_add3_u32 v{vdst}")
                }
                0x146 => {
                    // v_bfi_b32
                    hlsl_stmt = format!(
                        "v[{vdst}] = ({} & {}) | (~{} & {});",
                        fmt_src(s0, None, false),
                        fmt_src(s1, None, false),
                        fmt_src(s0, None, false),
                        fmt_src(s2, None, false)
                    );
                    format!("v_bfi_b32 v{vdst}")
                }
                0x148 => {
                    // v_min3_f32
                    hlsl_stmt = format!(
                        "v[{vdst}] = asuint(min(min({}, {}), {}));",
                        fmt_src(s0, None, true),
                        fmt_src(s1, None, true),
                        fmt_src(s2, None, true)
                    );
                    format!("v_min3_f32 v{vdst}")
                }
                0x14B => {
                    // v_max3_f32
                    hlsl_stmt = format!(
                        "v[{vdst}] = asuint(max(max({}, {}), {}));",
                        fmt_src(s0, None, true),
                        fmt_src(s1, None, true),
                        fmt_src(s2, None, true)
                    );
                    format!("v_max3_f32 v{vdst}")
                }
                0x100 => {
                    // v_cndmask_b32 (VOP3)
                    hlsl_stmt = format!(
                        "v[{vdst}] = (({} & 1u) != 0u) ? {} : {};",
                        fmt_src(s2, None, false),
                        fmt_src(s1, None, false),
                        fmt_src(s0, None, false)
                    );
                    format!("v_cndmask_b32 v{vdst}")
                }
                0x103 => {
                    hlsl_stmt = format!("v[{vdst}] = asuint({} + {});", fmt_src(s0, None, true), fmt_src(s1, None, true));
                    format!("v_add_f32 v{vdst}")
                }
                0x104 => {
                    hlsl_stmt = format!("v[{vdst}] = asuint({} - {});", fmt_src(s0, None, true), fmt_src(s1, None, true));
                    format!("v_sub_f32 v{vdst}")
                }
                0x108 => {
                    hlsl_stmt = format!("v[{vdst}] = asuint({} * {});", fmt_src(s0, None, true), fmt_src(s1, None, true));
                    format!("v_mul_f32 v{vdst}")
                }
                0x10F => {
                    hlsl_stmt = format!("v[{vdst}] = asuint(min({}, {}));", fmt_src(s0, None, true), fmt_src(s1, None, true));
                    format!("v_min_f32 v{vdst}")
                }
                0x110 => {
                    hlsl_stmt = format!("v[{vdst}] = asuint(max({}, {}));", fmt_src(s0, None, true), fmt_src(s1, None, true));
                    format!("v_max_f32 v{vdst}")
                }
                0x11B => {
                    hlsl_stmt = format!("v[{vdst}] = {} & {};", fmt_src(s0, None, false), fmt_src(s1, None, false));
                    format!("v_and_b32 v{vdst}")
                }
                0x11C => {
                    hlsl_stmt = format!("v[{vdst}] = {} | {};", fmt_src(s0, None, false), fmt_src(s1, None, false));
                    format!("v_or_b32 v{vdst}")
                }
                0x11D => {
                    hlsl_stmt = format!("v[{vdst}] = {} ^ {};", fmt_src(s0, None, false), fmt_src(s1, None, false));
                    format!("v_xor_b32 v{vdst}")
                }
                0x125 => {
                    hlsl_stmt = format!("v[{vdst}] = {} + {};", fmt_src(s0, None, false), fmt_src(s1, None, false));
                    format!("v_add_nc_u32 v{vdst}")
                }
                0x126 => {
                    hlsl_stmt = format!("v[{vdst}] = {} - {};", fmt_src(s0, None, false), fmt_src(s1, None, false));
                    format!("v_sub_nc_u32 v{vdst}")
                }
                0x127 => {
                    hlsl_stmt = format!("v[{vdst}] = {} - {};", fmt_src(s1, None, false), fmt_src(s0, None, false));
                    format!("v_subrev_nc_u32 v{vdst}")
                }
                0x147 => {
                    // v_alignbit_b32
                    hlsl_stmt = format!(
                        "v[{vdst}] = (({} << (32u - ({} & 31u))) | ({} >> ({} & 31u)));",
                        fmt_src(s0, None, false),
                        fmt_src(s2, None, false),
                        fmt_src(s1, None, false),
                        fmt_src(s2, None, false)
                    );
                    format!("v_alignbit_b32 v{vdst}")
                }
                0x170 | 0x177 => {
                    uses_wave = true;
                    hlsl_stmt = format!("v[{vdst}] = WaveReadLaneFirst({});", fmt_src(s0, None, false));
                    format!("v_wave_0x{op:03x} v{vdst}")
                }
                0x160 | 0x360 => {
                    uses_wave = true;
                    hlsl_stmt = format!(
                        "v[{vdst}] = WaveReadLaneAt({}, {});",
                        fmt_src(s0, None, false),
                        fmt_src(s1, None, false)
                    );
                    format!("v_readlane_b32 v{vdst}")
                }
                0x364 | 0x365 => {
                    uses_wave = true;
                    hlsl_stmt = format!("v[{vdst}] = WaveGetLaneIndex();");
                    format!("v_mbcnt_u32 v{vdst}")
                }
                _ => {
                    is_known = false;
                    hlsl_stmt = format!("/* UNKNOWN vop3_0x{op:03x} */");
                    format!("vop3_0x{op:03x} v{vdst}")
                }
            }
        } else if (w0 >> 26) == 0x3C {
            // MIMG (image_load / image_sample, 64-bit+)
            len_words = 2;
            let op = (w0 >> 18) & 0xFF;
            let vaddr = w1 & 0xFF;
            let vdata = (w1 >> 8) & 0xFF;
            let srsrc = ((w1 >> 16) & 0x1F) * 4;
            let ssamp = ((w1 >> 21) & 0x1F) * 4;
            let tex_idx = (srsrc / 4) & 7;
            let samp_idx = (ssamp / 4) & 3;
            if op == 0x66 || op == 0x67 {
                hlsl_stmt = format!("v[{vdata}] = asuint(1.0); /* image_bvh64_0x{op:02x} */");
            } else {
                hlsl_stmt = format!(
                    "{{ float4 _ts = g_tex[{tex_idx}].SampleLevel(g_samp[{samp_idx}], float2(asfloat(v[{vaddr}]), asfloat(v[{}])), 0.0); v[{vdata}] = asuint(_ts.x); v[{}] = asuint(_ts.y); }}",
                    (vaddr + 1) & 0xFF,
                    (vdata + 1) & 0xFF
                );
            }
            format!("mimg_0x{op:02x} v{vdata}, v{vaddr}, t#{tex_idx}, s#{samp_idx}")
        } else if (w0 >> 26) == 0x38 || (w0 >> 26) == 0x3A {
            // MUBUF (0x38) / MTBUF (0x3A) buffer & vertex loads/stores
            len_words = 2;
            let op = (w0 >> 18) & 0x7F;
            let off12 = w0 & 0xFFF;
            let vaddr = w1 & 0xFF;
            let vdata = (w1 >> 8) & 0xFF;
            if (w0 >> 26) == 0x38 && (0x18..=0x1B).contains(&op) {
                let words = (op - 0x18 + 1).min(4);
                for k in 0..words {
                    let _ = writeln!(
                        hlsl_stmt,
                        "g_out.Store((v[{vaddr}] + {}u) & 0xFFFCu, v[{}]);",
                        off12 + k * 4,
                        (vdata + k) & 0xFF
                    );
                }
                format!("buffer_store_x{words} v{vdata}, v{vaddr}, off={off12}")
            } else {
                hlsl_stmt = format!("v[{vdata}] = g_cbuf.Load((v[{vaddr}] + {off12}u) & 0xFFFCu);");
                format!("buf_load v{vdata}, v{vaddr}, off={off12}")
            }
        } else if (w0 >> 26) == 0x33 {
            // VOP3P (64-bit packed FP16/I16/dot/fma_mix)
            len_words = 2;
            let op = (w0 >> 16) & 0x7F;
            let vdst = w0 & 0xFF;
            let s0 = w1 & 0x1FF;
            let s1 = (w1 >> 9) & 0x1FF;
            let s2 = (w1 >> 18) & 0x1FF;
            match op {
                0x00..=0x1F | 0x20..=0x27 => {
                    // v_pk_* / v_dot* / v_fma_mix*
                    hlsl_stmt = format!(
                        "v[{vdst}] = asuint({} * {} + {});",
                        fmt_src(s0, None, true),
                        fmt_src(s1, None, true),
                        fmt_src(s2, None, true)
                    );
                    format!("vop3p_0x{op:02x} v{vdst}")
                }
                _ => {
                    is_known = false;
                    hlsl_stmt = format!("/* UNKNOWN vop3p_0x{op:02x} */");
                    format!("vop3p_0x{op:02x} v{vdst}")
                }
            }
        } else if (w0 >> 26) == 0x32 {
            // VINTRP (GFX10.3: 32-bit, 1 word)
            let op = (w0 >> 16) & 0x3;
            let vdst = (w0 >> 18) & 0xFF;
            let attr = (w0 >> 10) & 0x3F;
            let chan = (w0 >> 8) & 0x3;
            hlsl_stmt = format!("v[{vdst}] = asuint(p0[{chan}]); /* v_interp attr{attr}.{chan} */");
            format!("v_interp_{op} v{vdst}, attr{attr}.{chan}")
        } else if (w0 >> 26) == 0x34 || (w0 >> 26) == 0x37 {
            // VINTERP / FLAT / GLOBAL
            len_words = 2;
            let enc = w0 >> 26;
            if enc == 0x37 {
                let op = (w0 >> 18) & 0x7F;
                let vaddr = w1 & 0xFF;
                let vdata = (w1 >> 8) & 0xFF;
                let vdst = (w1 >> 24) & 0xFF;
                if (0x14..=0x17).contains(&op) {
                    hlsl_stmt = format!("v[{vdst}] = g_cbuf.Load(v[{vaddr}] & 0xFFFCu);");
                    format!("global_load_0x{op:02x} v{vdst}, v{vaddr}")
                } else if (0x1C..=0x1F).contains(&op) {
                    hlsl_stmt = format!("g_out.Store(v[{vaddr}] & 0xFFFCu, v[{vdata}]);");
                    format!("global_store_0x{op:02x} v{vaddr}, v{vdata}")
                } else {
                    is_known = false;
                    hlsl_stmt = format!("/* UNKNOWN flat_0x{op:02x} */");
                    format!("flat_0x{op:02x}")
                }
            } else {
                // VINTERP
                let op = (w0 >> 16) & 0x3;
                let vdst = (w0 >> 18) & 0xFF;
                let attr = (w0 >> 10) & 0x3F;
                let chan = (w0 >> 8) & 0x3;
                hlsl_stmt = format!("v[{vdst}] = asuint(p0[{chan}]); /* v_interp attr{attr}.{chan} */");
                format!("v_interp_{op} v{vdst}, attr{attr}.{chan}")
            }
        } else if (w0 >> 25) == 0x3F {
            // VOP1 (must be checked before VOP2 because bit 31 is 0)
            let op = (w0 >> 9) & 0xFF;
            let src0 = w0 & 0x1FF;
            let vdst = (w0 >> 17) & 0xFF;
            let lit = if matches!(src0, 249 | 250 | 255) {
                len_words = 2;
                if src0 == 255 { Some(w1) } else { None }
            } else {
                None
            };
            let a_f = fmt_src(src0, lit, true);
            let a_u = fmt_src(src0, lit, false);
            hlsl_stmt = match op {
                0x00 => "/* v_nop */".into(),
                0x01 => format!("v[{vdst}] = {a_u};"),                                   // v_mov_b32
                0x02 => {
                    uses_wave = true;
                    format!("_s_sink = WaveReadLaneFirst({a_u});") // v_readfirstlane_b32
                }
                0x05 => format!("v[{vdst}] = asuint((float)asint({a_u}));"),             // v_cvt_f32_i32
                0x06 => format!("v[{vdst}] = asuint((float){a_u});"),                    // v_cvt_f32_u32
                0x07 => format!("v[{vdst}] = (uint)(int){a_f};"),                        // v_cvt_u32_f32
                0x08 => format!("v[{vdst}] = asuint((int){a_f});"),                      // v_cvt_i32_f32
                0x0A => format!("v[{vdst}] = f32tof16({a_f});"),                         // v_cvt_f16_f32
                0x0B => format!("v[{vdst}] = asuint(f16tof32({a_u} & 0xFFFFu));"),       // v_cvt_f32_f16
                0x11 => format!("v[{vdst}] = asuint((float)asint({a_u}) * 0.0625);"),    // v_cvt_off_f32_i4
                0x12 => format!("v[{vdst}] = asuint((float)({a_u} & 0xFFu));"),          // v_cvt_f32_ubyte0
                0x13 => format!("v[{vdst}] = asuint((float)(({a_u} >> 8) & 0xFFu));"),   // v_cvt_f32_ubyte1
                0x14 => format!("v[{vdst}] = asuint((float)(({a_u} >> 16) & 0xFFu));"),  // v_cvt_f32_ubyte2
                0x15 => format!("v[{vdst}] = asuint((float)(({a_u} >> 24) & 0xFFu));"),  // v_cvt_f32_ubyte3
                0x1F | 0x20 => format!("v[{vdst}] = asuint(frac({a_f}));"),             // v_fract_f32 / v_trunc_f32
                0x21 => format!("v[{vdst}] = asuint(trunc({a_f}));"),                    // v_trunc_f32
                0x22 => format!("v[{vdst}] = asuint(ceil({a_f}));"),                     // v_ceil_f32
                0x23 => format!("v[{vdst}] = asuint(round({a_f}));"),                    // v_rndne_f32
                0x24 => format!("v[{vdst}] = asuint(floor({a_f}));"),                    // v_floor_f32
                0x25 => format!("v[{vdst}] = asuint(exp2({a_f}));"),                     // v_exp_f32
                0x27 => format!("v[{vdst}] = asuint(log2(max({a_f}, 1e-12)));"),         // v_log_f32
                0x2A => format!("v[{vdst}] = asuint(rcp({a_f}));"),                      // v_rcp_f32
                0x2E => format!("v[{vdst}] = asuint(rsqrt(max({a_f}, 1e-12)));"),        // v_rsq_f32
                0x33 => format!("v[{vdst}] = asuint(sqrt(max({a_f}, 0.0)));"),           // v_sqrt_f32
                0x35 => format!("v[{vdst}] = asuint(sin({a_f} * 6.2831853));"),          // v_sin_f32
                0x36 => format!("v[{vdst}] = asuint(cos({a_f} * 6.2831853));"),          // v_cos_f32
                0x37 => format!("v[{vdst}] = ~{a_u};"),                                  // v_not_b32
                0x38 => format!("v[{vdst}] = reversebits({a_u});"),                      // v_bfrev_b32
                0x39 => format!("v[{vdst}] = firstbithigh({a_u});"),                     // v_ffbh_u32
                0x3A => format!("v[{vdst}] = firstbitlow({a_u});"),                      // v_ffbl_b32
                _ => {
                    is_known = false;
                    format!("/* UNKNOWN vop1_0x{op:02x} */")
                }
            };
            format!("vop1_0x{op:02x} v{vdst}, src{src0}")
        } else if (w0 >> 25) == 0x3E {
            // VOPC (must be checked before VOP2 because bit 31 is 0)
            let op = (w0 >> 17) & 0xFF;
            let src0 = w0 & 0x1FF;
            let vsrc1 = (w0 >> 9) & 0xFF;
            let lit = if matches!(src0, 249 | 250 | 255) {
                len_words = 2;
                if src0 == 255 { Some(w1) } else { None }
            } else {
                None
            };
            let a_f = fmt_src(src0, lit, true);
            let b_f = format!("asfloat(v[{vsrc1}])");
            let a_u = fmt_src(src0, lit, false);
            let b_u = format!("v[{vsrc1}]");
            hlsl_stmt = match op & 0x0F {
                0x00 | 0x08 => "vcc = 0u;".into(),
                0x07 | 0x0F => "vcc = 1u;".into(),
                0x01 | 0x09 if op < 0x80 => format!("vcc = ({a_f} < {b_f}) ? 1u : 0u;"),
                0x02 | 0x0A if op < 0x80 => format!("vcc = ({a_f} == {b_f}) ? 1u : 0u;"),
                0x03 | 0x0B if op < 0x80 => format!("vcc = ({a_f} <= {b_f}) ? 1u : 0u;"),
                0x04 | 0x0C if op < 0x80 => format!("vcc = ({a_f} > {b_f}) ? 1u : 0u;"),
                0x05 | 0x0D if op < 0x80 => format!("vcc = ({a_f} != {b_f}) ? 1u : 0u;"),
                0x06 | 0x0E if op < 0x80 => format!("vcc = ({a_f} >= {b_f}) ? 1u : 0u;"),
                0x01..=0x0E if (0x80..0xC0).contains(&op) => {
                    let cmp = match op & 0x07 {
                        0x01 => "<", 0x02 => "==", 0x03 => "<=", 0x04 => ">", 0x05 => "!=", _ => ">=",
                    };
                    format!("vcc = (asint({a_u}) {cmp} asint({b_u})) ? 1u : 0u;")
                }
                0x01..=0x0E if op >= 0xC0 => {
                    let cmp = match op & 0x07 {
                        0x01 => "<", 0x02 => "==", 0x03 => "<=", 0x04 => ">", 0x05 => "!=", _ => ">=",
                    };
                    format!("vcc = ({a_u} {cmp} {b_u}) ? 1u : 0u;")
                }
                _ => {
                    is_known = false;
                    format!("/* UNKNOWN vopc_0x{op:02x} */")
                }
            };
            format!("vopc_0x{op:02x} src{src0}, v{vsrc1}")
        } else if (w0 >> 31) == 0 {
            // VOP2
            let op = (w0 >> 25) & 0x3F;
            let src0 = w0 & 0x1FF;
            let vsrc1 = (w0 >> 9) & 0xFF;
            let vdst = (w0 >> 17) & 0xFF;
            let always_lit = matches!(op, 0x20 | 0x21 | 0x2C | 0x2D | 0x37 | 0x38);
            let lit = if always_lit || matches!(src0, 249 | 250 | 255) {
                len_words = 2;
                if always_lit || src0 == 255 { Some(w1) } else { None }
            } else {
                None
            };
            let a_f = fmt_src(src0, lit, true);
            let b_f = format!("asfloat(v[{vsrc1}])");
            let a_u = fmt_src(src0, lit, false);
            let b_u = format!("v[{vsrc1}]");
            let imm_f = format!("asfloat(0x{:08x}u)", w1);
            hlsl_stmt = match op {
                0x00 => format!("v[{vdst}] = ((vcc & 1u) != 0u) ? {b_u} : {a_u};"),      // v_cndmask_b32
                0x01 | 0x02 => format!("v[{vdst}] = asuint(asfloat(v[{vdst}]) + {a_f} * {b_f});"), // v_dot2c_f32_f16 / v_dot2c_f16
                0x03 => format!("v[{vdst}] = asuint({a_f} + {b_f});"),                   // v_add_f32
                0x04 => format!("v[{vdst}] = asuint({a_f} - {b_f});"),                   // v_sub_f32
                0x05 => format!("v[{vdst}] = asuint({b_f} - {a_f});"),                   // v_subrev_f32
                0x06 => format!("v[{vdst}] = asuint(asfloat(v[{vdst}]) + {a_f} * {b_f});"), // v_fmac_legacy_f32
                0x07 | 0x08 => format!("v[{vdst}] = asuint({a_f} * {b_f});"),            // v_mul_legacy_f32 / v_mul_f32
                0x09..=0x0C => format!("v[{vdst}] = {a_u} * {b_u};"),                    // v_mul_i32_i24 / u32_u24
                0x0F => format!("v[{vdst}] = asuint(min({a_f}, {b_f}));"),               // v_min_f32
                0x10 => format!("v[{vdst}] = asuint(max({a_f}, {b_f}));"),               // v_max_f32
                0x11 => format!("v[{vdst}] = asuint(min(asint({a_u}), asint({b_u})));"), // v_min_i32
                0x12 => format!("v[{vdst}] = asuint(max(asint({a_u}), asint({b_u})));"), // v_max_i32
                0x13 => format!("v[{vdst}] = min({a_u}, {b_u});"),                       // v_min_u32
                0x14 => format!("v[{vdst}] = max({a_u}, {b_u});"),                       // v_max_u32
                0x16 => format!("v[{vdst}] = {b_u} >> ({a_u} & 31u);"),                  // v_lshrrev_b32
                0x18 => format!("v[{vdst}] = asuint(asint({b_u}) >> ({a_u} & 31u));"),   // v_ashrrev_i32
                0x1A => format!("v[{vdst}] = {b_u} << ({a_u} & 31u);"),                  // v_lshlrev_b32
                0x1B => format!("v[{vdst}] = {a_u} & {b_u};"),                           // v_and_b32
                0x1C => format!("v[{vdst}] = {a_u} | {b_u};"),                           // v_or_b32
                0x1D => format!("v[{vdst}] = {a_u} ^ {b_u};"),                           // v_xor_b32
                0x1E => format!("v[{vdst}] = ~({a_u} ^ {b_u});"),                        // v_xnor_b32
                0x1F | 0x2B => format!("v[{vdst}] = asuint(asfloat(v[{vdst}]) + {a_f} * {b_f});"), // v_mac_f32 (Scarlett) / v_fmac_f32
                0x20 | 0x2C => format!("v[{vdst}] = asuint({a_f} * {imm_f} + {b_f});"),  // v_madmk_f32 (Scarlett) / v_fmamk_f32
                0x21 | 0x2D => format!("v[{vdst}] = asuint({a_f} * {b_f} + {imm_f});"),  // v_madak_f32 (Scarlett) / v_fmaak_f32
                0x25 => format!("v[{vdst}] = {a_u} + {b_u};"),                           // v_add_nc_u32
                0x26 => format!("v[{vdst}] = {a_u} - {b_u};"),                           // v_sub_nc_u32
                0x27 => format!("v[{vdst}] = {b_u} - {a_u};"),                           // v_subrev_nc_u32
                0x2A => format!("v[{vdst}] = asuint(asfloat(v[{vdst}]) + {a_f} * {b_f});"), // v_fmac_legacy_f32
                0x2F => format!("v[{vdst}] = (f32tof16({a_f}) & 0xFFFFu) | ((f32tof16({b_f}) & 0xFFFFu) << 16);"), // v_cvt_pkrtz_f16_f32
                0x32 => format!("v[{vdst}] = f32tof16(f16tof32({a_u} & 0xFFFFu) + f16tof32({b_u} & 0xFFFFu));"), // v_add_f16
                0x33 => format!("v[{vdst}] = f32tof16(f16tof32({a_u} & 0xFFFFu) - f16tof32({b_u} & 0xFFFFu));"), // v_sub_f16
                0x34 => format!("v[{vdst}] = f32tof16(f16tof32({b_u} & 0xFFFFu) - f16tof32({a_u} & 0xFFFFu));"), // v_subrev_f16
                0x35 => format!("v[{vdst}] = f32tof16(f16tof32({a_u} & 0xFFFFu) * f16tof32({b_u} & 0xFFFFu));"), // v_mul_f16
                0x36 => format!("v[{vdst}] = f32tof16(f16tof32(v[{vdst}] & 0xFFFFu) + f16tof32({a_u} & 0xFFFFu) * f16tof32({b_u} & 0xFFFFu));"), // v_fmac_f16
                0x37 => format!("v[{vdst}] = f32tof16(f16tof32({a_u} & 0xFFFFu) * f16tof32({w1}u & 0xFFFFu) + f16tof32({b_u} & 0xFFFFu));"), // v_fmamk_f16
                0x38 => format!("v[{vdst}] = f32tof16(f16tof32({a_u} & 0xFFFFu) * f16tof32({b_u} & 0xFFFFu) + f16tof32({w1}u & 0xFFFFu));"), // v_fmaak_f16
                0x39 => format!("v[{vdst}] = f32tof16(max(f16tof32({a_u} & 0xFFFFu), f16tof32({b_u} & 0xFFFFu)));"), // v_max_f16
                0x3A => format!("v[{vdst}] = f32tof16(min(f16tof32({a_u} & 0xFFFFu), f16tof32({b_u} & 0xFFFFu)));"), // v_min_f16
                0x3B => format!("v[{vdst}] = f32tof16(ldexp(f16tof32({a_u} & 0xFFFFu), asint({b_u})));"), // v_ldexp_f16
                _ => {
                    is_known = false;
                    format!("/* UNKNOWN vop2_0x{op:02x} */")
                }
            };
            format!("vop2_0x{op:02x} v{vdst}, src{src0}, v{vsrc1}")
        } else if (w0 >> 30) == 2 {
            // SOP2
            let op = (w0 >> 23) & 0x7F;
            let ssrc0 = w0 & 0xFF;
            let ssrc1 = (w0 >> 8) & 0xFF;
            let sdst = (w0 >> 16) & 0x7F;
            let lit = if ssrc0 == 255 || ssrc1 == 255 {
                len_words = 2;
                Some(w1)
            } else {
                None
            };
            let a = fmt_src(ssrc0, lit, false);
            let b = fmt_src(ssrc1, lit, false);
            let dst = fmt_sdst(sdst);
            hlsl_stmt = match op {
                0x00 | 0x02 => format!("{dst} = {a} + {b}; scc = ({dst} < {a}) ? 1u : 0u;"),
                0x01 | 0x03 => format!("{dst} = {a} - {b}; scc = ({a} < {b}) ? 1u : 0u;"),
                0x04 => format!("{dst} = {a} + {b} + (scc & 1u);"),
                0x05 => format!("{dst} = {a} - {b} - (scc & 1u);"),
                0x06 => format!("{dst} = asuint(min(asint({a}), asint({b})));"),
                0x07 => format!("{dst} = min({a}, {b});"),
                0x08 => format!("{dst} = asuint(max(asint({a}), asint({b})));"),
                0x09 => format!("{dst} = max({a}, {b});"),
                0x0A | 0x0B => format!("{dst} = (scc != 0u) ? {a} : {b};"),
                0x34..=0x37 => format!("{dst} = ({a} << {}) + {b};", op - 0x33),
                0x0E | 0x0F => format!("{dst} = {a} & {b}; scc = ({dst} != 0u) ? 1u : 0u;"),
                0x10 | 0x11 => format!("{dst} = {a} | {b}; scc = ({dst} != 0u) ? 1u : 0u;"),
                0x12 | 0x13 => format!("{dst} = {a} ^ {b}; scc = ({dst} != 0u) ? 1u : 0u;"),
                0x14 | 0x15 => format!("{dst} = {a} & ~{b}; scc = ({dst} != 0u) ? 1u : 0u;"),
                0x16 | 0x17 => format!("{dst} = ~({a} & {b}); scc = ({dst} != 0u) ? 1u : 0u;"),
                0x18 | 0x19 => format!("{dst} = ~({a} | {b}); scc = ({dst} != 0u) ? 1u : 0u;"),
                0x1A | 0x1B => format!("{dst} = ~({a} ^ {b}); scc = ({dst} != 0u) ? 1u : 0u;"),
                0x1C | 0x1D => format!("{dst} = {a} << ({b} & 31u);"),
                0x1E | 0x1F => format!("{dst} = {a} >> ({b} & 31u);"),
                0x20 => format!("{dst} = asuint(asint({a}) >> ({b} & 31u));"),
                0x24 | 0x25 => format!("{dst} = ({a} >> ({b} & 31u)) & ((1u << (({b} >> 16) & 31u)) - 1u);"),
                0x26 => format!("{dst} = ({a} & 0xFFFFu) | (({b} & 0xFFFFu) << 16);"),
                0x27 => format!("{dst} = ({a} & 0xFFFFu) | ({b} & 0xFFFF0000u);"),
                0x28 => format!("{dst} = ({a} >> 16) | ({b} & 0xFFFF0000u);"),
                0x2C => format!("{dst} = asuint(asint({a}) * asint({b}));"),
                0x31 => format!("{dst} = (uint)abs(asint({a}) - asint({b})); scc = ({dst} != 0u) ? 1u : 0u;"),
                _ => {
                    is_known = false;
                    format!("/* UNKNOWN sop2_0x{op:02x} */")
                }
            };
            format!("sop2_0x{op:02x} s{sdst}, s{ssrc0}, s{ssrc1}")
        } else {
            is_known = false;
            hlsl_stmt = format!("/* UNKNOWN raw_0x{w0:08x} */");
            format!("raw_0x{w0:08x}")
        };

        ops.push(DecodedOp {
            offset: off,
            word0: w0,
            word1: w1,
            len_words,
            mnemonic,
            hlsl_stmt,
            is_known,
            is_endpgm,
            is_export,
            is_export_prim,
            uses_lds,
            uses_wave,
            branch_target,
            branch_cond,
        });
        off += len_words * 4;
        if is_endpgm {
            // Stop at s_code_end or trailing padding, but keep decoding if non-padding code follows an early-return s_endpgm
            let tail = &bytes[off..];
            let only_pad = tail.chunks_exact(4).all(|c| {
                let w = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
                w == 0 || w == 0xBF9F_0000
            });
            if only_pad {
                break;
            }
        }
    }
    ops
}

pub fn detect_stage(ops: &[DecodedOp], hint: ShaderStage) -> ShaderStage {
    if ops.iter().any(|op| op.is_export_prim) {
        return ShaderStage::Mesh;
    }
    if ops.iter().any(|op| op.mnemonic.starts_with("mimg_0x66") || op.mnemonic.starts_with("mimg_0x67")) {
        return ShaderStage::RayTracing;
    }
    if ops.iter().any(|op| op.is_export && (op.hlsl_stmt.contains("exp_pos0") || op.hlsl_stmt.contains("exp_param"))) {
        return ShaderStage::Vertex;
    }
    if ops.iter().any(|op| op.is_export && op.hlsl_stmt.contains("exp_mrt")) {
        return ShaderStage::Pixel;
    }
    hint
}

/// Scans a binary blob for embedded `FSSM` shader containers and returns their code slices.
pub fn extract_fssm_shaders(blob: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 16 <= blob.len() {
        if &blob[i..i + 4] == b"FSSM" {
            let sz = u32::from_le_bytes([blob[i + 4], blob[i + 5], blob[i + 6], blob[i + 7]]) as usize;
            if sz >= 16 && i + sz <= blob.len() {
                out.push(&blob[i + 16..i + sz]);
                i += sz;
                continue;
            }
        }
        i += 4;
    }
    out
}

/// Lifts an RDNA2 instruction stream into per-instruction compilable HLSL.
pub fn lift_to_hlsl(bytes: &[u8], hint: ShaderStage) -> (ShaderStage, String, usize) {
    let ops = decode_rdna2(bytes);
    lift_to_hlsl_from_ops(&ops, hint)
}

/// Strictly lifts an RDNA2 instruction stream: fails if any instruction is unrecognized.
pub fn lift_to_hlsl_strict(bytes: &[u8], hint: ShaderStage) -> Result<(ShaderStage, String, usize), String> {
    let ops = decode_rdna2(bytes);
    if ops.is_empty() {
        return Err("empty RDNA2 instruction stream".into());
    }
    let unknown: Vec<&str> = ops
        .iter()
        .filter(|o| !o.is_known)
        .map(|o| o.mnemonic.as_str())
        .collect();
    if !unknown.is_empty() {
        let sample = unknown.iter().take(4).copied().collect::<Vec<_>>().join(", ");
        return Err(format!("{} unknown RDNA2 insns: {sample}", unknown.len()));
    }
    Ok(lift_to_hlsl_from_ops(&ops, hint))
}

pub fn lift_to_hlsl_from_ops(ops: &[DecodedOp], hint: ShaderStage) -> (ShaderStage, String, usize) {
    let stage = detect_stage(ops, hint);
    let insn_count = ops.len();
    let export_count = ops.iter().filter(|o| o.is_export).count();
    let uses_lds = ops.iter().any(|o| o.uses_lds) && matches!(stage, ShaderStage::Compute | ShaderStage::Mesh);

    let mut hlsl = String::with_capacity(2048 + insn_count * 48);
    let _ = writeln!(
        hlsl,
        "// PercyScarlett RDNA2 -> HLSL ({}, {} insns, {} exports)",
        stage.as_str(),
        insn_count,
        export_count
    );
    hlsl.push_str("cbuffer PerDrawCB : register(b0) { uint4 g_cb0[64]; };\n");
    hlsl.push_str("ByteAddressBuffer g_cbuf : register(t0);\n");
    hlsl.push_str("Texture2D<float4> g_tex[8] : register(t1);\n");
    hlsl.push_str("SamplerState g_samp[4] : register(s0);\n");
    if stage == ShaderStage::Compute {
        hlsl.push_str("RWByteAddressBuffer g_out : register(u0);\n");
    }
    if uses_lds {
        hlsl.push_str("groupshared uint g_lds[4096];\n");
    }
    hlsl.push('\n');

    let mut body = String::with_capacity(1024 + insn_count * 40);
    body.push_str("    uint s[106]; uint v[256]; uint vcc = 0u; uint scc = 0u; uint exec_lo = 0xFFFFFFFFu; uint exec_hi = 0u; uint _s_sink = 0u;\n");
    body.push_str("    [unroll] for (uint _i = 0u; _i < 16u; ++_i) { s[_i] = 0u; }\n");
    body.push_str("    [unroll] for (uint _j = 0u; _j < 32u; ++_j) { v[_j] = 0u; }\n");
    if stage == ShaderStage::Compute {
        body.push_str("    v[0] = tid.x;\n");
    } else if stage == ShaderStage::Vertex {
        body.push_str("    v[0] = vid;\n");
    }
    body.push_str("    float4 exp_pos0 = float4(0.0, 0.0, 0.0, 1.0);\n");
    body.push_str("    float4 exp_mrt[8] = { float4(0,0,0,1), float4(0,0,0,1), float4(0,0,0,1), float4(0,0,0,1), float4(0,0,0,1), float4(0,0,0,1), float4(0,0,0,1), float4(0,0,0,1) };\n");
    body.push_str("    float4 exp_param[32]; [unroll] for (uint _p = 0u; _p < 32u; ++_p) { exp_param[_p] = float4(0,0,0,0); }\n");

    let has_branches = ops.iter().any(|o| o.branch_target.is_some());
    if !has_branches {
        for op in ops {
            if op.uses_lds && !uses_lds {
                continue;
            }
            if !op.hlsl_stmt.is_empty() {
                for line in op.hlsl_stmt.lines() {
                    let _ = writeln!(body, "    {line}");
                }
            }
        }
    } else {
        // Structured CFG state-machine for real RDNA2 loops & conditional branches (bounded for DXC safety)
        body.push_str("    uint _pc = 0u; uint _iters = 0u;\n");
        body.push_str("    while (_pc != 0xFFFFFFFFu && _iters < 256u) {\n        _iters++;\n        switch (_pc) {\n");
        for (idx, op) in ops.iter().enumerate() {
            let _ = writeln!(body, "        case {}u: {{", op.offset);
            if !(op.uses_lds && !uses_lds) && !op.hlsl_stmt.is_empty() {
                for line in op.hlsl_stmt.lines() {
                    let _ = writeln!(body, "            {line}");
                }
            }
            let next_pc = ops.get(idx + 1).map(|n| n.offset as u32).unwrap_or(0xFFFFFFFFu32);
            if op.is_endpgm {
                body.push_str("            _pc = 0xFFFFFFFFu; break;\n");
            } else if let Some(tgt) = op.branch_target {
                let valid_tgt = if ops.iter().any(|o| o.offset == tgt) { tgt as u32 } else { next_pc };
                if let Some(cond) = op.branch_cond {
                    let _ = writeln!(body, "            _pc = {cond} ? {valid_tgt}u : {next_pc}u; break;");
                } else {
                    let _ = writeln!(body, "            _pc = {valid_tgt}u; break;");
                }
            } else {
                let _ = writeln!(body, "            _pc = {next_pc}u; break;");
            }
            body.push_str("        }\n");
        }
        body.push_str("        default: _pc = 0xFFFFFFFFu; break;\n        }\n    }\n");
    }

    match stage {
        ShaderStage::Vertex => {
            hlsl.push_str(
                "struct VSOut { float4 pos : SV_POSITION; float4 p0 : TEXCOORD0; };\n\
                 VSOut main(uint vid : SV_VertexID) {\n",
            );
            hlsl.push_str(&body);
            hlsl.push_str(
                "    VSOut o;\n\
                 \to.pos = exp_pos0;\n\
                 \to.p0 = exp_param[0];\n\
                 \treturn o;\n\
                 }\n",
            );
        }
        ShaderStage::Pixel => {
            hlsl.push_str("float4 main(float4 pos : SV_POSITION, float4 p0 : TEXCOORD0) : SV_TARGET {\n");
            hlsl.push_str(&body);
            hlsl.push_str("    return exp_mrt[0] + p0 * 0.0;\n}\n");
        }
        ShaderStage::Compute => {
            hlsl.push_str("[numthreads(64, 1, 1)]\nvoid main(uint3 tid : SV_DispatchThreadID) {\n");
            hlsl.push_str(&body);
            hlsl.push_str("    if (tid.x == 0xFFFFFFFEu) { g_out.Store(0, v[0]); }\n}\n");
        }
        ShaderStage::Mesh => {
            hlsl.push_str(
                "struct MSOut { float4 pos : SV_POSITION; };\n\
                 [outputtopology(\"triangle\")]\n\
                 [numthreads(32, 1, 1)]\n\
                 void main(uint gtid : SV_GroupThreadID, out vertices MSOut verts[3], out indices uint3 tris[1]) {\n",
            );
            hlsl.push_str(&body);
            hlsl.push_str(
                "    SetMeshOutputCounts(3, 1);\n\
                 \tif (gtid < 3) { verts[gtid].pos = exp_pos0; }\n\
                 \tif (gtid == 0) { tris[0] = uint3(0, 1, 2); }\n\
                 }\n",
            );
        }
        ShaderStage::Geometry => {
            hlsl.push_str(
                "struct GSOut { float4 pos : SV_POSITION; };\n\
                 [maxvertexcount(3)]\n\
                 void main(triangle float4 inp[3] : SV_POSITION, inout TriangleStream<GSOut> stream) {\n",
            );
            hlsl.push_str(&body);
            hlsl.push_str(
                "    GSOut o;\n\
                 \t[unroll] for (uint i = 0u; i < 3u; ++i) { o.pos = inp[i] + exp_pos0 * 0.0; stream.Append(o); }\n\
                 }\n",
            );
        }
        ShaderStage::RayTracing => {
            hlsl.push_str("RWByteAddressBuffer g_rt_out : register(u0);\n[shader(\"raygeneration\")]\nvoid main() {\n");
            hlsl.push_str(&body);
            hlsl.push_str("    if (v[0] == 0xFFFFFFFEu) { g_rt_out.Store(0, v[1]); }\n}\n");
        }
    }

    (stage, hlsl, insn_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_rdna2_alu_smem_lds_and_detects_mesh_stage() {
        let mut code = Vec::new();
        // s_load_x2 s0, s[2:3], 0x10
        code.extend_from_slice(&((0x3Du32 << 26) | (0x01u32 << 18) | 1u32).to_le_bytes());
        code.extend_from_slice(&0x10u32.to_le_bytes());
        // v_mov_b32 v1, 1.0 (src0=242)
        code.extend_from_slice(&((0x3Fu32 << 25) | (1u32 << 17) | (0x01u32 << 9) | 242u32).to_le_bytes());
        // v_add_f32 v2, v1, v0 -> VOP2 op=3, vdst=2, vsrc1=0, src0=256+1
        code.extend_from_slice(&((0x03u32 << 25) | (2u32 << 17) | (0u32 << 9) | 257u32).to_le_bytes());
        // exp target=20 (prim)
        code.extend_from_slice(&((0x3Eu32 << 26) | (20u32 << 4) | 0xF).to_le_bytes());
        code.extend_from_slice(&0u32.to_le_bytes());
        // s_endpgm
        code.extend_from_slice(&0xBF81_0000u32.to_le_bytes());

        let (stage, hlsl, count) = lift_to_hlsl(&code, ShaderStage::Vertex);
        assert_eq!(stage, ShaderStage::Mesh);
        assert_eq!(count, 5);
        assert!(hlsl.contains("g_cbuf.Load(16u)"));
        assert!(hlsl.contains("v[1] = 0x3F800000u;"));
        assert!(hlsl.contains("asfloat(v[1]) + asfloat(v[0])"));
        assert!(hlsl.contains("[outputtopology(\"triangle\")]"));
    }

    #[test]
    fn decodes_sopp_branches_and_wave64_intrinsics() {
        let mut code = Vec::new();
        // s_bcnt1_i32_b64 s0, exec -> SOP1 op=0x0B, sdst=0, ssrc0=126
        code.extend_from_slice(&((0x17Du32 << 23) | (0x0Bu32 << 8) | 126u32).to_le_bytes());
        // s_cbranch_scc0 +1 (skip next insn) -> SOPP op=0x04, imm=1
        code.extend_from_slice(&(0xBF80_0000u32 | (0x04u32 << 16) | 1u32).to_le_bytes());
        // v_mov_b32 v3, 0
        code.extend_from_slice(&((0x3Fu32 << 25) | (3u32 << 17) | (0x01u32 << 9) | 128u32).to_le_bytes());
        // s_code_end (0xBF9F0000)
        code.extend_from_slice(&0xBF9F_0000u32.to_le_bytes());

        let (stage, hlsl, count) = lift_to_hlsl(&code, ShaderStage::Compute);
        assert_eq!(stage, ShaderStage::Compute);
        assert_eq!(count, 4);
        assert!(hlsl.contains("countbits(exec_lo) + countbits(exec_hi)"));
        assert!(hlsl.contains("switch (_pc)"));
        assert!(hlsl.contains("(scc == 0u)"));
        assert!(hlsl.contains("// s_code_end"));
    }

    #[test]
    fn decodes_mubuf_compressed_exp_and_fssm_containers() {
        let mut code = Vec::new();
        // MUBUF (0x38) offset=16, vaddr=1, vdata=2
        code.extend_from_slice(&((0x38u32 << 26) | 16u32).to_le_bytes());
        code.extend_from_slice(&((2u32 << 8) | 1u32).to_le_bytes());
        // EXP target=0 (MRT0), compr=1 (bit 10), vs=[2, 3, 0, 0]
        code.extend_from_slice(&((0x3Eu32 << 26) | (1u32 << 10) | 0xFu32).to_le_bytes());
        code.extend_from_slice(&((3u32 << 8) | 2u32).to_le_bytes());
        code.extend_from_slice(&0xBF81_0000u32.to_le_bytes());

        let (stage, hlsl, count) = lift_to_hlsl(&code, ShaderStage::Pixel);
        assert_eq!(stage, ShaderStage::Pixel);
        assert_eq!(count, 3);
        assert!(hlsl.contains("g_cbuf.Load((v[1] + 16u) & 0xFFFCu)"));
        assert!(hlsl.contains("f16tof32(v[2] & 0xFFFFu)"));

        let mut fssm = Vec::new();
        fssm.extend_from_slice(b"FSSM");
        fssm.extend_from_slice(&((16 + code.len()) as u32).to_le_bytes());
        fssm.extend_from_slice(&[0u8; 8]);
        fssm.extend_from_slice(&code);
        let chunks = extract_fssm_shaders(&fssm);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], code.as_slice());
    }

    #[test]
    fn strict_lift_rejects_unknown_opcodes() {
        let mut bad = Vec::new();
        // VOP2 unknown opcode 0x3E (bit 31 = 0, op = 0x3E -> not VOP1/VOPC)
        bad.extend_from_slice(&((0x2Eu32 << 25) | (1u32 << 17) | (0u32 << 9) | 256u32).to_le_bytes());
        bad.extend_from_slice(&0xBF81_0000u32.to_le_bytes());
        let err = lift_to_hlsl_strict(&bad, ShaderStage::Compute).unwrap_err();
        assert!(err.contains("unknown RDNA2 insns"), "unexpected err: {err}");
        assert!(err.contains("vop2_0x2e"), "unexpected err: {err}");
    }
}
