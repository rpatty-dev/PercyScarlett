//! Clean-room parser for Xbox Series (Scarlett) DXBC shader containers:
//! ISG1/OSG1 signatures and DXIL -> XDAT -> XBCH (zlib) -> FSSM native GFX10 programs.
#![allow(dead_code)]

use std::io::Read;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HwStage {
    Vs,
    Ps,
    Ms,
    Cs,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SigParam {
    pub stream: u32,
    pub semantic: String,
    pub index: u32,
    pub sys_val: u32,
    pub comp_type: u32, // 1=uint, 2=int, 3=float
    pub register: u32,
    pub mask: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootParamKind {
    Constants,
    Descriptor,
    Table,
    ImmediateSampler,
    ResourceHeap,
    SamplerHeap,
    Other(u32),
}

impl RootParamKind {
    fn from_raw(v: u32) -> Self {
        match v {
            0 => Self::Constants,
            1 => Self::Descriptor,
            3 => Self::Table,
            4 => Self::ImmediateSampler,
            5 => Self::ResourceHeap,
            6 => Self::SamplerHeap,
            x => Self::Other(x),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RootLayoutEntry {
    pub kind: RootParamKind,
    pub table_offset_dw: u32,
    pub user_sgprs: u32,
    pub base_reg: u32,
    pub first_user_data: u8,
    pub root_slot: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResKind {
    Srv,
    Sampler,
    Cbv,
    Uav,
    Other(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsedResEntry {
    pub res_kind: ResKind,
    pub root_kind: RootParamKind,
    pub table_offset_dw: u32,
    pub lower_bound: u32,
    pub stride: u32,
    pub desc_dwords: u8,
    pub first_user_data: u8,
    pub root_slot: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VsInputMap {
    pub usage: u8,
    pub row: u8,
    pub mask: u8,
    pub vgpr: u8,
    pub highest_ch: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VsOutputMap {
    pub usage: u8,
    pub row: u8,
    pub param: u8,
    pub flags: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PsInputMap {
    pub usage: u8,
    pub row: u8,
    pub mask: u8,
    pub attr: u8,
    pub flags: u8, // bit 0 flat, bit 1 linear, bit 3 fp16, bit 4 fp16 packed
}

#[derive(Debug, Clone)]
pub struct XboxProgram {
    pub stage: HwStage,
    pub wave32: bool,
    pub code: Vec<u8>,
    pub isg: Vec<SigParam>,
    pub osg: Vec<SigParam>,
    pub root_layout: Vec<RootLayoutEntry>,
    pub used_res: Vec<UsedResEntry>,
    pub vs_inputs: Vec<VsInputMap>,
    pub vs_outputs: Vec<VsOutputMap>,
    pub ps_inputs: Vec<PsInputMap>,
    pub spi_ps_input_addr: u32,
    pub db_shader_control: u32,
    pub cs_threads: [u32; 3],
}

fn rd32(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}
fn rd64(b: &[u8], o: usize) -> Option<u64> {
    b.get(o..o + 8).map(|s| u64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))
}

fn find_part<'a>(dxbc: &'a [u8], fourcc: &[u8; 4]) -> Option<&'a [u8]> {
    if dxbc.len() < 32 || &dxbc[0..4] != b"DXBC" {
        return None;
    }
    let count = rd32(dxbc, 28)? as usize;
    if count > 64 || 32 + count * 4 > dxbc.len() {
        return None;
    }
    for i in 0..count {
        let off = rd32(dxbc, 32 + i * 4)? as usize;
        if off + 8 <= dxbc.len() && &dxbc[off..off + 4] == fourcc {
            let sz = rd32(dxbc, off + 4)? as usize;
            return dxbc.get(off + 8..off + 8 + sz);
        }
    }
    None
}

fn parse_sig(part: &[u8], is_v1: bool) -> Vec<SigParam> {
    let Some(count) = rd32(part, 0) else { return Vec::new() };
    let stride = if is_v1 { 32 } else { 24 };
    let mut out = Vec::with_capacity(count as usize);
    for i in 0..count as usize {
        let base = 8 + i * stride;
        if base + stride > part.len() {
            break;
        }
        let (stream, name_off, index, sys_val, comp_type, register, mask) = if is_v1 {
            (
                rd32(part, base).unwrap_or(0),
                rd32(part, base + 4).unwrap_or(0) as usize,
                rd32(part, base + 8).unwrap_or(0),
                rd32(part, base + 12).unwrap_or(0),
                rd32(part, base + 16).unwrap_or(0),
                rd32(part, base + 20).unwrap_or(0),
                part[base + 24],
            )
        } else {
            (
                0,
                rd32(part, base).unwrap_or(0) as usize,
                rd32(part, base + 4).unwrap_or(0),
                rd32(part, base + 8).unwrap_or(0),
                rd32(part, base + 12).unwrap_or(0),
                rd32(part, base + 16).unwrap_or(0),
                part[base + 20],
            )
        };
        let semantic = if name_off < part.len() {
            let tail = &part[name_off..];
            let nul = tail.iter().position(|&c| c == 0).unwrap_or(tail.len());
            String::from_utf8_lossy(&tail[..nul]).into_owned()
        } else {
            String::new()
        };
        out.push(SigParam { stream, semantic, index, sys_val, comp_type, register, mask });
    }
    out
}

/// Parses an Xbox Series DXBC shader container and extracts the native GFX10 program for `want`
/// (or the first matching standalone stage if `want` is `None`).
pub fn parse_container(dxbc: &[u8], want: Option<HwStage>) -> Result<XboxProgram, String> {
    let isg = find_part(dxbc, b"ISG1")
        .map(|p| parse_sig(p, true))
        .or_else(|| find_part(dxbc, b"ISGN").map(|p| parse_sig(p, false)))
        .unwrap_or_default();
    let osg = find_part(dxbc, b"OSG1")
        .map(|p| parse_sig(p, true))
        .or_else(|| find_part(dxbc, b"OSGN").map(|p| parse_sig(p, false)))
        .unwrap_or_default();

    let dxil = find_part(dxbc, b"DXIL").ok_or_else(|| "no DXIL part in container".to_string())?;
    if dxil.len() < 0x24 || &dxil[0x18..0x1c] != b"XDAT" {
        return Err("no XDAT index in DXIL part".into());
    }
    let n_entries = rd32(dxil, 0x1c).ok_or("short XDAT")? as usize;
    if n_entries > 64 || 0x24 + n_entries * 12 > dxil.len() {
        return Err(format!("bad XDAT entry count {n_entries}"));
    }

    let mut last_err = "no native kind=0x0d XBCH blob in XDAT".to_string();
    for i in 0..n_entries {
        let e = 0x24 + i * 12;
        let kind = rd32(dxil, e).unwrap_or(0);
        if kind != 0x0d {
            continue;
        }
        let off = rd32(dxil, e + 4).unwrap_or(0) as usize;
        let sz = rd32(dxil, e + 8).unwrap_or(0) as usize;
        let Some(xbch) = dxil.get(off..off + sz) else { continue };
        if xbch.len() < 0x18 || &xbch[0..4] != b"XBCH" {
            continue;
        }
        let ver = rd32(xbch, 4).unwrap_or(0);
        if ver != 3 {
            last_err = format!("unsupported XBCH version {ver}");
            continue;
        }
        let unpacked_sz = rd64(xbch, 16).unwrap_or(0) as usize;
        let mut dec = flate2::read::ZlibDecoder::new(&xbch[0x18..]);
        let mut fssm = Vec::with_capacity(unpacked_sz.min(4 << 20));
        if let Err(err) = dec.read_to_end(&mut fssm) {
            last_err = format!("XBCH zlib decode failed: {err}");
            continue;
        }
        if fssm.len() < 0x80 || &fssm[0..4] != b"FSSM" {
            last_err = "unpacked XBCH is not FSSM".into();
            continue;
        }
        let payload = &fssm[0x10..];
        const SLOTS: [(HwStage, usize); 4] = [
            (HwStage::Vs, 0x28),
            (HwStage::Ps, 0x30),
            (HwStage::Ms, 0x48),
            (HwStage::Cs, 0x60),
        ];
        for &(stage, slot_off) in &SLOTS {
            if let Some(w) = want {
                if w != stage {
                    continue;
                }
            }
            let Some(rec_off) = rd64(payload, slot_off) else { continue };
            if rec_off == 0xDEAD_BEEF_DEAD_BEEF || rec_off as usize + 0x54 > payload.len() {
                continue;
            }
            let rec = rec_off as usize;
            let code_off = rd64(payload, rec).ok_or("bad code_off")? as usize;
            let code_sz = rd64(payload, rec + 8).ok_or("bad code_sz")? as usize;
            let flags = rd32(payload, rec + 0x14).unwrap_or(0);
            let wave32 = (flags & (1 << 5)) != 0;
            let mut code = payload
                .get(code_off..code_off + code_sz)
                .ok_or("code slice out of bounds")?
                .to_vec();

            let rl_off = rd64(payload, rec + 0x28).unwrap_or(0) as usize;
            let rl_cnt = rd32(payload, rec + 0x38).unwrap_or(0) as usize;
            let mut root_layout = Vec::with_capacity(rl_cnt.min(64));
            for k in 0..rl_cnt.min(64) {
                let p = rl_off + k * 20;
                if p + 20 > payload.len() { break; }
                root_layout.push(RootLayoutEntry {
                    kind: RootParamKind::from_raw(rd32(payload, p).unwrap_or(0)),
                    table_offset_dw: rd32(payload, p + 4).unwrap_or(0),
                    user_sgprs: rd32(payload, p + 8).unwrap_or(0),
                    base_reg: rd32(payload, p + 12).unwrap_or(0),
                    first_user_data: payload[p + 16],
                    root_slot: payload[p + 17],
                });
            }

            let ur_off = rd64(payload, rec + 0x30).unwrap_or(0) as usize;
            let ur_cnt = rd32(payload, rec + 0x3c).unwrap_or(0) as usize;
            let mut used_res = Vec::with_capacity(ur_cnt.min(128));
            for k in 0..ur_cnt.min(128) {
                let p = ur_off + k * 20;
                if p + 20 > payload.len() { break; }
                used_res.push(UsedResEntry {
                    res_kind: match payload[p] {
                        0 => ResKind::Srv,
                        1 => ResKind::Sampler,
                        2 => ResKind::Cbv,
                        3 => ResKind::Uav,
                        x => ResKind::Other(x),
                    },
                    root_kind: RootParamKind::from_raw(payload[p + 1] as u32),
                    table_offset_dw: rd32(payload, p + 4).unwrap_or(0),
                    lower_bound: rd32(payload, p + 8).unwrap_or(0),
                    stride: rd32(payload, p + 12).unwrap_or(0),
                    desc_dwords: payload[p + 16],
                    first_user_data: payload[p + 17],
                    root_slot: payload[p + 18],
                });
            }

            let e_base = rec + 0x54;
            let mut vs_inputs = Vec::new();
            let mut vs_outputs = Vec::new();
            let mut ps_inputs = Vec::new();
            let mut spi_ps_input_addr = 0u32;
            let mut db_shader_control = 0u32;
            let mut cs_threads = [64u32, 1, 1];

            match stage {
                HwStage::Vs => {
                    if e_base + 0x18c <= payload.len() {
                        let icnt = rd32(payload, e_base + 0xc4).unwrap_or(0).min(32) as usize;
                        for k in 0..icnt {
                            let p = e_base + 0x24 + k * 5;
                            vs_inputs.push(VsInputMap {
                                usage: payload[p],
                                row: payload[p + 1],
                                mask: payload[p + 2],
                                vgpr: payload[p + 3],
                                highest_ch: payload[p + 4],
                            });
                        }
                        let ocnt = rd32(payload, e_base + 0x188).unwrap_or(0).min(32) as usize;
                        for k in 0..ocnt {
                            let p = e_base + 0xc8 + k * 6;
                            vs_outputs.push(VsOutputMap {
                                usage: payload[p],
                                row: payload[p + 1],
                                param: payload[p + 2],
                                flags: payload[p + 5],
                            });
                        }
                    }
                }
                HwStage::Ps => {
                    if e_base + 0x14c <= payload.len() {
                        let icnt = rd32(payload, e_base + 0x124).unwrap_or(0).min(32) as usize;
                        for k in 0..icnt {
                            let p = e_base + 0x24 + k * 8;
                            ps_inputs.push(PsInputMap {
                                usage: payload[p],
                                row: payload[p + 1],
                                mask: payload[p + 2],
                                attr: payload[p + 3],
                                flags: payload[p + 7],
                            });
                        }
                        spi_ps_input_addr = rd32(payload, e_base + 0x134).unwrap_or(0);
                        db_shader_control = rd32(payload, e_base + 0x138).unwrap_or(0);
                        let patch_dw = rd32(payload, e_base + 0x148).unwrap_or(0).min(10) as usize;
                        // 8 MRT patches at e_base + 0x14c: each = u32 code_dw_off + 10 slots * 10 dwords (404 bytes).
                        // Apply slot 9 (32_ABGR) so MRT exports are plain uncompressed 32-bit floats.
                        for mrt in 0..8usize {
                            let p = e_base + 0x14c + mrt * 404;
                            if p + 404 > payload.len() { break; }
                            let dw_off = rd32(payload, p).unwrap_or(u32::MAX);
                            if dw_off == u32::MAX { continue; }
                            let byte_off = dw_off as usize * 4;
                            let slot9 = p + 4 + 9 * 40;
                            if byte_off + patch_dw * 4 <= code.len() && slot9 + patch_dw * 4 <= payload.len() {
                                code[byte_off..byte_off + patch_dw * 4]
                                    .copy_from_slice(&payload[slot9..slot9 + patch_dw * 4]);
                            }
                        }
                    }
                }
                HwStage::Cs => {
                    if e_base + 0x34 <= payload.len() {
                        cs_threads = [
                            rd32(payload, e_base + 0x28).unwrap_or(64).max(1),
                            rd32(payload, e_base + 0x2c).unwrap_or(1).max(1),
                            rd32(payload, e_base + 0x30).unwrap_or(1).max(1),
                        ];
                    }
                }
                HwStage::Ms => {}
            }

            return Ok(XboxProgram {
                stage,
                wave32,
                code,
                isg,
                osg,
                root_layout,
                used_res,
                vs_inputs,
                vs_outputs,
                ps_inputs,
                spi_ps_input_addr,
                db_shader_control,
                cs_threads,
            });
        }
    }
    Err(last_err)
}
