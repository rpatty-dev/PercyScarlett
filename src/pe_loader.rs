use std::collections::HashMap;
use std::ffi::{c_void, CString};
use std::path::Path;

#[cfg(windows)]
extern "system" {
    fn VirtualAlloc(addr: usize, size: usize, alloc_type: u32, protect: u32) -> usize;
    fn VirtualProtect(addr: *mut c_void, size: usize, new_protect: u32, old_protect: *mut u32) -> i32;
    fn LoadLibraryA(name: *const i8) -> usize;
    fn GetProcAddress(module: usize, name: *const i8) -> usize;
    fn RtlAddFunctionTable(function_table: *const c_void, entry_count: u32, base_address: u64) -> u8;
    fn RtlPcToFileHeader(pc_value: *const c_void, base_of_image: *mut *mut c_void) -> *mut c_void;
    fn AddVectoredExceptionHandler(first: u32, handler: unsafe extern "system" fn(*const c_void) -> i32) -> *mut c_void;
    fn GetModuleFileNameW(module: usize, buf: *mut u16, size: u32) -> u32;
    fn GetModuleFileNameA(module: usize, buf: *mut u8, size: u32) -> u32;
    fn GetModuleHandleW(name: *const u16) -> usize;
    fn GetModuleHandleA(name: *const u8) -> usize;
}

static GUEST_EXE_PATH: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
static GUEST_IMAGE_BASE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub fn set_guest_exe_path(path: &Path) {
    if let Ok(mut g) = GUEST_EXE_PATH.lock() {
        *g = Some(path.to_string_lossy().into_owned());
    }
}

#[cfg(windows)]
unsafe extern "system" fn percy_get_module_handle_w(name: *const u16) -> usize {
    if name.is_null() {
        let b = GUEST_IMAGE_BASE.load(std::sync::atomic::Ordering::Acquire);
        if b != 0 {
            return b;
        }
    }
    GetModuleHandleW(name)
}

#[cfg(windows)]
unsafe extern "system" fn percy_get_module_handle_a(name: *const u8) -> usize {
    if name.is_null() {
        let b = GUEST_IMAGE_BASE.load(std::sync::atomic::Ordering::Acquire);
        if b != 0 {
            return b;
        }
    }
    GetModuleHandleA(name)
}

#[cfg(windows)]
unsafe extern "system" fn percy_get_module_file_name_w(module: usize, buf: *mut u16, size: u32) -> u32 {
    let gbase = GUEST_IMAGE_BASE.load(std::sync::atomic::Ordering::Acquire);
    if (module == 0 || module == gbase) && !buf.is_null() && size > 0 {
        if let Ok(g) = GUEST_EXE_PATH.lock() {
            if let Some(ref p) = *g {
                let mut w: Vec<u16> = p.encode_utf16().collect();
                let cap = (size as usize).saturating_sub(1);
                w.truncate(cap);
                std::ptr::copy_nonoverlapping(w.as_ptr(), buf, w.len());
                *buf.add(w.len()) = 0;
                return w.len() as u32;
            }
        }
    }
    GetModuleFileNameW(module, buf, size)
}

#[cfg(windows)]
unsafe extern "system" fn percy_get_module_file_name_a(module: usize, buf: *mut u8, size: u32) -> u32 {
    let gbase = GUEST_IMAGE_BASE.load(std::sync::atomic::Ordering::Acquire);
    if (module == 0 || module == gbase) && !buf.is_null() && size > 0 {
        if let Ok(g) = GUEST_EXE_PATH.lock() {
            if let Some(ref p) = *g {
                let bytes = p.as_bytes();
                let n = bytes.len().min((size as usize).saturating_sub(1));
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, n);
                *buf.add(n) = 0;
                return n as u32;
            }
        }
    }
    GetModuleFileNameA(module, buf, size)
}

