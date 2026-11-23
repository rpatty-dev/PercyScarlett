pub mod compute_exec;
pub mod rdna2_hlsl;
pub mod rdna2_isa;

#[cfg(windows)]
#[link(name = "user32")]
extern "system" {}


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
    if std::env::var("PERCY_TRACE").is_ok() && msg.starts_with('[') {
        eprintln!("{msg}");
    }
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
    vtbl: *const [*const c_void; 256],
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
        // IDXGIDevice2 {05008617-fbfd-4051-a790-144884b4f6a9}
        const IID_IDXGIDEVICE2: [u8; 16] = [
            0x17, 0x86, 0x00, 0x05, 0xfd, 0xfb, 0x51, 0x40, 0xa7, 0x90, 0x14, 0x48, 0x84, 0xb4, 0xf6, 0xa9,
        ];
        // IDXGIDevice3 {6007896c-3244-4afd-bf18-a6d3beda5023}
        const IID_IDXGIDEVICE3: [u8; 16] = [
            0x6c, 0x89, 0x07, 0x60, 0x44, 0x32, 0xfd, 0x4a, 0xbf, 0x18, 0xa6, 0xd3, 0xbe, 0xda, 0x50, 0x23,
        ];
        if !riid.is_null() {
            let g = *(riid as *const [u8; 16]);
            if std::env::var("PERCY_TRACE").is_ok() {
                eprintln!("[d3d12_xs] proxy_qi E_NOINTERFACE riid={:02x?}", g);
            }
            if g == IID_IDXGIDEVICE || g == IID_IDXGIDEVICE1 || g == IID_IDXGIDEVICE2 || g == IID_IDXGIDEVICE3 {
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
        pub(crate) unsafe extern "system" fn $name() {
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
    device_fwd_slot!(s44, 44);
    device_fwd_slot!(s45, 45);
    device_fwd_slot!(s46, 46);
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


/// Per-slot Xbox-only ID3D12DeviceX stubs: log the slot index once, return S_OK.
mod console_list;
mod replay;
mod mirror;
mod present;
mod xbox_slots {
    use super::*;
    static SEEN: [std::sync::atomic::AtomicBool; 128] = [const { std::sync::atomic::AtomicBool::new(false) }; 128];
    pub(super) fn note(slot: usize, a: [usize; 6]) {
        if !SEEN[slot].swap(true, Ordering::Relaxed) {
            // First guest return address on the stack (guest is mapped at its preferred base).
            let rsp: usize;
            unsafe { std::arch::asm!("mov {}, rsp", out(reg) rsp) };
            let ra = (0..96)
                .map(|i| unsafe { *((rsp as *const usize).add(i)) })
                .find(|&v| (0x1_4000_1000..0x1_43d0_0000).contains(&v))
                .unwrap_or(0);
            log_d3d12(&format!(
                "[d3d12x] ID3D12DeviceX slot {slot} (unimplemented) from G+{:x} args=({:#x},{:#x},{:#x},{:#x},{:#x}) -> S_OK",
                ra.wrapping_sub(0x1_4000_0000), a[1], a[2], a[3], a[4], a[5]
            ));
        }
    }
    macro_rules! xslot {
        ($name:ident, $n:literal) => {
            unsafe extern "system" fn $name(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> i32 {
                note($n, [a, b, c, d, e, f]);
                0
            }
        };
    }
    xslot!(x44, 44);
    xslot!(x45, 45);
    xslot!(x46, 46);
    xslot!(x47, 47);
    xslot!(x48, 48);
    xslot!(x49, 49);
    xslot!(x50, 50);
    xslot!(x51, 51);
    xslot!(x52, 52);
    xslot!(x53, 53);
    xslot!(x54, 54);
    xslot!(x55, 55);
    xslot!(x56, 56);
    xslot!(x57, 57);
    xslot!(x58, 58);
    xslot!(x59, 59);
    xslot!(x60, 60);
    xslot!(x61, 61);
    xslot!(x62, 62);
    xslot!(x63, 63);
    xslot!(x64, 64);
    xslot!(x65, 65);
    xslot!(x66, 66);
    xslot!(x67, 67);
    xslot!(x68, 68);
    xslot!(x69, 69);
    xslot!(x70, 70);
    xslot!(x71, 71);
    xslot!(x72, 72);
    xslot!(x73, 73);
    xslot!(x74, 74);
    xslot!(x75, 75);
    xslot!(x76, 76);
    xslot!(x77, 77);
    xslot!(x78, 78);
    xslot!(x79, 79);
    xslot!(x80, 80);
    xslot!(x81, 81);
    xslot!(x82, 82);
    xslot!(x83, 83);
    xslot!(x84, 84);
    xslot!(x85, 85);
    xslot!(x86, 86);
    xslot!(x87, 87);
    xslot!(x88, 88);
    xslot!(x89, 89);
    xslot!(x90, 90);
    xslot!(x91, 91);
    xslot!(x92, 92);
    xslot!(x93, 93);
    xslot!(x94, 94);
    xslot!(x95, 95);
    xslot!(x96, 96);
    xslot!(x97, 97);
    xslot!(x98, 98);
    xslot!(x99, 99);
    xslot!(x100, 100);
    xslot!(x101, 101);
    xslot!(x102, 102);
    xslot!(x103, 103);
    xslot!(x104, 104);
    xslot!(x105, 105);
    xslot!(x106, 106);
    xslot!(x107, 107);
    xslot!(x108, 108);
    xslot!(x109, 109);
    xslot!(x110, 110);
    xslot!(x111, 111);
    xslot!(x112, 112);
    xslot!(x113, 113);
    xslot!(x114, 114);
    xslot!(x115, 115);
    xslot!(x116, 116);
    xslot!(x117, 117);
    xslot!(x118, 118);
    xslot!(x119, 119);
    xslot!(x120, 120);
    xslot!(x121, 121);
    xslot!(x122, 122);
    xslot!(x123, 123);
    xslot!(x124, 124);
    xslot!(x125, 125);
    xslot!(x126, 126);
    xslot!(x127, 127);
    pub const SLOTS: [*const c_void; 84] = [x44 as *const c_void, x45 as *const c_void, x46 as *const c_void, x47 as *const c_void, x48 as *const c_void, x49 as *const c_void, x50 as *const c_void, x51 as *const c_void, x52 as *const c_void, x53 as *const c_void, x54 as *const c_void, x55 as *const c_void, x56 as *const c_void, x57 as *const c_void, x58 as *const c_void, x59 as *const c_void, x60 as *const c_void, x61 as *const c_void, x62 as *const c_void, x63 as *const c_void, x64 as *const c_void, x65 as *const c_void, x66 as *const c_void, x67 as *const c_void, x68 as *const c_void, x69 as *const c_void, x70 as *const c_void, x71 as *const c_void, x72 as *const c_void, x73 as *const c_void, x74 as *const c_void, x75 as *const c_void, x76 as *const c_void, x77 as *const c_void, x78 as *const c_void, x79 as *const c_void, x80 as *const c_void, x81 as *const c_void, x82 as *const c_void, x83 as *const c_void, x84 as *const c_void, x85 as *const c_void, x86 as *const c_void, x87 as *const c_void, x88 as *const c_void, x89 as *const c_void, x90 as *const c_void, x91 as *const c_void, x92 as *const c_void, x93 as *const c_void, x94 as *const c_void, x95 as *const c_void, x96 as *const c_void, x97 as *const c_void, x98 as *const c_void, x99 as *const c_void, x100 as *const c_void, x101 as *const c_void, x102 as *const c_void, x103 as *const c_void, x104 as *const c_void, x105 as *const c_void, x106 as *const c_void, x107 as *const c_void, x108 as *const c_void, x109 as *const c_void, x110 as *const c_void, x111 as *const c_void, x112 as *const c_void, x113 as *const c_void, x114 as *const c_void, x115 as *const c_void, x116 as *const c_void, x117 as *const c_void, x118 as *const c_void, x119 as *const c_void, x120 as *const c_void, x121 as *const c_void, x122 as *const c_void, x123 as *const c_void, x124 as *const c_void, x125 as *const c_void, x126 as *const c_void, x127 as *const c_void];
}

// Struct-returning methods: the console D3D12.X ABI passes the hidden return buffer FIRST
// (rcx = ret, rdx = this, ...), while PC D3D12Core (MSVC C++ ABI) expects (rcx = this, rdx = ret, ...).
// These adapters swap the two and unwrap the proxy. Observed on ID3D12DeviceX slot 25.
unsafe fn devx_inner_fn(this: *mut c_void, slot: usize) -> (*mut c_void, *const c_void) {
    let inner = (*(this as *const DeviceXProxy)).inner;
    let vt = *(inner as *const *const *const c_void);
    (inner, *vt.add(slot))
}
/// Slot 25: GetResourceAllocationInfo(ret, this, visibleMask, numDescs, descs) -> ret.
unsafe extern "system" fn devx_get_resource_allocation_info(ret: *mut c_void, this: *mut c_void, mask: u32, n: u32, descs: *const c_void) -> *mut c_void {
    let (inner, f) = devx_inner_fn(this, 25);
    let f: unsafe extern "system" fn(*mut c_void, *mut c_void, u32, u32, *const c_void) -> *mut c_void = std::mem::transmute(f);
    // Console-only flag bits make the PC runtime report SizeInBytes = UINT64_MAX.
    let clean: Vec<[u64; 7]> = (0..n as usize).map(|i| sanitize_resource_desc((descs as *const u8).add(i * 56))).collect();
    f(inner, ret, mask, n, if descs.is_null() { descs } else { clean.as_ptr() as *const c_void });
    let r = ret as *const u64;
    static AI_N: AtomicU32 = AtomicU32::new(0);
    if n > 0 && (*r == u64::MAX || *r > (1 << 32) || (clean[0][0] as u32 == 3 && AI_N.fetch_add(1, Ordering::Relaxed) < 40)) {
        let d = &clean[0];
        log_d3d12(&format!(
            "[d3d12x] AllocInfo n={n} ra={:#x} size={:#x} align={:#x} dim={} w={} h={} da={} fmt={} fl={:#x}",
            0usize, *r, *r.add(1), d[0] as u32, d[2], d[3] as u32, (d[3] >> 32) as u16, d[4] as u32, d[6] as u32
        ));
    }
    ret
}
/// Slot 26: GetCustomHeapProperties(ret, this, nodeMask, heapType) -> ret.
unsafe extern "system" fn devx_get_custom_heap_properties(ret: *mut c_void, this: *mut c_void, node: u32, ty: u32) -> *mut c_void {
    let (inner, f) = devx_inner_fn(this, 26);
    let f: unsafe extern "system" fn(*mut c_void, *mut c_void, u32, u32) -> *mut c_void = std::mem::transmute(f);
    f(inner, ret, node, ty);
    ret
}
/// Slot 43: GetAdapterLuid(this) -> LUID in RAX (8-byte struct, console C-style return).
unsafe extern "system" fn devx_get_adapter_luid(this: *mut c_void) -> u64 {
    let (inner, f) = devx_inner_fn(this, 43);
    let f: unsafe extern "system" fn(*mut c_void, *mut u64) -> *mut u64 = std::mem::transmute(f);
    let mut l = 0u64;
    f(inner, &mut l);
    l
}

// ---- Per-object vtable patching for real D3D12Core objects handed to the title ---------------
// (copy the object's vtable once per original vtable, override selected slots, repoint vptr).
static OBJ_VTBLS: std::sync::Mutex<Vec<(usize, usize)>> = std::sync::Mutex::new(Vec::new());
/// Original slot pointers per patched vtable: (patched_vtbl, slot) -> original fn.
static OBJ_ORIG: std::sync::Mutex<Vec<(usize, usize, usize)>> = std::sync::Mutex::new(Vec::new());

unsafe fn patch_object(obj: *mut c_void, n_slots: usize, overrides: &[(usize, usize)]) {
    if obj.is_null() {
        return;
    }
    let vp = obj as *mut usize;
    let orig = *vp;
    let mut v = OBJ_VTBLS.lock().unwrap();
    if v.iter().any(|&(_, n)| n == orig) {
        return;
    }
    let newv = match v.iter().find(|&&(o, _)| o == orig) {
        Some(&(_, n)) => n,
        None => {
            let count = n_slots.max(64);
            let mut tbl: Vec<usize> = std::slice::from_raw_parts(orig as *const usize, count).to_vec();
            let mut origs = OBJ_ORIG.lock().unwrap();
            for &(slot, f) in overrides {
                origs.push((0, slot, tbl[slot]));
                tbl[slot] = f;
            }
            let n = Box::leak(tbl.into_boxed_slice()).as_ptr() as usize;
            for e in origs.iter_mut().filter(|e| e.0 == 0) {
                e.0 = n;
            }
            v.push((orig, n));
            n
        }
    };
    *vp = newv;
}

unsafe fn orig_slot(obj: *mut c_void, slot: usize) -> usize {
    let vt = *(obj as *const usize);
    if let Some(e) = OBJ_ORIG.lock().unwrap().iter().find(|&&(t, s, _)| t == vt && s == slot) {
        return e.2;
    }
    // Not a vtable we patched: the object still has its original vtable.
    log_d3d12(&format!("[d3d12x] orig_slot miss slot={slot} obj={obj:p} vt={vt:#x}"));
    *((vt as *const usize).add(slot))
}

/// Shadow CPU memory for Map() of non-CPU-visible resources (UMA emulation): resource -> ptr.
/// (resource, ptr, bytes, desc): keyed by desc too, since freed resource addresses are reused.
static SHADOW_MAPS: std::sync::Mutex<Vec<(usize, usize, usize, [u64; 7])>> = std::sync::Mutex::new(Vec::new());

/// ID3D12Resource slot 8: Map(subresource, readRange, out).
unsafe extern "system" fn res_map(this: *mut c_void, sub: u32, range: *const c_void, out: *mut *mut c_void) -> i32 {
    let f: unsafe extern "system" fn(*mut c_void, u32, *const c_void, *mut *mut c_void) -> i32 = std::mem::transmute(orig_slot(this, 8));
    let hr = f(this, sub, range, out);
    if hr >= 0 && !out.is_null() && sub == 0 {
        mirror::on_map(this, *out);
    }
    if hr >= 0 && !out.is_null() {
        static N: AtomicU32 = AtomicU32::new(0);
        if N.fetch_add(1, Ordering::Relaxed) < 400 {
            let get_desc: unsafe extern "system" fn(*mut c_void, *mut [u64; 7]) -> *mut [u64; 7] = std::mem::transmute(orig_slot(this, 10));
            let mut d = [0u64; 7];
            get_desc(this, &mut d);
            log_d3d12(&format!("[d3d12x] Map real {:p} w={:#x} dim={}", *out, d[2], d[0] as u32));
        }
    }
    if hr >= 0 || out.is_null() {
        return hr;
    }
    // Size via the real GetDesc (PC ABI) + device GetResourceAllocationInfo would need the device;
    // a generous bound from the desc suffices for a write sink.
    // D3D12_RESOURCE_DESC (56 bytes): Width @16, Height @24, DepthOrArraySize @28.
    let get_desc: unsafe extern "system" fn(*mut c_void, *mut [u64; 7]) -> *mut [u64; 7] = std::mem::transmute(orig_slot(this, 10));
    let mut d = [0u64; 7];
    get_desc(this, &mut d);
    let width = d[2];
    let height = d[3] & 0xFFFF_FFFF;
    let depth = (d[3] >> 32) & 0xFFFF;
    let mut bytes = (width.max(1) * height.max(1) * depth.max(1) * 16 + 0xFFFF) as usize;
    // Titles fill whole allocations (GetResourceAllocationInfo size); cover at least that.
    {
        const IID_DEV: [u8; 16] = [0xf1, 0x19, 0x98, 0x18, 0xb6, 0x1d, 0x57, 0x4b, 0xbe, 0x54, 0x18, 0x21, 0x33, 0x9b, 0x85, 0xf7];
        let gd: unsafe extern "system" fn(*mut c_void, *const u8, *mut *mut c_void) -> i32 = std::mem::transmute(orig_slot(this, 7));
        let mut dev = std::ptr::null_mut();
        if gd(this, IID_DEV.as_ptr(), &mut dev) >= 0 && !dev.is_null() {
            let vt = *(dev as *const *const usize);
            let ai: unsafe extern "system" fn(*mut c_void, *mut [u64; 2], u32, u32, *const [u64; 7]) -> *mut [u64; 2] = std::mem::transmute(*vt.add(25));
            let mut info = [0u64; 2];
            ai(dev, &mut info, 0, 1, &d);
            if info[0] != u64::MAX && info[0] < (1 << 34) {
                bytes = bytes.max(info[0] as usize + 0xFFFF);
            }
            let rel: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(*vt.add(2));
            rel(dev);
        }
    }
    let mut sm = SHADOW_MAPS.lock().unwrap();
    if let Some(&(_, p, _, _)) = sm.iter().find(|e| e.0 == this as usize && e.3 == d && e.2 >= bytes) {
        *out = p as *mut c_void;
        return 0;
    }
    sm.retain(|e| e.0 != this as usize); // stale entry from a freed resource at this address (leaked)
    extern "system" {
        fn VirtualAlloc(a: *mut c_void, s: usize, t: u32, p: u32) -> *mut c_void;
    }
    let p = VirtualAlloc(std::ptr::null_mut(), bytes, 0x3000, 0x04);
    if p.is_null() {
        return hr;
    }
    log_d3d12(&format!("[d3d12x] Map shadow {p:p}+{bytes:#x} dim={} w={} h={} da={} mips={} fmt={} fl={:#x}", d[0] as u32, d[2], d[3] as u32, (d[3] >> 32) as u16, (d[3] >> 48) as u16, d[4] as u32, d[6] as u32));
    sm.push((this as usize, p as usize, bytes, d));
    *out = p;
    0
}
/// ID3D12Resource slot 9: Unmap — shadow maps are kept for the resource's lifetime.
unsafe extern "system" fn res_unmap(this: *mut c_void, sub: u32, range: *const c_void) {
    if SHADOW_MAPS.lock().unwrap().iter().any(|e| e.0 == this as usize) {
        return;
    }
    let f: unsafe extern "system" fn(*mut c_void, u32, *const c_void) = std::mem::transmute(orig_slot(this, 9));
    f(this, sub, range)
}
// Patched vtables are shared with D3D12Core's own internal calls (PC ABI), so struct-returning
// slots dispatch on the caller: a return address inside a loaded module (D3D12Core, system DLLs)
// means PC ABI (this, ret); one in manually mapped guest code means console ABI
// (large struct: ret, this; <= 8 bytes: this -> RAX). A naked trampoline passes the return address.
unsafe fn caller_is_guest(ra: usize) -> bool {
    extern "system" {
        fn GetModuleHandleExW(flags: u32, addr: usize, out: *mut usize) -> i32;
    }
    let mut h = 0usize;
    GetModuleHandleExW(0x6, ra, &mut h) == 0
}

macro_rules! ra_tramp {
    ($tramp:ident, $target:ident) => {
        #[unsafe(naked)]
        unsafe extern "system" fn $tramp() {
            std::arch::naked_asm!("mov r8, qword ptr [rsp]", "jmp {t}", t = sym $target);
        }
    };
}

/// Large-struct getter at `slot`: console (ret, this) or PC (this, ret); returns ret.
unsafe fn big_struct_get(slot: usize, a: *mut c_void, b: *mut c_void, ra: usize) -> *mut c_void {
    // Titles mix (ret, this) and (this, ret) call sites; the object is the arg whose vptr is ours.
    let ours = |p: *mut c_void| !p.is_null() && {
        let vt = *(p as *const usize);
        OBJ_VTBLS.lock().unwrap().iter().any(|&(_, n)| n == vt)
    };
    let (this, ret) = if ours(a) { (a, b) } else if ours(b) { (b, a) } else if caller_is_guest(ra) { (b, a) } else { (a, b) };
    let f: unsafe extern "system" fn(*mut c_void, *mut c_void) -> *mut c_void = std::mem::transmute(orig_slot(this, slot));
    f(this, ret);
    ret
}
/// 8-byte-struct getter at `slot`: console (this) -> RAX value, or PC (this, ret) -> ret.
unsafe fn small_struct_get(slot: usize, this: *mut c_void, b: *mut u64, ra: usize) -> u64 {
    let f: unsafe extern "system" fn(*mut c_void, *mut u64) -> *mut u64 = std::mem::transmute(orig_slot(this, slot));
    let mut handle = 0u64;
    f(this, &mut handle);
    log_d3d12(&format!("[d3d12x] small_struct_get(slot={slot}, this={this:p}, guest={}) -> {handle:#x}", caller_is_guest(ra)));
    if !caller_is_guest(ra) && !b.is_null() {
        *b = handle;
    }
    handle
}
unsafe extern "system" fn res_get_desc_d(a: *mut c_void, b: *mut c_void, ra: usize) -> *mut c_void { big_struct_get(10, a, b, ra) }
unsafe extern "system" fn heap_get_desc_d(a: *mut c_void, b: *mut c_void, ra: usize) -> *mut c_void { big_struct_get(8, a, b, ra) }
unsafe extern "system" fn heap_cpu_start_d(t: *mut c_void, b: *mut u64, ra: usize) -> u64 { small_struct_get(9, t, b, ra) }
unsafe extern "system" fn heap_gpu_start_d(t: *mut c_void, b: *mut u64, ra: usize) -> u64 { small_struct_get(10, t, b, ra) }
ra_tramp!(res_get_desc, res_get_desc_d);
ra_tramp!(heap_get_desc, heap_get_desc_d);
ra_tramp!(heap_cpu_start, heap_cpu_start_d);
ra_tramp!(heap_gpu_start, heap_gpu_start_d);

/// ID3D12Resource slot 11: GetGPUVirtualAddress — a mirrored buffer answers with its VRAM twin.
unsafe extern "system" fn res_gpu_va(this: *mut c_void) -> u64 {
    if let Some(va) = mirror::gpu_va(this) {
        return va;
    }
    let f: unsafe extern "system" fn(*mut c_void) -> u64 = std::mem::transmute(orig_slot(this, 11));
    f(this)
}
/// Device slots 18/19: Create{Shader Resource,Unordered Access}View — views of mirrored buffers
/// read the twin.
unsafe extern "system" fn devx_create_srv(this: *mut c_void, res: *mut c_void, desc: *const c_void, h: usize) {
    if h == 0 {
        log_d3d12("[d3d12x] CreateShaderResourceView called with NULL dest handle! Ignoring.");
        return;
    }
    let (inner, f) = devx_inner_fn(this, 18);
    let f: unsafe extern "system" fn(*mut c_void, *mut c_void, *const c_void, usize) = std::mem::transmute(f);
    f(inner, mirror::sub(res), desc, h)
}
unsafe extern "system" fn devx_create_uav(this: *mut c_void, res: *mut c_void, ctr: *mut c_void, desc: *const c_void, h: usize) {
    if h == 0 {
        log_d3d12("[d3d12x] CreateUnorderedAccessView called with NULL dest handle! Ignoring.");
        return;
    }
    let real_res = mirror::sub(res);
    let mut uav_buf_desc = [0u32; 8];
    let actual_desc = if desc.is_null() && !real_res.is_null() {
        uav_buf_desc[0] = 39; // DXGI_FORMAT_R32_TYPELESS
        uav_buf_desc[1] = 1;  // D3D12_UAV_DIMENSION_BUFFER
        uav_buf_desc[4] = 0x1000; // num_elements
        uav_buf_desc[7] = 1;  // D3D12_BUFFER_UAV_FLAG_RAW
        uav_buf_desc.as_ptr() as *const c_void
    } else {
        desc
    };
    let (inner, f) = devx_inner_fn(this, 19);
    let f: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void, *const c_void, usize) = std::mem::transmute(f);
    f(inner, real_res, mirror::sub(ctr), actual_desc, h)
}

unsafe fn patch_resource(r: *mut c_void) {
    patch_object(r, 16, &[(8, res_map as usize), (9, res_unmap as usize), (10, res_get_desc as usize), (11, res_gpu_va as usize)]);
}

/// Slot 14: CreateDescriptorHeap(desc, riid, out) — patch struct-returning heap methods.
unsafe extern "system" fn devx_create_descriptor_heap(this: *mut c_void, desc: *const c_void, riid: *const c_void, out: *mut *mut c_void) -> i32 {
    let (inner, f) = devx_inner_fn(this, 14);
    let f: unsafe extern "system" fn(*mut c_void, *const c_void, *const c_void, *mut *mut c_void) -> i32 = std::mem::transmute(f);
    let mut clamped_desc = [0u32; 4];
    let actual_desc = if !desc.is_null() {
        let d = desc as *const u32;
        let heap_type = *d;
        let count = *d.add(1);
        let flags = *d.add(2);
        let node = *d.add(3);
        // PC D3D12 limits shader-visible CBV_SRV_UAV heaps to 1,000,000 descriptors.
        // Scarlett consoles allow larger allocations, which triggers E_INVALIDARG on PC drivers.
        if heap_type == 0 && (flags & 1) != 0 && count > 1_000_000 {
            log_d3d12(&format!("[d3d12x] Clamping shader-visible CBV_SRV_UAV descriptor count from {count} to 1000000"));
            clamped_desc = [heap_type, 1_000_000, flags, node];
            clamped_desc.as_ptr() as *const c_void
        } else {
            desc
        }
    } else {
        desc
    };
    let hr = f(inner, actual_desc, riid, out);
    if !desc.is_null() {
        let d = actual_desc as *const u32;
        log_d3d12(&format!("[d3d12x] CreateDescriptorHeap type={} n={} flags={:#x} node={} -> {:08x}", *d, *d.add(1), *d.add(2), *d.add(3), hr as u32));
    }
    if hr >= 0 && !out.is_null() {
        patch_object(*out, 12, &[(8, heap_get_desc as usize), (9, heap_cpu_start as usize), (10, heap_gpu_start as usize)]);
    }
    hr
}

/// Slot 27: CreateCommittedResource(heapProps, flags, desc, state, clear, riid, out).
/// Scarlett memory is unified: titles Map() DEFAULT-heap buffers directly. On a PC dGPU that is
/// E_INVALIDARG, so DEFAULT-heap *buffers* are placed on a CPU-visible CUSTOM heap
/// (WRITE_COMBINE, L0) instead. Textures keep their heap (console tiling is handled elsewhere).
unsafe extern "system" fn devx_create_committed_resource(
    this: *mut c_void,
    props: *const u32,
    flags: u32,
    desc: *const u32,
    state: u32,
    clear: *const c_void,
    riid: *const c_void,
    out: *mut *mut c_void,
) -> i32 {
    let (inner, f) = devx_inner_fn(this, 27);
    let f: unsafe extern "system" fn(*mut c_void, *const u32, u32, *const u32, u32, *const c_void, *const c_void, *mut *mut c_void) -> i32 =
        std::mem::transmute(f);
    const HEAP_DEFAULT: u32 = 1;
    const DIM_BUFFER: u32 = 1;
    let mut pc_desc = sanitize_resource_desc(desc as *const u8);
    if !desc.is_null() && pc_desc[0] as u32 == DIM_BUFFER {
        // Titles fill buffers up to GetResourceAllocationInfo size (64 KiB granular); a committed
        // buffer only maps Width bytes, so allocate the rounded size.
        pc_desc[2] = (pc_desc[2] + 0xFFFF) & !0xFFFF;
    }
    let desc: *const u32 = if desc.is_null() { desc } else { pc_desc.as_ptr() as *const u32 };
    if !props.is_null() && !desc.is_null() && *props == HEAP_DEFAULT && *desc == DIM_BUFFER {
        // {Type=CUSTOM, CPUPageProperty=WRITE_COMBINE, MemoryPoolPreference=L0, CreationNodeMask, VisibleNodeMask}
        let uma = [4u32, 2, 1, *props.add(3), *props.add(4)];
        let fl = flags & !0x8; // ALLOW_DISPLAY is texture-only
        let hr = f(inner, uma.as_ptr(), fl, desc, state, clear, riid, out);
        log_d3d12(&format!("[d3d12x] CommittedResource DEFAULT buffer -> UMA heap w={} flags={fl:#x} state={state:#x} hr={:08x}", pc_desc[2], hr as u32));
        if hr >= 0 {
            if mirror::enabled() {
                let def = [1u32, 0, 0, *props.add(3), *props.add(4)];
                let mut twin = std::ptr::null_mut();
                if f(inner, def.as_ptr(), fl, desc, 0, std::ptr::null(), riid, &mut twin) >= 0 && !twin.is_null() {
                    mirror::add(*out, twin, pc_desc[2]);
                }
            }
            patch_resource(*out);
            replay::note_buffer(*out, pc_desc[2], 1);
            return hr;
        }
        let hr2 = f(inner, uma.as_ptr(), fl, desc, 0xAC3 /* GENERIC_READ */, clear, riid, out);
        if hr2 >= 0 {
            replay::note_buffer(*out, pc_desc[2], 1);
            return hr2;
        }
        log_d3d12(&format!("[d3d12x] UMA buffer fallback failed {:08x}/{:08x}; using DEFAULT", hr as u32, hr2 as u32));
    }
    let hr = f(inner, props, flags, desc, state, clear, riid, out);
    if hr >= 0 && !out.is_null() {
        patch_resource(*out);
        if pc_desc[0] as u32 == DIM_BUFFER {
            replay::note_buffer(*out, pc_desc[2], if !props.is_null() && *props == 2 { 2 } else { 0 });
        }
    }
    log_d3d12(&format!(
        "[d3d12x] CommittedResource heap={} dim={} flags={flags:#x} -> {:08x}",
        if props.is_null() { 99 } else { *props },
        if desc.is_null() { 99 } else { *desc },
        hr as u32
    ));
    hr
}

/// Slot 29: CreatePlacedResource(heap, offset, desc, state, clear, riid, out).
unsafe extern "system" fn devx_create_placed_resource(
    this: *mut c_void,
    heap: *mut c_void,
    offset: u64,
    desc: *const u8,
    state: u32,
    clear: *const c_void,
    riid: *const c_void,
    out: *mut *mut c_void,
) -> i32 {
    let (inner, f) = devx_inner_fn(this, 29);
    let f: unsafe extern "system" fn(*mut c_void, *mut c_void, u64, *const u8, u32, *const c_void, *const c_void, *mut *mut c_void) -> i32 =
        std::mem::transmute(f);
    if !desc.is_null() && std::env::var("PERCY_TRACE").is_ok() {
        let rd = |o: usize| std::ptr::read_unaligned(desc.add(o) as *const u32);
        let rq = |o: usize| std::ptr::read_unaligned(desc.add(o) as *const u64);
        // heap GetDesc (PC ABI, slot 8): {SizeInBytes, Properties[5], Alignment, Flags}
        let hv = *(heap as *const *const usize);
        let hgd: unsafe extern "system" fn(*mut c_void, *mut [u64; 6]) -> *mut [u64; 6] = std::mem::transmute(*hv.add(8));
        let mut hd = [0u64; 6];
        hgd(heap, &mut hd);
        log_d3d12(&format!(
            "[d3d12x] PlacedResource dim={} align={:#x} w={} h={} d={} mips={} fmt={} samples={} layout={} flags={:#x} off={offset:#x} state={state:#x} heap(size={:#x} type={} flags={:#x})",
            rd(0), rq(8), rq(16), rd(24), rd(28) & 0xFFFF, rd(28) >> 16, rd(32), rd(36), rd(44), rd(48),
            hd[0], hd[1] & 0xFFFF_FFFF, hd[5] & 0xFFFF_FFFF
        ));
    }
    let pc_desc = sanitize_resource_desc(desc);
    let hr = f(inner, heap, offset, pc_desc.as_ptr() as *const u8, state, clear, riid, out);
    if hr >= 0 && !out.is_null() {
        patch_resource(*out);
        if pc_desc[0] as u32 == 1 {
            replay::note_buffer(*out, pc_desc[2], 3);
        }
    }
    hr
}

/// PC-defined D3D12_RESOURCE_FLAGS bits (RT .. RAYTRACING_ACCELERATION_STRUCTURE).
const PC_RESOURCE_FLAGS: u32 = 0x1FF;
/// Copies a D3D12_RESOURCE_DESC (56 bytes) and strips console-only resource flags (e.g. 0x20000),
/// which PC drivers reject or mis-handle.
unsafe fn sanitize_resource_desc(desc: *const u8) -> [u64; 7] {
    let mut d = [0u64; 7];
    if desc.is_null() {
        return d;
    }
    std::ptr::copy_nonoverlapping(desc, d.as_mut_ptr() as *mut u8, 56);
    let flags = (d.as_mut_ptr() as *mut u8).add(48) as *mut u32;
    if *flags & !PC_RESOURCE_FLAGS != 0 {
        log_d3d12(&format!("[d3d12x] stripped console resource flags {:#x}", *flags & !PC_RESOURCE_FLAGS));
        *flags &= PC_RESOURCE_FLAGS;
    }
    d
}

/// Slots 10/11 (Create{Graphics,Compute}PipelineState) and 16 (CreateRootSignature): logged
/// pass-through. First 4 bytes of each shader blob tell DXIL ('DXBC' container) from console ISA.
unsafe fn shader_kind(p: *const u8, len: usize) -> &'static str {
    if p.is_null() || len == 0 {
        "none"
    } else if len >= 4 && std::slice::from_raw_parts(p, 4) == b"DXBC" {
        "DXBC/DXIL"
    } else {
        "console-ISA"
    }
}
static PSO_LOGGED: AtomicU32 = AtomicU32::new(0);
unsafe extern "system" fn devx_create_graphics_pso(this: *mut c_void, desc: *const u8, riid: *const c_void, out: *mut *mut c_void) -> i32 {
    let (inner, f) = devx_inner_fn(this, 10);
    let f: unsafe extern "system" fn(*mut c_void, *const u8, *const c_void, *mut *mut c_void) -> i32 = std::mem::transmute(f);
    let hr = f(inner, desc, riid, out);
    if hr >= 0 && !out.is_null() {
        *out = replay::wrap_pso(*out);
    }
    if PSO_LOGGED.fetch_add(1, Ordering::Relaxed) < 6 && !desc.is_null() {
        // D3D12_GRAPHICS_PIPELINE_STATE_DESC: pRootSignature @0, VS {ptr,len} @8, PS @24
        let vs = (*(desc.add(8) as *const *const u8), *(desc.add(16) as *const usize));
        let ps = (*(desc.add(24) as *const *const u8), *(desc.add(32) as *const usize));
        log_d3d12(&format!(
            "[d3d12x] CreateGraphicsPSO vs={}({:#x}) ps={}({:#x}) -> {:08x}",
            shader_kind(vs.0, vs.1), vs.1, shader_kind(ps.0, ps.1), ps.1, hr as u32
        ));
    }
    hr
}
unsafe extern "system" fn devx_create_compute_pso(this: *mut c_void, desc: *const u8, riid: *const c_void, out: *mut *mut c_void) -> i32 {
    let (inner, f) = devx_inner_fn(this, 11);
    let f: unsafe extern "system" fn(*mut c_void, *const u8, *const c_void, *mut *mut c_void) -> i32 = std::mem::transmute(f);
    let hr = f(inner, desc, riid, out);
    if hr >= 0 && !out.is_null() {
        *out = replay::wrap_pso(*out);
    }
    if PSO_LOGGED.fetch_add(1, Ordering::Relaxed) < 6 && !desc.is_null() {
        let cs = (*(desc.add(8) as *const *const u8), *(desc.add(16) as *const usize));
        log_d3d12(&format!("[d3d12x] CreateComputePSO cs={}({:#x}) -> {:08x}", shader_kind(cs.0, cs.1), cs.1, hr as u32));
    }
    hr
}
static RS_LOGGED: AtomicU32 = AtomicU32::new(0);
unsafe extern "system" fn devx_create_root_signature(this: *mut c_void, node: u32, blob: *const u8, len: usize, riid: *const c_void, out: *mut *mut c_void) -> i32 {
    let (inner, f) = devx_inner_fn(this, 16);
    let f: unsafe extern "system" fn(*mut c_void, u32, *const u8, usize, *const c_void, *mut *mut c_void) -> i32 = std::mem::transmute(f);
    let hr = f(inner, node, blob, len, riid, out);
    if hr >= 0 && !out.is_null() {
        replay::register_rs(*out, blob, len);
    }
    if RS_LOGGED.fetch_add(1, Ordering::Relaxed) < 4 {
        log_d3d12(&format!("[d3d12x] CreateRootSignature blob={}({len:#x}) -> {:08x}", shader_kind(blob, len), hr as u32));
    }
    hr
}

// ---- Command-list call census (PERCY_TRACE): per-slot counting thunks ---------------------------
const CL_SLOTS: usize = 80;
#[no_mangle]
static mut PERCY_CL_COUNTS: [u64; CL_SLOTS] = [0; CL_SLOTS];
#[no_mangle]
static mut PERCY_CL_ORIG: [usize; CL_SLOTS] = [0; CL_SLOTS];

// Generated: per-slot thunk = count, then tail-jump to the original method.
#[unsafe(naked)]
unsafe extern "system" fn cl_t0() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 0]", "jmp qword ptr [rip + {o} + 0]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t1() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 8]", "jmp qword ptr [rip + {o} + 8]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t2() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 16]", "jmp qword ptr [rip + {o} + 16]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t3() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 24]", "jmp qword ptr [rip + {o} + 24]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t4() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 32]", "jmp qword ptr [rip + {o} + 32]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t5() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 40]", "jmp qword ptr [rip + {o} + 40]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t6() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 48]", "jmp qword ptr [rip + {o} + 48]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t7() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 56]", "jmp qword ptr [rip + {o} + 56]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t8() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 64]", "jmp qword ptr [rip + {o} + 64]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t9() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 72]", "jmp qword ptr [rip + {o} + 72]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t10() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 80]", "jmp qword ptr [rip + {o} + 80]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t11() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 88]", "jmp qword ptr [rip + {o} + 88]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t12() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 96]", "jmp qword ptr [rip + {o} + 96]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t13() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 104]", "jmp qword ptr [rip + {o} + 104]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t14() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 112]", "jmp qword ptr [rip + {o} + 112]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t15() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 120]", "jmp qword ptr [rip + {o} + 120]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t16() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 128]", "jmp qword ptr [rip + {o} + 128]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t17() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 136]", "jmp qword ptr [rip + {o} + 136]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t18() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 144]", "jmp qword ptr [rip + {o} + 144]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t19() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 152]", "jmp qword ptr [rip + {o} + 152]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t20() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 160]", "jmp qword ptr [rip + {o} + 160]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t21() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 168]", "jmp qword ptr [rip + {o} + 168]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t22() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 176]", "jmp qword ptr [rip + {o} + 176]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t23() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 184]", "jmp qword ptr [rip + {o} + 184]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t24() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 192]", "jmp qword ptr [rip + {o} + 192]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t25() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 200]", "jmp qword ptr [rip + {o} + 200]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t26() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 208]", "jmp qword ptr [rip + {o} + 208]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t27() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 216]", "jmp qword ptr [rip + {o} + 216]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t28() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 224]", "jmp qword ptr [rip + {o} + 224]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t29() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 232]", "jmp qword ptr [rip + {o} + 232]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t30() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 240]", "jmp qword ptr [rip + {o} + 240]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t31() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 248]", "jmp qword ptr [rip + {o} + 248]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t32() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 256]", "jmp qword ptr [rip + {o} + 256]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t33() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 264]", "jmp qword ptr [rip + {o} + 264]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t34() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 272]", "jmp qword ptr [rip + {o} + 272]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t35() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 280]", "jmp qword ptr [rip + {o} + 280]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t36() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 288]", "jmp qword ptr [rip + {o} + 288]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t37() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 296]", "jmp qword ptr [rip + {o} + 296]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t38() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 304]", "jmp qword ptr [rip + {o} + 304]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t39() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 312]", "jmp qword ptr [rip + {o} + 312]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t40() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 320]", "jmp qword ptr [rip + {o} + 320]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t41() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 328]", "jmp qword ptr [rip + {o} + 328]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t42() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 336]", "jmp qword ptr [rip + {o} + 336]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t43() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 344]", "jmp qword ptr [rip + {o} + 344]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t44() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 352]", "jmp qword ptr [rip + {o} + 352]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t45() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 360]", "jmp qword ptr [rip + {o} + 360]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t46() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 368]", "jmp qword ptr [rip + {o} + 368]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t47() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 376]", "jmp qword ptr [rip + {o} + 376]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t48() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 384]", "jmp qword ptr [rip + {o} + 384]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t49() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 392]", "jmp qword ptr [rip + {o} + 392]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t50() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 400]", "jmp qword ptr [rip + {o} + 400]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t51() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 408]", "jmp qword ptr [rip + {o} + 408]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t52() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 416]", "jmp qword ptr [rip + {o} + 416]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t53() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 424]", "jmp qword ptr [rip + {o} + 424]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t54() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 432]", "jmp qword ptr [rip + {o} + 432]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t55() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 440]", "jmp qword ptr [rip + {o} + 440]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t56() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 448]", "jmp qword ptr [rip + {o} + 448]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t57() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 456]", "jmp qword ptr [rip + {o} + 456]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t58() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 464]", "jmp qword ptr [rip + {o} + 464]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t59() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 472]", "jmp qword ptr [rip + {o} + 472]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t60() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 480]", "jmp qword ptr [rip + {o} + 480]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t61() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 488]", "jmp qword ptr [rip + {o} + 488]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t62() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 496]", "jmp qword ptr [rip + {o} + 496]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t63() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 504]", "jmp qword ptr [rip + {o} + 504]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t64() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 512]", "jmp qword ptr [rip + {o} + 512]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t65() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 520]", "jmp qword ptr [rip + {o} + 520]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t66() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 528]", "jmp qword ptr [rip + {o} + 528]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t67() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 536]", "jmp qword ptr [rip + {o} + 536]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t68() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 544]", "jmp qword ptr [rip + {o} + 544]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t69() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 552]", "jmp qword ptr [rip + {o} + 552]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t70() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 560]", "jmp qword ptr [rip + {o} + 560]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t71() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 568]", "jmp qword ptr [rip + {o} + 568]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t72() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 576]", "jmp qword ptr [rip + {o} + 576]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t73() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 584]", "jmp qword ptr [rip + {o} + 584]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t74() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 592]", "jmp qword ptr [rip + {o} + 592]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t75() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 600]", "jmp qword ptr [rip + {o} + 600]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t76() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 608]", "jmp qword ptr [rip + {o} + 608]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t77() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 616]", "jmp qword ptr [rip + {o} + 616]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t78() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 624]", "jmp qword ptr [rip + {o} + 624]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
#[unsafe(naked)]
unsafe extern "system" fn cl_t79() {
    std::arch::naked_asm!("lock inc qword ptr [rip + {c} + 632]", "jmp qword ptr [rip + {o} + 632]", c = sym PERCY_CL_COUNTS, o = sym PERCY_CL_ORIG);
}
const CL_THUNKS: [unsafe extern "system" fn(); CL_SLOTS] = [cl_t0, cl_t1, cl_t2, cl_t3, cl_t4, cl_t5, cl_t6, cl_t7, cl_t8, cl_t9, cl_t10, cl_t11, cl_t12, cl_t13, cl_t14, cl_t15, cl_t16, cl_t17, cl_t18, cl_t19, cl_t20, cl_t21, cl_t22, cl_t23, cl_t24, cl_t25, cl_t26, cl_t27, cl_t28, cl_t29, cl_t30, cl_t31, cl_t32, cl_t33, cl_t34, cl_t35, cl_t36, cl_t37, cl_t38, cl_t39, cl_t40, cl_t41, cl_t42, cl_t43, cl_t44, cl_t45, cl_t46, cl_t47, cl_t48, cl_t49, cl_t50, cl_t51, cl_t52, cl_t53, cl_t54, cl_t55, cl_t56, cl_t57, cl_t58, cl_t59, cl_t60, cl_t61, cl_t62, cl_t63, cl_t64, cl_t65, cl_t66, cl_t67, cl_t68, cl_t69, cl_t70, cl_t71, cl_t72, cl_t73, cl_t74, cl_t75, cl_t76, cl_t77, cl_t78, cl_t79];

