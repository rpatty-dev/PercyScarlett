//! VRAM mirrors for console "UMA" buffers.
//!
//! Console titles Map() DEFAULT-heap buffers and write them from the CPU. Those buffers live on a
//! CPU-visible system-memory heap (see devx_create_committed_resource), which a discrete GPU
//! without resizable BAR reads over PCIe: geometry fetch stalls the shaders. Each such buffer
//! gets a DEFAULT-heap twin in VRAM that the GPU uses instead (its GPU VA, descriptors, copies,
//! barriers). CPU writes are tracked per 64 KiB page by write-protecting the mapping; dirty pages
//! are copied into the twin on the direct queue before the next submit.
use std::ffi::c_void;
use std::sync::Mutex;

#[link(name = "kernel32")]
extern "system" {
    fn VirtualProtect(addr: *mut c_void, size: usize, prot: u32, old: *mut u32) -> i32;
    fn VirtualQuery(addr: *const c_void, info: *mut [usize; 6], len: usize) -> usize;
    fn AddVectoredExceptionHandler(first: u32, h: unsafe extern "system" fn(*const c_void) -> i32) -> *mut c_void;
}

const PAGE: usize = 64 << 10;
const IID_DEV: [u8; 16] = [0xf1, 0x19, 0x98, 0x18, 0xb6, 0x1d, 0x57, 0x4b, 0xbe, 0x54, 0x18, 0x21, 0x33, 0x9b, 0x85, 0xf7];
const IID_ALLOC: [u8; 16] = [0xe4, 0xde, 0x02, 0x61, 0x59, 0xaf, 0x09, 0x4b, 0xb9, 0x99, 0xb4, 0x4d, 0x73, 0xf0, 0x9b, 0x24];
const IID_LIST: [u8; 16] = [0x0f, 0x0d, 0x16, 0x5b, 0x1b, 0xac, 0x85, 0x41, 0x8b, 0xa8, 0xb3, 0xae, 0x42, 0xa5, 0xa4, 0x55];
const IID_FENCE: [u8; 16] = [0xcf, 0x3d, 0x75, 0x0a, 0xd8, 0xc4, 0x91, 0x4b, 0xad, 0xf6, 0xbe, 0x5a, 0x60, 0xd9, 0x5a, 0x76];

struct Mirror {
    uma: usize,
    twin: usize,
    size: usize,
    cpu: usize,      // mapped CPU address (0 until first Map)
    prot_rw: u32,    // page protection with write access (keeps WRITE_COMBINE etc.)
    dirty: Vec<u64>, // bit per PAGE
    any: bool,
    untracked: bool,
}

static MIRRORS: Mutex<Vec<Mirror>> = Mutex::new(Vec::new());

unsafe fn vt(o: usize, slot: usize) -> usize {
    *(*(o as *const *const usize)).add(slot)
}
unsafe fn raw_va(res: usize) -> u64 {
    let f: unsafe extern "system" fn(usize) -> u64 = std::mem::transmute(vt(res, 11));
    f(res)
}

pub(crate) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !std::env::current_exe().map(|e| e.with_file_name("percy_no_mirror").exists()).unwrap_or(false))
}

/// Registers `uma` with a freshly created DEFAULT twin (both PC resources).
pub(crate) unsafe fn add(uma: *mut c_void, twin: *mut c_void, size: u64) {
    let pages = (size as usize).div_ceil(PAGE);
    let mut m = MIRRORS.lock().unwrap();
    // shortcut: freed buffers are never unregistered; an address reused by a new buffer replaces it.
    m.retain(|e| e.uma != uma as usize);
    m.push(Mirror { uma: uma as usize, twin: twin as usize, size: size as usize, cpu: 0, prot_rw: 0, dirty: vec![!0u64; pages.div_ceil(64)], any: true, untracked: false });
}

/// The twin standing in for `res` on the GPU, if any.
pub(crate) fn twin_of(res: *mut c_void) -> Option<*mut c_void> {
    if res.is_null() {
        return None;
    }
    MIRRORS.lock().unwrap().iter().find(|e| e.uma == res as usize).map(|e| e.twin as *mut c_void)
}
pub(crate) fn sub(res: *mut c_void) -> *mut c_void {
    twin_of(res).unwrap_or(res)
}

