//! Clean-room descriptor shadowing & draw-time materialization for console descriptor heaps.
//!
//! Xbox Series titles copy 32-byte CBV/SRV/UAV (and 16-byte sampler) descriptors on the CPU
//! via `memcpy` into placed shader-visible descriptor heaps, and also create placed views via
//! `ID3D12DeviceX` slots 70/71. Host D3D12 heaps live in separate driver memory, so we stamp
//! a compact `(MAGIC, record_id)` tag into each title-visible descriptor slot and materialize
//! the referenced PC descriptors into the host shader-visible heap right before each draw.
#![allow(dead_code)]

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use super::{depth_to_srv, gfx_pso, is_depth_resource, log_d3d12, map_console_format, mirror, replay};

const TAG_MAGIC: u64 = 0x5045_5243_5944_5343; // "PERCYDSC"

#[derive(Clone, Copy)]
pub(crate) struct ShadowHeap {
    pub heap: usize,
    pub title_start: usize,
    pub size: usize,
    pub host_cpu: usize,
    pub ty: u32,
    pub inc: u32,
}

#[derive(Clone)]
pub(crate) enum DescRecord {
    Cbv { va: u64, size: u32 },
    Srv { res: usize, desc: Option<[u32; 10]> },
    Uav { res: usize, ctr: usize, desc: Option<[u32; 10]> },
    PlacedSrv { res_desc: [u32; 14], srv_desc: [u32; 16] },
    PlacedUav { res_desc: [u32; 14], uav_desc: [u32; 16] },
    Sampler { desc: [u32; 13] },
}

static SHADOW_HEAPS: Mutex<Vec<ShadowHeap>> = Mutex::new(Vec::new());
static RECORDS: Mutex<Vec<DescRecord>> = Mutex::new(Vec::new());
/// Placed texture cache for slot-70 PlacedSrv textures: (cpu_va, width, height, fmt) -> host ID3D12Resource*.
static PLACED_TEX_CACHE: Mutex<Option<HashMap<(u64, u64, u32, u32), usize>>> = Mutex::new(None);
/// Last materialized record id per host CPU handle (for non-CBV descriptors that don't need per-draw refresh).
static LAST_MAT: Mutex<Option<HashMap<usize, u32>>> = Mutex::new(None);

unsafe fn vt_slot(obj: *mut c_void, slot: usize) -> usize {
    *(*(obj as *const *const usize)).add(slot)
}

pub(crate) fn register_heap(
    heap: *mut c_void,
    title_start: usize,
    size: usize,
    host_cpu: usize,
    ty: u32,
    inc: u32,
) {
    if heap.is_null() || title_start == 0 || size == 0 || inc == 0 {
        return;
    }
    SHADOW_HEAPS.lock().unwrap().push(ShadowHeap {
        heap: heap as usize,
        title_start,
        size,
        host_cpu,
        ty,
        inc,
    });
}

pub(crate) fn title_start_of(heap: *mut c_void) -> Option<usize> {
    let h = heap as usize;
    SHADOW_HEAPS.lock().unwrap().iter().find(|p| p.heap == h).map(|p| p.title_start)
}

pub(crate) fn heap_by_obj(heap: usize) -> Option<ShadowHeap> {
    SHADOW_HEAPS.lock().unwrap().iter().find(|p| p.heap == heap).copied()
}

pub(crate) fn xlate_cpu(h: usize) -> usize {
    SHADOW_HEAPS
        .lock()
        .unwrap()
        .iter()
        .find(|p| h >= p.title_start && h < p.title_start + p.size)
        .map(|p| p.host_cpu + (h - p.title_start))
        .unwrap_or(h)
}

fn find_shadow_slot(h: usize) -> Option<ShadowHeap> {
    SHADOW_HEAPS
        .lock()
        .unwrap()
        .iter()
        .find(|p| h >= p.title_start && h < p.title_start + p.size)
        .copied()
}

pub(crate) fn is_shadow_slot(h: usize) -> bool {
    find_shadow_slot(h).is_some()
}

pub(crate) fn invalidate_slot(h: usize) {
    if let Some(sh) = find_shadow_slot(h) {
        let host_h = sh.host_cpu + (h - sh.title_start);
        if let Some(m) = LAST_MAT.lock().unwrap().as_mut() {
            m.remove(&host_h);
        }
    }
}

