//! Lazy graphics PSO storage, console pipeline-state stream parser, SM 6.6 DXIL host PSO
//! compiler, upload scratch ring, rect-list expansion, and live draw replay.
#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap};
use std::ffi::{c_void, CStr, CString};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use super::dxbc_xbox::{self, HwStage};
use super::hlsl_xbox::{self, HostShaderMeta, UdSource};
use super::rdna2_hlsl::ShaderStage;
use super::{compile_hlsl_to_dxil, depth_to_dsv, log_d3d12, map_console_format};

pub(crate) const TOPO_RECTLIST: u32 = 17;
pub(crate) const TOPO_TYPE_RECT: u32 = 19;

const IID_ROOT_SIG: [u8; 16] = [0x66, 0x6b, 0x4a, 0xc5, 0xdf, 0x72, 0xe8, 0x4e, 0x8b, 0xe5, 0xa9, 0x46, 0xa1, 0x42, 0x92, 0x14];
const IID_PSO: [u8; 16] = [0xf3, 0x30, 0x5a, 0x76, 0x24, 0xf6, 0x6f, 0x4c, 0xa8, 0x28, 0xac, 0xe9, 0x48, 0x62, 0x24, 0x45];
const IID_RESOURCE: [u8; 16] = [0xbe, 0x42, 0x64, 0x69, 0x2e, 0xa7, 0x59, 0x40, 0xbc, 0x79, 0x5b, 0x5c, 0x98, 0x04, 0x0f, 0xad];

unsafe fn vt_slot(obj: *mut c_void, slot: usize) -> usize {
    *(*(obj as *const *const usize)).add(slot)
}

#[derive(Clone)]
pub(crate) struct ShaderBlob {
    pub hash: u64,
    pub stage: &'static str,
    pub bytes: Vec<u8>,
}

static SHADERS: Mutex<Option<HashMap<u64, Arc<ShaderBlob>>>> = Mutex::new(None);

fn fnv64(b: &[u8]) -> u64 {
    b.iter().fold(0xcbf29ce484222325u64, |h, &x| (h ^ x as u64).wrapping_mul(0x100000001b3))
}

pub(crate) unsafe fn intern_shader(stage: &'static str, ptr: *const u8, len: usize) -> Option<Arc<ShaderBlob>> {
    if ptr.is_null() || len == 0 || len > (64 << 20) {
        return None;
    }
    let slice = std::slice::from_raw_parts(ptr, len);
    let hash = fnv64(slice);
    let mut map = SHADERS.lock().unwrap();
    let entry = map.get_or_insert_with(HashMap::new).entry(hash).or_insert_with(|| {
        Arc::new(ShaderBlob { hash, stage, bytes: slice.to_vec() })
    });
    Some(entry.clone())
}

// ---- Descriptor heap info & placed buffer CPU->GPU VA translation -------------------------

#[derive(Clone, Copy)]
struct HeapInfo {
    ty: u32,        // 0=CBV_SRV_UAV, 1=SAMPLER
    gpu_start: u64,
    inc: u32,
}
static HEAP_INFOS: Mutex<Option<HashMap<usize, HeapInfo>>> = Mutex::new(None);

pub(crate) fn note_heap(heap: *mut c_void, ty: u32, gpu_start: u64, inc: u32) {
    if heap.is_null() || inc == 0 {
        return;
    }
    HEAP_INFOS.lock().unwrap().get_or_insert_with(HashMap::new).insert(heap as usize, HeapInfo { ty, gpu_start, inc });
}

fn heap_info(heap: usize) -> Option<HeapInfo> {
    HEAP_INFOS.lock().unwrap().as_ref().and_then(|m| m.get(&heap).copied())
}

/// CPU VA -> (end_cpu_va, pc_gpu_va) for OpenExistingHeapFromAddress PlacedResourceX buffers.
static PLACED_BUFS: Mutex<BTreeMap<u64, (u64, u64)>> = Mutex::new(BTreeMap::new());
/// Host GPU VA -> (end_host_gpu, title_cpu_va) for disconnected PlacedResourceX fallback buffers.
static FALLBACK_BUFS: Mutex<BTreeMap<u64, (u64, u64)>> = Mutex::new(BTreeMap::new());

pub(crate) fn note_placed_buf(cpu_va: u64, width: u64, gpu_va: u64) {
    if cpu_va != 0 && width != 0 && gpu_va != 0 {
        PLACED_BUFS.lock().unwrap().insert(cpu_va, (cpu_va.saturating_add(width), gpu_va));
    }
}

pub(crate) fn note_fallback_buf(host_gpu: u64, width: u64, title_cpu_va: u64) {
    if host_gpu != 0 && width != 0 && title_cpu_va != 0 {
        FALLBACK_BUFS.lock().unwrap().insert(host_gpu, (host_gpu.saturating_add(width), title_cpu_va));
    }
}

pub(crate) fn cpu_to_placed_gpu(va: u64) -> Option<(u64, u64)> {
    let m = PLACED_BUFS.lock().unwrap();
    m.range(..=va).next_back().and_then(|(&base, &(end, gpu))| {
        if va < end { Some((gpu + (va - base), end - va)) } else { None }
    })
}

fn fallback_gpu_to_cpu(va: u64) -> Option<u64> {
    let m = FALLBACK_BUFS.lock().unwrap();
    m.range(..=va).next_back().and_then(|(&base, &(end, cpu))| {
        if va < end { Some(cpu + (va - base)) } else { None }
    })
}

// ---- Persistently mapped 128 MiB UPLOAD ring buffer ---------------------------------------

const RING_SIZE: usize = 128 << 20;

struct UploadRing {
    res: usize,
    gpu_base: u64,
    cpu_base: usize,
    cursor: usize,
    gen: u64,
}

static UPLOAD_RING: Mutex<Option<UploadRing>> = Mutex::new(None);

unsafe fn init_upload_ring(dev_inner: *mut c_void) -> Option<UploadRing> {
    let props: [u32; 5] = [2, 0, 0, 1, 1]; // UPLOAD
    let mut desc = [0u64; 7];
    desc[0] = 1; // BUFFER
    desc[2] = RING_SIZE as u64;
    desc[3] = 1 | (1u64 << 32) | (1u64 << 48); // h=1, da=1, mips=1
    desc[4] = 1u64 << 32; // fmt=0, samples=1
    desc[5] = 1u64 << 32; // layout=ROW_MAJOR
    let f: unsafe extern "system" fn(*mut c_void, *const u32, u32, *const u64, u32, *const c_void, *const u8, *mut *mut c_void) -> i32 =
        std::mem::transmute(vt_slot(dev_inner, 27));
    let mut res = std::ptr::null_mut();
    let hr = f(dev_inner, props.as_ptr(), 0, desc.as_ptr(), 0xAC3 /* GENERIC_READ */, std::ptr::null(), IID_RESOURCE.as_ptr(), &mut res);
    if hr < 0 || res.is_null() {
        log_d3d12(&format!("[gfxpso] upload ring create failed {hr:08x}"));
        return None;
    }
    let map: unsafe extern "system" fn(*mut c_void, u32, *const [usize; 2], *mut *mut u8) -> i32 = std::mem::transmute(vt_slot(res, 8));
    let empty_read = [0usize; 2];
    let mut cpu = std::ptr::null_mut();
    if map(res, 0, &empty_read, &mut cpu) < 0 || cpu.is_null() {
        return None;
    }
    let gva: unsafe extern "system" fn(*mut c_void) -> u64 = std::mem::transmute(vt_slot(res, 11));
    let gpu = gva(res);
    log_d3d12(&format!("[gfxpso] upload ring ready {} MiB gpu={gpu:#x}", RING_SIZE >> 20));
    Some(UploadRing { res: res as usize, gpu_base: gpu, cpu_base: cpu as usize, cursor: 0, gen: 1 })
}

static UPLOAD_CACHE: Mutex<Vec<(u64, usize, u64, u64, u64)>> = Mutex::new(Vec::new()); // (cpu_va, len, hash, gpu_va, gen)

unsafe fn upload_alloc(dev_inner: *mut c_void, bytes: usize, align: usize) -> Option<(u64, *mut u8)> {
    let (_res, off, cpu, gpu_base) = upload_alloc_raw(dev_inner, bytes, align)?;
    Some((gpu_base + off as u64, cpu))
}

unsafe fn upload_alloc_raw(dev_inner: *mut c_void, bytes: usize, align: usize) -> Option<(usize, usize, *mut u8, u64)> {
    if bytes == 0 || bytes > (RING_SIZE / 4) {
        return None;
    }
    let mut guard = UPLOAD_RING.lock().unwrap();
    if guard.is_none() {
        *guard = init_upload_ring(dev_inner);
    }
    let r = guard.as_mut()?;
    let mask = align.max(16) - 1;
    let mut off = (r.cursor + mask) & !mask;
    if off + bytes > RING_SIZE {
        off = 0;
        r.gen = r.gen.wrapping_add(1);
    }
    r.cursor = off + bytes;
    Some((r.res, off, (r.cpu_base + off) as *mut u8, r.gpu_base))
}

fn tex_fmt_block(fmt: u32) -> (u32, u32, u32) {
    match fmt {
        70..=72 | 79..=81 => (4, 4, 8),
        73..=78 | 82..=84 | 94..=99 => (4, 4, 16),
        1..=4 => (1, 1, 16),
        9..=14 => (1, 1, 8),
        27..=32 | 39..=46 | 67 | 87..=91 => (1, 1, 4),
        48..=59 => (1, 1, 2),
        60..=65 => (1, 1, 1),
        _ => (1, 1, 4),
    }
}

fn concrete_copy_fmt(fmt: u32) -> u32 {
    match fmt {
        1 => 2,   // R32G32B32A32_TYPELESS -> FLOAT
        9 => 10,  // R16G16B16A16_TYPELESS -> FLOAT
        27 => 28, // R8G8B8A8_TYPELESS -> UNORM
        39 => 41, // R32_TYPELESS -> FLOAT
        48 => 49, // R8G8_TYPELESS -> UNORM
        53 => 56, // R16_TYPELESS -> UNORM
        60 => 61, // R8_TYPELESS -> UNORM
        70 => 71, // BC1
        73 => 74, // BC2
        76 => 77, // BC3
        79 => 80, // BC4
        82 => 83, // BC5
        90 => 87, // B8G8R8A8_TYPELESS -> UNORM
        94 => 95, // BC6H
        97 => 98, // BC7
        f => f,
    }
}

