use std::ffi::c_void;

#[cfg(windows)]
extern "system" {
    fn VirtualAlloc(addr: *mut c_void, size: usize, alloc_type: u32, protect: u32) -> *mut c_void;
    fn VirtualFree(addr: *mut c_void, size: usize, free_type: u32) -> i32;
}

#[repr(C)]
pub struct TitleMemoryStatus {
    pub length: u32,
    pub reserved: u32,
    pub total_mem: u64,
    pub avail_mem: u64,
    pub legacy_used: u64,
    pub title_used: u64,
    pub title_peak: u64,
}

#[no_mangle]
pub unsafe extern "system" fn XMemAlloc(size: usize, _attributes: u64) -> *mut c_void {
    #[cfg(windows)]
    {
        VirtualAlloc(std::ptr::null_mut(), size.max(1), 0x3000, 0x04)
    }
    #[cfg(not(windows))]
    {
        let _ = size;
        std::ptr::null_mut()
    }
}

#[no_mangle]
pub unsafe extern "system" fn XMemFree(ptr: *mut c_void, _attributes: u64) {
    #[cfg(windows)]
    if !ptr.is_null() {
        VirtualFree(ptr, 0, 0x8000);
    }
    #[cfg(not(windows))]
    let _ = ptr;
}

#[no_mangle]
pub unsafe extern "system" fn XMemAllocPhysical(size: usize, attributes: u64) -> *mut c_void {
    XMemAlloc(size, attributes)
}

#[no_mangle]
pub unsafe extern "system" fn XMemFreePhysical(ptr: *mut c_void, attributes: u64) {
    XMemFree(ptr, attributes)
}

#[no_mangle]
pub unsafe extern "system" fn TitleMemoryStatus(status: *mut TitleMemoryStatus) -> i32 {
    if status.is_null() {
        return 0;
    }
    (*status).length = std::mem::size_of::<TitleMemoryStatus>() as u32;
    (*status).reserved = 0;
    (*status).total_mem = 10 * 1024 * 1024 * 1024;
    (*status).avail_mem = 8 * 1024 * 1024 * 1024;
    (*status).legacy_used = 0;
    (*status).title_used = 0;
    (*status).title_peak = 0;
    1
}

#[no_mangle]
pub unsafe extern "system" fn JobTitleMemoryStatus(status: *mut TitleMemoryStatus) -> i32 {
    TitleMemoryStatus(status)
}

#[no_mangle]
pub unsafe extern "system" fn SetThreadName(_thread: usize, _name: *const u16) -> i32 {
    0
}

#[no_mangle]
pub unsafe extern "system" fn RegisterAppStateChangeNotification(
    _routine: *const c_void,
    _context: *const c_void,
    out_handle: *mut usize,
) -> i32 {
    if !out_handle.is_null() {
        *out_handle = 0x5045_5243;
    }
    0 // S_OK
}

#[no_mangle]
pub unsafe extern "system" fn UnregisterAppStateChangeNotification(_handle: usize) {}

#[no_mangle]
pub unsafe extern "system" fn RegisterAppConstrainedChangeNotification(
    _routine: *const c_void,
    _context: *const c_void,
    out_handle: *mut usize,
) -> i32 {
    if !out_handle.is_null() {
        *out_handle = 0x5045_5244;
    }
    0 // S_OK
}

#[no_mangle]
pub unsafe extern "system" fn UnregisterAppConstrainedChangeNotification(_handle: usize) {}

#[no_mangle]
pub unsafe extern "system" fn XMemGetAuxiliaryTitleMemory(
    size: usize,
    _flags: u64,
    out_ptr: *mut *mut c_void,
) -> i32 {
    if out_ptr.is_null() {
        return 0x8000_4003u32 as i32;
    }
    *out_ptr = XMemAlloc(size, 0);
    if (*out_ptr).is_null() { 0x8007_000Eu32 as i32 } else { 0 }
}

#[no_mangle]
pub unsafe extern "system" fn AllocateTitlePhysicalPages(
    _process: usize,
    _flags: u32,
    num_pages: usize,
    _page_size: usize,
    out_pfn: *mut u64,
) -> i32 {
    if !out_pfn.is_null() {
        for i in 0..num_pages {
            *out_pfn.add(i) = 0x1000 + i as u64;
        }
    }
    1
}

#[no_mangle]
pub unsafe extern "system" fn FreeTitlePhysicalPages(
    _process: usize,
    _num_pages: usize,
    _pfns: *const u64,
) -> i32 {
    1
}

#[no_mangle]
pub unsafe extern "system" fn WaitOnAddress(
    address: *const c_void,
    compare_address: *const c_void,
    address_size: usize,
    _dw_milliseconds: u32,
) -> i32 {
    if address.is_null() || compare_address.is_null() {
        return 0;
    }
    let equal = match address_size {
        1 => *(address as *const u8) == *(compare_address as *const u8),
        2 => *(address as *const u16) == *(compare_address as *const u16),
        4 => *(address as *const u32) == *(compare_address as *const u32),
        8 => *(address as *const u64) == *(compare_address as *const u64),
        _ => false,
    };
    // Wake* are no-ops here, so never block: yield and report a (spec-allowed) spurious wake.
    if equal {
        std::thread::yield_now();
    }
    1
}

#[no_mangle]
pub unsafe extern "system" fn WakeByAddressSingle(_address: *const c_void) {}

#[no_mangle]
pub unsafe extern "system" fn WakeByAddressAll(_address: *const c_void) {}

#[no_mangle]
pub unsafe extern "system" fn SetThreadAffinityMask(_thread: usize, mask: usize) -> usize {
    mask.max(1)
}

#[no_mangle]
pub unsafe extern "system" fn SetThreadPriority(_thread: usize, _priority: i32) -> i32 {
    1
}

pub fn rewrite_xbox_path(raw: &str) -> String {
    let trimmed = raw.trim();
    for (drive, env_k) in [
        ("G:\\", "PERCY_VFS_G"),
        ("g:\\", "PERCY_VFS_G"),
        ("T:\\", "PERCY_VFS_T"),
        ("t:\\", "PERCY_VFS_T"),
        ("S:\\", "PERCY_VFS_S"),
        ("s:\\", "PERCY_VFS_S"),
    ] {
        if let Some(rest) = trimmed.strip_prefix(drive) {
            if let Ok(root) = std::env::var(env_k) {
                if !root.is_empty() {
                    return format!("{}\\{}", root.trim_end_matches(['\\', '/']), rest);
                }
            }
        }
    }
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_memory_status_works() {
        unsafe {
            let mut st = std::mem::zeroed::<TitleMemoryStatus>();
            assert_eq!(TitleMemoryStatus(&mut st), 1);
            assert!(st.total_mem >= st.avail_mem && st.avail_mem > 0);
        }
    }
}
