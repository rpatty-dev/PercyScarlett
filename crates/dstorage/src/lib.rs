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
    fn VirtualQuery(addr: *const c_void, buf: *mut Mbi, len: usize) -> usize;
    fn VirtualAlloc(addr: *mut c_void, size: usize, alloc_type: u32, protect: u32) -> *mut c_void;
}

#[cfg(windows)]
#[repr(C)]
struct Mbi {
    base: usize,
    alloc_base: usize,
    alloc_prot: u32,
    part: u16,
    size: usize,
    state: u32,
    prot: u32,
    ty: u32,
}

#[cfg(windows)]
unsafe fn ensure_writable(dst: *mut u8, len: usize) -> bool {
    let mut cur = dst as usize;
    let end = cur.saturating_add(len);
    while cur < end {
        let mut m: Mbi = std::mem::zeroed();
        if VirtualQuery(cur as *const c_void, &mut m, std::mem::size_of::<Mbi>()) == 0 || m.size == 0 {
            return false;
        }
        let chunk_end = m.base.saturating_add(m.size).min(end);
        let chunk = chunk_end.saturating_sub(cur);
        if m.state == 0x2000 {
            // MEM_RESERVE (e.g. console physical pool VA): commit on demand before DirectStorage write.
            let p = VirtualAlloc(cur as *mut c_void, chunk, 0x1000, 0x04);
            if p.is_null() {
                static NF: AtomicU32 = AtomicU32::new(0);
                if NF.fetch_add(1, Ordering::Relaxed) < 8 {
                    eprintln!("[dstorage] commit MEM_RESERVE failed va={cur:#x} len={chunk:#x}");
                }
                return false;
            }
        } else if m.state != 0x1000 || (m.prot & 0xCC) == 0 {
            return false;
        }
        cur = chunk_end;
    }
    true
}

#[repr(C)]
struct FactoryVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const c_void, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    create_queue: unsafe extern "system" fn(*mut c_void, *const c_void, *const c_void, *mut *mut c_void) -> i32,
    open_file: unsafe extern "system" fn(*mut c_void, *const u16, *const c_void, *mut *mut c_void) -> i32,
    create_status_array: unsafe extern "system" fn(*mut c_void, u32, *const u8, *const c_void, *mut *mut c_void) -> i32,
    // Console IDStorageFactoryX: slot 8 = SetStagingBufferSize (title call site).
    slot6: unsafe extern "system" fn(*mut c_void, usize, usize) -> i32,
    slot7: unsafe extern "system" fn(*mut c_void, usize, usize) -> i32,
    set_staging_buffer_size: unsafe extern "system" fn(*mut c_void, u32) -> i32,
    slot9: unsafe extern "system" fn(*mut c_void, usize, usize) -> i32,
    create_queue_x: unsafe extern "system" fn(*mut c_void, usize, usize, usize) -> i32,
    extra: [unsafe extern "system" fn(*mut c_void, usize, usize) -> i32; 6],
}

macro_rules! fslot {
    ($name:ident, $n:expr) => {
        unsafe extern "system" fn $name(_this: *mut c_void, a: usize, b: usize) -> i32 {
            eprintln!("[dstorage] factory slot {} (0x{:x}, 0x{:x}) -> S_OK", $n, a, b);
            0
        }
    };
}
fslot!(fs6, 6); fslot!(fs7, 7); fslot!(fs9, 9);
/// Console slot 10: CreateQueue(desc, riid, out) (title call site; PC slot 3 equivalent).
unsafe extern "system" fn fs10(this: *mut c_void, desc: usize, riid: usize, out: usize) -> i32 {
    eprintln!("[dstorage] factory slot 10 CreateQueue");
    factory_create_queue(this, desc as *const c_void, riid as *const c_void, out as *mut *mut c_void)
} fslot!(fs11, 11); fslot!(fs12, 12);
fslot!(fs13, 13); fslot!(fs14, 14); fslot!(fs15, 15); fslot!(fs16, 16);

