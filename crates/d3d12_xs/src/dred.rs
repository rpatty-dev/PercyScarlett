//! Device Removed Extended Data: forced on before device creation; a watcher thread polls
//! GetDeviceRemovedReason and, on the first removal, logs breadcrumbs (last executed op per list)
//! and the page-fault VA with nearby allocations.
use super::*;

const IID_SETTINGS: [u8; 16] = [0x1c, 0x48, 0xbc, 0x82, 0x9b, 0x6b, 0x30, 0x40, 0xae, 0xdb, 0x7e, 0xe3, 0xd1, 0xdf, 0x1e, 0x63];
const IID_DRED: [u8; 16] = [0x33, 0x1d, 0x93, 0x98, 0xe8, 0x5a, 0x91, 0x47, 0xaa, 0x3c, 0x1a, 0x73, 0xa2, 0x93, 0x4e, 0x71];

unsafe fn vt(o: *mut c_void, i: usize) -> usize { *(*(o as *const *const usize)).add(i) }

/// Before D3D12CreateDevice: breadcrumbs + page faults FORCED_ON.
pub(crate) unsafe fn enable() {
    let gdi = host_proc("D3D12GetDebugInterface");
    if gdi == 0 { return; }
    let gdi: unsafe extern "system" fn(*const u8, *mut *mut c_void) -> i32 = std::mem::transmute(gdi);
    let mut s = std::ptr::null_mut();
    let r = gdi(IID_SETTINGS.as_ptr(), &mut s);
    if r >= 0 && !s.is_null() {
        let set: unsafe extern "system" fn(*mut c_void, u32) = std::mem::transmute(vt(s, 3));
        set(s, 2);
        let set: unsafe extern "system" fn(*mut c_void, u32) = std::mem::transmute(vt(s, 4));
        set(s, 2);
    }
    log_d3d12(&format!("[dred] enable -> {:08x}", r as u32));
}

unsafe fn cstr(p: *const u8) -> String {
    if p.is_null() { return "-".into(); }
    std::ffi::CStr::from_ptr(p as *const i8).to_string_lossy().into_owned()
}

/// Start the removal watcher for the PC device.
pub(crate) unsafe fn watch(dev: *mut c_void) {
    let d = dev as usize;
    std::thread::spawn(move || unsafe {
        let dev = d as *mut c_void;
        let rr: unsafe extern "system" fn(*mut c_void) -> i32 = std::mem::transmute(vt(dev, 37));
        let t0 = std::time::Instant::now();
        loop {
            std::thread::sleep(std::time::Duration::from_millis(20));
            let r = rr(dev);
            if r == 0 { continue; }
            log_d3d12(&format!("[dred] device removed {:08x} at {} ms", r as u32, t0.elapsed().as_millis()));
            dump(dev);
            return;
        }
    });
}

unsafe fn dump(dev: *mut c_void) {
    let qi: unsafe extern "system" fn(*mut c_void, *const u8, *mut *mut c_void) -> i32 = std::mem::transmute(vt(dev, 0));
    let mut x = std::ptr::null_mut();
    if qi(dev, IID_DRED.as_ptr(), &mut x) < 0 || x.is_null() {
        log_d3d12("[dred] no DRED interface");
        return;
    }
    let mut head: usize = 0;
    let gb: unsafe extern "system" fn(*mut c_void, *mut usize) -> i32 = std::mem::transmute(vt(x, 3));
    let hr = gb(x, &mut head);
    log_d3d12(&format!("[dred] breadcrumbs -> {:08x}", hr as u32));
    let mut n = head as *const u8;
    let mut k = 0;
    while !n.is_null() && k < 64 {
        let count = *(n.add(48) as *const u32);
        let last_p = *(n.add(56) as *const *const u32);
        let last = if last_p.is_null() { 0 } else { *last_p };
        let hist = *(n.add(64) as *const *const u32);
        // Only lists the GPU did not finish are interesting.
        if last < count && !hist.is_null() {
            let lo = last.saturating_sub(4);
            let hi = (last + 3).min(count);
            let ops: Vec<String> = (lo..hi).map(|i| format!("{}{}", if i == last { "*" } else { "" }, *hist.add(i as usize))).collect();
            log_d3d12(&format!("[dred] list={:#x} q={:#x} name={} done={last}/{count} ops[{lo}..]={}", *(n.add(32) as *const usize), *(n.add(40) as *const usize), cstr(*(n as *const *const u8)), ops.join(" ")));
        }
        n = *(n.add(72) as *const *const u8);
        k += 1;
    }
    let mut pf = [0usize; 3];
    let gp: unsafe extern "system" fn(*mut c_void, *mut [usize; 3]) -> i32 = std::mem::transmute(vt(x, 4));
    let hr = gp(x, &mut pf);
    log_d3d12(&format!("[dred] page fault va={:#x} -> {:08x}", pf[0], hr as u32));
    for (tag, h) in [("live", pf[1]), ("freed", pf[2])] {
        let mut a = h as *const u8;
        let mut k = 0;
        while !a.is_null() && k < 12 {
            log_d3d12(&format!("[dred] {tag} alloc type={} name={}", *(a.add(16) as *const u32), cstr(*(a as *const *const u8))));
            a = *(a.add(24) as *const *const u8);
            k += 1;
        }
    }
}
