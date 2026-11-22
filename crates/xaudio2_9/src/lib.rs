use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

const S_OK: i32 = 0;
const E_POINTER: i32 = 0x8000_4003u32 as i32;

#[repr(C)]
pub struct XAudio2VoiceState {
    pub current_buffer_context: *mut c_void,
    pub buffers_queued: u32,
    pub samples_played: u64,
}

#[repr(C)]
struct ComObj {
    vtbl: *const *const c_void,
    refs: AtomicU32,
    samples: AtomicU64,
}

unsafe extern "system" fn com_qi(
    this: *mut c_void,
    _riid: *const c_void,
    out: *mut *mut c_void,
) -> i32 {
    if out.is_null() {
        return E_POINTER;
    }
    com_add_ref(this);
    *out = this;
    S_OK
}

unsafe extern "system" fn com_add_ref(this: *mut c_void) -> u32 {
    (*(this as *const ComObj))
        .refs
        .fetch_add(1, Ordering::Relaxed)
        + 1
}

unsafe extern "system" fn com_release(this: *mut c_void) -> u32 {
    let prev = (*(this as *const ComObj))
        .refs
        .fetch_sub(1, Ordering::Release);
    if prev == 1 {
        std::sync::atomic::fence(Ordering::Acquire);
        drop(Box::from_raw(this as *mut ComObj));
    }
    prev - 1
}

unsafe extern "system" fn stub_hr() -> i32 {
    S_OK
}

unsafe extern "system" fn stub_void() {}

unsafe extern "system" fn create_source_voice(
    _this: *mut c_void,
    out_voice: *mut *mut c_void,
    _fmt: *const c_void,
    _flags: u32,
    _max_freq_ratio: f32,
    _callback: *mut c_void,
    _send_list: *const c_void,
    _effect_chain: *const c_void,
) -> i32 {
    if out_voice.is_null() {
        return E_POINTER;
    }
    *out_voice = Box::into_raw(Box::new(ComObj {
        vtbl: SOURCE_VOICE_VTBL.0.as_ptr(),
        refs: AtomicU32::new(1),
        samples: AtomicU64::new(0),
    })) as *mut c_void;
    S_OK
}

unsafe extern "system" fn create_mastering_voice(
    _this: *mut c_void,
    out_voice: *mut *mut c_void,
    _input_channels: u32,
    _input_sample_rate: u32,
    _flags: u32,
    _device_id: *const u16,
    _effect_chain: *const c_void,
    _stream_category: u32,
) -> i32 {
    if out_voice.is_null() {
        return E_POINTER;
    }
    *out_voice = Box::into_raw(Box::new(ComObj {
        vtbl: VOICE_VTBL.0.as_ptr(),
        refs: AtomicU32::new(1),
        samples: AtomicU64::new(0),
    })) as *mut c_void;
    S_OK
}

unsafe extern "system" fn voice_destroy(this: *mut c_void) {
    if !this.is_null() {
        let _ = com_release(this);
    }
}

unsafe extern "system" fn source_submit_buffer(
    this: *mut c_void,
    buf: *const c_void,
    _wma_buf: *const c_void,
) -> i32 {
    if !this.is_null() && !buf.is_null() {
        // XAUDIO2_BUFFER: +0 Flags(u32), +4 AudioBytes(u32)
        let audio_bytes = *(buf as *const u32).add(1);
        let samples = (audio_bytes / 4).max(1) as u64;
        (*(this as *const ComObj))
            .samples
            .fetch_add(samples, Ordering::Relaxed);
    }
    S_OK
}

unsafe extern "system" fn source_get_state(
    this: *mut c_void,
    out_state: *mut XAudio2VoiceState,
    _flags: u32,
) {
    if !out_state.is_null() {
        let samples = if this.is_null() {
            0
        } else {
            (*(this as *const ComObj)).samples.load(Ordering::Relaxed)
        };
        *out_state = XAudio2VoiceState {
            current_buffer_context: std::ptr::null_mut(),
            buffers_queued: 0,
            samples_played: samples,
        };
    }
}

#[repr(transparent)]
struct SyncVtbl<const N: usize>([*const c_void; N]);
unsafe impl<const N: usize> Sync for SyncVtbl<N> {}