#[repr(C)]
struct FileVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const c_void, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    close: unsafe extern "system" fn(*mut c_void),
    get_file_information: unsafe extern "system" fn(*mut c_void, *mut u8) -> i32,
}

#[repr(C)]
struct QueueVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const c_void, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    enqueue_request: unsafe extern "system" fn(*mut c_void, *const DStorageRequest),
    enqueue_status: unsafe extern "system" fn(*mut c_void, *mut c_void, u32),
    enqueue_set_event: unsafe extern "system" fn(*mut c_void, usize),
    submit: unsafe extern "system" fn(*mut c_void),
    cancel_requests_with_tag: unsafe extern "system" fn(*mut c_void, u64, u64),
    close: unsafe extern "system" fn(*mut c_void),
    retrieve_error_record: unsafe extern "system" fn(*mut c_void, *mut u8),
    query: unsafe extern "system" fn(*mut c_void, *mut u8),
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DStorageRequest {
    pub options: u64,
    pub source_ptr: *const u8,
    pub source_offset: u64,
    pub source_size: u32,
    pub _pad0: u32,
    pub dest_ptr: *mut u8,
    pub dest_size: u32,
    pub _pad1: u32,
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
}

#[repr(C)]
struct ComQueue {
    vtbl: *const QueueVtbl,
    refs: AtomicU32,
    pending_events: Mutex<Vec<usize>>,
}

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
    _desc: *const c_void,
    _riid: *const c_void,
    out_q: *mut *mut c_void,
) -> i32 {
    if out_q.is_null() {
        return E_POINTER;
    }
    let q = Box::new(ComQueue {
        vtbl: queue_vtbl_logged(),
        refs: AtomicU32::new(1),
        pending_events: Mutex::new(Vec::new()),
    });
    *out_q = Box::into_raw(q) as *mut c_void;
    S_OK
}
fn resolve_vfs_path(raw: &str) -> PathBuf {
    let norm = raw.replace('/', "\\");
    let lower = norm.to_ascii_lowercase();
    let (drive, rest) = if lower.starts_with("g:\\") {
        ("PERCY_VFS_G", &norm[3..])
    } else if lower.starts_with("t:\\") {
        ("PERCY_VFS_T", &norm[3..])
    } else if lower.starts_with("s:\\") {
        ("PERCY_VFS_S", &norm[3..])
    } else {
        return PathBuf::from(norm);
    };
    let base = std::env::var(drive).map(PathBuf::from).unwrap_or_else(|_| {
        if drive == "PERCY_VFS_T" {
            std::env::temp_dir().join("percy_T")
        } else if drive == "PERCY_VFS_S" {
            std::env::temp_dir().join("percy_S")
        } else {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        }
    });
    base.join(rest)
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
    eprintln!("[dstorage] OpenFile {s} -> {}", p.display());
    let Ok(meta) = std::fs::metadata(&p) else {
        *out_file = std::ptr::null_mut();
        return E_FAIL;
    };
    let f = Box::new(ComFile {
        vtbl: &FILE_VTBL,
        refs: AtomicU32::new(1),
        path: p,
        size: meta.len(),
    });
    *out_file = Box::into_raw(f) as *mut c_void;
    S_OK
}
/// CreateStatusArray(capacity, name, riid, out). Requests complete synchronously at Submit, so
/// every entry reads complete with S_OK.
unsafe extern "system" fn factory_create_status(_this: *mut c_void, cap: u32, _name: *const u8, _riid: *const c_void, out: *mut *mut c_void) -> i32 {
    if out.is_null() {
        return E_POINTER;
    }
    let o = Box::new(StatusArray { vtbl: &STATUS_VTBL, refs: AtomicU32::new(1) });
    *out = Box::into_raw(o) as *mut c_void;
    eprintln!("[dstorage] CreateStatusArray cap={cap}");
    S_OK
}

