use std::collections::HashMap;
use std::ffi::{c_void, CString};
use std::path::Path;

#[cfg(windows)]
extern "system" {
    fn VirtualAlloc(addr: usize, size: usize, alloc_type: u32, protect: u32) -> usize;
    fn VirtualProtect(addr: *mut c_void, size: usize, new_protect: u32, old_protect: *mut u32) -> i32;
    fn LoadLibraryA(name: *const i8) -> usize;
    fn GetProcAddress(module: usize, name: *const i8) -> usize;
    fn RtlAddFunctionTable(function_table: *const c_void, entry_count: u32, base_address: u64) -> u8;
    fn RtlPcToFileHeader(pc_value: *const c_void, base_of_image: *mut *mut c_void) -> *mut c_void;
    fn RtlCaptureStackBackTrace(skip: u32, n: u32, out: *mut usize, hash: *mut u32) -> u16;
    fn AddVectoredExceptionHandler(first: u32, handler: unsafe extern "system" fn(*const c_void) -> i32) -> *mut c_void;
    fn GetModuleFileNameW(module: usize, buf: *mut u16, size: u32) -> u32;
    fn GetModuleFileNameA(module: usize, buf: *mut u8, size: u32) -> u32;
    fn GetModuleHandleW(name: *const u16) -> usize;
    fn GetModuleHandleA(name: *const u8) -> usize;
    fn CreateFileW(
        name: *const u16,
        access: u32,
        share: u32,
        sa: *const c_void,
        disp: u32,
        flags: u32,
        tmpl: usize,
    ) -> usize;
    fn CreateFileA(
        name: *const u8,
        access: u32,
        share: u32,
        sa: *const c_void,
        disp: u32,
        flags: u32,
        tmpl: usize,
    ) -> usize;
    fn GetFileAttributesW(name: *const u16) -> u32;
    fn GetDiskFreeSpaceExW(dir: *const u16, avail: *mut u64, total: *mut u64, free: *mut u64) -> i32;
    fn CreateDirectoryW(name: *const u16, sa: *const c_void) -> i32;
    fn GetFileAttributesExW(name: *const u16, level: u32, info: *mut c_void) -> i32;
    fn FindFirstFileExW(name: *const u16, level: u32, data: *mut c_void, op: u32, filter: *const c_void, flags: u32) -> isize;
    fn FindFirstFileW(name: *const u16, data: *mut c_void) -> isize;
    fn MoveFileExW(src: *const u16, dst: *const u16, flags: u32) -> i32;
    fn DeleteFileW(name: *const u16) -> i32;
    fn RemoveDirectoryW(name: *const u16) -> i32;
    fn LoadLibraryExW(name: *const u16, file: usize, flags: u32) -> usize;
    fn SetLastError(code: u32);
}

/// Redirects Xbox console virtual paths (`G:\`, `GAME:\`, `T:\`, `S:\`) to host folders.
pub fn rewrite_xbox_vfs_path(raw: &str, game_dir: &Path, save_root: &Path) -> Option<std::path::PathBuf> {
    let trimmed = raw.trim_start_matches(r"\\?\");
    let lower = trimmed.to_ascii_lowercase();
    let gd_lower = game_dir.to_string_lossy().to_ascii_lowercase();
    let gd_norm = gd_lower.trim_end_matches(['\\', '/']);
    if !gd_norm.is_empty() && (lower == gd_norm || lower.starts_with(&format!("{gd_norm}\\")) || lower.starts_with(&format!("{gd_norm}/"))) {
        return None;
    }
    let (rest, base) = if let Some(r) = lower.strip_prefix("g:\\").or_else(|| lower.strip_prefix("g:/")) {
        (&trimmed[trimmed.len() - r.len()..], game_dir.to_path_buf())
    } else if let Some(r) = lower.strip_prefix("game:\\").or_else(|| lower.strip_prefix("game:/")).or_else(|| lower.strip_prefix("game:")) {
        (&trimmed[trimmed.len() - r.len()..], game_dir.to_path_buf())
    } else if let Some(r) = lower.strip_prefix("common:\\").or_else(|| lower.strip_prefix("common:/")).or_else(|| lower.strip_prefix("common:")) {
        (&trimmed[trimmed.len() - r.len()..], game_dir.join("common"))
    } else if let Some(r) = lower.strip_prefix("platform:\\").or_else(|| lower.strip_prefix("platform:/")).or_else(|| lower.strip_prefix("platform:")) {
        (&trimmed[trimmed.len() - r.len()..], game_dir.join("platform"))
    } else if let Some(r) = lower.strip_prefix("t:\\").or_else(|| lower.strip_prefix("t:/")) {
        (&trimmed[trimmed.len() - r.len()..], save_root.join("T"))
    } else if let Some(r) = lower.strip_prefix("s:\\").or_else(|| lower.strip_prefix("s:/")) {
        (&trimmed[trimmed.len() - r.len()..], save_root.join("S"))
    } else if let Some(r) = lower.strip_prefix("d:\\users\\").or_else(|| lower.strip_prefix("d:/users/")) {
        // Console per-user data folder (users/<id>/games/com.mojang): kept in the data root.
        (&trimmed[trimmed.len() - r.len()..], save_root.join("D").join("Users"))
    } else {
        return None;
    };
    let rel = rest.trim_start_matches(['\\', '/']).replace('/', "\\");
    let _ = std::fs::create_dir_all(&base);
    Some(base.join(rel))
}

#[cfg(windows)]
fn vfs_roots() -> (std::path::PathBuf, std::path::PathBuf) {
    let game_dir = GUEST_EXE_PATH
        .lock()
        .ok()
        .and_then(|g| g.as_ref().and_then(|p| {
            let path = Path::new(p);
            let parent = path.parent()?;
            if parent.as_os_str().is_empty() {
                std::env::current_dir().ok()
            } else {
                std::fs::canonicalize(parent).ok().or_else(|| Some(parent.to_path_buf()))
            }
        }))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")));
    // Same data root as the runtime shims: `percy_data_dir.txt` next to the exe, else %LOCALAPPDATA%.
    let save_root = std::env::current_exe()
        .ok()
        .and_then(|e| std::fs::read_to_string(e.with_file_name("percy_data_dir.txt")).ok())
        .map(|s| std::path::PathBuf::from(s.trim()))
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.join("vfs"))
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(|b| std::path::PathBuf::from(b).join("PercyScarlett").join("vfs")))
        .unwrap_or_else(|| std::env::temp_dir().join("percyscarlett_vfs"));
    (game_dir, save_root)
}

#[cfg(windows)]
unsafe extern "system" fn percy_create_file_w(
    name: *const u16,
    access: u32,
    share: u32,
    sa: *const c_void,
    disp: u32,
    flags: u32,
    tmpl: usize,
) -> usize {
    if !name.is_null() {
        let mut len = 0usize;
        while len < 512 && *name.add(len) != 0 {
            len += 1;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(name, len));
        let (gd, sr) = vfs_roots();
        if let Some(mapped) = rewrite_xbox_vfs_path(&s, &gd, &sr) {
            let mut w: Vec<u16> = mapped.to_string_lossy().encode_utf16().collect();
            w.push(0);
            let h = CreateFileW(w.as_ptr(), access, share, sa, disp, flags, tmpl);
            file_trace(&s, h, flags);
            return h;
        }
        let h = CreateFileW(name, access, share, sa, disp, flags, tmpl);
        file_trace(&s, h, flags);
        return h;
    }
    CreateFileW(name, access, share, sa, disp, flags, tmpl)
}