static UPLOADED_TEX_HASH: Mutex<Option<HashMap<usize, u64>>> = Mutex::new(None);

unsafe fn upload_placed_tex_if_dirty(
    dev_inner: *mut c_void,
    list_inner: *mut c_void,
    tex: *mut c_void,
    pt: &super::desc_mirror::PlacedTexInfo,
) {
    // Only CPU-written shader resources (never overwrite GPU-rendered RTV/DSV textures).
    if (pt.flags & 0x3) != 0 || pt.cpu_va == 0 || pt.width == 0 || pt.height == 0
        || pt.width > 4096 || pt.height > 4096 || pt.depth_array > 1
    {
        return;
    }
    let (bw, bh, bpb) = tex_fmt_block(pt.fmt);
    let w = pt.width as u32;
    let h = pt.height;
    let w_blk = (w + bw - 1) / bw;
    let h_blk = (h + bh - 1) / bh;
    let row_pitch = ((w_blk * bpb + 255) & !255).max(256);
    let total = (row_pitch as usize).saturating_mul(h_blk as usize);
    if total == 0 || total > (RING_SIZE / 4) || !cpu_readable(pt.cpu_va, total) {
        return;
    }
    // Fast strided FNV hash + non-zero check over the CPU backing memory.
    let src = std::slice::from_raw_parts(pt.cpu_va as *const u8, total);
    let step = (total / 1024).max(1);
    let mut hval: u64 = 0xcbf29ce484222325 ^ (total as u64);
    let mut any_nz = false;
    let mut i = 0usize;
    while i < total {
        let b = src[i];
        if b != 0 { any_nz = true; }
        hval ^= b as u64;
        hval = hval.wrapping_mul(0x100000001b3);
        i += step;
    }
    // Also scan first 256 bytes tightly (small textures like 256x1 gradients or 1x1).
    for &b in src.iter().take(256) {
        if b != 0 { any_nz = true; }
        hval ^= b as u64;
        hval = hval.wrapping_mul(0x100000001b3);
    }
    if !any_nz {
        return;
    }
    {
        let mut map = UPLOADED_TEX_HASH.lock().unwrap();
        let m = map.get_or_insert_with(HashMap::new);
        if m.get(&(tex as usize)) == Some(&hval) {
            return;
        }
        m.insert(tex as usize, hval);
    }
    let Some((ring_res, ring_off, dst_ptr, _)) = upload_alloc_raw(dev_inner, total, 512) else {
        return;
    };
    std::ptr::copy_nonoverlapping(src.as_ptr(), dst_ptr, total);

    // Transition tex -> COPY_DEST, CopyTextureRegion, transition back -> ALL_SHADER_RESOURCE.
    let make_bar = |before: u32, after: u32| -> [u8; 32] {
        let mut b = [0u8; 32];
        b[8..16].copy_from_slice(&(tex as usize).to_le_bytes());
        b[16..20].copy_from_slice(&0xffff_ffffu32.to_le_bytes());
        b[20..24].copy_from_slice(&before.to_le_bytes());
        b[24..28].copy_from_slice(&after.to_le_bytes());
        b
    };
    let bar_fn: unsafe extern "system" fn(*mut c_void, u32, *const [u8; 32]) = std::mem::transmute(vt_slot(list_inner, 26));
    let ctr_fn: unsafe extern "system" fn(*mut c_void, *const u8, u32, u32, u32, *const u8, *const u32) =
        std::mem::transmute(vt_slot(list_inner, 16));

    let pre = make_bar(0xC0, 0x400);
    bar_fn(list_inner, 1, &pre);

    let mut dloc = [0u8; 48];
    dloc[0..8].copy_from_slice(&(tex as usize).to_le_bytes());
    // Type = 0 (SUBRESOURCE_INDEX), SubresourceIndex = 0
    let mut sloc = [0u8; 48];
    sloc[0..8].copy_from_slice(&ring_res.to_le_bytes());
    sloc[8..12].copy_from_slice(&1u32.to_le_bytes()); // PLACED_FOOTPRINT
    sloc[16..24].copy_from_slice(&(ring_off as u64).to_le_bytes());
    sloc[24..28].copy_from_slice(&concrete_copy_fmt(pt.fmt).to_le_bytes());
    sloc[28..32].copy_from_slice(&(w_blk * bw).to_le_bytes());
    sloc[32..36].copy_from_slice(&(h_blk * bh).to_le_bytes());
    sloc[36..40].copy_from_slice(&1u32.to_le_bytes());
    sloc[40..44].copy_from_slice(&row_pitch.to_le_bytes());

    ctr_fn(list_inner, dloc.as_ptr(), 0, 0, 0, sloc.as_ptr(), std::ptr::null());

    let post = make_bar(0x400, 0xC0);
    bar_fn(list_inner, 1, &post);

    static UN: AtomicU32 = AtomicU32::new(0);
    if UN.fetch_add(1, Ordering::Relaxed) < 24 {
        log_d3d12(&format!(
            "[gfxpso] UPLOAD PLACED TEX {tex:p} va={:#x} {}x{} fmt={} clayout={:#x} bytes={total} pitch={row_pitch}",
            pt.cpu_va, w, h, pt.fmt, pt.console_layout
        ));
    }
}

pub(crate) unsafe fn cpu_readable(addr: u64, len: usize) -> bool {
    if addr < 0x10000 || len == 0 {
        return false;
    }
    super::cpu_writes::usable(addr as usize, len, false)
}

/// Copies `copy_len` bytes from CPU-readable `src_va` into the upload ring and returns
/// `(ring_resource, ring_byte_offset)` for creating a raw SRV over the copied bytes.
pub(crate) unsafe fn upload_copy_res(
    dev_inner: *mut c_void,
    src_va: u64,
    copy_len: usize,
    align: usize,
) -> Option<(*mut c_void, usize)> {
    if !cpu_readable(src_va, copy_len) || copy_len == 0 || copy_len > (RING_SIZE / 4) {
        return None;
    }
    let mut guard = UPLOAD_RING.lock().unwrap();
    if guard.is_none() {
        *guard = init_upload_ring(dev_inner);
    }
    let r = guard.as_mut()?;
    let mask = align.max(16) - 1;
    let mut off = (r.cursor + mask) & !mask;
    if off + copy_len > RING_SIZE {
        off = 0;
        r.gen = r.gen.wrapping_add(1);
    }
    r.cursor = off + copy_len;
    std::ptr::copy_nonoverlapping(src_va as *const u8, (r.cpu_base + off) as *mut u8, copy_len);
    Some((r.res as *mut c_void, off))
}

/// Copies `copy_len` bytes from CPU-readable `src_va` into a zero-padded `alloc_len`-byte
/// slot in the upload ring, returning the host GPU VA.
pub(crate) unsafe fn upload_copy(
    dev_inner: *mut c_void,
    src_va: u64,
    copy_len: usize,
    alloc_len: usize,
    align: usize,
) -> Option<u64> {
    if !cpu_readable(src_va, copy_len) || alloc_len < copy_len {
        return None;
    }
    let (gpu, dst) = upload_alloc(dev_inner, alloc_len, align)?;
    std::ptr::copy_nonoverlapping(src_va as *const u8, dst, copy_len);
    if alloc_len > copy_len {
        std::ptr::write_bytes(dst.add(copy_len), 0, alloc_len - copy_len);
    }
    Some(gpu)
}

/// Resolves a title GPU/CPU address to a valid host GPU VA for reading `len` bytes.
unsafe fn resolve_read_va(dev_inner: *mut c_void, va: u64, len: usize, align: usize) -> Option<u64> {
    if va == 0 || len == 0 {
        return None;
    }
    // 0. Disconnected PlacedResourceX fallback buffer GPU VA -> remap back to title CPU memory!
    let (va, force_cpu) = match fallback_gpu_to_cpu(va) {
        Some(cpu) => (cpu, true),
        None => (va, false),
    };
    if !force_cpu {
        // 1. Already a tracked PC GPU buffer VA?
        if let Some(ext) = super::replay::buf_extent(va) {
            if ext >= len as u64 {
                return Some(va);
            }
        }
        // 2. Title CPU VA inside an OpenExistingHeapFromAddress PlacedResourceX buffer?
        if let Some((gpu, ext)) = cpu_to_placed_gpu(va) {
            if ext >= len as u64 && (gpu as usize) % align.max(1) == 0 {
                return Some(gpu);
            }
        }
    }
    // 3. Ordinary CPU-readable title memory -> copy into persistently mapped upload ring (with cache for big VBs/IBs).
    if cpu_readable(va, len) {
        if len >= 4096 {
            let src = std::slice::from_raw_parts(va as *const u8, len);
            let step = (len / 256).max(1);
            let mut h: u64 = 0xcbf29ce484222325 ^ (len as u64);
            let mut i = 0;
            while i < len {
                h = (h ^ src[i] as u64).wrapping_mul(0x100000001b3);
                i += step;
            }
            let cur_gen = UPLOAD_RING.lock().unwrap().as_ref().map(|r| r.gen).unwrap_or(0);
            if cur_gen != 0 {
                let cache = UPLOAD_CACHE.lock().unwrap();
                if let Some(&(_, _, _, gpu, _)) = cache.iter().find(|e| e.0 == va && e.1 == len && e.2 == h && e.4 == cur_gen) {
                    return Some(gpu);
                }
            }
            let (gpu, dst) = upload_alloc(dev_inner, len, align)?;
            std::ptr::copy_nonoverlapping(src.as_ptr(), dst, len);
            let new_gen = UPLOAD_RING.lock().unwrap().as_ref().map(|r| r.gen).unwrap_or(0);
            let mut cache = UPLOAD_CACHE.lock().unwrap();
            cache.retain(|e| e.4 == new_gen);
            if cache.len() >= 32 { cache.remove(0); }
            cache.push((va, len, h, gpu, new_gen));
            return Some(gpu);
        }
        let (gpu, dst) = upload_alloc(dev_inner, len, align)?;
        std::ptr::copy_nonoverlapping(va as *const u8, dst, len);
        return Some(gpu);
    }
    None
}

