use std::ffi::c_void;

const S_OK: i32 = 0;
const E_POINTER: i32 = 0x8000_4003u32 as i32;
const E_NOINTERFACE: i32 = 0x8000_4002u32 as i32;

#[no_mangle]
pub unsafe extern "system" fn DStorageSetConfiguration(_config: *const c_void) -> i32 {
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn DStorageSetConfiguration1(_config: *const c_void) -> i32 {
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn DStorageGetFactory(
    _riid: *const c_void,
    out_factory: *mut *mut c_void,
) -> i32 {
    if out_factory.is_null() {
        return E_POINTER;
    }
    *out_factory = std::ptr::null_mut();
    E_NOINTERFACE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dstorage_config_and_null_factory_guard() {
        unsafe {
            assert_eq!(DStorageSetConfiguration(std::ptr::null()), S_OK);
            assert_eq!(DStorageSetConfiguration1(std::ptr::null()), S_OK);
            assert_eq!(DStorageGetFactory(std::ptr::null(), std::ptr::null_mut()), E_POINTER);
        }
    }
}
