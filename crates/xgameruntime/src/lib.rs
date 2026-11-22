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
pub unsafe extern "system" fn XSystemGetDeviceType() -> u32 {
    4 // XSystemDeviceType::XboxScarlettLockhart/Anaconda (Series X|S)
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

#[no_mangle]
pub unsafe extern "system" fn XGameSaveInitializeProvider(
    _user: *mut c_void,
    _scid: *const u8,
    _sync_on_demand: i32,
    out_provider: *mut *mut c_void,
) -> i32 {
    if out_provider.is_null() {
        return E_POINTER;
    }
    *out_provider = Box::into_raw(Box::new(0x5053_4753u32)) as *mut c_void;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_queue_and_xuser_work() {
        unsafe {
            assert_eq!(XSystemGetDeviceType(), 4);
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

            XUserCloseHandle(user);
            XTaskQueueCloseHandle(q);
        }
    }
}
