//! Debug aid: hardware write-watch (DR0) on one address across all threads; the VEH logs the
//! writer's RIP. Enabled by the `percy_watch_fence` flag file.
use super::*;

#[repr(C, align(16))]
struct Ctx([u8; 1232]);
extern "system" {
    fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> isize;
    fn Thread32First(h: isize, e: *mut [u32; 7]) -> i32;
    fn Thread32Next(h: isize, e: *mut [u32; 7]) -> i32;
    fn OpenThread(access: u32, inherit: i32, tid: u32) -> isize;
    fn SuspendThread(h: isize) -> u32;
    fn ResumeThread(h: isize) -> u32;
    fn GetThreadContext(h: isize, c: *mut Ctx) -> i32;
    fn SetThreadContext(h: isize, c: *const Ctx) -> i32;
    fn CloseHandle(h: isize) -> i32;
    fn GetCurrentThreadId() -> u32;
    fn GetCurrentProcessId() -> u32;
    fn AddVectoredExceptionHandler(first: u32, h: unsafe extern "system" fn(*mut [*mut u8; 2]) -> i32) -> usize;
}

unsafe extern "system" fn veh(ep: *mut [*mut u8; 2]) -> i32 {
    let code = *((*ep)[0] as *const u32);
    let c = (*ep)[1];
    if code != 0x8000_0004 || *(c.add(0x68) as *const u64) & 1 == 0 {
        return 0;
    }
    static N: AtomicU32 = AtomicU32::new(0);
    let rip = *(c.add(0xf8) as *const usize);
    let a = *(c.add(0x48) as *const usize);
    if N.fetch_add(1, Ordering::Relaxed) < 12 {
        let rsp = *(c.add(0x98) as *const usize);
        let ras: Vec<String> = (0..64).map(|i| *((rsp as *const usize).add(i))).filter(|&v| (0x1_4000_1000..0x1_4c00_0000).contains(&v)).take(6).map(|v| format!("{:x}", v - 0x1_4000_0000)).collect();
        log_d3d12(&format!("[watch] write to {a:#x} now={:#x} rip={rip:#x} (G+{:x}) stack G+[{}]", *(a as *const u64), rip.wrapping_sub(0x1_4000_0000), ras.join(" ")));
    }
    *(c.add(0x68) as *mut u64) = 0;
    -1
}

/// Arm DR0 (write, 8 bytes) at `addr` on every thread of this process except the caller's helper.
pub(crate) fn arm(addr: usize) {
    static ONCE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if ONCE.swap(true, Ordering::Relaxed) {
        return;
    }
    let _ = std::thread::spawn(move || unsafe {
        AddVectoredExceptionHandler(1, veh);
        let me = GetCurrentThreadId();
        let pid = GetCurrentProcessId();
        let snap = CreateToolhelp32Snapshot(4, 0);
        let mut e = [0u32; 7];
        e[0] = 28;
        let mut ok = Thread32First(snap, &mut e);
        let mut n = 0;
        while ok != 0 {
            if e[3] == pid && e[2] != me {
                let h = OpenThread(0x1a, 0, e[2]); // SUSPEND_RESUME | GET_CONTEXT | SET_CONTEXT
                if h != 0 {
                    SuspendThread(h);
                    let mut c = Ctx([0; 1232]);
                    *(c.0.as_mut_ptr().add(0x30) as *mut u32) = 0x10_0010;
                    if GetThreadContext(h, &mut c) != 0 {
                        *(c.0.as_mut_ptr().add(0x48) as *mut usize) = addr;
                        let d7 = c.0.as_mut_ptr().add(0x70) as *mut u64;
                        *d7 = (*d7 & !0xf_0003) | 1 | (1 << 16) | (3 << 18);
                        if SetThreadContext(h, &c) != 0 { n += 1; }
                    }
                    ResumeThread(h);
                    CloseHandle(h);
                }
            }
            ok = Thread32Next(snap, &mut e);
        }
        CloseHandle(snap);
        log_d3d12(&format!("[watch] armed {addr:#x} on {n} threads"));
    })
    .join();
}
