pub mod compute_exec;
pub mod rdna2_hlsl;

use std::ffi::{c_void, CString};
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

const S_OK: i32 = 0;

const E_FAIL: i32 = 0x8000_4005u32 as i32;
#[allow(dead_code)]
const E_NOTIMPL: i32 = 0x8000_4001u32 as i32;
static LOG_LOCK: Mutex<()> = Mutex::new(());

fn log_d3d12(msg: &str) {
    let Ok(path) = std::env::var("PERCY_D3D12_LOG") else {
        return;
    };
    if path.is_empty() {
        return;
    }
    let _g = LOG_LOCK.lock();
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{msg}");
    }
}

fn fnv1a_64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

pub fn cache_dir() -> std::path::PathBuf {
    std::env::var("PERCY_SHADER_CACHE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("percy_shader_cache"))
}

pub fn cache_load_blob(tag: &str, key: &[u8]) -> Option<Vec<u8>> {
    let p = cache_dir().join(format!("{tag}_{:016x}.bin", fnv1a_64(key)));
    std::fs::read(p).ok()
}

pub fn cache_store_blob(tag: &str, key: &[u8], blob: &[u8]) {
    let dir = cache_dir();
    if std::fs::create_dir_all(&dir).is_ok() {
        let p = dir.join(format!("{tag}_{:016x}.bin", fnv1a_64(key)));
        let _ = std::fs::write(p, blob);
    }
}

/// Translates an RDNA2 blob to HLSL (with disk cache) and returns the HLSL bytes.
pub fn translate_and_cache_rdna2(
    bytes: &[u8],
    hint: rdna2_hlsl::ShaderStage,
) -> (rdna2_hlsl::ShaderStage, String) {
    if let Some(cached) = cache_load_blob(hint.as_str(), bytes) {
        if let Ok(s) = String::from_utf8(cached) {
            return (hint, s);
        }
    }
    let (stage, hlsl, _) = rdna2_hlsl::lift_to_hlsl(bytes, hint);
    cache_store_blob(hint.as_str(), bytes, hlsl.as_bytes());
    (stage, hlsl)
}

/// Compiles lifted HLSL into Windows DXBC bytecode via system `d3dcompiler_47.dll` (`D3DCompile`),
/// caching compiled DXBC blobs on disk under `dxbc_<hash>.bin`.
#[cfg(windows)]
pub fn compile_hlsl_to_dxbc(hlsl: &str, stage: rdna2_hlsl::ShaderStage) -> Result<Vec<u8>, String> {
    let cache_key = format!("{}:{hlsl}", stage.dxbc_target());
    if let Some(cached) = cache_load_blob("dxbc", cache_key.as_bytes()) {
        return Ok(cached);
    }

    #[repr(C)]
    struct BlobVtbl {
        _qi: usize,
        _add: usize,
        release: unsafe extern "system" fn(*mut c_void) -> u32,
        get_ptr: unsafe extern "system" fn(*mut c_void) -> *const u8,
        get_size: unsafe extern "system" fn(*mut c_void) -> usize,
    }
    type D3DCompileFn = unsafe extern "system" fn(
        *const u8,
        usize,
        *const i8,
        *const c_void,
        *const c_void,
        *const i8,
        *const i8,
        u32,
        u32,
        *mut *mut c_void,
        *mut *mut c_void,
    ) -> i32;

    unsafe {
        let dll = CString::new("d3dcompiler_47.dll").unwrap();
        let mod_h = LoadLibraryA(dll.as_ptr());
        if mod_h == 0 {
            return Err("d3dcompiler_47.dll not found".into());
        }
        let sym = CString::new("D3DCompile").unwrap();
        let addr = GetProcAddress(mod_h, sym.as_ptr());
        if addr == 0 {
            return Err("D3DCompile export missing".into());
        }
        let compile: D3DCompileFn = std::mem::transmute(addr);
        let entry = CString::new("main").unwrap();
        let target = CString::new(stage.dxbc_target()).unwrap();
        let mut code_blob: *mut c_void = std::ptr::null_mut();
        let mut err_blob: *mut c_void = std::ptr::null_mut();
        let hr = compile(
            hlsl.as_ptr(),
            hlsl.len(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            entry.as_ptr(),
            target.as_ptr(),
            1 << 11, // D3DCOMPILE_OPTIMIZATION_LEVEL1
            0,
            &mut code_blob,
            &mut err_blob,
        );
        if hr < 0 || code_blob.is_null() {
            let mut msg = format!("D3DCompile failed: 0x{hr:08x}");
            if !err_blob.is_null() {
                let vtbl = &*(*(err_blob as *const *const BlobVtbl));
                let ptr = (vtbl.get_ptr)(err_blob);
                let sz = (vtbl.get_size)(err_blob);
                if !ptr.is_null() && sz > 0 {
                    msg = String::from_utf8_lossy(std::slice::from_raw_parts(ptr, sz)).into_owned();
                }
                (vtbl.release)(err_blob);
            }
            return Err(msg);
        }
        if !err_blob.is_null() {
            let vtbl = &*(*(err_blob as *const *const BlobVtbl));
            (vtbl.release)(err_blob);
        }
        let vtbl = &*(*(code_blob as *const *const BlobVtbl));
        let ptr = (vtbl.get_ptr)(code_blob);
        let sz = (vtbl.get_size)(code_blob);
        let bytes = std::slice::from_raw_parts(ptr, sz).to_vec();
        (vtbl.release)(code_blob);
        cache_store_blob("dxbc", cache_key.as_bytes(), &bytes);
        Ok(bytes)
    }
}

#[cfg(windows)]
fn load_dxcompiler() -> Option<usize> {
    static MOD: OnceLock<usize> = OnceLock::new();
    let h = *MOD.get_or_init(|| unsafe {
        let mut candidates: Vec<std::path::PathBuf> = Vec::new();
        if let Ok(p) = std::env::var("PERCY_DXC_DLL") {
            if !p.is_empty() {
                candidates.push(std::path::PathBuf::from(p));
            }
        }
        if let Ok(exe) = std::env::current_exe() {
            let mut cur = exe.parent();
            for _ in 0..4 {
                let Some(dir) = cur else { break };
                candidates.push(dir.join("dxcompiler.dll"));
                candidates.push(dir.join("tools").join("dxc").join("bin").join("x64").join("dxcompiler.dll"));
                cur = dir.parent();
            }
        }
        candidates.push(std::path::PathBuf::from(r"tools\dxc\bin\x64\dxcompiler.dll"));
        candidates.push(std::path::PathBuf::from(r"..\..\tools\dxc\bin\x64\dxcompiler.dll"));
        candidates.push(std::path::PathBuf::from("dxcompiler.dll"));
        for c in candidates {
            if let Ok(cs) = CString::new(c.to_string_lossy().as_bytes()) {
                let m = LoadLibraryA(cs.as_ptr());
                if m != 0 {
                    return m;
                }
            }
        }
        0
    });
    if h != 0 { Some(h) } else { None }
}

/// Compiles lifted HLSL into SM 6.6 DXIL bytecode via `dxcompiler.dll` (`IDxcCompiler3::Compile`),
/// supporting Vertex, Pixel, Compute, Mesh (`ms_6_6`), Geometry, and RayTracing (`lib_6_6`).
#[cfg(windows)]
pub fn compile_hlsl_to_dxil(hlsl: &str, stage: rdna2_hlsl::ShaderStage) -> Result<Vec<u8>, String> {
    let target_str = stage.dxil_target();
    let cache_key = format!("{target_str}:{hlsl}");
    if let Some(cached) = cache_load_blob("dxil", cache_key.as_bytes()) {
        return Ok(cached);
    }

    #[repr(C)]
    struct DxcBuffer {
        ptr: *const c_void,
        size: usize,
        encoding: u32,
    }
    #[repr(C)]
    struct DxcBlobVtbl {
        _qi: usize,
        _add: usize,
        release: unsafe extern "system" fn(*mut c_void) -> u32,
        get_ptr: unsafe extern "system" fn(*mut c_void) -> *const u8,
        get_size: unsafe extern "system" fn(*mut c_void) -> usize,
    }
    #[repr(C)]
    struct DxcResultVtbl {
        _qi: usize,
        _add: usize,
        release: unsafe extern "system" fn(*mut c_void) -> u32,
        get_status: unsafe extern "system" fn(*mut c_void, *mut i32) -> i32,
        get_result: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32,
        get_error_buffer: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32,
    }
    #[repr(C)]
    struct DxcCompiler3Vtbl {
        _qi: usize,
        _add: usize,
        release: unsafe extern "system" fn(*mut c_void) -> u32,
        compile: unsafe extern "system" fn(
            *mut c_void,
            *const DxcBuffer,
            *const *const u16,
            u32,
            *mut c_void,
            *const [u8; 16],
            *mut *mut c_void,
        ) -> i32,
    }
    type DxcCreateInstanceFn = unsafe extern "system" fn(
        *const [u8; 16],
        *const [u8; 16],
        *mut *mut c_void,
    ) -> i32;

    const CLSID_DXC_COMPILER: [u8; 16] = [
        0x93, 0x2d, 0xe2, 0x73, 0xce, 0xe6, 0xf3, 0x47, 0xb5, 0xbf, 0xf0, 0x66, 0x4f, 0x39, 0xc1, 0xb0,
    ];
    const IID_IDXC_COMPILER3: [u8; 16] = [
        0x87, 0x46, 0x8b, 0x22, 0x6a, 0x5a, 0x30, 0x47, 0x90, 0x0c, 0x97, 0x02, 0xb2, 0x20, 0x3f, 0x54,
    ];
    const IID_IDXC_RESULT: [u8; 16] = [
        0xda, 0x6c, 0x34, 0x58, 0xe7, 0xdd, 0x97, 0x44, 0x94, 0x61, 0x6f, 0x87, 0xaf, 0x5e, 0x06, 0x59,
    ];

    let Some(mod_h) = load_dxcompiler() else {
        return Err("dxcompiler.dll not found".into());
    };
    unsafe {
        let sym = CString::new("DxcCreateInstance").unwrap();
        let addr = GetProcAddress(mod_h, sym.as_ptr());
        if addr == 0 {
            return Err("DxcCreateInstance export missing".into());
        }
        let create_inst: DxcCreateInstanceFn = std::mem::transmute(addr);
        std::thread_local! {
            static TL_COMPILER: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
        }
        let mut compiler = TL_COMPILER.with(|c| c.get()) as *mut c_void;
        if compiler.is_null() {
            let hr = create_inst(&CLSID_DXC_COMPILER, &IID_IDXC_COMPILER3, &mut compiler);
            if hr < 0 || compiler.is_null() {
                return Err(format!("DxcCreateInstance failed: 0x{hr:08x}"));
            }
            TL_COMPILER.with(|c| c.set(compiler as usize));
        }

        let to_wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() };
        let w_e = to_wide("-E");
        let w_main = to_wide("main");
        let w_t = to_wide("-T");
        let w_target = to_wide(target_str);
        let w_o1 = to_wide("-Od");
        let mut args = vec![w_e.as_ptr(), w_main.as_ptr(), w_t.as_ptr(), w_target.as_ptr(), w_o1.as_ptr()];
        if stage == rdna2_hlsl::ShaderStage::RayTracing {
            // lib_6_6 does not take -E entry point
            args = vec![w_t.as_ptr(), w_target.as_ptr(), w_o1.as_ptr()];
        }

        let src_buf = DxcBuffer {
            ptr: hlsl.as_ptr() as *const c_void,
            size: hlsl.len(),
            encoding: 65001, // DXC_CP_UTF8
        };
        let comp_vtbl = &*(*(compiler as *const *const DxcCompiler3Vtbl));
        let mut res_ptr: *mut c_void = std::ptr::null_mut();
        let hr_c = (comp_vtbl.compile)(
            compiler,
            &src_buf,
            args.as_ptr(),
            args.len() as u32,
            std::ptr::null_mut(),
            &IID_IDXC_RESULT,
            &mut res_ptr,
        );
        if hr_c < 0 || res_ptr.is_null() {
            return Err(format!("IDxcCompiler3::Compile failed: 0x{hr_c:08x}"));
        }

        let res_vtbl = &*(*(res_ptr as *const *const DxcResultVtbl));
        let mut status: i32 = -1;
        let _ = (res_vtbl.get_status)(res_ptr, &mut status);
        if status < 0 {
            let mut err_blob: *mut c_void = std::ptr::null_mut();
            let mut msg = format!("DXC compile status: 0x{status:08x}");
            if (res_vtbl.get_error_buffer)(res_ptr, &mut err_blob) >= 0 && !err_blob.is_null() {
                let bvtbl = &*(*(err_blob as *const *const DxcBlobVtbl));
                let ptr = (bvtbl.get_ptr)(err_blob);
                let sz = (bvtbl.get_size)(err_blob);
                if !ptr.is_null() && sz > 0 {
                    msg = String::from_utf8_lossy(std::slice::from_raw_parts(ptr, sz)).into_owned();
                }
                (bvtbl.release)(err_blob);
            }
            (res_vtbl.release)(res_ptr);
            return Err(msg);
        }

        let mut obj_blob: *mut c_void = std::ptr::null_mut();
        let hr_r = (res_vtbl.get_result)(res_ptr, &mut obj_blob);
        if hr_r < 0 || obj_blob.is_null() {
            (res_vtbl.release)(res_ptr);
            return Err(format!("IDxcResult::GetResult failed: 0x{hr_r:08x}"));
        }
        let bvtbl = &*(*(obj_blob as *const *const DxcBlobVtbl));
        let ptr = (bvtbl.get_ptr)(obj_blob);
        let sz = (bvtbl.get_size)(obj_blob);
        let bytes = std::slice::from_raw_parts(ptr, sz).to_vec();
        (bvtbl.release)(obj_blob);
        (res_vtbl.release)(res_ptr);
        cache_store_blob("dxil", cache_key.as_bytes(), &bytes);
        Ok(bytes)
    }
}

#[cfg(windows)]
extern "system" {
    fn LoadLibraryA(name: *const i8) -> usize;
    fn GetProcAddress(module: usize, name: *const i8) -> usize;
}

#[cfg(windows)]
fn host_d3d12() -> usize {
    static MOD: OnceLock<usize> = OnceLock::new();
    *MOD.get_or_init(|| {
        let name = CString::new("d3d12.dll").unwrap();
        unsafe { LoadLibraryA(name.as_ptr()) }
    })
}

#[cfg(windows)]
unsafe fn host_proc(sym: &str) -> usize {
    let m = host_d3d12();
    if m == 0 {
        return 0;
    }
    let c = CString::new(sym).unwrap();
    GetProcAddress(m, c.as_ptr())
}

/// D3D12_ROOT_SIGNATURE_DESC (x64):
/// +0: NumParameters (u32), +8: pParameters (*const), +16: NumStaticSamplers (u32),
/// +24: pStaticSamplers (*const), +32: Flags (u32) -> 40 bytes total.
#[repr(C)]
#[derive(Clone, Copy)]
struct RootSigDesc {
    num_params: u32,
    _pad0: u32,
    params: *const c_void,
    num_samplers: u32,
    _pad1: u32,
    samplers: *const c_void,
    flags: u32,
    _pad2: u32,
}

/// D3D12_VERSIONED_ROOT_SIGNATURE_DESC (x64):
/// +0: Version (u32: 1 or 2), +8: Desc_1_0 / Desc_1_1 (40 bytes) -> 48 bytes total.
#[repr(C)]
#[derive(Clone, Copy)]
struct VersionedRootSigDesc {
    version: u32,
    _pad: u32,
    desc: RootSigDesc,
}

#[no_mangle]
pub unsafe extern "system" fn D3D12SerializeRootSignature(
    desc: *const c_void,
    version: u32,
    blob: *mut *mut c_void,
    error_blob: *mut *mut c_void,
) -> i32 {
    #[cfg(windows)]
    {
        type FnTy = unsafe extern "system" fn(*const c_void, u32, *mut *mut c_void, *mut *mut c_void) -> i32;
        let addr = host_proc("D3D12SerializeRootSignature");
        if addr == 0 || desc.is_null() {
            return E_FAIL;
        }
        let mut copy = *(desc as *const RootSigDesc);
        copy.flags &= 0x00FF_FFFF;
        let f: FnTy = std::mem::transmute(addr);
        f(&copy as *const RootSigDesc as *const c_void, version, blob, error_blob)
    }
    #[cfg(not(windows))]
    {
        let _ = (desc, version, blob, error_blob);
        E_NOTIMPL
    }
}

#[no_mangle]
pub unsafe extern "system" fn D3D12SerializeVersionedRootSignature(
    desc: *const c_void,
    blob: *mut *mut c_void,
    error_blob: *mut *mut c_void,
) -> i32 {
    #[cfg(windows)]
    {
        type FnTy = unsafe extern "system" fn(*const c_void, *mut *mut c_void, *mut *mut c_void) -> i32;
        let addr = host_proc("D3D12SerializeVersionedRootSignature");
        if addr == 0 || desc.is_null() {
            return E_FAIL;
        }
        let mut copy = *(desc as *const VersionedRootSigDesc);
        copy.desc.flags &= 0x00FF_FFFF;
        let f: FnTy = std::mem::transmute(addr);
        f(&copy as *const VersionedRootSigDesc as *const c_void, blob, error_blob)
    }
    #[cfg(not(windows))]
    {
        let _ = (desc, blob, error_blob);
        E_NOTIMPL
    }
}

#[no_mangle]
pub unsafe extern "system" fn D3D12CreateRootSignatureDeserializer(
    src_data: *const c_void,
    src_bytes: usize,
    riid: *const c_void,
    out_deserializer: *mut *mut c_void,
) -> i32 {
    #[cfg(windows)]
    {
        type FnTy = unsafe extern "system" fn(*const c_void, usize, *const c_void, *mut *mut c_void) -> i32;
        let addr = host_proc("D3D12CreateRootSignatureDeserializer");
        if addr == 0 {
            return E_FAIL;
        }
        let f: FnTy = std::mem::transmute(addr);
        f(src_data, src_bytes, riid, out_deserializer)
    }
    #[cfg(not(windows))]
    {
        let _ = (src_data, src_bytes, riid, out_deserializer);
        E_NOTIMPL
    }
}

#[no_mangle]
pub unsafe extern "system" fn D3D12CreateVersionedRootSignatureDeserializer(
    src_data: *const c_void,
    src_bytes: usize,
    riid: *const c_void,
    out_deserializer: *mut *mut c_void,
) -> i32 {
    #[cfg(windows)]
    {
        type FnTy = unsafe extern "system" fn(*const c_void, usize, *const c_void, *mut *mut c_void) -> i32;
        let addr = host_proc("D3D12CreateVersionedRootSignatureDeserializer");
        if addr == 0 {
            return E_FAIL;
        }
        let f: FnTy = std::mem::transmute(addr);
        f(src_data, src_bytes, riid, out_deserializer)
    }
    #[cfg(not(windows))]
    {
        let _ = (src_data, src_bytes, riid, out_deserializer);
        E_NOTIMPL
    }
}

#[no_mangle]
pub unsafe extern "system" fn D3D12CreateDevice(
    adapter: *mut c_void,
    min_feature_level: u32,
    riid: *const c_void,
    out_device: *mut *mut c_void,
) -> i32 {
    #[cfg(windows)]
    {
        type FnTy = unsafe extern "system" fn(*mut c_void, u32, *const c_void, *mut *mut c_void) -> i32;
        let addr = host_proc("D3D12CreateDevice");
        if addr == 0 {
            return E_FAIL;
        }
        let f: FnTy = std::mem::transmute(addr);
        f(adapter, min_feature_level, riid, out_device)
    }
    #[cfg(not(windows))]
    {
        let _ = (adapter, min_feature_level, riid, out_device);
        E_NOTIMPL
    }
}

#[no_mangle]
pub unsafe extern "system" fn D3D12GetDebugInterface(
    riid: *const c_void,
    out_debug: *mut *mut c_void,
) -> i32 {
    #[cfg(windows)]
    {
        type FnTy = unsafe extern "system" fn(*const c_void, *mut *mut c_void) -> i32;
        let addr = host_proc("D3D12GetDebugInterface");
        if addr == 0 {
            return E_FAIL;
        }
        let f: FnTy = std::mem::transmute(addr);
        f(riid, out_debug)
    }
    #[cfg(not(windows))]
    {
        let _ = (riid, out_debug);
        E_NOTIMPL
    }
}

#[no_mangle]
pub unsafe extern "system" fn D3D12EnableExperimentalFeatures(
    num_features: u32,
    iids: *const c_void,
    configs: *const c_void,
    config_sizes: *const u32,
) -> i32 {
    #[cfg(windows)]
    {
        type FnTy = unsafe extern "system" fn(u32, *const c_void, *const c_void, *const u32) -> i32;
        let addr = host_proc("D3D12EnableExperimentalFeatures");
        if addr == 0 {
            return 0;
        }
        let f: FnTy = std::mem::transmute(addr);
        f(num_features, iids, configs, config_sizes)
    }
    #[cfg(not(windows))]
    {
        let _ = (num_features, iids, configs, config_sizes);
        E_NOTIMPL
    }
}

#[repr(C)]
struct DeviceXProxy {
    vtbl: *const [*const c_void; 128],
    inner: *mut c_void,
    refs: AtomicU32,
}

unsafe extern "system" fn proxy_qi(
    this: *mut c_void,
    riid: *const c_void,
    out: *mut *mut c_void,
) -> i32 {
    if out.is_null() {
        return 0x8000_4003u32 as i32;
    }
    let p = &*(this as *const DeviceXProxy);
    let inner_vtbl = *(p.inner as *const *const *const c_void);
    let qi: unsafe extern "system" fn(*mut c_void, *const c_void, *mut *mut c_void) -> i32 =
        std::mem::transmute(*inner_vtbl.add(0));
    let mut raw_out: *mut c_void = std::ptr::null_mut();
    let hr = qi(p.inner, riid, &mut raw_out);
    if hr >= 0 && !raw_out.is_null() {
        // Release the extra ref from inner QI because proxy holds one ref to inner.
        let rel: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(*inner_vtbl.add(2));
        rel(raw_out);
        proxy_add_ref(this);
        *out = this;
        0
    } else if hr == 0x8000_4002u32 as i32 {
        const IID_IDXGIDEVICE: [u8; 16] = [
            0xfa, 0x77, 0xec, 0x54, 0x77, 0x13, 0xe6, 0x44, 0x8c, 0x32, 0x88, 0xfd, 0x5f, 0x44, 0xc8, 0x4c,
        ];
        const IID_IDXGIDEVICE1: [u8; 16] = [
            0x0f, 0x97, 0xdb, 0x77, 0x76, 0x62, 0xba, 0x48, 0xba, 0x28, 0x07, 0x01, 0x43, 0xb4, 0x39, 0x2c,
        ];
        const IID_IDXGIDEVICE2: [u8; 16] = [
            0x05, 0x5c, 0x00, 0x05, 0xe3, 0x2d, 0x57, 0x4e, 0xa8, 0x05, 0x5a, 0x0e, 0x4d, 0x21, 0x3f, 0x74,
        ];
        if !riid.is_null() {
            let g = *(riid as *const [u8; 16]);
            if std::env::var("PERCY_TRACE").is_ok() {
                eprintln!("[d3d12_xs] proxy_qi E_NOINTERFACE riid={:02x?}", g);
            }
            if g == IID_IDXGIDEVICE || g == IID_IDXGIDEVICE1 || g == IID_IDXGIDEVICE2 {
                *out = create_dxgi_device_proxy();
                return 0;
            }
        }
        // Xbox titles often QI for Scarlett-specific ID3D12DeviceX / ID3D12Device* IIDs; return our proxy.
        proxy_add_ref(this);
        *out = this;
        0
    } else {
        *out = std::ptr::null_mut();
        hr
    }
}

unsafe extern "system" fn proxy_add_ref(this: *mut c_void) -> u32 {
    (*(this as *const DeviceXProxy)).refs.fetch_add(1, Ordering::Relaxed) + 1
}

unsafe extern "system" fn proxy_release(this: *mut c_void) -> u32 {
    let p = &*(this as *const DeviceXProxy);
    let prev = p.refs.fetch_sub(1, Ordering::Release);
    if prev == 1 {
        std::sync::atomic::fence(Ordering::Acquire);
        let inner_vtbl = *(p.inner as *const *const *const c_void);
        let rel: unsafe extern "system" fn(*mut c_void) -> u32 =
            std::mem::transmute(*inner_vtbl.add(2));
        rel(p.inner);
        drop(Box::from_raw(this as *mut DeviceXProxy));
    }
    prev - 1
}

unsafe extern "system" fn xbox_slot_unimpl() -> i32 {
    log_d3d12("ID3D12DeviceX: Xbox-only vtable slot called -> S_OK");
    0
}

#[cfg(all(windows, target_arch = "x86_64"))]
macro_rules! device_fwd_slot {
    ($name:ident, $slot:literal) => {
        #[unsafe(naked)]
        unsafe extern "system" fn $name() {
            std::arch::naked_asm!(
                "mov rcx, qword ptr [rcx + 8]",
                "mov rax, qword ptr [rcx]",
                concat!("jmp qword ptr [rax + ", stringify!($slot), " * 8]"),
            );
        }
    };
}

#[cfg(all(windows, target_arch = "x86_64"))]
mod dev_fwd {
    use super::*;
    device_fwd_slot!(s3, 3);
    device_fwd_slot!(s4, 4);
    device_fwd_slot!(s5, 5);
    device_fwd_slot!(s6, 6);
    device_fwd_slot!(s7, 7);
    device_fwd_slot!(s8, 8);
    device_fwd_slot!(s9, 9);
    device_fwd_slot!(s10, 10);
    device_fwd_slot!(s11, 11);
    device_fwd_slot!(s12, 12);
    device_fwd_slot!(s13, 13);
    device_fwd_slot!(s14, 14);
    device_fwd_slot!(s15, 15);
    device_fwd_slot!(s16, 16);
    device_fwd_slot!(s17, 17);
    device_fwd_slot!(s18, 18);
    device_fwd_slot!(s19, 19);
    device_fwd_slot!(s20, 20);
    device_fwd_slot!(s21, 21);
    device_fwd_slot!(s22, 22);
    device_fwd_slot!(s23, 23);
    device_fwd_slot!(s24, 24);
    device_fwd_slot!(s25, 25);
    device_fwd_slot!(s26, 26);
    device_fwd_slot!(s27, 27);
    device_fwd_slot!(s28, 28);
    device_fwd_slot!(s29, 29);
    device_fwd_slot!(s30, 30);
    device_fwd_slot!(s31, 31);
    device_fwd_slot!(s32, 32);
    device_fwd_slot!(s33, 33);
    device_fwd_slot!(s34, 34);
    device_fwd_slot!(s35, 35);
    device_fwd_slot!(s36, 36);
    device_fwd_slot!(s37, 37);
    device_fwd_slot!(s38, 38);
    device_fwd_slot!(s39, 39);
    device_fwd_slot!(s40, 40);
    device_fwd_slot!(s41, 41);
    device_fwd_slot!(s42, 42);
    device_fwd_slot!(s43, 43);
    pub const SLOTS: [*const c_void; 41] = [
        s3 as *const c_void, s4 as *const c_void, s5 as *const c_void, s6 as *const c_void,
        s7 as *const c_void, s8 as *const c_void, s9 as *const c_void, s10 as *const c_void,
        s11 as *const c_void, s12 as *const c_void, s13 as *const c_void, s14 as *const c_void,
        s15 as *const c_void, s16 as *const c_void, s17 as *const c_void, s18 as *const c_void,
        s19 as *const c_void, s20 as *const c_void, s21 as *const c_void, s22 as *const c_void,
        s23 as *const c_void, s24 as *const c_void, s25 as *const c_void, s26 as *const c_void,
        s27 as *const c_void, s28 as *const c_void, s29 as *const c_void, s30 as *const c_void,
        s31 as *const c_void, s32 as *const c_void, s33 as *const c_void, s34 as *const c_void,
        s35 as *const c_void, s36 as *const c_void, s37 as *const c_void, s38 as *const c_void,
        s39 as *const c_void, s40 as *const c_void, s41 as *const c_void, s42 as *const c_void,
        s43 as *const c_void,
    ];
}

unsafe fn wrap_device_pointer(inner: *mut c_void) -> *mut c_void {
    if inner.is_null() {
        return inner;
    }
    let mut vtbl = [xbox_slot_unimpl as *const c_void; 128];
    vtbl[0] = proxy_qi as *const c_void;
    vtbl[1] = proxy_add_ref as *const c_void;
    vtbl[2] = proxy_release as *const c_void;
    #[cfg(all(windows, target_arch = "x86_64"))]
    {
        vtbl[3..44].copy_from_slice(&dev_fwd::SLOTS);
    }
    let leaked_vtbl = Box::leak(Box::new(vtbl));
    let proxy = Box::new(DeviceXProxy {
        vtbl: leaked_vtbl as *const [*const c_void; 128],
        inner,
        refs: AtomicU32::new(1),
    });
    Box::into_raw(proxy) as *mut c_void
}

#[cfg(windows)]
#[repr(C)]
struct DxgiDeviceProxy {
    vtbl: *const [*const c_void; 16],
    refs: AtomicU32,
}

unsafe extern "system" fn dxgi_qi(this: *mut c_void, _riid: *const c_void, out: *mut *mut c_void) -> i32 {
    if out.is_null() {
        return 0x8000_4003u32 as i32;
    }
    dxgi_add_ref(this);
    *out = this;
    0
}

unsafe extern "system" fn dxgi_add_ref(this: *mut c_void) -> u32 {
    (*(this as *const DxgiDeviceProxy)).refs.fetch_add(1, Ordering::Relaxed) + 1
}

unsafe extern "system" fn dxgi_release(this: *mut c_void) -> u32 {
    let p = &*(this as *const DxgiDeviceProxy);
    let prev = p.refs.fetch_sub(1, Ordering::Release);
    if prev == 1 {
        std::sync::atomic::fence(Ordering::Acquire);
        drop(Box::from_raw(this as *mut DxgiDeviceProxy));
    }
    prev - 1
}

unsafe extern "system" fn dxgi_get_adapter(_this: *mut c_void, out_adapter: *mut *mut c_void) -> i32 {
    if out_adapter.is_null() {
        return 0x8000_4003u32 as i32;
    }
    #[cfg(windows)]
    {
        let ad = pick_high_perf_adapter();
        *out_adapter = ad;
        if ad.is_null() { E_FAIL } else { 0 }
    }
    #[cfg(not(windows))]
    {
        *out_adapter = std::ptr::null_mut();
        E_NOTIMPL
    }
}

unsafe fn create_dxgi_device_proxy() -> *mut c_void {
    let mut vtbl = [xbox_slot_unimpl as *const c_void; 16];
    vtbl[0] = dxgi_qi as *const c_void;
    vtbl[1] = dxgi_add_ref as *const c_void;
    vtbl[2] = dxgi_release as *const c_void;
    vtbl[7] = dxgi_get_adapter as *const c_void;
    let leaked_vtbl = Box::leak(Box::new(vtbl));
    Box::into_raw(Box::new(DxgiDeviceProxy {
        vtbl: leaked_vtbl as *const [*const c_void; 16],
        refs: AtomicU32::new(1),
    })) as *mut c_void
}

#[cfg(windows)]
unsafe fn pick_high_perf_adapter() -> *mut c_void {
    #[repr(C)]
    struct AdapterDesc1 {
        _desc: [u16; 128],
        _ven: u32,
        _dev: u32,
        _sub: u32,
        _rev: u32,
        ded_vram: usize,
        _ded_sys: usize,
        _shared_sys: usize,
        _luid: u64,
        flags: u32,
    }
    extern "system" {
        fn LoadLibraryA(name: *const i8) -> usize;
        fn GetProcAddress(module: usize, name: *const i8) -> usize;
    }
    let dxgi = LoadLibraryA(b"dxgi.dll\0".as_ptr() as *const i8);
    if dxgi == 0 {
        return std::ptr::null_mut();
    }
    let p_create = GetProcAddress(dxgi, b"CreateDXGIFactory1\0".as_ptr() as *const i8);
    if p_create == 0 {
        return std::ptr::null_mut();
    }
    let iid_fac1: [u8; 16] = [0x78, 0xae, 0x0a, 0x77, 0x6f, 0xf2, 0xba, 0x4d, 0xa8, 0x29, 0x25, 0x3c, 0x83, 0xd1, 0xb3, 0x87];
    let create_fac: unsafe extern "system" fn(*const [u8; 16], *mut *mut c_void) -> i32 = std::mem::transmute(p_create);
    let mut fac: *mut c_void = std::ptr::null_mut();
    if create_fac(&iid_fac1, &mut fac) != 0 || fac.is_null() {
        return std::ptr::null_mut();
    }
    let fac_vtbl = &*(*(fac as *const *const [*const c_void; 16]));
    let enum_ad: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> i32 = std::mem::transmute(fac_vtbl[12]);
    let fac_rel: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(fac_vtbl[2]);

    let mut best: *mut c_void = std::ptr::null_mut();
    let mut best_vram: usize = 0;
    for i in 0..16u32 {
        let mut cand: *mut c_void = std::ptr::null_mut();
        if enum_ad(fac, i, &mut cand) != 0 || cand.is_null() {
            break;
        }
        let ad_vtbl = &*(*(cand as *const *const [*const c_void; 12]));
        let get_desc: unsafe extern "system" fn(*mut c_void, *mut AdapterDesc1) -> i32 = std::mem::transmute(ad_vtbl[10]);
        let ad_rel: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(ad_vtbl[2]);
        let mut d: AdapterDesc1 = std::mem::zeroed();
        if get_desc(cand, &mut d) == 0 && (d.flags & 2) == 0 && (best.is_null() || d.ded_vram > best_vram) {
            if !best.is_null() {
                let old_vtbl = &*(*(best as *const *const [*const c_void; 12]));
                let old_rel: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(old_vtbl[2]);
                old_rel(best);
            }
            best = cand;
            best_vram = d.ded_vram;
            continue;
        }
        ad_rel(cand);
    }
    fac_rel(fac);
    best
}

#[no_mangle]
pub unsafe extern "system" fn D3D12XboxCreateDevice(
    adapter: *mut c_void,
    _params: *const c_void,
    riid: *const c_void,
    out_device: *mut *mut c_void,
) -> i32 {
    if out_device.is_null() {
        return 0x8000_4003u32 as i32; // E_POINTER
    }
    const D3D_FEATURE_LEVEL_12_0: u32 = 0xC000;
    #[cfg(windows)]
    let chosen = if adapter.is_null() { pick_high_perf_adapter() } else { adapter };
    #[cfg(not(windows))]
    let chosen = adapter;
    let mut hr = D3D12CreateDevice(chosen, D3D_FEATURE_LEVEL_12_0, riid, out_device);
    let mut forced_wrap = false;
    if hr < 0 {
        // Xbox titles often pass a Scarlett-specific ID3D12DeviceX IID; fall back to PC ID3D12Device and wrap.
        const IID_ID3D12_DEVICE: [u8; 16] = [
            0xf1, 0x19, 0x98, 0x18, 0xb6, 0x1d, 0x57, 0x4b, 0xbe, 0x54, 0x18, 0x21, 0x33, 0x9b, 0x85, 0xf7,
        ];
        hr = D3D12CreateDevice(chosen, D3D_FEATURE_LEVEL_12_0, IID_ID3D12_DEVICE.as_ptr() as *const c_void, out_device);
        forced_wrap = true;
    }
    #[cfg(windows)]
    if adapter.is_null() && !chosen.is_null() {
        let ad_vtbl = &*(*(chosen as *const *const [*const c_void; 12]));
        let ad_rel: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(ad_vtbl[2]);
        ad_rel(chosen);
    }
    if hr >= 0 && !(*out_device).is_null() {
        *out_device = wrap_device_pointer(*out_device);
    }
    if std::env::var("PERCY_TRACE").is_ok() {
        eprintln!("[d3d12_xs] D3D12XboxCreateDevice -> hr=0x{hr:08x} forced_wrap={forced_wrap}");
    }
    hr
}

static PRESENT_FRAMES: AtomicU64 = AtomicU64::new(0);
static BOUND_SWAPCHAIN: AtomicUsize = AtomicUsize::new(0);

#[no_mangle]
pub unsafe extern "system" fn PercyRegisterSwapChain(swapchain: *mut c_void) {
    BOUND_SWAPCHAIN.store(swapchain as usize, Ordering::Release);
}

#[no_mangle]
pub extern "system" fn PercyGetPresentedFrameCount() -> u64 {
    PRESENT_FRAMES.load(Ordering::Acquire)
}

#[no_mangle]
pub unsafe extern "system" fn PresentX(
    _queue_or_plane: *mut c_void,
    _present_params: *const c_void,
    _reserved: *const c_void,
) -> i32 {
    let frame = PRESENT_FRAMES.fetch_add(1, Ordering::AcqRel) + 1;
    let sc = BOUND_SWAPCHAIN.load(Ordering::Acquire) as *mut c_void;
    if !sc.is_null() {
        // IDXGISwapChain::Present is vtable slot 8
        let vtbl = *(sc as *const *const *const c_void);
        let present: unsafe extern "system" fn(*mut c_void, u32, u32) -> i32 =
            std::mem::transmute(*vtbl.add(8));
        return present(sc, 1, 0);
    }
    log_d3d12(&format!("PresentX frame={frame}"));
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn WaitForOriginX(
    _queue: *mut c_void,
    _reserved: u64,
    _timeout_us: u32,
) -> i32 {
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn RegisterFrameEventX(
    _queue: *mut c_void,
    _event_type: u32,
    _out_token: *mut u64,
) -> i32 {
    if !_out_token.is_null() {
        *_out_token = 1;
    }
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn SetFrameIntervalX(
    _device: *mut c_void,
    _period_us: u32,
    _flags: u32,
) -> i32 {
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn ScheduleFrameEventX(
    _queue: *mut c_void,
    _event_token: u64,
    _flags: u32,
) -> i32 {
    S_OK
}

#[repr(C)]
pub struct CommandQueueXProxy {
    vtbl: *const [*const c_void; 24],
    inner: *mut c_void,
    refs: AtomicU32,
}

unsafe extern "system" fn cq_qi(this: *mut c_void, riid: *const c_void, out: *mut *mut c_void) -> i32 {
    if out.is_null() { return 0x8000_4003u32 as i32; }
    let p = &*(this as *const CommandQueueXProxy);
    if p.inner.is_null() {
        cq_add_ref(this);
        *out = this;
        return S_OK;
    }
    let inner_vtbl = *(p.inner as *const *const *const c_void);
    let qi: unsafe extern "system" fn(*mut c_void, *const c_void, *mut *mut c_void) -> i32 =
        std::mem::transmute(*inner_vtbl.add(0));
    let mut raw: *mut c_void = std::ptr::null_mut();
    let hr = qi(p.inner, riid, &mut raw);
    if hr >= 0 && !raw.is_null() {
        cq_add_ref(this);
        *out = this;
    } else {
        *out = std::ptr::null_mut();
    }
    hr
}
unsafe extern "system" fn cq_add_ref(this: *mut c_void) -> u32 {
    (*(this as *const CommandQueueXProxy)).refs.fetch_add(1, Ordering::Relaxed) + 1
}
unsafe extern "system" fn cq_release(this: *mut c_void) -> u32 {
    let p = &*(this as *const CommandQueueXProxy);
    let prev = p.refs.fetch_sub(1, Ordering::Release);
    if prev == 1 {
        std::sync::atomic::fence(Ordering::Acquire);
        if !p.inner.is_null() {
            let inner_vtbl = *(p.inner as *const *const *const c_void);
            let rel: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(*inner_vtbl.add(2));
            rel(p.inner);
        }
        drop(Box::from_raw(this as *mut CommandQueueXProxy));
    }
    prev - 1
}

pub unsafe fn wrap_command_queue(inner: *mut c_void) -> *mut c_void {
    let mut vtbl = [xbox_slot_unimpl as *const c_void; 24];
    vtbl[0] = cq_qi as *const c_void;
    vtbl[1] = cq_add_ref as *const c_void;
    vtbl[2] = cq_release as *const c_void;
    vtbl[19] = PresentX as *const c_void;
    vtbl[20] = WaitForOriginX as *const c_void;
    let leaked = Box::leak(Box::new(vtbl));
    Box::into_raw(Box::new(CommandQueueXProxy {
        vtbl: leaked as *const [*const c_void; 24],
        inner,
        refs: AtomicU32::new(1),
    })) as *mut c_void
}

/// Auto-creates a Win32 window + DXGI `IDXGISwapChain3` bound to `BOUND_SWAPCHAIN` so `PresentX`
/// presents real frames on Windows 10/11 when given a valid `ID3D12CommandQueue`.
#[no_mangle]
pub unsafe extern "system" fn PercyEnsureAutoSwapChain(
    command_queue: *mut c_void,
    width: u32,
    height: u32,
) -> i32 {
    #[cfg(windows)]
    {
        if BOUND_SWAPCHAIN.load(Ordering::Acquire) != 0 {
            return S_OK;
        }
        if command_queue.is_null() {
            return 0x8000_4003u32 as i32;
        }
        #[repr(C)]
        struct WndClassExW {
            cb_size: u32,
            style: u32,
            wnd_proc: unsafe extern "system" fn(usize, u32, usize, isize) -> isize,
            cls_extra: i32,
            wnd_extra: i32,
            instance: usize,
            icon: usize,
            cursor: usize,
            bg: usize,
            menu: *const u16,
            class_name: *const u16,
            icon_sm: usize,
        }
        #[repr(C)]
        struct SwapChainDesc1 {
            width: u32,
            height: u32,
            format: u32,
            stereo: i32,
            sample_count: u32,
            sample_quality: u32,
            buffer_usage: u32,
            buffer_count: u32,
            scaling: u32,
            swap_effect: u32,
            alpha_mode: u32,
            flags: u32,
        }
        extern "system" {
            fn GetModuleHandleW(name: *const u16) -> usize;
            fn RegisterClassExW(wc: *const WndClassExW) -> u16;
            fn CreateWindowExW(
                ex: u32,
                cls: *const u16,
                title: *const u16,
                style: u32,
                x: i32,
                y: i32,
                w: i32,
                h: i32,
                parent: usize,
                menu: usize,
                inst: usize,
                param: *mut c_void,
            ) -> usize;
            fn DefWindowProcW(hwnd: usize, msg: u32, w: usize, l: isize) -> isize;
        }
        let cls_name: Vec<u16> = "PercyScarlettWnd\0".encode_utf16().collect();
        let title: Vec<u16> = "PercyScarlett\0".encode_utf16().collect();
        let hinst = GetModuleHandleW(std::ptr::null());
        let wc = WndClassExW {
            cb_size: std::mem::size_of::<WndClassExW>() as u32,
            style: 0,
            wnd_proc: DefWindowProcW,
            cls_extra: 0,
            wnd_extra: 0,
            instance: hinst,
            icon: 0,
            cursor: 0,
            bg: 0,
            menu: std::ptr::null(),
            class_name: cls_name.as_ptr(),
            icon_sm: 0,
        };
        let _ = RegisterClassExW(&wc);
        let hwnd = CreateWindowExW(
            0,
            cls_name.as_ptr(),
            title.as_ptr(),
            0x00CF_0000, // WS_OVERLAPPEDWINDOW
            100,
            100,
            width.max(320) as i32,
            height.max(240) as i32,
            0,
            0,
            hinst,
            std::ptr::null_mut(),
        );
        if hwnd == 0 {
            return E_FAIL;
        }
        let dxgi_name = CString::new("dxgi.dll").unwrap();
        let dxgi = LoadLibraryA(dxgi_name.as_ptr());
        if dxgi == 0 {
            return E_FAIL;
        }
        let sym = CString::new("CreateDXGIFactory1").unwrap();
        let addr = GetProcAddress(dxgi, sym.as_ptr());
        if addr == 0 {
            return E_FAIL;
        }
        type CreateFactoryFn = unsafe extern "system" fn(*const [u8; 16], *mut *mut c_void) -> i32;
        let create_factory: CreateFactoryFn = std::mem::transmute(addr);
        // IID_IDXGIFactory2 {50c83a1c-e072-4c48-87b0-3630fa36a6d0}
        const IID_IDXGIFACTORY2: [u8; 16] = [
            0x1c, 0x3a, 0xc8, 0x50, 0x72, 0xe0, 0x48, 0x4c, 0x87, 0xb0, 0x36, 0x30, 0xfa, 0x36, 0xa6, 0xd0,
        ];
        let mut factory: *mut c_void = std::ptr::null_mut();
        let hr = create_factory(&IID_IDXGIFACTORY2, &mut factory);
        if hr < 0 || factory.is_null() {
            return hr;
        }
        let desc = SwapChainDesc1 {
            width: width.max(320),
            height: height.max(240),
            format: 28, // DXGI_FORMAT_R8G8B8A8_UNORM
            stereo: 0,
            sample_count: 1,
            sample_quality: 0,
            buffer_usage: 0x20, // DXGI_USAGE_RENDER_TARGET_OUTPUT
            buffer_count: 2,
            scaling: 0,
            swap_effect: 4, // DXGI_SWAP_EFFECT_FLIP_DISCARD
            alpha_mode: 0,
            flags: 0,
        };
        // IDXGIFactory2::CreateSwapChainForHwnd is vtable slot 15
        let fvtbl = *(factory as *const *const *const c_void);
        let create_sc: unsafe extern "system" fn(
            *mut c_void,
            *mut c_void,
            usize,
            *const SwapChainDesc1,
            *const c_void,
            *mut c_void,
            *mut *mut c_void,
        ) -> i32 = std::mem::transmute(*fvtbl.add(15));
        let mut sc: *mut c_void = std::ptr::null_mut();
        let hr_sc = create_sc(factory, command_queue, hwnd, &desc, std::ptr::null(), std::ptr::null_mut(), &mut sc);
        if hr_sc >= 0 && !sc.is_null() {
            BOUND_SWAPCHAIN.store(sc as usize, Ordering::Release);
            log_d3d12("PercyEnsureAutoSwapChain: created IDXGISwapChain1 + HWND");
        }
        hr_sc
    }
    #[cfg(not(windows))]
    {
        let _ = (command_queue, width, height);
        E_NOTIMPL
    }
}

/// Lifts RDNA2 bytecode at runtime to SM 6.6 DXIL via `dxcompiler.dll` (using disk cache).
#[no_mangle]
pub unsafe extern "system" fn PercyLiftRdna2ToDxil(
    rdna2_ptr: *const u8,
    rdna2_len: usize,
    stage_hint: u32,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    #[cfg(windows)]
    {
        if rdna2_ptr.is_null() || rdna2_len == 0 || out_ptr.is_null() || out_len.is_null() {
            return 0x8000_4003u32 as i32;
        }
        let hint = match stage_hint {
            0 => rdna2_hlsl::ShaderStage::Vertex,
            1 => rdna2_hlsl::ShaderStage::Pixel,
            2 => rdna2_hlsl::ShaderStage::Compute,
            3 => rdna2_hlsl::ShaderStage::Mesh,
            4 => rdna2_hlsl::ShaderStage::Geometry,
            _ => rdna2_hlsl::ShaderStage::RayTracing,
        };
        let bytes = std::slice::from_raw_parts(rdna2_ptr, rdna2_len);
        let (stage, hlsl, _) = rdna2_hlsl::lift_to_hlsl(bytes, hint);
        match compile_hlsl_to_dxil(&hlsl, stage) {
            Ok(dxil) => {
                let len = dxil.len();
                let boxed = dxil.into_boxed_slice();
                *out_ptr = Box::into_raw(boxed) as *mut u8;
                *out_len = len;
                S_OK
            }
            Err(_) => E_FAIL,
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (rdna2_ptr, rdna2_len, stage_hint, out_ptr, out_len);
        E_NOTIMPL
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_host_d3d12_exports_on_windows() {
        if cfg!(windows) {
            unsafe {
                assert_ne!(host_d3d12(), 0);
                assert_ne!(host_proc("D3D12CreateDevice"), 0);
                assert_ne!(host_proc("D3D12SerializeRootSignature"), 0);
                assert_ne!(host_proc("D3D12SerializeVersionedRootSignature"), 0);
            }
        }
    }

    #[test]
    fn device_x_proxy_wraps_and_releases_cleanly() {
        unsafe extern "system" fn dummy_qi(_: *mut c_void, _: *const c_void, _: *mut *mut c_void) -> i32 { E_NOTIMPL }
        unsafe extern "system" fn dummy_add(_: *mut c_void) -> u32 { 2 }
        unsafe extern "system" fn dummy_rel(_: *mut c_void) -> u32 { 0 }
        #[repr(C)]
        struct DummyCom { vtbl: *const [*const c_void; 3] }

        unsafe {
            let vtbl = [dummy_qi as *const c_void, dummy_add as *const c_void, dummy_rel as *const c_void];
            let mut inner = DummyCom { vtbl: &vtbl };
            let proxy = wrap_device_pointer(&mut inner as *mut DummyCom as *mut c_void);
            let p_vtbl = &*(*(proxy as *const *const [*const c_void; 64]));
            let xbox_slot: unsafe extern "system" fn() -> i32 = std::mem::transmute(p_vtbl[50]);
            assert_eq!(xbox_slot(), 0);
            let rel: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(p_vtbl[2]);
            assert_eq!(rel(proxy), 0);
        }
    }

    #[test]
    fn disk_shader_cache_roundtrip() {
        let key = b"rsig_v1_test_key";
        let blob = b"\x44\x58\x42\x43_cached_pso";
        cache_store_blob("pso", key, blob);
        assert_eq!(cache_load_blob("pso", key).as_deref(), Some(blob.as_slice()));

        let rdna2 = [0x00, 0x00, 0x81, 0xBF]; // s_endpgm
        let (stage, hlsl) = translate_and_cache_rdna2(&rdna2, rdna2_hlsl::ShaderStage::Pixel);
        assert_eq!(stage, rdna2_hlsl::ShaderStage::Pixel);
        assert!(hlsl.contains("SV_TARGET"));

        if cfg!(windows) {
            // Compile lifted VS, PS, and CS RDNA2 programs to real DXBC via d3dcompiler_47.dll.
            for st in [
                rdna2_hlsl::ShaderStage::Vertex,
                rdna2_hlsl::ShaderStage::Pixel,
                rdna2_hlsl::ShaderStage::Compute,
            ] {
                let mut prog = Vec::new();
                prog.extend_from_slice(&((0x3Fu32 << 25) | (0u32 << 17) | (0x01u32 << 9) | 242u32).to_le_bytes()); // v_mov_b32 v0, 1.0
                prog.extend_from_slice(&((0x3Eu32 << 26) | (12u32 << 4) | 0xFu32).to_le_bytes()); // exp pos0
                prog.extend_from_slice(&0u32.to_le_bytes());
                prog.extend_from_slice(&0xBF81_0000u32.to_le_bytes()); // s_endpgm
                let (_, lifted) = translate_and_cache_rdna2(&prog, st);
                let dxbc = compile_hlsl_to_dxbc(&lifted, st).expect("d3dcompiler_47 DXBC compile");
                assert!(dxbc.len() > 32 && &dxbc[0..4] == b"DXBC");
            }
        }
    }

    #[test]
    fn compiles_rdna2_to_sm66_dxil_and_presents_frames() {
        if cfg!(windows) {
            for st in [
                rdna2_hlsl::ShaderStage::Vertex,
                rdna2_hlsl::ShaderStage::Pixel,
                rdna2_hlsl::ShaderStage::Compute,
                rdna2_hlsl::ShaderStage::Mesh,
                rdna2_hlsl::ShaderStage::RayTracing,
            ] {
                let mut prog = Vec::new();
                prog.extend_from_slice(&((0x3Fu32 << 25) | (0u32 << 17) | (0x01u32 << 9) | 242u32).to_le_bytes()); // v_mov_b32 v0, 1.0
                prog.extend_from_slice(&((0x3Eu32 << 26) | (12u32 << 4) | 0xFu32).to_le_bytes()); // exp pos0
                prog.extend_from_slice(&0u32.to_le_bytes());
                prog.extend_from_slice(&0xBF81_0000u32.to_le_bytes()); // s_endpgm
                let (_, lifted) = translate_and_cache_rdna2(&prog, st);
                let dxil = compile_hlsl_to_dxil(&lifted, st).expect("dxcompiler.dll SM 6.6 DXIL compile");
                assert!(dxil.len() > 32 && &dxil[0..4] == b"DXBC");
            }
        }
        unsafe {
            let before = PercyGetPresentedFrameCount();
            assert_eq!(PresentX(std::ptr::null_mut(), std::ptr::null(), std::ptr::null()), S_OK);
            assert_eq!(WaitForOriginX(std::ptr::null_mut(), 0, 16667), S_OK);
            let mut tok = 0u64;
            assert_eq!(RegisterFrameEventX(std::ptr::null_mut(), 0, &mut tok), S_OK);
            assert_eq!(tok, 1);
            assert_eq!(PercyGetPresentedFrameCount(), before + 1);

            let cq = wrap_command_queue(std::ptr::null_mut());
            let cq_vtbl = &*(*(cq as *const *const [*const c_void; 24]));
            let rel: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(cq_vtbl[2]);
            assert_eq!(rel(cq), 0);
        }
    }
}