// IXAudio2 has 13 methods (0..2 IUnknown, 3..12 IXAudio2)
static XAUDIO2_VTBL: SyncVtbl<16> = SyncVtbl([
    com_qi as *const c_void,
    com_add_ref as *const c_void,
    com_release as *const c_void,
    stub_hr as *const c_void,                // RegisterForCallbacks
    stub_void as *const c_void,              // UnregisterForCallbacks
    create_source_voice as *const c_void,    // CreateSourceVoice
    create_mastering_voice as *const c_void, // CreateSubmixVoice (compat)
    create_mastering_voice as *const c_void, // CreateMasteringVoice
    stub_hr as *const c_void,                // StartEngine
    stub_void as *const c_void,              // StopEngine
    stub_hr as *const c_void,                // CommitChanges
    stub_void as *const c_void,              // GetPerformanceData
    stub_void as *const c_void,              // SetDebugConfiguration
    stub_hr as *const c_void,
    stub_hr as *const c_void,
    stub_hr as *const c_void,
]);

// IXAudio2Voice (19 methods)
static VOICE_VTBL: SyncVtbl<20> = SyncVtbl([
    stub_void as *const c_void,    // 0 GetVoiceDetails
    stub_hr as *const c_void,      // 1 SetOutputVoices
    stub_hr as *const c_void,      // 2 SetEffectChain
    stub_hr as *const c_void,      // 3 EnableEffect
    stub_hr as *const c_void,      // 4 DisableEffect
    stub_void as *const c_void,    // 5 GetEffectState
    stub_hr as *const c_void,      // 6 SetEffectParameters
    stub_hr as *const c_void,      // 7 GetEffectParameters
    stub_hr as *const c_void,      // 8 SetFilterParameters
    stub_void as *const c_void,    // 9 GetFilterParameters
    stub_hr as *const c_void,      // 10 SetOutputFilterParameters
    stub_void as *const c_void,    // 11 GetOutputFilterParameters
    stub_hr as *const c_void,      // 12 SetVolume
    stub_void as *const c_void,    // 13 GetVolume
    stub_hr as *const c_void,      // 14 SetChannelVolumes
    stub_void as *const c_void,    // 15 GetChannelVolumes
    stub_hr as *const c_void,      // 16 SetOutputMatrix
    stub_void as *const c_void,    // 17 GetOutputMatrix
    voice_destroy as *const c_void, // 18 DestroyVoice
    stub_void as *const c_void,
]);

// IXAudio2SourceVoice extends IXAudio2Voice (19..28)
static SOURCE_VOICE_VTBL: SyncVtbl<32> = SyncVtbl([
    stub_void as *const c_void,          // 0 GetVoiceDetails
    stub_hr as *const c_void,            // 1 SetOutputVoices
    stub_hr as *const c_void,            // 2 SetEffectChain
    stub_hr as *const c_void,            // 3 EnableEffect
    stub_hr as *const c_void,            // 4 DisableEffect
    stub_void as *const c_void,          // 5 GetEffectState
    stub_hr as *const c_void,            // 6 SetEffectParameters
    stub_hr as *const c_void,            // 7 GetEffectParameters
    stub_hr as *const c_void,            // 8 SetFilterParameters
    stub_void as *const c_void,          // 9 GetFilterParameters
    stub_hr as *const c_void,            // 10 SetOutputFilterParameters
    stub_void as *const c_void,          // 11 GetOutputFilterParameters
    stub_hr as *const c_void,            // 12 SetVolume
    stub_void as *const c_void,          // 13 GetVolume
    stub_hr as *const c_void,            // 14 SetChannelVolumes
    stub_void as *const c_void,          // 15 GetChannelVolumes
    stub_hr as *const c_void,            // 16 SetOutputMatrix
    stub_void as *const c_void,          // 17 GetOutputMatrix
    voice_destroy as *const c_void,      // 18 DestroyVoice
    stub_hr as *const c_void,            // 19 Start
    stub_hr as *const c_void,            // 20 Stop
    source_submit_buffer as *const c_void, // 21 SubmitSourceBuffer
    stub_hr as *const c_void,            // 22 FlushSourceBuffers
    stub_hr as *const c_void,            // 23 Discontinuity
    stub_hr as *const c_void,            // 24 ExitLoop
    source_get_state as *const c_void,   // 25 GetState
    stub_hr as *const c_void,            // 26 SetFrequencyRatio
    stub_void as *const c_void,          // 27 GetFrequencyRatio
    stub_hr as *const c_void,            // 28 SetSourceSampleRate
    stub_hr as *const c_void,
    stub_hr as *const c_void,
    stub_hr as *const c_void,
]);

