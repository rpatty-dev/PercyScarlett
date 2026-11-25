//! Virtual gamepad for keyboard/mouse hosts. Console titles take gamepad input only; this device
//! joins the system GameInput device list and reads its state from the keyboard and mouse while
//! the title's window has focus.
//!
//! Keys: WASD left stick, arrows / mouse motion right stick, Space/Enter A, Q/Backspace B,
//! R X, E Y, Esc Menu, Tab View, Z/C bumpers, right/left mouse button triggers, Shift left
//! stick click, F right stick click, 1-4 d-pad.
use super::GameInputGamepadState;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};

pub const DEVICE_ID: [u8; 32] = *b"PercyScarlett-VirtualGamepad-001";
const KIND_GAMEPAD: u32 = 0x40000;

type P = *mut c_void;

#[cfg(windows)]
extern "system" {
    fn GetAsyncKeyState(vkey: i32) -> i16;
    fn GetForegroundWindow() -> P;
    fn GetWindowThreadProcessId(h: P, pid: *mut u32) -> u32;
    fn GetCurrentProcessId() -> u32;
    fn GetCursorPos(p: *mut [i32; 2]) -> i32;
    fn QueryPerformanceCounter(c: *mut i64) -> i32;
    fn LoadLibraryA(name: *const u8) -> usize;
    fn GetProcAddress(h: usize, name: *const u8) -> usize;
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct XInputGamepad {
    buttons: u16,
    lt: u8,
    rt: u8,
    lx: i16,
    ly: i16,
    rx: i16,
    ry: i16,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct XInputState {
    packet: u32,
    pad: XInputGamepad,
}

#[cfg(windows)]
fn poll_xinput(s: &mut GameInputGamepadState) {
    type FnGetState = unsafe extern "system" fn(u32, *mut XInputState) -> u32;
    static FN: std::sync::OnceLock<Option<FnGetState>> = std::sync::OnceLock::new();
    let Some(get_state) = *FN.get_or_init(|| unsafe {
        for dll in [b"xinput1_4.dll\0".as_slice(), b"xinput1_3.dll\0", b"xinput9_1_0.dll\0"] {
            let h = LoadLibraryA(dll.as_ptr());
            if h != 0 {
                let p = GetProcAddress(h, b"XInputGetState\0".as_ptr());
                if p != 0 {
                    return Some(std::mem::transmute::<usize, FnGetState>(p));
                }
            }
        }
        None
    }) else {
        return;
    };
    let mut xs = XInputState::default();
    for idx in 0..4u32 {
        if unsafe { get_state(idx, &mut xs) } != 0 {
            continue;
        }
        let b = xs.pad.buttons;
        if b & 0x0010 != 0 { s.buttons |= 0x1; }    // Start -> Menu
        if b & 0x0020 != 0 { s.buttons |= 0x2; }    // Back -> View
        if b & 0x1000 != 0 { s.buttons |= 0x4; }    // A
        if b & 0x2000 != 0 { s.buttons |= 0x8; }    // B
        if b & 0x4000 != 0 { s.buttons |= 0x10; }   // X
        if b & 0x8000 != 0 { s.buttons |= 0x20; }   // Y
        if b & 0x0001 != 0 { s.buttons |= 0x40; }   // DPad Up
        if b & 0x0002 != 0 { s.buttons |= 0x80; }   // DPad Down
        if b & 0x0004 != 0 { s.buttons |= 0x100; }  // DPad Left
        if b & 0x0008 != 0 { s.buttons |= 0x200; }  // DPad Right
        if b & 0x0100 != 0 { s.buttons |= 0x400; }  // LB
        if b & 0x0200 != 0 { s.buttons |= 0x800; }  // RB
        if b & 0x0040 != 0 { s.buttons |= 0x1000; } // LS
        if b & 0x0080 != 0 { s.buttons |= 0x2000; } // RS
        let lt = xs.pad.lt as f32 / 255.0;
        let rt = xs.pad.rt as f32 / 255.0;
        if lt > s.left_trigger { s.left_trigger = lt; }
        if rt > s.right_trigger { s.right_trigger = rt; }
        let norm = |v: i16| {
            let f = (v as f32 / 32767.0).clamp(-1.0, 1.0);
            if f.abs() < 0.12 { 0.0 } else { f }
        };
        let (lx, ly, rx, ry) = (norm(xs.pad.lx), norm(xs.pad.ly), norm(xs.pad.rx), norm(xs.pad.ry));
        if lx != 0.0 { s.left_thumbstick_x = lx; }
        if ly != 0.0 { s.left_thumbstick_y = ly; }
        if rx != 0.0 { s.right_thumbstick_x = rx; }
        if ry != 0.0 { s.right_thumbstick_y = ry; }
        break;
    }
}

fn down(vk: i32) -> bool {
    unsafe { GetAsyncKeyState(vk) as u16 & 0x8000 != 0 }
}
fn pressed(vk: i32) -> bool {
    unsafe { GetAsyncKeyState(vk) as u16 & 0x8001 != 0 }
}
fn focused() -> bool {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(GetForegroundWindow(), &mut pid);
        pid == GetCurrentProcessId()
    }
}
fn now_us() -> u64 {
    let mut c = 0i64;
    unsafe { QueryPerformanceCounter(&mut c) };
    c as u64 / 10
}

static LAST_CURSOR: std::sync::Mutex<Option<([i32; 2], f32, f32)>> = std::sync::Mutex::new(None);
static SEQ: AtomicU64 = AtomicU64::new(1);

/// Scripted input (automation / recording): `percy_pad.txt` next to the host holds the pad state,
/// e.g. `A`, `LY=1 RT`, empty for idle. Buttons: MENU VIEW A B X Y UP DOWN LEFT RIGHT LB RB LS RS
/// LT RT; axes LX LY RX RY. Re-read every 30 ms; works without window focus.
fn scripted() -> Option<GameInputGamepadState> {
    static CACHE: std::sync::Mutex<(u64, Option<GameInputGamepadState>)> = std::sync::Mutex::new((0, None));
    static PATH: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    let path = PATH.get_or_init(|| std::env::current_exe().ok().map(|p| p.with_file_name("percy_pad.txt"))).as_ref()?;
    let mut c = CACHE.lock().unwrap();
    let t = now_us();
    if t.saturating_sub(c.0) < 30_000 {
        return c.1;
    }
    c.0 = t;
    c.1 = std::fs::read_to_string(path).ok().map(|txt| {
        let mut s = GameInputGamepadState::default();
        for tok in txt.split_whitespace() {
            let (k, v) = tok.split_once('=').unwrap_or((tok, "1"));
            let f: f32 = v.parse().unwrap_or(0.0);
            match k.to_ascii_uppercase().as_str() {
                "MENU" => s.buttons |= 0x1, "VIEW" => s.buttons |= 0x2, "A" => s.buttons |= 0x4, "B" => s.buttons |= 0x8,
                "X" => s.buttons |= 0x10, "Y" => s.buttons |= 0x20, "UP" => s.buttons |= 0x40, "DOWN" => s.buttons |= 0x80,
                "LEFT" => s.buttons |= 0x100, "RIGHT" => s.buttons |= 0x200, "LB" => s.buttons |= 0x400, "RB" => s.buttons |= 0x800,
                "LS" => s.buttons |= 0x1000, "RS" => s.buttons |= 0x2000,
                "LT" => s.left_trigger = f, "RT" => s.right_trigger = f,
                "LX" => s.left_thumbstick_x = f, "LY" => s.left_thumbstick_y = f,
                "RX" => s.right_thumbstick_x = f, "RY" => s.right_thumbstick_y = f,
                _ => {}
            }
        }
        s
    });
    c.1
}

pub fn state() -> GameInputGamepadState {
    if let Some(s) = scripted() {
        if s.buttons != 0 || s.left_trigger != 0.0 || s.right_trigger != 0.0 || s.left_thumbstick_x != 0.0
            || s.left_thumbstick_y != 0.0 || s.right_thumbstick_x != 0.0 || s.right_thumbstick_y != 0.0 {
            static LAST_B: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            if LAST_B.swap(s.buttons, Ordering::Relaxed) != s.buttons {
                eprintln!("[gi] vpad scripted buttons={:#x}", s.buttons);
            }
            return s;
        }
    }
    let mut s = GameInputGamepadState::default();
    #[cfg(windows)]
    poll_xinput(&mut s);
    if !focused() {
        return s;
    }
    let axis = |neg: bool, pos: bool| (pos as i32 - neg as i32) as f32;
    let lx_kb = axis(down(b'A' as i32), down(b'D' as i32));
    let ly_kb = axis(down(b'S' as i32), down(b'W' as i32));
    if lx_kb != 0.0 { s.left_thumbstick_x = lx_kb; }
    if ly_kb != 0.0 { s.left_thumbstick_y = ly_kb; }
    let mut rx = axis(down(b'J' as i32), down(b'L' as i32)); // IJKL right stick
    let mut ry = axis(down(b'K' as i32), down(b'I' as i32));
    // mouse motion -> right stick (smoothed, by speed)
    let mut p = [0i32; 2];
    unsafe { GetCursorPos(&mut p) };
    let mut lc = LAST_CURSOR.lock().unwrap();
    let (mut sx, mut sy) = (0.0f32, 0.0f32);
    if let Some((q, px, py)) = *lc {
        let (dx, dy) = ((p[0] - q[0]) as f32, (p[1] - q[1]) as f32);
        sx = px * 0.6 + (dx / 12.0).clamp(-1.0, 1.0) * 0.4;
        sy = py * 0.6 + (-dy / 12.0).clamp(-1.0, 1.0) * 0.4;
    }
    *lc = Some((p, sx, sy));
    if rx == 0.0 && sx.abs() > 0.02 {
        rx = sx;
    }
    if ry == 0.0 && sy.abs() > 0.02 {
        ry = sy;
    }
    if rx != 0.0 { s.right_thumbstick_x = rx.clamp(-1.0, 1.0); }
    if ry != 0.0 { s.right_thumbstick_y = ry.clamp(-1.0, 1.0); }
    let lt_kb = down(0x02) as u32 as f32; // RMB
    let rt_kb = down(0x01) as u32 as f32; // LMB
    if lt_kb > s.left_trigger { s.left_trigger = lt_kb; }
    if rt_kb > s.right_trigger { s.right_trigger = rt_kb; }
    let map: [(i32, u32); 25] = [
        (0x1B, 0x1),          // Esc -> Menu
        (0x09, 0x2),          // Tab -> View
        (0x20, 0x4),          // Space -> A
        (0x0D, 0x4),          // Enter -> A
        (b'N' as i32, 0x4),   // N -> A (VBNM = YBAX)
        (b'B' as i32, 0x8),   // B -> B
        (b'Q' as i32, 0x8),   // Q -> B
        (0x08, 0x8),          // Backspace -> B
        (b'M' as i32, 0x10),  // M -> X
        (b'R' as i32, 0x10),  // R -> X
        (b'V' as i32, 0x20),  // V -> Y
        (b'E' as i32, 0x20),  // E -> Y
        (b'1' as i32, 0x40),  // d-pad up
        (b'2' as i32, 0x80),  // down
        (b'3' as i32, 0x100), // left
        (b'4' as i32, 0x200), // right
        (b'Z' as i32, 0x400), // LB
        (b'C' as i32, 0x800), // RB
        (0x26, 0x40),         // Up arrow -> d-pad up
        (0x28, 0x80),         // Down arrow -> d-pad down
        (0x25, 0x100),        // Left arrow -> d-pad left
        (0x27, 0x200),        // Right arrow -> d-pad right
        (0x10, 0x1000),       // Shift -> left stick click
        (b'F' as i32, 0x2000),// right stick click
        (0x11, 0x1000),       // Ctrl -> left stick click
    ];
    for (vk, b) in map {
        if pressed(vk) {
            s.buttons |= b;
        }
    }
    // Latch any button press for 180ms so quick taps never get missed between slow frames.
    static LATCH: std::sync::Mutex<(u64, u32)> = std::sync::Mutex::new((0, 0));
    let now = now_us();
    {
        let mut l = LATCH.lock().unwrap();
        if s.buttons != 0 {
            l.0 = now;
            l.1 |= s.buttons;
        } else if l.1 != 0 && now.saturating_sub(l.0) < 180_000 {
            s.buttons |= l.1;
        } else {
            l.1 = 0;
        }
    }
    s
}

// ---- IGameInputDevice --------------------------------------------------------------------------
#[repr(C)]
pub struct Device {
    vtbl: *const usize,
    info: [u8; 512],
}
unsafe extern "system" fn d_qi(t: P, _r: *const c_void, out: *mut P) -> i32 {
    if !out.is_null() {
        *out = t;
    }
    0
}
unsafe extern "system" fn d_ref(_t: P) -> u32 {
    2
}
unsafe extern "system" fn d_info(t: P) -> *const u8 {
    (*(t as *const Device)).info.as_ptr()
}
unsafe extern "system" fn d_status(_t: P) -> u32 {
    3 // Connected | InputEnabled
}
unsafe extern "system" fn d_battery(_t: P, out: *mut u8) {
    if !out.is_null() {
        std::ptr::write_bytes(out, 0, 24);
    }
}
unsafe extern "system" fn d_zero(_t: P) -> usize {
    0
}
fn dev_vtbl() -> *const usize {
    static V: std::sync::OnceLock<[usize; 24]> = std::sync::OnceLock::new();
    V.get_or_init(|| {
        let mut v = [d_zero as usize; 24];
        v[0] = d_qi as usize;
        v[1] = d_ref as usize;
        v[2] = d_ref as usize;
        v[3] = d_info as usize;
        v[4] = d_status as usize;
        v[5] = d_battery as usize;
        v
    })
    .as_ptr()
}
pub fn device() -> P {
    static DEV: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *DEV.get_or_init(|| {
        let mut info = [0u8; 512];
        info[0..4].copy_from_slice(&512u32.to_le_bytes());
        info[4..6].copy_from_slice(&0x045Eu16.to_le_bytes()); // Microsoft
        info[6..8].copy_from_slice(&0x0B12u16.to_le_bytes()); // Xbox Series controller
        info[12..14].copy_from_slice(&0x01u16.to_le_bytes()); // usage page: generic desktop
        info[14..16].copy_from_slice(&0x05u16.to_le_bytes()); // usage: gamepad
        info[32..64].copy_from_slice(&DEVICE_ID);
        info[64..96].copy_from_slice(&DEVICE_ID);
        info[96..100].copy_from_slice(&1u32.to_le_bytes()); // family: Xbox One
        info[104..108].copy_from_slice(&KIND_GAMEPAD.to_le_bytes()); // supportedInput
        Box::leak(Box::new(Device { vtbl: dev_vtbl(), info })) as *mut Device as usize
    }) as P
}

// ---- IGameInputReading -------------------------------------------------------------------------
#[repr(C)]
struct Reading {
    vtbl: *const usize,
    refs: std::sync::atomic::AtomicU32,
    seq: u64,
    ts: u64,
    pad: GameInputGamepadState,
}
unsafe extern "system" fn r_add(t: P) -> u32 {
    (*(t as *const Reading)).refs.fetch_add(1, Ordering::Relaxed) + 1
}
unsafe extern "system" fn r_rel(t: P) -> u32 {
    let n = (*(t as *const Reading)).refs.fetch_sub(1, Ordering::AcqRel) - 1;
    if n == 0 {
        drop(Box::from_raw(t as *mut Reading));
    }
    n
}
unsafe extern "system" fn r_kind(_t: P) -> u32 {
    KIND_GAMEPAD
}
unsafe extern "system" fn r_seq(t: P, _k: u32) -> u64 {
    (*(t as *const Reading)).seq
}
unsafe extern "system" fn r_ts(t: P) -> u64 {
    (*(t as *const Reading)).ts
}
unsafe extern "system" fn r_dev(_t: P, out: *mut P) {
    if !out.is_null() {
        *out = device();
    }
}
unsafe extern "system" fn r_gamepad(t: P, out: *mut GameInputGamepadState) -> u8 {
    if out.is_null() {
        return 0;
    }
    *out = (*(t as *const Reading)).pad;
    1
}
fn rd_vtbl() -> *const usize {
    static V: std::sync::OnceLock<[usize; 26]> = std::sync::OnceLock::new();
    V.get_or_init(|| {
        let mut v = [d_zero as usize; 26];
        v[0] = d_qi as usize;
        v[1] = r_add as usize;
        v[2] = r_rel as usize;
        v[3] = r_kind as usize;
        v[4] = r_seq as usize;
        v[5] = r_ts as usize;
        v[6] = r_dev as usize;
        v[22] = r_gamepad as usize;
        v
    })
    .as_ptr()
}
pub fn reading() -> P {
    let pad = state();
    let r = Reading {
        vtbl: rd_vtbl(),
        refs: std::sync::atomic::AtomicU32::new(1),
        seq: SEQ.fetch_add(1, Ordering::Relaxed),
        ts: now_us(),
        pad,
    };
    Box::into_raw(Box::new(r)) as P
}
pub fn timestamp() -> u64 {
    now_us()
}