/// ID3D12GraphicsCommandList slot 10: Reset(allocator, initialPso) — logged.
unsafe extern "system" fn cl_reset_logged(this: *mut c_void, alloc: *mut c_void, pso: *mut c_void) -> i32 {
    std::ptr::write_volatile(std::ptr::addr_of_mut!(PERCY_CL_COUNTS[10]), PERCY_CL_COUNTS[10] + 1);
    let f: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void) -> i32 = std::mem::transmute(PERCY_CL_ORIG[10]);
    let vt = |p: *mut c_void| if p.is_null() { 0 } else { *(p as *const usize) };
    let hr = f(this, alloc, pso);
    log_d3d12(&format!("[d3d12x] CL Reset alloc={alloc:p}(vt {:#x}) pso={pso:p}(vt {:#x}) -> {:08x}", vt(alloc), vt(pso), hr as u32));
    hr
}

unsafe fn patch_command_list(cl: *mut c_void) {
    if cl.is_null() || std::env::var("PERCY_TRACE").is_err() {
        return;
    }
    let vt = *(cl as *const *const usize);
    if PERCY_CL_ORIG[0] == 0 {
        for i in 0..CL_SLOTS {
            PERCY_CL_ORIG[i] = *vt.add(i);
        }
    }
    let mut over: Vec<(usize, usize)> = (3..CL_SLOTS).map(|i| (i, CL_THUNKS[i] as usize)).collect();
    over[10 - 3].1 = cl_reset_logged as usize;
    patch_object(cl, CL_SLOTS, &over);
}

