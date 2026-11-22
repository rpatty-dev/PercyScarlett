#![allow(non_snake_case)]
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};

const S_OK: i32 = 0;
const E_POINTER: i32 = 0x8000_4003u32 as i32;

#[cfg(windows)]
extern "system" {
    fn GetAsyncKeyState(vkey: i32) -> i16;
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct GameInputGamepadState {
    pub buttons: u32,
    pub left_trigger: f32,
    pub right_trigger: f32,
    pub left_thumbstick_x: f32,
    pub left_thumbstick_y: f32,
    pub right_thumbstick_x: f32,
    pub right_thumbstick_y: f32,
}

#[repr(C)]
struct GameInputVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const c_void, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    get_current_reading: unsafe extern "system" fn(*mut c_void, u32, *mut c_void, *mut *mut c_void) -> i32,
}

#[repr(C)]
struct ReadingVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const c_void, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    get_gamepad_state: unsafe extern "system" fn(*mut c_void, *mut GameInputGamepadState) -> u8,
}

#[repr(C)]
struct ComGameInput {
    vtbl: *const GameInputVtbl,
    refs: AtomicU32,
}

#[repr(C)]
struct ComReading {
    vtbl: *const ReadingVtbl,
    refs: AtomicU32,
    gamepad: GameInputGamepadState,
}

fn key_down(_vk: i32) -> bool {
    #[cfg(windows)]
    unsafe {
        (GetAsyncKeyState(_vk) as u16 & 0x8000) != 0
    }
    #[cfg(not(windows))]
    false
}

unsafe extern "system" fn gi_qi(this: *mut c_void, _riid: *const c_void, out: *mut *mut c_void) -> i32 {
    if out.is_null() {
        return E_POINTER;
    }
    gi_add_ref(this);
    *out = this;
    S_OK
}
unsafe extern "system" fn gi_add_ref(this: *mut c_void) -> u32 {
    (*(this as *const ComGameInput)).refs.fetch_add(1, Ordering::Relaxed) + 1
}
unsafe extern "system" fn gi_release(this: *mut c_void) -> u32 {
    let prev = (*(this as *const ComGameInput)).refs.fetch_sub(1, Ordering::Release);
    if prev == 1 {
        std::sync::atomic::fence(Ordering::Acquire);
        drop(Box::from_raw(this as *mut ComGameInput));
    }
    prev - 1
}
unsafe extern "system" fn gi_get_reading(
    _this: *mut c_void,
    _kind: u32,
    _device: *mut c_void,
    out_reading: *mut *mut c_void,
) -> i32 {
    if out_reading.is_null() {
        return E_POINTER;
    }
    let mut gp = GameInputGamepadState::default();
    if key_down(0x57) { gp.left_thumbstick_y += 1.0; } // W
    if key_down(0x53) { gp.left_thumbstick_y -= 1.0; } // S
    if key_down(0x41) { gp.left_thumbstick_x -= 1.0; } // A
    if key_down(0x44) { gp.left_thumbstick_x += 1.0; } // D
    if key_down(0x20) { gp.buttons |= 0x0010; }        // Space -> A
    if key_down(0x0D) { gp.buttons |= 0x0004; }        // Enter -> Menu

    let r = Box::new(ComReading {
        vtbl: &READING_VTBL,
        refs: AtomicU32::new(1),
        gamepad: gp,
    });
    *out_reading = Box::into_raw(r) as *mut c_void;
    S_OK
}

unsafe extern "system" fn rd_qi(this: *mut c_void, _riid: *const c_void, out: *mut *mut c_void) -> i32 {
    if out.is_null() {
        return E_POINTER;
    }
    rd_add_ref(this);
    *out = this;
    S_OK
}
unsafe extern "system" fn rd_add_ref(this: *mut c_void) -> u32 {
    (*(this as *const ComReading)).refs.fetch_add(1, Ordering::Relaxed) + 1
}
unsafe extern "system" fn rd_release(this: *mut c_void) -> u32 {
    let prev = (*(this as *const ComReading)).refs.fetch_sub(1, Ordering::Release);
    if prev == 1 {
        std::sync::atomic::fence(Ordering::Acquire);
        drop(Box::from_raw(this as *mut ComReading));
    }
    prev - 1
}
unsafe extern "system" fn rd_get_gamepad(this: *mut c_void, out: *mut GameInputGamepadState) -> u8 {
    if out.is_null() {
        return 0;
    }
    *out = (*(this as *const ComReading)).gamepad;
    1
}

static GI_VTBL: GameInputVtbl = GameInputVtbl {
    query_interface: gi_qi,
    add_ref: gi_add_ref,
    release: gi_release,
    get_current_reading: gi_get_reading,
};

static READING_VTBL: ReadingVtbl = ReadingVtbl {
    query_interface: rd_qi,
    add_ref: rd_add_ref,
    release: rd_release,
    get_gamepad_state: rd_get_gamepad,
};

#[no_mangle]
pub unsafe extern "system" fn GameInputCreate(out_input: *mut *mut c_void) -> i32 {
    if out_input.is_null() {
        return E_POINTER;
    }
    let gi = Box::new(ComGameInput {
        vtbl: &GI_VTBL,
        refs: AtomicU32::new(1),
    });
    *out_input = Box::into_raw(gi) as *mut c_void;
    S_OK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gameinput_create_and_reading_work() {
        unsafe {
            assert_eq!(GameInputCreate(std::ptr::null_mut()), E_POINTER);
            let mut gi: *mut c_void = std::ptr::null_mut();
            assert_eq!(GameInputCreate(&mut gi), S_OK);
            assert!(!gi.is_null());

            let vtbl = &*(*(gi as *const *const GameInputVtbl));
            let mut rd: *mut c_void = std::ptr::null_mut();
            assert_eq!((vtbl.get_current_reading)(gi, 0, std::ptr::null_mut(), &mut rd), S_OK);
            assert!(!rd.is_null());

            let r_vtbl = &*(*(rd as *const *const ReadingVtbl));
            let mut gp = GameInputGamepadState::default();
            assert_eq!((r_vtbl.get_gamepad_state)(rd, &mut gp), 1);

            assert_eq!((r_vtbl.release)(rd), 0);
            assert_eq!((vtbl.release)(gi), 0);
        }
    }
}