/// Allocates a record ID and stamps the tag into title-visible shadow memory at `h` (if tracked).
pub(crate) unsafe fn record_and_stamp(h: usize, rec: DescRecord) -> u32 {
    let id = {
        let mut v = RECORDS.lock().unwrap();
        v.push(rec);
        v.len() as u32
    };
    if let Some(sh) = find_shadow_slot(h) {
        if sh.ty == 0 && h + 32 <= sh.title_start + sh.size {
            std::ptr::write_unaligned(h as *mut [u64; 4], [TAG_MAGIC, id as u64, 0, 0]);
        } else if sh.ty == 1 && h + 16 <= sh.title_start + sh.size {
            std::ptr::write_unaligned(h as *mut [u64; 2], [TAG_MAGIC, id as u64]);
        }
        // Track that the host heap slot was written directly (unless caller skips host write).
        let host_h = sh.host_cpu + (h - sh.title_start);
        LAST_MAT.lock().unwrap().get_or_insert_with(HashMap::new).insert(host_h, id);
    }
    id
}

/// Mirrors `CopyDescriptors` / `CopyDescriptorsSimple` tags across tracked shadow/placed heaps.
pub(crate) unsafe fn copy_tags(dst: usize, src: usize, count: u32, ty: u32) {
    if count == 0 || ty > 1 {
        return;
    }
    let step = if ty == 1 { 16usize } else { 32usize };
    let bytes = count as usize * step;
    let Some(dsh) = find_shadow_slot(dst) else { return };
    let Some(ssh) = find_shadow_slot(src) else { return };
    if dst + bytes <= dsh.title_start + dsh.size && src + bytes <= ssh.title_start + ssh.size {
        std::ptr::copy(src as *const u8, dst as *mut u8, bytes);
        let mut lm = LAST_MAT.lock().unwrap();
        if let Some(m) = lm.as_mut() {
            for i in 0..count as usize {
                m.remove(&(dsh.host_cpu + (dst - dsh.title_start) + i * step));
            }
        }
    }
}

/// Sanitize a D3D12_SHADER_RESOURCE_VIEW_DESC for PC D3D12 (depth-to-srv + structured-to-raw).
pub(crate) fn sanitize_srv_desc(raw: [u32; 10]) -> [u32; 10] {
    let mut d = raw;
    let mapped = map_console_format(d[0]);
    d[0] = if d[1] != 1 { depth_to_srv(mapped) } else { mapped };
    if d[1] == 1 && d[7] > 0 && d[7] % 4 == 0 {
        let stride = d[7] as u64;
        let first_elem = (d[4] as u64) | ((d[5] as u64) << 32);
        let num_elems = d[6] as u64;
        let raw_first = (first_elem * stride) / 4;
        let raw_num = ((num_elems * stride) / 4).min(u32::MAX as u64) as u32;
        d[0] = 39; // R32_TYPELESS
        d[4] = raw_first as u32;
        d[5] = (raw_first >> 32) as u32;
        d[6] = raw_num;
        d[7] = 0;
        d[8] = 1; // D3D12_BUFFER_SRV_FLAG_RAW
    }
    d
}

