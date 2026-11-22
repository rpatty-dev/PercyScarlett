#![allow(non_snake_case)]
use std::ffi::c_void;

const E_POINTER: i32 = 0x8000_4003u32 as i32;
const E_NOINTERFACE: i32 = 0x8000_4002u32 as i32;

#[no_mangle]
pub unsafe extern "system" fn GameInputCreate(out_input: *mut *mut c_void) -> i32 {
    if out_input.is_null() {
        return E_POINTER;
    }
    *out_input = std::ptr::null_mut();
    E_NOINTERFACE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gameinput_null_guard() {
        unsafe {
            assert_eq!(GameInputCreate(std::ptr::null_mut()), E_POINTER);
        }
    }
}
