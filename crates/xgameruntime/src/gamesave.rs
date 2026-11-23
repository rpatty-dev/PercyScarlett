//! Local XGameSave provider for the offline user. The PC runtime needs an Xbox Live user to
//! initialize a provider (0x8083000C otherwise), so titles that keep worlds and settings in
//! XGameSave cannot start. Containers are directories, blobs are files, under
//! %LOCALAPPDATA%\PercyScarlett\saves\<configuration id>\.
//!
//! Slots (CLSID 704c3f58): 3 InitializeProvider, 4/5 ...Async/Result, 6 CloseProvider,
//! 7 GetRemainingQuota, 8/9 ...Async/Result, 10 DeleteContainer, 11/12 ...Async/Result,
//! 13 GetContainerInfo, 14 EnumerateContainerInfo, 15 EnumerateContainerInfoByName,
//! 16 CreateContainer, 17 CloseContainer, 18 EnumerateBlobInfo, 19 EnumerateBlobInfoByName,
//! 20 ReadBlobData, 21/22 ...Async/Result, 23 CreateUpdate, 24 CloseUpdate, 25 SubmitBlobWrite,
//! 26 SubmitBlobDelete, 27 SubmitUpdate, 28/29 ...Async/Result.
use std::collections::HashMap;
use std::ffi::{c_void, CStr};
use std::path::PathBuf;
use std::sync::Mutex;

type P = *mut c_void;
const S_OK: i32 = 0;
const E_INVALIDARG: i32 = 0x8007_0057u32 as i32;
const E_NOT_FOUND: i32 = 0x8007_0490u32 as i32; // HRESULT_FROM_WIN32(ERROR_NOT_FOUND)
const E_INSUFFICIENT_BUFFER: i32 = 0x8007_007Au32 as i32;
const QUOTA: i64 = 16 << 30;

struct Provider {
    root: PathBuf,
}
struct Container {
    dir: PathBuf,
    name: String,
}
struct Update {
    dir: PathBuf,
    writes: Vec<(String, Option<Vec<u8>>)>,
}

static ASYNC_RESULTS: Mutex<Vec<(usize, AsyncResult)>> = Mutex::new(Vec::new());
enum AsyncResult {
    Provider(usize),
    Quota(i64),
    Blobs(Vec<(String, Vec<u8>)>),
}

