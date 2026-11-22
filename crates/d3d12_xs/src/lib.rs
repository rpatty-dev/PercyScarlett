use std::ffi::{c_void, CString};
use std::sync::OnceLock;

const E_FAIL: i32 = 0x8000_4005u32 as i32;
const E_NOTIMPL: i32 = 0x8000_4001u32 as i32;

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

#[no_mangle]
pub unsafe extern "system" fn D3D12XboxCreateDevice(
    _adapter: *mut c_void,
    _params: *const c_void,
    _riid: *const c_void,
    _out_device: *mut *mut c_void,
) -> i32 {
    E_NOTIMPL
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
}
