//! Scarlett Audio Control Processor HAL (`AcpHal.dll`) host shim.
//!
//! The console exposes SHAPE/XMA hardware contexts through this library. No such hardware exists
//! on a Windows host, so creation reports `E_NOTIMPL` and middleware (e.g. FMOD) falls back or
//! disables hardware decode. APU memory helpers are backed by host virtual memory.
#![allow(non_snake_case)]
use std::ffi::c_void;

const S_OK: i32 = 0;
const E_NOTIMPL: i32 = 0x8000_4001u32 as i32;
const E_POINTER: i32 = 0x8000_4003u32 as i32;
const E_OUTOFMEMORY: i32 = 0x8007_000Eu32 as i32;

#[cfg(windows)]
extern "system" {
    fn VirtualAlloc(addr: *mut c_void, size: usize, ty: u32, protect: u32) -> *mut c_void;
    fn VirtualFree(addr: *mut c_void, size: usize, ty: u32) -> i32;
}

#[no_mangle]
pub unsafe extern "system" fn AcpHalCreate(out: *mut *mut c_void) -> i32 {
    eprintln!("[apu] AcpHalCreate out={:?}", out);
    if out.is_null() {
        return E_POINTER;
    }
    *out = acp_object();
    S_OK
}

// ---- host ACP object --------------------------------------------------------------------------
// Command/message channel as FMOD drives it:
//   slot 0  Initialize(cmd_queue, msg_queue, ...)
//   slot 2  Send(cmd, seq, -1, payload, 0)       seq != 0 asks for an acknowledgement
//   slot 3  Receive(msg*) -> bool                msg: +0 type, +4 error, +0x10 seq
// Messages: type 0x10 = command acknowledged (seq), type 2 = audio frame (every 2.667 ms; the
// mixer advances one block per four). No hardware: commands are acknowledged at once.
static SEEN: [std::sync::atomic::AtomicBool; 32] = [const { std::sync::atomic::AtomicBool::new(false) }; 32];
fn acp_note(slot: usize, a: [usize; 4]) {
    if !SEEN[slot].swap(true, std::sync::atomic::Ordering::Relaxed) {
        eprintln!("[apu] IAcpHal slot {slot} args=({:#x},{:#x},{:#x},{:#x})", a[0], a[1], a[2], a[3]);
    }
}
struct Chan {
    acks: std::collections::VecDeque<u64>,
    last_frame: Option<std::time::Instant>,
}
static CHAN: std::sync::Mutex<Chan> = std::sync::Mutex::new(Chan { acks: std::collections::VecDeque::new(), last_frame: None });
const FRAME: std::time::Duration = std::time::Duration::from_micros(2667);

unsafe extern "system" fn acp_send(_this: usize, cmd: u32, seq: u64, a: usize, b: usize) -> i32 {
    acp_note(2, [cmd as usize, seq as usize, a, b]);
    if seq != 0 {
        CHAN.lock().unwrap().acks.push_back(seq);
    }
    S_OK
}
unsafe extern "system" fn acp_receive(_this: usize, msg: *mut u8, b: usize, c: usize, d: usize) -> i32 {
    acp_note(3, [msg as usize, b, c, d]);
    if msg.is_null() {
        return 0;
    }
    let mut ch = CHAN.lock().unwrap();
    let (ty, seq) = if let Some(s) = ch.acks.pop_front() {
        (0x10u32, s)
    } else {
        let now = std::time::Instant::now();
        let last = *ch.last_frame.get_or_insert(now);
        if now.duration_since(last) < FRAME {
            return 0;
        }
        // catch up at most a few frames after a stall
        let next = last + FRAME;
        ch.last_frame = Some(if now.duration_since(next) > FRAME * 8 { now } else { next });
        (2u32, 0)
    };
    std::ptr::write_bytes(msg, 0, 0x20);
    *(msg as *mut u32) = ty;
    *(msg.add(0x10) as *mut u64) = seq;
    1
}
macro_rules! acp_slots {
    ($($name:ident = $n:literal),*) => {
        $(unsafe extern "system" fn $name(_this: usize, a: usize, b: usize, c: usize, d: usize) -> i32 { acp_note($n, [a, b, c, d]); S_OK })*
    };
}
acp_slots!(a0 = 0, a1 = 1, a4 = 4, a5 = 5, a6 = 6, a7 = 7, a8 = 8, a9 = 9, a10 = 10, a11 = 11, a12 = 12, a13 = 13, a14 = 14, a15 = 15);
type Slot = usize;
static ACP_VTBL: std::sync::OnceLock<[Slot; 16]> = std::sync::OnceLock::new();