// ---- Graphics PSO descriptor & lazy compilation -------------------------------------------

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct InputElemRaw {
    pub semantic_name: *const u8,
    pub semantic_index: u32,
    pub format: u32,
    pub input_slot: u32,
    pub aligned_byte_offset: u32,
    pub input_slot_class: u32,
    pub instance_data_step_rate: u32,
}
unsafe impl Send for InputElemRaw {}
unsafe impl Sync for InputElemRaw {}

#[derive(Clone)]
pub(crate) struct GfxPsoDesc {
    pub device_inner: usize,
    pub root_sig: usize,
    pub vs: Option<Arc<ShaderBlob>>,
    pub ps: Option<Arc<ShaderBlob>>,
    pub ms: Option<Arc<ShaderBlob>>,
    pub as_: Option<Arc<ShaderBlob>>,
    pub cs: Option<Arc<ShaderBlob>>,
    pub has_other_stage: bool,
    pub blend: [u8; 328],
    pub sample_mask: u32,
    pub rasterizer: [u8; 44],
    pub depth_stencil: [u8; 52],
    pub input_names: Vec<CString>,
    pub input_elems: Vec<InputElemRaw>,
    pub strip_cut: u32,
    pub topology_type: u32,
    pub num_rt: u32,
    pub rt_formats: [u32; 8],
    pub dsv_format: u32,
    pub sample_count: u32,
    pub sample_quality: u32,
}

impl Default for GfxPsoDesc {
    fn default() -> Self {
        let mut blend = [0u8; 328];
        for i in 0..8 {
            let b = 8 + i * 40;
            blend[b + 8..b + 12].copy_from_slice(&2u32.to_le_bytes());  // SrcBlend = ONE
            blend[b + 12..b + 16].copy_from_slice(&1u32.to_le_bytes()); // DestBlend = ZERO
            blend[b + 16..b + 20].copy_from_slice(&1u32.to_le_bytes()); // BlendOp = ADD
            blend[b + 20..b + 24].copy_from_slice(&2u32.to_le_bytes()); // SrcBlendAlpha = ONE
            blend[b + 24..b + 28].copy_from_slice(&1u32.to_le_bytes()); // DestBlendAlpha = ZERO
            blend[b + 28..b + 32].copy_from_slice(&1u32.to_le_bytes()); // BlendOpAlpha = ADD
            blend[b + 32..b + 36].copy_from_slice(&4u32.to_le_bytes()); // LogicOp = NOOP
            blend[b + 36] = 0x0F;
        }
        let mut rasterizer = [0u8; 44];
        rasterizer[0..4].copy_from_slice(&3u32.to_le_bytes());
        rasterizer[4..8].copy_from_slice(&3u32.to_le_bytes());
        rasterizer[24..28].copy_from_slice(&1u32.to_le_bytes());
        let mut depth_stencil = [0u8; 52];
        depth_stencil[4..8].copy_from_slice(&1u32.to_le_bytes());
        depth_stencil[8..12].copy_from_slice(&2u32.to_le_bytes());
        depth_stencil[16] = 0xFF;
        depth_stencil[17] = 0xFF;
        for base in [20usize, 36usize] {
            depth_stencil[base..base + 4].copy_from_slice(&1u32.to_le_bytes());
            depth_stencil[base + 4..base + 8].copy_from_slice(&1u32.to_le_bytes());
            depth_stencil[base + 8..base + 12].copy_from_slice(&1u32.to_le_bytes());
            depth_stencil[base + 12..base + 16].copy_from_slice(&8u32.to_le_bytes());
        }
        Self {
            device_inner: 0,
            root_sig: 0,
            vs: None,
            ps: None,
            ms: None,
            as_: None,
            cs: None,
            has_other_stage: false,
            blend,
            sample_mask: u32::MAX,
            rasterizer,
            depth_stencil,
            input_names: Vec::new(),
            input_elems: Vec::new(),
            strip_cut: 0,
            topology_type: 3,
            num_rt: 0,
            rt_formats: [0; 8],
            dsv_format: 0,
            sample_count: 1,
            sample_quality: 0,
        }
    }
}

struct StageBind {
    ud_param_idx: Option<u32>,
    ud_sources: Vec<UdSource>,
    root_srvs: Vec<(u8, u32)>, // (title_root_slot, host_param_idx)
    table_reads: Vec<(u8, u32, bool, bool)>, // (title_root_slot, rel_byte_off, is_sampler, is_cbv)
}

pub(crate) struct CompiledPso {
    pub host_pso: usize,
    pub host_rs: usize,
    vs_bind: StageBind,
    ps_bind: StageBind,
    rect_elems: Vec<(u32, u32)>, // (byte_offset, dxgi_format) for slot 0 rect expansion
}