#[cfg(windows)]
unsafe extern "system" fn trace_veh(info: *const c_void) -> i32 {
    let er = *(info as *const *const u32);
    let code = *er;
    let _addr = *(er.add(4) as *const usize);
    if code == 0xE06D_7363 || code == 0xC000_0005 {
        let ctx = *((info as *const *const u64).add(1));
        let _rip = *ctx.add(0xF8 / 8);
        let _rcx = *ctx.add(0x80 / 8);
        let _rdx = *ctx.add(0x88 / 8);
        let obj = *(er.add(10) as *const *const u64);
        let (q0, q1, _q2, q3) = if code == 0xE06D_7363 && !obj.is_null() {
            (*obj, *obj.add(1), *obj.add(2), *obj.add(3))
        } else {
            (0, 0, 0, 0)
        };
        let _msg = if code == 0xE06D_7363 && q1 > 0x10000 && (q1 >> 48) == 0 {
            std::ffi::CStr::from_ptr(q1 as *const i8).to_string_lossy().into_owned()
        } else {
            String::new()
        };
        eprintln!("[veh] code=0x{code:08x} rip=0x{_rip:x} rcx=0x{_rcx:x} rdx=0x{_rdx:x} vt=0x{q0:x} hr=0x{q3:08x}");
    }
    0 // EXCEPTION_CONTINUE_SEARCH
}

static MAPPED_RANGES: std::sync::Mutex<Vec<(usize, usize)>> = std::sync::Mutex::new(Vec::new());

/// Redirected `RtlPcToFileHeader` so MSVC C++ `throw` / SEH inside manually mapped PEs resolves `ImageBase`.
#[cfg(windows)]
unsafe extern "system" fn percy_rtl_pc_to_file_header(
    pc_value: *const c_void,
    base_of_image: *mut *mut c_void,
) -> *mut c_void {
    let pc = pc_value as usize;
    if let Ok(ranges) = MAPPED_RANGES.lock() {
        for &(start, end) in ranges.iter() {
            if pc >= start && pc < end {
                if !base_of_image.is_null() {
                    *base_of_image = start as *mut c_void;
                }
                return start as *mut c_void;
            }
        }
    }
    RtlPcToFileHeader(pc_value, base_of_image)
}

unsafe extern "system" fn unresolved_xbox_stub() -> i32 {
    0
}

