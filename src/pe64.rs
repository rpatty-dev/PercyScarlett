#[derive(Clone, Debug)]
pub struct PeSection {
    pub name: String,
    pub virtual_addr: u32,
    pub virtual_size: u32,
    pub raw_ptr: u32,
    pub raw_size: u32,
}

#[derive(Clone, Debug)]
pub struct PeImportDll {
    pub name: String,
    pub functions: Vec<String>,
    pub delay_loaded: bool,
}

#[derive(Clone, Debug)]
pub struct Pe64Summary {
    pub machine: u16,
    pub entry_rva: u32,
    pub image_base: u64,
    pub subsystem: u16,
    pub sections: Vec<PeSection>,
    pub imports: Vec<PeImportDll>,
    pub exports: Vec<String>,
}

fn ru16(b: &[u8], o: usize) -> Option<u16> {
    b.get(o..o + 2).map(|s| u16::from_le_bytes([s[0], s[1]]))
}
fn ru32(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}
fn ru64(b: &[u8], o: usize) -> Option<u64> {
    b.get(o..o + 8).map(|s| u64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))
}

fn rva_to_off(rva: u32, secs: &[PeSection]) -> Option<usize> {
    for s in secs {
        let span = s.virtual_size.max(s.raw_size);
        if rva >= s.virtual_addr && rva < s.virtual_addr.saturating_add(span) {
            let delta = rva - s.virtual_addr;
            return Some((s.raw_ptr + delta) as usize);
        }
    }
    None
}

fn read_cstr(b: &[u8], off: usize) -> String {
    let mut end = off;
    while end < b.len() && b[end] != 0 && end - off < 256 {
        end += 1;
    }
    String::from_utf8_lossy(&b[off..end]).into_owned()
}

fn read_thunk_names(data: &[u8], thunk_rva: u32, sections: &[PeSection]) -> Vec<String> {
    let mut funcs = Vec::new();
    let Some(mut toff) = rva_to_off(thunk_rva, sections) else {
        return funcs;
    };
    while toff + 8 <= data.len() {
        let val = ru64(data, toff).unwrap_or(0);
        if val == 0 {
            break;
        }
        if (val & (1u64 << 63)) != 0 {
            funcs.push(format!("ord#{}", val & 0xFFFF));
        } else if let Some(hint_off) = rva_to_off(val as u32, sections) {
            funcs.push(read_cstr(data, hint_off + 2));
        }
        toff += 8;
    }
    funcs
}