unsafe fn acp_object() -> *mut c_void {
    let v = ACP_VTBL.get_or_init(|| {
        [a0 as usize, a1 as usize, acp_send as usize, acp_receive as usize, a4 as usize, a5 as usize, a6 as usize, a7 as usize,
         a8 as usize, a9 as usize, a10 as usize, a11 as usize, a12 as usize, a13 as usize, a14 as usize, a15 as usize]
    });
    #[repr(C)]
    struct Obj {
        vtbl: *const Slot,
        pad: [u64; 63],
    }
    Box::into_raw(Box::new(Obj { vtbl: v.as_ptr(), pad: [0; 63] })) as *mut c_void
}

#[no_mangle]
pub unsafe extern "system" fn AcpHalAllocateShapeContexts(desc: *mut c_void, b: usize, c: usize, d: usize) -> i32 {
    eprintln!("[apu] AcpHalAllocateShapeContexts {desc:?} {b:#x} {c:#x} {d:#x}");
    if desc.is_null() {
        return E_POINTER;
    }
    // Five u32 counts at +0x00..+0x10, matching context-array pointers at +0x18..+0x38
    // (observed element sizes 32/?/?/16/64 bytes; 64 is used as an upper bound).
    let d = desc as *mut u8;
    for i in 0..5 {
        let n = *(d.add(i * 4) as *const u32) as usize;
        let slot = d.add(0x18 + i * 8) as *mut *mut c_void;
        *slot = if n == 0 {
            std::ptr::null_mut()
        } else {
            VirtualAlloc(std::ptr::null_mut(), n * 64, 0x3000, 0x04)
        };
    }
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn AcpHalReleaseShapeContexts() -> i32 {
    eprintln!("[apu] AcpHalReleaseShapeContexts ");
    S_OK
}

/// `ApuAlloc(void** va, UINT32* pa, UINT32 size, UINT32 alignment, ...)`: host-backed allocation.
#[no_mangle]
pub unsafe extern "system" fn ApuAlloc(va: *mut *mut c_void, pa: *mut u32, size: u32, _align: u32) -> i32 {
    eprintln!("[apu] ApuAlloc va={:?} pa={:?} size={:?}", va, pa, size);
    if va.is_null() {
        return E_POINTER;
    }
    let p = VirtualAlloc(std::ptr::null_mut(), (size as usize).max(1), 0x3000, 0x04);
    if p.is_null() {
        return E_OUTOFMEMORY;
    }
    *va = p;
    if !pa.is_null() {
        *pa = p as usize as u32;
    }
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn ApuFree(va: *mut c_void) -> i32 {
    eprintln!("[apu] ApuFree va={:?}", va);
    if !va.is_null() {
        VirtualFree(va, 0, 0x8000);
    }
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn ApuIsVirtualAddressValid(va: *const c_void, _pa: u32) -> u8 {
    eprintln!("[apu] ApuIsVirtualAddressValid va={:?}", va);
    (!va.is_null()) as u8
}

#[no_mangle]
pub unsafe extern "system" fn ApuMapVirtualAddress(va: *const c_void) -> u32 {
    eprintln!("[apu] ApuMapVirtualAddress va={:?}", va);
    va as usize as u32
}
