use std::ffi::c_void;

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

#[no_mangle]
pub unsafe extern "system" fn XUserGetId(_user: *mut c_void, out_id: *mut u64) -> i32 {
    if out_id.is_null() {
        return E_POINTER;
    }
    *out_id = 0x0009_0000_0000_0001;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_queue_and_xuser_work() {
        unsafe {
            let mut q: *mut c_void = std::ptr::null_mut();
            assert_eq!(XTaskQueueCreate(0, 0, &mut q), S_OK);
            assert!(!q.is_null());
            let mut uid = 0u64;
            assert_eq!(XUserGetId(std::ptr::null_mut(), &mut uid), S_OK);
            assert_ne!(uid, 0);
            XTaskQueueCloseHandle(q);
        }
    }
}
