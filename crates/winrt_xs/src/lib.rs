use std::ffi::c_void;

const S_OK: i32 = 0;
const E_POINTER: i32 = 0x8000_4003u32 as i32;
const REGDB_E_CLASSNOTREG: i32 = 0x8004_0154u32 as i32;

#[repr(C)]
struct HStringHeader {
    magic: u32,
    len: u32,
}

const HSTRING_MAGIC: u32 = 0x5053_4853; // "PSHS"

#[no_mangle]
pub unsafe extern "system" fn RoInitialize(_init_type: u32) -> i32 {
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn RoUninitialize() {}

#[no_mangle]
pub unsafe extern "system" fn RoGetActivationFactory(
    _activatable_class_id: *mut c_void,
    _iid: *const c_void,
    out_factory: *mut *mut c_void,
) -> i32 {
    if !out_factory.is_null() {
        *out_factory = std::ptr::null_mut();
    }
    REGDB_E_CLASSNOTREG
}

#[no_mangle]
pub unsafe extern "system" fn WindowsCreateString(
    source: *const u16,
    length: u32,
    out_hstring: *mut *mut c_void,
) -> i32 {
    if out_hstring.is_null() {
        return E_POINTER;
    }
    if length == 0 || source.is_null() {
        *out_hstring = std::ptr::null_mut();
        return S_OK;
    }
    let slice = std::slice::from_raw_parts(source, length as usize);
    let total_u16 = (std::mem::size_of::<HStringHeader>() / 2) + slice.len() + 1;
    let mut buf = vec![0u16; total_u16];
    let hdr = buf.as_mut_ptr() as *mut HStringHeader;
    (*hdr).magic = HSTRING_MAGIC;
    (*hdr).len = length;
    let payload = buf.as_mut_ptr().add(std::mem::size_of::<HStringHeader>() / 2);
    std::ptr::copy_nonoverlapping(slice.as_ptr(), payload, slice.len());
    *out_hstring = Box::into_raw(buf.into_boxed_slice()) as *mut c_void;
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn WindowsDeleteString(hstring: *mut c_void) -> i32 {
    if hstring.is_null() {
        return S_OK;
    }
    let hdr = &*(hstring as *const HStringHeader);
    if hdr.magic == HSTRING_MAGIC {
        let total_u16 = (std::mem::size_of::<HStringHeader>() / 2) + hdr.len as usize + 1;
        let slice_ptr = std::ptr::slice_from_raw_parts_mut(hstring as *mut u16, total_u16);
        drop(Box::from_raw(slice_ptr));
    }
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn WindowsGetStringRawBuffer(
    hstring: *mut c_void,
    out_len: *mut u32,
) -> *const u16 {
    static EMPTY: [u16; 1] = [0];
    if hstring.is_null() {
        if !out_len.is_null() {
            *out_len = 0;
        }
        return EMPTY.as_ptr();
    }
    let hdr = &*(hstring as *const HStringHeader);
    if hdr.magic != HSTRING_MAGIC {
        if !out_len.is_null() {
            *out_len = 0;
        }
        return EMPTY.as_ptr();
    }
    if !out_len.is_null() {
        *out_len = hdr.len;
    }
    (hstring as *const u16).add(std::mem::size_of::<HStringHeader>() / 2)
}

#[no_mangle]
pub unsafe extern "system" fn WindowsGetStringLen(hstring: *mut c_void) -> u32 {
    let mut len = 0u32;
    WindowsGetStringRawBuffer(hstring, &mut len);
    len
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hstring_create_read_delete_roundtrip() {
        unsafe {
            assert_eq!(RoInitialize(1), S_OK);
            let utf16: Vec<u16> = "Scarlett".encode_utf16().collect();
            let mut hs: *mut c_void = std::ptr::null_mut();
            assert_eq!(WindowsCreateString(utf16.as_ptr(), utf16.len() as u32, &mut hs), S_OK);
            assert!(!hs.is_null());
            assert_eq!(WindowsGetStringLen(hs), 8);
            let mut out_len = 0u32;
            let raw = WindowsGetStringRawBuffer(hs, &mut out_len);
            assert_eq!(out_len, 8);
            assert_eq!(std::slice::from_raw_parts(raw, 8), utf16.as_slice());
            assert_eq!(WindowsDeleteString(hs), S_OK);
            RoUninitialize();
        }
    }
}