fn ru16(b: &[u8], o: usize) -> u16 {
    b.get(o..o + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
        .unwrap_or(0)
}
fn ru32(b: &[u8], o: usize) -> u32 {
    b.get(o..o + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .unwrap_or(0)
}
fn ru64(b: &[u8], o: usize) -> u64 {
    b.get(o..o + 8)
        .map(|s| u64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))
        .unwrap_or(0)
}

unsafe fn read_cstr_mem(ptr: *const u8) -> String {
    let mut len = 0usize;
    while len < 512 && *ptr.add(len) != 0 {
        len += 1;
    }
    String::from_utf8_lossy(std::slice::from_raw_parts(ptr, len)).into_owned()
}

#[derive(Debug, Clone)]
pub struct LoadedImageStats {
    pub mapped_base: u64,
    pub entry_va: u64,
    pub pdata_entries: u32,
    pub tls_callbacks: usize,
    pub static_tls_bytes: usize,
    pub resolved_imports: usize,
    pub stubbed_imports: usize,
    pub relocations_applied: usize,
    pub missing_imports: Vec<String>,
}

#[cfg(windows)]
fn resolve_shim_or_host_dll(dll_name: &str, shim_dir: &Path, cache: &mut HashMap<String, usize>) -> usize {
    let key = dll_name.to_ascii_lowercase();
    if let Some(&h) = cache.get(&key) {
        return h;
    }
    let shim_names = [
        "xmem.dll",
        "kernelx.dll",
        "d3d12_xs.dll",
        "xg_xs.dll",
        "xgameruntime.dll",
        "dstorage.dll",
        "gameinput.dll",
        "winrt_xs.dll",
        "xaudio2_9.dll",
    ];
    let mut handle = 0usize;
    if shim_names.contains(&key.as_str()) {
        let p = shim_dir.join(dll_name);
        if let Ok(cs) = CString::new(p.to_string_lossy().as_bytes()) {
            handle = unsafe { LoadLibraryA(cs.as_ptr()) };
        }
    }
    if handle == 0 {
        if let Ok(cs) = CString::new(dll_name) {
            handle = unsafe { LoadLibraryA(cs.as_ptr()) };
        }
    }
    if handle == 0 && (key == "kernelx.dll" || key == "kernel32.dll") {
        let k32 = CString::new("kernel32.dll").unwrap();
        handle = unsafe { LoadLibraryA(k32.as_ptr()) };
    }
    cache.insert(key, handle);
    handle
}

/// Maps a PE32+ executable in-process, applies `.reloc` base relocations, resolves IAT imports
/// (prioritizing PercyScarlett shim DLLs), registers `.pdata` unwind entries via `RtlAddFunctionTable`,
/// invokes TLS callbacks, and executes `AddressOfEntryPoint`.
#[cfg(windows)]
pub fn load_and_run_pe64(data: &[u8], shim_dir: &Path, execute_entry: bool) -> Result<(LoadedImageStats, i32), String> {
    if data.len() < 0x100 || &data[0..2] != b"MZ" {
        return Err("Not an MZ executable".into());
    }
    let pe_off = ru32(data, 0x3C) as usize;
    if data.get(pe_off..pe_off + 4) != Some(b"PE\0\0") {
        return Err("Missing PE signature".into());
    }
    let machine = ru16(data, pe_off + 4);
    if machine != 0x8664 {
        return Err(format!("Unsupported machine 0x{machine:04x}"));
    }
    let n_sec = ru16(data, pe_off + 6) as usize;
    let opt_sz = ru16(data, pe_off + 20) as usize;
    let opt_off = pe_off + 24;
    if ru16(data, opt_off) != 0x020B {
        return Err("Expected PE32+ (0x020B)".into());
    }

    let entry_rva = ru32(data, opt_off + 16);
    let pref_base = ru64(data, opt_off + 24);
    let size_of_image = ru32(data, opt_off + 56) as usize;
    let size_of_headers = ru32(data, opt_off + 60) as usize;
    let num_rvas = ru32(data, opt_off + 108) as usize;
    let dd = |idx: usize| -> (u32, u32) {
        if idx < num_rvas {
            let o = opt_off + 112 + idx * 8;
            (ru32(data, o), ru32(data, o + 4))
        } else {
            (0, 0)
        }
    };

    unsafe {
        let mut base = VirtualAlloc(pref_base as usize, size_of_image, 0x3000, 0x04) as *mut c_void;
        if base.is_null() {
            base = VirtualAlloc(0, size_of_image, 0x3000, 0x04) as *mut c_void;
        }
        if base.is_null() {
            return Err("VirtualAlloc failed for PE32+ image".into());
        }
        let base_u8 = base as *mut u8;
        let hdr_copy = size_of_headers.min(data.len()).min(size_of_image);
        std::ptr::copy_nonoverlapping(data.as_ptr(), base_u8, hdr_copy);

        let sec_off = opt_off + opt_sz;
        let mut sec_protects = Vec::with_capacity(n_sec);
        for i in 0..n_sec {
            let o = sec_off + i * 40;
            let vsize = ru32(data, o + 8) as usize;
            let vaddr = ru32(data, o + 12) as usize;
            let rsize = ru32(data, o + 16) as usize;
            let rptr = ru32(data, o + 20) as usize;
            let chars = ru32(data, o + 36);
            if vaddr < size_of_image && rsize > 0 && rptr + rsize <= data.len() {
                let n = rsize.min(size_of_image - vaddr);
                std::ptr::copy_nonoverlapping(data.as_ptr().add(rptr), base_u8.add(vaddr), n);
            }
            sec_protects.push((vaddr, vsize.max(rsize), chars));
        }

        // Base relocations (.reloc, directory index 5)
        let delta = (base as u64).wrapping_sub(pref_base) as i64;
        let mut relocations_applied = 0usize;
        let (reloc_rva, reloc_sz) = dd(5);
        if delta != 0 && reloc_rva != 0 && reloc_sz >= 8 {
            let mut off = 0usize;
            while off + 8 <= reloc_sz as usize {
                let blk_rva = *(base_u8.add(reloc_rva as usize + off) as *const u32);
                let blk_sz = *(base_u8.add(reloc_rva as usize + off + 4) as *const u32) as usize;
                if blk_sz < 8 {
                    break;
                }
                let count = (blk_sz - 8) / 2;
                let entries = base_u8.add(reloc_rva as usize + off + 8) as *const u16;
                for j in 0..count {
                    let ent = *entries.add(j);
                    let ty = ent >> 12;
                    let r_off = (ent & 0x0FFF) as usize;
                    if ty == 10 {
                        // IMAGE_REL_BASED_DIR64
                        let p = base_u8.add(blk_rva as usize + r_off) as *mut u64;
                        *p = (*p as i64).wrapping_add(delta) as u64;
                        relocations_applied += 1;
                    }
                }
                off += blk_sz;
            }
        }

        // IAT import resolution (directory index 1)
        let mut dll_cache: HashMap<String, usize> = HashMap::new();
        let mut resolved_imports = 0usize;
        let mut stubbed_imports = 0usize;
        let mut missing_imports: Vec<String> = Vec::new();
        let (imp_rva, _) = dd(1);
        if imp_rva != 0 {
            let mut desc_off = imp_rva as usize;
            while desc_off + 20 <= size_of_image {
                let oft = *(base_u8.add(desc_off) as *const u32);
                let name_rva = *(base_u8.add(desc_off + 12) as *const u32);
                let ft = *(base_u8.add(desc_off + 16) as *const u32);
                if name_rva == 0 {
                    break;
                }
                let dll_name = read_cstr_mem(base_u8.add(name_rva as usize));
                let hmod = resolve_shim_or_host_dll(&dll_name, shim_dir, &mut dll_cache);
                let int_rva = if oft != 0 { oft } else { ft } as usize;
                let iat_rva = ft as usize;
                let mut idx = 0usize;
                while iat_rva + idx * 8 + 8 <= size_of_image {
                    let thunk = *(base_u8.add(int_rva + idx * 8) as *const u64);
                    if thunk == 0 {
                        break;
                    }
                    let mut addr = 0usize;
                    let mut sym_label = String::new();
                    if (thunk & (1u64 << 63)) != 0 {
                        let ord = (thunk & 0xFFFF) as usize;
                        if hmod != 0 {
                            addr = GetProcAddress(hmod, ord as *const i8);
                        }
                        if addr == 0 {
                            sym_label = format!("{dll_name}!#{ord}");
                        }
                    } else {
                        let hint_rva = (thunk & 0x7FFF_FFFF) as usize;
                        if hint_rva + 2 < size_of_image {
                            let fn_name_ptr = base_u8.add(hint_rva + 2) as *const i8;
                            if hmod != 0 {
                                let name_str = read_cstr_mem(fn_name_ptr as *const u8);
                                addr = match name_str.as_str() {
                                    "RtlPcToFileHeader" => percy_rtl_pc_to_file_header as *const () as usize,
                                    "GetModuleFileNameW" => percy_get_module_file_name_w as *const () as usize,
                                    "GetModuleFileNameA" => percy_get_module_file_name_a as *const () as usize,
                                    "GetModuleHandleW" => percy_get_module_handle_w as *const () as usize,
                                    "GetModuleHandleA" => percy_get_module_handle_a as *const () as usize,
                                    _ => GetProcAddress(hmod, fn_name_ptr),
                                };
                                if addr == 0 {
                                    sym_label = format!("{dll_name}!{name_str}");
                                }
                            } else {
                                let s = read_cstr_mem(fn_name_ptr as *const u8);
                                sym_label = format!("{dll_name}!{s}");
                            }
                        }
                    }
                    if addr != 0 {
                        resolved_imports += 1;
                    } else {
                        addr = unresolved_xbox_stub as *const () as usize;
                        stubbed_imports += 1;
                        if !sym_label.is_empty() && !missing_imports.contains(&sym_label) {
                            missing_imports.push(sym_label);
                        }
                    }
                    *(base_u8.add(iat_rva + idx * 8) as *mut usize) = addr;
                    idx += 1;
                }
                desc_off += 20;
            }
        }

        // Apply section protections
        for (vaddr, vsize, chars) in sec_protects {
            if vsize == 0 || vaddr >= size_of_image {
                continue;
            }
            let exec = (chars & 0x2000_0000) != 0;
            let write = (chars & 0x8000_0000) != 0;
            let prot = match (exec, write) {
                (true, true) => 0x40,   // PAGE_EXECUTE_READWRITE
                (true, false) => 0x20,  // PAGE_EXECUTE_READ
                (false, true) => 0x04,  // PAGE_READWRITE
                (false, false) => 0x02, // PAGE_READONLY
            };
            let mut old = 0u32;
            VirtualProtect(base_u8.add(vaddr) as *mut c_void, vsize, prot, &mut old);
        }

        if let Ok(mut ranges) = MAPPED_RANGES.lock() {
            ranges.push((base as usize, base as usize + size_of_image));
        }

        // Register sanitized .pdata SEH table (directory index 3)
        let (pdata_rva, pdata_sz) = dd(3);
        let pdata_entries = if pdata_rva != 0 && pdata_sz >= 12 {
            let raw_count = (pdata_sz / 12) as usize;
            let raw_ptr = base_u8.add(pdata_rva as usize) as *const [u32; 3];
            let mut valid: Vec<[u32; 3]> = (0..raw_count)
                .map(|i| *raw_ptr.add(i))
                .filter(|&[begin, end, _]| begin < end && (end as usize) <= size_of_image)
                .collect();
            valid.sort_unstable_by_key(|e| e[0]);
            let count = valid.len() as u32;
            if count > 0 {
                // Keep sanitized table alive in mapped image when possible, else leak boxed slice for process lifetime
                let tbl = valid.into_boxed_slice();
                let ptr = Box::into_raw(tbl) as *const c_void;
                let _ = RtlAddFunctionTable(ptr, count, base as u64);
            }
            count
        } else {
            0
        };

        // Initialize static .tls block + TEB gs:[0x58] slot & run TLS callbacks (directory index 9)
        let mut tls_callbacks = 0usize;
        let mut static_tls_bytes = 0usize;
        let mut saved_teb_tls: Option<*mut *mut u8> = None;
        let (tls_rva, tls_sz) = dd(9);
        if tls_rva != 0 && tls_sz >= 40 {
            let tls_dir = base_u8.add(tls_rva as usize) as *const u64;
            let raw_start = *tls_dir.add(0);
            let raw_end = *tls_dir.add(1);
            let addr_of_index = *tls_dir.add(2);
            let cb_va = *tls_dir.add(3);
            let zero_fill = (*((tls_dir.add(4)) as *const u32)) as usize;
            let raw_len = raw_end.saturating_sub(raw_start) as usize;
            let total_tls = raw_len + zero_fill;
            if total_tls > 0 && total_tls <= 16 * 1024 * 1024 {
                static NEXT_TLS_SLOT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(16);
                let slot = NEXT_TLS_SLOT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if (slot as usize) < 64 {
                    if addr_of_index != 0 {
                        *(addr_of_index as *mut u32) = slot;
                    }
                    let block = VirtualAlloc(0, total_tls, 0x3000, 0x04) as *mut u8;
                    if !block.is_null() {
                        if raw_len > 0 && raw_start != 0 {
                            std::ptr::copy_nonoverlapping(raw_start as *const u8, block, raw_len);
                        }
                        #[cfg(target_arch = "x86_64")]
                        {
                            let mut teb_tls: *mut *mut u8;
                            std::arch::asm!("mov {}, gs:[0x58]", out(reg) teb_tls);
                            saved_teb_tls = Some(teb_tls);
                            let new_tbl = VirtualAlloc(0, 64 * 8, 0x3000, 0x04) as *mut *mut u8;
                            if !new_tbl.is_null() {
                                if !teb_tls.is_null() {
                                    let page_rem = (0x1000 - (teb_tls as usize & 0xFFF)) / 8;
                                    let copy_slots = page_rem.min(16).max(1);
                                    std::ptr::copy_nonoverlapping(teb_tls, new_tbl, copy_slots);
                                }
                                *new_tbl.add(slot as usize) = block;
                                std::arch::asm!("mov gs:[0x58], {}", in(reg) new_tbl);
                            }
                        }
                        static_tls_bytes = total_tls;
                    }
                }
            }
            if cb_va != 0 {
                let mut cb_ptr = cb_va as *const u64;
                while *cb_ptr != 0 && tls_callbacks < 64 {
                    let cb: unsafe extern "system" fn(*mut c_void, u32, *mut c_void) =
                        std::mem::transmute(*cb_ptr as usize);
                    cb(base, 1 /* DLL_PROCESS_ATTACH */, std::ptr::null_mut());
                    tls_callbacks += 1;
                    cb_ptr = cb_ptr.add(1);
                }
            }
        }

        let entry_va = (base as u64).wrapping_add(entry_rva as u64);
        GUEST_IMAGE_BASE.store(base as usize, std::sync::atomic::Ordering::Release);
        if execute_entry && std::env::var("PERCY_TRACE").is_ok() {
            AddVectoredExceptionHandler(1, trace_veh);
        }
        let rc = if execute_entry && entry_rva != 0 {
            let entry_fn: unsafe extern "system" fn() -> i32 = std::mem::transmute(entry_va as usize);
            entry_fn()
        } else {
            0
        };
        #[cfg(target_arch = "x86_64")]
        if let Some(orig) = saved_teb_tls {
            std::arch::asm!("mov gs:[0x58], {}", in(reg) orig);
        }

        Ok((
            LoadedImageStats {
                mapped_base: base as u64,
                entry_va,
                pdata_entries,
                tls_callbacks,
                static_tls_bytes,
                resolved_imports,
                stubbed_imports,
                relocations_applied,
                missing_imports,
            },
            rc,
        ))
    }
}

/// Launches a target executable via Windows `CreateProcessW(CREATE_SUSPENDED)` with `shim_dir`
/// prepended to `PATH` and `PERCY_VFS_G` pointed at the executable's directory, then resumes it.
#[cfg(windows)]
pub fn launch_suspended_with_overlay(exe_path: &Path, shim_dir: &Path) -> Result<u32, String> {
    use std::os::windows::process::CommandExt;
    let exe_dir = exe_path.parent().unwrap_or_else(|| Path::new("."));
    let old_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{};{};{old_path}", shim_dir.display(), exe_dir.display());
    let child = std::process::Command::new(exe_path)
        .current_dir(exe_dir)
        .env("PATH", new_path)
        .env("PERCY_VFS_G", exe_dir)
        .env("PERCY_D3D12_WRAP", "1")
        .creation_flags(0x0000_0000)
        .spawn()
        .map_err(|e| format!("Failed to spawn {}: {e}", exe_path.display()))?;
    Ok(child.id())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn maps_relocates_registers_pdata_and_executes_pe64_in_process() {
        // Build a minimal valid PE32+ in memory with .text (`mov eax, 42; ret`), .pdata, and .reloc
        let mut pe = vec![0u8; 0x2000];
        pe[0..2].copy_from_slice(b"MZ");
        pe[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        pe[0x80..0x84].copy_from_slice(b"PE\0\0");
        pe[0x84..0x86].copy_from_slice(&0x8664u16.to_le_bytes());
        pe[0x86..0x88].copy_from_slice(&1u16.to_le_bytes());
        pe[0x94..0x96].copy_from_slice(&0xF0u16.to_le_bytes());
        pe[0x98..0x9A].copy_from_slice(&0x020Bu16.to_le_bytes());
        pe[0xA8..0xAC].copy_from_slice(&0x1000u32.to_le_bytes()); // entry RVA
        pe[0xB0..0xB8].copy_from_slice(&0x1_4000_0000u64.to_le_bytes()); // ImageBase
        pe[0xD0..0xD4].copy_from_slice(&0x2000u32.to_le_bytes()); // SizeOfImage
        pe[0xD4..0xD8].copy_from_slice(&0x200u32.to_le_bytes()); // SizeOfHeaders
        pe[0x104..0x108].copy_from_slice(&16u32.to_le_bytes());

        // .pdata at RVA 0x1020 (size 12): 1 RUNTIME_FUNCTION entry
        pe[0x120..0x124].copy_from_slice(&0x1020u32.to_le_bytes());
        pe[0x124..0x128].copy_from_slice(&12u32.to_le_bytes());

        // Section .text at VA=0x1000, RawPtr=0x1000, RawSize=0x1000, RX
        pe[0x188..0x18D].copy_from_slice(b".text");
        pe[0x190..0x194].copy_from_slice(&0x1000u32.to_le_bytes());
        pe[0x194..0x198].copy_from_slice(&0x1000u32.to_le_bytes());
        pe[0x198..0x19C].copy_from_slice(&0x1000u32.to_le_bytes());
        pe[0x19C..0x1A0].copy_from_slice(&0x1000u32.to_le_bytes());
        pe[0x1AC..0x1B0].copy_from_slice(&0x6000_0020u32.to_le_bytes());

        // x64 code at RVA 0x1000 (file 0x1000): mov eax, 42; ret
        pe[0x1000..0x1006].copy_from_slice(&[0xB8, 42, 0, 0, 0, 0xC3]);
        // RUNTIME_FUNCTION at 0x1020: Begin=0x1000, End=0x1006, Unwind=0x1030
        pe[0x1020..0x1024].copy_from_slice(&0x1000u32.to_le_bytes());
        pe[0x1024..0x1028].copy_from_slice(&0x1006u32.to_le_bytes());
        pe[0x1028..0x102C].copy_from_slice(&0x1030u32.to_le_bytes());
        pe[0x1030] = 0x01; // UNWIND_INFO version=1, flags=0

        let (stats, rc) = load_and_run_pe64(&pe, Path::new("."), true).expect("in-process PE64 run");
        assert_eq!(rc, 42);
        assert_eq!(stats.pdata_entries, 1);
        assert!(stats.missing_imports.is_empty());
    }

    #[test]
    fn maps_static_tls_and_sanitizes_pdata() {
        let mut pe = vec![0u8; 0x3000];
        pe[0..2].copy_from_slice(b"MZ");
        pe[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        pe[0x80..0x84].copy_from_slice(b"PE\0\0");
        pe[0x84..0x86].copy_from_slice(&0x8664u16.to_le_bytes());
        pe[0x86..0x88].copy_from_slice(&1u16.to_le_bytes());
        pe[0x94..0x96].copy_from_slice(&0xF0u16.to_le_bytes());
        pe[0x98..0x9A].copy_from_slice(&0x020Bu16.to_le_bytes());
        pe[0xA8..0xAC].copy_from_slice(&0x1000u32.to_le_bytes()); // entry RVA
        pe[0xB0..0xB8].copy_from_slice(&0x1_4000_0000u64.to_le_bytes()); // ImageBase
        pe[0xD0..0xD4].copy_from_slice(&0x3000u32.to_le_bytes()); // SizeOfImage
        pe[0xD4..0xD8].copy_from_slice(&0x200u32.to_le_bytes()); // SizeOfHeaders
        pe[0x104..0x108].copy_from_slice(&16u32.to_le_bytes());

        // .pdata at RVA 0x1020 (size 24): 1 valid + 1 invalid (begin >= end)
        pe[0x120..0x124].copy_from_slice(&0x1020u32.to_le_bytes());
        pe[0x124..0x128].copy_from_slice(&24u32.to_le_bytes());

        // .tls directory at RVA 0x2000 (size 40)
        pe[0x150..0x154].copy_from_slice(&0x2000u32.to_le_bytes());
        pe[0x154..0x158].copy_from_slice(&40u32.to_le_bytes());

        // Base relocation block at RVA 0x2100 (size 16) to relocate TLS raw_start, raw_end, addr_of_index
        pe[0x130..0x134].copy_from_slice(&0x2100u32.to_le_bytes());
        pe[0x134..0x138].copy_from_slice(&16u32.to_le_bytes());

        // Section .text at VA=0x1000, RawPtr=0x1000, RawSize=0x2000, RWX
        pe[0x188..0x18D].copy_from_slice(b".text");
        pe[0x190..0x194].copy_from_slice(&0x2000u32.to_le_bytes());
        pe[0x194..0x198].copy_from_slice(&0x1000u32.to_le_bytes());
        pe[0x198..0x19C].copy_from_slice(&0x2000u32.to_le_bytes());
        pe[0x19C..0x1A0].copy_from_slice(&0x1000u32.to_le_bytes());
        pe[0x1AC..0x1B0].copy_from_slice(&0xE000_0020u32.to_le_bytes());

        // mov eax, 7; ret
        pe[0x1000..0x1006].copy_from_slice(&[0xB8, 7, 0, 0, 0, 0xC3]);
        // Valid RUNTIME_FUNCTION
        pe[0x1020..0x1024].copy_from_slice(&0x1000u32.to_le_bytes());
        pe[0x1024..0x1028].copy_from_slice(&0x1006u32.to_le_bytes());
        pe[0x1028..0x102C].copy_from_slice(&0x1040u32.to_le_bytes());
        // Invalid RUNTIME_FUNCTION (begin == end)
        pe[0x102C..0x1030].copy_from_slice(&0x1050u32.to_le_bytes());
        pe[0x1030..0x1034].copy_from_slice(&0x1050u32.to_le_bytes());
        pe[0x1040] = 0x01;

        // IMAGE_TLS_DIRECTORY64 at 0x2000: raw 0x2040..0x2050 (16B), index at 0x2060, zero_fill=16
        pe[0x2000..0x2008].copy_from_slice(&0x1_4000_2040u64.to_le_bytes());
        pe[0x2008..0x2010].copy_from_slice(&0x1_4000_2050u64.to_le_bytes());
        pe[0x2010..0x2018].copy_from_slice(&0x1_4000_2060u64.to_le_bytes());
        pe[0x2020..0x2024].copy_from_slice(&16u32.to_le_bytes());

        // Reloc block at 0x2100: page RVA=0x2000, size=16, 3x DIR64 (0xA000 | off) + pad
        pe[0x2100..0x2104].copy_from_slice(&0x2000u32.to_le_bytes());
        pe[0x2104..0x2108].copy_from_slice(&16u32.to_le_bytes());
        pe[0x2108..0x210A].copy_from_slice(&0xA000u16.to_le_bytes());
        pe[0x210A..0x210C].copy_from_slice(&0xA008u16.to_le_bytes());
        pe[0x210C..0x210E].copy_from_slice(&0xA010u16.to_le_bytes());

        let (stats, rc) = load_and_run_pe64(&pe, Path::new("."), true).expect("in-process TLS PE64 run");
        assert_eq!(rc, 7);
        assert_eq!(stats.pdata_entries, 1);
        assert_eq!(stats.static_tls_bytes, 32);

        let mut out_base: *mut c_void = std::ptr::null_mut();
        let ret = unsafe { percy_rtl_pc_to_file_header((stats.mapped_base + 0x1002) as *const c_void, &mut out_base) };
        assert_eq!(ret as u64, stats.mapped_base);
        assert_eq!(out_base as u64, stats.mapped_base);
    }
}