/// Materialize a `DescRecord` directly into the host descriptor heap at `host_cpu`.
/// Returns `Some(res_ptr)` if the descriptor references a Texture SRV (so the caller can sync it).
pub(crate) unsafe fn materialize_record(
    dev_inner: *mut c_void,
    host_cpu: usize,
    rec: &DescRecord,
    id: u32,
) -> Option<*mut c_void> {
    match rec {
        DescRecord::Cbv { va, size } => {
            let va = *va;
            let size = (*size as u64).min(65536);
            let rounded = ((size + 255) & !255).min(65536);
            let mut d = [0u64; 2];
            let mut pd = std::ptr::null::<c_void>();
            // Always prefer fresh upload copy if CPU-readable, so CPU writes after CreateCBV are visible!
            if va != 0 && size > 0 && rounded <= 65536 {
                if let Some(gpu_va) = gfx_pso::upload_copy(dev_inner, va, size as usize, rounded as usize, 256) {
                    d[0] = gpu_va;
                    d[1] = rounded;
                    pd = d.as_ptr() as *const c_void;
                } else if let Some(room) = replay::buf_extent(va) {
                    if va % 256 == 0 && rounded <= room && rounded != 0 {
                        d[0] = va;
                        d[1] = rounded;
                        pd = d.as_ptr() as *const c_void;
                    }
                } else if let Some((gpu_va, room)) = gfx_pso::cpu_to_placed_gpu(va) {
                    if gpu_va % 256 == 0 && rounded <= room && rounded != 0 {
                        d[0] = gpu_va;
                        d[1] = rounded;
                        pd = d.as_ptr() as *const c_void;
                    }
                }
            }
            let f: unsafe extern "system" fn(*mut c_void, *const c_void, usize) =
                std::mem::transmute(vt_slot(dev_inner, 17));
            f(dev_inner, pd, host_cpu);
            None
        }
        DescRecord::Srv { res, desc } => {
            let f: unsafe extern "system" fn(*mut c_void, *mut c_void, *const c_void, usize) =
                std::mem::transmute(vt_slot(dev_inner, 18));
            let res_p = mirror::sub(*res as *mut c_void);
            let mut is_tex = false;
            if let Some(raw) = desc {
                let d = sanitize_srv_desc(*raw);
                is_tex = d[1] != 1 && !res_p.is_null();
                f(dev_inner, res_p, d.as_ptr() as *const c_void, host_cpu);
            } else {
                is_tex = !res_p.is_null();
                f(dev_inner, res_p, std::ptr::null(), host_cpu);
            }
            LAST_MAT.lock().unwrap().get_or_insert_with(HashMap::new).insert(host_cpu, id);
            if is_tex { Some(res_p) } else { None }
        }
        DescRecord::Uav { res, ctr, desc } => {
            let f: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void, *const c_void, usize) =
                std::mem::transmute(vt_slot(dev_inner, 19));
            let res_p = *res as *mut c_void;
            let ctr_p = *ctr as *mut c_void;
            if is_depth_resource(res_p) {
                let mut null_uav = [0u32; 10];
                null_uav[0] = 42;
                null_uav[1] = 4;
                f(dev_inner, std::ptr::null_mut(), std::ptr::null_mut(), null_uav.as_ptr() as *const c_void, host_cpu);
            } else if let Some(raw) = desc {
                let mut d = *raw;
                d[0] = map_console_format(d[0]);
                f(dev_inner, mirror::sub(res_p), mirror::sub(ctr_p), d.as_ptr() as *const c_void, host_cpu);
            } else {
                f(dev_inner, mirror::sub(res_p), mirror::sub(ctr_p), std::ptr::null(), host_cpu);
            }
            LAST_MAT.lock().unwrap().get_or_insert_with(HashMap::new).insert(host_cpu, id);
            None
        }
        DescRecord::PlacedSrv { res_desc, srv_desc } => {
            let addr = (srv_desc[10] as u64) | ((srv_desc[11] as u64) << 32);
            let mut srv10 = [0u32; 10];
            srv10.copy_from_slice(&srv_desc[..10]);
            let f: unsafe extern "system" fn(*mut c_void, *mut c_void, *const c_void, usize) =
                std::mem::transmute(vt_slot(dev_inner, 18));
            if res_desc[0] == 1 {
                // Buffer placed SRV: compute byte span and upload fresh copy into our UploadRing buffer!
                let d = sanitize_srv_desc(srv10);
                let first_dw = (d[4] as u64) | ((d[5] as u64) << 32);
                let num_dw = d[6] as u64;
                let res_w = (res_desc[4] as u64) | ((res_desc[5] as u64) << 32);
                let byte_off = first_dw * 4;
                let copy_bytes = if num_dw > 0 {
                    (num_dw * 4).min(4 << 20) as usize
                } else {
                    res_w.saturating_sub(byte_off).min(4 << 20) as usize
                };
                let src_va = addr.saturating_add(byte_off);
                if copy_bytes > 0 {
                    if let Some((ring_res, ring_off)) = gfx_pso::upload_copy_res(dev_inner, src_va, copy_bytes, 16) {
                        let mut raw_d = [0u32; 10];
                        raw_d[0] = 39; // R32_TYPELESS
                        raw_d[1] = 1;  // BUFFER
                        raw_d[2] = 0x1688; // DEFAULT_SHADER_4_COMPONENT_MAPPING
                        let fe = (ring_off / 4) as u64;
                        raw_d[4] = fe as u32;
                        raw_d[5] = (fe >> 32) as u32;
                        raw_d[6] = ((copy_bytes + 3) / 4) as u32;
                        raw_d[7] = 0;
                        raw_d[8] = 1; // RAW
                        f(dev_inner, ring_res, raw_d.as_ptr() as *const c_void, host_cpu);
                        return None;
                    }
                }
                // Fallback: null raw buffer SRV
                let mut null_d = [0u32; 10];
                null_d[0] = 39;
                null_d[1] = 1;
                null_d[2] = 0x1688;
                null_d[8] = 1;
                f(dev_inner, std::ptr::null_mut(), null_d.as_ptr() as *const c_void, host_cpu);
                None
            } else {
                // Texture placed SRV: look up matching PlacedResourceX texture (same addr+dims+fmt), or create committed texture.
                let tex = get_or_create_placed_tex(dev_inner, addr, res_desc);
                let mut d = sanitize_srv_desc(srv10);
                if tex.is_null() {
                    d = [28, 4, 0x1688, 0, 0, 1, 0, 0, 0, 0]; // clean null Texture2D SRV
                } else {
                    // Match single-mip backing texture so MipLevels/MostDetailedMip never exceed 1.
                    if matches!(d[1], 2 | 4 | 8 | 9) {
                        d[4] = 0; // MostDetailedMip = 0
                        d[5] = 1; // MipLevels = 1
                    }
                }
                f(dev_inner, tex, d.as_ptr() as *const c_void, host_cpu);
                LAST_MAT.lock().unwrap().get_or_insert_with(HashMap::new).insert(host_cpu, id);
                if tex.is_null() { None } else { Some(tex) }
            }
        }
        DescRecord::PlacedUav { .. } => {
            let f: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void, *const c_void, usize) =
                std::mem::transmute(vt_slot(dev_inner, 19));
            let mut null_uav = [0u32; 10];
            null_uav[0] = 42; // R32_UINT
            null_uav[1] = 4;  // D3D12_UAV_DIMENSION_TEXTURE2D
            f(dev_inner, std::ptr::null_mut(), std::ptr::null_mut(), null_uav.as_ptr() as *const c_void, host_cpu);
            LAST_MAT.lock().unwrap().get_or_insert_with(HashMap::new).insert(host_cpu, id);
            None
        }
        DescRecord::Sampler { desc } => {
            let f: unsafe extern "system" fn(*mut c_void, *const c_void, usize) =
                std::mem::transmute(vt_slot(dev_inner, 22));
            f(dev_inner, desc.as_ptr() as *const c_void, host_cpu);
            LAST_MAT.lock().unwrap().get_or_insert_with(HashMap::new).insert(host_cpu, id);
            None
        }
    }
}