#[repr(C)]
struct StatusArray {
    vtbl: *const StatusVtbl,
    refs: AtomicU32,
}
#[repr(C)]
struct StatusVtbl {
    qi: unsafe extern "system" fn(*mut c_void, *const c_void, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    is_complete: unsafe extern "system" fn(*mut c_void, u32) -> u8,
    get_hresult: unsafe extern "system" fn(*mut c_void, u32) -> i32,
}
unsafe extern "system" fn st_qi(this: *mut c_void, _riid: *const c_void, out: *mut *mut c_void) -> i32 {
    if out.is_null() {
        return E_POINTER;
    }
    st_add_ref(this);
    *out = this;
    S_OK
}
unsafe extern "system" fn st_add_ref(this: *mut c_void) -> u32 {
    (*(this as *const StatusArray)).refs.fetch_add(1, std::sync::atomic::Ordering::AcqRel) + 1
}
unsafe extern "system" fn st_release(this: *mut c_void) -> u32 {
    let n = (*(this as *const StatusArray)).refs.fetch_sub(1, std::sync::atomic::Ordering::AcqRel) - 1;
    if n == 0 {
        drop(Box::from_raw(this as *mut StatusArray));
    }
    n
}
unsafe extern "system" fn st_is_complete(_this: *mut c_void, _i: u32) -> u8 {
    1
}
unsafe extern "system" fn st_get_hresult(_this: *mut c_void, _i: u32) -> i32 {
    S_OK
}
static STATUS_VTBL: StatusVtbl = StatusVtbl { qi: st_qi, add_ref: st_add_ref, release: st_release, is_complete: st_is_complete, get_hresult: st_get_hresult };
unsafe extern "system" fn factory_set_staging(_this: *mut c_void, _sz: u32) -> i32 {
    S_OK
}

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
        drop(Box::from_raw(this as *mut ComFile));
    }
    prev - 1
}
unsafe extern "system" fn file_close(_this: *mut c_void) {}
unsafe extern "system" fn file_get_info(this: *mut c_void, out_info: *mut u8) -> i32 {
    if out_info.is_null() {
        return E_POINTER;
    }
    let f = &*(this as *const ComFile);
    std::ptr::write_bytes(out_info, 0, 24);
    std::ptr::copy_nonoverlapping(f.size.to_le_bytes().as_ptr(), out_info, 8);
    S_OK
}

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
    static RN: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = RN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if n < 12 && !req.is_null() {
        let w = std::slice::from_raw_parts(req as *const u64, 10);
        eprintln!("[dstorage] EnqueueRequest #{n} {:x?}", w);
    }
    if req.is_null() {
        return;
    }
    // Console request (u64 words): [0] options (low byte: 0 none, 1 zlib), [1] dest buffer,
    // [3] dest size, [4] source (IDStorageFile or memory), [5] source offset, [6] source size.
    let w = std::slice::from_raw_parts(req as *const u64, 10);
    let (opt, dst, dsz, src, off, ssz) = (w[0], w[1] as *mut u8, w[3] as u32 as usize, w[4] as *const u8, w[5], w[6] as u32 as usize);
    if dst.is_null() || dsz == 0 || src.is_null() || ssz == 0 {
        return;
    }
    let is_file = *(src as *const *const FileVtbl) == &FILE_VTBL as *const FileVtbl;
    let data: Vec<u8> = if is_file {
        let cf = &*(src as *const ComFile);
        let mut buf = vec![0u8; ssz];
        let ok = File::open(&cf.path).map(|mut f| f.seek(SeekFrom::Start(off)).is_ok() && f.read_exact(&mut buf).is_ok()).unwrap_or(false);
        match ok {
            true => buf,
            false => {
                eprintln!("[dstorage] read failed {} off={off:#x} size={ssz:#x}", cf.path.display());
                return;
            }
        }
    } else {
        std::slice::from_raw_parts(src.add(off as usize), ssz).to_vec()
    };
    #[cfg(windows)]
    if !ensure_writable(dst, dsz) {
        return;
    }
    let out = std::slice::from_raw_parts_mut(dst, dsz);
    match opt & 0xff {
        0 => {
            let n = data.len().min(dsz);
            out[..n].copy_from_slice(&data[..n]);
        }
        1 => {
            let mut z = flate2::Decompress::new(true);
            if z.decompress(&data, out, flate2::FlushDecompress::Finish).is_err() {
                let mut r = flate2::Decompress::new(false);
                if r.decompress(&data, out, flate2::FlushDecompress::Finish).is_err() {
                    eprintln!("[dstorage] inflate failed off={off:#x} size={ssz:#x}");
                }
            }
        }
        c => {
            static UC: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            if UC.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 4 {
                eprintln!("[dstorage] unsupported compression {c} (opt={opt:#x}) size={ssz:#x}->{dsz:#x}");
            }
        }
    }
}
unsafe extern "system" fn queue_enqueue_status(_this: *mut c_void, _status: *mut c_void, _idx: u32) {}
unsafe extern "system" fn queue_enqueue_event(this: *mut c_void, ev: usize) {
    if ev != 0 {
        (*(this as *const ComQueue)).pending_events.lock().unwrap().push(ev);
    }
}
unsafe extern "system" fn queue_submit(this: *mut c_void) {
    let mut evs = (*(this as *const ComQueue)).pending_events.lock().unwrap();
    #[cfg(windows)]
    for ev in evs.drain(..) {
        SetEvent(ev);
    }
    #[cfg(not(windows))]
    evs.clear();
}
unsafe extern "system" fn queue_cancel(_this: *mut c_void, _mask: u64, _val: u64) {}
unsafe extern "system" fn queue_close(_this: *mut c_void) {}
unsafe extern "system" fn queue_retrieve_err(_this: *mut c_void, out: *mut u8) {
    if !out.is_null() {
        std::ptr::write_bytes(out, 0, 64);
    }
}
unsafe extern "system" fn queue_query(_this: *mut c_void, out: *mut u8) {
    if !out.is_null() {
        std::ptr::write_bytes(out, 0, 32);
    }
}

