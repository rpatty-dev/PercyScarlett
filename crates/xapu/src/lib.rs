//! Scarlett APU command-queue (`xapu.dll`) host shim.
//!
//! No audio processing unit is present on a Windows host: connecting reports `E_NOTIMPL` so
//! callers take their software path instead of dereferencing a fake queue.
#![allow(non_snake_case)]
use std::ffi::c_void;

const S_OK: i32 = 0;
const E_NOTIMPL: i32 = 0x8000_4001u32 as i32;

#[no_mangle]
pub unsafe extern "system" fn XApuConnect(_desc: *const c_void, out: *mut *mut c_void, _ctx: *mut c_void) -> i32 {
    eprintln!("[apu] XApuConnect out={:?}", out);
    if !out.is_null() {
        *out = std::ptr::null_mut();
    }
    E_NOTIMPL
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
