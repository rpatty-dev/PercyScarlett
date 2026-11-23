//! Offline loopback `XCurl.dll` (libcurl ABI, the 16 exports titles import).
//!
//! Every transfer completes locally with `200 OK` and a small JSON body; Xbox Live auth hosts
//! get a synthetic XSTS reply. Titles whose sign-in path needs an HTTP answer (e.g. PlayFab
//! login) then take their offline route instead of failing on the real service.
//!
//! Ported from OpenScarlett `src/shims/xcurl_shim.cpp` (`PerformOfflineTransfer`) by
//! californiaAlmonds. MIT License, Copyright (c) 2026 OpenScarlett Contributors.
#![allow(non_snake_case, clippy::missing_safety_doc)]
use std::ffi::{c_char, c_void, CStr, CString};

type WriteCb = unsafe extern "C" fn(*const c_char, usize, usize, *mut c_void) -> usize;

const CURLE_OK: i32 = 0;
const CURLE_URL_MALFORMAT: i32 = 3;
const CURLE_WRITE_ERROR: i32 = 23;
const CURLE_BAD_FUNCTION_ARGUMENT: i32 = 43;

#[derive(Default)]
pub struct Easy {
    url: String,
    method: String,
    post_size: usize,
    nobody: bool,
    write_cb: Option<WriteCb>,
    write_data: usize,
    header_cb: Option<WriteCb>,
    header_data: usize,
    private: usize,
    header_size: i32,
    size_download: i64,
    effective_url: CString,
}

fn host_lower(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let auth = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = auth.rsplit('@').next().unwrap_or("");
    host.split(':').next().unwrap_or("").to_ascii_lowercase()
}