static FACTORY_VTBL: FactoryVtbl = FactoryVtbl {
    query_interface: factory_qi,
    add_ref: factory_add_ref,
    release: factory_release,
    create_queue: factory_create_queue,
    open_file: factory_open_file,
    create_status_array: factory_create_status,
    slot6: fs6,
    slot7: fs7,
    set_staging_buffer_size: factory_set_staging,
    slot9: fs9,
    create_queue_x: fs10,
    extra: [fs11, fs12, fs13, fs14, fs15, fs16],
};

static FILE_VTBL: FileVtbl = FileVtbl {
    query_interface: file_qi,
    add_ref: file_add_ref,
    release: file_release,
    close: file_close,
    get_file_information: file_get_info,
};

// Logged queue vtable: slots 0..10 pass through to QUEUE_VTBL (first call logged), 11..31 log.
macro_rules! qwrap {
    ($name:ident, $n:literal) => {
        unsafe extern "system" fn $name(t: usize, a: usize, b: usize, c: usize) -> usize {
            static ONCE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            if !ONCE.swap(true, std::sync::atomic::Ordering::Relaxed) {
                eprintln!("[dstorage] queue slot {} ({a:#x}, {b:#x}, {c:#x})", $n);
            }
            let real = *((&QUEUE_VTBL as *const QueueVtbl as *const usize).add($n));
            if real == 0 {
                return 0;
            }
            std::mem::transmute::<usize, unsafe extern "system" fn(usize, usize, usize, usize) -> usize>(real)(t, a, b, c)
        }
    };
}
qwrap!(q3, 3); qwrap!(q4, 4); qwrap!(q5, 5); qwrap!(q6, 6); qwrap!(q7, 7); qwrap!(q8, 8); qwrap!(q9, 9); qwrap!(q10, 10);
macro_rules! qstub {
    ($name:ident, $n:literal) => {
        unsafe extern "system" fn $name(_t: usize, a: usize, b: usize, c: usize) -> usize {
            static ONCE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            if !ONCE.swap(true, std::sync::atomic::Ordering::Relaxed) {
                eprintln!("[dstorage] queue slot {} UNIMPL ({a:#x}, {b:#x}, {c:#x})", $n);
            }
            0
        }
    };
}
qstub!(q11, 11); qstub!(q12, 12); qstub!(q13, 13); qstub!(q14, 14); qstub!(q15, 15); qstub!(q16, 16); qstub!(q17, 17);
qstub!(q18, 18); qstub!(q19, 19); qstub!(q20, 20); qstub!(q21, 21); qstub!(q22, 22); qstub!(q23, 23); qstub!(q24, 24);
qstub!(q25, 25); qstub!(q26, 26); qstub!(q27, 27); qstub!(q28, 28); qstub!(q29, 29); qstub!(q30, 30); qstub!(q31, 31);

