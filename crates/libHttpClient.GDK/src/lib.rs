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
pub unsafe extern "system" fn HCCleanupAsync(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCCleanupAsync", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCHttpCallCloseHandle(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCHttpCallCloseHandle", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCHttpCallCreate(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCHttpCallCreate", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCHttpCallDuplicateHandle(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCHttpCallDuplicateHandle", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCHttpCallGetRequestUrl(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCHttpCallGetRequestUrl", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCHttpCallPerformAsync(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCHttpCallPerformAsync", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCHttpCallRequestGetHeaderAtIndex(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCHttpCallRequestGetHeaderAtIndex", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCHttpCallRequestGetNumHeaders(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCHttpCallRequestGetNumHeaders", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCHttpCallRequestGetRequestBodyBytes(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCHttpCallRequestGetRequestBodyBytes", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCHttpCallRequestGetRetryAllowed(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCHttpCallRequestGetRetryAllowed", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCHttpCallRequestGetRetryCacheId(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCHttpCallRequestGetRetryCacheId", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCHttpCallRequestGetRetryDelay(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCHttpCallRequestGetRetryDelay", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCHttpCallRequestGetTimeout(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCHttpCallRequestGetTimeout", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCHttpCallRequestGetTimeoutWindow(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCHttpCallRequestGetTimeoutWindow", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCHttpCallRequestGetUrl(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCHttpCallRequestGetUrl", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCHttpCallRequestSetHeader(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCHttpCallRequestSetHeader", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCHttpCallRequestSetRequestBodyBytes(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCHttpCallRequestSetRequestBodyBytes", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCHttpCallRequestSetRequestBodyString(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCHttpCallRequestSetRequestBodyString", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCHttpCallRequestSetRetryAllowed(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCHttpCallRequestSetRetryAllowed", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCHttpCallRequestSetRetryCacheId(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCHttpCallRequestSetRetryCacheId", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCHttpCallRequestSetRetryDelay(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCHttpCallRequestSetRetryDelay", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCHttpCallRequestSetTimeout(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCHttpCallRequestSetTimeout", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCHttpCallRequestSetTimeoutWindow(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCHttpCallRequestSetTimeoutWindow", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCHttpCallRequestSetUrl(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCHttpCallRequestSetUrl", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCHttpCallResponseGetHeader(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCHttpCallResponseGetHeader", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCHttpCallResponseGetHeaderAtIndex(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCHttpCallResponseGetHeaderAtIndex", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCHttpCallResponseGetNetworkErrorCode(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCHttpCallResponseGetNetworkErrorCode", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCHttpCallResponseGetNumHeaders(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCHttpCallResponseGetNumHeaders", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCHttpCallResponseGetPlatformNetworkErrorMessage(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCHttpCallResponseGetPlatformNetworkErrorMessage", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCHttpCallResponseGetResponseBodyBytes(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCHttpCallResponseGetResponseBodyBytes", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCHttpCallResponseGetResponseBodyBytesSize(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCHttpCallResponseGetResponseBodyBytesSize", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCHttpCallResponseGetResponseString(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCHttpCallResponseGetResponseString", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCHttpCallResponseGetStatusCode(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCHttpCallResponseGetStatusCode", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCHttpCallSetTracing(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCHttpCallSetTracing", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCInitialize(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCInitialize", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCSettingsGetTraceLevel(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCSettingsGetTraceLevel", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCTraceImplMessage(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCTraceImplMessage", a0, a1, a2, a3);
    0
}

#[no_mangle]
pub unsafe extern "system" fn HCTraceSetEtwEnabled(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    log_stub("libHttpClient.GDK.dll!HCTraceSetEtwEnabled", a0, a1, a2, a3);
    0
}

