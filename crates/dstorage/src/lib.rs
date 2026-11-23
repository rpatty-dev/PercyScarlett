#![allow(non_snake_case)]
#![allow(dead_code)]

use std::ffi::c_void;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

const S_OK: i32 = 0;
const E_NOINTERFACE: i32 = 0x8000_4002u32 as i32;
const E_POINTER: i32 = 0x8000_4003u32 as i32;
const E_FAIL: i32 = 0x8000_4005u32 as i32;

#[cfg(windows)]
extern "system" {
    fn SetEvent(h_event: usize) -> i32;
    fn GetCurrentProcess() -> usize;
    fn DuplicateHandle(
        hSourceProcessHandle: usize,
        hSourceHandle: usize,
        hTargetProcessHandle: usize,
        lpTargetHandle: *mut usize,
        dwDesiredAccess: u32,
        bInheritHandle: i32,
        dwOptions: u32,
    ) -> i32;
}

#[cfg(not(windows))]
unsafe fn SetEvent(_h_event: usize) -> i32 {
    1
}

// Xbox Scarlett DirectStorage COM Vtables

#[repr(C)]
struct FactoryVtbl {
    // IUnknown
    query_interface: unsafe extern "system" fn(*mut c_void, *const c_void, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    // IDStorageFactoryX
    create_queue: unsafe extern "system" fn(*mut c_void, *const c_void, *const c_void, *mut *mut c_void) -> i32, // slot 3 (0x18)
    open_file: unsafe extern "system" fn(*mut c_void, *const u16, *const c_void, *mut *mut c_void) -> i32,       // slot 4 (0x20)
    create_status_array: unsafe extern "system" fn(*mut c_void, u32, *const i8, *const c_void, *mut *mut c_void) -> i32, // slot 5 (0x28)
    set_cpu_affinity: unsafe extern "system" fn(*mut c_void, u64) -> i32,                                        // slot 6 (0x30)
    set_debug_flags: unsafe extern "system" fn(*mut c_void, u32),                                                // slot 7 (0x38)
    set_staging_buffer_size: unsafe extern "system" fn(*mut c_void, u32) -> i32,                                 // slot 8 (0x40)
    // IDStorageFactoryX1
    get_stats: unsafe extern "system" fn(*mut c_void, *mut c_void),                                              // slot 9 (0x48)
    // IDStorageFactoryX2
    create_queue1: unsafe extern "system" fn(*mut c_void, *const c_void, *const c_void, *mut *mut c_void) -> i32,// slot 10 (0x50)
    _pad: [unsafe extern "system" fn() -> i32; 21],
}

#[repr(C)]
struct FileVtbl {
    // IUnknown
    query_interface: unsafe extern "system" fn(*mut c_void, *const c_void, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    // IDStorageFileX
    close: unsafe extern "system" fn(*mut c_void),             // slot 3 (0x18)
    get_handle: unsafe extern "system" fn(*mut c_void) -> usize,// slot 4 (0x20)
    _pad: [unsafe extern "system" fn() -> i32; 27],
}

#[repr(C)]
struct QueueVtbl {
    // IUnknown
    query_interface: unsafe extern "system" fn(*mut c_void, *const c_void, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    // IDStorageQueueX
    enqueue_request: unsafe extern "system" fn(*mut c_void, *const DStorageRequest), // slot 3 (0x18)
    enqueue_status: unsafe extern "system" fn(*mut c_void, *mut c_void, u32),        // slot 4 (0x20)
    enqueue_signal: unsafe extern "system" fn(*mut c_void, *mut c_void, u64),        // slot 5 (0x28)
    submit: unsafe extern "system" fn(*mut c_void),                                  // slot 6 (0x30)
    cancel_requests_with_tag: unsafe extern "system" fn(*mut c_void, u64, u64),       // slot 7 (0x38)
    close: unsafe extern "system" fn(*mut c_void),                                   // slot 8 (0x40)
    get_error_event: unsafe extern "system" fn(*mut c_void) -> usize,                // slot 9 (0x48)
    retrieve_error_record: unsafe extern "system" fn(*mut c_void, *mut u8),          // slot 10 (0x50)
    query: unsafe extern "system" fn(*mut c_void, *mut u8),                          // slot 11 (0x58)
    // IDStorageQueueX1
    enqueue_set_event: unsafe extern "system" fn(*mut c_void, usize),                 // slot 12 (0x60)
    // IDStorageQueueX2
    query1: unsafe extern "system" fn(*mut c_void, *mut u8),                         // slot 13 (0x68)
    _pad: [unsafe extern "system" fn() -> i32; 18],
}

#[repr(C)]
struct StatusArrayVtbl {
    // IUnknown
    query_interface: unsafe extern "system" fn(*mut c_void, *const c_void, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    // IDStorageStatusArrayX
    is_complete: unsafe extern "system" fn(*mut c_void, u32) -> bool, // slot 3 (0x18)
    get_hresult: unsafe extern "system" fn(*mut c_void, u32) -> i32, // slot 4 (0x20)
    _pad: [unsafe extern "system" fn() -> i32; 27],
}

// Layout verified against game binary and dstorage_xs_abi.h (72 bytes)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DStorageRequest {
    pub options: u64,            // 0..8
    pub destination: *mut u8,    // 8..16
    pub _dest_pad: u64,          // 16..24
    pub destination_size: u32,   // 24..28
    pub _pad0: u32,              // 28..32
    pub source: *const u8,       // 32..40
    pub file_offset: u64,        // 40..48
    pub source_size: u32,        // 48..52
    pub intermediate_size: u32,  // 52..56
    pub cancellation_tag: u64,   // 56..64
    pub name: *const i8,         // 64..72
}

#[repr(C)]
struct ComFactory {
    vtbl: *const FactoryVtbl,
    refs: AtomicU32,
}

#[repr(C)]
struct ComFile {
    vtbl: *const FileVtbl,
    refs: AtomicU32,
    path: PathBuf,
    size: u64,
    raw_handle: usize,
}

#[repr(C)]
struct ComQueue {
    vtbl: *const QueueVtbl,
    refs: AtomicU32,
    pending_events: Mutex<Vec<usize>>,
    pending_fences: Mutex<Vec<(*mut c_void, u64)>>,
}

#[repr(C)]
struct ComStatusArray {
    vtbl: *const StatusArrayVtbl,
    refs: AtomicU32,
    capacity: u32,
}

unsafe extern "system" fn dummy_slot_fn() -> i32 {
    0
}

// ---- ComFactory implementation ----

unsafe extern "system" fn factory_qi(this: *mut c_void, _riid: *const c_void, out: *mut *mut c_void) -> i32 {
    if out.is_null() {
        return E_POINTER;
    }
    factory_add_ref(this);
    *out = this;
    S_OK
}

unsafe extern "system" fn factory_add_ref(this: *mut c_void) -> u32 {
    (*(this as *const ComFactory)).refs.fetch_add(1, Ordering::Relaxed) + 1
}

unsafe extern "system" fn factory_release(this: *mut c_void) -> u32 {
    let prev = (*(this as *const ComFactory)).refs.fetch_sub(1, Ordering::Release);
    if prev == 1 {
        std::sync::atomic::fence(Ordering::Acquire);
        drop(Box::from_raw(this as *mut ComFactory));
    }
    prev - 1
}

unsafe extern "system" fn factory_create_queue(
    _this: *mut c_void,
    desc: *const c_void,
    riid: *const c_void,
    out_q: *mut *mut c_void,
) -> i32 {
    if out_q.is_null() {
        eprintln!("[dstorage] Factory CreateQueue called with null out_q!");
        return E_POINTER;
    }
    let q = Box::new(ComQueue {
        vtbl: &QUEUE_VTBL,
        refs: AtomicU32::new(1),
        pending_events: Mutex::new(Vec::new()),
        pending_fences: Mutex::new(Vec::new()),
    });
    let ptr = Box::into_raw(q) as *mut c_void;
    *out_q = ptr;
    eprintln!("[dstorage] Factory CreateQueue -> {:p} (desc={:p}, riid={:p})", ptr, desc, riid);
    S_OK
}

unsafe extern "system" fn factory_create_queue1(
    this: *mut c_void,
    desc: *const c_void,
    riid: *const c_void,
    out_q: *mut *mut c_void,
) -> i32 {
    eprintln!("[dstorage] Factory CreateQueue1 called -> forwarding to CreateQueue");
    factory_create_queue(this, desc, riid, out_q)
}

fn get_game_dir() -> PathBuf {
    if let Ok(p) = std::env::var("PERCY_GAME_DIR") {
        return PathBuf::from(p);
    }
    if let Ok(p) = std::env::var("PERCY_VFS_G") {
        return PathBuf::from(p);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            return parent.to_path_buf();
        }
    }
    PathBuf::from(r"S:\gta\Gta6fordevelopersacces")
}

fn resolve_vfs_path(raw: &str) -> PathBuf {
    let clean = raw.trim().trim_matches('"');
    let norm = clean.replace('/', "\\");
    let lower = norm.to_ascii_lowercase();

    let game_dir = get_game_dir();

    // Check specific Xbox mount prefixes
    if lower.starts_with("g:\\") {
        return game_dir.join(&norm[3..]);
    }
    if lower.starts_with("s:\\gta\\gta6fordevelopersacces\\") {
        return PathBuf::from(&norm);
    }
    if lower.starts_with("common:\\") || lower.starts_with("common:/") {
        return game_dir.join("common").join(&norm[8..]);
    }
    if lower.starts_with("platform:\\") || lower.starts_with("platform:/") {
        return game_dir.join("platform").join(&norm[10..]);
    }
    if lower.starts_with("game:\\") || lower.starts_with("title:\\") {
        return game_dir.join(&norm[6..]);
    }
    if lower.starts_with("t:\\") {
        return std::env::temp_dir().join("percy_T").join(&norm[3..]);
    }

    let p = PathBuf::from(&norm);
    if p.exists() {
        return p;
    }
    let direct = game_dir.join(&norm);
    if direct.exists() {
        return direct;
    }
    if let Some(fname) = p.file_name() {
        let in_game = game_dir.join(fname);
        if in_game.exists() {
            return in_game;
        }
    }
    direct
}

unsafe extern "system" fn factory_open_file(
    _this: *mut c_void,
    path_w: *const u16,
    _riid: *const c_void,
    out_file: *mut *mut c_void,
) -> i32 {
    if out_file.is_null() || path_w.is_null() {
        return E_POINTER;
    }
    let mut len = 0usize;
    while *path_w.add(len) != 0 {
        len += 1;
    }
    let s = String::from_utf16_lossy(std::slice::from_raw_parts(path_w, len));
    let p = resolve_vfs_path(&s);
    let Ok(meta) = std::fs::metadata(&p) else {
        eprintln!("[XCRDAPI] OpenFile: file not found: {} (resolved: {})", s, p.display());
        *out_file = std::ptr::null_mut();
        return E_FAIL;
    };

    let raw_handle = {
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            match std::fs::OpenOptions::new()
                .read(true)
                .share_mode(7) // FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
                .open(&p)
            {
                Ok(f) => {
                    use std::os::windows::io::IntoRawHandle;
                    f.into_raw_handle() as usize
                }
                Err(_) => 0usize,
            }
        }
        #[cfg(not(windows))]
        {
            0usize
        }
    };

