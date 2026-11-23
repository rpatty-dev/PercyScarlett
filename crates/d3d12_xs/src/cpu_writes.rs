// SPDX-License-Identifier: GPL-2.0-only
//! Console bottom-of-pipe writes and memory waits (list slots 71/72 and 73/74 in the newer layout).
//!
//! The title marks GPU progress with Write{32,64}BitValueBottomOfPipeX into its own memory and
//! waits on those values with Wait{32,64}BitValueX. The PC GPU cannot write there, so each queue
//! gets a fence: after a submission carrying such writes the queue signals it and a worker thread
//! does the writes, in order, once the GPU got there. Waits are evaluated on the CPU before the
//! submission. Model and slot meanings after xcl (github.com/ivall/xcl, GPL-2.0,
//! crates/xcl/src/hle/d3d12x/cpu_writes.rs and list_x.rs).

use super::*;
use std::collections::HashMap;
use std::sync::{mpsc, OnceLock};

extern "system" {
    fn VirtualQuery(p: *const c_void, mbi: *mut [usize; 6], n: usize) -> usize;
    fn CreateEventW(a: *const c_void, manual: i32, initial: i32, name: *const u16) -> usize;
    fn WaitForSingleObject(h: usize, ms: u32) -> u32;
    fn QueryPerformanceCounter(c: *mut i64) -> i32;
    fn QueryPerformanceFrequency(f: *mut i64) -> i32;
}

/// value written at completion: plain dwords, or the completion time (flags 1/2 of the 64-bit write)
#[derive(Clone)]
enum Val {
    Words(Vec<u32>),
    Timestamp,
}

#[derive(Clone, Copy)]
struct Wait {
    va: usize,
    width: u8,
    cmp: u32,
    value: u64,
    mask: u64,
}

#[derive(Default)]
struct ListState {
    writes: Vec<(usize, Val)>,
    waits: Vec<Wait>,
}

static LISTS: Mutex<Option<HashMap<usize, ListState>>> = Mutex::new(None);
/// queue -> (fence, last value)
static FENCES: Mutex<Option<HashMap<usize, (usize, u64)>>> = Mutex::new(None);
/// how long a write waits for its submission, and a wait for its value (removed device, a queue that never gets there)
const WAIT_MS: u32 = 10_000;

unsafe fn vt(o: usize, slot: usize) -> usize {
    *(*(o as *const *const usize)).add(slot)
}

/// committed, writable (or readable for `write == false`) memory covering `[va, va + n)`
fn usable(va: usize, n: usize, write: bool) -> bool {
    let mut m = [0usize; 6];
    if va < 0x10000 || unsafe { VirtualQuery(va as *const c_void, &mut m, 48) } == 0 {
        return false;
    }
    // MEMORY_BASIC_INFORMATION: BaseAddress @0, RegionSize @0x18, State @0x20, Protect @0x24
    let (state, prot) = (m[4] as u32, (m[4] >> 32) as u32);
    let ok = if write { prot & 0x44 != 0 } else { prot & 0x66 != 0 };
    state == 0x1000 && ok && m[0] + m[3] >= va + n
}

fn now_ns() -> u64 {
    let (mut c, mut f) = (0i64, 0i64);
    unsafe {
        QueryPerformanceCounter(&mut c);
        QueryPerformanceFrequency(&mut f);
    }
    (c as u128 * 1_000_000_000 / f.max(1) as u128) as u64
}

unsafe fn write_now(va: usize, v: &Val) {
    let words = match v {
        Val::Words(w) => w.clone(),
        Val::Timestamp => {
            let t = now_ns();
            vec![t as u32, (t >> 32) as u32]
        }
    };
    if !usable(va, 4 * words.len(), true) {
        return;
    }
    for (i, &w) in words.iter().enumerate() {
        (*((va + 4 * i) as *const AtomicU32)).store(w, Ordering::Release);
    }
}

fn record(list: *mut c_void, f: impl FnOnce(&mut ListState)) {
    f(LISTS.lock().unwrap().get_or_insert_with(HashMap::new).entry(list as usize).or_default());
}

/// List Reset: forget what the previous recording carried.
pub(crate) fn reset(list: *mut c_void) {
    if let Some(m) = LISTS.lock().unwrap().as_mut() {
        m.remove(&(list as usize));
    }
}

