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

#[derive(Clone, Copy)]
struct TexDesc {
    dim: u32,
    width: u64,
    height: u32,
    depth_array: u16,
    mips: u16,
    fmt: u32,
    samples: u32,
    layout: u32,
}

impl TexDesc {
    unsafe fn from_d3d12(p: *const c_void) -> Self {
        if p.is_null() {
            return Self { dim: 3, width: 1, height: 1, depth_array: 1, mips: 1, fmt: 28, samples: 1, layout: 0 };
        }
        let b = p as *const u8;
        let dim = std::ptr::read_unaligned(b as *const u32);
        let width = std::ptr::read_unaligned(b.add(0x10) as *const u64).max(1);
        let height = std::ptr::read_unaligned(b.add(0x18) as *const u32).max(1);
        let depth_array = std::ptr::read_unaligned(b.add(0x1c) as *const u16).max(1);
        let mips = std::ptr::read_unaligned(b.add(0x1e) as *const u16).clamp(1, 15);
        let fmt = std::ptr::read_unaligned(b.add(0x20) as *const u32);
        let samples = std::ptr::read_unaligned(b.add(0x24) as *const u32).max(1);
        let layout = std::ptr::read_unaligned(b.add(0x2c) as *const u32);
        Self { dim, width, height, depth_array, mips, fmt, samples, layout }
    }
}

#[derive(Clone, Copy, Default)]
struct MipGeom {
    size_bytes: u64,
    offset_bytes: u64,
    slice_bytes: u64,
    pitch_pixels: u32,
    pitch_bytes: u32,
    w_elem: u32,
    h_elem: u32,
    d_elem: u32,
    row_bytes: u32,
}

#[derive(Clone, Copy)]
struct TexLayout {
    total_bytes: u64,
    bpb: u32,
    bw: u32,
    bh: u32,
    mips: u32,
    is_3d: bool,
    mip: [MipGeom; 15],
}

fn compute_linear_layout(d: &TexDesc) -> TexLayout {
    let mut info = XgFormatBlockInfo { block_width: 1, block_height: 1, bytes_per_block: 4 };
    unsafe { XGGetFormatBlockInfo(d.fmt, &mut info); }
    let mips = (d.mips as u32).clamp(1, 15);
    let is_3d = d.dim == 4;
    let mut out = TexLayout {
        total_bytes: 0,
        bpb: info.bytes_per_block,
        bw: info.block_width,
        bh: info.block_height,
        mips,
        is_3d,
        mip: [MipGeom::default(); 15],
    };
    let mut off = 0u64;
    for m in 0..mips as usize {
        let w = ((d.width as u32) >> m).max(1);
        let h = if d.dim >= 3 { (d.height >> m).max(1) } else { 1 };
        let depth = if is_3d { ((d.depth_array as u32) >> m).max(1) } else { d.depth_array as u32 };
        let w_blk = (w + info.block_width - 1) / info.block_width;
        let h_blk = (h + info.block_height - 1) / info.block_height;
        let row_bytes = w_blk * info.bytes_per_block;
        let pitch_bytes = ((row_bytes + 255) & !255).max(256);
        let pitch_pixels = (pitch_bytes / info.bytes_per_block.max(1)) * info.block_width;
        let slice_bytes = pitch_bytes as u64 * h_blk as u64;
        let size_bytes = slice_bytes * depth as u64;
        out.mip[m] = MipGeom {
            size_bytes,
            offset_bytes: off,
            slice_bytes,
            pitch_pixels,
            pitch_bytes,
            w_elem: w_blk,
            h_elem: h_blk,
            d_elem: depth,
            row_bytes,
        };
        off = (off + size_bytes + 255) & !255;
    }
    out.total_bytes = ((off.max(4096)) + 4095) & !4095;
    out
}