/// Tracked PlacedResourceX textures: host ID3D12Resource* -> (cpu_va, width, height, depth_or_array, mips, fmt, console_layout, flags).
#[derive(Clone, Copy)]
pub(crate) struct PlacedTexInfo {
    pub res: usize,
    pub cpu_va: u64,
    pub width: u64,
    pub height: u32,
    pub depth_array: u16,
    pub mips: u16,
    pub fmt: u32,
    pub console_layout: u32,
    pub flags: u32,
}

static PLACED_TEX_BY_RES: Mutex<Option<HashMap<usize, PlacedTexInfo>>> = Mutex::new(None);
static PLACED_TEX_BY_VA: Mutex<Option<HashMap<u64, PlacedTexInfo>>> = Mutex::new(None);
static RTV_BY_CPU: Mutex<Option<HashMap<usize, usize>>> = Mutex::new(None);

pub(crate) fn note_rtv(host_cpu: usize, res: *mut c_void) {
    if host_cpu != 0 && !res.is_null() {
        RTV_BY_CPU.lock().unwrap().get_or_insert_with(HashMap::new).insert(host_cpu, res as usize);
    }
}

pub(crate) fn rtv_res(host_cpu: usize) -> usize {
    RTV_BY_CPU.lock().unwrap().as_ref().and_then(|m| m.get(&host_cpu).copied()).unwrap_or(0)
}

