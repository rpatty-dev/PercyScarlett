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

/// Decodes an RDNA2 (GFX10.3) instruction stream into per-instruction ops + HLSL statements.
pub fn decode_rdna2(bytes: &[u8]) -> Vec<DecodedOp> {
    let mut ops = Vec::with_capacity(bytes.len() / 4);
    let mut off = 0usize;
    while off + 4 <= bytes.len() {
        let w0 = read_u32(bytes, off).unwrap();
        let w1 = read_u32(bytes, off + 4).unwrap_or(0);
        let mut len_words = 1usize;
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
                0x0C => {
                    hlsl_stmt = "/* s_waitcnt */".into();
                    format!("s_waitcnt 0x{:04x}", w0 & 0xFFFF)
                }
                _ => format!("sopp_0x{op:02x} {imm}"),
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
                0x04 => format!("{dst} = {a}; s[{sdst} + 1] = 0u;"), // s_mov_b64
                0x06 => format!("{dst} = ~{a}; scc = ({dst} != 0u) ? 1u : 0u;"), // s_not_b32
                0x0A => {
                    uses_wave = true;
                    format!("{dst} = countbits({a}); scc = ({dst} != 0u) ? 1u : 0u;") // s_bcnt1_i32_b32
                }
                0x0B => {
                    uses_wave = true;
                    format!("{dst} = countbits(exec_lo) + countbits(exec_hi); scc = ({dst} != 0u) ? 1u : 0u;") // s_bcnt1_i32_b64
                }
                0x0F => format!("{dst} = firstbitlow({a});"), // s_ff1_i32_b32
                0x13 => format!("{dst} = firstbithigh({a});"), // s_flbit_i32_b32
                0x24 => {
                    uses_wave = true;
                    format!("{dst} = exec_lo; exec_lo &= {a}; scc = (exec_lo != 0u) ? 1u : 0u;") // s_and_saveexec_b64
                }
                _ => format!("{dst} = {a}; /* sop1_0x{op:02x} */"),
            };
            format!("sop1_0x{op:02x} s{sdst}, s{ssrc0}")
        } else if (w0 >> 26) == 0x3E {
            // EXP (64-bit)
            len_words = 2;
            is_export = true;
            let target = (w0 >> 4) & 0x3F;
            let en = w0 & 0xF;
            let vs = [w1 & 0xFF, (w1 >> 8) & 0xFF, (w1 >> 16) & 0xFF, (w1 >> 24) & 0xFF];
            if target == 20 {
                is_export_prim = true;
            }
            let val = format!(
                "float4(asfloat(v[{}]), asfloat(v[{}]), asfloat(v[{}]), asfloat(v[{}]))",
                vs[0], vs[1], vs[2], vs[3]
            );
            match target {
                12 => hlsl_stmt = format!("exp_pos0 = {val};"),
                0..=7 => hlsl_stmt = format!("exp_mrt[{target}] = {val};"),
                32..=63 => hlsl_stmt = format!("exp_param[{}] = {val};", target - 32),
                20 => hlsl_stmt = format!("/* exp_prim v[{},{},{}] */", vs[0], vs[1], vs[2]),
                _ => hlsl_stmt = format!("/* exp target={target} en=0x{en:x} */"),
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
                0x00 => 1,
                0x01 => 2,
                0x02 => 4,
                0x03 => 8,
                _ => 1,
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
            if op == 0x0D || op == 0x0E {
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
                0x141 | 0x16B => {
                    // v_fma_f32 / v_fmac_f32
                    hlsl_stmt = format!(
                        "v[{vdst}] = asuint({} * {} + {});",
                        fmt_src(s0, None, true),
                        fmt_src(s1, None, true),
                        fmt_src(s2, None, true)
                    );
                    format!("v_fma_f32 v{vdst}")
                }
                0x144 => {
                    // v_bfe_u32
                    hlsl_stmt = format!(
                        "v[{vdst}] = ({} >> ({} & 31u)) & ((1u << ({} & 31u)) - 1u);",
                        fmt_src(s0, None, false),
                        fmt_src(s1, None, false),
                        fmt_src(s2, None, false)
                    );
                    format!("v_bfe_u32 v{vdst}")
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
                    hlsl_stmt = format!("v[{vdst}] = {}; /* vop3_0x{op:03x} */", fmt_src(s0, None, false));
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
                // bvh64 ray intersection
                hlsl_stmt = format!("v[{vdata}] = asuint(1.0); /* image_bvh64_0x{op:02x} */");
            } else {
                hlsl_stmt = format!(
                    "{{ float4 _ts = g_tex[{tex_idx}].SampleLevel(g_samp[{samp_idx}], float2(asfloat(v[{vaddr}]), asfloat(v[{}])), 0.0); v[{vdata}] = asuint(_ts.x); v[{}] = asuint(_ts.y); }}",
                    (vaddr + 1) & 0xFF,
                    (vdata + 1) & 0xFF
                );
            }
            format!("mimg_0x{op:02x} v{vdata}, v{vaddr}, t#{tex_idx}, s#{samp_idx}")
        } else if (w0 >> 26) == 0x34 || (w0 >> 26) == 0x37 || (w0 >> 26) == 0x38 {
            len_words = 2;
            hlsl_stmt = format!("/* mem_0x{:02x} */", w0 >> 26);
            format!("mem_0x{:02x}", w0 >> 26)
        } else if (w0 >> 25) == 0x3F {
            // VOP1 (must be checked before VOP2 because bit 31 is 0)
            let op = (w0 >> 9) & 0xFF;
            let src0 = w0 & 0x1FF;
            let vdst = (w0 >> 17) & 0xFF;
            let lit = if src0 == 255 {
                len_words = 2;
                Some(w1)
            } else {
                None
            };
            let a_f = fmt_src(src0, lit, true);
            let a_u = fmt_src(src0, lit, false);
            hlsl_stmt = match op {
                0x01 => format!("v[{vdst}] = {a_u};"),                                   // v_mov_b32
                0x05 => format!("v[{vdst}] = asuint((float)asint({a_u}));"),             // v_cvt_f32_i32
                0x06 => format!("v[{vdst}] = asuint((float){a_u});"),                    // v_cvt_f32_u32
                0x07 => format!("v[{vdst}] = (uint)(int){a_f};"),                        // v_cvt_u32_f32
                0x08 => format!("v[{vdst}] = asuint((int){a_f});"),                      // v_cvt_i32_f32
                0x1F => format!("v[{vdst}] = asuint(frac({a_f}));"),                     // v_fract_f32
                0x24 => format!("v[{vdst}] = asuint(floor({a_f}));"),                    // v_floor_f32
                0x25 => format!("v[{vdst}] = asuint(exp2({a_f}));"),                     // v_exp_f32
                0x27 => format!("v[{vdst}] = asuint(log2(max({a_f}, 1e-12)));"),         // v_log_f32
                0x2A => format!("v[{vdst}] = asuint(rcp({a_f}));"),                      // v_rcp_f32
                0x2E => format!("v[{vdst}] = asuint(rsqrt(max({a_f}, 1e-12)));"),        // v_rsq_f32
                0x33 => format!("v[{vdst}] = asuint(sqrt(max({a_f}, 0.0)));"),           // v_sqrt_f32
                0x35 => format!("v[{vdst}] = asuint(sin({a_f} * 6.2831853));"),          // v_sin_f32
                0x36 => format!("v[{vdst}] = asuint(cos({a_f} * 6.2831853));"),          // v_cos_f32
                _ => format!("v[{vdst}] = {a_u}; /* vop1_0x{op:02x} */"),
            };
            format!("vop1_0x{op:02x} v{vdst}, src{src0}")
        } else if (w0 >> 25) == 0x3E {
            // VOPC (must be checked before VOP2 because bit 31 is 0)
            let op = (w0 >> 17) & 0xFF;
            let src0 = w0 & 0x1FF;
            let vsrc1 = (w0 >> 9) & 0xFF;
            let lit = if src0 == 255 {
                len_words = 2;
                Some(w1)
            } else {
                None
            };
            let a_f = fmt_src(src0, lit, true);
            let b_f = format!("asfloat(v[{vsrc1}])");
            hlsl_stmt = format!("vcc = ({a_f} < {b_f}) ? 1u : 0u; /* vopc_0x{op:02x} */");
            format!("vopc_0x{op:02x} src{src0}, v{vsrc1}")
        } else if (w0 >> 31) == 0 {
            // VOP2
            let op = (w0 >> 25) & 0x3F;
            let src0 = w0 & 0x1FF;
            let vsrc1 = (w0 >> 9) & 0xFF;
            let vdst = (w0 >> 17) & 0xFF;
            let lit = if src0 == 255 {
                len_words = 2;
                Some(w1)
            } else {
                None
            };
            let a_f = fmt_src(src0, lit, true);
            let b_f = format!("asfloat(v[{vsrc1}])");
            let a_u = fmt_src(src0, lit, false);
            let b_u = format!("v[{vsrc1}]");
            hlsl_stmt = match op {
                0x01 => format!("v[{vdst}] = asuint({a_f} * {b_f});"),                   // v_mul_f32
                0x03 => format!("v[{vdst}] = asuint({a_f} + {b_f});"),                   // v_add_f32
                0x04 => format!("v[{vdst}] = asuint({a_f} - {b_f});"),                   // v_sub_f32
                0x05 => format!("v[{vdst}] = asuint({b_f} - {a_f});"),                   // v_subrev_f32
                0x0F => format!("v[{vdst}] = asuint(min({a_f}, {b_f}));"),               // v_min_f32
                0x10 => format!("v[{vdst}] = asuint(max({a_f}, {b_f}));"),               // v_max_f32
                0x1B => format!("v[{vdst}] = {a_u} & {b_u};"),                           // v_and_b32
                0x1C => format!("v[{vdst}] = {a_u} | {b_u};"),                           // v_or_b32
                0x1D => format!("v[{vdst}] = {a_u} ^ {b_u};"),                           // v_xor_b32
                0x25 => format!("v[{vdst}] = {a_u} + {b_u};"),                           // v_add_nc_u32
                0x26 => format!("v[{vdst}] = {b_u} - {a_u};"),                           // v_sub_nc_u32
                0x2B => format!("v[{vdst}] = asuint(asfloat(v[{vdst}]) + {a_f} * {b_f});"), // v_fmac_f32
                0x00 => format!("v[{vdst}] = ((vcc & 1u) != 0u) ? {b_u} : {a_u};"),      // v_cndmask_b32
                _ => format!("v[{vdst}] = {a_u} ^ {b_u}; /* vop2_0x{op:02x} */"),
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
                0x00 => format!("{dst} = {a} + {b};"),
                0x01 => format!("{dst} = {a} - {b};"),
                0x0E => format!("{dst} = {a} & {b};"),
                0x10 => format!("{dst} = {a} | {b};"),
                0x12 => format!("{dst} = {a} ^ {b};"),
                0x1C => format!("{dst} = {a} << ({b} & 31u);"),
                0x1E => format!("{dst} = {a} >> ({b} & 31u);"),
                0x2C => format!("{dst} = asuint(asint({a}) * asint({b}));"),
                _ => format!("{dst} = {a} ^ {b}; /* sop2_0x{op:02x} */"),
            };
            format!("sop2_0x{op:02x} s{sdst}, s{ssrc0}, s{ssrc1}")
        } else {
            hlsl_stmt = format!("/* raw_0x{w0:08x} */");
            format!("raw_0x{w0:08x}")
        };

        ops.push(DecodedOp {
            offset: off,
            word0: w0,
            word1: w1,
            len_words,
            mnemonic,
            hlsl_stmt,
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
            break;
        }
    }
    ops
}

pub fn detect_stage(ops: &[DecodedOp], hint: ShaderStage) -> ShaderStage {
    if ops.iter().any(|op| op.is_export_prim) {
        return ShaderStage::Mesh;
    }
    hint
}

/// Lifts an RDNA2 instruction stream into per-instruction compilable HLSL.
pub fn lift_to_hlsl(bytes: &[u8], hint: ShaderStage) -> (ShaderStage, String, usize) {
    let ops = decode_rdna2(bytes);
    let stage = detect_stage(&ops, hint);
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
    body.push_str("    float4 exp_pos0 = float4(0.0, 0.0, 0.0, 1.0);\n");
    body.push_str("    float4 exp_mrt[8] = { float4(0,0,0,1), float4(0,0,0,1), float4(0,0,0,1), float4(0,0,0,1), float4(0,0,0,1), float4(0,0,0,1), float4(0,0,0,1), float4(0,0,0,1) };\n");
    body.push_str("    float4 exp_param[32]; [unroll] for (uint _p = 0u; _p < 32u; ++_p) { exp_param[_p] = float4(0,0,0,0); }\n");

    let has_branches = ops.iter().any(|o| o.branch_target.is_some());
    if !has_branches {
        for op in &ops {
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
            let _ = writeln!(hlsl, "    // seed v[0] = vid");
            hlsl.push_str(&body);
            hlsl.push_str(
                "    v[0] = vid;\n\
                 \tVSOut o;\n\
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
}