static N_WARN: AtomicU32 = AtomicU32::new(0);
fn warn(msg: String) {
    if N_WARN.fetch_add(1, Ordering::Relaxed) < 12 {
        log_d3d12(&format!("[cpuw] {msg}"));
    }
}

/// Slot 71: Write32BitValueBottomOfPipeX(va, value, flush, flags)
pub(crate) unsafe extern "system" fn write32_bottom(list: *mut c_void, va: u64, value: u32, _flush: u32, _flags: u32) {
    super::replay::flush(list);
    if va % 4 != 0 || !usable(va as usize, 4, true) {
        return warn(format!("Write32BottomOfPipe {va:#x} not writable"));
    }
    record(list, |s| s.writes.push((va as usize, Val::Words(vec![value]))));
}

/// Slot 72: Write64BitValueBottomOfPipeX(va, value, flush, flags); flags 1/2 = a timestamp
pub(crate) unsafe extern "system" fn write64_bottom(list: *mut c_void, va: u64, value: u64, _flush: u32, flags: u32) {
    super::replay::flush(list);
    if va % 8 != 0 || !usable(va as usize, 8, true) {
        return warn(format!("Write64BottomOfPipe {va:#x} flags={flags:#x} not writable"));
    }
    let v = if flags & 3 != 0 { Val::Timestamp } else { Val::Words(vec![value as u32, (value >> 32) as u32]) };
    record(list, |s| s.writes.push((va as usize, v)));
}

unsafe fn wait(list: *mut c_void, w: Wait) {
    super::replay::flush(list);
    if w.va % w.width as usize != 0 || !usable(w.va, w.width as usize, false) {
        return warn(format!("Wait{}BitValue {:#x} not readable", 8 * w.width, w.va));
    }
    match w.cmp {
        8 => return,                                                  // ALWAYS
        1 => return warn(format!("Wait NEVER at {:#x} skipped", w.va)), // would hang
        2..=7 => {}
        c => return warn(format!("Wait unknown comparison {c}")),
    }
    record(list, |s| {
        // a write earlier in the same list orders it on the GPU already; waiting would deadlock on ourselves
        if !s.writes.iter().any(|&(a, _)| a == w.va) {
            s.waits.push(w);
        }
    });
}
/// Slot 73: Wait32BitValueX(va, comparison, value, mask, flags)
pub(crate) unsafe extern "system" fn wait32(list: *mut c_void, va: u64, cmp: u32, value: u32, mask: u32, _flags: u32) {
    wait(list, Wait { va: va as usize, width: 4, cmp, value: value as u64, mask: mask as u64 });
}
/// Slot 74: Wait64BitValueX(va, comparison, value, mask, flags)
pub(crate) unsafe extern "system" fn wait64(list: *mut c_void, va: u64, cmp: u32, value: u64, mask: u64, _flags: u32) {
    wait(list, Wait { va: va as usize, width: 8, cmp, value, mask });
}

fn holds(w: &Wait) -> bool {
    let cur = unsafe {
        if w.width == 4 { (*(w.va as *const AtomicU32)).load(Ordering::Acquire) as u64 } else { (*(w.va as *const std::sync::atomic::AtomicU64)).load(Ordering::Acquire) }
    };
    let (a, b) = (cur & w.mask, w.value & w.mask);
    match w.cmp {
        2 => a < b,
        3 => a == b,
        4 => a <= b,
        5 => a > b,
        6 => a != b,
        7 => a >= b,
        _ => true,
    }
}

/// Before `lists` go to a queue: the memory waits they recorded, on the CPU.
pub(crate) unsafe fn before_submit(lists: &[*mut c_void]) {
    let waits: Vec<Wait> = match LISTS.lock().unwrap().as_mut() {
        Some(m) => lists.iter().flat_map(|l| m.get_mut(&(*l as usize)).map(|s| std::mem::take(&mut s.waits)).unwrap_or_default()).collect(),
        None => return,
    };
    for w in waits {
        let t = std::time::Instant::now();
        while !holds(&w) {
            if t.elapsed().as_millis() > WAIT_MS as u128 {
                warn(format!("Wait at {:#x} cmp={} value={:#x} timed out", w.va, w.cmp, w.value));
                break;
            }
            std::thread::yield_now();
        }
    }
}

struct Job {
    fence: usize,
    value: u64,
    writes: Vec<(usize, Val)>,
}
unsafe impl Send for Job {}