pub(crate) unsafe fn debug_slot(heap_obj: usize, byte_off: u32) -> String {
    let Some(sh) = heap_by_obj(heap_obj) else { return "no_heap".into() };
    let off = byte_off as usize;
    if off + 32 > sh.size { return format!("oob({off:#x}/{:#x})", sh.size); }
    let title_va = sh.title_start + off;
    let w: [u32; 8] = std::ptr::read_unaligned(title_va as *const [u32; 8]);
    let magic = std::ptr::read_unaligned(title_va as *const u64);
    let id = std::ptr::read_unaligned((title_va + 8) as *const u64) as u32;
    if magic == TAG_MAGIC && id > 0 {
        let rec = RECORDS.lock().unwrap().get((id - 1) as usize).cloned();
        if let Some(DescRecord::Cbv { va, size }) = rec {
            let floats: Vec<f32> = if gfx_pso::cpu_readable(va, 64) {
                std::slice::from_raw_parts(va as *const f32, 16).to_vec()
            } else { vec![] };
            return format!("TAG_CBV(va={va:#x},sz={size},f={floats:.3?})");
        }
        return format!("TAG(id={id})");
    }
    let addr = (w[0] as u64) | (((w[1] & 0xFFFF) as u64) << 32);
    let stride = ((w[1] >> 16) & 0x3FFF) as u64;
    let num_rec = w[2] as u64;
    let bytes = if stride > 0 { num_rec.saturating_mul(stride) } else { num_rec };
    let floats: Vec<f32> = if bytes >= 64 && gfx_pso::cpu_readable(addr, 64) {
        std::slice::from_raw_parts(addr as *const f32, 16).to_vec()
    } else { vec![] };
    format!("RAW(w={w:08x?},addr={addr:#x},st={stride},n={num_rec},b={bytes},f={floats:.3?})")
}

pub(crate) fn note_placed_tex(info: PlacedTexInfo) {
    if info.res == 0 || info.cpu_va == 0 {
        return;
    }
    PLACED_TEX_BY_RES.lock().unwrap().get_or_insert_with(HashMap::new).insert(info.res, info);
    PLACED_TEX_BY_VA.lock().unwrap().get_or_insert_with(HashMap::new).insert(info.cpu_va, info);
}

pub(crate) fn placed_tex_info(res: *mut c_void) -> Option<PlacedTexInfo> {
    PLACED_TEX_BY_RES.lock().unwrap().as_ref().and_then(|m| m.get(&(res as usize)).copied())
}

pub(crate) fn set_placed_tex_cpu_va(res: *mut c_void, va: u64) {
    if res.is_null() || va == 0 {
        return;
    }
    if let Some(m) = PLACED_TEX_BY_RES.lock().unwrap().as_mut() {
        if let Some(info) = m.get_mut(&(res as usize)) {
            info.cpu_va = va;
        }
    }
}