#[no_mangle]
pub unsafe extern "system" fn XAudio2Create(
    out_xa2: *mut *mut c_void,
    _flags: u32,
    _processor: u32,
) -> i32 {
    if out_xa2.is_null() {
        return E_POINTER;
    }
    *out_xa2 = Box::into_raw(Box::new(ComObj {
        vtbl: XAUDIO2_VTBL.0.as_ptr(),
        refs: AtomicU32::new(1),
        samples: AtomicU64::new(0),
    })) as *mut c_void;
    S_OK
}

#[no_mangle]
pub unsafe extern "system" fn XAudio2CreateWithVersionInfo(
    out_xa2: *mut *mut c_void,
    flags: u32,
    processor: u32,
    _ntddi_version: u32,
) -> i32 {
    XAudio2Create(out_xa2, flags, processor)
}

#[no_mangle]
pub unsafe extern "system" fn CreateAudioReverb(out_apo: *mut *mut c_void) -> i32 {
    XAudio2Create(out_apo, 0, 1)
}

#[no_mangle]
pub unsafe extern "system" fn CreateAudioVolumeMeter(out_apo: *mut *mut c_void) -> i32 {
    XAudio2Create(out_apo, 0, 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xaudio2_create_mastering_and_source_voice_work() {
        unsafe {
            let mut xa2: *mut c_void = std::ptr::null_mut();
            assert_eq!(XAudio2Create(&mut xa2, 0, 1), S_OK);
            assert!(!xa2.is_null());

            let vtbl = *(xa2 as *const *const *const c_void);
            let create_master: unsafe extern "system" fn(
                *mut c_void,
                *mut *mut c_void,
                u32,
                u32,
                u32,
                *const u16,
                *const c_void,
                u32,
            ) -> i32 = std::mem::transmute(*vtbl.add(7));
            let mut master: *mut c_void = std::ptr::null_mut();
            assert_eq!(create_master(xa2, &mut master, 2, 48000, 0, std::ptr::null(), std::ptr::null(), 0), S_OK);
            assert!(!master.is_null());

            let create_src: unsafe extern "system" fn(
                *mut c_void,
                *mut *mut c_void,
                *const c_void,
                u32,
                f32,
                *mut c_void,
                *const c_void,
                *const c_void,
            ) -> i32 = std::mem::transmute(*vtbl.add(5));
            let mut src: *mut c_void = std::ptr::null_mut();
            assert_eq!(create_src(xa2, &mut src, std::ptr::null(), 0, 2.0, std::ptr::null_mut(), std::ptr::null(), std::ptr::null()), S_OK);
            assert!(!src.is_null());

            let src_vtbl = *(src as *const *const *const c_void);
            let submit: unsafe extern "system" fn(*mut c_void, *const c_void, *const c_void) -> i32 =
                std::mem::transmute(*src_vtbl.add(21));
            let buf = [0u32, 4096u32]; // Flags=0, AudioBytes=4096 -> 1024 samples
            assert_eq!(submit(src, buf.as_ptr() as *const c_void, std::ptr::null()), S_OK);

            let get_state: unsafe extern "system" fn(*mut c_void, *mut XAudio2VoiceState, u32) =
                std::mem::transmute(*src_vtbl.add(25));
            let mut st = std::mem::zeroed::<XAudio2VoiceState>();
            get_state(src, &mut st, 0);
            assert_eq!(st.samples_played, 1024);

            let destroy_src: unsafe extern "system" fn(*mut c_void) = std::mem::transmute(*src_vtbl.add(18));
            destroy_src(src);
            let master_vtbl = *(master as *const *const *const c_void);
            let destroy_master: unsafe extern "system" fn(*mut c_void) = std::mem::transmute(*master_vtbl.add(18));
            destroy_master(master);

            let release: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(*vtbl.add(2));
            assert_eq!(release(xa2), 0);
        }
    }
}
