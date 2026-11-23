#![allow(non_snake_case)]
use std::ffi::c_void;

const S_OK: i32 = 0;

#[no_mangle]
pub unsafe extern "system" fn XDspActivate(_a0: usize, _a1: usize) -> i32 {
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XDspConnectWithMaximumStreamLimit(_a0: usize, _a1: usize, _a2: usize) -> i32 {
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XDspDeactivate(_a0: usize) -> i32 {
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XDspDisconnect(_a0: usize) -> i32 {
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XDspSubmitCommand(_a0: usize, _a1: *const c_void) -> i32 {
    S_OK
}
