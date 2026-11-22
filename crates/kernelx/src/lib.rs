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