/// ID3D12CommandQueue slot 10: ExecuteCommandLists(n, lists) — dump the census, then submit.
unsafe extern "system" fn queue_execute(this: *mut c_void, n: u32, lists: *const *mut c_void) {
    static DUMPS: AtomicU32 = AtomicU32::new(0);
    if DUMPS.fetch_add(1, Ordering::Relaxed) < 3 {
        let mut s = String::new();
        for i in 0..CL_SLOTS {
            let c = std::ptr::read_volatile(std::ptr::addr_of!(PERCY_CL_COUNTS[i]));
            if c > 0 {
                s.push_str(&format!(" {i}:{c}"));
            }
        }
        let l0 = if n > 0 { *lists } else { std::ptr::null_mut() };
        let ours = console_list::inner_of(l0).is_some();
        let q: Vec<String> = if l0.is_null() { vec![] } else { (0..8).map(|i| format!("{:x}", *(l0 as *const usize).add(i))).collect() };
        log_d3d12(&format!("[d3d12x] ExecuteCommandLists(n={n}) l0={l0:p} ours={ours} [{}] census{s} clx{} {}", q.join(" "), console_list::census(), replay::stats()));
    }
    static DR: AtomicU32 = AtomicU32::new(0);
    let k = DR.fetch_add(1, Ordering::Relaxed);
    if k.is_power_of_two() {
        log_d3d12(&format!("[d3d12x] submits={k} binds def/uma/upl/placed/untracked={:?}", replay::BIND_KIND.iter().map(|c| c.load(Ordering::Relaxed)).collect::<Vec<_>>()));
    }
    if k < 50 {
        drain_info("execute", 6);
    }
    let mut pc: Vec<*mut c_void> = (0..n as usize)
        .filter_map(|i| { let l = *lists.add(i); match console_list::inner_of(l) { Some(p) => { console_list::ensure_closed(l); if console_list::failed(l) { None } else { Some(p) } } None => Some(l) } })
        .collect();
    if !pc.is_empty() {
        mirror::flush(this);
        let f: unsafe extern "system" fn(*mut c_void, u32, *const *mut c_void) = std::mem::transmute(console_list::queue_orig(this, 10));
        f(this, pc.len() as u32, pc.as_mut_ptr());
    }
    console_list::signal_fences(this, n, lists);
    if pc.is_empty() { return; }
    static CHK: AtomicU32 = AtomicU32::new(0);
    if CHK.fetch_add(1, Ordering::Relaxed) < 4 {
        const IID_DEV: [u8; 16] = [0xf1, 0x19, 0x98, 0x18, 0xb6, 0x1d, 0x57, 0x4b, 0xbe, 0x54, 0x18, 0x21, 0x33, 0x9b, 0x85, 0xf7];
        let gd: unsafe extern "system" fn(*mut c_void, *const u8, *mut *mut c_void) -> i32 = std::mem::transmute(console_list::queue_orig(this, 7));
        let mut dev = std::ptr::null_mut();
        if gd(this, IID_DEV.as_ptr(), &mut dev) >= 0 && !dev.is_null() {
            let vt = *(dev as *const *const usize);
            let rr: unsafe extern "system" fn(*mut c_void) -> i32 = std::mem::transmute(*vt.add(37));
            let rel: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(*vt.add(2));
            log_d3d12(&format!("[d3d12x] after Execute: removed reason {:08x}", rr(dev) as u32));
            rel(dev);
        }
    }
}

