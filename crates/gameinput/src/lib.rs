#![allow(non_snake_case)]
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};
mod vpad;

const S_OK: i32 = 0;
const E_POINTER: i32 = 0x8000_4003u32 as i32;

#[cfg(windows)]
#[link(name = "user32")]
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

/// Minimal built-in fallback (used only when the Windows GameInput runtime is unavailable).
pub unsafe extern "system" fn stub_game_input_create(out_input: *mut *mut c_void) -> i32 {
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

/// Console GameInput (GDK v0 `IGameInput`) is ABI-compatible with the Windows inbox GameInput
/// runtime (`%SystemRoot%\System32\GameInput.dll`, v0 interface). Forward to it (loaded by full
/// path, distinct from this same-named shim); fall back to the built-in stub if absent.
#[no_mangle]
pub unsafe extern "system" fn GameInputCreate(out_input: *mut *mut c_void) -> i32 {
    if out_input.is_null() {
        return E_POINTER;
    }
    #[cfg(windows)]
    if std::env::var("PERCY_GAMEINPUT_STUB").is_err() {
        extern "system" {
            fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
            fn GetProcAddress(m: *mut c_void, name: *const u8) -> *mut c_void;
            fn GetSystemDirectoryW(buf: *mut u16, n: u32) -> u32;
        }
        let mut buf = [0u16; 260];
        let n = GetSystemDirectoryW(buf.as_mut_ptr(), 260) as usize;
        let mut path: Vec<u16> = buf[..n].to_vec();
        path.extend("\\GameInput.dll".encode_utf16());
        path.push(0);
        let h = LoadLibraryExW(path.as_ptr(), std::ptr::null_mut(), 0);
        if !h.is_null() {
            let p = GetProcAddress(h, b"GameInputCreate\0".as_ptr());
            if !p.is_null() {
                let f: unsafe extern "system" fn(*mut *mut c_void) -> i32 = std::mem::transmute(p);
                let hr = f(out_input);
                if hr >= 0 {
                    trace_wrap(*out_input);
                    return hr;
                }
            }
        }
    }
    stub_game_input_create(out_input)
}

// ---- call trace of the system IGameInput (first call per slot) ----
static ORIG_VT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static GSEEN: [std::sync::atomic::AtomicU32; 64] = [const { std::sync::atomic::AtomicU32::new(0) }; 64];
unsafe fn gtrace(slot: usize, t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize {
    let orig = *(ORIG_VT.load(Ordering::Relaxed) as *const usize).add(slot);
    let fun: unsafe extern "system" fn(usize, usize, usize, usize, usize, usize, usize, usize) -> usize = std::mem::transmute(orig);
    let r = fun(t, a, b, c, d, e, f, g);
    if GSEEN[slot].fetch_add(1, Ordering::Relaxed) < 2 {
        eprintln!("[gi] slot {slot} ({a:#x},{b:#x},{c:#x},{d:#x}) -> {r:#x}");
    }
    r
}
unsafe extern "system" fn g3(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(3, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g4(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(4, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g5(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(5, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g6(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(6, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g7(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(7, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g8(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(8, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g9(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(9, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g10(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(10, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g11(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(11, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g12(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(12, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g13(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(13, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g14(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(14, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g15(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(15, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g16(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(16, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g17(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(17, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g18(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(18, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g19(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(19, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g20(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(20, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g21(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(21, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g22(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(22, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g23(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(23, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g24(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(24, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g25(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(25, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g26(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(26, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g27(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(27, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g28(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(28, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g29(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(29, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g30(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(30, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g31(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(31, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g32(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(32, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g33(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(33, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g34(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(34, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g35(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(35, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g36(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(36, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g37(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(37, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g38(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(38, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g39(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(39, t, a, b, c, d, e, f, g) }
unsafe extern "system" fn g40(t: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize) -> usize { gtrace(40, t, a, b, c, d, e, f, g) }

const KIND_GAMEPAD: u32 = 0x40000;
/// IGameInput slot 4 GetCurrentReading(kind, device, reading**): the virtual pad answers for itself.
unsafe extern "system" fn gi_current_reading(t: usize, kind: u32, dev: *mut c_void, out: *mut *mut c_void) -> i32 {
    if !out.is_null() && kind & KIND_GAMEPAD != 0 && dev == vpad::device() {
        *out = vpad::reading();
        return S_OK;
    }
    let r = gtrace(4, t, kind as usize, dev as usize, out as usize, 0, 0, 0, 0) as u32 as i32;
    if r < 0 && !out.is_null() && kind & KIND_GAMEPAD != 0 && dev.is_null() {
        *out = vpad::reading();
        return S_OK;
    }
    r
}
/// IGameInput slot 9 RegisterDeviceCallback(device, kind, status, enumeration, ctx, cb, token*):
/// also report the virtual pad as connected.
unsafe extern "system" fn gi_register_device_cb(t: usize, dev: usize, kind: u32, status: u32, en: u32, ctx: usize, cb: usize, token: *mut u64) -> i32 {
    let r = gtrace(9, t, dev, kind as usize, status as usize, en as usize, ctx, cb, token as usize) as u32 as i32;
    if r >= 0 && cb != 0 && kind & KIND_GAMEPAD != 0 && (dev == 0 || dev == vpad::device() as usize) {
        let tok = if token.is_null() { 0 } else { *token };
        std::thread::spawn(move || unsafe {
            std::thread::sleep(std::time::Duration::from_millis(300));
            let f: unsafe extern "system" fn(u64, usize, *mut c_void, u64, u32, u32) = std::mem::transmute(cb);
            eprintln!("[gi] virtual gamepad connected");
            f(tok, ctx, vpad::device(), vpad::timestamp(), 3, 0);
        });
    }
    r
}

unsafe fn trace_wrap(gi: *mut c_void) {
    if gi.is_null() { return; }
    let vp = gi as *mut usize;
    let orig = *vp;
    ORIG_VT.store(orig, Ordering::Relaxed);
    let mut v: Vec<usize> = std::slice::from_raw_parts(orig as *const usize, 41).to_vec();
    let thunks = [g3 as usize, g4 as usize, g5 as usize, g6 as usize, g7 as usize, g8 as usize, g9 as usize, g10 as usize, g11 as usize, g12 as usize, g13 as usize, g14 as usize, g15 as usize, g16 as usize, g17 as usize, g18 as usize, g19 as usize, g20 as usize, g21 as usize, g22 as usize, g23 as usize, g24 as usize, g25 as usize, g26 as usize, g27 as usize, g28 as usize, g29 as usize, g30 as usize, g31 as usize, g32 as usize, g33 as usize, g34 as usize, g35 as usize, g36 as usize, g37 as usize, g38 as usize, g39 as usize, g40 as usize];
    v[3..41].copy_from_slice(&thunks);
    v[4] = gi_current_reading as usize;
    v[9] = gi_register_device_cb as usize;
    *vp = Box::leak(v.into_boxed_slice()).as_ptr() as usize;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gameinput_create_and_reading_work() {
        unsafe {
            assert_eq!(GameInputCreate(std::ptr::null_mut()), E_POINTER);
            let mut gi: *mut c_void = std::ptr::null_mut();
            assert_eq!(stub_game_input_create(&mut gi), S_OK);
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