/// GetGPUVirtualAddress of a mirrored buffer: the twin's.
pub(crate) unsafe fn gpu_va(res: *mut c_void) -> Option<u64> {
    twin_of(res).map(|t| raw_va(t as usize))
}

/// After a successful Map: start write tracking on the mapping.
pub(crate) unsafe fn on_map(res: *mut c_void, ptr: *mut c_void) {
    let mut m = MIRRORS.lock().unwrap();
    let Some(e) = m.iter_mut().find(|e| e.uma == res as usize) else { return };
    if e.cpu != 0 || e.untracked || ptr.is_null() {
        return;
    }
    let mut info = [0usize; 6];
    if VirtualQuery(ptr, &mut info, std::mem::size_of_val(&info)) == 0 {
        return;
    }
    let rw = (info[4] >> 32) as u32; // MEMORY_BASIC_INFORMATION.Protect (byte 36)
    e.cpu = ptr as usize;
    e.prot_rw = rw;
    let mut old = 0;
    if VirtualProtect(ptr, e.size, (rw & !0xFF) | 0x02, &mut old) == 0 {
        e.cpu = 0;
        e.untracked = true; // cannot track writes: copy the whole buffer every flush
        super::log_d3d12(&format!("[mirror] cannot protect {ptr:p} (prot {rw:#x})"));
        return;
    }
    static VEH: std::sync::Once = std::sync::Once::new();
    VEH.call_once(|| {
        AddVectoredExceptionHandler(1, veh);
    });
}

unsafe extern "system" fn veh(info: *const c_void) -> i32 {
    let er = *(info as *const *const usize);
    if *(er as *const u32) != 0xC000_0005 || *(er as *const u32).add(6) < 2 {
        return 0;
    }
    // EXCEPTION_RECORD: NumberParameters @24, ExceptionInformation @32
    let kind = *er.add(4);
    let addr = *er.add(5);
    if kind != 1 {
        return 0;
    }
    let mut m = MIRRORS.lock().unwrap();
    let Some(e) = m.iter_mut().find(|e| e.cpu != 0 && addr >= e.cpu && addr < e.cpu + e.size) else { return 0 };
    let p = (addr - e.cpu) / PAGE;
    let mut old = 0;
    let len = PAGE.min(e.size - p * PAGE);
    if VirtualProtect((e.cpu + p * PAGE) as *mut c_void, len, e.prot_rw, &mut old) == 0 {
        return 0;
    }
    e.dirty[p / 64] |= 1 << (p % 64);
    e.any = true;
    -1 // EXCEPTION_CONTINUE_EXECUTION
}

struct Ring {
    alloc: [usize; 4],
    list: [usize; 4],
    wait: [u64; 4],
    fence: usize,
    next: u64,
}
static RING: Mutex<Option<Ring>> = Mutex::new(None);

unsafe fn make_ring(queue: *mut c_void) -> Option<Ring> {
    let gd: unsafe extern "system" fn(*mut c_void, *const u8, *mut usize) -> i32 = std::mem::transmute(super::console_list::queue_orig(queue, 7));
    let mut dev = 0usize;
    if gd(queue, IID_DEV.as_ptr(), &mut dev) < 0 || dev == 0 {
        return None;
    }
    let ca: unsafe extern "system" fn(usize, u32, *const u8, *mut usize) -> i32 = std::mem::transmute(vt(dev, 9));
    let cl: unsafe extern "system" fn(usize, u32, u32, usize, usize, *const u8, *mut usize) -> i32 = std::mem::transmute(vt(dev, 12));
    let cf: unsafe extern "system" fn(usize, u64, u32, *const u8, *mut usize) -> i32 = std::mem::transmute(vt(dev, 36));
    let mut r = Ring { alloc: [0; 4], list: [0; 4], wait: [0; 4], fence: 0, next: 1 };
    for i in 0..4 {
        if ca(dev, 0, IID_ALLOC.as_ptr(), &mut r.alloc[i]) < 0 || cl(dev, 0, 0, r.alloc[i], 0, IID_LIST.as_ptr(), &mut r.list[i]) < 0 {
            return None;
        }
        let close: unsafe extern "system" fn(usize) -> i32 = std::mem::transmute(vt(r.list[i], 9));
        close(r.list[i]);
    }
    if cf(dev, 0, 0, IID_FENCE.as_ptr(), &mut r.fence) < 0 {
        return None;
    }
    let rel: unsafe extern "system" fn(usize) -> u32 = std::mem::transmute(vt(dev, 2));
    rel(dev);
    Some(r)
}