/// Console queue slot 30 (frame present): argument probe.
unsafe extern "system" fn queue_present_x(q: *mut c_void, a: usize, b: usize, c: usize, d: usize, e: usize) -> i32 {
    static N: AtomicU32 = AtomicU32::new(0);
    let k = N.fetch_add(1, Ordering::Relaxed);
    if k < 6 || k.is_power_of_two() {
        let dump = |p: usize| -> String {
            if p < 0x10000 || p >> 47 != 0 { return String::new(); }
            let w = std::slice::from_raw_parts(p as *const u64, 6);
            format!(" *{p:#x}=[{:x} {:x} {:x} {:x} {:x} {:x}]", w[0], w[1], w[2], w[3], w[4], w[5])
        };
        log_d3d12(&format!("[qx] present#{k} q={q:p} ({a:#x},{b:#x},{c:#x},{d:#x},{e:#x}){}{}{}", dump(a), dump(b), dump(c)));
    }
    present::present_x(q, a as u32, b as *const u8)
}

/// Slot 8: CreateCommandQueue(desc, riid, out) — patch ExecuteCommandLists for the census.
unsafe extern "system" fn devx_create_command_queue(this: *mut c_void, desc: *const c_void, riid: *const c_void, out: *mut *mut c_void) -> i32 {
    let (inner, f) = devx_inner_fn(this, 8);
    let f: unsafe extern "system" fn(*mut c_void, *const c_void, *const c_void, *mut *mut c_void) -> i32 = std::mem::transmute(f);
    let hr = f(inner, desc, riid, out);
    if !desc.is_null() { let d = desc as *const u32; log_d3d12(&format!("[d3d12x] CreateCommandQueue desc={:x} {:x} {:x} {:x} -> {:08x}", *d, *d.add(1), *d.add(2), *d.add(3), hr as u32)); }
    if hr >= 0 && !out.is_null() {
        console_list::patch_queue(*out, &[(10, queue_execute as usize), (30, queue_present_x as usize)]);
    }
    hr
}

