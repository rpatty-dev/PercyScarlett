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
pub unsafe extern "system" fn BinkClose(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("Bink2Scarlett.dll!BinkClose", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn BinkDoFrame(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("Bink2Scarlett.dll!BinkDoFrame", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn BinkDoFrameAsyncMulti(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("Bink2Scarlett.dll!BinkDoFrameAsyncMulti", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn BinkDoFrameAsyncWait(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("Bink2Scarlett.dll!BinkDoFrameAsyncWait", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn BinkFreeGlobals(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("Bink2Scarlett.dll!BinkFreeGlobals", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn BinkGetFrameBuffersInfo(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("Bink2Scarlett.dll!BinkGetFrameBuffersInfo", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn BinkGetKeyFrame(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("Bink2Scarlett.dll!BinkGetKeyFrame", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn BinkGetPlatformInfo(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("Bink2Scarlett.dll!BinkGetPlatformInfo", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn BinkGoto(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("Bink2Scarlett.dll!BinkGoto", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn BinkNextFrame(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("Bink2Scarlett.dll!BinkNextFrame", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn BinkOpenWithOptions(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("Bink2Scarlett.dll!BinkOpenWithOptions", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn BinkPause(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("Bink2Scarlett.dll!BinkPause", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn BinkRegisterFrameBuffers(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("Bink2Scarlett.dll!BinkRegisterFrameBuffers", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn BinkRequestStopAsyncThreadsMulti(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("Bink2Scarlett.dll!BinkRequestStopAsyncThreadsMulti", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn BinkSetMemory(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("Bink2Scarlett.dll!BinkSetMemory", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn BinkSetOSFileCallbacks(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("Bink2Scarlett.dll!BinkSetOSFileCallbacks", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn BinkSetSoundSystem(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("Bink2Scarlett.dll!BinkSetSoundSystem", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn BinkSetVolume(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("Bink2Scarlett.dll!BinkSetVolume", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn BinkSetWillLoop(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("Bink2Scarlett.dll!BinkSetWillLoop", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn BinkShouldSkip(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("Bink2Scarlett.dll!BinkShouldSkip", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn BinkStartAsyncThread(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("Bink2Scarlett.dll!BinkStartAsyncThread", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn BinkWait(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("Bink2Scarlett.dll!BinkWait", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn BinkWaitStopAsyncThreadsMulti(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("Bink2Scarlett.dll!BinkWaitStopAsyncThreadsMulti", a0, a1, a2, a3);
    0
}

