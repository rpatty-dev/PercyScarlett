use std::ffi::c_void;

mod gamesave;
const S_OK: i32 = 0;
const E_POINTER: i32 = 0x8000_4003u32 as i32;

#[repr(C)]
struct TaskQueueHandle {
    magic: u32,
}

const TQ_MAGIC: u32 = 0x5053_5451; // "PSTQ"

#[no_mangle]
pub unsafe extern "system" fn XTaskQueueCreate(
    _work_mode: u32,
    _completion_mode: u32,
    out_queue: *mut *mut c_void,
) -> i32 {
    if out_queue.is_null() {
        return E_POINTER;
    }
    *out_queue = Box::into_raw(Box::new(TaskQueueHandle { magic: TQ_MAGIC })) as *mut c_void;
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XTaskQueueCloseHandle(queue: *mut c_void) {
    if queue.is_null() {
        return;
    }
    let h = &*(queue as *const TaskQueueHandle);
    if h.magic == TQ_MAGIC {
        drop(Box::from_raw(queue as *mut TaskQueueHandle));
    }
}

#[no_mangle]
pub unsafe extern "system" fn XTaskQueueDispatch(
    _queue: *mut c_void,
    _port: u32,
    _timeout_ms: u32,
) -> i32 {
    0
}

#[repr(C)]
pub struct XAsyncBlock {
    pub queue: *mut c_void,
    pub context: *mut c_void,
    pub callback: Option<unsafe extern "system" fn(*mut XAsyncBlock)>,
}

#[repr(C)]
struct XUserHandle {
    magic: u32,
    id: u64,
}

const XUSER_MAGIC: u32 = 0x5053_5855; // "PSXU"

#[no_mangle]
pub unsafe extern "system" fn XGameRuntimeInitialize() -> i32 {
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XGameRuntimeUninitialize() {}

#[no_mangle]
pub unsafe extern "system" fn XSystemGetDeviceType() -> u32 {
    7 // XSystemDeviceType::XboxScarlettAnaconda (Xbox Series X; Lockhart=6, XboxOneX=4)
}

#[no_mangle]
pub unsafe extern "system" fn XSystemGetConsoleId(
    size: usize,
    buf: *mut u8,
    used: *mut usize,
) -> i32 {
    let id = b"PERCY-XSX-0001\0";
    if !used.is_null() {
        *used = id.len();
    }
    if !buf.is_null() && size > 0 {
        let n = id.len().min(size);
        std::ptr::copy_nonoverlapping(id.as_ptr(), buf, n);
        *buf.add(n - 1) = 0;
    }
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XSystemGetXboxLiveSandboxId(
    size: usize,
    buf: *mut u8,
    used: *mut usize,
) -> i32 {
    let s = b"RETAIL\0";
    if !used.is_null() {
        *used = s.len();
    }
    if !buf.is_null() && size > 0 {
        let n = s.len().min(size);
        std::ptr::copy_nonoverlapping(s.as_ptr(), buf, n);
        *buf.add(n - 1) = 0;
    }
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XPackageGetCurrentProcessPackageIdentifier(
    size: usize,
    buf: *mut u8,
) -> i32 {
    let id = b"PercyScarlett.Title_1.0.0.0_x64__8wekyb3d8bbwe\0";
    if buf.is_null() || size == 0 {
        return E_POINTER;
    }
    let n = id.len().min(size);
    std::ptr::copy_nonoverlapping(id.as_ptr(), buf, n);
    *buf.add(n - 1) = 0;
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XPackageEnumeratePackages(
    _kind: u32,
    _scope: u32,
    _context: *mut c_void,
    _callback: *const c_void,
) -> i32 {
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XUserAddAsync(
    _options: u32,
    async_block: *mut XAsyncBlock,
) -> i32 {
    if async_block.is_null() {
        return E_POINTER;
    }
    if let Some(cb) = (*async_block).callback {
        cb(async_block);
    }
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XUserAddResult(
    _async_block: *mut XAsyncBlock,
    out_user: *mut *mut c_void,
) -> i32 {
    if out_user.is_null() {
        return E_POINTER;
    }
    *out_user = Box::into_raw(Box::new(XUserHandle {
        magic: XUSER_MAGIC,
        id: 0x0009_0000_0000_0001,
    })) as *mut c_void;
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XUserCloseHandle(user: *mut c_void) {
    if user.is_null() {
        return;
    }
    let u = &*(user as *const XUserHandle);
    if u.magic == XUSER_MAGIC {
        drop(Box::from_raw(user as *mut XUserHandle));
    }
}

#[no_mangle]
pub unsafe extern "system" fn XUserGetGamertag(
    _user: *mut c_void,
    _component: u32,
    gamertag_size: usize,
    gamertag: *mut u8,
    gamertag_used: *mut usize,
) -> i32 {
    if gamertag.is_null() || gamertag_size == 0 {
        return E_POINTER;
    }
    let tag = b"PercyPlayer\0";
    let n = tag.len().min(gamertag_size);
    std::ptr::copy_nonoverlapping(tag.as_ptr(), gamertag, n);
    *gamertag.add(n - 1) = 0;
    if !gamertag_used.is_null() {
        *gamertag_used = n;
    }
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XUserGetState(_user: *mut c_void, out_state: *mut u32) -> i32 {
    if out_state.is_null() {
        return E_POINTER;
    }
    *out_state = 0; // XUserState::SignedIn
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XUserCheckPrivilege(
    _user: *mut c_void,
    _options: u32,
    _privilege: u32,
    out_has_privilege: *mut i32,
    out_reason: *mut u32,
) -> i32 {
    if !out_has_privilege.is_null() {
        *out_has_privilege = 1;
    }
    if !out_reason.is_null() {
        *out_reason = 0;
    }
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XblInitialize(_args: *const c_void) -> i32 {
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XblCleanup(_async_block: *mut XAsyncBlock) -> i32 {
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XblContextCreateHandle(
    _user: *mut c_void,
    out_ctx: *mut *mut c_void,
) -> i32 {
    if out_ctx.is_null() {
        return E_POINTER;
    }
    *out_ctx = Box::into_raw(Box::new(0x5053_5842u32)) as *mut c_void;
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XblContextCloseHandle(ctx: *mut c_void) {
    if !ctx.is_null() {
        drop(Box::from_raw(ctx as *mut u32));
    }
}

#[repr(C)]
struct XGameSaveProviderHandle {
    magic: u32,
    save_dir: std::path::PathBuf,
}

#[repr(C)]
struct XGameSaveContainerHandle {
    magic: u32,
    container_dir: std::path::PathBuf,
}

const XGS_PROV_MAGIC: u32 = 0x5053_4753; // "PSGS"
const XGS_CONT_MAGIC: u32 = 0x5053_4743; // "PSGC"

fn save_root_dir() -> std::path::PathBuf {
    std::env::var("PERCY_VFS_S")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("percy_S"))
        .join("saves")
}

#[no_mangle]
pub unsafe extern "system" fn XGameSaveInitializeProvider(
    _user: *mut c_void,
    scid: *const u8,
    _sync_on_demand: i32,
    out_provider: *mut *mut c_void,
) -> i32 {
    if out_provider.is_null() {
        return E_POINTER;
    }
    let scid_str = if !scid.is_null() {
        std::ffi::CStr::from_ptr(scid as *const i8)
            .to_string_lossy()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect::<String>()
    } else {
        "default".to_string()
    };
    let save_dir = save_root_dir().join(if scid_str.is_empty() { "default" } else { &scid_str });
    let _ = std::fs::create_dir_all(&save_dir);
    *out_provider = Box::into_raw(Box::new(XGameSaveProviderHandle {
        magic: XGS_PROV_MAGIC,
        save_dir,
    })) as *mut c_void;
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XGameSaveCloseProvider(provider: *mut c_void) {
    if !provider.is_null() && (*(provider as *const XGameSaveProviderHandle)).magic == XGS_PROV_MAGIC {
        drop(Box::from_raw(provider as *mut XGameSaveProviderHandle));
    }
}

#[no_mangle]
pub unsafe extern "system" fn XGameSaveCreateContainer(
    provider: *mut c_void,
    name: *const u8,
    out_container: *mut *mut c_void,
) -> i32 {
    if provider.is_null() || name.is_null() || out_container.is_null() {
        return E_POINTER;
    }
    let p = &*(provider as *const XGameSaveProviderHandle);
    let cname = std::ffi::CStr::from_ptr(name as *const i8)
        .to_string_lossy()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect::<String>();
    let container_dir = p.save_dir.join(if cname.is_empty() { "slot0" } else { &cname });
    let _ = std::fs::create_dir_all(&container_dir);
    *out_container = Box::into_raw(Box::new(XGameSaveContainerHandle {
        magic: XGS_CONT_MAGIC,
        container_dir,
    })) as *mut c_void;
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XGameSaveCloseContainer(container: *mut c_void) {
    if !container.is_null() && (*(container as *const XGameSaveContainerHandle)).magic == XGS_CONT_MAGIC {
        drop(Box::from_raw(container as *mut XGameSaveContainerHandle));
    }
}

#[no_mangle]
pub unsafe extern "system" fn XGameSaveWriteBlob(
    container: *mut c_void,
    blob_name: *const u8,
    data: *const u8,
    size: usize,
) -> i32 {
    if container.is_null() || blob_name.is_null() || (size > 0 && data.is_null()) {
        return E_POINTER;
    }
    let c = &*(container as *const XGameSaveContainerHandle);
    let bname = std::ffi::CStr::from_ptr(blob_name as *const i8).to_string_lossy();
    let bytes = if size == 0 { &[] } else { std::slice::from_raw_parts(data, size) };
    if std::fs::write(c.container_dir.join(format!("{bname}.bin")), bytes).is_ok() {
        S_OK
    } else {
        0x8000_4005u32 as i32
    }
}

#[no_mangle]
pub unsafe extern "system" fn XGameSaveReadBlobData(
    container: *mut c_void,
    blob_name: *const u8,
    out_size: *mut usize,
    out_buf: *mut u8,
    buf_cap: usize,
) -> i32 {
    if container.is_null() || blob_name.is_null() || out_size.is_null() {
        return E_POINTER;
    }
    let c = &*(container as *const XGameSaveContainerHandle);
    let bname = std::ffi::CStr::from_ptr(blob_name as *const i8).to_string_lossy();
    let Ok(bytes) = std::fs::read(c.container_dir.join(format!("{bname}.bin"))) else {
        *out_size = 0;
        return 0x8007_0002u32 as i32; // ERROR_FILE_NOT_FOUND
    };
    *out_size = bytes.len();
    if !out_buf.is_null() && buf_cap >= bytes.len() {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out_buf, bytes.len());
    }
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XUserGetId(user: *mut c_void, out_id: *mut u64) -> i32 {
    if out_id.is_null() {
        return E_POINTER;
    }
    if !user.is_null() && (*(user as *const XUserHandle)).magic == XUSER_MAGIC {
        *out_id = (*(user as *const XUserHandle)).id;
    } else {
        *out_id = 0x0009_0000_0000_0001;
    }
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XAsyncGetStatus(_async_block: *mut c_void, _wait: i32) -> i32 {
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XAsyncGetResultSize(
    _async_block: *mut c_void,
    out_size: *mut usize,
) -> i32 {
    if out_size.is_null() {
        return E_POINTER;
    }
    *out_size = 0;
    S_OK
}

#[repr(C)]
pub struct XNetworkingConnectivityHint {
    pub connectivity_level: u32,
    pub connectivity_cost: u32,
    pub iana_interface_type: u32,
    pub network_initialized: u8,
    pub approaching_data_limit: u8,
    pub over_data_limit: u8,
    pub roaming: u8,
}

#[no_mangle]
pub unsafe extern "system" fn XNetworkingGetConnectivityHint(
    hint: *mut XNetworkingConnectivityHint,
) -> i32 {
    if hint.is_null() {
        return E_POINTER;
    }
    *hint = XNetworkingConnectivityHint {
        connectivity_level: 1, // LocalAccess (offline-safe)
        connectivity_cost: 1,  // Unrestricted
        iana_interface_type: 6, // Ethernet
        network_initialized: 1,
        approaching_data_limit: 0,
        over_data_limit: 0,
        roaming: 0,
    };
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XStoreCreateContext(
    _user: *mut c_void,
    out_ctx: *mut *mut c_void,
) -> i32 {
    if out_ctx.is_null() {
        return E_POINTER;
    }
    *out_ctx = Box::into_raw(Box::new(TaskQueueHandle { magic: TQ_MAGIC })) as *mut c_void;
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XStoreCloseContextHandle(ctx: *mut c_void) {
    XTaskQueueCloseHandle(ctx);
}

#[no_mangle]
pub unsafe extern "system" fn XStoreQueryGameAndDlcPackageUpdatesAsync(
    _ctx: *mut c_void,
    async_block: *mut XAsyncBlock,
) -> i32 {
    if !async_block.is_null() {
        if let Some(cb) = (*async_block).callback {
            cb(async_block);
        }
    }
    S_OK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_queue_and_xuser_work() {
        unsafe {
            assert_eq!(XSystemGetDeviceType(), 7);
            let mut q: *mut c_void = std::ptr::null_mut();
            assert_eq!(XTaskQueueCreate(0, 0, &mut q), S_OK);
            assert!(!q.is_null());

            let mut ab = XAsyncBlock { queue: q, context: std::ptr::null_mut(), callback: None };
            assert_eq!(XUserAddAsync(0, &mut ab), S_OK);
            let mut user: *mut c_void = std::ptr::null_mut();
            assert_eq!(XUserAddResult(&mut ab, &mut user), S_OK);
            assert!(!user.is_null());

            let mut uid = 0u64;
            assert_eq!(XUserGetId(user, &mut uid), S_OK);
            assert_ne!(uid, 0);

            let mut tag = [0u8; 32];
            let mut used = 0usize;
            assert_eq!(XUserGetGamertag(user, 0, tag.len(), tag.as_mut_ptr(), &mut used), S_OK);
            assert_eq!(&tag[..used], b"PercyPlayer\0");

            assert_eq!(XblInitialize(std::ptr::null()), S_OK);
            let mut xbl_ctx: *mut c_void = std::ptr::null_mut();
            assert_eq!(XblContextCreateHandle(user, &mut xbl_ctx), S_OK);
            XblContextCloseHandle(xbl_ctx);

            let mut prov: *mut c_void = std::ptr::null_mut();
            assert_eq!(XGameSaveInitializeProvider(user, b"test_scid\0".as_ptr(), 0, &mut prov), S_OK);
            let mut cont: *mut c_void = std::ptr::null_mut();
            assert_eq!(XGameSaveCreateContainer(prov, b"slot0\0".as_ptr(), &mut cont), S_OK);
            assert_eq!(XGameSaveWriteBlob(cont, b"save\0".as_ptr(), b"HELLO".as_ptr(), 5), S_OK);
            let mut out_sz = 0usize;
            let mut buf = [0u8; 8];
            assert_eq!(XGameSaveReadBlobData(cont, b"save\0".as_ptr(), &mut out_sz, buf.as_mut_ptr(), buf.len()), S_OK);
            assert_eq!(&buf[..out_sz], b"HELLO");
            XGameSaveCloseContainer(cont);
            XGameSaveCloseProvider(prov);

            let mut net = std::mem::zeroed::<XNetworkingConnectivityHint>();
            assert_eq!(XNetworkingGetConnectivityHint(&mut net), S_OK);
            assert_eq!(net.network_initialized, 1);
            let mut store: *mut c_void = std::ptr::null_mut();
            assert_eq!(XStoreCreateContext(user, &mut store), S_OK);
            assert!(!store.is_null());
            XStoreCloseContextHandle(store);

            XUserCloseHandle(user);
            XTaskQueueCloseHandle(q);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Gaming Runtime ABI forwarders. GDK titles bind the runtime with
// LoadLibraryExA("xgameruntime.dll", LOAD_LIBRARY_SEARCH_SYSTEM32) + GetProcAddress. When this shim
// is already loaded under that base name, the loader hands it back, so it must expose the same
// entry points and forward them to the system runtime (loaded by full path as a distinct module).
// This is also the single choke point for overriding console-only API interfaces.
// ---------------------------------------------------------------------------------------------
type RtFn8 = unsafe extern "system" fn(usize, usize, usize, usize, usize, usize, usize, usize) -> i32;

#[cfg(windows)]
extern "system" {
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    fn GetSystemDirectoryW(buf: *mut u16, n: u32) -> u32;
}

static SYSTEM_RUNTIME: std::sync::OnceLock<usize> = std::sync::OnceLock::new();

#[cfg(windows)]
fn system_runtime() -> usize {
    *SYSTEM_RUNTIME.get_or_init(|| unsafe {
        let mut buf = [0u16; 260];
        let n = GetSystemDirectoryW(buf.as_mut_ptr(), 260) as usize;
        let mut path: Vec<u16> = buf[..n].to_vec();
        path.extend("\\xgameruntime.dll".encode_utf16());
        path.push(0);
        LoadLibraryExW(path.as_ptr(), std::ptr::null_mut(), 0) as usize
    })
}

#[cfg(windows)]
unsafe fn forward(name: &[u8], a: [usize; 8]) -> i32 {
    const E_NOTIMPL: i32 = 0x8000_4001u32 as i32;
    let h = system_runtime();
    if h == 0 {
        return E_NOTIMPL;
    }
    // QueryApiImpl is the hot path (every runtime call): resolve it once.
    static QUERY: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let hot = name == b"QueryApiImpl\0";
    let cached = if hot { QUERY.load(std::sync::atomic::Ordering::Relaxed) } else { 0 };
    let p = if cached != 0 { cached as *mut c_void } else { GetProcAddress(h as *mut c_void, name.as_ptr()) };
    if hot && cached == 0 && !p.is_null() {
        QUERY.store(p as usize, std::sync::atomic::Ordering::Relaxed);
    }
    if p.is_null() {
        return E_NOTIMPL;
    }
    let f: RtFn8 = std::mem::transmute(p);
    f(a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7])
}

macro_rules! runtime_forwarder {
    ($export:ident) => {
        #[cfg(windows)]
        #[no_mangle]
        pub unsafe extern "system" fn $export(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize, h: usize) -> i32 {
            forward(concat!(stringify!($export), "\0").as_bytes(), [a, b, c, d, e, f, g, h])
        }
    };
}
// ---------------------------------------------------------------------------------------------
// Offline XUser. Console titles' XUser sign-in cannot complete on PC (the runtime's
// XUserAddResult throws: no push-notification registration for the console identity), and titles
// treat sign-in failure as fatal. We answer every handle-taking XUser slot with one local,
// signed-in, offline user. Slot layout recovered from the title's runtime stubs (this, args...).
// CLSID 01acd177-91f9-4763-a38e-ccbb55ce32e0; IID eb9bf948 = core XUser, IID cef4fac0 = gamertag.
// ---------------------------------------------------------------------------------------------
const CLSID_XUSER: [u8; 16] = [0x77, 0xd1, 0xac, 0x01, 0xf9, 0x91, 0x63, 0x47, 0xa3, 0x8e, 0xcc, 0xbb, 0x55, 0xce, 0x32, 0xe0];
const IID_XUSER_GAMERTAG_PREFIX: u32 = 0xcef4fac0;
const VTBL_SLOTS: usize = 96;
const E_FAIL: i32 = 0x8000_4005u32 as i32;
const E_INVALIDARG: i32 = 0x8007_0057u32 as i32;

#[repr(C)]
pub struct LocalUser {
    magic: u32,
    local_id: u64,
    xuid: u64,
}
static LOCAL_USER: LocalUser = LocalUser { magic: 0x5053_4C55 /* "PSLU" */, local_id: 1, xuid: 0x0009_0000_5053_0001 };
const GAMERTAG: &[u8] = b"Player";

fn local_handle() -> *mut c_void {
    &LOCAL_USER as *const LocalUser as *mut c_void
}
unsafe fn is_local(h: *mut c_void) -> bool {
    h == local_handle()
}
fn ulog(s: &str) {
    let mut v = format!("[xuser] {s}").into_bytes();
    v.push(0);
    unsafe { OutputDebugStringA(v.as_ptr()) };
}

type P = *mut c_void;
unsafe extern "system" fn xu_duplicate(_t: P, u: P, out: *mut P) -> i32 {
    if out.is_null() || !is_local(u) { return E_INVALIDARG; }
    *out = u;
    S_OK
}
unsafe extern "system" fn xu_close(_t: P, _u: P) {}
unsafe extern "system" fn xu_compare(_t: P, a: P, b: P) -> i32 {
    (a as isize - b as isize).signum() as i32
}
unsafe extern "system" fn xu_add_result(_t: P, _async: P, out: *mut P) -> i32 {
    if out.is_null() { return E_POINTER; }
    *out = local_handle();
    ulog("XUserAddResult -> offline local user");
    S_OK
}
unsafe extern "system" fn xu_get_local_id(_t: P, u: P, out: *mut u64) -> i32 {
    if out.is_null() || !is_local(u) { return E_INVALIDARG; }
    *out = LOCAL_USER.local_id;
    S_OK
}
unsafe extern "system" fn xu_find_by_local_id(_t: P, id: u64, out: *mut P) -> i32 {
    if out.is_null() { return E_POINTER; }
    if id != LOCAL_USER.local_id { return E_INVALIDARG; }
    *out = local_handle();
    S_OK
}
unsafe extern "system" fn xu_get_id(_t: P, u: P, out: *mut u64) -> i32 {
    if out.is_null() || !is_local(u) { return E_INVALIDARG; }
    *out = LOCAL_USER.xuid;
    S_OK
}
unsafe extern "system" fn xu_find_by_id(_t: P, xuid: u64, out: *mut P) -> i32 {
    if out.is_null() { return E_POINTER; }
    if xuid != LOCAL_USER.xuid { return E_INVALIDARG; }
    *out = local_handle();
    S_OK
}
unsafe extern "system" fn xu_get_is_guest(_t: P, u: P, out: *mut u8) -> i32 {
    if out.is_null() || !is_local(u) { return E_INVALIDARG; }
    *out = 0;
    S_OK
}
unsafe extern "system" fn xu_get_state(_t: P, u: P, out: *mut u32) -> i32 {
    if out.is_null() || !is_local(u) { return E_INVALIDARG; }
    *out = 0; // XUserState::SignedIn
    S_OK
}
unsafe extern "system" fn xu_get_age_group(_t: P, _u: P, out: *mut u32) -> i32 {
    if !out.is_null() { *out = 0; } // Unknown
    S_OK
}
unsafe extern "system" fn xu_check_privilege(_t: P, _u: P, _opts: u32, privilege: u32, has: *mut u8, reason: *mut u32) -> i32 {
    if !has.is_null() { *has = 1; }
    if !reason.is_null() { *reason = 0; }
    ulog(&format!("XUserCheckPrivilege({privilege}) -> granted"));
    S_OK
}
unsafe extern "system" fn xu_fail(_t: P) -> i32 {
    E_FAIL
}
/// Token requests: the network is unreachable (HRESULT_FROM_WIN32(ERROR_INTERNET_CANNOT_CONNECT)).
/// E_GAMEUSER_NO_TOKEN_REQUIRED: the endpoint takes no Xbox Live token; titles go on offline.
const E_GAMEUSER_NO_TOKEN_REQUIRED: i32 = 0x8924_5105u32 as i32;
unsafe extern "system" fn xu_offline(_t: P, a: usize, b: usize, c: usize, d: usize) -> i32 {
    let s = |p: usize| -> String {
        if p < 0x10000 || p & 0xFFFF_0000_0000_0000 != 0 { return String::new(); }
        let mut v = Vec::new();
        for i in 0..160 { let ch = *((p + i) as *const u8); if ch == 0 { break; } if !(0x20..0x7f).contains(&ch) { return String::new(); } v.push(ch); }
        String::from_utf8_lossy(&v).into_owned()
    };
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    if N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 12 {
        ulog(&format!("token request ({a:#x},{b:#x},{c:#x},{d:#x}) '{}' '{}' -> offline", s(c), s(d)));
    }
    E_GAMEUSER_NO_TOKEN_REQUIRED
}
// XAsync (CLSID = IID 073b7dcb-1fcf-4030-94be-e3c9eb623428): slot 7 Begin, slot 10 Complete.
const CLSID_XASYNC: [u8; 16] = [0xcb, 0x7d, 0x3b, 0x07, 0xcf, 0x1f, 0x30, 0x40, 0x94, 0xbe, 0xe3, 0xc9, 0xeb, 0x62, 0x34, 0x28];
unsafe extern "system" fn async_provider(_op: u32, _data: P) -> i32 {
    S_OK
}
/// Start `async` and complete it at once with `hr` (the title sees a normal asynchronous failure).
unsafe fn complete_async_with(async_block: P, hr: i32) -> i32 {
    let mut obj: *mut *const usize = std::ptr::null_mut();
    let q = forward(b"QueryApiImpl\0", [CLSID_XASYNC.as_ptr() as usize, CLSID_XASYNC.as_ptr() as usize, &mut obj as *mut _ as usize, 0, 0, 0, 0, 0]);
    if q < 0 || obj.is_null() {
        return hr;
    }
    let vt = *obj;
    let begin: unsafe extern "system" fn(P, P, P, P, *const u8, usize) -> i32 = std::mem::transmute(*vt.add(7));
    let complete: unsafe extern "system" fn(P, P, i32, usize) = std::mem::transmute(*vt.add(10));
    let r = begin(obj as P, async_block, std::ptr::null_mut(), async_provider as P, b"PercyOffline\0".as_ptr(), async_provider as usize);
    if r < 0 {
        return hr;
    }
    complete(obj as P, async_block, hr, 0);
    S_OK
}
/// GetTokenAndSignature[Utf16]Async(user, options, method, url, headerCount, headers, bodySize, body, async):
/// the request starts and fails with "cannot connect", as on a console without network.
unsafe extern "system" fn xu_token_async(_t: P, _u: P, _o: u32, _m: P, url: P, _hc: usize, _h: P, _bs: usize, _b: P, async_block: P) -> i32 {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let r = complete_async_with(async_block, if mock_xsts() { S_OK } else { E_GAMEUSER_NO_TOKEN_REQUIRED });
    if N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 6 {
        let u = if url.is_null() { String::new() } else { std::ffi::CStr::from_ptr(url as *const std::ffi::c_char).to_string_lossy().into_owned() };
        ulog(&format!("token request '{u}' -> async offline (begin {r:08x})"));
    }
    r
}
// Synthetic offline XSTS token (flag file `percy_mock_xsts`): unsigned XBL3.0 JWT so titles that
// insist on a token see a signed-in user; any real service rejects it.
fn mock_xsts() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::current_exe().map(|e| e.with_file_name("percy_mock_xsts").exists()).unwrap_or(false))
}
fn b64url(d: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut o = String::new();
    for c in d.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..=c.len() { o.push(A[(n >> (18 - 6 * i) & 63) as usize] as char); }
    }
    o
}
fn mock_token() -> (Vec<u8>, Vec<u8>) {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let body = format!("{{\"xuid\":\"{}\",\"uhs\":\"0000000000000001\",\"gtg\":\"Player\",\"agg\":\"Adult\",\"iat\":{now},\"exp\":{}}}", LOCAL_USER.xuid, now + 86400);
    let tok = format!("XBL3.0 x=0000000000000001;{}.{}.{}\0", b64url(br#"{"alg":"ES256","typ":"JWT"}"#), b64url(body.as_bytes()), b64url(b"offline"));
    (tok.into_bytes(), b"AAAAAQ\0".to_vec())
}
/// GetTokenAndSignatureResultSize(async, size).
unsafe extern "system" fn xu_tok_size(_t: P, _a: P, size: *mut usize) -> i32 {
    if !mock_xsts() { return E_GAMEUSER_NO_TOKEN_REQUIRED; }
    let (t, g) = mock_token();
    if !size.is_null() { *size = 32 + t.len() + g.len(); }
    S_OK
}
/// GetTokenAndSignatureResult(async, bufSize, buf, &data, used): {tokenSize, sigSize, token*, sig*} + strings.
unsafe extern "system" fn xu_tok_result(_t: P, _a: P, n: usize, buf: *mut u8, out: *mut *mut u8, used: *mut usize) -> i32 {
    if !mock_xsts() { return E_GAMEUSER_NO_TOKEN_REQUIRED; }
    let (t, g) = mock_token();
    let need = 32 + t.len() + g.len();
    if buf.is_null() || n < need { return E_INVALIDARG; }
    let w = buf as *mut usize;
    *w = t.len(); *w.add(1) = g.len();
    *w.add(2) = buf.add(32) as usize; *w.add(3) = buf.add(32 + t.len()) as usize;
    std::ptr::copy_nonoverlapping(t.as_ptr(), buf.add(32), t.len());
    std::ptr::copy_nonoverlapping(g.as_ptr(), buf.add(32 + t.len()), g.len());
    if !out.is_null() { *out = buf; }
    if !used.is_null() { *used = need; }
    ulog("token result -> synthetic XSTS");
    S_OK
}
/// AddAsync(options, async): sign-in completes at once with the offline local user (AddResult).
unsafe extern "system" fn xu_add_async(_t: P, _options: u32, async_block: P) -> i32 {
    complete_async_with(async_block, S_OK)
}
/// ResolveIssueWithUiAsync(user, url, async): nothing to resolve offline.
unsafe extern "system" fn xu_ui_async3(_t: P, _u: P, _url: P, async_block: P) -> i32 {
    complete_async_with(async_block, S_OK)
}
/// ResolvePrivilegeWithUiAsync(user, options, privilege, async).
unsafe extern "system" fn xu_ui_async4(_t: P, _u: P, _o: u32, _p: u32, async_block: P) -> i32 {
    complete_async_with(async_block, S_OK)
}
unsafe extern "system" fn xu_ok(_t: P) -> i32 {
    S_OK
}
unsafe extern "system" fn xu_register_change(_t: P, _q: P, _ctx: P, _cb: P, token: *mut u64) -> i32 {
    if !token.is_null() { *token = 0x5053_0001; }
    S_OK
}
unsafe extern "system" fn xu_unregister_change(_t: P, _token: u64, _wait: u8) {}
unsafe extern "system" fn xu_close_deferral(_t: P, _h: P) {}
/// IID cef4fac0 slot 3: GetGamertag(user, component, size, buf, used).
unsafe extern "system" fn xu_get_gamertag(_t: P, u: P, _component: u32, size: usize, buf: *mut u8, used: *mut usize) -> i32 {
    if !is_local(u) { return E_INVALIDARG; }
    let need = GAMERTAG.len() + 1;
    if !used.is_null() { *used = need; }
    if buf.is_null() || size < need { return 0x8007_007Au32 as i32; } // ERROR_INSUFFICIENT_BUFFER
    std::ptr::copy_nonoverlapping(GAMERTAG.as_ptr(), buf, GAMERTAG.len());
    *buf.add(GAMERTAG.len()) = 0;
    S_OK
}

/// Slot table for IID eb9bf948 (offset/8 -> replacement). Unlisted slots forward to the runtime.
fn core_overrides() -> Vec<(usize, usize)> {
    let f = xu_fail as usize;
    let n = xu_offline as usize;
    vec![
        (0x18 / 8, xu_duplicate as usize),
        (0x20 / 8, xu_close as usize),
        (0x28 / 8, xu_compare as usize),
        (0x38 / 8, xu_add_async as usize),
        (0x40 / 8, xu_add_result as usize),
        (0x48 / 8, xu_get_local_id as usize),
        (0x50 / 8, xu_find_by_local_id as usize),
        (0x58 / 8, xu_get_id as usize),
        (0x60 / 8, xu_find_by_id as usize),
        (0x68 / 8, xu_get_is_guest as usize),
        (0x70 / 8, xu_get_state as usize),
        (0x80 / 8, f), (0x88 / 8, f), (0x90 / 8, f), // gamer picture
        (0x98 / 8, xu_get_age_group as usize),
        (0xa0 / 8, xu_check_privilege as usize),
        (0xa8 / 8, xu_ui_async4 as usize), (0xb0 / 8, xu_ok as usize), // resolve privilege UI: nothing to resolve
        (0xb8 / 8, xu_token_async as usize), (0xc0 / 8, xu_tok_size as usize), (0xc8 / 8, xu_tok_result as usize), // token & signature: no network
        (0xd0 / 8, xu_token_async as usize), (0xd8 / 8, n), (0xe0 / 8, n),
        (0xe8 / 8, xu_ui_async3 as usize), (0xf0 / 8, xu_ok as usize), (0xf8 / 8, f), (0x100 / 8, f), // resolve issue UI
        (0x108 / 8, xu_register_change as usize),
        (0x110 / 8, xu_unregister_change as usize),
        (0x118 / 8, f),
        (0x120 / 8, xu_close_deferral as usize),
    ]
}

static PATCHED_VTBLS: std::sync::Mutex<Vec<(usize, usize)>> = std::sync::Mutex::new(Vec::new());

// ---------------------------------------------------------------------------------------------
// Persistent local storage. The PC runtime has no PLS for a console package identity (the title's
// storage bootstrap fails and nothing is ever written); answer with a local directory.
// CLSID f4faf4d4-2d04-4fce-b3e0-474a713a3e84; slots: 3 GetPathSize, 4 GetPath, 5 GetSpaceInfo,
// 6/7 PromptUserForSpace (async; answered by GetSpaceInfo having room).
// ---------------------------------------------------------------------------------------------
const CLSID_PLS: [u8; 16] = [0xd4, 0xf4, 0xfa, 0xf4, 0x04, 0x2d, 0xce, 0x4f, 0xb3, 0xe0, 0x47, 0x4a, 0x71, 0x3a, 0x3e, 0x84];

/// Data root for saves / local storage: the path in `percy_data_dir.txt` next to the exe (so a
/// packaged title can live on a roomy drive), else %LOCALAPPDATA%\PercyScarlett.
pub(crate) fn data_base() -> std::path::PathBuf {
    if let Some(p) = std::env::current_exe().ok().and_then(|e| std::fs::read_to_string(e.with_file_name("percy_data_dir.txt")).ok()) {
        let p = p.trim();
        if !p.is_empty() { return std::path::PathBuf::from(p); }
    }
    std::env::var_os("LOCALAPPDATA").map(|b| std::path::PathBuf::from(b).join("PercyScarlett")).unwrap_or_else(std::env::temp_dir)
}

fn pls_dir() -> Vec<u8> {
    // Per-title folder; Minecraft keeps its original name so existing saves stay put.
    let stem = std::env::current_exe().ok().and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned())).unwrap_or_default();
    let title = if stem.is_empty() || stem.to_ascii_lowercase().contains("minecraft") { "Minecraft".to_string() } else { stem };
    let d = data_base().join("pls").join(title);
    let _ = std::fs::create_dir_all(&d);
    let mut s = d.to_string_lossy().into_owned().into_bytes();
    s.push(b'\\');
    s
}
unsafe extern "system" fn pls_get_path_size(_t: P, out: *mut usize) -> i32 {
    if out.is_null() { return E_POINTER; }
    *out = pls_dir().len() + 1;
    S_OK
}
unsafe extern "system" fn pls_get_path(_t: P, size: usize, buf: *mut u8, used: *mut usize) -> i32 {
    let d = pls_dir();
    if !used.is_null() { *used = d.len() + 1; }
    if buf.is_null() || size < d.len() + 1 { return 0x8007_007Au32 as i32; }
    std::ptr::copy_nonoverlapping(d.as_ptr(), buf, d.len());
    *buf.add(d.len()) = 0;
    ulog(&format!("PLS path -> {}", String::from_utf8_lossy(&d)));
    S_OK
}
unsafe extern "system" fn pls_get_space_info(_t: P, info: *mut u64) -> i32 {
    if info.is_null() { return E_POINTER; }
    // {availableFreeBytes, totalFreeBytes, usedBytes, totalBytes}
    *info = 8 << 30;
    *info.add(1) = 8 << 30;
    *info.add(2) = 0;
    *info.add(3) = 8 << 30;
    S_OK
}

// ---------------------------------------------------------------------------------------------
// Device association (CLSID 7d824997-10dc-45ab-86b7-2737767c0bf1): slot 3 FindForDevice,
// slot 4 RegisterForDeviceAssociationChanged, slot 5 Unregister. Titles count an input device as
// the player's only after the association callback names it; the PC runtime never associates
// devices with our offline user, so every GameInput keyboard / mouse / gamepad is associated
// with it once the title registers.
// ---------------------------------------------------------------------------------------------
const CLSID_DEVASSOC: [u8; 16] = [0x97, 0x49, 0x82, 0x7d, 0xdc, 0x10, 0xab, 0x45, 0x86, 0xb7, 0x27, 0x37, 0x76, 0x7c, 0x0b, 0xf1];

unsafe extern "system" fn da_find_for_device(_t: P, _dev: *const u8, out: *mut P) -> i32 {
    if out.is_null() { return E_POINTER; }
    *out = local_handle();
    S_OK
}

/// Device ids of the current keyboard / mouse / gamepad from the system GameInput.
unsafe fn input_device_ids() -> Vec<[u8; 32]> {
    let mut buf = [0u16; 260];
    let n = GetSystemDirectoryW(buf.as_mut_ptr(), 260) as usize;
    let mut path: Vec<u16> = buf[..n].to_vec();
    path.extend("\\GameInput.dll".encode_utf16());
    path.push(0);
    let h = LoadLibraryExW(path.as_ptr(), std::ptr::null_mut(), 0);
    if h.is_null() { return Vec::new(); }
    let p = GetProcAddress(h, b"GameInputCreate\0".as_ptr());
    if p.is_null() { return Vec::new(); }
    let create: unsafe extern "system" fn(*mut P) -> i32 = std::mem::transmute(p);
    let mut gi: P = std::ptr::null_mut();
    if create(&mut gi) < 0 || gi.is_null() { return Vec::new(); }
    let vt = |o: P, s: usize| *(*(o as *const *const usize)).add(s);
    let mut ids = Vec::new();
    for kind in [0x40000u32, 0x10, 0x20] {
        // IGameInput slot 4 GetCurrentReading(kind, device, reading**)
        let get: unsafe extern "system" fn(P, u32, P, *mut P) -> i32 = std::mem::transmute(vt(gi, 4));
        let mut rd: P = std::ptr::null_mut();
        if get(gi, kind, std::ptr::null_mut(), &mut rd) < 0 || rd.is_null() { continue; }
        // IGameInputReading slot 6 GetDevice(device**)
        let gd: unsafe extern "system" fn(P, *mut P) = std::mem::transmute(vt(rd, 6));
        let mut dev: P = std::ptr::null_mut();
        gd(rd, &mut dev);
        if !dev.is_null() {
            // IGameInputDevice slot 3 GetDeviceInfo() -> info; deviceId at +32
            let info: unsafe extern "system" fn(P) -> *const u8 = std::mem::transmute(vt(dev, 3));
            let i = info(dev);
            if !i.is_null() {
                let mut id = [0u8; 32];
                std::ptr::copy_nonoverlapping(i.add(32), id.as_mut_ptr(), 32);
                if !ids.contains(&id) { ids.push(id); }
            }
            let rel: unsafe extern "system" fn(P) -> u32 = std::mem::transmute(vt(dev, 2));
            rel(dev);
        }
        let rel: unsafe extern "system" fn(P) -> u32 = std::mem::transmute(vt(rd, 2));
        rel(rd);
    }
    ids
}

/// RegisterForDeviceAssociationChanged(queue, ctx, cb, token): the callback must run on the title's
/// queue (callbacks from a foreign thread race the title's own device bookkeeping), so the
/// notification is delivered as the completion callback of an XAsyncBlock bound to that queue.
unsafe extern "system" fn da_register(_t: P, queue: P, ctx: P, cb: P, token: *mut u64) -> i32 {
    if cb.is_null() { return E_INVALIDARG; }
    if !token.is_null() { *token = 0x5053_0002; }
    #[repr(C)]
    struct Block { queue: P, context: P, callback: unsafe extern "system" fn(*mut Block), internal: [usize; 4] }
    unsafe extern "system" fn deliver(b: *mut Block) {
        let (ctx, cb) = *((*b).context as *const (usize, usize));
        let f: unsafe extern "system" fn(usize, *const u8) = std::mem::transmute(cb);
        let mut ids = input_device_ids();
        ids.insert(0, *b"PercyScarlett-VirtualGamepad-001"); // gameinput shim's virtual pad
        ulog(&format!("device association: {} input devices -> local user", ids.len()));
        for id in ids {
            // XUserDeviceAssociationChange { deviceId[32], oldUser, newUser }
            let mut ch = [0u8; 48];
            ch[..32].copy_from_slice(&id);
            ch[40..48].copy_from_slice(&LOCAL_USER.local_id.to_le_bytes());
            f(ctx, ch.as_ptr());
        }
    }
    let data = Box::leak(Box::new((ctx as usize, cb as usize))) as *mut (usize, usize) as P;
    let block = Box::leak(Box::new(Block { queue, context: data, callback: deliver, internal: [0; 4] })) as *mut Block as usize;
    if std::env::current_exe().map(|e| e.with_file_name("percy_no_assoc").exists()).unwrap_or(false) {
        ulog("device association: skipped (percy_no_assoc)");
        return S_OK;
    }
    std::thread::spawn(move || unsafe {
        std::thread::sleep(std::time::Duration::from_millis(1500));
        complete_async_with(block as P, S_OK);
    });
    S_OK
}
unsafe extern "system" fn da_unregister(_t: P, _token: u64, _wait: u8) -> u8 { 1 }

// ---- XGameSave call trace (CLSID 704c3f58) ----
const CLSID_GAMESAVE_D1: u32 = 0x704c_3f58;
static GS_ORIG: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static GS_N: [std::sync::atomic::AtomicU32; 40] = [const { std::sync::atomic::AtomicU32::new(0) }; 40];
unsafe fn gs_call(slot: usize, t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize, h: usize) -> usize {
    let o = *(GS_ORIG.load(std::sync::atomic::Ordering::Relaxed) as *const usize).add(slot);
    let fun: unsafe extern "system" fn(usize, usize, usize, usize, usize, usize, usize, usize, usize) -> usize = std::mem::transmute(o);
    let r = fun(t, a, b, c, d, e, f, g, h);
    if GS_N[slot].fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 3 {
        ulog(&format!("XGameSave slot {slot} ({a:#x},{b:#x},{c:#x}) -> {:08x}", r as u32));
    }
    r
}
unsafe extern "system" fn gs3(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize, h: usize) -> usize { gs_call(3, t, a, b, c, d, e, f, g, h) }
unsafe extern "system" fn gs4(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize, h: usize) -> usize { gs_call(4, t, a, b, c, d, e, f, g, h) }
unsafe extern "system" fn gs5(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize, h: usize) -> usize { gs_call(5, t, a, b, c, d, e, f, g, h) }
unsafe extern "system" fn gs6(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize, h: usize) -> usize { gs_call(6, t, a, b, c, d, e, f, g, h) }
unsafe extern "system" fn gs7(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize, h: usize) -> usize { gs_call(7, t, a, b, c, d, e, f, g, h) }
unsafe extern "system" fn gs8(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize, h: usize) -> usize { gs_call(8, t, a, b, c, d, e, f, g, h) }
unsafe extern "system" fn gs9(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize, h: usize) -> usize { gs_call(9, t, a, b, c, d, e, f, g, h) }
unsafe extern "system" fn gs10(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize, h: usize) -> usize { gs_call(10, t, a, b, c, d, e, f, g, h) }
unsafe extern "system" fn gs11(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize, h: usize) -> usize { gs_call(11, t, a, b, c, d, e, f, g, h) }
unsafe extern "system" fn gs12(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize, h: usize) -> usize { gs_call(12, t, a, b, c, d, e, f, g, h) }
unsafe extern "system" fn gs13(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize, h: usize) -> usize { gs_call(13, t, a, b, c, d, e, f, g, h) }
unsafe extern "system" fn gs14(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize, h: usize) -> usize { gs_call(14, t, a, b, c, d, e, f, g, h) }
unsafe extern "system" fn gs15(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize, h: usize) -> usize { gs_call(15, t, a, b, c, d, e, f, g, h) }
unsafe extern "system" fn gs16(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize, h: usize) -> usize { gs_call(16, t, a, b, c, d, e, f, g, h) }
unsafe extern "system" fn gs17(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize, h: usize) -> usize { gs_call(17, t, a, b, c, d, e, f, g, h) }
unsafe extern "system" fn gs18(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize, h: usize) -> usize { gs_call(18, t, a, b, c, d, e, f, g, h) }
unsafe extern "system" fn gs19(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize, h: usize) -> usize { gs_call(19, t, a, b, c, d, e, f, g, h) }
unsafe extern "system" fn gs20(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize, h: usize) -> usize { gs_call(20, t, a, b, c, d, e, f, g, h) }
unsafe extern "system" fn gs21(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize, h: usize) -> usize { gs_call(21, t, a, b, c, d, e, f, g, h) }
unsafe extern "system" fn gs22(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize, h: usize) -> usize { gs_call(22, t, a, b, c, d, e, f, g, h) }
unsafe extern "system" fn gs23(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize, h: usize) -> usize { gs_call(23, t, a, b, c, d, e, f, g, h) }
unsafe extern "system" fn gs24(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize, h: usize) -> usize { gs_call(24, t, a, b, c, d, e, f, g, h) }
unsafe extern "system" fn gs25(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize, h: usize) -> usize { gs_call(25, t, a, b, c, d, e, f, g, h) }
unsafe extern "system" fn gs26(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize, h: usize) -> usize { gs_call(26, t, a, b, c, d, e, f, g, h) }
unsafe extern "system" fn gs27(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize, h: usize) -> usize { gs_call(27, t, a, b, c, d, e, f, g, h) }
unsafe extern "system" fn gs28(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize, h: usize) -> usize { gs_call(28, t, a, b, c, d, e, f, g, h) }
unsafe extern "system" fn gs29(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize, h: usize) -> usize { gs_call(29, t, a, b, c, d, e, f, g, h) }
unsafe extern "system" fn gs30(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize, h: usize) -> usize { gs_call(30, t, a, b, c, d, e, f, g, h) }
unsafe extern "system" fn gs31(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize, h: usize) -> usize { gs_call(31, t, a, b, c, d, e, f, g, h) }
unsafe extern "system" fn gs32(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize, h: usize) -> usize { gs_call(32, t, a, b, c, d, e, f, g, h) }
unsafe extern "system" fn gs33(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize, h: usize) -> usize { gs_call(33, t, a, b, c, d, e, f, g, h) }

unsafe fn trace_gamesave(obj: *mut *const usize) {
    let orig = *obj as usize;
    if GS_ORIG.load(std::sync::atomic::Ordering::Relaxed) == 0 {
        GS_ORIG.store(orig, std::sync::atomic::Ordering::Relaxed);
    }
    static NEWV: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let n = *NEWV.get_or_init(|| {
        let mut v: Vec<usize> = std::slice::from_raw_parts(orig as *const usize, 34).to_vec();
        let th = [gs3 as usize, gs4 as usize, gs5 as usize, gs6 as usize, gs7 as usize, gs8 as usize, gs9 as usize, gs10 as usize, gs11 as usize, gs12 as usize, gs13 as usize, gs14 as usize, gs15 as usize, gs16 as usize, gs17 as usize, gs18 as usize, gs19 as usize, gs20 as usize, gs21 as usize, gs22 as usize, gs23 as usize, gs24 as usize, gs25 as usize, gs26 as usize, gs27 as usize, gs28 as usize, gs29 as usize, gs30 as usize, gs31 as usize, gs32 as usize, gs33 as usize];
        v[3..34].copy_from_slice(&th);
        Box::leak(v.into_boxed_slice()).as_ptr() as usize
    });
    if orig == GS_ORIG.load(std::sync::atomic::Ordering::Relaxed) {
        *obj = n as *const usize;
    }
}

// XNetworking (CLSID 37e56907): slot 15 GetConnectivityHint. Report a console with only local
// network access, so titles take their offline path instead of treating the refused Xbox Live
// tokens as a sign-in failure.
const CLSID_NETWORKING_D1: u32 = 0x37e5_6907;
unsafe extern "system" fn net_connectivity_hint(_t: P, hint: *mut u8) -> i32 {
    if hint.is_null() { return E_POINTER; }
    std::ptr::write_bytes(hint, 0, 16);
    *(hint as *mut u32) = 3; // XNetworkingConnectivityLevelHint::InternetAccess
    *(hint.add(4) as *mut u32) = 1; // Unrestricted
    *(hint.add(8) as *mut u32) = 6; // ethernet
    *hint.add(12) = 1; // networkInitialized
    S_OK
}

// Call tracer (flag file `percy_trace_rt` next to the exe): every forwarded runtime slot logs
// its CLSID/slot on the first few calls, then jumps to the real implementation.
fn trace_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::current_exe().map(|e| e.with_file_name("percy_trace_rt").exists()).unwrap_or(false))
}
extern "system" fn trace_hit(id: usize) {
    static HITS: std::sync::Mutex<Vec<(usize, u32)>> = std::sync::Mutex::new(Vec::new());
    let n = {
        let mut h = HITS.lock().unwrap();
        match h.iter_mut().find(|e| e.0 == id) {
            Some(e) => { e.1 += 1; e.1 }
            None => { h.push((id, 1)); 1 }
        }
    };
    if n <= 3 || n.is_power_of_two() {
        ulog(&format!("call {:08x} slot {} #{n}", id >> 8, id & 0xff));
    }
}
/// Stub: save arg regs, trace_hit(id), restore, jmp orig. shortcut: xmm args not preserved (none of the traced runtime APIs take floats).
unsafe fn trace_thunk(id: usize, orig: usize) -> usize {
    extern "system" { fn VirtualAlloc(a: *mut c_void, n: usize, t: u32, p: u32) -> *mut u8; }
    let mut c: Vec<u8> = vec![0x51, 0x52, 0x41, 0x50, 0x41, 0x51, 0x48, 0x83, 0xEC, 0x28, 0x48, 0xB9];
    c.extend_from_slice(&id.to_le_bytes());
    c.extend_from_slice(&[0x48, 0xB8]);
    c.extend_from_slice(&(trace_hit as usize).to_le_bytes());
    c.extend_from_slice(&[0xFF, 0xD0, 0x48, 0x83, 0xC4, 0x28, 0x41, 0x59, 0x41, 0x58, 0x5A, 0x59, 0x48, 0xB8]);
    c.extend_from_slice(&orig.to_le_bytes());
    c.extend_from_slice(&[0xFF, 0xE0]);
    let m = VirtualAlloc(std::ptr::null_mut(), c.len(), 0x3000, 0x40);
    if m.is_null() { return orig; }
    std::ptr::copy_nonoverlapping(c.as_ptr(), m, c.len());
    m as usize
}

// URL security-information lookups (slots 8-13: ANSI and UTF-16 Async / ResultSize / Result). The
// real service fails them with ERROR_NOT_FOUND for this package and the title aborts on the failed
// completion; XCurl here never opens a connection, so an empty record (no pinned data) is the
// truthful answer.
const SECINFO_BYTES: usize = 32;
unsafe extern "system" fn net_secinfo_async(_t: P, _url: P, async_block: P) -> i32 {
    complete_async_with(async_block, S_OK)
}
unsafe extern "system" fn net_secinfo_size(_t: P, _a: P, size: *mut usize) -> i32 {
    if size.is_null() { return E_POINTER; }
    *size = SECINFO_BYTES;
    S_OK
}
unsafe extern "system" fn net_secinfo_result(_t: P, _a: P, n: usize, used: *mut usize, buf: *mut u8, out: *mut *mut u8) -> i32 {
    if buf.is_null() || n < SECINFO_BYTES { return 0x8007_007Au32 as i32; } // ERROR_INSUFFICIENT_BUFFER
    std::ptr::write_bytes(buf, 0, SECINFO_BYTES);
    if !out.is_null() { *out = buf; }
    if !used.is_null() { *used = SECINFO_BYTES; }
    S_OK
}

unsafe fn override_interface(clsid: usize, iid: usize, obj: *mut *const usize) {
    if clsid != 0 && !obj.is_null() && *(clsid as *const u32) == CLSID_GAMESAVE_D1 {
        if std::env::var("PERCY_GAMESAVE_TRACE").is_ok() {
            trace_gamesave(obj);
            return;
        }
        let orig = *obj as usize;
        let mut cache = PATCHED_VTBLS.lock().unwrap();
        if cache.iter().any(|&(_, n)| n == orig) {
            return;
        }
        let newv = if let Some(&(_, n)) = cache.iter().find(|&&(o, _)| o == orig) {
            n
        } else {
            let mut v: Vec<usize> = std::slice::from_raw_parts(orig as *const usize, 34).to_vec();
            for (s, f) in gamesave::overrides() {
                v[s] = f;
            }
            let n = Box::leak(v.into_boxed_slice()).as_ptr() as usize;
            cache.push((orig, n));
            ulog("XGameSave -> local provider");
            n
        };
        *obj = newv as *const usize;
        return;
    }
    if clsid == 0 || iid == 0 || obj.is_null() {
        return;
    }
    let cls = std::slice::from_raw_parts(clsid as *const u8, 16);
    let iid_d1 = *(iid as *const u32);
    let overrides: Vec<(usize, usize)> = if *(clsid as *const u32) == CLSID_NETWORKING_D1 {
        vec![(8, net_secinfo_async as usize), (9, net_secinfo_size as usize), (10, net_secinfo_result as usize), (11, net_secinfo_async as usize), (12, net_secinfo_size as usize), (13, net_secinfo_result as usize), (15, net_connectivity_hint as usize), (16, xu_register_change as usize), (17, xu_unregister_change as usize)] // hint-change events stay local: the real service would call back into the title
    } else if cls == CLSID_DEVASSOC {
        vec![(3, da_find_for_device as usize), (4, da_register as usize), (5, da_unregister as usize)]
    } else if cls == CLSID_PLS {
        vec![(3, pls_get_path_size as usize), (4, pls_get_path as usize), (5, pls_get_space_info as usize)]
    } else if cls != CLSID_XUSER {
        if !trace_on() { return; }
        vec![]
    } else if iid_d1 == IID_XUSER_GAMERTAG_PREFIX {
        vec![(0x18 / 8, xu_get_gamertag as usize)]
    } else {
        core_overrides()
    };
    let orig = *obj as usize;
    let mut cache = PATCHED_VTBLS.lock().unwrap();
    if cache.iter().any(|&(_, n)| n == orig) {
        return;
    }
    let newv = if let Some(&(_, n)) = cache.iter().find(|&&(o, _)| o == orig) {
        n
    } else {
        let mut v: Vec<usize> = std::slice::from_raw_parts(orig as *const usize, VTBL_SLOTS).to_vec();
        if trace_on() {
            let d1 = *(clsid as *const u32);
            for (i, s) in v.iter_mut().enumerate().take(48).skip(3) {
                if !overrides.iter().any(|&(o, _)| o == i) && *s > 0x10000 {
                    *s = trace_thunk(((d1 as usize) << 8) | i, *s);
                }
            }
        }
        for (slot, f) in overrides {
            v[slot] = f;
        }
        let n = Box::leak(v.into_boxed_slice()).as_ptr() as usize;
        cache.push((orig, n));
        ulog(&format!("runtime vtable {:08x} patched", iid_d1));
        n
    };
    *obj = newv as *const usize;
}

#[cfg(windows)]
extern "system" {
    fn OutputDebugStringA(s: *const u8);
}
#[cfg(windows)]
#[no_mangle]
pub unsafe extern "system" fn QueryApiImpl(clsid: usize, iid: usize, out: *mut *mut *const usize, d: usize, e: usize, f: usize, g: usize, h: usize) -> i32 {
    // XStore (CLSID 0dd112ac-...): the console package has no Store identity on PC; refuse the
    // interface as an offline console does. No licence is granted: titles fall back to trial.
    if clsid != 0 && *(clsid as *const u32) == 0x0dd1_12ac {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        if N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == 0 {
            ulog("XStore refused (offline, no licence)");
        }
        if !out.is_null() { *out = std::ptr::null_mut(); }
        return 0x8007_04CFu32 as i32; // HRESULT_FROM_WIN32(ERROR_NETWORK_UNREACHABLE)
    }
    let hr = forward(b"QueryApiImpl\0", [clsid, iid, out as usize, d, e, f, g, h]);
    if clsid != 0 && iid != 0 && trace_on() {
        static SEEN: std::sync::Mutex<Vec<(u128, u128)>> = std::sync::Mutex::new(Vec::new());
        let k = ((clsid as *const u128).read_unaligned(), (iid as *const u128).read_unaligned());
        let mut seen = SEEN.lock().unwrap();
        if !seen.contains(&k) {
            seen.push(k);
            ulog(&format!("query clsid {:08x} iid {:08x} hr {:08x}", k.0 as u32, k.1 as u32, hr));
        }
    }
    if hr >= 0 && !out.is_null() && !(*out).is_null() {
        override_interface(clsid, iid, *out);
    }
    hr
}
runtime_forwarder!(InitializeApiImpl);
runtime_forwarder!(InitializeApiImplEx);
runtime_forwarder!(UninitializeApiImpl);
runtime_forwarder!(XErrorReport);