/// ID3D12DeviceX slot 73: CreateCommandList(desc, allocator, initialPso, riid, out).
/// desc (observed 16 bytes): { Type: u32, NodeMask: u32, .. }. Forwards to ID3D12Device::CreateCommandList (slot 12).
unsafe extern "system" fn devx_create_command_list(
    this: *mut c_void,
    desc: *const u32,
    allocator: *mut c_void,
    pso: *mut c_void,
    riid: *const c_void,
    out: *mut *mut c_void,
) -> i32 {
    const IID_CMD_LIST: [u8; 16] = [
        0x0f, 0x0d, 0x16, 0x5b, 0x1b, 0xac, 0x85, 0x41, 0x8b, 0xa8, 0xb3, 0xae, 0x42, 0xa5, 0xa4, 0x55,
    ];
    let inner = (*(this as *const DeviceXProxy)).inner;
    let vt = *(inner as *const *const *const c_void);
    let f: unsafe extern "system" fn(*mut c_void, u32, u32, *mut c_void, *mut c_void, *const c_void, *mut *mut c_void) -> i32 =
        std::mem::transmute(*vt.add(12));
    let (ty, node) = if desc.is_null() { (0, 0) } else { (*desc, *desc.add(1)) };
    let unwrapped_pso = replay::unwrap_pso(pso);
    let target_riid = if riid.is_null() { IID_CMD_LIST.as_ptr() as *const c_void } else { riid };
    let mut hr = f(inner, 0, ty, allocator, unwrapped_pso, target_riid, out);
    if hr < 0 {
        hr = f(inner, 0, ty, allocator, unwrapped_pso, IID_CMD_LIST.as_ptr() as *const c_void, out);
    }
    if hr < 0 && node != 0 {
        hr = f(inner, node, ty, allocator, unwrapped_pso, IID_CMD_LIST.as_ptr() as *const c_void, out);
    }
    if hr >= 0 && !out.is_null() {
        if std::env::var("PERCY_CLX_OFF").is_ok() { patch_command_list(*out); } else { *out = console_list::wrap(*out); }
    }
    let rsp: usize;
    std::arch::asm!("mov {}, rsp", out(reg) rsp);
    let ras: Vec<String> = (0..400).map(|i| *((rsp as *const usize).add(i))).filter(|&v| (0x1_4000_1000..0x1_43d0_0000).contains(&v)).take(8).map(|v| format!("{:x}", v - 0x1_4000_0000)).collect();
    log_d3d12(&format!("[d3d12x] CreateCommandList(type={ty}, node={node}) G+[{}] -> {:08x}", ras.join(" "), hr as u32));
    hr
}
unsafe fn wrap_device_pointer(inner: *mut c_void) -> *mut c_void {
    if inner.is_null() {
        return inner;
    }
    let mut vtbl = [xbox_slot_unimpl as *const c_void; 256];
    vtbl[0] = proxy_qi as *const c_void;
    vtbl[1] = proxy_add_ref as *const c_void;
    vtbl[2] = proxy_release as *const c_void;
    #[cfg(all(windows, target_arch = "x86_64"))]
    {
        vtbl[3..44].copy_from_slice(&dev_fwd::SLOTS);
    }
    vtbl[44..128].copy_from_slice(&xbox_slots::SLOTS);
    vtbl[73] = devx_create_command_list as *const c_void;
    vtbl[25] = devx_get_resource_allocation_info as *const c_void;
    vtbl[26] = devx_get_custom_heap_properties as *const c_void;
    vtbl[43] = devx_get_adapter_luid as *const c_void;
    vtbl[27] = devx_create_committed_resource as *const c_void;
    vtbl[14] = devx_create_descriptor_heap as *const c_void;
    vtbl[29] = devx_create_placed_resource as *const c_void;
    vtbl[10] = devx_create_graphics_pso as *const c_void;
    vtbl[11] = devx_create_compute_pso as *const c_void;
    vtbl[16] = devx_create_root_signature as *const c_void;
    vtbl[18] = devx_create_srv as *const c_void;
    vtbl[19] = devx_create_uav as *const c_void;
    vtbl[8] = devx_create_command_queue as *const c_void;
    let leaked_vtbl = Box::leak(Box::new(vtbl));
    let proxy = Box::new(DeviceXProxy {
        vtbl: leaked_vtbl as *const [*const c_void; 256],
        inner,
        refs: AtomicU32::new(1),
    });
    Box::into_raw(proxy) as *mut c_void
}