fn b64url(d: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut o = String::new();
    for c in d.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..=c.len() {
            o.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    o
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// JSON body returned for a request.
pub fn offline_body(url: &str, method: &str, post: usize) -> String {
    let tail = format!(
        "\"service\":\"percy-xcurl\",\"offline\":true,\"status\":200,\"method\":\"{}\",\"url\":\"{}\",\"postBytes\":{post}}}",
        esc(method),
        esc(url)
    );
    // PlayFab LoginWithXbox needs a session ticket and entity token to proceed (offline stand-in).
    if url.contains("LoginWithXbox") {
        return "{\"code\":200,\"status\":\"OK\",\"data\":{\"PlayFabId\":\"0000000000000001\",\"SessionTicket\":\"percy-offline-ticket\",\"NewlyCreated\":false,\"EntityToken\":{\"EntityToken\":\"percy-offline\",\"TokenExpiration\":\"2099-01-01T00:00:00Z\",\"Entity\":{\"Id\":\"0000000000000001\",\"Type\":\"master_player_account\"}},\"SettingsForUser\":{\"NeedsAttribution\":false,\"GatherDeviceInfo\":false,\"GatherFocusInfo\":false},\"LastLoginTime\":\"2026-01-01T00:00:00Z\"}}".into();
    }
    match host_lower(url).as_str() {
        "xsts.auth.xboxlive.com" | "user.auth.xboxlive.com" | "device.auth.xboxlive.com" | "title.auth.xboxlive.com" | "sisu.xboxlive.com" => {
            let jwt = format!(
                "{}.{}.{}",
                b64url(br#"{"alg":"ES256","typ":"JWT"}"#),
                b64url(br#"{"xuid":"2533274790396727","uhs":"0000000000000001","gtg":"Player","agg":"Adult","iss":"https://xsts.auth.xboxlive.com","offline":true}"#),
                b64url(b"offline")
            );
            format!(
                "{{\"IssueInstant\":\"2026-01-01T00:00:00.0000000Z\",\"NotAfter\":\"2099-01-01T00:00:00.0000000Z\",\"Token\":\"{jwt}\",\"DisplayClaims\":{{\"xui\":[{{\"uhs\":\"0000000000000001\",\"xid\":\"2533274790396727\",\"gtg\":\"Player\",\"agg\":\"Adult\",\"usr\":\"191\",\"prv\":\"191\"}}]}},{tail}"
            )
        }
        "title.mgt.xboxlive.com" => format!("{{\"titleId\":\"5CA00001\",\"scid\":\"00000000-0000-0000-0000-00005ca00001\",{tail}"),
        // Other Xbox services (session directory, privacy, people, profile): empty collections under
        // every key their clients read, so parsers see "nothing here" rather than a missing field.
        h if h.ends_with(".xboxlive.com") => format!(
            "{{\"results\":[],\"users\":[],\"people\":[],\"items\":[],\"profileUsers\":[],\"devices\":[],\"titles\":[],\"subscriptions\":[],\"pagingInfo\":{{\"totalItems\":0}},{tail}"
        ),
        _ => format!("{{{tail}"),
    }
}

unsafe fn perform(e: &mut Easy) -> i32 {
    if e.url.is_empty() {
        return CURLE_URL_MALFORMAT;
    }
    let method = if e.method.is_empty() { "GET" } else { &e.method };
    let body = if e.nobody { String::new() } else { offline_body(&e.url, method, e.post_size) };
    log(&format!("{method} {} -> 200 offline", e.url));
    let hdrs = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nX-Percy-Offline: 1\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    e.header_size = hdrs.len() as i32;
    e.size_download = body.len() as i64;
    e.effective_url = CString::new(e.url.clone()).unwrap_or_default();
    if let Some(cb) = e.header_cb {
        for line in hdrs.split_inclusive('\n') {
            if cb(line.as_ptr() as _, 1, line.len(), e.header_data as _) != line.len() {
                return CURLE_WRITE_ERROR;
            }
        }
    }
    if let (Some(cb), false) = (e.write_cb, body.is_empty()) {
        if cb(body.as_ptr() as _, 1, body.len(), e.write_data as _) != body.len() {
            return CURLE_WRITE_ERROR;
        }
    }
    CURLE_OK
}

fn log(s: &str) {
    #[cfg(windows)]
    {
        extern "system" {
            fn OutputDebugStringA(s: *const u8);
        }
        let mut v = format!("[xcurl] {s}").into_bytes();
        v.push(0);
        unsafe { OutputDebugStringA(v.as_ptr()) };
    }
}

unsafe fn cstr(p: usize) -> String {
    if p == 0 {
        String::new()
    } else {
        CStr::from_ptr(p as *const c_char).to_string_lossy().into_owned()
    }
}

#[no_mangle]
pub extern "C" fn curl_global_init(_flags: i64) -> i32 {
    CURLE_OK
}
#[no_mangle]
pub extern "C" fn curl_global_cleanup() {}

#[no_mangle]
pub extern "C" fn curl_easy_init() -> *mut Easy {
    Box::into_raw(Box::default())
}
#[no_mangle]
pub unsafe extern "C" fn curl_easy_cleanup(h: *mut Easy) {
    if !h.is_null() {
        drop(Box::from_raw(h));
    }
}

/// Variadic in C; on Win64 the one argument arrives in r8 exactly like a fixed third parameter.
#[no_mangle]
pub unsafe extern "C" fn curl_easy_setopt(h: *mut Easy, opt: i32, arg: usize) -> i32 {
    let Some(e) = h.as_mut() else { return CURLE_BAD_FUNCTION_ARGUMENT };
    let on = arg as u32 != 0;
    match opt {
        10002 => e.url = cstr(arg),            // URL
        10036 => e.method = cstr(arg),         // CUSTOMREQUEST
        47 if on => e.method = "POST".into(),  // POST
        54 | 46 if on => e.method = "PUT".into(), // PUT / UPLOAD
        80 if on => e.method = "GET".into(),   // HTTPGET
        44 => e.nobody = on,                   // NOBODY
        10015 | 10165 => {
            // POSTFIELDS / COPYPOSTFIELDS
            if e.method.is_empty() {
                e.method = "POST".into();
            }
            if e.post_size == 0 && arg != 0 {
                e.post_size = cstr(arg).len();
            }
        }
        60 | 14 => e.post_size = arg as u32 as usize, // POSTFIELDSIZE / INFILESIZE
        30120 | 30115 => e.post_size = arg,          // POSTFIELDSIZE_LARGE / INFILESIZE_LARGE
        20011 => e.write_cb = std::mem::transmute::<usize, Option<WriteCb>>(arg),
        10001 => e.write_data = arg,
        20079 => e.header_cb = std::mem::transmute::<usize, Option<WriteCb>>(arg),
        10029 => e.header_data = arg,
        10103 => e.private = arg, // PRIVATE
        _ => {}
    }
    CURLE_OK
}

#[no_mangle]
pub unsafe extern "C" fn curl_easy_getinfo(h: *mut Easy, info: i32, out: *mut c_void) -> i32 {
    let (Some(e), false) = (h.as_ref(), out.is_null()) else { return CURLE_BAD_FUNCTION_ARGUMENT };
    match info & 0xF0_0000 {
        0x10_0000 => {
            *(out as *mut usize) = match info {
                0x10_0001 => e.effective_url.as_ptr() as usize,     // EFFECTIVE_URL
                0x10_0012 => c"application/json".as_ptr() as usize, // CONTENT_TYPE
                0x10_0015 => e.private,                             // PRIVATE
                0x10_0031 => c"https".as_ptr() as usize,            // SCHEME
                _ => 0,
            }
        }
        0x20_0000 => {
            *(out as *mut i32) = match info {
                0x20_0002 => 200,           // RESPONSE_CODE
                0x20_000B => e.header_size, // HEADER_SIZE
                _ => 0,
            }
        }
        0x30_0000 => *(out as *mut f64) = if info == 0x30_0008 { e.size_download as f64 } else { 0.0 },
        0x60_0000 => *(out as *mut i64) = if matches!(info, 0x60_0008 | 0x60_000F) { e.size_download } else { 0 },
        _ => *(out as *mut usize) = 0,
    }
    CURLE_OK
}

#[no_mangle]
pub extern "C" fn curl_easy_strerror(code: i32) -> *const c_char {
    if code == 0 { c"No error".as_ptr() } else { c"offline transfer error".as_ptr() }
}

#[repr(C)]
pub struct Slist {
    data: *mut c_char,
    next: *mut Slist,
}
#[no_mangle]
pub unsafe extern "C" fn curl_slist_append(list: *mut Slist, s: *const c_char) -> *mut Slist {
    if s.is_null() {
        return std::ptr::null_mut();
    }
    let node = Box::into_raw(Box::new(Slist { data: CStr::from_ptr(s).to_owned().into_raw(), next: std::ptr::null_mut() }));
    if list.is_null() {
        return node;
    }
    let mut p = list;
    while !(*p).next.is_null() {
        p = (*p).next;
    }
    (*p).next = node;
    list
}
#[no_mangle]
pub unsafe extern "C" fn curl_slist_free_all(mut p: *mut Slist) {
    while !p.is_null() {
        let b = Box::from_raw(p);
        drop(CString::from_raw(b.data));
        p = b.next;
    }
}

#[repr(C)]
pub struct Msg {
    msg: i32,
    easy: *mut Easy,
    result: usize,
}
#[derive(Default)]
pub struct Multi {
    pending: Vec<usize>,
    done: std::collections::VecDeque<(usize, i32)>,
    cur: Option<Box<Msg>>,
}

#[no_mangle]
pub extern "C" fn curl_multi_init() -> *mut Multi {
    Box::into_raw(Box::default())
}
#[no_mangle]
pub unsafe extern "C" fn curl_multi_cleanup(m: *mut Multi) -> i32 {
    if !m.is_null() {
        drop(Box::from_raw(m));
    }
    0
}
#[no_mangle]
pub unsafe extern "C" fn curl_multi_add_handle(m: *mut Multi, e: *mut Easy) -> i32 {
    let Some(m) = m.as_mut() else { return 1 };
    if e.is_null() {
        return 2;
    }
    m.pending.push(e as usize);
    0
}
#[no_mangle]
pub unsafe extern "C" fn curl_multi_remove_handle(m: *mut Multi, e: *mut Easy) -> i32 {
    let Some(m) = m.as_mut() else { return 1 };
    m.pending.retain(|&p| p != e as usize);
    m.done.retain(|&(p, _)| p != e as usize);
    0
}
#[no_mangle]
pub unsafe extern "C" fn curl_multi_perform(m: *mut Multi, running: *mut i32) -> i32 {
    let Some(m) = m.as_mut() else { return 1 };
    for p in std::mem::take(&mut m.pending) {
        let r = perform(&mut *(p as *mut Easy));
        m.done.push_back((p, r));
    }
    if !running.is_null() {
        *running = 0;
    }
    0
}
#[no_mangle]
pub unsafe extern "C" fn curl_multi_info_read(m: *mut Multi, left: *mut i32) -> *mut Msg {
    let Some(m) = m.as_mut() else { return std::ptr::null_mut() };
    let next = m.done.pop_front();
    if !left.is_null() {
        *left = m.done.len() as i32;
    }
    let Some((e, r)) = next else { return std::ptr::null_mut() };
    &mut **m.cur.insert(Box::new(Msg { msg: 1 /* CURLMSG_DONE */, easy: e as *mut Easy, result: r as u32 as usize }))
}
/// Nothing is ever in flight: wait at most 50 ms so idle loops neither spin nor hang.
#[no_mangle]
pub unsafe extern "C" fn curl_multi_poll(m: *mut Multi, _fds: *mut c_void, _n: u32, timeout_ms: i32, numfds: *mut i32) -> i32 {
    if !numfds.is_null() {
        *numfds = 0;
    }
    if m.as_ref().is_some_and(|m| m.pending.is_empty() && m.done.is_empty()) {
        std::thread::sleep(std::time::Duration::from_millis(timeout_ms.clamp(0, 50) as u64));
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    unsafe extern "C" fn sink(p: *const c_char, s: usize, n: usize, d: *mut c_void) -> usize {
        (*(d as *mut Vec<u8>)).extend_from_slice(std::slice::from_raw_parts(p as *const u8, s * n));
        s * n
    }
    #[test]
    fn multi_roundtrip() {
        unsafe {
            let mut body: Vec<u8> = Vec::new();
            let e = curl_easy_init();
            curl_easy_setopt(e, 10002, c"https://xsts.auth.xboxlive.com/xsts/authorize".as_ptr() as usize);
            curl_easy_setopt(e, 20011, sink as usize);
            curl_easy_setopt(e, 10001, &mut body as *mut _ as usize);
            curl_easy_setopt(e, 10103, 0x1234);
            let m = curl_multi_init();
            curl_multi_add_handle(m, e);
            let mut run = 1;
            curl_multi_perform(m, &mut run);
            let msg = &*curl_multi_info_read(m, &mut run);
            assert_eq!((msg.msg, msg.easy, msg.result, run), (1, e, 0, 0));
            assert!(curl_multi_info_read(m, &mut run).is_null());
            let (mut code, mut private) = (0i32, 0usize);
            curl_easy_getinfo(e, 0x20_0002, &mut code as *mut _ as _);
            curl_easy_getinfo(e, 0x10_0015, &mut private as *mut _ as _);
            assert_eq!((code, private), (200, 0x1234));
            assert!(String::from_utf8(body).unwrap().contains("\"Token\":\"eyJ"));
            assert_eq!(host_lower("https://u:p@B980A380.minecraft.playfabapi.com:443/x?y"), "b980a380.minecraft.playfabapi.com");
            let l = curl_slist_append(std::ptr::null_mut(), c"A: 1".as_ptr());
            curl_slist_append(l, c"B: 2".as_ptr());
            curl_slist_free_all(l);
            curl_multi_cleanup(m);
            curl_easy_cleanup(e);
        }
    }
}
