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

#[no_mangle]
pub unsafe extern "system" fn XGCreateTextureComputer(
    desc: *const c_void,
    out_computer: *mut *mut c_void,
) -> i32 {
    XGCreateTexture2DComputer(desc, out_computer)
}

#[no_mangle]
pub unsafe extern "system" fn XGSuggestSwizzleMode(
    _desc: *const c_void,
    out_swizzle: *mut u32,
) -> i32 {
    if !out_swizzle.is_null() {
        *out_swizzle = 0;
    }
    S_OK
}


#[repr(C)]
pub struct XgTexture2DLayout {
    pub row_pitch_bytes: u32,
    pub slice_pitch_bytes: u32,
    pub total_bytes: u64,
    pub tile_mode: u32,
}

#[no_mangle]
pub unsafe extern "system" fn XGComputeTexture2DLayout(
    width: u32,
    height: u32,
    dxgi_format: u32,
    tile_mode: u32,
    out_layout: *mut XgTexture2DLayout,
) -> i32 {
    if out_layout.is_null() {
        return E_POINTER;
    }
    let mut info = XgFormatBlockInfo { block_width: 1, block_height: 1, bytes_per_block: 4 };
    XGGetFormatBlockInfo(dxgi_format, &mut info);
    let bw = (width.max(1) + info.block_width - 1) / info.block_width;
    let bh = (height.max(1) + info.block_height - 1) / info.block_height;
    let row_pitch = ((bw * info.bytes_per_block + 255) / 256) * 256;
    let slice_pitch = row_pitch * bh;
    let total = if tile_mode != 0 {
        ((slice_pitch as u64 + 4095) / 4096) * 4096
    } else {
        slice_pitch as u64
    };
    *out_layout = XgTexture2DLayout {
        row_pitch_bytes: row_pitch,
        slice_pitch_bytes: slice_pitch,
        total_bytes: total,
        tile_mode,
    };
    S_OK
}

#[inline]
fn morton_2d(x: u32, y: u32) -> u32 {
    let mut xx = x & 0xFFFF;
    let mut yy = y & 0xFFFF;
    xx = (xx | (xx << 8)) & 0x00FF_00FF;
    xx = (xx | (xx << 4)) & 0x0F0F_0F0F;
    xx = (xx | (xx << 2)) & 0x3333_3333;
    xx = (xx | (xx << 1)) & 0x5555_5555;
    yy = (yy | (yy << 8)) & 0x00FF_00FF;
    yy = (yy | (yy << 4)) & 0x0F0F_0F0F;
    yy = (yy | (yy << 2)) & 0x3333_3333;
    yy = (yy | (yy << 1)) & 0x5555_5555;
    xx | (yy << 1)
}

/// Clean-room GFX10.3 2D_THIN micro-tile swizzle/unswizzle over 8x8 element/block tiles.
pub fn swizzle_2d_thin(src: &[u8], dst: &mut [u8], width_blocks: u32, height_blocks: u32, bpb: u32, to_tiled: bool) {
    let wb = width_blocks.max(1) as usize;
    let hb = height_blocks.max(1) as usize;
    let stride = bpb.max(1) as usize;
    let tiles_x = (wb + 7) / 8;
    for y in 0..hb {
        for x in 0..wb {
            let lin_off = (y * wb + x) * stride;
            let tile_idx = (y / 8) * tiles_x + (x / 8);
            let in_tile = morton_2d((x & 7) as u32, (y & 7) as u32) as usize;
            let tile_off = (tile_idx * 64 + in_tile) * stride;
            let (s_off, d_off) = if to_tiled { (lin_off, tile_off) } else { (tile_off, lin_off) };
            if s_off + stride <= src.len() && d_off + stride <= dst.len() {
                dst[d_off..d_off + stride].copy_from_slice(&src[s_off..s_off + stride]);
            }
        }
    }
}

#[no_mangle]
pub unsafe extern "system" fn XGTileTexture2D(
    src_linear: *const u8,
    dst_tiled: *mut u8,
    size_bytes: usize,
) -> i32 {
    if src_linear.is_null() || dst_tiled.is_null() {
        return E_POINTER;
    }
    std::ptr::copy_nonoverlapping(src_linear, dst_tiled, size_bytes);
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XGUntileTexture2D(
    src_tiled: *const u8,
    dst_linear: *mut u8,
    size_bytes: usize,
) -> i32 {
    if src_tiled.is_null() || dst_linear.is_null() {
        return E_POINTER;
    }
    std::ptr::copy_nonoverlapping(src_tiled, dst_linear, size_bytes);
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

            let mut lay = std::mem::zeroed::<XgTexture2DLayout>();
            assert_eq!(XGComputeTexture2DLayout(256, 256, 98, 1, &mut lay), S_OK);
            assert!(lay.row_pitch_bytes >= 1024 && lay.total_bytes >= 65536);

            let mut comp: *mut c_void = std::ptr::null_mut();
            assert_eq!(XGCreateTexture2DComputer(std::ptr::null(), &mut comp), S_OK);
            assert!(!comp.is_null());
            let vtbl = &*(*(comp as *const *const XgComputerVtbl));
            assert_eq!((vtbl.release)(comp), 0);

            let lin: Vec<u8> = (0..64 * 4).map(|i| (i & 0xFF) as u8).collect();
            let mut tiled = vec![0u8; 64 * 4];
            let mut back = vec![0u8; 64 * 4];
            swizzle_2d_thin(&lin, &mut tiled, 8, 8, 4, true);
            assert_ne!(tiled, lin);
            swizzle_2d_thin(&tiled, &mut back, 8, 8, 4, false);
            assert_eq!(back, lin);
        }
    }
}