/// Direct-queue submit hook: copy pages written since the last flush into the twins.
pub(crate) unsafe fn flush(queue: *mut c_void) {
    let mut m = MIRRORS.lock().unwrap();
    if !m.iter().any(|e| e.any || e.untracked) {
        return;
    }
    // D3D12_COMMAND_QUEUE_DESC via GetDesc (slot 18, struct return): only the direct queue.
    let gdesc: unsafe extern "system" fn(*mut c_void, *mut [u32; 4]) -> *mut [u32; 4] = std::mem::transmute(super::console_list::queue_orig(queue, 18));
    let mut d = [0u32; 4];
    gdesc(queue, &mut d);
    if d[0] != 0 {
        return;
    }
    let mut ring = RING.lock().unwrap();
    if ring.is_none() {
        *ring = make_ring(queue);
    }
    let Some(r) = ring.as_mut() else { return };
    let i = (r.next % 4) as usize;
    let done: unsafe extern "system" fn(usize) -> u64 = std::mem::transmute(vt(r.fence, 8));
    if done(r.fence) < r.wait[i] {
        let set: unsafe extern "system" fn(usize, u64, usize) -> i32 = std::mem::transmute(vt(r.fence, 9));
        set(r.fence, r.wait[i], 0); // null event: blocks until reached
    }
    let (alloc, list) = (r.alloc[i], r.list[i]);
    let ar: unsafe extern "system" fn(usize) -> i32 = std::mem::transmute(vt(alloc, 8));
    ar(alloc);
    let reset: unsafe extern "system" fn(usize, usize, usize) -> i32 = std::mem::transmute(vt(list, 10));
    reset(list, alloc, 0);
    let copy: unsafe extern "system" fn(usize, usize, u64, usize, u64, u64) = std::mem::transmute(vt(list, 15));
    let mut bytes = 0usize;
    for e in m.iter_mut() {
        if !(e.any || e.untracked) {
            continue;
        }
        let pages = e.size.div_ceil(PAGE);
        let mut p = 0;
        while p < pages {
            let is_dirty = |q: usize| e.untracked || e.dirty[q / 64] & (1 << (q % 64)) != 0;
            if !is_dirty(p) {
                p += 1;
                continue;
            }
            let s = p;
            while p < pages && is_dirty(p) {
                p += 1;
            }
            let (off, len) = (s * PAGE, (p * PAGE).min(e.size) - s * PAGE);
            if e.cpu != 0 {
                let mut old = 0;
                VirtualProtect((e.cpu + off) as *mut c_void, len, (e.prot_rw & !0xFF) | 0x02, &mut old);
            }
            copy(list, e.twin, off as u64, e.uma, off as u64, len as u64);
            bytes += len;
        }
        e.dirty.iter_mut().for_each(|w| *w = 0);
        e.any = false;
    }
    drop(m);
    let close: unsafe extern "system" fn(usize) -> i32 = std::mem::transmute(vt(list, 9));
    close(list);
    let ecl: unsafe extern "system" fn(*mut c_void, u32, *const usize) = std::mem::transmute(super::console_list::queue_orig(queue, 10));
    ecl(queue, 1, &list);
    let sig: unsafe extern "system" fn(*mut c_void, usize, u64) -> i32 = std::mem::transmute(super::console_list::queue_orig(queue, 14));
    sig(queue, r.fence, r.next);
    r.wait[i] = r.next;
    r.next += 1;
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if n < 4 || n.is_power_of_two() {
        super::log_d3d12(&format!("[mirror] flush #{n}: {} KiB", bytes >> 10));
    }
}