// ---- Host Root Signature & PSO creation ---------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy)]
struct RootParam1 {
    param_type: u32,
    _pad0: u32,
    a: u32, // ShaderRegister
    b: u32, // RegisterSpace
    c: u32, // Num32BitValues or Flags
    d: u32,
    visibility: u32,
    _pad1: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct StaticSamplerDesc {
    filter: u32,
    addr_u: u32,
    addr_v: u32,
    addr_w: u32,
    mip_lod_bias: f32,
    max_aniso: u32,
    comp_func: u32,
    border_color: u32,
    min_lod: f32,
    max_lod: f32,
    shader_reg: u32,
    reg_space: u32,
    visibility: u32,
}

#[repr(C)]
struct RootSigDesc1 {
    num_params: u32,
    p_params: *const RootParam1,
    num_samplers: u32,
    p_samplers: *const StaticSamplerDesc,
    flags: u32,
}

#[repr(C)]
struct VersionedRootSigDesc {
    version: u32,
    desc1: RootSigDesc1,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct ShaderBytecode {
    ptr: *const u8,
    len: usize,
}

#[repr(C)]
struct StreamOutputDesc {
    p_decl: *const c_void,
    num_entries: u32,
    p_strides: *const u32,
    num_strides: u32,
    rast_stream: u32,
}

#[repr(C)]
struct InputLayoutDesc {
    p_elems: *const InputElemRaw,
    num_elems: u32,
}

#[repr(C)]
struct GraphicsPsoDescPc {
    p_root_sig: *mut c_void,
    vs: ShaderBytecode,
    ps: ShaderBytecode,
    ds: ShaderBytecode,
    hs: ShaderBytecode,
    gs: ShaderBytecode,
    stream_output: StreamOutputDesc,
    blend: [u8; 328],
    sample_mask: u32,
    rasterizer: [u8; 44],
    depth_stencil: [u8; 52],
    input_layout: InputLayoutDesc,
    ib_strip_cut: u32,
    topo_type: u32,
    num_rts: u32,
    rt_formats: [u32; 8],
    dsv_format: u32,
    sample_count: u32,
    sample_quality: u32,
    node_mask: u32,
    cached_pso: ShaderBytecode,
    flags: u32,
}

fn decode_gfx10_sampler(w: [u32; 4], shader_reg: u32, reg_space: u32, visibility: u32) -> StaticSamplerDesc {
    let bits = |word: u32, at: u32, n: u32| (word >> at) & ((1u32 << n) - 1);
    let addr = |m: u32| [1u32, 2, 3, 5, 3, 5, 4, 5][(m & 7) as usize];
    let mag = bits(w[2], 20, 2);
    let min = bits(w[2], 22, 2);
    let mip = bits(w[2], 26, 2);
    let red = match bits(w[0], 29, 2) { 1 => 2, 2 => 3, _ => 0 };
    let filter = if mag >= 2 || min >= 2 {
        0x55 | (red << 7)
    } else {
        ((min & 1) << 4) | ((mag & 1) << 2) | u32::from(mip == 2) | (red << 7)
    };
    let (min_lod, max_lod) = if mip == 0 {
        (0.0, 0.0)
    } else {
        (bits(w[1], 0, 12) as f32 / 256.0, bits(w[1], 12, 12) as f32 / 256.0)
    };
    let bias = ((bits(w[2], 0, 14) << 18) as i32 >> 18) as f32 / 256.0;
    StaticSamplerDesc {
        filter,
        addr_u: addr(bits(w[0], 0, 3)),
        addr_v: addr(bits(w[0], 3, 3)),
        addr_w: addr(bits(w[0], 6, 3)),
        mip_lod_bias: bias,
        max_aniso: 1 << bits(w[0], 9, 3).min(4),
        comp_func: bits(w[0], 12, 3) + 1,
        border_color: bits(w[3], 30, 2) % 3,
        min_lod,
        max_lod,
        shader_reg,
        reg_space,
        visibility,
    }
}

unsafe fn build_host_rs(
    dev_inner: *mut c_void,
    vs: &HostShaderMeta,
    ps: Option<&HostShaderMeta>,
    has_ia: bool,
) -> Result<(*mut c_void, StageBind, StageBind), String> {
    let mut params: Vec<RootParam1> = Vec::new();
    let mut samplers: Vec<StaticSamplerDesc> = Vec::new();

    let mut add_stage = |meta: &HostShaderMeta, vis: u32| -> StageBind {
        let ud_param_idx = if meta.ud_dwords > 0 {
            let idx = params.len() as u32;
            params.push(RootParam1 {
                param_type: 1, // 32BIT_CONSTANTS
                _pad0: 0,
                a: 0, // b0
                b: 0, // space0
                c: meta.ud_dwords,
                d: 0,
                visibility: vis,
                _pad1: 0,
            });
            Some(idx)
        } else {
            None
        };
        let mut root_srvs = Vec::new();
        for &slot in &meta.root_descs {
            let idx = params.len() as u32;
            params.push(RootParam1 {
                param_type: 3, // SRV
                _pad0: 0,
                a: slot as u32,
                b: 1, // space1
                c: 0, // flags
                d: 0,
                visibility: vis,
                _pad1: 0,
            });
            root_srvs.push((slot, idx));
        }
        for (k, &sw) in meta.static_samplers.iter().enumerate() {
            samplers.push(decode_gfx10_sampler(sw, k as u32, 2, vis));
        }
        StageBind { ud_param_idx, ud_sources: meta.ud_sources.clone(), root_srvs, table_reads: Vec::new() }
    };

    let vs_bind = add_stage(vs, 1 /* VERTEX */);
    let ps_bind = match ps {
        Some(m) => add_stage(m, 5 /* PIXEL */),
        None => StageBind { ud_param_idx: None, ud_sources: Vec::new(), root_srvs: Vec::new(), table_reads: Vec::new() },
    };

    // ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT (0x1) | CBV_SRV_UAV_HEAP_DIRECTLY_INDEXED (0x400) | SAMPLER_HEAP_DIRECTLY_INDEXED (0x800)
    let flags = (if has_ia { 0x1 } else { 0 }) | 0x400 | 0x800;
    let vdesc = VersionedRootSigDesc {
        version: 2, // D3D_ROOT_SIGNATURE_VERSION_1_1
        desc1: RootSigDesc1 {
            num_params: params.len() as u32,
            p_params: if params.is_empty() { std::ptr::null() } else { params.as_ptr() },
            num_samplers: samplers.len() as u32,
            p_samplers: if samplers.is_empty() { std::ptr::null() } else { samplers.as_ptr() },
            flags,
        },
    };

    extern "system" {
        fn LoadLibraryA(n: *const u8) -> usize;
        fn GetProcAddress(h: usize, n: *const u8) -> usize;
    }
    let d3d12 = LoadLibraryA(b"d3d12.dll\0".as_ptr());
    let ser_fn = GetProcAddress(d3d12, b"D3D12SerializeVersionedRootSignature\0".as_ptr());
    if ser_fn == 0 {
        return Err("D3D12SerializeVersionedRootSignature missing".into());
    }
    let serialize: unsafe extern "system" fn(*const VersionedRootSigDesc, *mut *mut c_void, *mut *mut c_void) -> i32 =
        std::mem::transmute(ser_fn);
    let mut blob = std::ptr::null_mut();
    let mut err_blob = std::ptr::null_mut();
    let hr = serialize(&vdesc, &mut blob, &mut err_blob);
    if hr < 0 || blob.is_null() {
        let mut msg = format!("SerializeVersionedRS failed {hr:08x}");
        if !err_blob.is_null() {
            let gp: unsafe extern "system" fn(*mut c_void) -> *const u8 = std::mem::transmute(vt_slot(err_blob, 3));
            let gs: unsafe extern "system" fn(*mut c_void) -> usize = std::mem::transmute(vt_slot(err_blob, 4));
            let rel: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(vt_slot(err_blob, 2));
            let p = gp(err_blob);
            let s = gs(err_blob);
            if !p.is_null() && s > 0 {
                msg = format!("{msg}: {}", String::from_utf8_lossy(std::slice::from_raw_parts(p, s)).trim());
            }
            rel(err_blob);
        }
        return Err(msg);
    }
    let gp: unsafe extern "system" fn(*mut c_void) -> *const u8 = std::mem::transmute(vt_slot(blob, 3));
    let gs: unsafe extern "system" fn(*mut c_void) -> usize = std::mem::transmute(vt_slot(blob, 4));
    let rel: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(vt_slot(blob, 2));
    let create_rs: unsafe extern "system" fn(*mut c_void, u32, *const u8, usize, *const u8, *mut *mut c_void) -> i32 =
        std::mem::transmute(vt_slot(dev_inner, 16));
    let mut rs = std::ptr::null_mut();
    let hr = create_rs(dev_inner, 0, gp(blob), gs(blob), IID_ROOT_SIG.as_ptr(), &mut rs);
    rel(blob);
    if hr < 0 || rs.is_null() {
        return Err(format!("CreateRootSignature failed {hr:08x}"));
    }
    Ok((rs, vs_bind, ps_bind))
}

fn format_bytes(fmt: u32) -> u32 {
    match fmt {
        1..=4 => 16,
        5..=8 => 12,
        9..=22 => 8,
        23..=47 | 67 | 87..=93 => 4,
        48..=59 | 68 | 69 | 85 | 86 => 2,
        60..=65 => 1,
        _ => 4,
    }
}

unsafe fn compile_lazy_pso(lp: &LazyPso) -> Option<CompiledPso> {
    let d = &lp.desc;
    if d.device_inner == 0 || d.has_other_stage || d.ms.is_some() || d.cs.is_some() {
        log_d3d12(&format!(
            "[gfxpso] skip compile dev={:#x} other={} ms={} cs={}",
            d.device_inner, d.has_other_stage, d.ms.is_some(), d.cs.is_some()
        ));
        return None;
    }
    let vs_blob = d.vs.as_ref()?;
    let vs_prog = match dxbc_xbox::parse_container(&vs_blob.bytes, Some(HwStage::Vs)) {
        Ok(p) => p,
        Err(e) => {
            log_d3d12(&format!("[gfxpso] VS {:016x} parse fail: {e}", vs_blob.hash));
            return None;
        }
    };
    let vs_meta = match hlsl_xbox::translate(&vs_prog) {
        Ok(m) => m,
        Err(e) => {
            log_d3d12(&format!("[gfxpso] VS {:016x} lift fail: {e}", vs_blob.hash));
            return None;
        }
    };
    let vs_dxil = match compile_hlsl_to_dxil(&vs_meta.hlsl, ShaderStage::Vertex) {
        Ok(b) => b,
        Err(e) => {
            let _ = std::fs::write(super::pso_guard::dir().join(format!("fail_vs_{:016x}.hlsl", vs_blob.hash)), &vs_meta.hlsl);
            log_d3d12(&format!("[gfxpso] VS {:016x} DXC fail: {}", vs_blob.hash, e.lines().next().unwrap_or("")));
            return None;
        }
    };

    let (ps_meta_opt, ps_dxil_opt, ps_ur_dbg, ps_table_reads) = if let Some(ps_blob) = d.ps.as_ref() {
        let ps_prog = match dxbc_xbox::parse_container(&ps_blob.bytes, Some(HwStage::Ps)) {
            Ok(p) => p,
            Err(e) => {
                log_d3d12(&format!("[gfxpso] PS {:016x} parse fail: {e}", ps_blob.hash));
                return None;
            }
        };
        let ur_dbg: Vec<(u8, String, u32)> = ps_prog.used_res.iter().map(|u| (u.root_slot, format!("{:?}", u.res_kind), u.table_offset_dw)).collect();
        let t_reads: Vec<(u8, u32, bool, bool)> = ps_prog.used_res.iter()
            .filter(|u| u.root_kind == dxbc_xbox::RootParamKind::Table)
            .map(|u| (u.root_slot, u.table_offset_dw * 4, u.res_kind == dxbc_xbox::ResKind::Sampler, u.res_kind == dxbc_xbox::ResKind::Cbv))
            .collect();
        let ps_meta = match hlsl_xbox::translate(&ps_prog) {
            Ok(m) => m,
            Err(e) => {
                log_d3d12(&format!("[gfxpso] PS {:016x} lift fail: {e}", ps_blob.hash));
                return None;
            }
        };
        let ps_dxil = match compile_hlsl_to_dxil(&ps_meta.hlsl, ShaderStage::Pixel) {
            Ok(b) => b,
            Err(e) => {
                let _ = std::fs::write(super::pso_guard::dir().join(format!("fail_ps_{:016x}.hlsl", ps_blob.hash)), &ps_meta.hlsl);
                log_d3d12(&format!("[gfxpso] PS {:016x} DXC fail: {}", ps_blob.hash, e.lines().next().unwrap_or("")));
                return None;
            }
        };
        (Some(ps_meta), Some(ps_dxil), ur_dbg, t_reads)
    } else {
        (None, None, Vec::new(), Vec::new())
    };

    let dev_inner = d.device_inner as *mut c_void;
    let (host_rs, mut vs_bind, mut ps_bind) = match build_host_rs(dev_inner, &vs_meta, ps_meta_opt.as_ref(), !d.input_elems.is_empty()) {
        Ok(x) => x,
        Err(e) => {
            log_d3d12(&format!("[gfxpso] RS fail vs={:016x}: {e}", vs_blob.hash));
            return None;
        }
    };
    vs_bind.table_reads = vs_prog.used_res.iter()
        .filter(|u| u.root_kind == dxbc_xbox::RootParamKind::Table)
        .map(|u| (u.root_slot, u.table_offset_dw * 4, u.res_kind == dxbc_xbox::ResKind::Sampler, u.res_kind == dxbc_xbox::ResKind::Cbv))
        .collect();
    ps_bind.table_reads = ps_table_reads;

    // Sanitize depth-stencil state if no DSV format is specified.
    let mut ds = d.depth_stencil;
    if d.dsv_format == 0 {
        ds[0..4].copy_from_slice(&0u32.to_le_bytes()); // DepthEnable = FALSE
        ds[12..16].copy_from_slice(&0u32.to_le_bytes()); // StencilEnable = FALSE
    }

    let topo_type = match d.topology_type {
        TOPO_TYPE_RECT | 32 => 3, // TRIANGLE
        0 => 3,
        t => t,
    };

    let mut rt_formats = d.rt_formats;
    let mut num_rts = d.num_rt;
    if num_rts == 0 && d.dsv_format == 0 && ps_dxil_opt.is_some() {
        num_rts = 1;
        rt_formats[0] = 28; // R8G8B8A8_UNORM fallback
    }

    let pso_desc = GraphicsPsoDescPc {
        p_root_sig: host_rs,
        vs: ShaderBytecode { ptr: vs_dxil.as_ptr(), len: vs_dxil.len() },
        ps: match &ps_dxil_opt {
            Some(b) => ShaderBytecode { ptr: b.as_ptr(), len: b.len() },
            None => ShaderBytecode { ptr: std::ptr::null(), len: 0 },
        },
        ds: ShaderBytecode { ptr: std::ptr::null(), len: 0 },
        hs: ShaderBytecode { ptr: std::ptr::null(), len: 0 },
        gs: ShaderBytecode { ptr: std::ptr::null(), len: 0 },
        stream_output: StreamOutputDesc { p_decl: std::ptr::null(), num_entries: 0, p_strides: std::ptr::null(), num_strides: 0, rast_stream: 0 },
        blend: d.blend,
        sample_mask: d.sample_mask,
        rasterizer: d.rasterizer,
        depth_stencil: ds,
        input_layout: InputLayoutDesc {
            p_elems: if d.input_elems.is_empty() { std::ptr::null() } else { d.input_elems.as_ptr() },
            num_elems: d.input_elems.len() as u32,
        },
        ib_strip_cut: d.strip_cut,
        topo_type,
        num_rts,
        rt_formats,
        dsv_format: d.dsv_format,
        sample_count: d.sample_count.max(1),
        sample_quality: d.sample_quality,
        node_mask: 0,
        cached_pso: ShaderBytecode { ptr: std::ptr::null(), len: 0 },
        flags: 0,
    };

    let create_pso: unsafe extern "system" fn(*mut c_void, *const GraphicsPsoDescPc, *const u8, *mut *mut c_void) -> i32 =
        std::mem::transmute(vt_slot(dev_inner, 10));
    let mut host_pso = std::ptr::null_mut();
    let mut hr = create_pso(dev_inner, &pso_desc, IID_PSO.as_ptr(), &mut host_pso);
    if hr < 0 || host_pso.is_null() {
        let vs_sig = vs_dxil.len() >= 20 && vs_dxil[4..20].iter().any(|&b| b != 0);
        let ps_sig = ps_dxil_opt.as_ref().map(|b| b.len() >= 20 && b[4..20].iter().any(|&x| x != 0)).unwrap_or(true);
        let r_u32 = |buf: &[u8], n: usize| -> Vec<u32> {
            (0..n).map(|i| u32::from_le_bytes([buf[i*4], buf[i*4+1], buf[i*4+2], buf[i*4+3]])).collect()
        };
        log_d3d12(&format!(
            "[gfxpso] CreateGraphicsPSO failed {hr:08x} vs={:016x}(sig={vs_sig},len={}) ps={:?}(sig={ps_sig}) rts={num_rts}:{:?} dsv={} topo={topo_type} inputs={} blend={:x?} rast={:x?} ds={:x?}",
            vs_blob.hash, vs_dxil.len(), d.ps.as_ref().map(|p| format!("{:016x}", p.hash)),
            &rt_formats[..num_rts as usize], d.dsv_format, d.input_elems.len(),
            r_u32(&d.blend, 12), r_u32(&d.rasterizer, 10), r_u32(&ds, 13)
        ));
        // Retry with clean default state blocks (blend/rasterizer/depth-stencil) to isolate console state bits.
        let def = GfxPsoDesc::default();
        let mut pso_retry = pso_desc;
        pso_retry.blend = def.blend;
        pso_retry.rasterizer = def.rasterizer;
        pso_retry.rasterizer[4..8].copy_from_slice(&1u32.to_le_bytes()); // CULL_NONE
        pso_retry.depth_stencil = def.depth_stencil;
        pso_retry.sample_mask = u32::MAX;
        pso_retry.ib_strip_cut = 0;
        hr = create_pso(dev_inner, &pso_retry, IID_PSO.as_ptr(), &mut host_pso);
        if hr >= 0 && !host_pso.is_null() {
            log_d3d12(&format!("[gfxpso] CreateGraphicsPSO RETRY WITH DEFAULT STATE SUCCEEDED vs={:016x}", vs_blob.hash));
        } else {
            let _ = std::fs::write(super::pso_guard::dir().join(format!("fail_pso_vs_{:016x}.hlsl", vs_blob.hash)), &vs_meta.hlsl);
            if let Some(pm) = &ps_meta_opt {
                let _ = std::fs::write(super::pso_guard::dir().join(format!("fail_pso_ps_{:016x}.hlsl", d.ps.as_ref().unwrap().hash)), &pm.hlsl);
            }
            log_d3d12(&format!("[gfxpso] CreateGraphicsPSO retry ALSO failed {hr:08x} vs={:016x}", vs_blob.hash));
            return None;
        }
    }

    // Precompute slot-0 element byte offsets for rect-list vertex expansion.
    let mut rect_elems = Vec::new();
    let mut next_off = 0u32;
    for e in &d.input_elems {
        if e.input_slot == 0 && e.input_slot_class == 0 {
            let off = if e.aligned_byte_offset == 0xFFFF_FFFF { next_off } else { e.aligned_byte_offset };
            rect_elems.push((off, e.format));
            next_off = off + format_bytes(e.format);
        }
    }

    log_d3d12(&format!(
        "[gfxpso] COMPILED OK vs={:016x}(unk={},ud={},rd={},ur={:?}) ps={:?}(unk={},ur={:?}) rts={num_rts}:{:?} dsv={} inputs={}",
        vs_blob.hash, vs_meta.unknown_ops, vs_meta.ud_dwords, vs_meta.root_descs.len(),
        vs_prog.used_res.iter().map(|u| (u.root_slot, format!("{:?}", u.res_kind), u.table_offset_dw)).collect::<Vec<_>>(),
        d.ps.as_ref().map(|p| format!("{:016x}", p.hash)),
        ps_meta_opt.as_ref().map(|m| m.unknown_ops).unwrap_or(0),
        ps_ur_dbg,
        &rt_formats[..num_rts as usize], d.dsv_format, d.input_elems.len()
    ));
    {
        static DUMP_N: AtomicU32 = AtomicU32::new(0);
        if DUMP_N.fetch_add(1, Ordering::Relaxed) < 12 {
            let dir = super::pso_guard::dir();
            let _ = std::fs::write(dir.join(format!("pso_vs_{:016x}.hlsl", vs_blob.hash)), &vs_meta.hlsl);
            if let Some(pm) = &ps_meta_opt {
                let _ = std::fs::write(dir.join(format!("pso_ps_{:016x}.hlsl", d.ps.as_ref().unwrap().hash)), &pm.hlsl);
            }
        }
    }
    crate::present::request_capture();

    Some(CompiledPso {
        host_pso: host_pso as usize,
        host_rs: host_rs as usize,
        vs_bind,
        ps_bind,
        rect_elems,
    })
}

// ---- Rect list (3-vertex -> 6-vertex 2-triangle) expansion --------------------------------

fn f16_to_f32(h: u16) -> f32 {
    let s = ((h >> 15) as u32) << 31;
    let e = ((h >> 10) & 0x1F) as i32;
    let m = (h & 0x3FF) as u32;
    if e == 0 {
        if m == 0 { f32::from_bits(s) } else { f32::from_bits(s | (((127 - 14) as u32) << 23)) * (m as f32 / 1024.0) }
    } else if e == 31 {
        f32::from_bits(s | 0x7F80_0000 | (m << 13))
    } else {
        f32::from_bits(s | (((e + 112) as u32) << 23) | (m << 13))
    }
}

fn f32_to_f16(v: f32) -> u16 {
    let b = v.to_bits();
    let s = ((b >> 16) & 0x8000) as u16;
    let e = ((b >> 23) & 0xFF) as i32 - 127 + 15;
    let m = b & 0x7FFFFF;
    if e <= 0 { s } else if e >= 31 { s | 0x7C00 } else { s | ((e as u16) << 10) | ((m >> 13) as u16) }
}

fn expand_rects(src: &[u8], dst: &mut [u8], stride: usize, elems: &[(u32, u32)]) {
    for (rect, out) in src.chunks_exact(3 * stride).zip(dst.chunks_exact_mut(6 * stride)) {
        let v = |i: usize| &rect[i * stride..(i + 1) * stride];
        for (slot, &i) in [0usize, 1, 2, 0, 2].iter().enumerate() {
            out[slot * stride..(slot + 1) * stride].copy_from_slice(v(i));
        }
        let v3 = &mut out[5 * stride..6 * stride];
        v3.copy_from_slice(v(0));
        for &(off, fmt) in elems {
            let o = off as usize;
            let (n, kind) = match fmt {
                2 => (4, 0), 6 => (3, 0), 16 => (2, 0), 41 => (1, 0), // F32
                10 => (4, 1), 34 => (2, 1), 54 => (1, 1),             // F16
                11 => (4, 2), 35 => (2, 2),                           // UNORM16
                28 | 29 | 87 | 91 => (4, 3),                          // UNORM8
                _ => continue,
            };
            let bpc = match kind { 0 => 4, 1 | 2 => 2, _ => 1 };
            if o + n * bpc > stride { continue; }
            for c in 0..n {
                let at = o + c * bpc;
                let rd = |i: usize| -> f32 {
                    let b = &v(i)[at..at + bpc];
                    match kind {
                        0 => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                        1 => f16_to_f32(u16::from_le_bytes([b[0], b[1]])),
                        2 => u16::from_le_bytes([b[0], b[1]]) as f32 / 65535.0,
                        _ => b[0] as f32 / 255.0,
                    }
                };
                let val = rd(0) + rd(2) - rd(1);
                let out_b = &mut v3[at..at + bpc];
                match kind {
                    0 => out_b.copy_from_slice(&val.to_le_bytes()),
                    1 => out_b.copy_from_slice(&f32_to_f16(val).to_le_bytes()),
                    2 => out_b.copy_from_slice(&((val.clamp(0.0, 1.0) * 65535.0).round() as u16).to_le_bytes()),
                    _ => out_b[0] = (val.clamp(0.0, 1.0) * 255.0).round() as u8,
                }
            }
        }
    }
}

// ---- LazyPso COM object -------------------------------------------------------------------

#[repr(C)]
pub(crate) struct LazyPso {
    vtbl: usize,
    refcount: AtomicU32,
    pad0: u32,
    pkt: [u32; 7],
    pad1: u32,
    pub desc: GfxPsoDesc,
    logged: std::sync::atomic::AtomicBool,
    pub compiled: OnceLock<Option<CompiledPso>>,
}

static LAZY_VTBL: OnceLock<usize> = OnceLock::new();

unsafe extern "system" fn lp_qi(this: *mut LazyPso, _riid: *const c_void, out: *mut *mut c_void) -> i32 {
    if out.is_null() { return 0x8000_4003u32 as i32; }
    (*this).refcount.fetch_add(1, Ordering::Relaxed);
    *out = this as *mut c_void;
    0
}
unsafe extern "system" fn lp_addref(this: *mut LazyPso) -> u32 {
    (*this).refcount.fetch_add(1, Ordering::Relaxed) + 1
}
unsafe extern "system" fn lp_release(this: *mut LazyPso) -> u32 {
    (*this).refcount.fetch_sub(1, Ordering::Relaxed).saturating_sub(1)
}
unsafe extern "system" fn lp_stub(_this: *mut LazyPso) -> i32 { 0 }
unsafe extern "system" fn lp_get_device(this: *mut LazyPso, _riid: *const c_void, out: *mut *mut c_void) -> i32 {
    if out.is_null() { return 0x8000_4003u32 as i32; }
    let dev = (*this).desc.device_inner as *mut c_void;
    if !dev.is_null() {
        let addref: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(vt_slot(dev, 1));
        addref(dev);
    }
    *out = dev;
    0
}

fn lazy_vtbl() -> usize {
    *LAZY_VTBL.get_or_init(|| {
        let mut v = [lp_stub as usize; 16];
        v[0] = lp_qi as usize;
        v[1] = lp_addref as usize;
        v[2] = lp_release as usize;
        v[7] = lp_get_device as usize;
        Box::leak(Box::new(v)).as_ptr() as usize
    })
}

pub(crate) unsafe fn is_lazy_pso(p: *mut c_void) -> bool {
    !p.is_null() && (p as usize) > 0x10000 && *(p as *const usize) == lazy_vtbl()
}

pub(crate) unsafe fn as_lazy_pso<'a>(p: *mut c_void) -> Option<&'a LazyPso> {
    if is_lazy_pso(p) { Some(&*(p as *const LazyPso)) } else { None }
}

fn hand_out_lazy(desc: GfxPsoDesc) -> *mut c_void {
    let obj = Box::new(LazyPso {
        vtbl: lazy_vtbl(),
        refcount: AtomicU32::new(1),
        pad0: 0,
        pkt: [0; 7],
        pad1: 0,
        desc,
        logged: std::sync::atomic::AtomicBool::new(false),
        compiled: OnceLock::new(),
    });
    Box::into_raw(obj) as *mut c_void
}

fn subobject_size(kind: u32) -> Option<usize> {
    match kind {
        0 => Some(16),
        1..=6 | 23 | 24 => Some(24),
        7 | 15 => Some(40),
        8 => Some(336),
        9 | 13 | 14 | 16 | 18 | 20 => Some(8),
        10 => Some(48),
        11 => Some(56),
        12 | 19 => Some(24),
        17 => Some(16),
        21 => Some(64),
        22 => Some(32),
        _ => None,
    }
}

static STREAM_CNT: AtomicU32 = AtomicU32::new(0);

pub(crate) unsafe fn create_from_stream(device_inner: *mut c_void, desc: *const u8, out: *mut *mut c_void) -> i32 {
    if desc.is_null() || out.is_null() {
        return 0x8007_0057u32 as i32;
    }
    let size = *(desc as *const usize);
    let stream = *(desc.add(8) as *const *const u8);
    if stream.is_null() || size < 8 {
        return 0x8007_0057u32 as i32;
    }

    let mut d = GfxPsoDesc { device_inner: device_inner as usize, ..GfxPsoDesc::default() };
    let mut off = 0usize;
    while off + 4 <= size {
        let at = stream.add(off);
        let kind = std::ptr::read_unaligned(at as *const u32);
        let Some(step) = subobject_size(kind) else { break };
        if off + step > size { break; }
        let data = at.add(4);
        match kind {
            0 => d.root_sig = std::ptr::read_unaligned(at.add(8) as *const usize),
            1 => d.vs = intern_shader("vs", std::ptr::read_unaligned(at.add(8) as *const *const u8), std::ptr::read_unaligned(at.add(16) as *const usize)),
            2 => d.ps = intern_shader("ps", std::ptr::read_unaligned(at.add(8) as *const *const u8), std::ptr::read_unaligned(at.add(16) as *const usize)),
            3..=5 => {
                let p = std::ptr::read_unaligned(at.add(8) as *const *const u8);
                let l = std::ptr::read_unaligned(at.add(16) as *const usize);
                if !p.is_null() && l > 0 {
                    d.has_other_stage = true;
                }
            }
            6 => d.cs = intern_shader("cs", std::ptr::read_unaligned(at.add(8) as *const *const u8), std::ptr::read_unaligned(at.add(16) as *const usize)),
            23 => d.as_ = intern_shader("as", std::ptr::read_unaligned(at.add(8) as *const *const u8), std::ptr::read_unaligned(at.add(16) as *const usize)),
            24 => d.ms = intern_shader("ms", std::ptr::read_unaligned(at.add(8) as *const *const u8), std::ptr::read_unaligned(at.add(16) as *const usize)),
            8 => std::ptr::copy_nonoverlapping(data, d.blend.as_mut_ptr(), 328),
            9 => d.sample_mask = std::ptr::read_unaligned(data as *const u32),
            10 => std::ptr::copy_nonoverlapping(data, d.rasterizer.as_mut_ptr(), 44),
            11 | 21 => std::ptr::copy_nonoverlapping(data, d.depth_stencil.as_mut_ptr(), 52),
            12 => {
                let elems = std::ptr::read_unaligned(at.add(8) as *const *const InputElemRaw);
                let cnt = std::ptr::read_unaligned(at.add(16) as *const u32) as usize;
                if !elems.is_null() && cnt <= 64 {
                    d.input_names.clear();
                    d.input_elems.clear();
                    for i in 0..cnt {
                        let e = std::ptr::read_unaligned(elems.add(i));
                        let name = if e.semantic_name.is_null() {
                            CString::new("TEXCOORD").unwrap()
                        } else {
                            CStr::from_ptr(e.semantic_name as *const i8).to_owned()
                        };
                        d.input_names.push(name);
                        d.input_elems.push(InputElemRaw {
                            semantic_name: std::ptr::null(),
                            format: map_console_format(e.format),
                            ..e
                        });
                    }
                    for (e, name) in d.input_elems.iter_mut().zip(&d.input_names) {
                        e.semantic_name = name.as_ptr() as *const u8;
                    }
                }
            }
            13 => d.strip_cut = std::ptr::read_unaligned(data as *const u32),
            14 => d.topology_type = std::ptr::read_unaligned(data as *const u32),
            15 => {
                for i in 0..8 {
                    let f = std::ptr::read_unaligned(data.add(i * 4) as *const u32);
                    d.rt_formats[i] = map_console_format(f);
                }
                d.num_rt = std::ptr::read_unaligned(data.add(32) as *const u32).min(8);
            }
            16 => {
                let f = std::ptr::read_unaligned(data as *const u32);
                d.dsv_format = depth_to_dsv(map_console_format(f));
            }
            17 => {
                d.sample_count = std::ptr::read_unaligned(data as *const u32).max(1);
                d.sample_quality = std::ptr::read_unaligned(data.add(4) as *const u32);
            }
            _ => {}
        }
        off += step;
    }

    let n = STREAM_CNT.fetch_add(1, Ordering::Relaxed) + 1;
    if n <= 4 || n.is_power_of_two() {
        log_d3d12(&format!("[gfxpso] stream #{n}: vs={:?} ps={:?} rts={} topo={} inputs={}",
            d.vs.as_ref().map(|s| format!("{:016x}", s.hash)),
            d.ps.as_ref().map(|s| format!("{:016x}", s.hash)),
            d.num_rt, d.topology_type, d.input_elems.len()));
    }

    *out = hand_out_lazy(d);
    0
}

// ---- Per-list draw state & live draw execution --------------------------------------------

#[derive(Clone, Debug, Default)]
pub(crate) enum RootArg {
    #[default]
    None,
    Table(u64),
    Constants(Vec<u32>),
    View(u64),
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub(crate) struct VbView {
    pub addr: u64,
    pub size: u32,
    pub stride: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub(crate) struct IbView {
    pub addr: u64,
    pub size: u32,
    pub format: u32,
}

#[derive(Default)]
pub(crate) struct DrawState {
    pub pso: usize,
    pub gfx_rs: usize,
    pub cs_rs: usize,
    pub heaps: [usize; 2], // [0]=CBV_SRV_UAV, [1]=SAMPLER
    pub gfx_roots: Vec<RootArg>,
    pub cs_roots: Vec<RootArg>,
    pub vbs: [VbView; 32],
    pub ib: IbView,
    pub topology: u32,
    pub num_rtvs: u32,
    pub rtvs: [usize; 8],
    pub dsv: usize,
}

static LIST_STATE: Mutex<Option<HashMap<usize, DrawState>>> = Mutex::new(None);
static DRAWN_OK: AtomicU64 = AtomicU64::new(0);
static MESH_CNT: AtomicU64 = AtomicU64::new(0);
static DISP_CNT: AtomicU64 = AtomicU64::new(0);

fn with_state<R>(obj: *mut c_void, f: impl FnOnce(&mut DrawState) -> R) -> R {
    let mut map = LIST_STATE.lock().unwrap();
    let st = map.get_or_insert_with(HashMap::new).entry(obj as usize).or_default();
    f(st)
}

pub(crate) fn reset(obj: *mut c_void, pso: *mut c_void) {
    with_state(obj, |st| {
        *st = DrawState::default();
        st.pso = pso as usize;
    });
}
pub(crate) fn set_pso(obj: *mut c_void, pso: *mut c_void) {
    with_state(obj, |st| st.pso = pso as usize);
}
pub(crate) fn set_rs(obj: *mut c_void, rs: *mut c_void, cs: bool) {
    with_state(obj, |st| if cs { st.cs_rs = rs as usize; } else { st.gfx_rs = rs as usize; });
}
pub(crate) unsafe fn set_heaps(obj: *mut c_void, n: u32, heaps: *const *mut c_void) {
    with_state(obj, |st| {
        st.heaps = [0; 2];
        for i in 0..n as usize {
            let h = *heaps.add(i);
            if let Some(info) = heap_info(h as usize) {
                if info.ty <= 1 {
                    st.heaps[info.ty as usize] = h as usize;
                }
            }
        }
    });
}
pub(crate) fn set_topology(obj: *mut c_void, topo: u32) {
    with_state(obj, |st| st.topology = topo);
}
pub(crate) fn set_vbs(obj: *mut c_void, start: u32, views: &[VbView]) {
    with_state(obj, |st| {
        for (i, v) in views.iter().enumerate() {
            if let Some(slot) = st.vbs.get_mut(start as usize + i) { *slot = *v; }
        }
    });
}
pub(crate) fn set_ib(obj: *mut c_void, view: Option<IbView>) {
    with_state(obj, |st| st.ib = view.unwrap_or_default());
}
pub(crate) fn set_root_table(obj: *mut c_void, cs: bool, idx: u32, handle: u64) {
    with_state(obj, |st| {
        let v = if cs { &mut st.cs_roots } else { &mut st.gfx_roots };
        if v.len() <= idx as usize { v.resize(idx as usize + 1, RootArg::None); }
        v[idx as usize] = RootArg::Table(handle);
    });
}
pub(crate) fn set_root_view(obj: *mut c_void, cs: bool, idx: u32, addr: u64) {
    with_state(obj, |st| {
        let v = if cs { &mut st.cs_roots } else { &mut st.gfx_roots };
        if v.len() <= idx as usize { v.resize(idx as usize + 1, RootArg::None); }
        v[idx as usize] = RootArg::View(addr);
    });
}
pub(crate) fn set_root_consts(obj: *mut c_void, cs: bool, idx: u32, vals: &[u32], off: u32) {
    with_state(obj, |st| {
        let v = if cs { &mut st.cs_roots } else { &mut st.gfx_roots };
        if v.len() <= idx as usize { v.resize(idx as usize + 1, RootArg::None); }
        if !matches!(v[idx as usize], RootArg::Constants(_)) {
            v[idx as usize] = RootArg::Constants(Vec::new());
        }
        if let RootArg::Constants(c) = &mut v[idx as usize] {
            let end = off as usize + vals.len();
            if c.len() < end { c.resize(end, 0); }
            c[off as usize..end].copy_from_slice(vals);
        }
    });
}
pub(crate) fn set_rts(obj: *mut c_void, rtvs: &[usize], dsv: usize) {
    with_state(obj, |st| {
        st.num_rtvs = rtvs.len().min(8) as u32;
        for (i, &r) in rtvs.iter().take(8).enumerate() { st.rtvs[i] = r; }
        st.dsv = dsv;
    });
}

fn fill_user_data(ud_sources: &[UdSource], roots: &[RootArg], heaps: [usize; 2]) -> Vec<u32> {
    ud_sources.iter().map(|&src| match src {
        UdSource::Zero | UdSource::HeapBase => 0u32,
        UdSource::Constant { root_slot, dword_off } => {
            if let Some(RootArg::Constants(v)) = roots.get(root_slot as usize) {
                v.get(dword_off as usize).copied().unwrap_or(0)
            } else {
                0
            }
        }
        UdSource::Table { root_slot, is_sampler } => {
            let Some(RootArg::Table(h)) = roots.get(root_slot as usize) else { return 0 };
            let hi = heap_info(heaps[usize::from(is_sampler)]);
            if let Some(info) = hi {
                if *h >= info.gpu_start && info.inc > 0 {
                    let idx = ((*h - info.gpu_start) / info.inc as u64) as u32;
                    return idx.wrapping_mul(if is_sampler { 16 } else { 32 });
                }
            }
            0
        }
    }).collect()
}

pub(crate) unsafe fn on_draw(
    obj: *mut c_void,
    inner: *mut c_void,
    indexed: bool,
    count: u32,
    inst: u32,
    start_v_or_i: u32,
    base_v: i32,
    start_inst: u32,
) {
    if count == 0 || inst == 0 {
        return;
    }
    let (pso_ptr, topo, heaps, roots, vbs, ib, rtv0) = with_state(obj, |st| {
        (st.pso as *mut c_void, st.topology, st.heaps, st.gfx_roots.clone(), st.vbs, st.ib, st.rtvs[0])
    });
    let Some(lp) = as_lazy_pso(pso_ptr) else {
        static NL: AtomicU32 = AtomicU32::new(0);
        if NL.fetch_add(1, Ordering::Relaxed) < 4 {
            log_d3d12(&format!("[gfxpso] draw skip: non-lazy pso={pso_ptr:p} topo={topo} cnt={count}"));
        }
        return;
    };
    let Some(cp) = lp.compiled.get_or_init(|| compile_lazy_pso(lp)).as_ref() else { return };
    let dev_inner = lp.desc.device_inner as *mut c_void;

    // 1. Bind host root signature & host PSO.
    let set_rs: unsafe extern "system" fn(*mut c_void, *mut c_void) = std::mem::transmute(vt_slot(inner, 30));
    let set_pso: unsafe extern "system" fn(*mut c_void, *mut c_void) = std::mem::transmute(vt_slot(inner, 25));
    set_rs(inner, cp.host_rs as *mut c_void);
    set_pso(inner, cp.host_pso as *mut c_void);

    // 2. Bind per-stage user data & root SRVs.
    let set_consts: unsafe extern "system" fn(*mut c_void, u32, u32, *const u32, u32) = std::mem::transmute(vt_slot(inner, 36));
    let set_srv: unsafe extern "system" fn(*mut c_void, u32, u64) = std::mem::transmute(vt_slot(inner, 40));

    for sb in [&cp.vs_bind, &cp.ps_bind] {
        if let Some(pidx) = sb.ud_param_idx {
            let ud = fill_user_data(&sb.ud_sources, &roots, heaps);
            if !ud.is_empty() {
                set_consts(inner, pidx, ud.len() as u32, ud.as_ptr(), 0);
            }
        }
        for &(title_slot, host_param) in &sb.root_srvs {
            let gpu = if let Some(RootArg::View(va)) = roots.get(title_slot as usize) {
                resolve_read_va(dev_inner, *va, 4096, 16)
                    .or_else(|| resolve_read_va(dev_inner, *va, 256, 16))
            } else {
                None
            };
            if let Some(g) = gpu {
                set_srv(inner, host_param, g);
            } else if let Some((g, p)) = upload_alloc(dev_inner, 256, 16) {
                std::ptr::write_bytes(p, 0, 256);
                set_srv(inner, host_param, g);
            }
        }
    }

    // 2b. Materialize descriptor-table slots copied via CPU memcpy into the host shader-visible heap.
    for sb in [&cp.vs_bind, &cp.ps_bind] {
        for &(root_slot, rel_off, is_sampler, is_cbv) in &sb.table_reads {
            let Some(RootArg::Table(h)) = roots.get(root_slot as usize) else { continue };
            let heap_obj = heaps[usize::from(is_sampler)];
            let Some(info) = heap_info(heap_obj) else { continue };
            if *h < info.gpu_start || info.inc == 0 { continue; }
            let idx = ((*h - info.gpu_start) / info.inc as u64) as u32;
            let base_off = idx.wrapping_mul(if is_sampler { 16 } else { 32 });
            if let Some(tex) = super::desc_mirror::sync_draw_slot(dev_inner, heap_obj, base_off.wrapping_add(rel_off), is_sampler, is_cbv) {
                if let Some(pt) = super::desc_mirror::placed_tex_info(tex) {
                    upload_placed_tex_if_dirty(dev_inner, inner, tex, &pt);
                }
                static TEX_SEEN: Mutex<Option<Vec<usize>>> = Mutex::new(None);
                let mut seen = TEX_SEEN.lock().unwrap();
                let v = seen.get_or_insert_with(Vec::new);
                if v.len() < 12 && !v.contains(&(tex as usize)) {
                    v.push(tex as usize);
                    if let Some(pt) = super::desc_mirror::placed_tex_info(tex) {
                        let nz = if cpu_readable(pt.cpu_va, 4096) {
                            std::slice::from_raw_parts(pt.cpu_va as *const u8, 4096).iter().filter(|&&b| b != 0).count()
                        } else { 0 };
                        log_d3d12(&format!(
                            "[gfxpso] SAMPLED PLACED TEX {tex:p} va={:#x} {}x{} fmt={} clayout={:#x} fl={:#x} cpu_nz4k={nz}",
                            pt.cpu_va, pt.width, pt.height, pt.fmt, pt.console_layout, pt.flags
                        ));
                    } else {
                        log_d3d12(&format!("[gfxpso] SAMPLED NON-PLACED TEX {tex:p}"));
                    }
                }
            }
        }
    }

    // 3. Bind index buffer (if indexed) first so we know max vertex index for tight VB upload!
    let mut draw_count = count;
    let mut draw_start = start_v_or_i;
    let mut max_vtx_needed: Option<usize> = if !indexed {
        Some(start_v_or_i as usize + count as usize)
    } else {
        None
    };
    if indexed {
        if ib.addr == 0 || ib.size == 0 { return; }
        let idx_stride = if ib.format == 42 { 4usize } else { 2usize };
        let byte_off = (start_v_or_i as usize).saturating_mul(idx_stride);
        let slice_bytes = (count as usize).saturating_mul(idx_stride);
        let ib_cpu = fallback_gpu_to_cpu(ib.addr).unwrap_or(ib.addr);
        if slice_bytes > 0 && byte_off + slice_bytes <= ib.size as usize && cpu_readable(ib_cpu + byte_off as u64, slice_bytes) {
            // Scan max index in the exact drawn slice so VB upload is tight (1 KiB vs 1.2 MiB!).
            let p = (ib_cpu + byte_off as u64) as *const u8;
            let mut mx = 0usize;
            if idx_stride == 2 {
                let sl = std::slice::from_raw_parts(p as *const u16, count as usize);
                for &idx in sl { if idx != 0xFFFF { mx = mx.max(idx as usize); } }
            } else {
                let sl = std::slice::from_raw_parts(p as *const u32, count as usize);
                for &idx in sl { if idx != 0xFFFF_FFFF { mx = mx.max(idx as usize); } }
            }
            max_vtx_needed = Some((base_v.max(0) as usize).saturating_add(mx).saturating_add(1));
            let Some(gpu) = resolve_read_va(dev_inner, ib.addr + byte_off as u64, slice_bytes, 4) else { return };
            let pc_ib = IbView { addr: gpu, size: slice_bytes as u32, format: ib.format };
            let set_ib_fn: unsafe extern "system" fn(*mut c_void, *const IbView) = std::mem::transmute(vt_slot(inner, 43));
            set_ib_fn(inner, &pc_ib);
            draw_start = 0;
        } else {
            let Some(gpu) = resolve_read_va(dev_inner, ib.addr, ib.size as usize, 4) else { return };
            let pc_ib = IbView { addr: gpu, size: ib.size, format: ib.format };
            let set_ib_fn: unsafe extern "system" fn(*mut c_void, *const IbView) = std::mem::transmute(vt_slot(inner, 43));
            set_ib_fn(inner, &pc_ib);
        }
    }

    // 4. Bind vertex buffers (with rect-list expansion if topo == 17).
    let set_vb: unsafe extern "system" fn(*mut c_void, u32, u32, *const VbView) = std::mem::transmute(vt_slot(inner, 44));

    if !indexed && topo == TOPO_RECTLIST && count >= 3 && vbs[0].addr != 0 && vbs[0].stride > 0 {
        let rects = (count / 3) as usize;
        let stride = vbs[0].stride as usize;
        let src_off = start_v_or_i as usize * stride;
        let src_bytes = rects * 3 * stride;
        let dst_bytes = rects * 6 * stride;
        let src_va = vbs[0].addr.saturating_add(src_off as u64);
        if cpu_readable(src_va, src_bytes) {
            if let Some((gpu, dst_ptr)) = upload_alloc(dev_inner, dst_bytes, 16) {
                let src_slice = std::slice::from_raw_parts(src_va as *const u8, src_bytes);
                let dst_slice = std::slice::from_raw_parts_mut(dst_ptr, dst_bytes);
                expand_rects(src_slice, dst_slice, stride, &cp.rect_elems);
                let expanded = VbView { addr: gpu, size: dst_bytes as u32, stride: vbs[0].stride };
                set_vb(inner, 0, 1, &expanded);
                draw_count = (rects * 6) as u32;
                draw_start = 0;
            } else {
                return;
            }
        } else {
            return;
        }
    } else if !lp.desc.input_elems.is_empty() {
        let mut slot_mask = 0u32;
        let mut inst_mask = 0u32;
        for e in &lp.desc.input_elems {
            if e.input_slot < 32 {
                slot_mask |= 1 << e.input_slot;
                if e.input_slot_class != 0 { inst_mask |= 1 << e.input_slot; }
            }
        }
        for s in 0..32u32 {
            if slot_mask & (1 << s) == 0 { continue; }
            let v = vbs[s as usize];
            if v.addr == 0 || v.size == 0 { continue; }
            let upload_sz = if inst_mask & (1 << s) == 0 && v.stride > 0 {
                if let Some(nv) = max_vtx_needed {
                    nv.saturating_mul(v.stride as usize).min(v.size as usize).max(v.stride as usize)
                } else {
                    v.size as usize
                }
            } else {
                v.size as usize
            };
            if let Some(gpu) = resolve_read_va(dev_inner, v.addr, upload_sz, 16) {
                let pc_v = VbView { addr: gpu, size: upload_sz as u32, stride: v.stride };
                set_vb(inner, s, 1, &pc_v);
            } else {
                return;
            }
        }
    }

    // 5. Set topology and issue real host draw!
    let host_topo = match topo { 0 | 17 | 18 | 19 | 20 | 32 => 4, t => t };
    let set_topo: unsafe extern "system" fn(*mut c_void, u32) = std::mem::transmute(vt_slot(inner, 20));
    set_topo(inner, host_topo);

    if indexed {
        let draw_fn: unsafe extern "system" fn(*mut c_void, u32, u32, u32, i32, u32) = std::mem::transmute(vt_slot(inner, 13));
        draw_fn(inner, draw_count, inst, draw_start, base_v, start_inst);
    } else {
        let draw_fn: unsafe extern "system" fn(*mut c_void, u32, u32, u32, u32) = std::mem::transmute(vt_slot(inner, 12));
        draw_fn(inner, draw_count, inst, draw_start, start_inst);
    }

    let ok = DRAWN_OK.fetch_add(1, Ordering::Relaxed) + 1;
    let vs_hash = lp.desc.vs.as_ref().map(|s| s.hash).unwrap_or(0);
    let first_for_vs = {
        static VS_SEEN: Mutex<Option<Vec<u64>>> = Mutex::new(None);
        let mut g = VS_SEEN.lock().unwrap();
        let v = g.get_or_insert_with(Vec::new);
        if v.len() < 16 && !v.contains(&vs_hash) { v.push(vs_hash); true } else { false }
    };
    if first_for_vs {
        let ud_vs = fill_user_data(&cp.vs_bind.ud_sources, &roots, heaps);
        let ud_ps = fill_user_data(&cp.ps_bind.ud_sources, &roots, heaps);
        let elems_dbg: Vec<(u32, u32, u32)> = lp.desc.input_elems.iter().map(|e| (e.format, e.input_slot, e.aligned_byte_offset)).collect();
        let cull = u32::from_le_bytes(lp.desc.rasterizer[4..8].try_into().unwrap());
        let ccw = u32::from_le_bytes(lp.desc.rasterizer[8..12].try_into().unwrap());
        let depth_en = u32::from_le_bytes(lp.desc.depth_stencil[0..4].try_into().unwrap());
        let stencil_en = u32::from_le_bytes(lp.desc.depth_stencil[12..16].try_into().unwrap());
        let blend_en = u32::from_le_bytes(lp.desc.blend[8..12].try_into().unwrap());
        let b_src = u32::from_le_bytes(lp.desc.blend[16..20].try_into().unwrap());
        let b_dst = u32::from_le_bytes(lp.desc.blend[20..24].try_into().unwrap());
        let b_op = u32::from_le_bytes(lp.desc.blend[24..28].try_into().unwrap());
        let rt_mask = lp.desc.blend[44];
        let rtv0_res = super::desc_mirror::rtv_res(rtv0);
        let cbv0_dbg = if ud_vs.len() > 8 && heaps[0] != 0 {
            super::desc_mirror::debug_slot(heaps[0], ud_vs[8])
        } else {
            String::new()
        };
        let vb0_hex = if vbs[0].addr != 0 && cpu_readable(vbs[0].addr, 32) {
            let p = vbs[0].addr as *const u32;
            format!("{:08x?}", std::slice::from_raw_parts(p, 8))
        } else {
            format!("unreadable(fb={:?})", fallback_gpu_to_cpu(vbs[0].addr))
        };
        let ib_hex = if indexed && ib.addr != 0 && cpu_readable(ib.addr, 16) {
            let p = ib.addr as *const u16;
            format!("{:?}", std::slice::from_raw_parts(p, 8))
        } else {
            String::new()
        };
        log_d3d12(&format!(
            "[gfxpso] FIRST DRAW vs={vs_hash:016x} ({}, topo={topo}->{host_topo}, cnt={draw_count}x{inst}, start={draw_start}, basev={base_v}) rtv0={rtv0:#x}(res={rtv0_res:#x}) ud_vs={ud_vs:?} cbv0={cbv0_dbg} vb0=({:#x},sz={},st={}) {vb0_hex} ib=({:#x},sz={},fmt={}) {ib_hex} elems={elems_dbg:?} cull={cull} ccw={ccw} de={depth_en} se={stencil_en} be={blend_en}({b_src},{b_dst},{b_op}) wmask={rt_mask:#x}",
            if indexed { "idx" } else { "non-idx" },
            vbs[0].addr, vbs[0].size, vbs[0].stride, ib.addr, ib.size, ib.format
        ));
    }
    if ok <= 10 || ok.is_power_of_two() {
        let ud_vs = fill_user_data(&cp.vs_bind.ud_sources, &roots, heaps);
        let ud_ps = fill_user_data(&cp.ps_bind.ud_sources, &roots, heaps);
        let vb0_preview = if ok <= 10 && vbs[0].addr != 0 && vbs[0].stride == 24 && cpu_readable(vbs[0].addr, 72) {
            let p = vbs[0].addr as *const u8;
            let mut s = String::new();
            for vi in 0..3 {
                let q = p.add(vi * 24);
                let x = std::ptr::read_unaligned(q as *const f32);
                let y = std::ptr::read_unaligned(q.add(4) as *const f32);
                let z = std::ptr::read_unaligned(q.add(8) as *const f32);
                let col = std::ptr::read_unaligned(q.add(12) as *const u32);
                let u = std::ptr::read_unaligned(q.add(16) as *const f32);
                let v = std::ptr::read_unaligned(q.add(20) as *const f32);
                s.push_str(&format!(" v{vi}=({x:.2},{y:.2},{z:.2},c={col:08x},uv={u:.2},{v:.2})"));
            }
            s
        } else {
            String::new()
        };
        log_d3d12(&format!(
            "[gfxpso] DRAWN OK #{ok} ({}, topo={topo}->{host_topo}, count={draw_count}x{inst}, vs={:016x}, h0={:#x}, hi0={:?}, roots={:?}, ud_vs={:?}, ud_ps={:?}{vb0_preview})",
            if indexed { "idx" } else { "non-idx" }, lp.desc.vs.as_ref().map(|s| s.hash).unwrap_or(0),
            heaps[0], heap_info(heaps[0]).map(|i| (i.gpu_start, i.inc)),
            roots.iter().take(4).collect::<Vec<_>>(),
            ud_vs.last(), ud_ps.last()
        ));
    }
}

pub(crate) unsafe fn on_dispatch_mesh(obj: *mut c_void, x: u32, y: u32, z: u32) {
    let n = MESH_CNT.fetch_add(1, Ordering::Relaxed) + 1;
    let pso = with_state(obj, |st| st.pso);
    if n <= 4 || n.is_power_of_two() {
        log_d3d12(&format!("[gfxpso] DispatchMesh #{n} ({x},{y},{z}) pso={pso:#x}"));
    }
}

pub(crate) unsafe fn is_real_pc_compute_pso(obj: *mut c_void) -> Option<*mut c_void> {
    let pso = with_state(obj, |st| st.pso) as *mut c_void;
    if pso.is_null() || is_lazy_pso(pso) {
        return None;
    }
    let unwrapped = super::replay::unwrap_pso(pso);
    if unwrapped as usize == super::replay::unwrap_pso(super::pso_fallback_raw() as *mut c_void) as usize {
        return None;
    }
    Some(unwrapped)
}

pub(crate) fn note_dispatch() -> u64 {
    DISP_CNT.fetch_add(1, Ordering::Relaxed) + 1
}

pub(crate) fn drawn_ok_count() -> u64 {
    DRAWN_OK.load(Ordering::Relaxed)
}
