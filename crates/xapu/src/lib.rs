//! Scarlett APU command-queue (`xapu.dll`) host shim.
#![allow(non_snake_case)]
use std::ffi::c_void;

const S_OK: i32 = 0;

static mut FAKE_APU_HANDLE: [u8; 4096] = [0; 4096];

#[no_mangle]
pub unsafe extern "system" fn XApuConnect(
    desc: *const c_void,
    out: *mut *mut c_void,
    ctx: *mut c_void,
) -> i32 {
    eprintln!("[apu] XApuConnect desc={:?} out={:?} ctx={:?}", desc, out, ctx);
    if !out.is_null() {
        *out = std::ptr::addr_of_mut!(FAKE_APU_HANDLE) as *mut c_void;
    }
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XApuDisconnect(h: *mut c_void) -> i32 {
    eprintln!("[apu] XApuDisconnect h={:?}", h);
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XApuEnqueueCommand(h: *mut c_void, cmd: *const c_void) -> i32 {
    eprintln!("[apu] XApuEnqueueCommand h={:?} cmd={:?}", h, cmd);
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XApuDequeueResult(h: *mut c_void, res: *mut c_void) -> i32 {
    eprintln!("[apu] XApuDequeueResult h={:?} res={:?}", h, res);
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XApuGetResultCount(h: *mut c_void) -> u32 {
    eprintln!("[apu] XApuGetResultCount h={:?}", h);
    0
}