    let f = Box::new(ComFile {
        vtbl: &FILE_VTBL,
        refs: AtomicU32::new(1),
        path: p,
        size: meta.len(),
        raw_handle,
    });
    *out_file = Box::into_raw(f) as *mut c_void;
    S_OK
}

unsafe extern "system" fn factory_create_status(
    _this: *mut c_void,
    cap: u32,
    _name: *const i8,
    _riid: *const c_void,
    out: *mut *mut c_void,
) -> i32 {
    if out.is_null() {
        return E_POINTER;
    }
    let status = Box::new(ComStatusArray {
        vtbl: &STATUS_ARRAY_VTBL,
        refs: AtomicU32::new(1),
        capacity: cap,
    });
    *out = Box::into_raw(status) as *mut c_void;
    S_OK
}

unsafe extern "system" fn factory_set_cpu_affinity(_this: *mut c_void, _mask: u64) -> i32 {
    S_OK
}

unsafe extern "system" fn factory_set_debug_flags(_this: *mut c_void, _flags: u32) {}

unsafe extern "system" fn factory_set_staging(_this: *mut c_void, _sz: u32) -> i32 {
    S_OK
}

unsafe extern "system" fn factory_get_stats(_this: *mut c_void, stats: *mut c_void) {
    if !stats.is_null() {
        std::ptr::write_bytes(stats as *mut u8, 0, 316);
    }
}

// ---- ComFile implementation ----

unsafe extern "system" fn file_qi(this: *mut c_void, _riid: *const c_void, out: *mut *mut c_void) -> i32 {
    if out.is_null() {
        return E_POINTER;
    }
    file_add_ref(this);
    *out = this;
    S_OK
}

unsafe extern "system" fn file_add_ref(this: *mut c_void) -> u32 {
    (*(this as *const ComFile)).refs.fetch_add(1, Ordering::Relaxed) + 1
}

unsafe extern "system" fn file_release(this: *mut c_void) -> u32 {
    let prev = (*(this as *const ComFile)).refs.fetch_sub(1, Ordering::Release);
    if prev == 1 {
        std::sync::atomic::fence(Ordering::Acquire);
        let f = Box::from_raw(this as *mut ComFile);
        #[cfg(windows)]
        if f.raw_handle != 0 {
            extern "system" {
                fn CloseHandle(hObject: usize) -> i32;
            }
            CloseHandle(f.raw_handle);
        }
    }
    prev - 1
}

unsafe extern "system" fn file_close(_this: *mut c_void) {}

unsafe extern "system" fn file_get_handle(this: *mut c_void) -> usize {
    let f = &*(this as *const ComFile);
    #[cfg(windows)]
    if f.raw_handle != 0 {
        let mut target = 0usize;
        let proc = GetCurrentProcess();
        if DuplicateHandle(proc, f.raw_handle, proc, &mut target, 0, 0, 2) != 0 {
            return target;
        }
    }
    f.raw_handle
}

// ---- ComQueue implementation ----

unsafe extern "system" fn queue_qi(this: *mut c_void, _riid: *const c_void, out: *mut *mut c_void) -> i32 {
    if out.is_null() {
        return E_POINTER;
    }
    queue_add_ref(this);
    *out = this;
    S_OK
}

unsafe extern "system" fn queue_add_ref(this: *mut c_void) -> u32 {
    (*(this as *const ComQueue)).refs.fetch_add(1, Ordering::Relaxed) + 1
}

unsafe extern "system" fn queue_release(this: *mut c_void) -> u32 {
    let prev = (*(this as *const ComQueue)).refs.fetch_sub(1, Ordering::Release);
    if prev == 1 {
        std::sync::atomic::fence(Ordering::Acquire);
        drop(Box::from_raw(this as *mut ComQueue));
    }
    prev - 1
}

unsafe extern "system" fn queue_enqueue_req(_this: *mut c_void, req: *const DStorageRequest) {
    if req.is_null() {
        return;
    }
    let r = &*req;
    if r.destination.is_null() || r.destination_size == 0 {
        return;
    }
    let is_memory = (r.options & (1 << 22)) != 0;
    let is_zlib = (r.options & 1) != 0;

    if is_memory {
        if r.source.is_null() || r.source_size == 0 {
            return;
        }
        let src_slice = std::slice::from_raw_parts(r.source, r.source_size as usize);
        if is_zlib {
            use flate2::read::ZlibDecoder;
            let mut decoder = ZlibDecoder::new(src_slice);
            let dst_slice = std::slice::from_raw_parts_mut(r.destination, r.destination_size as usize);
            let _ = decoder.read_exact(dst_slice);
        } else {
            let copy_len = (r.source_size as usize).min(r.destination_size as usize);
            std::ptr::copy_nonoverlapping(r.source, r.destination, copy_len);
        }
    } else {
        // File source: r.source is *const ComFile
        if r.source.is_null() || r.source_size == 0 {
            return;
        }
        let com_file = &*(r.source as *const ComFile);
        if let Ok(mut file) = File::open(&com_file.path) {
            if file.seek(SeekFrom::Start(r.file_offset)).is_ok() {
                if is_zlib {
                    use flate2::read::ZlibDecoder;
                    let mut src_buf = vec![0u8; r.source_size as usize];
                    if file.read_exact(&mut src_buf).is_ok() {
                        let mut decoder = ZlibDecoder::new(&src_buf[..]);
                        let dst_slice = std::slice::from_raw_parts_mut(r.destination, r.destination_size as usize);
                        let _ = decoder.read_exact(dst_slice);
                    }
                } else {
                    let dst_slice = std::slice::from_raw_parts_mut(
                        r.destination,
                        (r.source_size as usize).min(r.destination_size as usize),
                    );
                    let _ = file.read_exact(dst_slice);
                }
            }
        }
    }
}

unsafe extern "system" fn queue_enqueue_status(_this: *mut c_void, _status: *mut c_void, _idx: u32) {}

unsafe extern "system" fn queue_enqueue_signal(this: *mut c_void, fence: *mut c_void, value: u64) {
    if !fence.is_null() {
        (*(this as *const ComQueue)).pending_fences.lock().unwrap().push((fence, value));
    }
}

unsafe extern "system" fn queue_submit(this: *mut c_void) {
    let q = &*(this as *const ComQueue);
    let mut evs = q.pending_events.lock().unwrap();
    #[cfg(windows)]
    for ev in evs.drain(..) {
        SetEvent(ev);
    }
    #[cfg(not(windows))]
    evs.clear();

    let mut fences = q.pending_fences.lock().unwrap();
    for (fence, value) in fences.drain(..) {
        unsafe {
            // ID3D12Fence::Signal is slot 6
            let vtbl = *(fence as *const *const usize);
            let signal_fn: unsafe extern "system" fn(*mut c_void, u64) -> i32 = std::mem::transmute(*vtbl.add(6));
            signal_fn(fence, value);
        }
    }
}

unsafe extern "system" fn queue_cancel(_this: *mut c_void, _mask: u64, _val: u64) {}

unsafe extern "system" fn queue_close(_this: *mut c_void) {}

unsafe extern "system" fn queue_get_error_event(_this: *mut c_void) -> usize {
    0
}

unsafe extern "system" fn queue_retrieve_err(_this: *mut c_void, out: *mut u8) {
    if !out.is_null() {
        std::ptr::write_bytes(out, 0, 656);
    }
}

unsafe extern "system" fn queue_query(_this: *mut c_void, out: *mut u8) {
    if !out.is_null() {
        std::ptr::write_bytes(out, 0, 32);
    }
}

unsafe extern "system" fn queue_enqueue_event(this: *mut c_void, ev: usize) {
    if ev != 0 {
        (*(this as *const ComQueue)).pending_events.lock().unwrap().push(ev);
    }
}

unsafe extern "system" fn queue_query1(_this: *mut c_void, out: *mut u8) {
    if !out.is_null() {
        std::ptr::write_bytes(out, 0, 40);
    }
}

// ---- ComStatusArray implementation ----

unsafe extern "system" fn status_qi(this: *mut c_void, _riid: *const c_void, out: *mut *mut c_void) -> i32 {
    if out.is_null() {
        return E_POINTER;
    }
    status_add_ref(this);
    *out = this;
    S_OK
}

unsafe extern "system" fn status_add_ref(this: *mut c_void) -> u32 {
    (*(this as *const ComStatusArray)).refs.fetch_add(1, Ordering::Relaxed) + 1
}

unsafe extern "system" fn status_release(this: *mut c_void) -> u32 {
    let prev = (*(this as *const ComStatusArray)).refs.fetch_sub(1, Ordering::Release);
    if prev == 1 {
        std::sync::atomic::fence(Ordering::Acquire);
        drop(Box::from_raw(this as *mut ComStatusArray));
    }
    prev - 1
}

unsafe extern "system" fn status_is_complete(_this: *mut c_void, _index: u32) -> bool {
    true
}

unsafe extern "system" fn status_get_hresult(_this: *mut c_void, _index: u32) -> i32 {
    S_OK
}

// ---- Static Vtables ----

static FACTORY_VTBL: FactoryVtbl = FactoryVtbl {
    query_interface: factory_qi,
    add_ref: factory_add_ref,
    release: factory_release,
    create_queue: factory_create_queue,
    open_file: factory_open_file,
    create_status_array: factory_create_status,
    set_cpu_affinity: factory_set_cpu_affinity,
    set_debug_flags: factory_set_debug_flags,
    set_staging_buffer_size: factory_set_staging,
    get_stats: factory_get_stats,
    create_queue1: factory_create_queue1,
    _pad: [dummy_slot_fn; 21],
};

static FILE_VTBL: FileVtbl = FileVtbl {
    query_interface: file_qi,
    add_ref: file_add_ref,
    release: file_release,
    close: file_close,
    get_handle: file_get_handle,
    _pad: [dummy_slot_fn; 27],
};

static QUEUE_VTBL: QueueVtbl = QueueVtbl {
    query_interface: queue_qi,
    add_ref: queue_add_ref,
    release: queue_release,
    enqueue_request: queue_enqueue_req,
    enqueue_status: queue_enqueue_status,
    enqueue_signal: queue_enqueue_signal,
    submit: queue_submit,
    cancel_requests_with_tag: queue_cancel,
    close: queue_close,
    get_error_event: queue_get_error_event,
    retrieve_error_record: queue_retrieve_err,
    query: queue_query,
    enqueue_set_event: queue_enqueue_event,
    query1: queue_query1,
    _pad: [dummy_slot_fn; 18],
};

static STATUS_ARRAY_VTBL: StatusArrayVtbl = StatusArrayVtbl {
    query_interface: status_qi,
    add_ref: status_add_ref,
    release: status_release,
    is_complete: status_is_complete,
    get_hresult: status_get_hresult,
    _pad: [dummy_slot_fn; 27],
};

// ---- Exported DLL entry points ----

#[no_mangle]
pub unsafe extern "system" fn DStorageSetConfiguration(_config: *const c_void) -> i32 {
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn DStorageSetConfiguration1(_config: *const c_void) -> i32 {
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn DStorageGetFactory(
    _riid: *const c_void,
    out_factory: *mut *mut c_void,
) -> i32 {
    if out_factory.is_null() {
        return E_POINTER;
    }

    let f = Box::new(ComFactory {
        vtbl: &FACTORY_VTBL,
        refs: AtomicU32::new(1),
    });
    *out_factory = Box::into_raw(f) as *mut c_void;
    S_OK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xcrdapi_vtable_offsets_match_game_calls() {
        let f_base = &FACTORY_VTBL as *const FactoryVtbl as usize;
        let open_file_ptr = &FACTORY_VTBL.open_file as *const _ as usize;
        let status_array_ptr = &FACTORY_VTBL.create_status_array as *const _ as usize;
        let staging_ptr = &FACTORY_VTBL.set_staging_buffer_size as *const _ as usize;
        let create_q1_ptr = &FACTORY_VTBL.create_queue1 as *const _ as usize;

        // Slot 4: OpenFile = 0x20
        assert_eq!(open_file_ptr - f_base, 0x20);
        // Slot 5: CreateStatusArray = 0x28
        assert_eq!(status_array_ptr - f_base, 0x28);
        // Slot 8: SetStagingBufferSize = 0x40
        assert_eq!(staging_ptr - f_base, 0x40);
        // Slot 10: CreateQueue1 = 0x50
        assert_eq!(create_q1_ptr - f_base, 0x50);
    }
}
