use core::ffi::c_void;
use core::mem::transmute;
use core::ptr::{null, null_mut};

#[link(name = "d3d12_xs", kind = "dylib")]
extern "system" {
    fn D3D12XboxCreateDevice(
        adapter: *mut c_void,
        params: *const c_void,
        riid: *const [u8; 16],
        out_dev: *mut *mut c_void,
    ) -> i32;
    fn PercyEnsureAutoSwapChain(queue: *mut c_void, width: u32, height: u32) -> i32;
    fn PercyLiftRdna2ToDxil(
        rdna2_ptr: *const u8,
        rdna2_len: usize,
        stage_hint: u32,
        out_ptr: *mut *mut u8,
        out_len: *mut usize,
    ) -> i32;
    fn WaitForOriginX(queue: *mut c_void, reserved: u64, timeout_us: u32) -> i32;
    fn PresentX(queue: *mut c_void, params: *const c_void, reserved: *const c_void) -> i32;
    fn PercyGetPresentedFrameCount() -> u64;
}

#[link(name = "xmem", kind = "dylib")]
extern "system" {
    fn XMemAlloc(size: usize, attributes: u64) -> *mut c_void;
    fn XMemFree(ptr: *mut c_void, attributes: u64);
}

#[link(name = "xgameruntime", kind = "dylib")]
extern "system" {
    fn XSystemGetDeviceType() -> u32;
    fn XTaskQueueCreate(work: u32, comp: u32, out_q: *mut *mut c_void) -> i32;
    fn XTaskQueueCloseHandle(q: *mut c_void);
}

#[link(name = "xaudio2_9", kind = "dylib")]
extern "system" {
    fn XAudio2Create(out_xa2: *mut *mut c_void, flags: u32, processor: u32) -> i32;
}

#[repr(C)]
struct CommandQueueDesc {
    ty: u32,
    priority: i32,
    flags: u32,
    node_mask: u32,
}

fn main() {
    unsafe {
        println!("[ScarlettSample] XSystemGetDeviceType = {}", XSystemGetDeviceType());

        let mem = XMemAlloc(1024 * 1024, 0);
        assert!(!mem.is_null(), "XMemAlloc failed");
        XMemFree(mem, 0);

        let mut tq: *mut c_void = null_mut();
        assert_eq!(XTaskQueueCreate(0, 0, &mut tq), 0);

        let mut xa2: *mut c_void = null_mut();
        assert_eq!(XAudio2Create(&mut xa2, 0, 1), 0);
        println!("[ScarlettSample] XAudio2Create OK ({xa2:?})");

        // Real IID_ID3D12Device {189819f1-1db6-4b57-be54-1821339b85f7}
        const IID_ID3D12_DEVICE: [u8; 16] = [
            0xF1, 0x19, 0x98, 0x18, 0xB6, 0x1D, 0x57, 0x4B,
            0xBE, 0x54, 0x18, 0x21, 0x33, 0x9B, 0x85, 0xF7,
        ];
        // Note: test direct hardware ID3D12Device without wrapper so vtable slot 8 (CreateCommandQueue) runs natively
        std::env::remove_var("PERCY_D3D12_WRAP");
        let mut dev: *mut c_void = null_mut();
        let hr = D3D12XboxCreateDevice(null_mut(), null(), &IID_ID3D12_DEVICE, &mut dev);
        assert_eq!(hr, 0, "D3D12XboxCreateDevice failed: 0x{hr:08x}");
        println!("[ScarlettSample] D3D12XboxCreateDevice OK ({dev:?})");

        // ID3D12Device::CreateCommandQueue is vtable slot 8
        // IID_ID3D12CommandQueue {0ec870a6-5d7e-4c22-8cfc-5baae07616ed}
        const IID_ID3D12_COMMAND_QUEUE: [u8; 16] = [
            0xA6, 0x70, 0xC8, 0x0E, 0x7E, 0x5D, 0x22, 0x4C,
            0x8C, 0xFC, 0x5B, 0xAA, 0xE0, 0x76, 0x16, 0xED,
        ];
        let dev_vtbl = *(dev as *const *const *const c_void);
        let create_cq: unsafe extern "system" fn(
            *mut c_void,
            *const CommandQueueDesc,
            *const [u8; 16],
            *mut *mut c_void,
        ) -> i32 = transmute(*dev_vtbl.add(8));
        let desc = CommandQueueDesc { ty: 0, priority: 0, flags: 0, node_mask: 0 };
        let mut cq: *mut c_void = null_mut();
        let hr_cq = create_cq(dev, &desc, &IID_ID3D12_COMMAND_QUEUE, &mut cq);
        assert_eq!(hr_cq, 0, "CreateCommandQueue failed: 0x{hr_cq:08x}");
        println!("[ScarlettSample] ID3D12CommandQueue created ({cq:?})");

        // Auto-create Win32 window + DXGI SwapChain
        let hr_sc = PercyEnsureAutoSwapChain(cq, 800, 600);
        assert_eq!(hr_sc, 0, "PercyEnsureAutoSwapChain failed: 0x{hr_sc:08x}");
        println!("[ScarlettSample] Auto-HWND + IDXGISwapChain1 created!");

        // Lift RDNA2 Vertex + Pixel shaders -> SM 6.6 DXIL live
        let mut rdna2 = Vec::new();
        rdna2.extend_from_slice(&((0x3Fu32 << 25) | (0u32 << 17) | (0x01u32 << 9) | 242u32).to_le_bytes()); // v_mov_b32 v0, 1.0
        rdna2.extend_from_slice(&((0x3Eu32 << 26) | (12u32 << 4) | 0xFu32).to_le_bytes()); // exp pos0
        rdna2.extend_from_slice(&0u32.to_le_bytes());
        rdna2.extend_from_slice(&0xBF81_0000u32.to_le_bytes()); // s_endpgm

        for (name, stage) in [("VS", 0u32), ("PS", 1u32)] {
            let mut dxil_ptr: *mut u8 = null_mut();
            let mut dxil_len: usize = 0;
            let hr_sh = PercyLiftRdna2ToDxil(rdna2.as_ptr(), rdna2.len(), stage, &mut dxil_ptr, &mut dxil_len);
            assert_eq!(hr_sh, 0, "PercyLiftRdna2ToDxil({name}) failed");
            println!("[ScarlettSample] RDNA2->{name}_6_6 DXIL compiled ({dxil_len} bytes)");
        }

        // Present 10 real DXGI frames via PresentX (accepts S_OK and DXGI_STATUS_OCCLUDED >= 0)
        for f in 1..=10 {
            assert_eq!(WaitForOriginX(cq, 0, 16667), 0);
            let hr_p = PresentX(cq, null(), null());
            assert!(hr_p >= 0, "PresentX frame {f} failed: 0x{hr_p:08x}");
        }
        println!(
            "[ScarlettSample] Presented {} real DXGI frames via PresentX!",
            PercyGetPresentedFrameCount()
        );

        XTaskQueueCloseHandle(tq);
    }
}
