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
}

#[repr(C)]
struct FactoryVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const c_void, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    create_queue: unsafe extern "system" fn(*mut c_void, *const c_void, *const c_void, *mut *mut c_void) -> i32,
    open_file: unsafe extern "system" fn(*mut c_void, *const u16, *const c_void, *mut *mut c_void) -> i32,
    create_status_array: unsafe extern "system" fn(*mut c_void, u32, *const c_void, *mut *mut c_void) -> i32,
    set_staging_buffer_size: unsafe extern "system" fn(*mut c_void, u32) -> i32,
}

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
        vtbl: &QUEUE_VTBL,
        refs: AtomicU32::new(1),
        pending_events: Mutex::new(Vec::new()),
    });
    *out_q = Box::into_raw(q) as *mut c_void;
    S_OK
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
    let p = PathBuf::from(s);
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
unsafe extern "system" fn factory_create_status(_this: *mut c_void, _cap: u32, _riid: *const c_void, out: *mut *mut c_void) -> i32 {
    if !out.is_null() {
        *out = std::ptr::null_mut();
    }
    E_NOINTERFACE
}
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
    if req.is_null() {
        return;
    }
    let r = &*req;
    if r.dest_ptr.is_null() || r.source_size == 0 || r.dest_size < r.source_size {
        return;
    }
    // Bit 0 of options = 0 (memory source) vs 1 (file source)
    if (r.options & 1) == 0 && !r.source_ptr.is_null() {
        std::ptr::copy_nonoverlapping(r.source_ptr, r.dest_ptr, r.source_size as usize);
    } else if (r.options & 1) == 1 && !r.source_ptr.is_null() {
        let cf = &*(r.source_ptr as *const ComFile);
        if let Ok(mut file) = File::open(&cf.path) {
            if file.seek(SeekFrom::Start(r.source_offset)).is_ok() {
                let dst = std::slice::from_raw_parts_mut(r.dest_ptr, r.source_size as usize);
                let _ = file.read_exact(dst);
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
    set_staging_buffer_size: factory_set_staging,
};

static FILE_VTBL: FileVtbl = FileVtbl {
    query_interface: file_qi,
    add_ref: file_add_ref,
    release: file_release,
    close: file_close,
    get_file_information: file_get_info,
};

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
        }
    }
}