fn worker() -> &'static Mutex<mpsc::Sender<Job>> {
    static W: OnceLock<Mutex<mpsc::Sender<Job>>> = OnceLock::new();
    W.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Job>();
        std::thread::spawn(move || unsafe {
            let ev = CreateEventW(std::ptr::null(), 0, 0, std::ptr::null());
            for j in rx {
                let done: unsafe extern "system" fn(usize) -> u64 = std::mem::transmute(vt(j.fence, 8));
                let on: unsafe extern "system" fn(usize, u64, usize) -> i32 = std::mem::transmute(vt(j.fence, 9));
                if done(j.fence) < j.value && on(j.fence, j.value, ev) >= 0 {
                    WaitForSingleObject(ev, WAIT_MS);
                }
                for (va, v) in &j.writes {
                    write_now(*va, v);
                }
            }
        });
        Mutex::new(tx)
    })
}

/// After `lists` went to PC queue `q`: their bottom-of-pipe writes, once the GPU finished them.
pub(crate) unsafe fn after_submit(q: *mut c_void, lists: &[*mut c_void]) {
    let writes: Vec<(usize, Val)> = match LISTS.lock().unwrap().as_mut() {
        Some(m) => lists.iter().flat_map(|l| m.get_mut(&(*l as usize)).map(|s| std::mem::take(&mut s.writes)).unwrap_or_default()).collect(),
        None => return,
    };
    if writes.is_empty() || q.is_null() {
        return;
    }
    let job = {
        let mut g = FENCES.lock().unwrap();
        let m = g.get_or_insert_with(HashMap::new);
        if !m.contains_key(&(q as usize)) {
            const IID_DEV: [u8; 16] = [0xf1, 0x19, 0x98, 0x18, 0xb6, 0x1d, 0x57, 0x4b, 0xbe, 0x54, 0x18, 0x21, 0x33, 0x9b, 0x85, 0xf7];
            const IID_FENCE: [u8; 16] = [0xcf, 0x3d, 0x75, 0x0a, 0xd8, 0xc4, 0x91, 0x4b, 0xad, 0xf6, 0xbe, 0x5a, 0x60, 0xd9, 0x5a, 0x76];
            let gd: unsafe extern "system" fn(*mut c_void, *const u8, *mut usize) -> i32 = std::mem::transmute(console_list::queue_orig(q, 7));
            let mut dev = 0usize;
            if gd(q, IID_DEV.as_ptr(), &mut dev) < 0 || dev == 0 {
                return;
            }
            let cf: unsafe extern "system" fn(usize, u64, u32, *const u8, *mut usize) -> i32 = std::mem::transmute(vt(dev, 36));
            let mut fence = 0usize;
            let hr = cf(dev, 0, 0, IID_FENCE.as_ptr(), &mut fence);
            let rel: unsafe extern "system" fn(usize) -> u32 = std::mem::transmute(vt(dev, 2));
            rel(dev);
            if hr < 0 || fence == 0 {
                return warn(format!("no fence for queue {q:p}: {:08x}", hr as u32));
            }
            m.insert(q as usize, (fence, 0));
        }
        let e = m.get_mut(&(q as usize)).unwrap();
        e.1 += 1;
        let sig: unsafe extern "system" fn(*mut c_void, usize, u64) -> i32 = std::mem::transmute(console_list::queue_orig(q, 14));
        if sig(q, e.0, e.1) < 0 {
            return;
        }
        Job { fence: e.0, value: e.1, writes }
    };
    static FIRST: std::sync::Once = std::sync::Once::new();
    FIRST.call_once(|| log_d3d12(&format!("[cpuw] bottom-of-pipe writes done by the CPU after the GPU ({} in the first submission)", job.writes.len())));
    let _ = worker().lock().unwrap().send(job);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_comparisons() {
        let cell = Box::new(5u64);
        let w = |cmp, value| Wait { va: &*cell as *const u64 as usize, width: 8, cmp, value, mask: !0 };
        assert!(holds(&w(7, 5)) && holds(&w(7, 4)) && !holds(&w(7, 6)));
        assert!(holds(&w(3, 5)) && !holds(&w(6, 5)) && holds(&w(2, 6)));
        assert!(usable(&*cell as *const u64 as usize, 8, true));
    }
}