pub fn parse_pe64(data: &[u8]) -> Result<Pe64Summary, String> {
    if data.len() < 0x100 || &data[0..2] != b"MZ" {
        return Err("Not a valid MZ executable".into());
    }
    let pe_off = ru32(data, 0x3C).ok_or("Missing e_lfanew")? as usize;
    if data.get(pe_off..pe_off + 4) != Some(b"PE\0\0") {
        return Err("Missing PE signature".into());
    }
    let machine = ru16(data, pe_off + 4).ok_or("Bad COFF header")?;
    let n_sec = ru16(data, pe_off + 6).unwrap_or(0) as usize;
    let opt_sz = ru16(data, pe_off + 20).unwrap_or(0) as usize;
    let opt_off = pe_off + 24;
    let magic = ru16(data, opt_off).ok_or("Missing optional header")?;
    if magic != 0x020B {
        return Err(format!("Expected PE32+ (0x020b), got 0x{magic:04x}"));
    }

    let entry_rva = ru32(data, opt_off + 16).unwrap_or(0);
    let image_base = ru64(data, opt_off + 24).unwrap_or(0);
    let subsystem = ru16(data, opt_off + 68).unwrap_or(0);
    let num_rva_sizes = ru32(data, opt_off + 108).unwrap_or(0);

    let sec_off = opt_off + opt_sz;
    let mut sections = Vec::with_capacity(n_sec);
    for i in 0..n_sec {
        let o = sec_off + i * 40;
        let Some(raw_name) = data.get(o..o + 8) else { break };
        let name = String::from_utf8_lossy(raw_name).trim_end_matches('\0').to_string();
        sections.push(PeSection {
            name,
            virtual_size: ru32(data, o + 8).unwrap_or(0),
            virtual_addr: ru32(data, o + 12).unwrap_or(0),
            raw_size: ru32(data, o + 16).unwrap_or(0),
            raw_ptr: ru32(data, o + 20).unwrap_or(0),
        });
    }

    let mut exports = Vec::new();
    let exp_rva = if num_rva_sizes > 0 { ru32(data, opt_off + 112).unwrap_or(0) } else { 0 };
    if exp_rva != 0 {
        if let Some(eoff) = rva_to_off(exp_rva, &sections) {
            if eoff + 40 <= data.len() {
                let n_names = ru32(data, eoff + 24).unwrap_or(0) as usize;
                let names_rva = ru32(data, eoff + 32).unwrap_or(0);
                if let Some(noff) = rva_to_off(names_rva, &sections) {
                    for i in 0..n_names {
                        let Some(name_rva) = ru32(data, noff + i * 4) else { break };
                        if let Some(soff) = rva_to_off(name_rva, &sections) {
                            exports.push(read_cstr(data, soff));
                        }
                    }
                }
            }
        }
    }

    let mut imports = Vec::new();
    let imp_rva = if num_rva_sizes > 1 { ru32(data, opt_off + 112 + 8).unwrap_or(0) } else { 0 };
    if imp_rva != 0 {
        if let Some(mut ioff) = rva_to_off(imp_rva, &sections) {
            while ioff + 20 <= data.len() {
                let oft = ru32(data, ioff).unwrap_or(0);
                let name_rva = ru32(data, ioff + 12).unwrap_or(0);
                let ft = ru32(data, ioff + 16).unwrap_or(0);
                if name_rva == 0 {
                    break;
                }
                if let Some(noff) = rva_to_off(name_rva, &sections) {
                    let dll_name = read_cstr(data, noff);
                    let thunk_rva = if oft != 0 { oft } else { ft };
                    let funcs = read_thunk_names(data, thunk_rva, &sections);
                    imports.push(PeImportDll { name: dll_name, functions: funcs, delay_loaded: false });
                }
                ioff += 20;
            }
        }
    }

    let delay_rva = if num_rva_sizes > 13 { ru32(data, opt_off + 112 + 13 * 8).unwrap_or(0) } else { 0 };
    if delay_rva != 0 {
        if let Some(mut doff) = rva_to_off(delay_rva, &sections) {
            while doff + 32 <= data.len() {
                let attrs = ru32(data, doff).unwrap_or(0);
                let raw_name = ru32(data, doff + 4).unwrap_or(0);
                let raw_int = ru32(data, doff + 16).unwrap_or(0);
                if raw_name == 0 {
                    break;
                }
                let to_rva = |v: u32| -> u32 {
                    if (attrs & 1) != 0 {
                        v
                    } else {
                        (v as u64).saturating_sub(image_base) as u32
                    }
                };
                if let Some(noff) = rva_to_off(to_rva(raw_name), &sections) {
                    let dll_name = read_cstr(data, noff);
                    let funcs = read_thunk_names(data, to_rva(raw_int), &sections);
                    imports.push(PeImportDll { name: dll_name, functions: funcs, delay_loaded: true });
                }
                doff += 32;
            }
        }
    }

    Ok(Pe64Summary { machine, entry_rva, image_base, subsystem, sections, imports, exports })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_pe64_header() {
        let mut pe = vec![0u8; 0x300];
        pe[0..2].copy_from_slice(b"MZ");
        pe[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        pe[0x80..0x84].copy_from_slice(b"PE\0\0");
        pe[0x84..0x86].copy_from_slice(&0x8664u16.to_le_bytes()); // AMD64
        pe[0x86..0x88].copy_from_slice(&1u16.to_le_bytes()); // 1 section
        pe[0x94..0x96].copy_from_slice(&0xF0u16.to_le_bytes()); // opt size
        pe[0x98..0x9A].copy_from_slice(&0x020Bu16.to_le_bytes()); // PE32+
        pe[0xA8..0xAC].copy_from_slice(&0x1000u32.to_le_bytes()); // entry RVA
        pe[0x104..0x108].copy_from_slice(&16u32.to_le_bytes()); // NumberOfRvaAndSizes
        // Section 0 at 0x188: ".text", VA=0x1000, VSize=0x200, RawSize=0x200, RawPtr=0x200
        pe[0x188..0x18D].copy_from_slice(b".text");
        pe[0x190..0x194].copy_from_slice(&0x200u32.to_le_bytes());
        pe[0x194..0x198].copy_from_slice(&0x1000u32.to_le_bytes());
        pe[0x198..0x19C].copy_from_slice(&0x200u32.to_le_bytes());
        pe[0x19C..0x1A0].copy_from_slice(&0x200u32.to_le_bytes());

        // Export table at RVA 0x1000 (off 0x200): 1 export "FooExport"
        pe[0x108..0x10C].copy_from_slice(&0x1000u32.to_le_bytes());
        pe[0x218..0x21C].copy_from_slice(&1u32.to_le_bytes()); // NumberOfNames
        pe[0x220..0x224].copy_from_slice(&0x1030u32.to_le_bytes()); // AddressOfNames
        pe[0x230..0x234].copy_from_slice(&0x1038u32.to_le_bytes()); // Name RVA
        pe[0x238..0x241].copy_from_slice(b"FooExport");

        // Delay import table at RVA 0x1050 (off 0x250): d3d12_xs.dll!D3D12XboxCreateDevice
        pe[0x170..0x174].copy_from_slice(&0x1050u32.to_le_bytes());
        pe[0x250..0x254].copy_from_slice(&1u32.to_le_bytes()); // grAttrs (RVA)
        pe[0x254..0x258].copy_from_slice(&0x1090u32.to_le_bytes()); // szName
        pe[0x260..0x264].copy_from_slice(&0x10A0u32.to_le_bytes()); // pINT
        pe[0x290..0x29C].copy_from_slice(b"d3d12_xs.dll");
        pe[0x2A0..0x2A8].copy_from_slice(&0x10B0u64.to_le_bytes()); // thunk -> hint/name
        pe[0x2B2..0x2C7].copy_from_slice(b"D3D12XboxCreateDevice");

        let summary = parse_pe64(&pe).unwrap();
        assert_eq!(summary.machine, 0x8664);
        assert_eq!(summary.entry_rva, 0x1000);
        assert_eq!(summary.sections[0].name, ".text");
        assert_eq!(summary.exports, vec!["FooExport"]);
        assert_eq!(summary.imports.len(), 1);
        assert!(summary.imports[0].delay_loaded);
        assert_eq!(summary.imports[0].name, "d3d12_xs.dll");
        assert_eq!(summary.imports[0].functions, vec!["D3D12XboxCreateDevice"]);
    }
}
