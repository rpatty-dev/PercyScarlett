//! Scarlett APU command-queue (`xapu.dll`) host shim.
//!
//! No audio processing unit is present on a Windows host: connecting reports `E_NOTIMPL` so
//! callers take their software path instead of dereferencing a fake queue.
#![allow(non_snake_case)]
use std::ffi::c_void;

const S_OK: i32 = 0;
const E_NOTIMPL: i32 = 0x8000_4001u32 as i32;

#[no_mangle]
pub unsafe extern "system" fn XApuConnect(desc: *const c_void, out: *mut *mut c_void, handle: *mut *mut c_void) -> i32 {
    // Caller memcpys into *out: allocate >= max(0x104000, cfg[+0xC]) zeroed bytes.
    let want = if desc.is_null() { 0 } else { *((desc as *const u8).add(0xC) as *const u32) as usize };
    let size = want.max(0x104000);
    let mem = std::alloc::alloc_zeroed(std::alloc::Layout::from_size_align(size, 0x1000).unwrap()) as *mut c_void;
    if !out.is_null() {
        *out = mem;
    }
    if !handle.is_null() {
        *handle = 0x4150_0001usize as *mut c_void;
    }
    eprintln!("[apu] XApuConnect size={size:#x}");
    0
}

#[no_mangle]
pub unsafe extern "system" fn XApuDisconnect(_h: *mut c_void) -> i32 {
    eprintln!("[apu] XApuDisconnect ");
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XApuEnqueueCommand(_h: *mut c_void, _cmd: *const c_void) -> i32 {
    eprintln!("[apu] XApuEnqueueCommand ");
    E_NOTIMPL
}

#[no_mangle]
pub unsafe extern "system" fn XApuDequeueResult(_h: *mut c_void, _res: *mut c_void) -> i32 {
    eprintln!("[apu] XApuDequeueResult ");
    E_NOTIMPL
}

/// Results pending for the title's APU queue: none (no host APU work is queued).
#[no_mangle]
pub unsafe extern "system" fn XApuGetResultCount(_a: usize, _b: usize) -> u32 {
    0
}

// ---- chad.dll (XDsp*): console audio DSP. No host DSP; calls succeed as no-ops. ----
#[no_mangle]
/// Writes a non-null handle through the pointer argument (title NULL-checks it).
pub unsafe extern "system" fn XDspActivate(a: usize, b: usize, c: usize, d: usize) -> i32 {
    for p in [a, b, c, d] {
        // Handle out-pointer: a writable stack/heap address holding 0.
        if p > 0x10000 && p & 7 == 0 && *(p as *const usize) == 0 {
            *(p as *mut usize) = 0x4453_0001;
            break;
        }
    }
    eprintln!("[xdsp] XDspActivate");
    0
}

/// (dsp, streams, flags, maxStreams, 1, out handle) — call site checks eax == 0.
#[no_mangle]
pub unsafe extern "system" fn XDspConnectWithMaximumStreamLimit(_a: usize, _b: u32, _c: u32, _d: u32, _e: u32, out: *mut usize) -> i32 {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0x5000);
    if !out.is_null() {
        *out = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    eprintln!("[xdsp] XDspConnectWithMaximumStreamLimit");
    0
}

#[no_mangle]
pub unsafe extern "system" fn XDspDeactivate(_a: usize) -> i32 {
    0
}

#[no_mangle]
pub unsafe extern "system" fn XDspDisconnect(_a: usize) -> i32 {
    0
}

#[no_mangle]
pub unsafe extern "system" fn XDspSubmitCommand(_a: usize, _b: usize, _c: usize, _d: usize) -> i32 {
    0
}
