use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};

const S_OK: i32 = 0;
const E_POINTER: i32 = 0x8000_4003u32 as i32;

#[repr(C)]
pub struct XgFormatBlockInfo {
    pub block_width: u32,
    pub block_height: u32,
    pub bytes_per_block: u32,
}

#[repr(C)]
struct XgComputerVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const c_void, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
}

#[repr(C)]
struct XgComputer {
    vtbl: *const XgComputerVtbl,
    refs: AtomicU32,
}

unsafe extern "system" fn xg_qi(this: *mut c_void, _riid: *const c_void, out: *mut *mut c_void) -> i32 {
    if out.is_null() {
        return E_POINTER;
    }
    xg_add_ref(this);
    *out = this;
    S_OK
}
unsafe extern "system" fn xg_add_ref(this: *mut c_void) -> u32 {
    (*(this as *const XgComputer)).refs.fetch_add(1, Ordering::Relaxed) + 1
}
unsafe extern "system" fn xg_release(this: *mut c_void) -> u32 {
    let prev = (*(this as *const XgComputer)).refs.fetch_sub(1, Ordering::Release);
    if prev == 1 {
        std::sync::atomic::fence(Ordering::Acquire);
        drop(Box::from_raw(this as *mut XgComputer));
    }
    prev - 1
}

static XG_VTBL: XgComputerVtbl = XgComputerVtbl {
    query_interface: xg_qi,
    add_ref: xg_add_ref,
    release: xg_release,
};

#[no_mangle]
pub unsafe extern "system" fn XGGetFormatBlockInfo(
    dxgi_format: u32,
    out_info: *mut XgFormatBlockInfo,
) -> i32 {
    if out_info.is_null() {
        return E_POINTER;
    }
    let (bw, bh, bpb) = match dxgi_format {
        70..=72 | 79..=81 => (4, 4, 8),           // BC1, BC4
        73..=78 | 82..=84 | 94..=99 => (4, 4, 16), // BC2, BC3, BC5, BC6H, BC7
        1..=4 => (1, 1, 16),                     // R32G32B32A32
        9..=14 => (1, 1, 8),                     // R16G16B16A16
        27..=32 | 39..=46 | 67 | 87..=91 => (1, 1, 4), // R8G8B8A8, D32F, D24S8, RGB9E5, B8G8R8A8
        48..=59 => (1, 1, 2),                    // R16, R8G8
        60..=65 => (1, 1, 1),                    // R8
        _ => (1, 1, 4),
    };
    *out_info = XgFormatBlockInfo {
        block_width: bw,
        block_height: bh,
        bytes_per_block: bpb,
    };
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XGCreateTexture2DComputer(
    _desc: *const c_void,
    out_computer: *mut *mut c_void,
) -> i32 {
    if out_computer.is_null() {
        return E_POINTER;
    }
    let c = Box::new(XgComputer {
        vtbl: &XG_VTBL,
        refs: AtomicU32::new(1),
    });
    *out_computer = Box::into_raw(c) as *mut c_void;
    S_OK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_block_info_and_computer_work() {
        unsafe {
            let mut info = std::mem::zeroed::<XgFormatBlockInfo>();
            assert_eq!(XGGetFormatBlockInfo(98, &mut info), S_OK); // BC7_UNORM
            assert_eq!((info.block_width, info.block_height, info.bytes_per_block), (4, 4, 16));

            let mut comp: *mut c_void = std::ptr::null_mut();
            assert_eq!(XGCreateTexture2DComputer(std::ptr::null(), &mut comp), S_OK);
            assert!(!comp.is_null());
            let vtbl = &*(*(comp as *const *const XgComputerVtbl));
            assert_eq!((vtbl.release)(comp), 0);
        }
    }
}
