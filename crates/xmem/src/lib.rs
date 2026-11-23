use std::ffi::c_void;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::{Mutex, Once};

static LOG_LOCK: Mutex<()> = Mutex::new(());
static VEH_INIT: Once = Once::new();

#[cfg(windows)]
#[repr(C)]
struct ExceptionRecord {
    code: u32,
    flags: u32,
    record: *const ExceptionRecord,
    address: *const c_void,
    num_params: u32,
    _pad: u32,
    info: [usize; 15],
}

#[cfg(windows)]
#[repr(C)]
struct ExceptionPointers {
    record: *const ExceptionRecord,
    context: *const u8,
}

#[cfg(windows)]
unsafe extern "system" fn veh_handler(ep: *mut ExceptionPointers) -> i32 {
    if ep.is_null() || (*ep).record.is_null() {
        return 0;
    }
    let rec = &*(*ep).record;
    // Log fatal hardware faults; ignore SetThreadName (0x406D1388) and C++ EH (0xE06D7363).
    if matches!(rec.code, 0xC000_0005 | 0xC000_001D | 0xC000_0094 | 0xC000_00FD) {
        let log_path = std::env::var("PERCY_CRASH_LOG").unwrap_or_else(|_| "percy_crash.log".into());
        let ctx = (*ep).context;
        let (rip, rsp, rcx, rdx) = if !ctx.is_null() {
            // Win64 CONTEXT offsets: Rcx=0x80, Rdx=0x88, Rsp=0x98, Rip=0xF8
            let u64_at = |off: usize| -> u64 { std::ptr::read_unaligned(ctx.add(off) as *const u64) };
            (u64_at(0xF8), u64_at(0x98), u64_at(0x80), u64_at(0x88))
        } else {
            (0, 0, 0, 0)
        };
        let extra = if rec.code == 0xC000_0005 && rec.num_params >= 2 {
            format!(" op={} target=0x{:x}", rec.info[0], rec.info[1])
        } else {
            String::new()
        };
        let _g = LOG_LOCK.lock();
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(log_path) {
            let _ = writeln!(
                f,
                "[CRASH] code=0x{:08x} addr=0x{:x}{extra} RIP=0x{rip:x} RSP=0x{rsp:x} RCX=0x{rcx:x} RDX=0x{rdx:x}",
                rec.code, rec.address as usize
            );
        }
    }
    0 // EXCEPTION_CONTINUE_SEARCH
}

fn ensure_veh_installed() {
    #[cfg(windows)]
    VEH_INIT.call_once(|| unsafe {
        AddVectoredExceptionHandler(0, veh_handler);
    });
}

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
    pub budget_bytes: u64,
    pub used_bytes: u64,
}

#[cfg(windows)]
extern "system" {
    fn VirtualAlloc(addr: *mut c_void, size: usize, alloc_type: u32, protect: u32) -> *mut c_void;
    fn VirtualFree(addr: *mut c_void, size: usize, free_type: u32) -> i32;
    fn VirtualQueryEx(proc_handle: usize, addr: *const c_void, buf: *mut u8, len: usize) -> usize;
    fn GetCurrentProcess() -> usize;
    fn AddVectoredExceptionHandler(
        first: u32,
        handler: unsafe extern "system" fn(*mut ExceptionPointers) -> i32,
    ) -> *mut c_void;
}

#[cfg(windows)]
#[no_mangle]
pub unsafe extern "system" fn DllMain(_inst: *mut c_void, reason: u32, _res: *mut c_void) -> i32 {
    if reason == 1 {
        ensure_veh_installed();
    }
    1
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
pub unsafe extern "system" fn XMemGetWorkingSetStatistics(_category: u32, stats: *mut XMemWorkingSetStats) -> i32 {
    if stats.is_null() {
        return 0;
    }
    #[repr(C)]
    struct Pmc { cb: u32, faults: u32, peak_ws: usize, ws: usize, qpp: usize, qp: usize, qnpp: usize, qnp: usize, pagefile: usize, peak_pf: usize }
    #[cfg(windows)]
    extern "system" {
        fn K32GetProcessMemoryInfo(h: usize, pmc: *mut Pmc, cb: u32) -> i32;
    }
    let mut used = 0u64;
    #[cfg(windows)]
    {
        let mut pmc: Pmc = std::mem::zeroed();
        pmc.cb = std::mem::size_of::<Pmc>() as u32;
        if K32GetProcessMemoryInfo(GetCurrentProcess(), &mut pmc, pmc.cb) != 0 {
            used = pmc.pagefile as u64;
        }
    }
    // Observed title ABI: 16-byte output {budget, used}; Scarlett titles get ~13.5 GiB of game memory.
    (*stats).budget_bytes = 13824 * 1024 * 1024;
    (*stats).used_bytes = used;
    log_call(&format!("XMemGetWorkingSetStatistics -> TRUE (budget 13.5 GiB, used 0x{used:x})"));
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

/// Title/system memory budget transfer. Host memory is not partitioned, so this is a no-op success.
#[no_mangle]
pub unsafe extern "system" fn XMemTransferMemory(_from: u64, _bytes: u64) -> i32 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xmem_alloc_free_and_stats_work() {
        unsafe {
            let mut stats = std::mem::zeroed::<XMemWorkingSetStats>();
            assert_eq!(XMemGetWorkingSetStatistics(0x2000_0001, &mut stats), 1);
            assert!(stats.budget_bytes > 0);
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
