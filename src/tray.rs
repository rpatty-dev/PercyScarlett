//! Windows system-tray mode (`percyscarlett -headless`) — detaches console and lives in the notification area.

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::ptr::{null, null_mut};

    const WM_DESTROY: u32 = 0x0002;
    const WM_COMMAND: u32 = 0x0111;
    const WM_RBUTTONUP: u32 = 0x0205;
    const WM_LBUTTONDBLCLK: u32 = 0x0203;
    const WM_TRAYICON: u32 = 0x8000 + 1;

    const NIM_ADD: u32 = 0x0000_0000;
    const NIM_DELETE: u32 = 0x0000_0002;
    const NIF_MESSAGE: u32 = 0x0000_0001;
    const NIF_ICON: u32 = 0x0000_0002;
    const NIF_TIP: u32 = 0x0000_0004;

    const TPM_BOTTOMALIGN: u32 = 0x0020;
    const TPM_LEFTALIGN: u32 = 0x0000;
    const MF_STRING: u32 = 0x0000;
    const MF_SEPARATOR: u32 = 0x0800;

    const ID_OPEN_GUI: usize = 1001;
    const ID_SELF_TEST: usize = 1002;
    const ID_EXIT: usize = 1003;

    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }

    #[repr(C)]
    struct Msg {
        hwnd: usize,
        message: u32,
        wparam: usize,
        lparam: isize,
        time: u32,
        pt: Point,
        lprivate: u32,
    }

    #[repr(C)]
    struct WndClassW {
        style: u32,
        lpfn_wnd_proc: unsafe extern "system" fn(usize, u32, usize, isize) -> isize,
        cb_cls_extra: i32,
        cb_wnd_extra: i32,
        h_instance: usize,
        h_icon: usize,
        h_cursor: usize,
        hbr_background: usize,
        lpsz_menu_name: *const u16,
        lpsz_class_name: *const u16,
    }

    #[repr(C)]
    pub(super) struct NotifyIconDataW {
        cb_size: u32,
        hwnd: usize,
        uid: u32,
        u_flags: u32,
        u_callback_message: u32,
        h_icon: usize,
        sz_tip: [u16; 128],
        dw_state: u32,
        dw_state_mask: u32,
        sz_info: [u16; 256],
        u_timeout_or_version: u32,
        sz_info_title: [u16; 64],
        dw_info_flags: u32,
        guid_item: [u8; 16],
        h_balloon_icon: usize,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn FreeConsole() -> i32;
        fn GetModuleHandleW(name: *const u16) -> usize;
    }

    #[link(name = "user32")]
    extern "system" {
        fn RegisterClassW(wc: *const WndClassW) -> u16;
        fn CreateWindowExW(
            ex_style: u32,
            class_name: *const u16,
            window_name: *const u16,
            style: u32,
            x: i32,
            y: i32,
            w: i32,
            h: i32,
            parent: usize,
            menu: usize,
            instance: usize,
            param: *const c_void,
        ) -> usize;
        fn DefWindowProcW(hwnd: usize, msg: u32, wp: usize, lp: isize) -> isize;
        fn PostQuitMessage(code: i32);
        fn DestroyWindow(hwnd: usize) -> i32;
        fn GetMessageW(msg: *mut Msg, hwnd: usize, min: u32, max: u32) -> i32;
        fn TranslateMessage(msg: *const Msg) -> i32;
        fn DispatchMessageW(msg: *const Msg) -> isize;
        fn LoadIconW(instance: usize, name: *const u16) -> usize;
        fn CreatePopupMenu() -> usize;
        fn AppendMenuW(menu: usize, flags: u32, id: usize, item: *const u16) -> i32;
        fn DestroyMenu(menu: usize) -> i32;
        fn TrackPopupMenu(menu: usize, flags: u32, x: i32, y: i32, res: i32, hwnd: usize, rect: *const c_void) -> i32;
        fn GetCursorPos(pt: *mut Point) -> i32;
        fn SetForegroundWindow(hwnd: usize) -> i32;
    }

    #[link(name = "shell32")]
    extern "system" {
        fn Shell_NotifyIconW(message: u32, data: *const NotifyIconDataW) -> i32;
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn spawn_self(args: &[&str]) {
        if let Ok(exe) = std::env::current_exe() {
            let _ = std::process::Command::new(exe).args(args).spawn();
        }
    }

    unsafe extern "system" fn wnd_proc(hwnd: usize, msg: u32, wp: usize, lp: isize) -> isize {
        match msg {
            WM_TRAYICON => {
                let ev = (lp as u32) & 0xFFFF;
                if ev == WM_LBUTTONDBLCLK {
                    spawn_self(&["gui"]);
                } else if ev == WM_RBUTTONUP {
                    let menu = CreatePopupMenu();
                    let open_w = wide("Open PercyScarlett Dashboard");
                    let test_w = wide("Run Self-Test");
                    let exit_w = wide("Exit");
                    AppendMenuW(menu, MF_STRING, ID_OPEN_GUI, open_w.as_ptr());
                    AppendMenuW(menu, MF_STRING, ID_SELF_TEST, test_w.as_ptr());
                    AppendMenuW(menu, MF_SEPARATOR, 0, null());
                    AppendMenuW(menu, MF_STRING, ID_EXIT, exit_w.as_ptr());
                    let mut pt = Point { x: 0, y: 0 };
                    GetCursorPos(&mut pt);
                    SetForegroundWindow(hwnd);
                    TrackPopupMenu(menu, TPM_BOTTOMALIGN | TPM_LEFTALIGN, pt.x, pt.y, 0, hwnd, null());
                    DestroyMenu(menu);
                }
                0
            }
            WM_COMMAND => {
                match wp & 0xFFFF {
                    ID_OPEN_GUI => spawn_self(&["gui"]),
                    ID_SELF_TEST => spawn_self(&["self-test", "--open"]),
                    ID_EXIT => {
                        DestroyWindow(hwnd);
                    }
                    _ => {}
                }
                0
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }

    pub fn run_tray_headless() -> Result<(), String> {
        unsafe {
            FreeConsole();
            let hinst = GetModuleHandleW(null());
            let cls_name = wide("PercyScarlettTrayClass");
            let wc = WndClassW {
                style: 0,
                lpfn_wnd_proc: wnd_proc,
                cb_cls_extra: 0,
                cb_wnd_extra: 0,
                h_instance: hinst,
                h_icon: 0,
                h_cursor: 0,
                hbr_background: 0,
                lpsz_menu_name: null(),
                lpsz_class_name: cls_name.as_ptr(),
            };
            RegisterClassW(&wc);
            let win_name = wide("PercyScarlettTray");
            let hwnd = CreateWindowExW(
                0,
                cls_name.as_ptr(),
                win_name.as_ptr(),
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                hinst,
                null(),
            );
            if hwnd == 0 {
                return Err("CreateWindowExW failed for tray window".into());
            }

            // Try exe icon #1, fall back to IDI_APPLICATION (32512)
            let mut icon = LoadIconW(hinst, 1usize as *const u16);
            if icon == 0 {
                icon = LoadIconW(0, 32512usize as *const u16);
            }

            let mut nid: NotifyIconDataW = std::mem::zeroed();
            nid.cb_size = std::mem::size_of::<NotifyIconDataW>() as u32;
            nid.hwnd = hwnd;
            nid.uid = 1;
            nid.u_flags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
            nid.u_callback_message = WM_TRAYICON;
            nid.h_icon = icon;
            let tip = format!("PercyScarlett v{} — System Tray", env!("CARGO_PKG_VERSION"));
            let tip_u16: Vec<u16> = tip.encode_utf16().take(127).collect();
            nid.sz_tip[..tip_u16.len()].copy_from_slice(&tip_u16);

            if Shell_NotifyIconW(NIM_ADD, &nid) == 0 {
                DestroyWindow(hwnd);
                return Err("Shell_NotifyIconW(NIM_ADD) failed".into());
            }

            let mut msg: Msg = std::mem::zeroed();
            while GetMessageW(&mut msg, 0, 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            Shell_NotifyIconW(NIM_DELETE, &nid);
            let _ = null_mut::<c_void>();
        }
        Ok(())
    }
}

#[cfg(windows)]
pub use imp::run_tray_headless;

#[cfg(not(windows))]
pub fn run_tray_headless() -> Result<(), String> {
    Err("-headless system tray mode is Windows-only".into())
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn tray_notify_icon_data_matches_win64_abi() {
        assert_eq!(std::mem::size_of::<super::imp::NotifyIconDataW>(), 976);
    }
}