/// PERCY_TRACE: failed opens (first 200) and every 64th open, to spot loads that never finish.
/// Opt-in (`percy_heapval` next to the exe): a watcher thread validates every process heap every
/// 20 ms; on the first failure it walks that heap and dumps the last intact block and what follows.
#[cfg(windows)]
fn heapval(_path: &str) {
    #[repr(C)]
    struct Phe { data: *mut u8, cb: u32, overhead: u8, region: u8, flags: u16, rest: [usize; 4] }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetProcessHeaps(n: u32, out: *mut usize) -> u32;
        fn HeapValidate(h: usize, flags: u32, p: *const c_void) -> i32;
        fn HeapWalk(h: usize, e: *mut Phe) -> i32;
        fn HeapLock(h: usize) -> i32;
        fn HeapUnlock(h: usize) -> i32;
    }
    static STARTED: std::sync::Once = std::sync::Once::new();
    if !std::env::current_exe().map(|e| e.with_file_name("percy_heapval").exists()).unwrap_or(false) {
        return;
    }
    STARTED.call_once(|| unsafe {
        extern "system" {
            fn CreateThread(sa: usize, stack: usize, f: unsafe extern "system" fn(usize) -> u32, p: usize, fl: u32, id: usize) -> usize;
        }
        CreateThread(0, 0, watch, 0, 0, 0);
    });
    unsafe extern "system" fn watch(_: usize) -> u32 {
        guest_tls::restore_current_thread(); // host code below: slot 0 must hold the host block
        {
            let mut hs = [0usize; 64];
            loop {
                std::thread::sleep(std::time::Duration::from_millis(20));
                let n = GetProcessHeaps(64, hs.as_mut_ptr()).min(64) as usize;
                for (i, &h) in hs[..n].iter().enumerate() {
                    if HeapValidate(h, 0, std::ptr::null()) != 0 {
                        continue;
                    }
                    raw_stderr(&format!("[heapval] heap #{i} {h:#x} of {n} corrupt
"));
                    HeapLock(h);
                    let mut e: Phe = std::mem::zeroed();
                    let mut last: (usize, u32, u16) = (0, 0, 0);
                    let mut walked = 0;
                    while HeapWalk(h, &mut e) != 0 {
                        last = (e.data as usize, e.cb, e.flags);
                        walked += 1;
                        if HeapValidate(h, 0, e.data as *const c_void) == 0 && e.flags & 4 != 0 {
                            break; // first busy block that fails its own validation
                        }
                    }
                    HeapUnlock(h);
                    let (p, cb, fl) = last;
                    let dump = |a: usize, n: usize| (0..n).map(|k| format!("{:02x}", *((a + k) as *const u8))).collect::<String>();
                    raw_stderr(&format!("[heapval] after {walked} entries: block {p:#x} size {cb} flags {fl:#x}
[heapval] head {}
[heapval] tail+ {}
",
                        dump(p, 64.min(cb as usize)), dump(p + cb as usize, 64)));
                    let txt: String = (0..(cb as usize).min(160)).map(|k| { let c = *((p + k) as *const u8); if (32..127).contains(&c) { c as char } else { '.' } }).collect();
                    raw_stderr(&format!("[heapval] ascii {txt}
"));
                    loop { std::thread::sleep(std::time::Duration::from_secs(60)); }
                }
            }
        }
    }
}

fn file_trace(path: &str, h: usize, flags: u32) {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    static F: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    heapval(path);
    if h == usize::MAX {
        if F.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 200 {
            raw_stderr(&format!("[file] FAIL {path}\n"));
        }
    } else if n % 64 == 0 {
        raw_stderr(&format!("[file] #{n} {path} flags={flags:#x}\n"));
    }
}

#[cfg(windows)]
unsafe extern "system" fn percy_create_file_a(
    name: *const u8,
    access: u32,
    share: u32,
    sa: *const c_void,
    disp: u32,
    flags: u32,
    tmpl: usize,
) -> usize {
    if !name.is_null() {
        let s = read_cstr_mem(name);
        let (gd, sr) = vfs_roots();
        if let Some(mapped) = rewrite_xbox_vfs_path(&s, &gd, &sr) {
            if let Ok(cs) = CString::new(mapped.to_string_lossy().as_bytes()) {
                let h = CreateFileA(cs.as_ptr() as *const u8, access, share, sa, disp, flags, tmpl);
                file_trace(&s, h, flags);
                return h;
            }
        }
        let h = CreateFileA(name, access, share, sa, disp, flags, tmpl);
        file_trace(&s, h, flags);
        return h;
    }
    CreateFileA(name, access, share, sa, disp, flags, tmpl)
}

#[cfg(windows)]
unsafe extern "system" fn percy_get_file_attributes_w(name: *const u16) -> u32 {
    if !name.is_null() {
        let mut len = 0usize;
        while len < 512 && *name.add(len) != 0 {
            len += 1;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(name, len));
        let (gd, sr) = vfs_roots();
        if let Some(mapped) = rewrite_xbox_vfs_path(&s, &gd, &sr) {
            let mut w: Vec<u16> = mapped.to_string_lossy().encode_utf16().collect();
            w.push(0);
            return GetFileAttributesW(w.as_ptr());
        }
    }
    GetFileAttributesW(name)
}

/// Free-space queries on console drives (`S:\`, `T:\`, `G:\`) are answered for the mapped host
/// folder; a drive the host lacks (the title asks about its console save volume) falls back to the
/// volume holding the data root. Without this the title sees a failed query as "out of storage".
#[cfg(windows)]
unsafe extern "system" fn percy_get_disk_free_space_ex_w(name: *const u16, avail: *mut u64, total: *mut u64, free: *mut u64) -> i32 {
    if !name.is_null() {
        let mut len = 0usize;
        while len < 512 && *name.add(len) != 0 {
            len += 1;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(name, len));
        let (gd, sr) = vfs_roots();
        let target = rewrite_xbox_vfs_path(&s, &gd, &sr);
        static Q: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        if Q.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 8 { raw_stderr(&format!("[disk] query '{s}'
")); }
        let w = |p: &Path| -> Vec<u16> { let mut v: Vec<u16> = p.to_string_lossy().encode_utf16().collect(); v.push(0); v };
        if let Some(m) = &target {
            let _ = std::fs::create_dir_all(m);
            if GetDiskFreeSpaceExW(w(m).as_ptr(), avail, total, free) != 0 { return 1; }
        } else if GetDiskFreeSpaceExW(name, avail, total, free) != 0 {
            return 1;
        }
        let _ = std::fs::create_dir_all(&sr);
        let r = GetDiskFreeSpaceExW(w(&sr).as_ptr(), avail, total, free);
        raw_stderr(&format!("[disk] GetDiskFreeSpaceExW('{s}') -> data root ({r})
"));
        return r;
    }
    GetDiskFreeSpaceExW(name, avail, total, free)
}

/// Wide path -> mapped host path (NUL-terminated), when it names a redirected console location.
#[cfg(windows)]
unsafe fn map_wide(name: *const u16) -> Option<Vec<u16>> {
    if name.is_null() {
        return None;
    }
    let mut len = 0usize;
    while len < 1024 && *name.add(len) != 0 {
        len += 1;
    }
    let s = String::from_utf16_lossy(std::slice::from_raw_parts(name, len));
    let (gd, sr) = vfs_roots();
    let m = rewrite_xbox_vfs_path(&s, &gd, &sr)?;
    let mut w: Vec<u16> = m.to_string_lossy().encode_utf16().collect();
    w.push(0);
    Some(w)
}
#[cfg(windows)]
unsafe extern "system" fn percy_create_directory_w(name: *const u16, sa: *const c_void) -> i32 {
    if let Some(w) = map_wide(name) {
        if let Some(parent) = Path::new(&String::from_utf16_lossy(&w[..w.len() - 1])).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        return CreateDirectoryW(w.as_ptr(), sa);
    }
    CreateDirectoryW(name, sa)
}
#[cfg(windows)]
unsafe extern "system" fn percy_get_file_attributes_ex_w(name: *const u16, level: u32, info: *mut c_void) -> i32 {
    match map_wide(name) {
        Some(w) => GetFileAttributesExW(w.as_ptr(), level, info),
        None => GetFileAttributesExW(name, level, info),
    }
}
#[cfg(windows)]
unsafe extern "system" fn percy_find_first_file_ex_w(name: *const u16, level: u32, data: *mut c_void, op: u32, filter: *const c_void, flags: u32) -> isize {
    match map_wide(name) {
        Some(w) => FindFirstFileExW(w.as_ptr(), level, data, op, filter, flags),
        None => FindFirstFileExW(name, level, data, op, filter, flags),
    }
}
#[cfg(windows)]
unsafe extern "system" fn percy_find_first_file_w(name: *const u16, data: *mut c_void) -> isize {
    match map_wide(name) {
        Some(w) => FindFirstFileW(w.as_ptr(), data),
        None => FindFirstFileW(name, data),
    }
}
#[cfg(windows)]
unsafe extern "system" fn percy_move_file_ex_w(src: *const u16, dst: *const u16, flags: u32) -> i32 {
    let (a, b) = (map_wide(src), map_wide(dst));
    MoveFileExW(a.as_ref().map_or(src, |v| v.as_ptr()), b.as_ref().map_or(dst, |v| v.as_ptr()), flags)
}

#[cfg(windows)]
unsafe extern "system" fn percy_delete_file_w(name: *const u16) -> i32 {
    match map_wide(name) {
        Some(w) => DeleteFileW(w.as_ptr()),
        None => DeleteFileW(name),
    }
}
#[cfg(windows)]
unsafe extern "system" fn percy_remove_directory_w(name: *const u16) -> i32 {
    match map_wide(name) {
        Some(w) => RemoveDirectoryW(w.as_ptr()),
        None => RemoveDirectoryW(name),
    }
}
/// Windows push notifications (wpnapps.dll) cannot register this sideloaded package: its failure
/// throws through a Gaming Services callback and aborts the title. Offline there is nothing to
/// deliver, so the in-proc load is refused and callers see an ordinary "module not found".
#[cfg(windows)]
unsafe extern "system" fn percy_load_library_ex_w(name: *const u16, file: usize, flags: u32) -> usize {
    if !name.is_null() {
        let mut len = 0usize;
        while len < 512 && *name.add(len) != 0 {
            len += 1;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(name, len)).to_ascii_lowercase();
        if s.ends_with("wpnapps.dll") {
            SetLastError(126); // ERROR_MOD_NOT_FOUND
            return 0;
        }
    }
    LoadLibraryExW(name, file, flags)
}

/// File APIs whose console paths need mapping, by import name. Shared by the title's IAT and the
/// VC runtime DLLs (std::filesystem / fopen call these from msvcp140 / ucrtbase, not the title).
#[cfg(windows)]
fn file_hook_for(name: &str) -> Option<usize> {
    Some(match name {
        "CreateFileW" => percy_create_file_w as *const () as usize,
        "GetFileAttributesW" => percy_get_file_attributes_w as *const () as usize,
        "GetFileAttributesExW" => percy_get_file_attributes_ex_w as *const () as usize,
        "CreateDirectoryW" => percy_create_directory_w as *const () as usize,
        "FindFirstFileExW" => percy_find_first_file_ex_w as *const () as usize,
        "FindFirstFileW" => percy_find_first_file_w as *const () as usize,
        "MoveFileExW" => percy_move_file_ex_w as *const () as usize,
        "DeleteFileW" => percy_delete_file_w as *const () as usize,
        "RemoveDirectoryW" => percy_remove_directory_w as *const () as usize,
        "GetDiskFreeSpaceExW" => percy_get_disk_free_space_ex_w as *const () as usize,
        "LoadLibraryExW" => percy_load_library_ex_w as *const () as usize,
        _ => return None,
    })
}

static GUEST_EXE_PATH: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
static GUEST_IMAGE_BASE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub fn set_guest_exe_path(path: &Path) {
    if let Ok(mut g) = GUEST_EXE_PATH.lock() {
        *g = Some(path.to_string_lossy().into_owned());
    }
}

/// Makes the process present the guest as its main image to system components that are not
/// IAT-hooked (e.g. the Gaming Runtime resolving `MicrosoftGame.config` from the process image path):
/// rewrites `PEB.ProcessParameters.ImagePathName`, the main loader entry's Full/BaseDllName, and the CWD.
/// Name of the per-title identity overlay folder created next to the game exe.
pub const IDENTITY_DIR: &str = ".percy_identity";

/// Builds `<game_dir>\.percy_identity\`: the PercyScarlett host + shim DLLs copied under the title's
/// exe name, and a `MicrosoftGame.config` translated for the PC Gaming Runtime (console device
/// families -> `PC`). Registered with `wdapp register`, activating it gives the host real package
/// identity; the host then loads the untouched title exe from the parent folder in-process.
pub fn build_identity_overlay(exe: &Path, host_exe: &Path) -> Result<std::path::PathBuf, String> {
    let dir = exe.parent().ok_or("exe has no parent dir")?;
    let cfg_src = ["MicrosoftGame.Config", "MicrosoftGame.config"]
        .iter()
        .map(|n| dir.join(n))
        .find(|p| p.exists())
        .ok_or("no MicrosoftGame.config next to the title exe")?;
    let cfg = std::fs::read_to_string(&cfg_src).map_err(|e| e.to_string())?;
    let translated = cfg
        .replace("TargetDeviceFamily=\"Scarlett\"", "TargetDeviceFamily=\"PC\"")
        .replace("TargetDeviceFamily=\"XboxOne\"", "TargetDeviceFamily=\"PC\"");
    let ov = dir.join(IDENTITY_DIR);
    std::fs::create_dir_all(&ov).map_err(|e| e.to_string())?;
    std::fs::write(ov.join("MicrosoftGame.config"), translated).map_err(|e| e.to_string())?;
    let id_exe = ov.join(exe.file_name().ok_or("bad exe name")?);
    // Remove first: never write through a hardlink that may alias the title exe.
    let _ = std::fs::remove_file(&id_exe);
    std::fs::copy(host_exe, &id_exe).map_err(|e| format!("copy host: {e}"))?;
    let host_dir = host_exe.parent().ok_or("host has no dir")?;
    if let Ok(rd) = std::fs::read_dir(host_dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().map(|x| x.eq_ignore_ascii_case("dll")).unwrap_or(false) {
                let dst = ov.join(e.file_name());
                let _ = std::fs::remove_file(&dst);
                let _ = std::fs::copy(&p, dst);
            }
        }
    }
    for asset in ["Logo.png", "SmallLogo.png", "StoreLogo.png", "MCSplashScreen.png", "resources.pri"] {
        let (s, d) = (dir.join(asset), ov.join(asset));
        if s.exists() {
            let _ = std::fs::copy(&s, &d);
        }
    }
    // Registration rejects a config whose ShellVisuals images are absent (console dumps often lack
    // them): copy each from the package, else write a plain placeholder of a valid size.
    for (attr, w, h) in [("Square150x150Logo", 150, 150), ("Square44x44Logo", 44, 44), ("Square480x480Logo", 480, 480), ("SplashScreenImage", 1920, 1080), ("StoreLogo", 100, 100)] {
        let Some(name) = cfg.split(&format!("{attr}=\"")).nth(1).and_then(|r| r.split('"').next()) else { continue };
        let (s, d) = (dir.join(name), ov.join(name));
        if d.exists() {
            continue;
        }
        if let Some(parent) = d.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if s.exists() {
            let _ = std::fs::copy(&s, &d);
        } else {
            std::fs::write(&d, placeholder_png(w, h)).map_err(|e| format!("placeholder {name}: {e}"))?;
        }
    }
    Ok(id_exe)
}

/// Solid black RGB PNG (stored via zlib).
fn placeholder_png(w: u32, h: u32) -> Vec<u8> {
    use std::io::Write;
    fn crc32(b: &[u8]) -> u32 {
        let mut c = !0u32;
        for &x in b {
            c ^= x as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, t: &[u8], d: &[u8]) {
        out.extend((d.len() as u32).to_be_bytes());
        let mut td = t.to_vec();
        td.extend_from_slice(d);
        out.extend(&td);
        out.extend(crc32(&td).to_be_bytes());
    }
    let raw = vec![0u8; (1 + 3 * w as usize) * h as usize]; // filter byte 0 + black pixels per row
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    let _ = z.write_all(&raw);
    let idat = z.finish().unwrap_or_default();
    let mut ihdr = Vec::new();
    ihdr.extend(w.to_be_bytes());
    ihdr.extend(h.to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]);
    let mut out = vec![0x89u8, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]; // PNG signature
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &idat);
    chunk(&mut out, b"IEND", &[]);
    out
}

/// If this process is an activated identity-overlay host, returns the real title exe to run.
pub fn overlay_guest_for_current_exe() -> Option<std::path::PathBuf> {
    let me = std::env::current_exe().ok()?;
    let ov = me.parent()?;
    if !ov.file_name()?.to_string_lossy().eq_ignore_ascii_case(IDENTITY_DIR) {
        return None;
    }
    let guest = ov.parent()?.join(me.file_name()?);
    guest.exists().then_some(guest)
}

/// `identity` is what the process reports as its main image; `cwd` becomes the working directory.
#[cfg(windows)]
pub fn adopt_guest_process_identity(identity: &Path, cwd: &Path) {
    let path = identity;
    extern "system" {
        fn SetCurrentDirectoryW(p: *const u16) -> i32;
    }
    #[repr(C)]
    struct UStr {
        len: u16,
        max: u16,
        buf: *mut u16,
    }
    let full = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let full_s = full.to_string_lossy().trim_start_matches(r"\\?\").to_string();
    let base_s = full.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let leak = |s: &str| -> (*mut u16, u16) {
        let mut w: Vec<u16> = s.encode_utf16().collect();
        let bytes = (w.len() * 2) as u16;
        w.push(0);
        (Box::leak(w.into_boxed_slice()).as_mut_ptr(), bytes)
    };
    unsafe {
        let peb: *mut u8;
        std::arch::asm!("mov {}, gs:[0x60]", out(reg) peb);
        let params = *(peb.add(0x20) as *const *mut u8);
        if !params.is_null() {
            let (p, n) = leak(&full_s);
            let us = params.add(0x60) as *mut UStr;
            (*us).buf = p;
            (*us).len = n;
            (*us).max = n + 2;
        }
        let ldr = *(peb.add(0x18) as *const *mut u8);
        if !ldr.is_null() {
            let first = *(ldr.add(0x10) as *const *mut u8); // InLoadOrderModuleList.Flink = main image entry
            if !first.is_null() {
                let (p, n) = leak(&full_s);
                let fu = first.add(0x48) as *mut UStr;
                (*fu).buf = p;
                (*fu).len = n;
                (*fu).max = n + 2;
                let (p, n) = leak(&base_s);
                let bu = first.add(0x58) as *mut UStr;
                (*bu).buf = p;
                (*bu).len = n;
                (*bu).max = n + 2;
            }
        }
        if let Some(dir) = full.parent() {
            let mut w: Vec<u16> = dir.to_string_lossy().trim_start_matches(r"\\?\").encode_utf16().collect();
            w.push(0);
            SetCurrentDirectoryW(w.as_ptr());
        }
    }
}

#[cfg(windows)]
unsafe extern "system" fn percy_get_module_handle_w(name: *const u16) -> usize {
    if name.is_null() {
        let b = GUEST_IMAGE_BASE.load(std::sync::atomic::Ordering::Acquire);
        if b != 0 {
            return b;
        }
    }
    GetModuleHandleW(name)
}

#[cfg(windows)]
unsafe extern "system" fn percy_get_module_handle_a(name: *const u8) -> usize {
    if name.is_null() {
        let b = GUEST_IMAGE_BASE.load(std::sync::atomic::Ordering::Acquire);
        if b != 0 {
            return b;
        }
    }
    GetModuleHandleA(name)
}

#[cfg(windows)]
unsafe extern "system" fn percy_get_module_file_name_w(module: usize, buf: *mut u16, size: u32) -> u32 {
    let gbase = GUEST_IMAGE_BASE.load(std::sync::atomic::Ordering::Acquire);
    if (module == 0 || module == gbase) && !buf.is_null() && size > 0 {
        if let Ok(g) = GUEST_EXE_PATH.lock() {
            if let Some(ref p) = *g {
                let mut w: Vec<u16> = p.encode_utf16().collect();
                let cap = (size as usize).saturating_sub(1);
                w.truncate(cap);
                std::ptr::copy_nonoverlapping(w.as_ptr(), buf, w.len());
                *buf.add(w.len()) = 0;
                return w.len() as u32;
            }
        }
    }
    GetModuleFileNameW(module, buf, size)
}

#[cfg(windows)]
unsafe extern "system" fn percy_get_module_file_name_a(module: usize, buf: *mut u8, size: u32) -> u32 {
    let gbase = GUEST_IMAGE_BASE.load(std::sync::atomic::Ordering::Acquire);
    if (module == 0 || module == gbase) && !buf.is_null() && size > 0 {
        if let Ok(g) = GUEST_EXE_PATH.lock() {
            if let Some(ref p) = *g {
                let bytes = p.as_bytes();
                let n = bytes.len().min((size as usize).saturating_sub(1));
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, n);
                *buf.add(n) = 0;
                return n as u32;
            }
        }
    }
    GetModuleFileNameA(module, buf, size)
}

#[cfg(windows)]
unsafe extern "system" fn percy_virtual_free(addr: *mut c_void, size: usize, free_type: u32) -> i32 {
    extern "system" {
        fn VirtualFree(addr: *mut c_void, size: usize, free_type: u32) -> i32;
    }
    // Xbox memory management uses 0x8002 (MEM_RELEASE | MEM_PRESERVE_PLACEHOLDER)
    // with non-zero size to split/unmap placeholders. On Windows without placeholder setup,
    // this fails with ERROR_INVALID_PARAMETER (487). Handle it cleanly by returning 1 (success).
    if free_type == 0x8002 || (free_type & 0x0002 != 0) {
        return 1;
    }
    // For standard MEM_RELEASE (0x8000), dwSize must be 0 in Win32
    if free_type & 0x8000 != 0 && size != 0 {
        return 1;
    }
    VirtualFree(addr, size, free_type)
}

/// Writes to stderr via `WriteFile` (no Rust std TLS; safe while the guest owns TLS slot 0).
#[cfg(windows)]
pub fn raw_stderr(s: &str) {
    extern "system" {
        fn GetStdHandle(n: u32) -> *mut c_void;
        fn WriteFile(h: *mut c_void, b: *const u8, n: u32, w: *mut u32, o: *mut c_void) -> i32;
    }
    unsafe {
        let mut w = 0u32;
        WriteFile(GetStdHandle(-12i32 as u32), s.as_ptr(), s.len() as u32, &mut w, std::ptr::null_mut());
    }
}

static REAL_EXIT_PROCESS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static REAL_CRT_EXIT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[cfg(windows)]
unsafe fn log_exit_caller(what: &str, code: u32) {
    let rsp: usize;
    std::arch::asm!("mov {}, rsp", out(reg) rsp);
    raw_stderr(&format!("[exit] {what}({code}) | {}\n", compact_frames(rsp, 1024, 8)));
}

#[cfg(windows)]
static REAL_FMOD_INIT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static REAL_FMOD_SET_OUTPUT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static REAL_FMOD_CREATE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

unsafe extern "system" fn percy_fmod_create(out: *mut *mut c_void, ver: u32) -> i32 {
    let f: unsafe extern "system" fn(*mut *mut c_void, u32) -> i32 = std::mem::transmute(REAL_FMOD_CREATE.load(std::sync::atomic::Ordering::Acquire));
    let r = f(out, ver);
    raw_stderr(&format!("[fmod] System_Create(ver={ver:#x}) -> {r}\n"));
    r
}

/// `FMOD::System::init`: the console output path needs the APU (absent on PC). On failure,
/// fall back to FMOD_OUTPUTTYPE_NOSOUND (2) so the title's sound system still initializes.
unsafe extern "system" fn percy_fmod_init(sys: *mut c_void, max_ch: i32, flags: u32, extra: *mut c_void) -> i32 {
    type Init = unsafe extern "system" fn(*mut c_void, i32, u32, *mut c_void) -> i32;
    type SetOut = unsafe extern "system" fn(*mut c_void, i32) -> i32;
    let init: Init = std::mem::transmute(REAL_FMOD_INIT.load(std::sync::atomic::Ordering::Acquire));
    let so0 = REAL_FMOD_SET_OUTPUT.load(std::sync::atomic::Ordering::Acquire);
    if so0 != 0 && std::env::current_exe().map(|e| e.with_file_name("percy_nosound").exists()).unwrap_or(false) {
        let set_out: SetOut = std::mem::transmute(so0);
        raw_stderr(&format!("[fmod] percy_nosound: setOutput(NOSOUND) -> {}
", set_out(sys, 2)));
    }
    let mut r = init(sys, max_ch, flags, extra);
    raw_stderr(&format!("[fmod] init(max={max_ch}, flags={flags:#x}) -> {r}\n"));
    let so = REAL_FMOD_SET_OUTPUT.load(std::sync::atomic::Ordering::Acquire);
    if r != 0 && so != 0 {
        let set_out: SetOut = std::mem::transmute(so);
        let s = set_out(sys, 2);
        r = init(sys, max_ch, flags, extra);
        raw_stderr(&format!("[fmod] setOutput(NOSOUND) -> {s}; init -> {r}\n"));
    }
    r
}

unsafe extern "system" fn percy_exit_process(code: u32) -> ! {
    log_exit_caller("ExitProcess", code);
    let f: unsafe extern "system" fn(u32) -> ! = std::mem::transmute(REAL_EXIT_PROCESS.load(std::sync::atomic::Ordering::Acquire));
    f(code)
}

#[cfg(windows)]
unsafe extern "C" fn percy_crt_exit(code: i32) -> ! {
    log_exit_caller("exit", code as u32);
    let f: unsafe extern "C" fn(i32) -> ! = std::mem::transmute(REAL_CRT_EXIT.load(std::sync::atomic::Ordering::Acquire));
    f(code)
}

// --- Gaming Runtime entry-point tracing (PERCY_TRACE): wraps runtime exports the title binds via GetProcAddress.
static RT_REAL: [std::sync::atomic::AtomicUsize; 4] = [
    std::sync::atomic::AtomicUsize::new(0),
    std::sync::atomic::AtomicUsize::new(0),
    std::sync::atomic::AtomicUsize::new(0),
    std::sync::atomic::AtomicUsize::new(0),
];
const RT_NAMES: [&str; 4] = ["QueryApiImpl", "InitializeApiImpl", "InitializeApiImplEx", "UninitializeApiImpl"];
type Fn8 = unsafe extern "system" fn(usize, usize, usize, usize, usize, usize, usize, usize) -> i32;

#[cfg(windows)]
unsafe fn guid_str(p: usize) -> String {
    if p < 0x10000 {
        return format!("{p:#x}");
    }
    let g = std::slice::from_raw_parts(p as *const u8, 16);
    let d1 = u32::from_le_bytes([g[0], g[1], g[2], g[3]]);
    format!("{d1:08x}-{:02x}{:02x}..{:02x}{:02x}", g[5], g[4], g[14], g[15])
}

#[cfg(windows)]
unsafe fn rt_call(i: usize, a: [usize; 8]) -> i32 {
    let f: Fn8 = std::mem::transmute(RT_REAL[i].load(std::sync::atomic::Ordering::Acquire));
    let hr = f(a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7]);
    // QueryApiImpl runs on every runtime API call (thousands per second): never log it here.
    if i != 0 {
        raw_stderr(&format!("[rt] {} a=({:#x},{:#x},{:#x}) hr={:08x}\n", RT_NAMES[i], a[0], a[1], a[2], hr as u32));
    }
    hr
}
macro_rules! rt_wrap {
    ($name:ident, $i:expr) => {
        #[cfg(windows)]
        unsafe extern "system" fn $name(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize, h: usize) -> i32 {
            rt_call($i, [a, b, c, d, e, f, g, h])
        }
    };
}
rt_wrap!(rt_wrap0, 0);
rt_wrap!(rt_wrap1, 1);
rt_wrap!(rt_wrap2, 2);
rt_wrap!(rt_wrap3, 3);

#[cfg(windows)]
unsafe extern "system" fn percy_get_proc_address(module: usize, name: *const i8) -> usize {
    let real = GetProcAddress(module, name);
    if (name as usize) >= 0x10000 && real == 0 {
        let (m, _) = describe_addr(module);
        raw_stderr(&format!("[gpa] MISS {m}!{}\n", std::ffi::CStr::from_ptr(name).to_string_lossy()));
    }
    if real == 0 || (name as usize) < 0x10000 {
        return real;
    }
    let n = std::ffi::CStr::from_ptr(name).to_bytes();
    let wraps: [usize; 4] = [rt_wrap0 as usize, rt_wrap1 as usize, rt_wrap2 as usize, rt_wrap3 as usize];
    for (i, rn) in RT_NAMES.iter().enumerate() {
        if n == rn.as_bytes() {
            RT_REAL[i].store(real, std::sync::atomic::Ordering::Release);
            return wraps[i];
        }
    }
    real
}

/// Logs a true backtrace (unwind data) for fatal exceptions; never handles them.
#[cfg(windows)]
unsafe extern "system" fn crash_veh(info: *const c_void) -> i32 {
    let code = **(info as *const *const u32);
    if code == 0xC000_0374 {
        // Heap is corrupt (and possibly locked): no allocation here. Guest frames as G+rva.
        #[link(name = "kernel32")]
        extern "system" {
            fn RtlLookupFunctionEntry(pc: u64, base: *mut u64, hist: *mut c_void) -> *mut c_void;
            fn RtlVirtualUnwind(t: u32, base: u64, pc: u64, fe: *mut c_void, ctx: *mut u8, hd: *mut *mut c_void, est: *mut u64, nv: *mut c_void) -> *mut c_void;
        }
        // Unwind from the faulting CONTEXT (CONTEXT: Rsp @0x98, Rip @0xF8; 1232 bytes).
        let mut ctx = [0u8; 1232];
        std::ptr::copy_nonoverlapping(*((info as *const *const u8).add(1)), ctx.as_mut_ptr(), 1232);
        let mut f = [0usize; 48];
        let mut n = 0;
        while n < 48 {
            let rip = *(ctx.as_ptr().add(0xF8) as *const u64);
            if rip == 0 { break; }
            f[n] = rip as usize;
            n += 1;
            let mut base = 0u64;
            let fe = RtlLookupFunctionEntry(rip, &mut base, std::ptr::null_mut());
            if fe.is_null() {
                let rsp = ctx.as_mut_ptr().add(0x98) as *mut u64;
                *(ctx.as_mut_ptr().add(0xF8) as *mut u64) = *(*rsp as *const u64);
                *rsp += 8;
            } else {
                let (mut hd, mut est) = (std::ptr::null_mut(), 0u64);
                RtlVirtualUnwind(0, base, rip, fe, ctx.as_mut_ptr(), &mut hd, &mut est, std::ptr::null_mut());
            }
        }
        let g = GUEST_IMAGE_BASE.load(std::sync::atomic::Ordering::Acquire);
        let mut buf = [0u8; 1024];
        let mut k = 0;
        for &b in b"[crash] 0xc0000374" { buf[k] = b; k += 1; }
        for &a in &f[..n] {
            let (pre, v): (&[u8], usize) = if a >= g && a < g + 0x600_0000 { (b" G+", a - g) } else { (b" ", a) };
            for &b in pre { buf[k] = b; k += 1; }
            let mut d = [0u8; 16];
            let mut m = 0;
            let mut x = v;
            loop { d[m] = b"0123456789abcdef"[x & 15]; m += 1; x >>= 4; if x == 0 { break; } }
            while m > 0 { m -= 1; buf[k] = d[m]; k += 1; }
            if k > 1000 { break; }
        }
        buf[k] = 10; k += 1;
        raw_stderr(std::str::from_utf8_unchecked(&buf[..k]));
        // EXCEPTION_RECORD: NumberParameters @24, ExceptionInformation @32. Dump around each pointer.
        let er = *(info as *const *const u8);
        let np = (*(er.add(24) as *const u32)).min(6) as usize;
        let mut line = [0u8; 1400];
        let mut k = 0;
        let mut put = |line: &mut [u8; 1400], k: &mut usize, v: usize, digits: usize| {
            for i in (0..digits).rev() { if *k < 1390 { line[*k] = b"0123456789abcdef"[(v >> (i * 4)) & 15]; *k += 1; } }
        };
        for &c in b"[crash] params" { line[k] = c; k += 1; }
        for i in 0..np {
            line[k] = b' '; k += 1;
            put(&mut line, &mut k, *(er.add(32 + 8 * i) as *const usize), 16);
        }
        line[k] = 10; k += 1;
        raw_stderr(std::str::from_utf8_unchecked(&line[..k]));
        extern "system" { fn IsBadReadPtr(p: *const c_void, n: usize) -> i32; }
        // HEAP_FAILURE_INFORMATION (Version 2): Address @24, Param1 @32 — dump those instead.
        let fi = if np > 0 { *(er.add(32) as *const usize) } else { 0 };
        let targets: [usize; 2] = if fi > 0x10000 && IsBadReadPtr(fi as *const c_void, 56) == 0 && *(fi as *const u32) == 2 {
            [*((fi + 24) as *const usize), *((fi + 32) as *const usize)]
        } else { [0, 0] };
        for i in 0..2 {
            let a = targets[i];
            if a < 0x10000 || IsBadReadPtr((a.saturating_sub(64)) as *const c_void, 192) != 0 { continue; }
            let mut k = 0;
            for &c in b"[crash] mem " { line[k] = c; k += 1; }
            put(&mut line, &mut k, a - 64, 16);
            line[k] = b':'; k += 1;
            for j in 0..192 {
                if j % 16 == 0 { line[k] = b' '; k += 1; }
                put(&mut line, &mut k, *((a - 64 + j) as *const u8) as usize, 2);
            }
            line[k] = b' '; k += 1; line[k] = b'|'; k += 1;
            for j in 0..192 { let c = *((a - 64 + j) as *const u8); line[k] = if (32..127).contains(&c) { c } else { b'.' }; k += 1; }
            line[k] = 10; k += 1;
            raw_stderr(std::str::from_utf8_unchecked(&line[..k]));
        }
        return 0;
    }
    if matches!(code, 0xC000_0409 | 0xC000_0005) { // not 0xC0000374: the heap lock is held there
        let mut f = [0usize; 24];
        let n = RtlCaptureStackBackTrace(0, 24, f.as_mut_ptr(), std::ptr::null_mut()) as usize;
        let fr: Vec<String> = f[..n].iter().map(|&a| { let (m, o) = describe_addr(a); short_mod(&m, o) }).collect();
        raw_stderr(&format!("[crash] {code:#x} {}
", fr.join(" ")));
    }
    0
}

// Opt-in (`percy_heapchk` next to the exe) guest heap-overrun detector: 32 tail bytes of slack per
// guest allocation, the first 16 a pointer-keyed canary checked on free.
static HC_REAL: [std::sync::atomic::AtomicUsize; 4] = [const { std::sync::atomic::AtomicUsize::new(0) }; 4];
const HC_PAD: usize = 32;
fn heapchk_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::current_exe().map(|e| e.with_file_name("percy_heapchk").exists()).unwrap_or(false))
}
#[cfg(windows)]
extern "C" { fn _msize(p: *mut u8) -> usize; }
fn hc_key(p: usize) -> u64 { (p as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x5045_5243_5948_4321 }
#[cfg(windows)]
unsafe fn hc_mark(p: *mut u8, n: usize) -> *mut u8 {
    if !p.is_null() {
        let c = p.add(n) as *mut u64;
        c.write_unaligned(hc_key(p as usize));
        c.add(1).write_unaligned(n as u64);
        let mut f = [0usize; 3];
        RtlCaptureStackBackTrace(2, 3, f.as_mut_ptr(), std::ptr::null_mut());
        HC_LIVE.lock().unwrap().insert(p as usize, (n, f));
        hc_watch_start();
    }
    p
}
/// Live marked blocks: ptr -> (size, allocation call sites).
static HC_LIVE: std::sync::Mutex<std::collections::BTreeMap<usize, (usize, [usize; 3])>> = std::sync::Mutex::new(std::collections::BTreeMap::new());
#[cfg(windows)]
fn hc_watch_start() {
    static S: std::sync::Once = std::sync::Once::new();
    S.call_once(|| unsafe {
        extern "system" {
            fn CreateThread(sa: usize, stack: usize, f: unsafe extern "system" fn(usize) -> u32, p: usize, fl: u32, id: usize) -> usize;
        }
        CreateThread(0, 0, hc_watch, 0, 0, 0);
    });
}
/// Scans every live block's canary every 50 ms; reports each smashed block once with its allocation site.
#[cfg(windows)]
unsafe extern "system" fn hc_watch(_: usize) -> u32 {
    guest_tls::restore_current_thread();
    let g = || GUEST_IMAGE_BASE.load(std::sync::atomic::Ordering::Acquire);
    let fmt = |a: usize| if a >= g() && a < g() + 0x600_0000 { format!("G+{:x}", a - g()) } else { let (m, o) = describe_addr(a); short_mod(&m, o) };
    let mut reported = 0;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(50));
        let mut bad = Vec::new();
        {
            let live = HC_LIVE.lock().unwrap();
            for (&p, &(n, f)) in live.iter() {
                let c = (p + n) as *const u64;
                if c.read_unaligned() != hc_key(p) || c.add(1).read_unaligned() != n as u64 {
                    let mut b = [0u8; 24];
                    std::ptr::copy_nonoverlapping((p + n) as *const u8, b.as_mut_ptr(), 24);
                    bad.push((p, n, f, b));
                }
            }
        }
        for (p, n, f, b) in bad {
            if reported < 20 {
                reported += 1;
                let head: String = (0..n.min(48)).map(|k| { let c = *((p + k) as *const u8); if (32..127).contains(&c) { c as char } else { '.' } }).collect();
                raw_stderr(&format!("[heapchk] live overrun p={p:#x} size={n} tail={b:02x?} alloc@ {} {} {} | head '{head}'
", fmt(f[0]), fmt(f[1]), fmt(f[2])));
            }
            HC_LIVE.lock().unwrap().remove(&p);
        }
    }
}
#[cfg(windows)]
unsafe fn hc_check(p: *mut u8, who: &str) {
    if p.is_null() { return; }
    let m = _msize(p);
    if m < HC_PAD { return; }
    let n = m - HC_PAD;
    let c = p.add(n) as *const u64;
    // Only blocks we marked carry the size in the second word; check the key there.
    if c.add(1).read_unaligned() != n as u64 { return; }
    if c.read_unaligned() != hc_key(p as usize) {
        let rsp: usize;
        std::arch::asm!("mov {}, rsp", out(reg) rsp);
        let mut b = [0u8; 16]; std::ptr::copy_nonoverlapping(p.add(n), b.as_mut_ptr(), 16);
        raw_stderr(&format!("[heapchk] {who} overrun p={:#x} size={n} tail={b:02x?} | {}
", p as usize, compact_frames(rsp, 256, 10)));
    }
}
#[cfg(windows)]
unsafe extern "C" fn hc_malloc(n: usize) -> *mut u8 {
    let f: unsafe extern "C" fn(usize) -> *mut u8 = std::mem::transmute(HC_REAL[0].load(std::sync::atomic::Ordering::Relaxed));
    hc_mark(f(n + HC_PAD), n)
}
#[cfg(windows)]
unsafe extern "C" fn hc_calloc(c: usize, s: usize) -> *mut u8 {
    let f: unsafe extern "C" fn(usize, usize) -> *mut u8 = std::mem::transmute(HC_REAL[1].load(std::sync::atomic::Ordering::Relaxed));
    let Some(n) = c.checked_mul(s) else { return std::ptr::null_mut() };
    hc_mark(f(1, n + HC_PAD), n)
}
#[cfg(windows)]
unsafe extern "C" fn hc_realloc(p: *mut u8, n: usize) -> *mut u8 {
    let f: unsafe extern "C" fn(*mut u8, usize) -> *mut u8 = std::mem::transmute(HC_REAL[2].load(std::sync::atomic::Ordering::Relaxed));
    hc_check(p, "realloc");
    if !p.is_null() { HC_LIVE.lock().unwrap().remove(&(p as usize)); }
    if n == 0 && !p.is_null() { return f(p, 0); }
    hc_mark(f(p, n + HC_PAD), n)
}
#[cfg(windows)]
unsafe extern "C" fn hc_free(p: *mut u8) {
    let f: unsafe extern "C" fn(*mut u8) = std::mem::transmute(HC_REAL[3].load(std::sync::atomic::Ordering::Relaxed));
    hc_check(p, "free");
    HC_LIVE.lock().unwrap().remove(&(p as usize));
    f(p)
}
#[cfg(windows)]
fn heapchk_hook(name: &str, real: usize) -> usize {
    if real == 0 { return 0; }
    let (i, w) = match name { "malloc" => (0, hc_malloc as usize), "calloc" => (1, hc_calloc as usize), "realloc" => (2, hc_realloc as usize), _ => (3, hc_free as usize) };
    HC_REAL[i].store(real, std::sync::atomic::Ordering::Relaxed);
    w
}

// Opt-in (`percy_quarantine` next to the exe) double-free / use-after-free detector for guest
// free/realloc: freed blocks are pattern-filled and held back; a second free of a held block, or a
// changed pattern when one is finally released, is logged with the stack of both frees.
const QZ_PAT: u8 = 0xFD;
const QZ_MAX: usize = 1 << 16;
struct QzEntry { p: usize, n: usize, who: [usize; 6] }
static QZ: std::sync::Mutex<(std::collections::VecDeque<QzEntry>, std::collections::BTreeMap<usize, usize>, u64)> =
    std::sync::Mutex::new((std::collections::VecDeque::new(), std::collections::BTreeMap::new(), 0));
fn quarantine_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::current_exe().map(|e| e.with_file_name("percy_quarantine").exists()).unwrap_or(false))
}
#[cfg(windows)]
unsafe fn qz_frames() -> [usize; 6] {
    let mut f = [0usize; 8];
    let n = RtlCaptureStackBackTrace(2, 8, f.as_mut_ptr(), std::ptr::null_mut()) as usize;
    let g = GUEST_IMAGE_BASE.load(std::sync::atomic::Ordering::Acquire);
    let mut o = [0usize; 6];
    // guest frames as RVAs (high bit marks them), others raw
    for (i, &a) in f[..n].iter().take(6).enumerate() {
        o[i] = if a >= g && a < g + 0x600_0000 { (a - g) | 1 << 63 } else { a };
    }
    o
}
fn qz_fmt(w: &[usize; 6]) -> String {
    w.iter().filter(|&&a| a != 0).map(|&a| if a >> 63 == 1 { format!("G+{:x}", a & !(1 << 63)) } else { format!("{a:#x}") }).collect::<Vec<_>>().join(" ")
}
#[cfg(windows)]
unsafe fn qz_take(p: usize, what: &str) -> bool {
    let mut q = QZ.lock().unwrap();
    if let Some(&id) = q.1.get(&p) {
        let first = q.0.iter().find(|e| e.p == p).map(|e| qz_fmt(&e.who)).unwrap_or_default();
        let _ = id;
        drop(q);
        raw_stderr(&format!("[qz] {what} of already-freed {p:#x}: now {} | first free {first}
", qz_fmt(&qz_frames())));
        return true;
    }
    false
}
#[cfg(windows)]
unsafe extern "C" fn qz_free(p: *mut u8) {
    if p.is_null() { return; }
    if qz_take(p as usize, "double free") { return; } // swallow: keep the process alive to log more
    let n = _msize(p);
    std::ptr::write_bytes(p, QZ_PAT, n.min(1 << 20));
    let who = qz_frames();
    qz_watch_start();
    let evict = {
        let mut q = QZ.lock().unwrap();
        q.2 += 1;
        let id = q.2 as usize;
        q.1.insert(p as usize, id);
        q.0.push_back(QzEntry { p: p as usize, n, who });
        if q.0.len() > QZ_MAX { let e = q.0.pop_front().unwrap(); q.1.remove(&e.p); Some(e) } else { None }
    };
    if let Some(e) = evict {
        let s = std::slice::from_raw_parts(e.p as *const u8, e.n.min(1 << 20));
        if let Some(off) = s.iter().position(|&b| b != QZ_PAT) {
            raw_stderr(&format!("[qz] write after free at {:#x}+{off} (size {}) freed by {}
", e.p, e.n, qz_fmt(&e.who)));
        }
        let f: unsafe extern "C" fn(*mut u8) = std::mem::transmute(HC_REAL[3].load(std::sync::atomic::Ordering::Relaxed));
        f(e.p as *mut u8);
    }
}
#[cfg(windows)]
fn qz_watch_start() {
    static S: std::sync::Once = std::sync::Once::new();
    S.call_once(|| unsafe {
        extern "system" {
            fn CreateThread(sa: usize, stack: usize, f: unsafe extern "system" fn(usize) -> u32, p: usize, fl: u32, id: usize) -> usize;
        }
        CreateThread(0, 0, qz_watch, 0, 0, 0);
    });
}
/// Every 50 ms: any held (freed) block whose fill pattern changed was written after free.
#[cfg(windows)]
unsafe extern "system" fn qz_watch(_: usize) -> u32 {
    guest_tls::restore_current_thread();
    let mut reported = 0;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(50));
        let mut hits = Vec::new();
        {
            let mut q = QZ.lock().unwrap();
            for e in q.0.iter_mut() {
                let s = std::slice::from_raw_parts(e.p as *const u8, e.n.min(1 << 20));
                if let Some(off) = s.iter().position(|&b| b != QZ_PAT) {
                    let mut b = [0u8; 16];
                    let k = (e.n.min(1 << 20) - off).min(16);
                    b[..k].copy_from_slice(&s[off..off + k]);
                    hits.push((e.p, e.n, off, e.who, b));
                    std::ptr::write_bytes(e.p as *mut u8, QZ_PAT, e.n.min(1 << 20)); // report each write once
                }
            }
        }
        for (p, n, off, who, b) in hits {
            if reported < 30 {
                reported += 1;
                raw_stderr(&format!("[qz] write after free {p:#x}+{off:#x} (size {n:#x}) bytes {b:02x?} freed by {}
", qz_fmt(&who)));
            }
        }
    }
}
#[cfg(windows)]
unsafe extern "C" fn qz_realloc(p: *mut u8, n: usize) -> *mut u8 {
    if !p.is_null() && qz_take(p as usize, "realloc") { return std::ptr::null_mut(); }
    let f: unsafe extern "C" fn(*mut u8, usize) -> *mut u8 = std::mem::transmute(HC_REAL[2].load(std::sync::atomic::Ordering::Relaxed));
    f(p, n)
}
#[cfg(windows)]
fn quarantine_hook(name: &str, real: usize) -> usize {
    if real == 0 { return 0; }
    if name == "free" { HC_REAL[3].store(real, std::sync::atomic::Ordering::Relaxed); qz_free as usize }
    else { HC_REAL[2].store(real, std::sync::atomic::Ordering::Relaxed); qz_realloc as usize }
}

fn short_mod(m: &str, off: usize) -> String {
    let m = m.trim_end_matches(".dll").trim_end_matches(".DLL").trim_end_matches(".exe");
    let m = match m {
        "guest" => "G",
        "ucrtbase" => "crt",
        "Minecraft.Windows" | "percyscarlett" => "host",
        o => o,
    };
    format!("{m}+{off:x}")
}

/// Deduped return-address scan: guest + non-system modules, at most `max` frames.
#[cfg(windows)]
unsafe fn compact_frames(rsp: usize, slots: usize, max: usize) -> String {
    let mut out: Vec<String> = Vec::new();
    for i in 0..slots {
        let (m, off) = describe_addr(*((rsp as *const usize).add(i)));
        if m.is_empty() || off == 0 || ["ntdll.dll", "KERNELBASE.dll", "kernel32.dll", "KERNEL32.DLL", "sechost.dll", "kernel.appcore.dll"].iter().any(|s| m.eq_ignore_ascii_case(s)) {
            continue;
        }
        let s = short_mod(&m, off);
        if !out.contains(&s) {
            out.push(s);
        }
        if out.len() >= max {
            break;
        }
    }
    out.join(" ")
}

static REAL_CREATE_PROCESS_W: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
type CreateProcessWFn = unsafe extern "system" fn(*const u16, *mut u16, usize, usize, i32, u32, usize, *const u16, usize, usize) -> i32;

#[cfg(windows)]
unsafe fn wstr(p: *const u16) -> String {
    if p.is_null() {
        return "null".into();
    }
    let mut n = 0;
    while *p.add(n) != 0 && n < 1024 {
        n += 1;
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(p, n))
}

#[cfg(windows)]
unsafe extern "system" fn percy_create_process_w(
    app: *const u16, cmd: *mut u16, pa: usize, ta: usize, inh: i32, flags: u32, env: usize, cwd: *const u16, si: usize, pi: usize,
) -> i32 {
    let rsp: usize;
    std::arch::asm!("mov {}, rsp", out(reg) rsp);
    raw_stderr(&format!("[spawn] app={} cmd={} | {}\n", wstr(app), wstr(cmd), compact_frames(rsp, 1024, 8)));
    let f: CreateProcessWFn = std::mem::transmute(REAL_CREATE_PROCESS_W.load(std::sync::atomic::Ordering::Acquire));
    f(app, cmd, pa, ta, inh, flags, env, cwd, si, pi)
}

static REAL_LOAD_LIBRARY_EX_A: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
#[cfg(windows)]
unsafe extern "system" fn percy_load_library_ex_a(name: *const i8, file: usize, flags: u32) -> usize {
    let f: unsafe extern "system" fn(*const i8, usize, u32) -> usize =
        std::mem::transmute(REAL_LOAD_LIBRARY_EX_A.load(std::sync::atomic::Ordering::Acquire));
    let n = if name.is_null() { "null".into() } else { std::ffi::CStr::from_ptr(name).to_string_lossy().into_owned() };
    // Titles load the runtime with LOAD_LIBRARY_SEARCH_SYSTEM32, which would bypass our runtime shim.
    let shim = std::env::current_exe().ok().map(|e| e.with_file_name("xgameruntime.dll")).filter(|p| n.eq_ignore_ascii_case("xgameruntime.dll") && p.exists());
    let h = match shim.and_then(|p| std::ffi::CString::new(p.to_string_lossy().into_owned()).ok()) {
        Some(p) => f(p.as_ptr(), 0, 0),
        None => f(name, file, flags),
    };
    raw_stderr(&format!("[ll] {n} flags={flags:#x} -> {h:#x}\n"));
    h
}

/// Logs `what` plus a compact return-address scan of the current stack.
#[cfg(windows)]
pub fn log_stack(what: &str) {
    unsafe {
        let rsp: usize;
        std::arch::asm!("mov {}, rsp", out(reg) rsp);
        raw_stderr(&format!("[stack] {what} | {}\n", compact_frames(rsp, 4096, 16)));
    }
}

/// Resolves an address to `(module, offset)`; guest-mapped images report as `guest`.
#[cfg(windows)]
unsafe fn describe_addr(a: usize) -> (String, usize) {
    extern "system" {
        fn GetModuleHandleExW(flags: u32, addr: usize, out: *mut usize) -> i32;
    }
    if a < 0x10000 {
        return (String::new(), 0);
    }
    if let Ok(ranges) = MAPPED_RANGES.try_lock() {
        for &(s, e) in ranges.iter() {
            if a >= s && a < e {
                return ("guest".into(), a - s);
            }
        }
    }
    let mut h = 0usize;
    if GetModuleHandleExW(0x6, a, &mut h) == 0 || h == 0 {
        return (String::new(), 0);
    }
    let mut buf = [0u16; 260];
    let n = GetModuleFileNameW(h, buf.as_mut_ptr(), 260) as usize;
    let full = String::from_utf16_lossy(&buf[..n]);
    let name = full.rsplit('\\').next().unwrap_or("").to_string();
    (name, a - h)
}

/// One-shot guest breakpoints from `PERCY_BP=rva,rva,...` (hex RVAs): (address, original byte).
static BREAKPOINTS: std::sync::Mutex<Vec<(usize, u8, u32)>> = std::sync::Mutex::new(Vec::new());
std::thread_local! { static BP_REARM: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

/// `value(module+off)` or `ptr->*ptr(module+off)` for readable pointers (breakpoint logging).
#[cfg(windows)]
unsafe fn bp_chain(p: u64) -> String {
    extern "system" {
        fn VirtualQuery(a: *const c_void, b: *mut u8, n: usize) -> usize;
    }
    let sym = |v: u64| {
        let (m, o) = describe_addr(v as usize);
        if m.is_empty() { String::new() } else { format!("({})", short_mod(&m, o)) }
    };
    if p < 0x10000 {
        return format!("{p:#x}");
    }
    let s = sym(p);
    if !s.is_empty() {
        return format!("{p:#x}{s}");
    }
    let mut mbi = [0u8; 48];
    let committed = VirtualQuery(p as *const c_void, mbi.as_mut_ptr(), 48) != 0
        && u32::from_le_bytes([mbi[32], mbi[33], mbi[34], mbi[35]]) == 0x1000;
    if !committed {
        return format!("{p:#x}");
    }
    let v = *(p as *const u64);
    format!("{p:#x}->{v:#x}{}", sym(v))
}

/// Hit at `addr`: (original byte, re-arm afterwards?). `rva*N` specs log N hits.
fn bp_take(addr: usize) -> Option<(u8, bool)> {
    let mut v = BREAKPOINTS.lock().ok()?;
    let i = v.iter().position(|&(a, _, _)| a == addr)?;
    if v[i].2 > 1 {
        v[i].2 -= 1;
        return Some((v[i].1, true));
    }
    Some((v.swap_remove(i).1, false))
}

/// Arms `PERCY_BP` breakpoints in a freshly mapped (still writable) guest image.
#[cfg(windows)]
unsafe fn arm_breakpoints(base: *mut u8, size: usize) {
    // Env var, or `percy_bp.txt` next to the host (package activation does not inherit env).
    let spec = std::env::var("PERCY_BP").ok().or_else(|| {
        let p = std::env::current_exe().ok()?.with_file_name("percy_bp.txt");
        std::fs::read_to_string(p).ok()
    });
    let Some(spec) = spec else { return };
    let mut v = BREAKPOINTS.lock().unwrap();
    for s in spec.split(',') {
        let (s, n) = s.trim().split_once('*').map(|(a, b)| (a, b.trim().parse().unwrap_or(1))).unwrap_or((s.trim(), 1u32));
        if let Ok(rva) = usize::from_str_radix(s.trim().trim_start_matches("0x"), 16) {
            if rva < size {
                let p = base.add(rva);
                v.push((p as usize, *p, n));
                *p = 0xCC;
            }
        }
    }
}

#[cfg(windows)]
static DUMMY_VTBL_ADDR: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[cfg(windows)]
unsafe fn get_or_create_dummy_vtable() -> usize {
    let existing = DUMMY_VTBL_ADDR.load(std::sync::atomic::Ordering::Acquire);
    if existing != 0 {
        return existing;
    }
    // Allocate executable stub: xor eax, eax; ret (48 31 c0 c3)
    let stub_mem = VirtualAlloc(0, 4096, 0x3000, 0x40);
    if stub_mem == 0 {
        return 0;
    }
    let code: [u8; 4] = [0x48, 0x31, 0xc0, 0xc3];
    std::ptr::copy_nonoverlapping(code.as_ptr(), stub_mem as *mut u8, code.len());

    // Allocate dummy vtable: 512 entries pointing to stub_mem
    let vtbl_mem = VirtualAlloc(0, 4096, 0x3000, 0x04) as *mut usize;
    if vtbl_mem.is_null() {
        return 0;
    }
    for i in 0..512 {
        *vtbl_mem.add(i) = stub_mem;
    }
    let addr = vtbl_mem as usize;
    DUMMY_VTBL_ADDR.store(addr, std::sync::atomic::Ordering::Release);
    addr
}

#[cfg(windows)]
fn is_readable_addr(p: u64) -> bool {
    if p < 0x10000 || p > 0x7fff_ffff_ffff { return false; }
    extern "system" {
        fn VirtualQuery(a: *const c_void, b: *mut u8, n: usize) -> usize;
    }
    unsafe {
        let mut mbi = [0u8; 48];
        if VirtualQuery(p as *const c_void, mbi.as_mut_ptr(), 48) == 0 {
            return false;
        }
        let state = *(mbi.as_ptr().add(0x20) as *const u32);
        let prot = *(mbi.as_ptr().add(0x24) as *const u32);
        state == 0x1000 && (prot & 0x6e != 0) && (prot & 0x100 == 0)
    }
}

#[cfg(windows)]
fn is_writable_addr(p: u64) -> bool {
    if p < 0x10000 || p > 0x7fff_ffff_ffff { return false; }
    extern "system" {
        fn VirtualQuery(a: *const c_void, b: *mut u8, n: usize) -> usize;
    }
    unsafe {
        let mut mbi = [0u8; 48];
        if VirtualQuery(p as *const c_void, mbi.as_mut_ptr(), 48) == 0 {
            return false;
        }
        let state = *(mbi.as_ptr().add(0x20) as *const u32);
        let prot = *(mbi.as_ptr().add(0x24) as *const u32);
        state == 0x1000 && (prot & 0xcc != 0) && (prot & 0x100 == 0)
    }
}

#[cfg(windows)]
unsafe fn decode_call_len(mut ptr: *const u8) -> Option<usize> {
    let mut len = 0;
    while len < 15 && matches!(*ptr, 0x40..=0x4F | 0x66 | 0x67) {
        ptr = ptr.add(1);
        len += 1;
    }
    if *ptr != 0xFF {
        return None;
    }
    ptr = ptr.add(1);
    len += 1;
    let modrm = *ptr;
    ptr = ptr.add(1);
    len += 1;
    let mod_bits = (modrm >> 6) & 3;
    let reg_bits = (modrm >> 3) & 7;
    let rm_bits = modrm & 7;
    if reg_bits != 2 {
        return None;
    }
    if mod_bits == 3 {
        return Some(len);
    }
    let has_sib = rm_bits == 4;
    let sib_base = if has_sib {
        let b = (*ptr) & 7;
        ptr = ptr.add(1);
        len += 1;
        Some(b)
    } else {
        None
    };
    if mod_bits == 0 {
        if (has_sib && sib_base == Some(5)) || (!has_sib && rm_bits == 5) {
            len += 4;
        }
    } else if mod_bits == 1 {
        len += 1;
    } else if mod_bits == 2 {
        len += 4;
    }
    Some(len)
}

#[cfg(windows)]
unsafe extern "system" fn trace_veh(info: *const c_void) -> i32 {
    let er = *(info as *const *const u32);
    let code = *er;
    let _addr = *(er.add(4) as *const usize);
    if code == 0x8000_0004 {
        let a = BP_REARM.with(|c| c.replace(0));
        if a != 0 {
            let mut old = 0u32;
            VirtualProtect(a as *mut c_void, 1, 0x40, &mut old);
            *(a as *mut u8) = 0xCC;
            VirtualProtect(a as *mut c_void, 1, old, &mut old);
            return -1;
        }
    }
    if code == 0x8000_0003 {
        // PERCY_BP breakpoint: log, restore original byte, resume at the same RIP (re-arm via TF).
        let ctx = *((info as *const *const u64).add(1)) as *mut u64;
        let rip = *ctx.add(0xF8 / 8) as usize;
        if let Some((orig, rearm)) = bp_take(rip) {
            let r = |o: usize| *ctx.add(o / 8);
            let rsp = r(0x98) as usize;
            raw_stderr(&format!(
                "[bp] {} rax={} rbx={} rcx={} rdx={} r8={:#x} r9={:#x} rsi={} rdi={} r14={} | {}\n",
                short_mod("guest", rip - GUEST_IMAGE_BASE.load(std::sync::atomic::Ordering::Acquire)),
                bp_chain(r(0x78)), bp_chain(r(0x90)), bp_chain(r(0x80)), bp_chain(r(0x88)), r(0xB8), r(0xC0), bp_chain(r(0xA8)), bp_chain(r(0xB0)), bp_chain(r(0xE8)),
                compact_frames(rsp, 512, 6)
            ));
            let mut old = 0u32;
            VirtualProtect(rip as *mut c_void, 1, 0x40, &mut old);
            *(rip as *mut u8) = orig;
            VirtualProtect(rip as *mut c_void, 1, old, &mut old);
            if rearm {
                BP_REARM.with(|c| c.set(rip));
                let fl = (ctx as *mut u8).add(0x44) as *mut u32;
                *fl |= 0x100; // TF: single-step, then re-insert int3
            }
            return -1; // EXCEPTION_CONTINUE_EXECUTION
        } else {
            let g = GUEST_IMAGE_BASE.load(std::sync::atomic::Ordering::Acquire);
            let rva = rip.saturating_sub(g);
            raw_stderr(&format!("[debugbreak] handled at 0x{:x} (guest+0x{:x})\n", rip, rva));
            if rva == 0x1e1d4e0 {
                let r15 = *ctx.add(0xF0 / 8) as usize;
                let sz = r15.max(0x100000);
                let buf = VirtualAlloc(0, sz, 0x3000, 0x04);
                *ctx.add(0x78 / 8) = buf as u64;
                *ctx.add(0xF8 / 8) = (g + 0x1e1d512) as u64;
                raw_stderr(&format!("[debugbreak] rescued G+1e1d4e0 -> provided {sz:#x} buffer at {buf:#x}\n"));
                return -1;
            }
            if rva == 0x1e1d390 {
                let r15 = *ctx.add(0xF0 / 8) as usize;
                let sz = r15.max(0x100000);
                let buf = VirtualAlloc(0, sz, 0x3000, 0x04);
                *ctx.add(0x78 / 8) = buf as u64;
                *ctx.add(0xF8 / 8) = (g + 0x1e1d373) as u64;
                raw_stderr(&format!("[debugbreak] rescued G+1e1d390 -> provided {sz:#x} buffer at {buf:#x}\n"));
                return -1;
            }
            if rva == 0x1d5cc69 {
                // Assertion failure: cx was <= 0x3e7 (expected >= 1000).
                // Set eax = 1005 (so ebx = eax - 1000 = 5), skip `add eax, 5` directly to `ret` at +4
                *ctx.add(0x78 / 8) = 1005;
                *ctx.add(0xF8 / 8) = (rip + 4) as u64;
                return -1;
            }
            if rva == 0x2224517 {
                // Missing gameconfig.zmt failure branch. Clean epilogue:
                // pop registers and return
                *ctx.add(0xF8 / 8) = (rip + 1) as u64; // step to 0x142224518 add rsp, 0xc8
                return -1;
            }
            if rva == 0xcc2936 {
                // rcx was null. Cleanly return from 0x140cc2840:
                // add rsp, 0x38; pop rbx; pop rdi; pop rsi; pop r14; ret
                let mut cur_rsp = *ctx.add(0x98 / 8) + 0x38;
                let rbx = *(cur_rsp as *const u64); cur_rsp += 8;
                let rdi = *(cur_rsp as *const u64); cur_rsp += 8;
                let rsi = *(cur_rsp as *const u64); cur_rsp += 8;
                let r14 = *(cur_rsp as *const u64); cur_rsp += 8;
                let ret_addr = *(cur_rsp as *const u64); cur_rsp += 8;
                *ctx.add(0x90 / 8) = rbx; // rbx
                *ctx.add(0xB0 / 8) = rdi; // rdi
                *ctx.add(0xA8 / 8) = rsi; // rsi
                *ctx.add(0xE8 / 8) = r14; // r14
                *ctx.add(0x98 / 8) = cur_rsp; // rsp
                *ctx.add(0xF8 / 8) = ret_addr; // rip
                *ctx.add(0x78 / 8) = 0; // rax = 0
                raw_stderr(&format!("[debugbreak] handled null rcx at G+cc2936 -> returned 0 to {ret_addr:#x}\n"));
                return -1;
            }
            *ctx.add(0xF8 / 8) = (rip + 1) as u64;
            return -1; // EXCEPTION_CONTINUE_EXECUTION
        }
    }
    if code == 0xC000_0005 {
        let ep = info as *const *const u8;
        let er_ptr = *ep;
        let ctx = *(ep.add(1)) as *mut u64;
        let num_params = *(er_ptr.add(24) as *const u32);
        let op = if num_params >= 1 { *(er_ptr.add(32) as *const usize) } else { 0 };
        let fault_addr = if num_params >= 2 { *(er_ptr.add(40) as *const usize) } else { 0 };
        let rip = *ctx.add(0xF8 / 8);
        let rsp = *ctx.add(0x98 / 8);
        let guest = GUEST_IMAGE_BASE.load(std::sync::atomic::Ordering::Acquire);
        let dummy_vtbl = get_or_create_dummy_vtable();

        // 1. Check if execute fault on invalid address (e.g. rip is invalid or op == 8)
        if (op == 8 || rip < 0x10000 || rip == 0xffff_ffff_ffff_ffff) && is_readable_addr(rsp) {
            let ret_addr = *(rsp as *const u64);
            if is_readable_addr(ret_addr) {
                *ctx.add(0x98 / 8) = rsp + 8; // pop stack
                *ctx.add(0x78 / 8) = 0;       // rax = 0 (S_OK)
                *ctx.add(0xF8 / 8) = ret_addr;
                raw_stderr(&format!(
                    "[veh] 0xc0000005 execute fault at {rip:#x}. Simulating ret to {ret_addr:#x}, set rax=0 (S_OK)\n"
                ));
                return -1; // EXCEPTION_CONTINUE_EXECUTION
            }
        }

        // 2. Check if reading an invalid vtable during an indirect call (e.g. G+1d8e515 call [rax+0x40])
        if is_readable_addr(rip) {
            let call_len = if rip == guest as u64 + 0x1d8e515 {
                Some(3)
            } else {
                decode_call_len(rip as *const u8)
            };

            if let Some(len) = call_len {
                let rcx = *ctx.add(0x80 / 8);
                if dummy_vtbl != 0 && is_writable_addr(rcx) {
                    *(rcx as *mut usize) = dummy_vtbl;
                }
                *ctx.add(0xF8 / 8) = rip + len as u64;
                *ctx.add(0x78 / 8) = 0; // rax = 0 (S_OK)
                raw_stderr(&format!(
                    "[veh] 0xc0000005 vtable call fault at {rip:#x} (G+{:#x}), target={fault_addr:#x}. Stepped RIP +{len}, set rax=0 (S_OK)\n",
                    rip.saturating_sub(guest as u64)
                ));
                return -1; // EXCEPTION_CONTINUE_EXECUTION
            }

            // 3. Check if mov instruction loading vtable failed on invalid pointer
            let p = rip as *const u8;
            if *p == 0x48 && *p.add(1) == 0x8B {
                let modrm = *p.add(2);
                let dst_reg = (modrm >> 3) & 7;
                let mod_bits = (modrm >> 6) & 3;
                let rm_bits = modrm & 7;
                let has_sib = rm_bits == 4;
                let mut mov_len = 3 + if has_sib { 1 } else { 0 };
                if mod_bits == 1 {
                    mov_len += 1;
                } else if mod_bits == 2 || (mod_bits == 0 && (rm_bits == 5 || (has_sib && (*p.add(3) & 7) == 5))) {
                    mov_len += 4;
                }
                if dst_reg == 0 { // RAX
                    let next_p = p.add(mov_len);
                    let is_call_next = if is_readable_addr(next_p as u64) {
                        *next_p == 0xFF && ((*next_p.add(1) >> 3) & 7) == 2
                    } else {
                        false
                    };
                    if is_call_next {
                        if dummy_vtbl != 0 {
                            *ctx.add(0x78 / 8) = dummy_vtbl as u64;
                        }
                        *ctx.add(0xF8 / 8) = rip + mov_len as u64;
                        raw_stderr(&format!(
                            "[veh] 0xc0000005 mov vtable load fault at {rip:#x} (G+{:#x}). Set rax=dummy_vtbl, stepped RIP +{mov_len}\n",
                            rip.saturating_sub(guest as u64)
                        ));
                        return -1; // EXCEPTION_CONTINUE_EXECUTION
                    }
                }
            }
        }

        // 3a1. Null rcx at G+1cf0cbc (mov [rcx], 0xff in inner init loop called from G+1cff4b8)
        if rip == guest as u64 + 0x1cf0cbc && *ctx.add(0x80 / 8) == 0 {
            // Allocate 0x100000 for this table so [rcx] to [rcx + 0xb8 * count] is valid memory
            let buf = VirtualAlloc(0, 0x100000, 0x3000, 0x04);
            *ctx.add(0x80 / 8) = buf as u64; // rcx = buf
            *ctx.add(0xA8 / 8) = buf as u64; // rsi = buf (since 0x141cf0cb9 was mov rsi, rcx)
            let rip_table_ptr = (guest + 0x71bf8e0) as *mut usize;
            if is_writable_addr(rip_table_ptr as u64) {
                *rip_table_ptr = buf;
            }
            raw_stderr(&format!(
                "[veh] 0xc0000005 null rcx at G+1cf0cbc -> allocated buf {buf:#x}, stored in [0x1471bf8e0], retrying\n"
            ));
            return -1; // EXCEPTION_CONTINUE_EXECUTION
        }

        // 3a1b. Null heap rbx at G+1f67809 (mov [rbx + 0x10], r14d in heap allocator init)
        if (rip == guest as u64 + 0x1f67809 || (rip >= guest as u64 + 0x1f67770 && rip <= guest as u64 + 0x1f67850)) && *ctx.add(0x90 / 8) == 0 {
            static HEAP_EMERGENCY: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let hbuf = if HEAP_EMERGENCY.load(std::sync::atomic::Ordering::Acquire) != 0 {
                HEAP_EMERGENCY.load(std::sync::atomic::Ordering::Acquire)
            } else {
                let p = VirtualAlloc(0, 0x10000000, 0x3000, 0x04);
                HEAP_EMERGENCY.store(p, std::sync::atomic::Ordering::Release);
                p
            };
            *ctx.add(0x90 / 8) = (hbuf + 0x40) as u64; // rbx
            *ctx.add(0x88 / 8) = (hbuf + 0x40) as u64; // rdx
            raw_stderr(&format!("[veh] 0xc0000005 rescued null heap rbx at G+1f67809 -> rbx={hbuf:#x}\n"));
            return -1;
        }

        // 3a1c. DirectStorage queue dispatch at G+0x28c9a08: mov rax, [rcx]; call [rax+0x18]
        if rip == guest as u64 + 0x28c9a08 {
            let rsi = *ctx.add(0xA8 / 8); // rsi = streaming device object
            let rcx = *ctx.add(0x80 / 8); // rcx = [rsi + 0x18]
            if rcx == 0 {
                let q_obj = VirtualAlloc(0, 0x1000, 0x3000, 0x04) as usize;
                *(q_obj as *mut usize) = dummy_vtbl;
                *ctx.add(0x80 / 8) = q_obj as u64; // rcx = q_obj
                *ctx.add(0x78 / 8) = dummy_vtbl as u64; // rax = dummy_vtbl
                if is_writable_addr(rsi + 0x18) {
                    *((rsi + 0x18) as *mut usize) = q_obj;
                }
                *ctx.add(0xF8 / 8) = rip + 3; // step over mov rax, [rcx] to call [rax+0x18]
                raw_stderr(&format!(
                    "[veh] G+0x28c9a08: null queue at rsi={rsi:#x}+0x18, assigned fallback queue {q_obj:#x}\n"
                ));
                return -1;
            }
        }

        // 3a2. Uninitialized global at G+5eb24bf (mov rax, [rax+0x18] where rax == 0 from [0x1475df128])
        if rip == guest as u64 + 0x5eb24bf {
            // Epilogue of 0x145eb2480: mov dword ptr [rip + 0x2ec86a8], eax; ret
            // Simply set eax = 0, store to [0x148d7ab7d], pop ret address from stack
            let target_ptr = (guest + 0x8d7ab7d) as *mut u32;
            if is_writable_addr(target_ptr as u64) {
                *target_ptr = 0;
            }
            if is_readable_addr(rsp) {
                let ret_addr = *(rsp as *const u64);
                *ctx.add(0x98 / 8) = rsp + 8; // pop ret
                *ctx.add(0xF8 / 8) = ret_addr;
                *ctx.add(0x78 / 8) = 0; // rax = 0
                raw_stderr(&format!(
                    "[veh] 0xc0000005 null global at G+5eb24bf -> simulated ret to {ret_addr:#x}\n"
                ));
                return -1; // EXCEPTION_CONTINUE_EXECUTION
            }
        }

        // 3b. Pool allocator fault at G+55ac264 (mov r10, [rax+0x30] with rax==0)
        if rip == guest as u64 + 0x55ac264 {
            *ctx.add(0xF8 / 8) = guest as u64 + 0x55ac2a1; // jump to mov [rcx], r8 (r8=0); pop rbx/rdi/rsi; ret
            raw_stderr("[veh] 0xc0000005 pool alloc fault at G+55ac264: skipped to exit with null\n");
            return -1; // EXCEPTION_CONTINUE_EXECUTION
        }

        // 3c. Null rdi at G+cc2861 (test byte ptr [rdi+10h], 4)
        if rip == guest as u64 + 0xcc2861 {
            *ctx.add(0xF8 / 8) = guest as u64 + 0xcc288d;
            raw_stderr("[veh] 0xc0000005 at G+cc2861: rdi was null, jumped to G+cc288d\n");
            return -1; // EXCEPTION_CONTINUE_EXECUTION
        }

        // 3d. Null rcx at G+14a6639 (test byte ptr [rcx+10h], 4)
        if rip == guest as u64 + 0x14a6639 && *ctx.add(0x80 / 8) == 0 {
            let mut cur_rsp = *ctx.add(0x98 / 8) + 0x38;
            let rbx = *(cur_rsp as *const u64); cur_rsp += 8;
            let rdi = *(cur_rsp as *const u64); cur_rsp += 8;
            let rsi = *(cur_rsp as *const u64); cur_rsp += 8;
            let r14 = *(cur_rsp as *const u64); cur_rsp += 8;
            let ret_addr = *(cur_rsp as *const u64); cur_rsp += 8;
            *ctx.add(0x90 / 8) = rbx;
            *ctx.add(0xB0 / 8) = rdi;
            *ctx.add(0xA8 / 8) = rsi;
            *ctx.add(0xE8 / 8) = r14;
            *ctx.add(0x98 / 8) = cur_rsp;
            *ctx.add(0xF8 / 8) = ret_addr;
            *ctx.add(0x78 / 8) = 0; // rax = 0
            raw_stderr(&format!("[veh] 0xc0000005 null rcx at G+14a6639 -> returned 0 to {ret_addr:#x}\n"));
            return -1; // EXCEPTION_CONTINUE_EXECUTION
        }

        // 3e. Null rcx at G+24844a9 ([0x1475DF268])
        if rip == guest as u64 + 0x24844a9 {
            static GLOBAL_BUF: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let buf = if GLOBAL_BUF.load(std::sync::atomic::Ordering::Acquire) != 0 {
                GLOBAL_BUF.load(std::sync::atomic::Ordering::Acquire)
            } else {
                let p = VirtualAlloc(0, 0x100000, 0x3000, 0x04);
                GLOBAL_BUF.store(p, std::sync::atomic::Ordering::Release);
                p
            };
            let global_ptr = (guest + 0x75df268) as *mut usize;
            if is_writable_addr(global_ptr as u64) {
                *global_ptr = buf;
            }
            *ctx.add(0x80 / 8) = buf as u64; // rcx = buf
            raw_stderr(&format!("[veh] 0xc0000005 G+24844a9 null [0x1475DF268] -> allocated buf {buf:#x}, set rcx, retrying\n"));
            return -1; // EXCEPTION_CONTINUE_EXECUTION
        }

        // 4. Check if crash happened in graphics driver (nvwgf2umx / D3D12Core) or CRT with guest on stack
        let in_guest = rip >= guest as u64 && rip < guest as u64 + 0x10000000;
        if !in_guest && is_readable_addr(rsp) {
            for i in 0..128 {
                let slot = (rsp + i * 8) as *const u64;
                if !is_readable_addr(slot as u64) { break; }
                let val = *slot;
                if val == guest as u64 + 0x1cf0e38 {
                    let buf = VirtualAlloc(0, 0x10000, 0x3000, 0x04);
                    *ctx.add(0xD8 / 8) = buf as u64; // r12 = buf
                    *ctx.add(0x98 / 8) = (slot as u64) + 8; // pop memset frame
                    *ctx.add(0xF8 / 8) = guest as u64 + 0x1cf0e2b; // jump to mov rcx, r12
                    raw_stderr(&format!(
                        "[veh] 0xc0000005 null r12 at G+1cf0e38 -> allocated buf {buf:#x}, restarting at G+1cf0e2b\n"
                    ));
                    return -1; // EXCEPTION_CONTINUE_EXECUTION
                }
                if val == guest as u64 + 0x4ecc884 || val == guest as u64 + 0x2478c99 || val == guest as u64 + 0x1647616 {
                    *ctx.add(0x98 / 8) = (slot as u64) + 8;
                    *ctx.add(0xF8 / 8) = val;
                    *ctx.add(0x78 / 8) = 0;
                    raw_stderr(&format!(
                        "[veh] 0xc0000005 driver fault recovered -> resumed at guest+{:#x}\n",
                        val - guest as u64
                    ));
                    return -1; // EXCEPTION_CONTINUE_EXECUTION
                }
            }
        }

        // 5. Check if writing to null/near-null or uninitialized TLS object (e.g. G+0x19f5165 mov [rax+0x12338], rdi where rax == 0)
        if rip == guest as u64 + 0x19f5165 || (op == 1 && (0x1000..0x200000).contains(&fault_addr) && *ctx.add(0x78 / 8) == 0 && rip >= guest as u64 && rip < guest as u64 + 0x10000000) {
            static SCRATCH: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let scratch = if SCRATCH.load(std::sync::atomic::Ordering::Acquire) != 0 {
                SCRATCH.load(std::sync::atomic::Ordering::Acquire)
            } else {
                let p = VirtualAlloc(0, 0x400000, 0x3000, 0x04);
                SCRATCH.store(p, std::sync::atomic::Ordering::Release);
                p
            };
            *ctx.add(0x78 / 8) = scratch as u64;
            let mut v: *mut *mut u8 = std::ptr::null_mut();
            std::arch::asm!("mov {}, gs:[0x58]", out(reg) v, options(nostack, readonly, preserves_flags));
            if !v.is_null() && !(*v).is_null() {
                let tls_block = *v;
                let slot = tls_block.add(0x12a8) as *mut usize;
                *slot = scratch;
            }
            raw_stderr(&format!(
                "[veh] 0xc0000005 TLS object write fault at {rip:#x} (G+{:#x}), fault_addr={fault_addr:#x}. Set rax=scratch (0x{scratch:x}), retrying\n",
                rip.saturating_sub(guest as u64)
            ));
            return -1; // EXCEPTION_CONTINUE_EXECUTION
        }

        // 6. On-demand commit/protect on write/read fault to user-space memory (e.g. memcpy to reserved/uncommitted heap page)
        if (op == 0 || op == 1) && fault_addr >= 0x10000 && fault_addr < 0x7fff_ffff_0000 {
            let page_base = fault_addr & !0xFFF;
            let mut old_protect = 0u32;
            let vp = VirtualProtect(page_base as *mut c_void, 0x1000, 0x04, &mut old_protect);
            if vp != 0 && (old_protect == 0x01 || old_protect == 0x02) {
                raw_stderr(&format!(
                    "[veh] 0xc0000005 VirtualProtect -> PAGE_READWRITE at {page_base:#x} (old={old_protect:#x})\n"
                ));
                return -1; // EXCEPTION_CONTINUE_EXECUTION
            }
            let mut p = VirtualAlloc(page_base, 0x1000, 0x1000 /* MEM_COMMIT */, 0x04 /* PAGE_READWRITE */);
            if p == 0 {
                p = VirtualAlloc(page_base, 0x1000, 0x3000 /* MEM_RESERVE | MEM_COMMIT */, 0x04 /* PAGE_READWRITE */);
            }
            if p != 0 {
                raw_stderr(&format!(
                    "[veh] 0xc0000005 on-demand committed page at {page_base:#x} (op={op}, fault_addr={fault_addr:#x}, rip={rip:#x})\n"
                ));
                return -1; // EXCEPTION_CONTINUE_EXECUTION
            }
            // If in host memcpy called from guest, recover stack to guest caller
            if !in_guest && is_readable_addr(rsp) {
                for i in 0..128 {
                    let slot = (rsp + i * 8) as *const u64;
                    if !is_readable_addr(slot as u64) { break; }
                    let val = *slot;
                    if val == guest as u64 + 0x51d22dd {
                        *ctx.add(0x98 / 8) = (slot as u64) + 8;
                        *ctx.add(0xF8 / 8) = val;
                        *ctx.add(0x78 / 8) = *ctx.add(0x80 / 8); // rax = dest
                        raw_stderr(&format!(
                            "[veh] 0xc0000005 memcpy fault recovered -> resumed at guest+{:#x}\n",
                            val - guest as u64
                        ));
                        return -1; // EXCEPTION_CONTINUE_EXECUTION
                    }
                }
            }
        }
    }
    if code == 0xE06D_7363 || code == 0xC000_0005 {
        let ctx = *((info as *const *const u64).add(1));
        let _rip = *ctx.add(0xF8 / 8);
        let _rcx = *ctx.add(0x80 / 8);
        let _rdx = *ctx.add(0x88 / 8);
        let obj = *(er.add(10) as *const *const u64);
        let readable = |p: u64| {
            #[repr(C)]
            struct Mbi { base: usize, abase: usize, aprot: u32, size: usize, state: u32, prot: u32, ty: u32 }
            extern "system" {
                fn VirtualQuery(a: *const c_void, b: *mut Mbi, l: usize) -> usize;
            }
            let mut m: Mbi = std::mem::zeroed();
            VirtualQuery(p as *const c_void, &mut m, std::mem::size_of::<Mbi>()) != 0
                && m.state == 0x1000
                && m.prot & 0x6e != 0 // R / RW / WC / XR / XRW
                && m.prot & 0x100 == 0
        };
        let obj = if code == 0xE06D_7363 && !obj.is_null() && !readable(obj as u64) { std::ptr::null() } else { obj };
        let (_q0, q1, _q2, q3) = if code == 0xE06D_7363 && !obj.is_null() {
            (*obj, *obj.add(1), *obj.add(2), *obj.add(3))
        } else {
            (0, 0, 0, 0)
        };
        let _msg = if code == 0xE06D_7363 && q1 > 0x10000 && (q1 >> 48) == 0 && readable(q1) {
            std::ffi::CStr::from_ptr(q1 as *const i8).to_string_lossy().into_owned()
        } else {
            String::new()
        };
        let rsp = *ctx.add(0x98 / 8) as usize;
        let guest = GUEST_IMAGE_BASE.load(std::sync::atomic::Ordering::Acquire);
        let frames = compact_frames(rsp, 512, 8);
        let (rm, ro) = describe_addr(_rip as usize);
        let _ = guest;
        let extra = if code == 0xE06D_7363 { format!(" hr={q3:08x} {_msg}") } else { format!(" va={:#x} rcx={_rcx:#x} rdx={_rdx:#x} r8={:#x}", obj as usize, *ctx.add(0xB8 / 8)) };
        raw_stderr(&format!("[veh] {code:08x} @{} {extra} | {frames}\n", short_mod(&rm, ro)));
    }
    0 // EXCEPTION_CONTINUE_SEARCH
}

static MAPPED_RANGES: std::sync::Mutex<Vec<(usize, usize)>> = std::sync::Mutex::new(Vec::new());

/// Redirected `RtlPcToFileHeader` so MSVC C++ `throw` / SEH inside manually mapped PEs resolves `ImageBase`.
#[cfg(windows)]
unsafe extern "system" fn percy_rtl_pc_to_file_header(
    pc_value: *const c_void,
    base_of_image: *mut *mut c_void,
) -> *mut c_void {
    let pc = pc_value as usize;
    if let Ok(ranges) = MAPPED_RANGES.lock() {
        for &(start, end) in ranges.iter() {
            if pc >= start && pc < end {
                if !base_of_image.is_null() {
                    *base_of_image = start as *mut c_void;
                }
                return start as *mut c_void;
            }
        }
    }
    RtlPcToFileHeader(pc_value, base_of_image)
}

/// The VC runtime (msvcp140 `__ExceptionPtrCopyException`, vcruntime throw/catch, ucrt) turns
/// ThrowInfo / handler RVAs into pointers with `RtlPcToFileHeader`; the system loader does not know
/// mapped images and answers 0, so the RVA is dereferenced as an address. Point their imports at
/// our resolver.
#[cfg(windows)]
unsafe fn patch_runtime_pc_to_file_header() {
    for dll in ["msvcp140.dll", "vcruntime140.dll", "vcruntime140_1.dll", "ucrtbase.dll", "msvcp140_1.dll", "msvcp140_2.dll", "combase.dll"] {
        let w: Vec<u16> = dll.encode_utf16().chain(std::iter::once(0)).collect();
        let m = GetModuleHandleW(w.as_ptr());
        if m == 0 {
            continue;
        }
        let b = m as *const u8;
        let nt = b.add(*(b.add(0x3c) as *const u32) as usize);
        let (imp_rva, imp_sz) = (*(nt.add(0x90) as *const u32) as usize, *(nt.add(0x94) as *const u32) as usize);
        if imp_rva == 0 || imp_sz == 0 {
            continue;
        }
        let mut d = b.add(imp_rva) as *const u32;
        while *d.add(3) != 0 {
            let (oft, ft) = (*d as usize, *d.add(4) as usize);
            let names = if oft != 0 { oft } else { ft };
            let mut i = 0usize;
            loop {
                let th = *(b.add(names + i * 8) as *const u64);
                if th == 0 {
                    break;
                }
                if th & (1 << 63) == 0 {
                    let name = read_cstr_mem(b.add(th as usize + 2));
                    let hook = if name == "RtlPcToFileHeader" { Some(percy_rtl_pc_to_file_header as *const () as usize) } else { file_hook_for(&name) };
                    if let Some(h) = hook {
                        let slot = b.add(ft + i * 8) as *mut usize;
                        let mut old = 0u32;
                        VirtualProtect(slot as *mut c_void, 8, 0x04, &mut old);
                        *slot = h;
                        VirtualProtect(slot as *mut c_void, 8, old, &mut old);
                    }
                }
                i += 1;
            }
            d = d.add(5);
        }
    }
}

unsafe extern "system" fn unresolved_xbox_stub() -> i32 {
    0
}

fn ru16(b: &[u8], o: usize) -> u16 {
    b.get(o..o + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
        .unwrap_or(0)
}
fn ru32(b: &[u8], o: usize) -> u32 {
    b.get(o..o + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .unwrap_or(0)
}
fn ru64(b: &[u8], o: usize) -> u64 {
    b.get(o..o + 8)
        .map(|s| u64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))
        .unwrap_or(0)
}

unsafe fn read_cstr_mem(ptr: *const u8) -> String {
    let mut len = 0usize;
    while len < 512 && *ptr.add(len) != 0 {
        len += 1;
    }
    String::from_utf8_lossy(std::slice::from_raw_parts(ptr, len)).into_owned()
}

#[derive(Debug, Clone)]
pub struct LoadedImageStats {
    pub mapped_base: u64,
    pub entry_va: u64,
    pub pdata_entries: u32,
    pub tls_callbacks: usize,
    pub static_tls_bytes: usize,
    pub resolved_imports: usize,
    pub stubbed_imports: usize,
    pub relocations_applied: usize,
    pub missing_imports: Vec<String>,
}

#[cfg(windows)]
fn resolve_shim_or_host_dll(dll_name: &str, shim_dir: &Path, cache: &mut HashMap<String, usize>) -> usize {
    let key = dll_name.to_ascii_lowercase();
    if let Some(&h) = cache.get(&key) {
        return h;
    }
    let shim_names = [
        "xmem.dll",
        "kernelx.dll",
        "d3d12_xs.dll",
        "xg_xs.dll",
        "xgameruntime.dll",
        "dstorage.dll",
        "gameinput.dll",
        "winrt_xs.dll",
        "xaudio2_9.dll",
        "acphal.dll",
        "xapu.dll",
        "xcurl.dll",
        "amd_fidelityfx_loader_xbox.dll",
        "bink2scarlett.dll",
        "libhttpclient.gdk.dll",
        "xboxbvhbuilder.dll",
        "chad.dll",
        "xcrdapi.dll",
    ];
    let mut handle = 0usize;
    if key == "xcrdapi.dll" {
        let p = shim_dir.join("XCRDAPI.dll");
        if p.exists() {
            if let Ok(cs) = CString::new(p.to_string_lossy().as_bytes()) {
                handle = unsafe { LoadLibraryA(cs.as_ptr()) };
            }
        }
        if handle == 0 {
            let p = shim_dir.join("dstorage.dll");
            if let Ok(cs) = CString::new(p.to_string_lossy().as_bytes()) {
                handle = unsafe { LoadLibraryA(cs.as_ptr()) };
            }
        }
    } else if shim_names.contains(&key.as_str()) {
        let p = shim_dir.join(dll_name);
        if let Ok(cs) = CString::new(p.to_string_lossy().as_bytes()) {
            handle = unsafe { LoadLibraryA(cs.as_ptr()) };
        }
    }
    if handle == 0 {
        // Title-local DLLs (e.g. middleware shipped beside the exe) load by full path with
        // LOAD_WITH_ALTERED_SEARCH_PATH so their own dependencies resolve from the title folder.
        // Packaged (identity-overlay) processes do not search PATH, so this is required there.
        let (game_dir, _) = vfs_roots();
        let local = game_dir.join(dll_name);
        if local.is_file() {
            extern "system" {
                fn LoadLibraryExW(name: *const u16, file: usize, flags: u32) -> usize;
            }
            let mut w: Vec<u16> = local.to_string_lossy().encode_utf16().collect();
            w.push(0);
            handle = unsafe { LoadLibraryExW(w.as_ptr(), 0, 0x8) };
        }
    }
    if handle == 0 {
        if let Ok(cs) = CString::new(dll_name) {
            handle = unsafe { LoadLibraryA(cs.as_ptr()) };
        }
    }
    if handle == 0 && (key == "kernelx.dll" || key == "kernel32.dll") {
        let k32 = CString::new("kernel32.dll").unwrap();
        handle = unsafe { LoadLibraryA(k32.as_ptr()) };
    }
    cache.insert(key, handle);
    handle
}

/// Maps a PE32+ executable in-process, applies `.reloc` base relocations, resolves IAT imports
/// (prioritizing PercyScarlett shim DLLs), registers `.pdata` unwind entries via `RtlAddFunctionTable`,
/// invokes TLS callbacks, and executes `AddressOfEntryPoint`.
#[cfg(windows)]
pub fn load_and_run_pe64(data: &[u8], shim_dir: &Path, execute_entry: bool) -> Result<(LoadedImageStats, i32), String> {
    if data.len() < 0x100 || &data[0..2] != b"MZ" {
        return Err("Not an MZ executable".into());
    }
    let pe_off = ru32(data, 0x3C) as usize;
    if data.get(pe_off..pe_off + 4) != Some(b"PE\0\0") {
        return Err("Missing PE signature".into());
    }
    let machine = ru16(data, pe_off + 4);
    if machine != 0x8664 {
        return Err(format!("Unsupported machine 0x{machine:04x}"));
    }
    let n_sec = ru16(data, pe_off + 6) as usize;
    let opt_sz = ru16(data, pe_off + 20) as usize;
    let opt_off = pe_off + 24;
    if ru16(data, opt_off) != 0x020B {
        return Err("Expected PE32+ (0x020B)".into());
    }

    let entry_rva = ru32(data, opt_off + 16);
    let pref_base = ru64(data, opt_off + 24);
    let size_of_image = ru32(data, opt_off + 56) as usize;
    let size_of_headers = ru32(data, opt_off + 60) as usize;
    let num_rvas = ru32(data, opt_off + 108) as usize;
    let dd = |idx: usize| -> (u32, u32) {
        if idx < num_rvas {
            let o = opt_off + 112 + idx * 8;
            (ru32(data, o), ru32(data, o + 4))
        } else {
            (0, 0)
        }
    };

    unsafe {
        let mut base = VirtualAlloc(pref_base as usize, size_of_image, 0x3000, 0x04) as *mut c_void;
        if base.is_null() {
            base = VirtualAlloc(0, size_of_image, 0x3000, 0x04) as *mut c_void;
        }
        if base.is_null() {
            return Err("VirtualAlloc failed for PE32+ image".into());
        }
        let base_u8 = base as *mut u8;
        let hdr_copy = size_of_headers.min(data.len()).min(size_of_image);
        std::ptr::copy_nonoverlapping(data.as_ptr(), base_u8, hdr_copy);

        let sec_off = opt_off + opt_sz;
        let mut sec_protects = Vec::with_capacity(n_sec);
        for i in 0..n_sec {
            let o = sec_off + i * 40;
            let vsize = ru32(data, o + 8) as usize;
            let vaddr = ru32(data, o + 12) as usize;
            let rsize = ru32(data, o + 16) as usize;
            let rptr = ru32(data, o + 20) as usize;
            let chars = ru32(data, o + 36);
            if vaddr < size_of_image && rsize > 0 && rptr + rsize <= data.len() {
                let n = rsize.min(size_of_image - vaddr);
                std::ptr::copy_nonoverlapping(data.as_ptr().add(rptr), base_u8.add(vaddr), n);
            }
            sec_protects.push((vaddr, vsize.max(rsize), chars));
        }

        // Base relocations (.reloc, directory index 5)
        let delta = (base as u64).wrapping_sub(pref_base) as i64;
        let mut relocations_applied = 0usize;
        let (reloc_rva, reloc_sz) = dd(5);
        if delta != 0 && reloc_rva != 0 && reloc_sz >= 8 {
            let mut off = 0usize;
            while off + 8 <= reloc_sz as usize {
                let blk_rva = *(base_u8.add(reloc_rva as usize + off) as *const u32);
                let blk_sz = *(base_u8.add(reloc_rva as usize + off + 4) as *const u32) as usize;
                if blk_sz < 8 {
                    break;
                }
                let count = (blk_sz - 8) / 2;
                let entries = base_u8.add(reloc_rva as usize + off + 8) as *const u16;
                for j in 0..count {
                    let ent = *entries.add(j);
                    let ty = ent >> 12;
                    let r_off = (ent & 0x0FFF) as usize;
                    if ty == 10 {
                        // IMAGE_REL_BASED_DIR64
                        let p = base_u8.add(blk_rva as usize + r_off) as *mut u64;
                        *p = (*p as i64).wrapping_add(delta) as u64;
                        relocations_applied += 1;
                    }
                }
                off += blk_sz;
            }
        }

        // IAT import resolution (directory index 1)
        let mut dll_cache: HashMap<String, usize> = HashMap::new();
        let mut resolved_imports = 0usize;
        let mut stubbed_imports = 0usize;
        let mut missing_imports: Vec<String> = Vec::new();
        let (imp_rva, _) = dd(1);
        if imp_rva != 0 {
            let mut desc_off = imp_rva as usize;
            while desc_off + 20 <= size_of_image {
                let oft = *(base_u8.add(desc_off) as *const u32);
                let name_rva = *(base_u8.add(desc_off + 12) as *const u32);
                let ft = *(base_u8.add(desc_off + 16) as *const u32);
                if name_rva == 0 {
                    break;
                }
                let dll_name = read_cstr_mem(base_u8.add(name_rva as usize));
                let hmod = resolve_shim_or_host_dll(&dll_name, shim_dir, &mut dll_cache);
                let int_rva = if oft != 0 { oft } else { ft } as usize;
                let iat_rva = ft as usize;
                let mut idx = 0usize;
                while iat_rva + idx * 8 + 8 <= size_of_image {
                    let thunk = *(base_u8.add(int_rva + idx * 8) as *const u64);
                    if thunk == 0 {
                        break;
                    }
                    let mut addr = 0usize;
                    let mut sym_label = String::new();
                    if (thunk & (1u64 << 63)) != 0 {
                        let ord = (thunk & 0xFFFF) as usize;
                        if hmod != 0 {
                            addr = GetProcAddress(hmod, ord as *const i8);
                        }
                        if addr == 0 {
                            sym_label = format!("{dll_name}!#{ord}");
                        }
                    } else {
                        let hint_rva = (thunk & 0x7FFF_FFFF) as usize;
                        if hint_rva + 2 < size_of_image {
                            let fn_name_ptr = base_u8.add(hint_rva + 2) as *const i8;
                            if hmod != 0 {
                                let name_str = read_cstr_mem(fn_name_ptr as *const u8);
                                addr = match name_str.as_str() {
                                    "RtlPcToFileHeader" => percy_rtl_pc_to_file_header as *const () as usize,
                                    "GetModuleFileNameW" => percy_get_module_file_name_w as *const () as usize,
                                    "GetModuleFileNameA" => percy_get_module_file_name_a as *const () as usize,
                                    "GetModuleHandleW" => percy_get_module_handle_w as *const () as usize,
                                    "GetModuleHandleA" => percy_get_module_handle_a as *const () as usize,
                                    "CreateFileW" => percy_create_file_w as *const () as usize,
                                    "CreateFileA" => percy_create_file_a as *const () as usize,
                                    "GetFileAttributesW" => percy_get_file_attributes_w as *const () as usize,
                                    "GetDiskFreeSpaceExW" => percy_get_disk_free_space_ex_w as *const () as usize,
                                    "CreateDirectoryW" => percy_create_directory_w as *const () as usize,
                                    "GetFileAttributesExW" => percy_get_file_attributes_ex_w as *const () as usize,
                                    "FindFirstFileExW" => percy_find_first_file_ex_w as *const () as usize,
                                    "FindFirstFileW" => percy_find_first_file_w as *const () as usize,
                                    "MoveFileExW" => percy_move_file_ex_w as *const () as usize,
                                    "VirtualFree" => percy_virtual_free as *const () as usize,
                                    "free" | "realloc" if quarantine_on() && !heapchk_on() => {
                                        let real = GetProcAddress(hmod, fn_name_ptr);
                                        quarantine_hook(&name_str, real)
                                    }
                                    "malloc" | "calloc" | "realloc" | "free" if heapchk_on() => {
                                        let real = GetProcAddress(hmod, fn_name_ptr);
                                        heapchk_hook(&name_str, real)
                                    }
                                    "LoadLibraryExA" => {
                                        let real = GetProcAddress(hmod, fn_name_ptr);
                                        REAL_LOAD_LIBRARY_EX_A.store(real, std::sync::atomic::Ordering::Release);
                                        if real == 0 { 0 } else { percy_load_library_ex_a as *const () as usize }
                                    }
                                    "CreateProcessW" if std::env::var("PERCY_TRACE").is_ok() => {
                                        let real = GetProcAddress(hmod, fn_name_ptr);
                                        REAL_CREATE_PROCESS_W.store(real, std::sync::atomic::Ordering::Release);
                                        if real == 0 { 0 } else { percy_create_process_w as *const () as usize }
                                    }
                                    "?init@System@FMOD@@QEAA?AW4FMOD_RESULT@@HIPEAX@Z" => {
                                        let real = GetProcAddress(hmod, fn_name_ptr);
                                        let so = GetProcAddress(hmod, b"?setOutput@System@FMOD@@QEAA?AW4FMOD_RESULT@@W4FMOD_OUTPUTTYPE@@@Z\0".as_ptr() as *const i8);
                                        REAL_FMOD_INIT.store(real, std::sync::atomic::Ordering::Release);
                                        REAL_FMOD_SET_OUTPUT.store(so, std::sync::atomic::Ordering::Release);
                                        if real == 0 { 0 } else { percy_fmod_init as *const () as usize }
                                    }
                                    "FMOD_System_Create" => {
                                        let real = GetProcAddress(hmod, fn_name_ptr);
                                        REAL_FMOD_CREATE.store(real, std::sync::atomic::Ordering::Release);
                                        if real == 0 { 0 } else { percy_fmod_create as *const () as usize }
                                    }
                                    "GetProcAddress" if std::env::var("PERCY_TRACE").is_ok() => {
                                        percy_get_proc_address as *const () as usize
                                    }
                                    "ExitProcess" | "exit" if std::env::var("PERCY_TRACE").is_ok() => {
                                        let real = GetProcAddress(hmod, fn_name_ptr);
                                        if real == 0 {
                                            0
                                        } else if name_str == "exit" {
                                            REAL_CRT_EXIT.store(real, std::sync::atomic::Ordering::Release);
                                            percy_crt_exit as *const () as usize
                                        } else {
                                            REAL_EXIT_PROCESS.store(real, std::sync::atomic::Ordering::Release);
                                            percy_exit_process as *const () as usize
                                        }
                                    }
                                    _ => GetProcAddress(hmod, fn_name_ptr),
                                };
                                if addr == 0 {
                                    sym_label = format!("{dll_name}!{name_str}");
                                }
                            } else {
                                let s = read_cstr_mem(fn_name_ptr as *const u8);
                                sym_label = format!("{dll_name}!{s}");
                            }
                        }
                    }
                    if addr != 0 {
                        resolved_imports += 1;
                    } else {
                        addr = unresolved_xbox_stub as *const () as usize;
                        stubbed_imports += 1;
                        if !sym_label.is_empty() && !missing_imports.contains(&sym_label) && std::env::var("PERCY_TRACE").is_ok() {
                            raw_stderr(&format!("[imp] STUB {sym_label} (dll handle {hmod:#x})\n"));
                        }
                        if !sym_label.is_empty() && !missing_imports.contains(&sym_label) {
                            missing_imports.push(sym_label);
                        }
                    }
                    if std::env::var("PERCY_TRACE").is_ok() && thunk & (1u64 << 63) == 0 {
                        let hint_rva = (thunk & 0x7FFF_FFFF) as usize;
                        let nm = read_cstr_mem(base_u8.add(hint_rva + 2));
                        if nm == "GetProcAddress" || nm == "LoadLibraryExA" {
                            let (m, o) = describe_addr(addr);
                            raw_stderr(&format!("[imp] {dll_name}!{nm} -> {}\n", short_mod(&m, o)));
                        }
                    }
                    *(base_u8.add(iat_rva + idx * 8) as *mut usize) = addr;
                    idx += 1;
                }
                desc_off += 20;
            }
        }

        if execute_entry {
            arm_breakpoints(base_u8, size_of_image);
        }

        // Apply section protections
        for (vaddr, vsize, chars) in sec_protects {
            if vsize == 0 || vaddr >= size_of_image {
                continue;
            }
            let exec = (chars & 0x2000_0000) != 0;
            let write = (chars & 0x8000_0000) != 0;
            let prot = match (exec, write) {
                (true, true) => 0x40,   // PAGE_EXECUTE_READWRITE
                (true, false) => 0x20,  // PAGE_EXECUTE_READ
                (false, true) => 0x04,  // PAGE_READWRITE
                (false, false) => 0x02, // PAGE_READONLY
            };
            let mut old = 0u32;
            VirtualProtect(base_u8.add(vaddr) as *mut c_void, vsize, prot, &mut old);
        }

        if let Ok(mut ranges) = MAPPED_RANGES.lock() {
            ranges.push((base as usize, base as usize + size_of_image));
        }
        patch_runtime_pc_to_file_header();

        // Register sanitized .pdata SEH table (directory index 3)
        let (pdata_rva, pdata_sz) = dd(3);
        let pdata_entries = if pdata_rva != 0 && pdata_sz >= 12 {
            let raw_count = (pdata_sz / 12) as usize;
            let raw_ptr = base_u8.add(pdata_rva as usize) as *const [u32; 3];
            let mut valid: Vec<[u32; 3]> = (0..raw_count)
                .map(|i| *raw_ptr.add(i))
                .filter(|&[begin, end, _]| begin < end && (end as usize) <= size_of_image)
                .collect();
            valid.sort_unstable_by_key(|e| e[0]);
            let count = valid.len() as u32;
            if count > 0 {
                // Keep sanitized table alive in mapped image when possible, else leak boxed slice for process lifetime
                let tbl = valid.into_boxed_slice();
                let ptr = Box::into_raw(tbl) as *const c_void;
                let _ = RtlAddFunctionTable(ptr, count, base as u64);
            }
            count
        } else {
            0
        };

        // Static .tls (directory index 9). MSVC EXEs hard-code TLS index 0 (`mov rcx,[gs:58h]; mov rcx,[rcx]`),
        // so the guest owns slot 0 of the ntdll-managed ThreadLocalStoragePointer vector while it runs.
        // The vector itself is never replaced (ntdll grows/frees it); only entry 0 is swapped per thread.
        let mut tls_callbacks = 0usize;
        let mut static_tls_bytes = 0usize;
        let mut guest_tls_installed = false;
        let (tls_rva, tls_sz) = dd(9);
        if tls_rva != 0 && tls_sz >= 40 {
            let tls_dir = base_u8.add(tls_rva as usize) as *const u64;
            let raw_start = *tls_dir.add(0);
            let raw_end = *tls_dir.add(1);
            let addr_of_index = *tls_dir.add(2);
            let cb_va = *tls_dir.add(3);
            let zero_fill = (*((tls_dir.add(4)) as *const u32)) as usize;
            let raw_len = raw_end.saturating_sub(raw_start) as usize;
            let total_tls = raw_len + zero_fill;
            if addr_of_index != 0 {
                *(addr_of_index as *mut u32) = 0;
            }
            if total_tls > 0 && total_tls <= 16 * 1024 * 1024 {
                static_tls_bytes = total_tls;
                if execute_entry {
                    guest_tls::register(raw_start as usize, raw_len, total_tls, cb_va as usize, base as usize);
                    guest_tls_installed = guest_tls::install_current_thread();
                }
            }
            if cb_va != 0 {
                let mut cb_ptr = cb_va as *const u64;
                while *cb_ptr != 0 && tls_callbacks < 64 {
                    if execute_entry {
                        let cb: unsafe extern "system" fn(*mut c_void, u32, *mut c_void) =
                            std::mem::transmute(*cb_ptr as usize);
                        cb(base, 1 /* DLL_PROCESS_ATTACH */, std::ptr::null_mut());
                    }
                    tls_callbacks += 1;
                    cb_ptr = cb_ptr.add(1);
                }
            }
        }

        let entry_va = (base as u64).wrapping_add(entry_rva as u64);
        GUEST_IMAGE_BASE.store(base as usize, std::sync::atomic::Ordering::Release);
        AddVectoredExceptionHandler(1, crash_veh);
        if execute_entry {
            AddVectoredExceptionHandler(1, trace_veh);
        }
        let rc = if execute_entry && entry_rva != 0 {
            let entry_fn: unsafe extern "system" fn() -> i32 = std::mem::transmute(entry_va as usize);
            entry_fn()
        } else {
            0
        };
        if guest_tls_installed {
            guest_tls::restore_current_thread();
        }

        Ok((
            LoadedImageStats {
                mapped_base: base as u64,
                entry_va,
                pdata_entries,
                tls_callbacks,
                static_tls_bytes,
                resolved_imports,
                stubbed_imports,
                relocations_applied,
                missing_imports,
            },
            rc,
        ))
    }
}

/// Launches a target executable via Windows `CreateProcessW(CREATE_SUSPENDED)` with `shim_dir`
/// prepended to `PATH` and `PERCY_VFS_G` pointed at the executable's directory, then resumes it.
#[cfg(windows)]
pub fn launch_suspended_with_overlay(exe_path: &Path, shim_dir: &Path) -> Result<u32, String> {
    use std::os::windows::process::CommandExt;
    let exe_dir = exe_path.parent().unwrap_or_else(|| Path::new("."));
    let old_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{};{};{old_path}", shim_dir.display(), exe_dir.display());
    let child = std::process::Command::new(exe_path)
        .current_dir(exe_dir)
        .env("PATH", new_path)
        .env("PERCY_VFS_G", exe_dir)
        .env("PERCY_D3D12_WRAP", "1")
        .creation_flags(0x0000_0000)
        .spawn()
        .map_err(|e| format!("Failed to spawn {}: {e}", exe_path.display()))?;
    Ok(child.id())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn maps_relocates_registers_pdata_and_executes_pe64_in_process() {
        // Build a minimal valid PE32+ in memory with .text (`mov eax, 42; ret`), .pdata, and .reloc
        let mut pe = vec![0u8; 0x2000];
        pe[0..2].copy_from_slice(b"MZ");
        pe[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        pe[0x80..0x84].copy_from_slice(b"PE\0\0");
        pe[0x84..0x86].copy_from_slice(&0x8664u16.to_le_bytes());
        pe[0x86..0x88].copy_from_slice(&1u16.to_le_bytes());
        pe[0x94..0x96].copy_from_slice(&0xF0u16.to_le_bytes());
        pe[0x98..0x9A].copy_from_slice(&0x020Bu16.to_le_bytes());
        pe[0xA8..0xAC].copy_from_slice(&0x1000u32.to_le_bytes()); // entry RVA
        pe[0xB0..0xB8].copy_from_slice(&0x1_4000_0000u64.to_le_bytes()); // ImageBase
        pe[0xD0..0xD4].copy_from_slice(&0x2000u32.to_le_bytes()); // SizeOfImage
        pe[0xD4..0xD8].copy_from_slice(&0x200u32.to_le_bytes()); // SizeOfHeaders
        pe[0x104..0x108].copy_from_slice(&16u32.to_le_bytes());

        // .pdata at RVA 0x1020 (size 12): 1 RUNTIME_FUNCTION entry
        pe[0x120..0x124].copy_from_slice(&0x1020u32.to_le_bytes());
        pe[0x124..0x128].copy_from_slice(&12u32.to_le_bytes());

        // Section .text at VA=0x1000, RawPtr=0x1000, RawSize=0x1000, RX
        pe[0x188..0x18D].copy_from_slice(b".text");
        pe[0x190..0x194].copy_from_slice(&0x1000u32.to_le_bytes());
        pe[0x194..0x198].copy_from_slice(&0x1000u32.to_le_bytes());
        pe[0x198..0x19C].copy_from_slice(&0x1000u32.to_le_bytes());
        pe[0x19C..0x1A0].copy_from_slice(&0x1000u32.to_le_bytes());
        pe[0x1AC..0x1B0].copy_from_slice(&0x6000_0020u32.to_le_bytes());

        // x64 code at RVA 0x1000 (file 0x1000): mov eax, 42; ret
        pe[0x1000..0x1006].copy_from_slice(&[0xB8, 42, 0, 0, 0, 0xC3]);
        // RUNTIME_FUNCTION at 0x1020: Begin=0x1000, End=0x1006, Unwind=0x1030
        pe[0x1020..0x1024].copy_from_slice(&0x1000u32.to_le_bytes());
        pe[0x1024..0x1028].copy_from_slice(&0x1006u32.to_le_bytes());
        pe[0x1028..0x102C].copy_from_slice(&0x1030u32.to_le_bytes());
        pe[0x1030] = 0x01; // UNWIND_INFO version=1, flags=0

        let (stats, rc) = load_and_run_pe64(&pe, Path::new("."), true).expect("in-process PE64 run");
        assert_eq!(rc, 42);
        assert_eq!(stats.pdata_entries, 1);
        assert!(stats.missing_imports.is_empty());
    }

    #[test]
    fn maps_static_tls_and_sanitizes_pdata() {
        let mut pe = vec![0u8; 0x3000];
        pe[0..2].copy_from_slice(b"MZ");
        pe[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        pe[0x80..0x84].copy_from_slice(b"PE\0\0");
        pe[0x84..0x86].copy_from_slice(&0x8664u16.to_le_bytes());
        pe[0x86..0x88].copy_from_slice(&1u16.to_le_bytes());
        pe[0x94..0x96].copy_from_slice(&0xF0u16.to_le_bytes());
        pe[0x98..0x9A].copy_from_slice(&0x020Bu16.to_le_bytes());
        pe[0xA8..0xAC].copy_from_slice(&0x1000u32.to_le_bytes()); // entry RVA
        pe[0xB0..0xB8].copy_from_slice(&0x1_4000_0000u64.to_le_bytes()); // ImageBase
        pe[0xD0..0xD4].copy_from_slice(&0x3000u32.to_le_bytes()); // SizeOfImage
        pe[0xD4..0xD8].copy_from_slice(&0x200u32.to_le_bytes()); // SizeOfHeaders
        pe[0x104..0x108].copy_from_slice(&16u32.to_le_bytes());

        // .pdata at RVA 0x1020 (size 24): 1 valid + 1 invalid (begin >= end)
        pe[0x120..0x124].copy_from_slice(&0x1020u32.to_le_bytes());
        pe[0x124..0x128].copy_from_slice(&24u32.to_le_bytes());

        // .tls directory at RVA 0x2000 (size 40)
        pe[0x150..0x154].copy_from_slice(&0x2000u32.to_le_bytes());
        pe[0x154..0x158].copy_from_slice(&40u32.to_le_bytes());

        // Base relocation block at RVA 0x2100 (size 16) to relocate TLS raw_start, raw_end, addr_of_index
        pe[0x130..0x134].copy_from_slice(&0x2100u32.to_le_bytes());
        pe[0x134..0x138].copy_from_slice(&16u32.to_le_bytes());

        // Section .text at VA=0x1000, RawPtr=0x1000, RawSize=0x2000, RWX
        pe[0x188..0x18D].copy_from_slice(b".text");
        pe[0x190..0x194].copy_from_slice(&0x2000u32.to_le_bytes());
        pe[0x194..0x198].copy_from_slice(&0x1000u32.to_le_bytes());
        pe[0x198..0x19C].copy_from_slice(&0x2000u32.to_le_bytes());
        pe[0x19C..0x1A0].copy_from_slice(&0x1000u32.to_le_bytes());
        pe[0x1AC..0x1B0].copy_from_slice(&0xE000_0020u32.to_le_bytes());

        // mov eax, 7; ret
        pe[0x1000..0x1006].copy_from_slice(&[0xB8, 7, 0, 0, 0, 0xC3]);
        // Valid RUNTIME_FUNCTION
        pe[0x1020..0x1024].copy_from_slice(&0x1000u32.to_le_bytes());
        pe[0x1024..0x1028].copy_from_slice(&0x1006u32.to_le_bytes());
        pe[0x1028..0x102C].copy_from_slice(&0x1040u32.to_le_bytes());
        // Invalid RUNTIME_FUNCTION (begin == end)
        pe[0x102C..0x1030].copy_from_slice(&0x1050u32.to_le_bytes());
        pe[0x1030..0x1034].copy_from_slice(&0x1050u32.to_le_bytes());
        pe[0x1040] = 0x01;

        // IMAGE_TLS_DIRECTORY64 at 0x2000: raw 0x2040..0x2050 (16B), index at 0x2060, zero_fill=16
        pe[0x2000..0x2008].copy_from_slice(&0x1_4000_2040u64.to_le_bytes());
        pe[0x2008..0x2010].copy_from_slice(&0x1_4000_2050u64.to_le_bytes());
        pe[0x2010..0x2018].copy_from_slice(&0x1_4000_2060u64.to_le_bytes());
        pe[0x2020..0x2024].copy_from_slice(&16u32.to_le_bytes());

        // Reloc block at 0x2100: page RVA=0x2000, size=16, 3x DIR64 (0xA000 | off) + pad
        pe[0x2100..0x2104].copy_from_slice(&0x2000u32.to_le_bytes());
        pe[0x2104..0x2108].copy_from_slice(&16u32.to_le_bytes());
        pe[0x2108..0x210A].copy_from_slice(&0xA000u16.to_le_bytes());
        pe[0x210A..0x210C].copy_from_slice(&0xA008u16.to_le_bytes());
        pe[0x210C..0x210E].copy_from_slice(&0xA010u16.to_le_bytes());

        let (stats, rc) = load_and_run_pe64(&pe, Path::new("."), true).expect("in-process TLS PE64 run");
        assert_eq!(rc, 7);
        assert_eq!(stats.pdata_entries, 1);
        assert_eq!(stats.static_tls_bytes, 32);

        let mut out_base: *mut c_void = std::ptr::null_mut();
        let ret = unsafe { percy_rtl_pc_to_file_header((stats.mapped_base + 0x1002) as *const c_void, &mut out_base) };
        assert_eq!(ret as u64, stats.mapped_base);
        assert_eq!(out_base as u64, stats.mapped_base);
    }

    #[test]
    fn vfs_redirects_g_game_t_and_s_drives() {
        let gd = Path::new(r"C:\Games\Test");
        let sr = std::env::temp_dir().join("percy_vfs_test");
        assert_eq!(rewrite_xbox_vfs_path(r"G:\data\shaders.rpf", gd, &sr).unwrap(), gd.join(r"data\shaders.rpf"));
        assert_eq!(rewrite_xbox_vfs_path("GAME:/media/a.bin", gd, &sr).unwrap(), gd.join(r"media\a.bin"));
        assert_eq!(rewrite_xbox_vfs_path(r"T:\temp.dat", gd, &sr).unwrap(), sr.join("T").join("temp.dat"));
        assert_eq!(rewrite_xbox_vfs_path(r"S:\save\slot1", gd, &sr).unwrap(), sr.join("S").join(r"save\slot1"));
        assert!(rewrite_xbox_vfs_path(r"C:\Windows\System32\d3d12.dll", gd, &sr).is_none());
    }
}

/// Guest static-TLS ownership of ThreadLocalStoragePointer slot 0.
///
/// Every thread (main, guest-created, and runtime worker threads) gets a private copy of the guest
/// `.tls` template in slot 0 on `DLL_THREAD_ATTACH`, and the host block is put back on
/// `DLL_THREAD_DETACH` before ntdll / Rust std run their own teardown. Code in this module must not
/// touch Rust `thread_local!` storage: while the guest owns slot 0, host static TLS is unavailable.
#[cfg(windows)]
pub mod guest_tls {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

    extern "system" {
        fn GetProcessHeap() -> *mut c_void;
        fn HeapAlloc(heap: *mut c_void, flags: u32, bytes: usize) -> *mut c_void;
        fn HeapFree(heap: *mut c_void, flags: u32, mem: *mut c_void) -> i32;
        fn TlsAlloc() -> u32;
        fn TlsGetValue(idx: u32) -> *mut c_void;
        fn TlsSetValue(idx: u32, v: *mut c_void) -> i32;
    }

    static TEMPLATE: AtomicUsize = AtomicUsize::new(0);
    static RAW_LEN: AtomicUsize = AtomicUsize::new(0);
    static TOTAL: AtomicUsize = AtomicUsize::new(0);
    static CALLBACKS: AtomicUsize = AtomicUsize::new(0);
    static IMAGE_BASE: AtomicUsize = AtomicUsize::new(0);
    /// Dynamic TLS slot (TEB TlsSlots, not static TLS) holding the displaced host block.
    static SAVE_SLOT: AtomicU32 = AtomicU32::new(u32::MAX);
    pub static THREADS_ATTACHED: AtomicUsize = AtomicUsize::new(0);

    pub unsafe fn register(template: usize, raw_len: usize, total: usize, callbacks: usize, base: usize) {
        if SAVE_SLOT.load(Ordering::Acquire) == u32::MAX {
            SAVE_SLOT.store(TlsAlloc(), Ordering::Release);
        }
        RAW_LEN.store(raw_len, Ordering::Release);
        TOTAL.store(total, Ordering::Release);
        CALLBACKS.store(callbacks, Ordering::Release);
        IMAGE_BASE.store(base, Ordering::Release);
        TEMPLATE.store(template, Ordering::Release);
    }

    #[inline(always)]
    unsafe fn tls_vector() -> *mut *mut u8 {
        let v: *mut *mut u8;
        std::arch::asm!("mov {}, gs:[0x58]", out(reg) v, options(nostack, readonly, preserves_flags));
        v
    }

    /// Swaps a fresh guest block into slot 0 of the current thread. Returns false if no guest is registered.
    pub unsafe fn install_current_thread() -> bool {
        let total = TOTAL.load(Ordering::Acquire);
        let slot = SAVE_SLOT.load(Ordering::Acquire);
        let vec = tls_vector();
        if total == 0 || slot == u32::MAX || vec.is_null() || !TlsGetValue(slot).is_null() {
            return false;
        }
        let block = HeapAlloc(GetProcessHeap(), 0x8 /* HEAP_ZERO_MEMORY */, total) as *mut u8;
        if block.is_null() {
            return false;
        }
        let tmpl = TEMPLATE.load(Ordering::Acquire);
        let raw = RAW_LEN.load(Ordering::Acquire);
        if tmpl != 0 && raw > 0 {
            std::ptr::copy_nonoverlapping(tmpl as *const u8, block, raw);
        }
        let host = *vec;
        // Store a non-null marker even when the host block is null so install stays idempotent.
        TlsSetValue(slot, if host.is_null() { 1usize as *mut c_void } else { host as *mut c_void });
        *vec = block;
        THREADS_ATTACHED.fetch_add(1, Ordering::Relaxed);
        true
    }

    /// Puts the host block back into slot 0 and frees the guest block.
    pub unsafe fn restore_current_thread() {
        let slot = SAVE_SLOT.load(Ordering::Acquire);
        let vec = tls_vector();
        if slot == u32::MAX || vec.is_null() {
            return;
        }
        let saved = TlsGetValue(slot) as usize;
        if saved == 0 {
            return;
        }
        let guest = *vec;
        *vec = if saved == 1 { std::ptr::null_mut() } else { saved as *mut u8 };
        TlsSetValue(slot, std::ptr::null_mut());
        if !guest.is_null() {
            HeapFree(GetProcessHeap(), 0, guest as *mut c_void);
        }
    }

    unsafe fn run_guest_callbacks(reason: u32) {
        let cbs = CALLBACKS.load(Ordering::Acquire);
        let base = IMAGE_BASE.load(Ordering::Acquire);
        if cbs == 0 {
            return;
        }
        let mut p = cbs as *const usize;
        let mut n = 0;
        while *p != 0 && n < 64 {
            let cb: unsafe extern "system" fn(*mut c_void, u32, *mut c_void) = std::mem::transmute(*p);
            cb(base as *mut c_void, reason, std::ptr::null_mut());
            p = p.add(1);
            n += 1;
        }
    }

    unsafe extern "system" fn on_thread(_h: *mut c_void, reason: u32, _r: *mut c_void) {
        if TEMPLATE.load(Ordering::Acquire) == 0 && TOTAL.load(Ordering::Acquire) == 0 {
            return;
        }
        match reason {
            2 => {
                if install_current_thread() {
                    run_guest_callbacks(2);
                }
            }
            3 => {
                let slot = SAVE_SLOT.load(Ordering::Acquire);
                if slot != u32::MAX && !TlsGetValue(slot).is_null() {
                    run_guest_callbacks(3);
                    restore_current_thread();
                }
            }
            _ => {}
        }
    }

    // `.CRT$XLAA` sorts after the CRT's `$XLA` start marker and before Rust std's `$XLB` destructor
    // callback, so on detach the host block is back before std walks its thread-local destructors.
    #[link_section = ".CRT$XLAA"]
    #[used]
    static PERCY_GUEST_TLS_CALLBACK: unsafe extern "system" fn(*mut c_void, u32, *mut c_void) = on_thread;
}