unsafe fn get_or_create_placed_tex(dev_inner: *mut c_void, addr: u64, res_desc: &[u32; 14]) -> *mut c_void {
    let dim = res_desc[0];
    let w = (res_desc[4] as u64) | ((res_desc[5] as u64) << 32);
    let h = res_desc[6];
    let da = (res_desc[7] & 0xFFFF).max(1);
    let fmt = map_console_format(res_desc[8]);
    if !(2..=4).contains(&dim) || w == 0 || w > 16384 || h == 0 || h > 16384 || da > 2048 || fmt == 0 || fmt > 132 {
        return std::ptr::null_mut();
    }
    if let Some(info) = PLACED_TEX_BY_VA.lock().unwrap().as_ref().and_then(|m| m.get(&addr).copied()) {
        if info.width == w && info.height == h && info.fmt == fmt {
            return info.res as *mut c_void;
        }
    }
    let key = (addr, w, h, fmt);
    if let Some(&r) = PLACED_TEX_CACHE.lock().unwrap().as_ref().and_then(|m| m.get(&key)) {
        return r as *mut c_void;
    }
    const IID_RESOURCE: [u8; 16] = [0xbe, 0x42, 0x64, 0x69, 0x2e, 0xa7, 0x59, 0x40, 0xbc, 0x79, 0x5b, 0x5c, 0x98, 0x04, 0x0f, 0xad];
    let mut pc = super::sanitize_resource_desc(res_desc.as_ptr() as *const u8);
    pc[1] = 0; // alignment
    pc[3] = (pc[3] & 0x0000_FFFF_FFFF_FFFF) | (1u64 << 48); // MipLevels = 1
    pc[4] = (pc[4] & 0xFFFF_FFFF) | (1u64 << 32); // SampleCount = 1
    pc[5] = 0; // SampleQuality = 0, Layout = UNKNOWN
    pc[6] = 0; // Flags = NONE (SRV-only backing texture)
    let props = [1u32, 0, 0, 1, 1]; // DEFAULT
    let ccr: unsafe extern "system" fn(*mut c_void, *const u32, u32, *const u8, u32, *const c_void, *const u8, *mut *mut c_void) -> i32 =
        std::mem::transmute(vt_slot(dev_inner, 27));
    let mut tex = std::ptr::null_mut();
    let hr = ccr(dev_inner, props.as_ptr(), 0, pc.as_ptr() as *const u8, 0, std::ptr::null(), IID_RESOURCE.as_ptr(), &mut tex);
    if hr >= 0 && !tex.is_null() {
        PLACED_TEX_CACHE.lock().unwrap().get_or_insert_with(HashMap::new).insert(key, tex as usize);
        note_placed_tex(PlacedTexInfo {
            res: tex as usize,
            cpu_va: addr,
            width: pc[2],
            height: pc[3] as u32,
            depth_array: ((pc[3] >> 32) & 0xFFFF) as u16,
            mips: ((pc[3] >> 48) & 0xFFFF) as u16,
            fmt: pc[4] as u32,
            console_layout: res_desc[11],
            flags: pc[6] as u32,
        });
        static TN: AtomicU32 = AtomicU32::new(0);
        if TN.fetch_add(1, Ordering::Relaxed) < 8 {
            log_d3d12(&format!("[desc] PlacedSrv created tex {tex:p} va={addr:#x} {w}x{h} fmt={fmt} clayout={:#x}", res_desc[11]));
        }
        tex
    } else {
        std::ptr::null_mut()
    }
}