#[repr(C)]
struct XgComputer {
    vtbl: *const XgComputerVtbl,
    refs: AtomicU32,
    desc: TexDesc,
    layout: TexLayout,
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
    desc: *const c_void,
    out_computer: *mut *mut c_void,
) -> i32 {
    if out_computer.is_null() {
        return E_POINTER;
    }
    let d = TexDesc::from_d3d12(desc);
    let layout = compute_linear_layout(&d);
    let c = Box::new(XgComputer {
        vtbl: &XG_VTBL,
        refs: AtomicU32::new(1),
        desc: d,
        layout,
    });
    *out_computer = Box::into_raw(c) as *mut c_void;
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

// ---- XGCreateTextureComputer (Scarlett IXGTextureAddressComputer) ----
// Verified against game_scarlett_master.exe call sites:
//   slot 0 (+0x00): AddRef
//   slot 1 (+0x08): Release
//   slot 2 (+0x10): GetResourceLayout(XG_RESOURCE_LAYOUT*)  [0x1b38 bytes: 0x18 hdr + 4 * 0x6c8 planes]
//   slot 3 (+0x18): GetResourceSizeBytes() -> u64
//   slot 4 (+0x20): GetResourceBaseAlignmentBytes() -> u64
//   slot 5 (+0x28): GetMipLevelOffsetBytes(plane, mip) -> u64
//   slot 8 (+0x40): CopyIntoSubresource(pDst, plane, subres, pSrc, srcRowPitch, srcSlicePitch) -> HR
//   slot 9 (+0x48): CopyFromSubresource(pDst, plane, subres, flags, pSrc, dstRowPitch, dstSlicePitch) -> HR
//   slot 10 (+0x50): GetResourceTiling(...) -> void
unsafe fn xg_mem_ok(ptr: usize, bytes: usize, writable: bool) -> bool {
    if ptr < 0x10000 || bytes == 0 {
        return false;
    }
    #[repr(C)]
    struct Mbi { base: usize, alloc: usize, ap: u32, part: u16, sz: usize, st: u32, pr: u32, ty: u32 }
    extern "system" { fn VirtualQuery(p: usize, m: *mut Mbi, n: usize) -> usize; }
    let mut m: Mbi = std::mem::zeroed();
    if VirtualQuery(ptr, &mut m, std::mem::size_of::<Mbi>()) == 0 || m.st != 0x1000 {
        return false;
    }
    let prot_ok = if writable { (m.pr & 0xCC) != 0 } else { (m.pr & 0xEE) != 0 };
    prot_ok && (m.base + m.sz).saturating_sub(ptr) >= bytes
}

unsafe extern "system" fn xg_get_resource_layout(this: *mut c_void, out: *mut u8) -> i32 {
    if this.is_null() || out.is_null() {
        return E_POINTER;
    }
    let c = &*(this as *const XgComputer);
    const TOTAL_SZ: usize = 0x18 + 4 * 0x6c8; // 0x1b38
    std::ptr::write_bytes(out, 0, TOTAL_SZ);
    // Header (+0x00..+0x18)
    std::ptr::write_unaligned(out.add(0x00) as *mut u64, c.layout.total_bytes);
    std::ptr::write_unaligned(out.add(0x08) as *mut u64, 4096u64);
    std::ptr::write_unaligned(out.add(0x10) as *mut u32, c.layout.mips);
    std::ptr::write_unaligned(out.add(0x14) as *mut u32, 1u32); // Planes = 1
    // Plane 0 (+0x18..+0x6e0)
    let p0 = out.add(0x18);
    std::ptr::write_unaligned(p0.add(0x00) as *mut u32, 0u32); // Usage = DEFAULT
    std::ptr::write_unaligned(p0.add(0x08) as *mut u64, c.layout.total_bytes);
    std::ptr::write_unaligned(p0.add(0x10) as *mut u64, 0u64); // BaseOffsetBytes
    std::ptr::write_unaligned(p0.add(0x18) as *mut u64, 4096u64);
    std::ptr::write_unaligned(p0.add(0x20) as *mut u32, c.layout.bpb);
    // MipLayout[0..mips] starting at Plane 0 + 0x38 (= out + 0x50), stride 0x70
    for m in 0..(c.layout.mips as usize).min(15) {
        let mg = &c.layout.mip[m];
        let mp = p0.add(0x38 + m * 0x70);
        std::ptr::write_unaligned(mp.add(0x00) as *mut u64, mg.size_bytes);
        std::ptr::write_unaligned(mp.add(0x08) as *mut u64, mg.offset_bytes);
        std::ptr::write_unaligned(mp.add(0x10) as *mut u64, mg.slice_bytes);
        std::ptr::write_unaligned(mp.add(0x18) as *mut u32, mg.pitch_pixels);
        std::ptr::write_unaligned(mp.add(0x1c) as *mut u32, mg.pitch_bytes);
        std::ptr::write_unaligned(mp.add(0x20) as *mut u32, 256u32); // AlignmentBytes
        std::ptr::write_unaligned(mp.add(0x24) as *mut u32, mg.w_elem);
        std::ptr::write_unaligned(mp.add(0x28) as *mut u32, mg.h_elem);
        std::ptr::write_unaligned(mp.add(0x2c) as *mut u32, mg.d_elem);
        std::ptr::write_unaligned(mp.add(0x30) as *mut u32, mg.w_elem);
        std::ptr::write_unaligned(mp.add(0x34) as *mut u32, mg.h_elem);
        std::ptr::write_unaligned(mp.add(0x38) as *mut u32, mg.d_elem);
        std::ptr::write_unaligned(mp.add(0x3c) as *mut u32, c.desc.samples);
        std::ptr::write_unaligned(mp.add(0x40) as *mut u32, 0u32); // TileMode = linear
    }
    static N: AtomicU32 = AtomicU32::new(0);
    if N.fetch_add(1, Ordering::Relaxed) < 8 {
        eprintln!(
            "[xg] GetResourceLayout {}x{}x{} mips={} fmt={} -> total={:#x} mip0(off={:#x} pitch={} slice={:#x})",
            c.desc.width, c.desc.height, c.desc.depth_array, c.layout.mips, c.desc.fmt,
            c.layout.total_bytes, c.layout.mip[0].offset_bytes, c.layout.mip[0].pitch_bytes, c.layout.mip[0].slice_bytes
        );
    }
    S_OK
}

unsafe extern "system" fn xg_get_resource_size(this: *mut c_void) -> u64 {
    if this.is_null() { 0 } else { (*(this as *const XgComputer)).layout.total_bytes }
}

unsafe extern "system" fn xg_get_resource_align(_this: *mut c_void) -> u64 {
    4096
}

unsafe extern "system" fn xg_get_mip_offset(this: *mut c_void, _plane: u32, mip: u32) -> u64 {
    if this.is_null() { return 0; }
    let c = &*(this as *const XgComputer);
    c.layout.mip[(mip as usize).min(14)].offset_bytes
}

unsafe extern "system" fn xg_copy_into_subresource(
    this: *mut c_void,
    dst: *mut u8,
    _plane: u32,
    subres: u32,
    src: *const u8,
    src_row_pitch: u32,
    src_slice_pitch: u32,
) -> i32 {
    if this.is_null() || dst.is_null() || src.is_null() {
        return S_OK;
    }
    let c = &*(this as *const XgComputer);
    let mips = c.layout.mips.max(1);
    let mip = (subres % mips) as usize;
    let slice = (subres / mips) as u64;
    let mg = &c.layout.mip[mip.min(14)];
    let dst_base = (dst as usize).wrapping_add((mg.offset_bytes + slice * mg.slice_bytes) as usize);
    let rows = mg.h_elem as usize;
    let copy_w = (mg.row_bytes as usize).min(src_row_pitch as usize).min(mg.pitch_bytes as usize);
    let slices_to_copy = if c.layout.is_3d { mg.d_elem as usize } else { 1 };
    let need_dst = mg.slice_bytes as usize * slices_to_copy;
    let need_src = if slices_to_copy > 1 {
        src_slice_pitch as usize * slices_to_copy
    } else if rows > 0 {
        (rows - 1) * src_row_pitch as usize + copy_w
    } else {
        0
    };
    if copy_w > 0 && rows > 0 && xg_mem_ok(dst_base, need_dst.max(copy_w), true) && xg_mem_ok(src as usize, need_src.max(copy_w), false) {
        for z in 0..slices_to_copy {
            let dz = dst_base + z * mg.slice_bytes as usize;
            let sz = (src as usize) + z * src_slice_pitch as usize;
            for y in 0..rows {
                std::ptr::copy_nonoverlapping(
                    (sz + y * src_row_pitch as usize) as *const u8,
                    (dz + y * mg.pitch_bytes as usize) as *mut u8,
                    copy_w,
                );
            }
        }
    }
    static N: AtomicU32 = AtomicU32::new(0);
    if N.fetch_add(1, Ordering::Relaxed) < 8 {
        eprintln!(
            "[xg] CopyIntoSubresource dst={dst:p} sub={subres} {}x{} fmt={} src={src:p} spitch={src_row_pitch} dpitch={}",
            c.desc.width, c.desc.height, c.desc.fmt, mg.pitch_bytes
        );
    }
    S_OK
}

unsafe extern "system" fn xg_copy_from_subresource(
    this: *mut c_void,
    dst: *mut u8,
    _plane: u32,
    subres: u32,
    _flags: u32,
    src: *const u8,
    dst_row_pitch: u32,
    dst_slice_pitch: u32,
) -> i32 {
    if this.is_null() || dst.is_null() || src.is_null() {
        return S_OK;
    }
    let c = &*(this as *const XgComputer);
    let mips = c.layout.mips.max(1);
    let mip = (subres % mips) as usize;
    let slice = (subres / mips) as u64;
    let mg = &c.layout.mip[mip.min(14)];
    let src_base = (src as usize).wrapping_add((mg.offset_bytes + slice * mg.slice_bytes) as usize);
    let rows = mg.h_elem as usize;
    let copy_w = (mg.row_bytes as usize).min(dst_row_pitch as usize).min(mg.pitch_bytes as usize);
    let slices_to_copy = if c.layout.is_3d { mg.d_elem as usize } else { 1 };
    let need_src = mg.slice_bytes as usize * slices_to_copy;
    let need_dst = if slices_to_copy > 1 {
        dst_slice_pitch as usize * slices_to_copy
    } else if rows > 0 {
        (rows - 1) * dst_row_pitch as usize + copy_w
    } else {
        0
    };
    if copy_w > 0 && rows > 0 && xg_mem_ok(dst as usize, need_dst.max(copy_w), true) && xg_mem_ok(src_base, need_src.max(copy_w), false) {
        for z in 0..slices_to_copy {
            let dz = (dst as usize) + z * dst_slice_pitch as usize;
            let sz = src_base + z * mg.slice_bytes as usize;
            for y in 0..rows {
                std::ptr::copy_nonoverlapping(
                    (sz + y * mg.pitch_bytes as usize) as *const u8,
                    (dz + y * dst_row_pitch as usize) as *mut u8,
                    copy_w,
                );
            }
        }
    }
    S_OK
}

unsafe extern "system" fn xg_get_resource_tiling(
    _this: *mut c_void,
    num_tiles: *mut u32,
    _packed_desc: *mut u8,
    _tile_shape: *mut u8,
    num_subres_tilings: *mut u32,
    _first_subres: u32,
    _tilings: *mut u8,
) {
    if !num_tiles.is_null() { *num_tiles = 0; }
    if !num_subres_tilings.is_null() { *num_subres_tilings = 0; }
}

macro_rules! xgslot {
    ($name:ident, $n:literal) => {
        unsafe extern "system" fn $name(_t: usize, a: usize, b: usize, c: usize) -> i32 {
            static ONCE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            if !ONCE.swap(true, Ordering::Relaxed) {
                eprintln!("[xg] computer slot {} ({a:#x}, {b:#x}, {c:#x}) -> S_OK", $n);
            }
            0
        }
    };
}
xgslot!(g6, 6); xgslot!(g7, 7);
xgslot!(g11, 11); xgslot!(g12, 12); xgslot!(g13, 13); xgslot!(g14, 14); xgslot!(g15, 15);
xgslot!(g16, 16); xgslot!(g17, 17); xgslot!(g18, 18); xgslot!(g19, 19); xgslot!(g20, 20); xgslot!(g21, 21);
xgslot!(g22, 22); xgslot!(g23, 23); xgslot!(g24, 24); xgslot!(g25, 25); xgslot!(g26, 26); xgslot!(g27, 27);
xgslot!(g28, 28); xgslot!(g29, 29); xgslot!(g30, 30); xgslot!(g31, 31);

fn xg_vtbl32() -> *const XgComputerVtbl {
    static V: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *V.get_or_init(|| {
        let mut v = Box::new([0usize; 32]);
        v[0] = xg_add_ref as *const () as usize;
        v[1] = xg_release as *const () as usize;
        v[2] = xg_get_resource_layout as *const () as usize;
        v[3] = xg_get_resource_size as *const () as usize;
        v[4] = xg_get_resource_align as *const () as usize;
        v[5] = xg_get_mip_offset as *const () as usize;
        v[6] = g6 as *const () as usize;
        v[7] = g7 as *const () as usize;
        v[8] = xg_copy_into_subresource as *const () as usize;
        v[9] = xg_copy_from_subresource as *const () as usize;
        v[10] = xg_get_resource_tiling as *const () as usize;
        let rest: [unsafe extern "system" fn(usize, usize, usize, usize) -> i32; 21] = [
            g11, g12, g13, g14, g15, g16, g17, g18, g19, g20, g21, g22, g23, g24, g25, g26, g27, g28, g29, g30, g31,
        ];
        for (i, f) in rest.iter().enumerate() {
            v[11 + i] = *f as usize;
        }
        Box::leak(v).as_ptr() as usize
    }) as *const XgComputerVtbl
}

#[no_mangle]
pub unsafe extern "system" fn XGCreateTextureComputer(desc: *const c_void, out_computer: *mut *mut c_void) -> i32 {
    if out_computer.is_null() {
        return E_POINTER;
    }
    let d = TexDesc::from_d3d12(desc);
    let layout = compute_linear_layout(&d);
    let c = Box::new(XgComputer { vtbl: xg_vtbl32(), refs: AtomicU32::new(1), desc: d, layout });
    *out_computer = Box::into_raw(c) as *mut c_void;
    S_OK
}

/// Host textures are linear: suggest swizzle mode 0.
#[no_mangle]
pub unsafe extern "system" fn XGSuggestSwizzleMode(_desc: usize, _flags: usize, out_mode: *mut u32) -> i32 {
    if !out_mode.is_null() {
        *out_mode = 0x102; // valid Scarlett swizzle enum
    }
    0
}