#[cfg(windows)]
#[repr(C)]
struct DxgiDeviceProxy {
    vtbl: *const [*const c_void; 18],
    refs: AtomicU32,
}

/// IDXGIDevice::GetGPUThreadPriority (slot 11).
unsafe extern "system" fn dxgi_get_i32_zero(_this: *mut c_void, out: *mut i32) -> i32 {
    if out.is_null() {
        return 0x8000_4003u32 as i32;
    }
    *out = 0;
    0
}
/// IDXGIDevice1::GetMaximumFrameLatency (slot 13).
unsafe extern "system" fn dxgi_get_frame_latency(_this: *mut c_void, out: *mut u32) -> i32 {
    if out.is_null() {
        return 0x8000_4003u32 as i32;
    }
    *out = 3;
    0
}
/// IDXGIDevice3::Trim (slot 17).
unsafe extern "system" fn dxgi_trim(_this: *mut c_void) {}

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
    // IDXGIObject 0-6, IDXGIDevice 7-11, IDXGIDevice1 12-13, IDXGIDevice2 14-16, IDXGIDevice3 17.
    let mut vtbl = [xbox_slot_unimpl as *const c_void; 18];
    vtbl[0] = dxgi_qi as *const c_void;
    vtbl[1] = dxgi_add_ref as *const c_void;
    vtbl[2] = dxgi_release as *const c_void;
    vtbl[7] = dxgi_get_adapter as *const c_void;
    vtbl[11] = dxgi_get_i32_zero as *const c_void;
    vtbl[13] = dxgi_get_frame_latency as *const c_void;
    vtbl[17] = dxgi_trim as *const c_void;
    let leaked_vtbl = Box::leak(Box::new(vtbl));
    Box::into_raw(Box::new(DxgiDeviceProxy {
        vtbl: leaked_vtbl as *const [*const c_void; 18],
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

/// Exported data symbol `IID_ID3D12Device` (titles import the GUID by address rather than linking `dxguid`).
#[repr(C)]
pub struct Guid(pub u32, pub u16, pub u16, pub [u8; 8]);
#[no_mangle]
#[allow(non_upper_case_globals)]
pub static IID_ID3D12Device: Guid = Guid(0x189819f1, 0x1db6, 0x4b57, [0xbe, 0x54, 0x18, 0x21, 0x33, 0x9b, 0x85, 0xf7]);

#[no_mangle]
pub unsafe extern "system" fn D3D12ConfigureVirtualMemoryX(
    _device: *mut c_void,
    _config: *const c_void,
) -> i32 {
    0 // S_OK
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
    // `percy_d3d12_debug` next to the host: enable the D3D12 debug layer (messages via ODS).
    if std::env::current_exe().map(|p| p.with_file_name("percy_d3d12_debug").exists()).unwrap_or(false) {
        const IID_DEBUG: [u8; 16] = [0xb7, 0x88, 0x44, 0x34, 0x46, 0x68, 0x4b, 0x47, 0xb9, 0x89, 0xf0, 0x27, 0x44, 0x82, 0x45, 0xe0];
        let gdi = host_proc("D3D12GetDebugInterface");
        if gdi != 0 {
            let gdi: unsafe extern "system" fn(*const u8, *mut *mut c_void) -> i32 = std::mem::transmute(gdi);
            let mut dbg = std::ptr::null_mut();
            let r = gdi(IID_DEBUG.as_ptr(), &mut dbg);
            if r >= 0 && !dbg.is_null() {
                let en: unsafe extern "system" fn(*mut c_void) = std::mem::transmute(*(*(dbg as *const *const usize)).add(3));
                en(dbg);
            }
            log_d3d12(&format!("[d3d12x] debug layer -> {:08x}", r as u32));
        }
    }
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
        // Keep the debug-layer info queue (present only when the layer is on) for message dumps.
        const IID_INFOQ: [u8; 16] = [0x0b, 0xa9, 0x42, 0x07, 0x87, 0xc3, 0x3f, 0x48, 0xb9, 0x46, 0x30, 0xa7, 0xe4, 0xe6, 0x14, 0x58];
        let d = *out_device;
        let qi: unsafe extern "system" fn(*mut c_void, *const u8, *mut *mut c_void) -> i32 = std::mem::transmute(*(*(d as *const *const usize)));
        let mut q = std::ptr::null_mut();
        if qi(d, IID_INFOQ.as_ptr(), &mut q) >= 0 && !q.is_null() {
            INFO_QUEUE.store(q as usize, Ordering::Release);
            log_d3d12("[d3d12x] info queue attached");
        }
        *out_device = wrap_device_pointer(*out_device);
    }
    if std::env::var("PERCY_TRACE").is_ok() {
        eprintln!("[d3d12_xs] D3D12XboxCreateDevice -> hr=0x{hr:08x} forced_wrap={forced_wrap}");
    }
    hr
}

static INFO_QUEUE: AtomicUsize = AtomicUsize::new(0);
static INFO_LOCK: Mutex<()> = Mutex::new(());

/// Print and clear stored debug-layer messages (severity <= WARNING), at most `max` distinct IDs.
pub(crate) unsafe fn drain_info(tag: &str, max: usize) {
    let q = INFO_QUEUE.load(Ordering::Acquire) as *mut c_void;
    if q.is_null() {
        return;
    }
    let vt = *(q as *const *const usize);
    let clear: unsafe extern "system" fn(*mut c_void) = std::mem::transmute(*vt.add(4));
    let get: unsafe extern "system" fn(*mut c_void, u64, *mut u8, *mut usize) -> i32 = std::mem::transmute(*vt.add(5));
    let num: unsafe extern "system" fn(*mut c_void) -> u64 = std::mem::transmute(*vt.add(8));
    let _g = INFO_LOCK.lock();
    let n = num(q);
    let mut seen: Vec<u32> = Vec::new();
    for i in 0..n {
        let mut len = 0usize;
        get(q, i, std::ptr::null_mut(), &mut len);
        if len == 0 || len > 1 << 16 {
            continue;
        }
        let mut buf = vec![0u8; len];
        if get(q, i, buf.as_mut_ptr(), &mut len) < 0 {
            continue;
        }
        let sev = *(buf.as_ptr().add(4) as *const u32);
        let id = *(buf.as_ptr().add(8) as *const u32);
        if sev > 2 || seen.contains(&id) {
            continue;
        }
        seen.push(id);
        let desc = *(buf.as_ptr().add(16) as *const *const i8);
        let s = std::ffi::CStr::from_ptr(desc).to_string_lossy();
        log_d3d12(&format!("[d3d12dbg] {tag} sev={sev} id={id} {}", &s[..s.len().min(400)]));
        if seen.len() >= max {
            break;
        }
    }
    clear(q);
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