unsafe fn cstr(p: *const u8) -> String {
    if p.is_null() {
        return String::new();
    }
    CStr::from_ptr(p as *const i8).to_string_lossy().into_owned()
}
/// File-system safe name (titles use '/' and other characters in blob names).
fn enc(name: &str) -> String {
    let mut s = String::new();
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.".contains(&b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("%{b:02X}"));
        }
    }
    s
}
fn dec(name: &str) -> String {
    let b = name.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() + 0 && i + 2 <= b.len() - 1 {
            if let Ok(v) = u8::from_str_radix(&name[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn root_for(config: &str) -> PathBuf {
    let r = crate::data_base().join("saves").join(enc(if config.is_empty() { "default" } else { config }));
    let _ = std::fs::create_dir_all(&r);
    r
}

fn blobs_of(dir: &PathBuf) -> Vec<(String, u64)> {
    let mut v: Vec<(String, u64)> = std::fs::read_dir(dir)
        .map(|it| it.filter_map(|e| e.ok()).filter(|e| e.path().is_file()).map(|e| (dec(&e.file_name().to_string_lossy()), e.metadata().map(|m| m.len()).unwrap_or(0))).collect())
        .unwrap_or_default();
    v.sort();
    v
}

fn containers_of(root: &PathBuf) -> Vec<(String, PathBuf)> {
    let mut v: Vec<(String, PathBuf)> = std::fs::read_dir(root)
        .map(|it| it.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()).map(|e| (dec(&e.file_name().to_string_lossy()), e.path())).collect())
        .unwrap_or_default();
    v.sort();
    v
}

/// XGameSaveContainerInfo { name, displayName, blobCount u32, totalSize u64, lastModifiedTime i64, needsSync bool }
unsafe fn with_container_info(name: &str, dir: &PathBuf, f: &mut dyn FnMut(*const u8) -> bool) -> bool {
    let blobs = blobs_of(dir);
    let n = std::ffi::CString::new(name).unwrap_or_default();
    let mut info = [0u8; 48];
    *(info.as_mut_ptr() as *mut *const i8) = n.as_ptr();
    *(info.as_mut_ptr().add(8) as *mut *const i8) = n.as_ptr();
    *(info.as_mut_ptr().add(16) as *mut u32) = blobs.len() as u32;
    *(info.as_mut_ptr().add(24) as *mut u64) = blobs.iter().map(|b| b.1).sum();
    let mtime = std::fs::metadata(dir).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0);
    *(info.as_mut_ptr().add(32) as *mut i64) = mtime;
    f(info.as_ptr())
}

type InfoCb = unsafe extern "system" fn(*const u8, P) -> u8;

// ---- provider ----
unsafe fn new_provider(config: &str) -> usize {
    let p = Box::into_raw(Box::new(Provider { root: root_for(config) })) as usize;
    super::ulog(&format!("XGameSave: local provider '{config}' -> {}", root_for(config).display()));
    p
}
pub unsafe extern "system" fn init_provider(_t: P, _user: P, config: *const u8, _sync: u8, out: *mut P) -> i32 {
    if out.is_null() {
        return E_INVALIDARG;
    }
    *out = new_provider(&cstr(config)) as P;
    S_OK
}
pub unsafe extern "system" fn init_provider_async(_t: P, _user: P, config: *const u8, _sync: u8, async_block: P) -> i32 {
    log_n("init async");
    let p = new_provider(&cstr(config));
    ASYNC_RESULTS.lock().unwrap().push((async_block as usize, AsyncResult::Provider(p)));
    super::complete_async_with(async_block, S_OK)
}
pub unsafe extern "system" fn init_provider_result(_t: P, async_block: P, out: *mut P) -> i32 {
    match take(async_block) {
        Some(AsyncResult::Provider(p)) if !out.is_null() => {
            *out = p as P;
            S_OK
        }
        _ => E_INVALIDARG,
    }
}
unsafe fn take(async_block: P) -> Option<AsyncResult> {
    let mut v = ASYNC_RESULTS.lock().unwrap();
    let i = v.iter().position(|(a, _)| *a == async_block as usize)?;
    Some(v.remove(i).1)
}
pub unsafe extern "system" fn close_provider(_t: P, _p: P) {}
pub unsafe extern "system" fn quota(_t: P, _p: P, out: *mut i64) -> i32 {
    log_n("quota");
    if !out.is_null() {
        *out = QUOTA;
    }
    S_OK
}
pub unsafe extern "system" fn quota_async(_t: P, _p: P, async_block: P) -> i32 {
    log_n("quota async");
    ASYNC_RESULTS.lock().unwrap().push((async_block as usize, AsyncResult::Quota(QUOTA)));
    super::complete_async_with(async_block, S_OK)
}
pub unsafe extern "system" fn quota_result(_t: P, async_block: P, out: *mut i64) -> i32 {
    if let Some(AsyncResult::Quota(q)) = take(async_block) {
        if !out.is_null() {
            *out = q;
        }
    }
    S_OK
}
unsafe fn prov<'a>(p: P) -> Option<&'a Provider> {
    (p as *const Provider).as_ref()
}
pub unsafe extern "system" fn delete_container(_t: P, p: P, name: *const u8) -> i32 {
    log_n("delete container");
    let Some(p) = prov(p) else { return E_INVALIDARG };
    let _ = std::fs::remove_dir_all(p.root.join(enc(&cstr(name))));
    S_OK
}
pub unsafe extern "system" fn delete_container_async(t: P, p: P, name: *const u8, async_block: P) -> i32 {
    let hr = delete_container(t, p, name);
    super::complete_async_with(async_block, hr)
}
pub unsafe extern "system" fn delete_container_result(_t: P, _a: P) -> i32 {
    S_OK
}
pub unsafe extern "system" fn get_container_info(_t: P, p: P, name: *const u8, ctx: P, cb: InfoCb) -> i32 {
    log_n("get container info");
    let Some(p) = prov(p) else { return E_INVALIDARG };
    let n = cstr(name);
    let dir = p.root.join(enc(&n));
    if !dir.is_dir() {
        return E_NOT_FOUND;
    }
    with_container_info(&n, &dir, &mut |i| cb(i, ctx) != 0);
    S_OK
}
pub unsafe extern "system" fn enum_container_info(_t: P, p: P, ctx: P, cb: InfoCb) -> i32 {
    enum_containers(p, "", ctx, cb)
}
pub unsafe extern "system" fn enum_container_info_by_name(_t: P, p: P, prefix: *const u8, ctx: P, cb: InfoCb) -> i32 {
    enum_containers(p, &cstr(prefix), ctx, cb)
}
unsafe fn enum_containers(p: P, prefix: &str, ctx: P, cb: InfoCb) -> i32 {
    log_n("enum containers");
    let Some(p) = prov(p) else { return E_INVALIDARG };
    for (name, dir) in containers_of(&p.root) {
        if !name.starts_with(prefix) {
            continue;
        }
        if !with_container_info(&name, &dir, &mut |i| cb(i, ctx) != 0) {
            break;
        }
    }
    S_OK
}

// ---- containers ----
pub unsafe extern "system" fn create_container(_t: P, p: P, name: *const u8, out: *mut P) -> i32 {
    log_n("create container");
    let Some(p) = prov(p) else { return E_INVALIDARG };
    if out.is_null() {
        return E_INVALIDARG;
    }
    let n = cstr(name);
    *out = Box::into_raw(Box::new(Container { dir: p.root.join(enc(&n)), name: n })) as P;
    S_OK
}
pub unsafe extern "system" fn close_container(_t: P, c: P) {
    if !c.is_null() {
        drop(Box::from_raw(c as *mut Container));
    }
}
unsafe fn cont<'a>(c: P) -> Option<&'a Container> {
    (c as *const Container).as_ref()
}
type BlobCb = unsafe extern "system" fn(*const u8, P) -> u8;
unsafe fn enum_blobs(c: P, prefix: &str, ctx: P, cb: BlobCb) -> i32 {
    log_n("enum blobs");
    let Some(c) = cont(c) else { return E_INVALIDARG };
    for (name, size) in blobs_of(&c.dir) {
        if !name.starts_with(prefix) {
            continue;
        }
        let n = std::ffi::CString::new(name).unwrap_or_default();
        let mut info = [0u8; 16]; // XGameSaveBlobInfo { name, size u32 }
        *(info.as_mut_ptr() as *mut *const i8) = n.as_ptr();
        *(info.as_mut_ptr().add(8) as *mut u32) = size as u32;
        if cb(info.as_ptr(), ctx) == 0 {
            break;
        }
    }
    S_OK
}
pub unsafe extern "system" fn enum_blob_info(_t: P, c: P, ctx: P, cb: BlobCb) -> i32 {
    enum_blobs(c, "", ctx, cb)
}
pub unsafe extern "system" fn enum_blob_info_by_name(_t: P, c: P, prefix: *const u8, ctx: P, cb: BlobCb) -> i32 {
    enum_blobs(c, &cstr(prefix), ctx, cb)
}

unsafe fn load_blobs(c: &Container, names: *const *const u8, count: u32) -> Vec<(String, Vec<u8>)> {
    let wanted: Vec<String> = if names.is_null() || count == 0 {
        blobs_of(&c.dir).into_iter().map(|b| b.0).collect()
    } else {
        (0..count as usize).map(|i| cstr(*names.add(i))).collect()
    };
    wanted.into_iter().filter_map(|n| std::fs::read(c.dir.join(enc(&n))).ok().map(|d| (n, d))).collect()
}
/// Packs XGameSaveBlob { info { name, size }, data } entries, then names and data, into `buf`.
unsafe fn pack(blobs: &[(String, Vec<u8>)], size: usize, buf: *mut u8, count_out: *mut u32) -> i32 {
    let head = blobs.len() * 24;
    let need = head + blobs.iter().map(|(n, d)| n.len() + 1 + d.len()).sum::<usize>();
    if buf.is_null() || size < need {
        if !count_out.is_null() {
            *count_out = 0;
        }
        return E_INSUFFICIENT_BUFFER;
    }
    let mut off = head;
    for (i, (n, d)) in blobs.iter().enumerate() {
        let e = buf.add(i * 24);
        let np = buf.add(off);
        std::ptr::copy_nonoverlapping(n.as_ptr(), np, n.len());
        *np.add(n.len()) = 0;
        off += n.len() + 1;
        let dp = buf.add(off);
        std::ptr::copy_nonoverlapping(d.as_ptr(), dp, d.len());
        off += d.len();
        *(e as *mut *const u8) = np;
        *(e.add(8) as *mut u32) = d.len() as u32;
        *(e.add(16) as *mut *const u8) = dp;
    }
    if !count_out.is_null() {
        *count_out = blobs.len() as u32;
    }
    S_OK
}
pub unsafe extern "system" fn read_blob_data(_t: P, c: P, names: *const *const u8, count_io: *mut u32, size: usize, buf: *mut u8) -> i32 {
    log_n("read blobs");
    let Some(c) = cont(c) else { return E_INVALIDARG };
    let n = if count_io.is_null() { 0 } else { *count_io };
    let blobs = load_blobs(c, names, n);
    pack(&blobs, size, buf, count_io)
}
pub unsafe extern "system" fn read_blob_data_async(_t: P, c: P, names: *const *const u8, count: u32, async_block: P) -> i32 {
    log_n("read blobs async");
    let Some(c) = cont(c) else { return E_INVALIDARG };
    let blobs = load_blobs(c, names, count);
    ASYNC_RESULTS.lock().unwrap().push((async_block as usize, AsyncResult::Blobs(blobs)));
    super::complete_async_with(async_block, S_OK)
}
pub unsafe extern "system" fn read_blob_data_result(_t: P, async_block: P, _expected: u32, size: usize, buf: *mut u8, count: *mut u32) -> i32 {
    match take(async_block) {
        Some(AsyncResult::Blobs(b)) => pack(&b, size, buf, count),
        _ => E_INVALIDARG,
    }
}

// ---- updates ----
pub unsafe extern "system" fn create_update(_t: P, c: P, _display: *const u8, out: *mut P) -> i32 {
    let Some(c) = cont(c) else { return E_INVALIDARG };
    if out.is_null() {
        return E_INVALIDARG;
    }
    *out = Box::into_raw(Box::new(Update { dir: c.dir.clone(), writes: Vec::new() })) as P;
    let _ = &c.name;
    S_OK
}
pub unsafe extern "system" fn close_update(_t: P, u: P) {
    if !u.is_null() {
        drop(Box::from_raw(u as *mut Update));
    }
}
pub unsafe extern "system" fn submit_blob_write(_t: P, u: P, name: *const u8, data: *const u8, size: usize) -> i32 {
    let Some(u) = (u as *mut Update).as_mut() else { return E_INVALIDARG };
    let d = if data.is_null() { Vec::new() } else { std::slice::from_raw_parts(data, size).to_vec() };
    u.writes.push((cstr(name), Some(d)));
    S_OK
}
pub unsafe extern "system" fn submit_blob_delete(_t: P, u: P, name: *const u8) -> i32 {
    let Some(u) = (u as *mut Update).as_mut() else { return E_INVALIDARG };
    u.writes.push((cstr(name), None));
    S_OK
}
pub unsafe extern "system" fn submit_update(_t: P, u: P) -> i32 {
    log_n("submit update");
    let Some(u) = (u as *mut Update).as_mut() else { return E_INVALIDARG };
    let _ = std::fs::create_dir_all(&u.dir);
    for (n, d) in u.writes.drain(..) {
        let path = u.dir.join(enc(&n));
        match d {
            Some(d) => {
                let tmp = path.with_extension("percytmp");
                if std::fs::write(&tmp, &d).is_ok() {
                    let _ = std::fs::rename(&tmp, &path);
                }
            }
            None => {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    S_OK
}
pub unsafe extern "system" fn submit_update_async(t: P, u: P, async_block: P) -> i32 {
    let hr = submit_update(t, u);
    super::complete_async_with(async_block, hr)
}
pub unsafe extern "system" fn submit_update_result(_t: P, _a: P) -> i32 {
    S_OK
}

pub fn overrides() -> Vec<(usize, usize)> {
    vec![
        (3, init_provider as usize),
        (4, init_provider_async as usize),
        (5, init_provider_result as usize),
        (6, close_provider as usize),
        (7, quota as usize),
        (8, quota_async as usize),
        (9, quota_result as usize),
        (10, delete_container as usize),
        (11, delete_container_async as usize),
        (12, delete_container_result as usize),
        (13, get_container_info as usize),
        (14, enum_container_info as usize),
        (15, enum_container_info_by_name as usize),
        (16, create_container as usize),
        (17, close_container as usize),
        (18, enum_blob_info as usize),
        (19, enum_blob_info_by_name as usize),
        (20, read_blob_data as usize),
        (21, read_blob_data_async as usize),
        (22, read_blob_data_result as usize),
        (23, create_update as usize),
        (24, close_update as usize),
        (25, submit_blob_write as usize),
        (26, submit_blob_delete as usize),
        (27, submit_update as usize),
        (28, submit_update_async as usize),
        (29, submit_update_result as usize),
    ]
}

#[allow(dead_code)]
fn _unused(_: HashMap<u8, u8>) {}

fn log_n(what: &str) {
    static SEEN: Mutex<Vec<(String, u32)>> = Mutex::new(Vec::new());
    let mut s = SEEN.lock().unwrap();
    let e = match s.iter_mut().position(|x| x.0 == what) { Some(i) => i, None => { s.push((what.to_string(), 0)); s.len() - 1 } };
    s[e].1 += 1;
    if s[e].1 <= 3 { super::ulog(&format!("XGameSave {what} #{}", s[e].1)); }
}
