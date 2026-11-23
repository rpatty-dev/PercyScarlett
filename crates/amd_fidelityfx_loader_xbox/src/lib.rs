use std::fs::OpenOptions;
use std::io::Write;

fn log_stub(name: &str, a0: usize, a1: usize, a2: usize, a3: usize) {
    if let Ok(p) = std::env::var("PERCY_STUB_LOG") {
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(p) {
            let _ = writeln!(f, "{name}(0x{a0:x}, 0x{a1:x}, 0x{a2:x}, 0x{a3:x})");
        }
    }
    if let Ok(ab) = std::env::var("PERCY_STUB_ABORT") {
        if !ab.is_empty() && name.contains(&ab) {
            std::process::abort();
        }
    }
}

#[no_mangle]
pub unsafe extern "system" fn ffxCreateContext(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("amd_fidelityfx_loader_xbox.dll!ffxCreateContext", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn ffxDestroyContext(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("amd_fidelityfx_loader_xbox.dll!ffxDestroyContext", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn ffxDispatch(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("amd_fidelityfx_loader_xbox.dll!ffxDispatch", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn ffxQuery(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("amd_fidelityfx_loader_xbox.dll!ffxQuery", a0, a1, a2, a3);
    0
}