fn queue_vtbl_logged() -> *const QueueVtbl {
    static V: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *V.get_or_init(|| {
        let base = &QUEUE_VTBL as *const QueueVtbl as *const usize;
        let w: [unsafe extern "system" fn(usize, usize, usize, usize) -> usize; 29] = [
            q3, q4, q5, q6, q7, q8, q9, q10, q11, q12, q13, q14, q15, q16, q17, q18, q19, q20, q21, q22, q23, q24, q25, q26, q27, q28, q29, q30, q31,
        ];
        let mut v = Box::new([0usize; 32]);
        for i in 0..3 {
            v[i] = unsafe { *base.add(i) };
        }
        for (i, f) in w.iter().enumerate() {
            v[3 + i] = *f as usize;
        }
        Box::leak(v).as_ptr() as usize
    }) as *const QueueVtbl
}

static QUEUE_VTBL: QueueVtbl = QueueVtbl {
    query_interface: queue_qi,
    add_ref: queue_add_ref,
    release: queue_release,
    enqueue_request: queue_enqueue_req,
    enqueue_status: queue_enqueue_status,
    enqueue_set_event: queue_enqueue_event,
    submit: queue_submit,
    cancel_requests_with_tag: queue_cancel,
    close: queue_close,
    retrieve_error_record: queue_retrieve_err,
    query: queue_query,
};

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
    fn dstorage_factory_and_queue_memory_copy_work() {
        unsafe {
            assert_eq!(DStorageSetConfiguration(std::ptr::null()), S_OK);
            assert_eq!(DStorageGetFactory(std::ptr::null(), std::ptr::null_mut()), E_POINTER);

            let mut factory: *mut c_void = std::ptr::null_mut();
            assert_eq!(DStorageGetFactory(std::ptr::null(), &mut factory), S_OK);
            assert!(!factory.is_null());

            let f_vtbl = &*(*(factory as *const *const FactoryVtbl));
            let mut queue: *mut c_void = std::ptr::null_mut();
            assert_eq!((f_vtbl.create_queue)(factory, std::ptr::null(), std::ptr::null(), &mut queue), S_OK);
            assert!(!queue.is_null());

            let q_vtbl = &*(*(queue as *const *const QueueVtbl));
            let src = [0x11u8, 0x22, 0x33, 0x44];
            let mut dst = [0u8; 4];
            let req = DStorageRequest {
                options: 0,
                source_ptr: src.as_ptr(),
                source_offset: 0,
                source_size: 4,
                _pad0: 0,
                dest_ptr: dst.as_mut_ptr(),
                dest_size: 4,
                _pad1: 0,
            };
            (q_vtbl.enqueue_request)(queue, &req);
            (q_vtbl.submit)(queue);
            assert_eq!(dst, src);

            assert_eq!((q_vtbl.release)(queue), 0);
            assert_eq!((f_vtbl.release)(factory), 0);

            std::env::set_var("PERCY_VFS_G", r"D:\PercyScarlett_Mount");
            assert_eq!(resolve_vfs_path(r"G:\data\levels.rpf"), PathBuf::from(r"D:\PercyScarlett_Mount\data\levels.rpf"));
        }
    }
}
