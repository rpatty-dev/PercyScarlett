use std::ffi::{c_void, CString};
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

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
    vtbl: *const [*const c_void; 64],
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
        proxy_add_ref(this);
        *out = this;
    } else {
        *out = std::ptr::null_mut();
    }
    hr
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
    log_d3d12("ID3D12DeviceX: unimplemented Xbox-only vtable slot called -> E_NOTIMPL");
    E_NOTIMPL
}

unsafe fn wrap_device_pointer(inner: *mut c_void) -> *mut c_void {
    if inner.is_null() {
        return inner;
    }
    let mut vtbl = [xbox_slot_unimpl as *const c_void; 64];
    vtbl[0] = proxy_qi as *const c_void;
    vtbl[1] = proxy_add_ref as *const c_void;
    vtbl[2] = proxy_release as *const c_void;
    let leaked_vtbl = Box::leak(Box::new(vtbl));
    let proxy = Box::new(DeviceXProxy {
        vtbl: leaked_vtbl as *const [*const c_void; 64],
        inner,
        refs: AtomicU32::new(1),
    });
    Box::into_raw(proxy) as *mut c_void
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
    let hr = D3D12CreateDevice(adapter, D3D_FEATURE_LEVEL_12_0, riid, out_device);
    if hr >= 0 && !(*out_device).is_null() && std::env::var("PERCY_D3D12_WRAP").ok().as_deref() == Some("1") {
        *out_device = wrap_device_pointer(*out_device);
    }
    hr
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
            assert_eq!(xbox_slot(), E_NOTIMPL);
            let rel: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(p_vtbl[2]);
            assert_eq!(rel(proxy), 0);
        }
    }
}
