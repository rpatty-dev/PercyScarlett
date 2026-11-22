use std::ffi::c_void;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Mutex;

static LOG_LOCK: Mutex<()> = Mutex::new(());

fn log_call(msg: &str) {
    let Ok(path) = std::env::var("PERCY_XMEM_LOG") else {
        return;
    };
    if path.is_empty() {
        return;
    }
    let _g = LOG_LOCK.lock();
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{msg}");
    }
}

#[repr(C)]
pub struct XMemWorkingSetStats {
    pub size: u32,
    pub flags: u32,
    pub available_bytes: u64,
    pub total_bytes: u64,
    pub reserved: [u64; 5],
}

#[cfg(windows)]
extern "system" {
    fn VirtualAlloc(addr: *mut c_void, size: usize, alloc_type: u32, protect: u32) -> *mut c_void;
    fn VirtualFree(addr: *mut c_void, size: usize, free_type: u32) -> i32;
    fn VirtualQueryEx(proc_handle: usize, addr: *const c_void, buf: *mut u8, len: usize) -> usize;
    fn GetCurrentProcess() -> usize;
}

#[no_mangle]
pub unsafe extern "system" fn XMemAlloc(size: usize, attributes: u64) -> *mut c_void {
    let aligned = size.max(1);
    #[cfg(windows)]
    let ptr = {
        const MEM_COMMIT_RESERVE: u32 = 0x3000;
        const PAGE_READWRITE: u32 = 0x04;
        VirtualAlloc(std::ptr::null_mut(), aligned, MEM_COMMIT_RESERVE, PAGE_READWRITE)
    };
    #[cfg(not(windows))]
    let ptr = std::ptr::null_mut();

    log_call(&format!("XMemAlloc(size=0x{size:x}, attr=0x{attributes:x}) -> 0x{:x}", ptr as usize));
    ptr
}

#[no_mangle]
pub unsafe extern "system" fn XMemFree(ptr: *mut c_void, attributes: u64) {
    log_call(&format!("XMemFree(ptr=0x{:x}, attr=0x{attributes:x})", ptr as usize));
    if ptr.is_null() {
        return;
    }
    #[cfg(windows)]
    {
        const MEM_RELEASE: u32 = 0x8000;
        VirtualFree(ptr, 0, MEM_RELEASE);
    }
}

#[no_mangle]
pub unsafe extern "system" fn XMemGetWorkingSetStatistics(stats: *mut XMemWorkingSetStats) -> i32 {
    if stats.is_null() {
        return 0;
    }
    (*stats).size = std::mem::size_of::<XMemWorkingSetStats>() as u32;
    (*stats).flags = 0;
    (*stats).available_bytes = 4 * 1024 * 1024 * 1024;
    (*stats).total_bytes = 8 * 1024 * 1024 * 1024;
    (*stats).reserved = [0; 5];
    log_call("XMemGetWorkingSetStatistics -> TRUE (4 GiB avail / 8 GiB total)");
    1
}

/// Scarlett XMemVirtualQuery takes (process, address, out_64bytes).
#[no_mangle]
pub unsafe extern "system" fn XMemVirtualQuery(
    process: usize,
    address: *const c_void,
    out_info: *mut [u8; 64],
) -> usize {
    if out_info.is_null() {
        return 0;
    }
    *out_info = [0u8; 64];
    #[cfg(windows)]
    let ret = {
        let proc_h = if process == 0 { GetCurrentProcess() } else { process };
        // MEMORY_BASIC_INFORMATION is 48 bytes on x64; zero-padded to 64 bytes.
        VirtualQueryEx(proc_h, address, (*out_info).as_mut_ptr(), 48)
    };
    #[cfg(not(windows))]
    let ret = 0;

    log_call(&format!(
        "XMemVirtualQuery(proc=0x{process:x}, addr=0x{:x}) -> 0x{ret:x}",
        address as usize
    ));
    ret
}

#[no_mangle]
pub unsafe extern "system" fn XMemAllocDefault(size: usize, attributes: u64) -> *mut c_void {
    XMemAlloc(size, attributes)
}

#[no_mangle]
pub unsafe extern "system" fn XMemFreeDefault(ptr: *mut c_void, attributes: u64) {
    XMemFree(ptr, attributes)
}

#[no_mangle]
pub unsafe extern "system" fn XMemAllocPhysical(size: usize, attributes: u64) -> *mut c_void {
    XMemAlloc(size, attributes)
}

#[no_mangle]
pub unsafe extern "system" fn XMemFreePhysical(ptr: *mut c_void, attributes: u64) {
    XMemFree(ptr, attributes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xmem_alloc_free_and_stats_work() {
        unsafe {
            let mut stats = std::mem::zeroed::<XMemWorkingSetStats>();
            assert_eq!(XMemGetWorkingSetStatistics(&mut stats), 1);
            assert!(stats.available_bytes > 0);
            if cfg!(windows) {
                let p = XMemAlloc(4096, 0);
                assert!(!p.is_null());
                let mut mbi = [0xAAu8; 64];
                let q = XMemVirtualQuery(0, p, &mut mbi);
                assert!(q > 0);
                XMemFree(p, 0);
            }
        }
    }
}
