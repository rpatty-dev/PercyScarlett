//! Clean-room RDNA2 (GFX10.3) instruction decoder and instruction-level HLSL lifter.

use std::fmt::Write as _;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShaderStage {
    Vertex,
    Pixel,
    Compute,
    Mesh,
    Geometry,
}

impl ShaderStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Vertex => "vs",
            Self::Pixel => "ps",
            Self::Compute => "cs",
            Self::Mesh => "ms",
            Self::Geometry => "gs",
        }
    }

    pub fn dxbc_target(self) -> &'static str {
        match self {
            Self::Vertex => "vs_5_0",
            Self::Pixel => "ps_5_0",
            Self::Compute => "cs_5_0",
            Self::Geometry => "gs_5_0",
            Self::Mesh => "cs_5_0", // SM 5.0 fallback target; SM 6.5 uses ms_6_5 via DXC
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
        let mut hlsl_stmt = String::new();

        let mnemonic = if (w0 & 0xFFFF_0000) == 0xBF81_0000 {
            is_endpgm = true;
            hlsl_stmt = "// s_endpgm".into();
            "s_endpgm".to_string()
        } else if (w0 & 0xFF80_0000) == 0xBF80_0000 {
            let op = (w0 >> 16) & 0x7F;
            let imm = (w0 & 0xFFFF) as i16;
            match op {
                0x00 => {
                    hlsl_stmt = "/* s_nop */".into();
                    "s_nop".into()
                }
                0x02..=0x09 => {
                    hlsl_stmt = format!("/* branch op=0x{op:02x} rel={imm} */");
                    format!("s_branch_0x{op:02x} {imm}")
                }
                0x0C => {
                    hlsl_stmt = "/* s_waitcnt */".into();
                    format!("s_waitcnt 0x{:04x}", w0 & 0xFFFF)
                }
                _ => format!("sopp_0x{op:02x} {imm}"),
            }
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
                let _ = writeln!(
                    hlsl_stmt,
                    "s[{}] = g_cbuf.Load({}u);",
                    sdata + i,
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
                _ => {
                    hlsl_stmt = format!("v[{vdst}] = {}; /* vop3_0x{op:03x} */", fmt_src(s0, None, false));
                    format!("vop3_0x{op:03x} v{vdst}")
                }
            }
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
            hlsl_stmt = match op {
                0x00 => format!("s[{sdst}] = {a} + {b};"),
                0x01 => format!("s[{sdst}] = {a} - {b};"),
                0x0E => format!("s[{sdst}] = {a} & {b};"),
                0x10 => format!("s[{sdst}] = {a} | {b};"),
                0x12 => format!("s[{sdst}] = {a} ^ {b};"),
                0x1C => format!("s[{sdst}] = {a} << ({b} & 31u);"),
                0x1E => format!("s[{sdst}] = {a} >> ({b} & 31u);"),
                0x2C => format!("s[{sdst}] = asuint(asint({a}) * asint({b}));"),
                _ => format!("s[{sdst}] = {a} ^ {b}; /* sop2_0x{op:02x} */"),
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
    let uses_lds = ops.iter().any(|o| o.uses_lds);

    let mut hlsl = String::with_capacity(2048 + insn_count * 48);
    let _ = writeln!(
        hlsl,
        "// PercyScarlett RDNA2 -> HLSL ({}, {} insns, {} exports)",
        stage.as_str(),
        insn_count,
        export_count
    );
    hlsl.push_str("ByteAddressBuffer g_cbuf : register(t0);\n");
    if stage == ShaderStage::Compute {
        hlsl.push_str("RWByteAddressBuffer g_out : register(u0);\n");
    }
    if uses_lds {
        hlsl.push_str("groupshared uint g_lds[4096];\n");
    }
    hlsl.push('\n');

    let mut body = String::with_capacity(1024 + insn_count * 40);
    body.push_str("    uint s[106]; uint v[32]; uint vcc = 0u; uint exec_lo = 0xFFFFFFFFu; uint exec_hi = 0u;\n");
    body.push_str("    [unroll] for (uint _i = 0u; _i < 16u; ++_i) { s[_i] = 0u; }\n");
    body.push_str("    [unroll] for (uint _j = 0u; _j < 32u; ++_j) { v[_j] = 0u; }\n");
    body.push_str("    float4 exp_pos0 = float4(0.0, 0.0, 0.0, 1.0);\n");
    body.push_str("    float4 exp_mrt[4] = { float4(0,0,0,1), float4(0,0,0,1), float4(0,0,0,1), float4(0,0,0,1) };\n");
    body.push_str("    float4 exp_param[4] = { float4(0,0,0,0), float4(0,0,0,0), float4(0,0,0,0), float4(0,0,0,0) };\n");

    for op in &ops {
        if !op.hlsl_stmt.is_empty() {
            for line in op.hlsl_stmt.lines() {
                let _ = writeln!(body, "    {line}");
            }
        }
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
}