/// Syncs descriptor table entries read by a draw from title-visible shadow heap memory into
/// the host shader-visible descriptor heap. Returns any sampled Texture `ID3D12Resource*`s.
pub(crate) unsafe fn sync_draw_slot(
    dev_inner: *mut c_void,
    heap_obj: usize,
    byte_off: u32,
    is_sampler: bool,
    is_cbv: bool,
) -> Option<*mut c_void> {
    let sh = heap_by_obj(heap_obj)?;
    let off = byte_off as usize;
    let step = if is_sampler { 16usize } else { 32usize };
    if off + step > sh.size {
        return None;
    }
    let title_va = sh.title_start + off;
    let host_cpu = sh.host_cpu + off;
    let magic = std::ptr::read_unaligned(title_va as *const u64);
    let id = std::ptr::read_unaligned((title_va + 8) as *const u64) as u32;
    if magic != TAG_MAGIC || id == 0 {
        // Title can write raw GFX10 4-dword V# buffer descriptors directly into placed heap memory
        // (bypassing CreateConstantBufferView / CreateShaderResourceView). Decode and materialize them!
        if !is_sampler {
            let w: [u32; 8] = std::ptr::read_unaligned(title_va as *const [u32; 8]);
            let is_vsharp = (w[0] != 0 || (w[1] & 0xFFFF) != 0)
                && w[2] != 0
                && ((w[3] >> 30) & 3) == 0
                && w[4] == 0 && w[5] == 0 && w[6] == 0 && w[7] == 0;
            if is_vsharp {
                let addr = (w[0] as u64) | (((w[1] & 0xFFFF) as u64) << 32);
                let stride = ((w[1] >> 16) & 0x3FFF) as u64;
                let num_rec = w[2] as u64;
                let byte_size = if stride > 0 { num_rec.saturating_mul(stride) } else { num_rec };
                static RAW_N: AtomicU32 = AtomicU32::new(0);
                if RAW_N.fetch_add(1, Ordering::Relaxed) < 12 {
                    let f4 = |off_b: usize| -> [f32; 4] {
                        if byte_size as usize >= off_b + 16 && gfx_pso::cpu_readable(addr + off_b as u64, 16) {
                            let p = (addr + off_b as u64) as *const f32;
                            [*p, *p.add(1), *p.add(2), *p.add(3)]
                        } else {
                            [0.0; 4]
                        }
                    };
                    let sample = if byte_size == 96 { f4(32) } else if byte_size >= 416 { f4(352) } else { f4(0) };
                    log_d3d12(&format!(
                        "[desc] raw V# off={off:#x} addr={addr:#x} stride={stride} recs={num_rec} bytes={byte_size} is_cbv={is_cbv} f={sample:.3?}"
                    ));
                }
                if byte_size > 0 && byte_size <= (4 << 20) {
                    if is_cbv {
                        let rec = DescRecord::Cbv { va: addr, size: byte_size as u32 };
                        let _ = materialize_record(dev_inner, host_cpu, &rec, 0);
                        return None;
                    } else if let Some((ring_res, ring_off)) = gfx_pso::upload_copy_res(dev_inner, addr, byte_size as usize, 16) {
                        let f: unsafe extern "system" fn(*mut c_void, *mut c_void, *const c_void, usize) =
                            std::mem::transmute(vt_slot(dev_inner, 18));
                        let mut raw_d = [0u32; 10];
                        raw_d[0] = 39; // R32_TYPELESS
                        raw_d[1] = 1;  // BUFFER
                        raw_d[2] = 0x1688;
                        let fe = (ring_off / 4) as u64;
                        raw_d[4] = fe as u32;
                        raw_d[5] = (fe >> 32) as u32;
                        raw_d[6] = ((byte_size as usize + 3) / 4) as u32;
                        raw_d[8] = 1; // RAW
                        f(dev_inner, ring_res, raw_d.as_ptr() as *const c_void, host_cpu);
                        return None;
                    }
                }
            }
        }
        static UNTAG: AtomicU32 = AtomicU32::new(0);
        if UNTAG.fetch_add(1, Ordering::Relaxed) < 12 {
            let w: [u32; 8] = std::ptr::read_unaligned(title_va as *const [u32; 8]);
            log_d3d12(&format!("[desc] UNTAGGED slot off={off:#x} title={title_va:#x} words={w:08x?}"));
        }
        return None;
    }
    let rec = {
        let v = RECORDS.lock().unwrap();
        v.get((id - 1) as usize)?.clone()
    };
    let always_refresh = match &rec {
        DescRecord::Cbv { .. } => true,
        DescRecord::PlacedSrv { res_desc, .. } => res_desc[0] == 1,
        _ => false,
    };
    if !always_refresh {
        if let Some(&prev) = LAST_MAT.lock().unwrap().as_ref().and_then(|m| m.get(&host_cpu)) {
            if prev == id {
                // Already materialized; still return texture resource for sync check.
                return match rec {
                    DescRecord::Srv { res, .. } if res != 0 => Some(mirror::sub(res as *mut c_void)),
                    DescRecord::PlacedSrv { res_desc, srv_desc } if res_desc[0] != 1 => {
                        let addr = (srv_desc[10] as u64) | ((srv_desc[11] as u64) << 32);
                        let t = get_or_create_placed_tex(dev_inner, addr, &res_desc);
                        if t.is_null() { None } else { Some(t) }
                    }
                    _ => None,
                };
            }
        }
    }
    static MAT_N: AtomicU32 = AtomicU32::new(0);
    if MAT_N.fetch_add(1, Ordering::Relaxed) < 16 {
        let kind = match &rec {
            DescRecord::Cbv { va, size } => format!("Cbv({va:#x}, sz={size})"),
            DescRecord::Srv { res, desc } => format!("Srv({res:#x}, dim={}, fmt={})", desc.map(|d| d[1]).unwrap_or(99), desc.map(|d| d[0]).unwrap_or(99)),
            DescRecord::Uav { res, .. } => format!("Uav({res:#x})"),
            DescRecord::PlacedSrv { res_desc, srv_desc } => {
                let a = (srv_desc[10] as u64) | ((srv_desc[11] as u64) << 32);
                format!("PlacedSrv(dim={}, va={a:#x}, w={}, h={}, fmt={})", res_desc[0], res_desc[4], res_desc[6], srv_desc[0])
            }
            DescRecord::PlacedUav { .. } => "PlacedUav".into(),
            DescRecord::Sampler { .. } => "Sampler".into(),
        };
        log_d3d12(&format!("[desc] sync off={off:#x} id={id} -> {kind}"));
    }
    materialize_record(dev_inner, host_cpu, &rec, id)
}
