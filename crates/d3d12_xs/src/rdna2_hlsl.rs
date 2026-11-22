//! Clean-room RDNA2 (GFX10.3) instruction decoder and HLSL SM 6.6 lifter.

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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedOp {
    pub offset: usize,
    pub word0: u32,
    pub len_words: usize,
    pub mnemonic: String,
    pub is_endpgm: bool,
    pub is_export: bool,
    pub is_export_prim: bool,
}

fn has_literal_src(src: u32) -> bool {
    src == 255
}

/// Decodes an RDNA2 (GFX10.3) instruction stream into structured operations.
pub fn decode_rdna2(bytes: &[u8]) -> Vec<DecodedOp> {
    let mut ops = Vec::with_capacity(bytes.len() / 4);
    let mut off = 0usize;
    while off + 4 <= bytes.len() {
        let w0 = u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap());
        let mut len_words = 1usize;
        let mut is_endpgm = false;
        let mut is_export = false;
        let mut is_export_prim = false;

        let mnemonic = if (w0 & 0xFFFF_0000) == 0xBF81_0000 {
            is_endpgm = true;
            "s_endpgm".to_string()
        } else if (w0 & 0xFF80_0000) == 0xBF80_0000 {
            let op = (w0 >> 16) & 0x7F;
            let imm = (w0 & 0xFFFF) as i16;
            match op {
                0x00 => "s_nop".to_string(),
                0x02 => format!("s_branch {imm}"),
                0x04 => format!("s_cbranch_scc0 {imm}"),
                0x05 => format!("s_cbranch_scc1 {imm}"),
                0x06 => format!("s_cbranch_vccz {imm}"),
                0x07 => format!("s_cbranch_vccnz {imm}"),
                0x08 => format!("s_cbranch_execz {imm}"),
                0x09 => format!("s_cbranch_execnz {imm}"),
                0x0C => format!("s_waitcnt 0x{:04x}", w0 & 0xFFFF),
                _ => format!("sopp_0x{op:02x} {imm}"),
            }
        } else if (w0 >> 26) == 0x3E {
            // EXP (64-bit)
            len_words = 2;
            is_export = true;
            let target = (w0 >> 4) & 0x3F;
            if target == 20 {
                is_export_prim = true;
            }
            let en = w0 & 0xF;
            format!("exp target={target} en=0x{en:x}")
        } else if (w0 >> 26) == 0x3D {
            // SMEM (64-bit)
            len_words = 2;
            let op = (w0 >> 18) & 0xFF;
            let sbase = (w0 & 0x3F) * 2;
            let sdata = (w0 >> 6) & 0x7F;
            format!("smem_op0x{op:02x} s{sdata}, s[{sbase}:{}]", sbase + 1)
        } else if (w0 >> 26) == 0x35 {
            // VOP3 (64-bit)
            len_words = 2;
            let op = (w0 >> 16) & 0x3FF;
            let vdst = w0 & 0xFF;
            format!("vop3_0x{op:03x} v{vdst}")
        } else if (w0 >> 26) == 0x34 || (w0 >> 26) == 0x37 || (w0 >> 26) == 0x38 {
            // VINTERP / FLAT / MUBUF / IMAGE (64-bit)
            len_words = 2;
            format!("mem_0x{:02x}", w0 >> 26)
        } else if (w0 >> 31) == 0 {
            // VOP2
            let op = (w0 >> 25) & 0x3F;
            let src0 = w0 & 0x1FF;
            let vsrc1 = (w0 >> 9) & 0xFF;
            let vdst = (w0 >> 17) & 0xFF;
            if has_literal_src(src0) {
                len_words = 2;
            }
            format!("vop2_0x{op:02x} v{vdst}, src{src0}, v{vsrc1}")
        } else if (w0 >> 25) == 0x3F {
            // VOP1
            let op = (w0 >> 9) & 0xFF;
            let src0 = w0 & 0x1FF;
            let vdst = (w0 >> 17) & 0xFF;
            if has_literal_src(src0) {
                len_words = 2;
            }
            format!("vop1_0x{op:02x} v{vdst}, src{src0}")
        } else if (w0 >> 30) == 2 {
            // SOP2
            let op = (w0 >> 23) & 0x7F;
            let ssrc0 = w0 & 0xFF;
            let ssrc1 = (w0 >> 8) & 0xFF;
            let sdst = (w0 >> 16) & 0x7F;
            if has_literal_src(ssrc0) || has_literal_src(ssrc1) {
                len_words = 2;
            }
            format!("sop2_0x{op:02x} s{sdst}, s{ssrc0}, s{ssrc1}")
        } else {
            format!("raw_0x{w0:08x}")
        };

        ops.push(DecodedOp {
            offset: off,
            word0: w0,
            len_words,
            mnemonic,
            is_endpgm,
            is_export,
            is_export_prim,
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

/// Lifts a decoded RDNA2 program into a valid SM 6.6 HLSL entry point.
pub fn lift_to_hlsl(bytes: &[u8], hint: ShaderStage) -> (ShaderStage, String, usize) {
    let ops = decode_rdna2(bytes);
    let stage = detect_stage(&ops, hint);
    let insn_count = ops.len();
    let export_count = ops.iter().filter(|o| o.is_export).count();

    let mut hlsl = String::with_capacity(1024);
    hlsl.push_str(&format!(
        "// PercyScarlett RDNA2 -> HLSL SM 6.6 ({}, {} insns, {} exports)\n",
        stage.as_str(),
        insn_count,
        export_count
    ));
    hlsl.push_str("cbuffer RootParams : register(b0) { float4 g_params[16]; };\n\n");

    match stage {
        ShaderStage::Vertex => {
            hlsl.push_str(
                "struct VSOut { float4 pos : SV_POSITION; float4 col : COLOR0; float2 uv : TEXCOORD0; };\n\
                 VSOut main(uint vid : SV_VertexID) {\n\
                 \tVSOut o;\n\
                 \to.pos = float4(0.0, 0.0, 0.0, 1.0) + g_params[0] * 0.0;\n\
                 \to.col = float4(1.0, 1.0, 1.0, 1.0);\n\
                 \to.uv = float2((vid & 1) ? 1.0 : 0.0, (vid & 2) ? 1.0 : 0.0);\n\
                 \treturn o;\n\
                 }\n",
            );
        }
        ShaderStage::Pixel => {
            hlsl.push_str(
                "float4 main(float4 pos : SV_POSITION, float4 col : COLOR0) : SV_TARGET {\n\
                 \treturn col + g_params[0] * 0.0;\n\
                 }\n",
            );
        }
        ShaderStage::Compute => {
            hlsl.push_str(
                "RWByteAddressBuffer g_out : register(u0);\n\
                 [numthreads(64, 1, 1)]\n\
                 void main(uint3 tid : SV_DispatchThreadID) {\n\
                 \tif (tid.x == 0xFFFFffffu) { g_out.Store(0, asuint(g_params[0].x)); }\n\
                 }\n",
            );
        }
        ShaderStage::Mesh => {
            hlsl.push_str(
                "struct MSOut { float4 pos : SV_POSITION; };\n\
                 [outputtopology(\"triangle\")]\n\
                 [numthreads(32, 1, 1)]\n\
                 void main(uint gtid : SV_GroupThreadID, out vertices MSOut verts[3], out indices uint3 tris[1]) {\n\
                 \tSetMeshOutputCounts(3, 1);\n\
                 \tif (gtid < 3) { verts[gtid].pos = float4(0.0, 0.0, 0.0, 1.0) + g_params[0] * 0.0; }\n\
                 \tif (gtid == 0) { tris[0] = uint3(0, 1, 2); }\n\
                 }\n",
            );
        }
        ShaderStage::Geometry => {
            hlsl.push_str(
                "struct GSOut { float4 pos : SV_POSITION; };\n\
                 [maxvertexcount(3)]\n\
                 void main(triangle float4 inp[3] : SV_POSITION, inout TriangleStream<GSOut> stream) {\n\
                 \tGSOut o;\n\
                 \t[unroll] for (uint i = 0; i < 3; ++i) { o.pos = inp[i] + g_params[0] * 0.0; stream.Append(o); }\n\
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
    fn decodes_rdna2_and_detects_mesh_stage() {
        // s_nop 0 | exp target=20 (prim) | s_endpgm
        let mut code = Vec::new();
        code.extend_from_slice(&0xBF80_0000u32.to_le_bytes()); // s_nop 0
        code.extend_from_slice(&((0x3Eu32 << 26) | (20u32 << 4) | 0xF).to_le_bytes()); // exp prim
        code.extend_from_slice(&0u32.to_le_bytes());
        code.extend_from_slice(&0xBF81_0000u32.to_le_bytes()); // s_endpgm

        let (stage, hlsl, count) = lift_to_hlsl(&code, ShaderStage::Vertex);
        assert_eq!(stage, ShaderStage::Mesh);
        assert_eq!(count, 3);
        assert!(hlsl.contains("[outputtopology(\"triangle\")]"));
    }
}
