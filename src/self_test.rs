use std::ffi::{c_void, CString};
use std::fmt::Write as _;
use std::path::Path;

#[derive(Clone, Debug)]
pub struct SelfTestResult {
    pub passed: usize,
    pub total: usize,
    pub summary: String,
    pub markdown: String,
    pub issue_url: String,
}

pub fn url_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len() * 2);
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push_str("%20"),
            b'\n' => out.push_str("%0A"),
            _ => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

#[cfg(windows)]
fn query_memory_and_os() -> (String, String) {
    #[repr(C)]
    struct MemoryStatusEx {
        len: u32,
        mem_load: u32,
        total_phys: u64,
        avail_phys: u64,
        total_page: u64,
        avail_page: u64,
        total_virt: u64,
        avail_virt: u64,
        avail_ext: u64,
    }
    #[repr(C)]
    struct OsVersionInfoW {
        size: u32,
        major: u32,
        minor: u32,
        build: u32,
        platform: u32,
        csd: [u16; 128],
    }
    extern "system" {
        fn GlobalMemoryStatusEx(buf: *mut MemoryStatusEx) -> i32;
        fn RtlGetVersion(info: *mut OsVersionInfoW) -> i32;
    }
    let ram = unsafe {
        let mut m: MemoryStatusEx = std::mem::zeroed();
        m.len = std::mem::size_of::<MemoryStatusEx>() as u32;
        if GlobalMemoryStatusEx(&mut m) != 0 {
            format!(
                "{:.1} GiB total ({:.1} GiB free)",
                m.total_phys as f64 / (1024.0 * 1024.0 * 1024.0),
                m.avail_phys as f64 / (1024.0 * 1024.0 * 1024.0)
            )
        } else {
            "unknown".into()
        }
    };
    let win_ver = unsafe {
        let mut v: OsVersionInfoW = std::mem::zeroed();
        v.size = std::mem::size_of::<OsVersionInfoW>() as u32;
        if RtlGetVersion(&mut v) == 0 {
            let label = if v.major == 10 && v.build >= 22000 { "Windows 11" } else if v.major == 10 { "Windows 10" } else { "Windows" };
            format!("{label} ({}.{}.{})", v.major, v.minor, v.build)
        } else {
            "Windows".into()
        }
    };
    (win_ver, ram)
}

#[cfg(not(windows))]
fn query_memory_and_os() -> (String, String) {
    (std::env::consts::OS.into(), "unknown".into())
}

#[cfg(windows)]
fn load_dll_handle(dir: &Path, name: &str) -> usize {
    extern "system" {
        fn LoadLibraryA(name: *const i8) -> usize;
    }
    let full = dir.join(name);
    if let Ok(cs) = CString::new(full.to_string_lossy().as_bytes()) {
        let h = unsafe { LoadLibraryA(cs.as_ptr()) };
        if h != 0 {
            return h;
        }
    }
    if let Ok(cs) = CString::new(name) {
        return unsafe { LoadLibraryA(cs.as_ptr()) };
    }
    0
}

#[cfg(windows)]
fn proc_addr(hmod: usize, sym: &str) -> usize {
    extern "system" {
        fn GetProcAddress(module: usize, name: *const i8) -> usize;
    }
    if hmod == 0 {
        return 0;
    }
    CString::new(sym)
        .ok()
        .map(|cs| unsafe { GetProcAddress(hmod, cs.as_ptr()) })
        .unwrap_or(0)
}

#[cfg(windows)]
fn probe_dxgi_and_d3d12_caps() -> (String, Result<String, String>) {
    type CreateFactory1Fn = unsafe extern "system" fn(*const [u8; 16], *mut *mut c_void) -> i32;
    type CreateDeviceFn = unsafe extern "system" fn(*mut c_void, u32, *const [u8; 16], *mut *mut c_void) -> i32;
    #[repr(C)]
    struct AdapterDesc1 {
        desc: [u16; 128],
        vendor_id: u32,
        device_id: u32,
        subsys_id: u32,
        rev: u32,
        ded_vram: usize,
        ded_sys: usize,
        shared_sys: usize,
        luid: u64,
        flags: u32,
    }
    #[repr(C)]
    struct Opt1 {
        wave_ops: i32,
        lane_min: u32,
        lane_max: u32,
        total_lanes: u32,
        expanded_compute: i32,
        int64_ops: i32,
    }
    #[repr(C)]
    struct Opt7 {
        mesh_tier: u32,
        sampler_feedback_tier: u32,
    }

    let dxgi = load_dll_handle(Path::new("."), "dxgi.dll");
    let d3d12 = load_dll_handle(Path::new("."), "d3d12.dll");
    let f_create = proc_addr(dxgi, "CreateDXGIFactory1");
    let d_create = proc_addr(d3d12, "D3D12CreateDevice");
    if f_create == 0 || d_create == 0 {
        return ("unknown".into(), Err("dxgi.dll or d3d12.dll missing".into()));
    }

    // IID_IDXGIFactory1: {770aae78-f26f-4dba-a829-253c83d1b387}
    let iid_factory1: [u8; 16] = [
        0x78, 0xae, 0x0a, 0x77, 0x6f, 0xf2, 0xba, 0x4d, 0xa8, 0x29, 0x25, 0x3c, 0x83, 0xd1, 0xb3, 0x87,
    ];
    // IID_ID3D12Device: {189819f1-1db6-4b57-be54-1821339b85f7}
    let iid_device: [u8; 16] = [
        0xf1, 0x19, 0x98, 0x18, 0xb6, 0x1d, 0x57, 0x4b, 0xbe, 0x54, 0x18, 0x21, 0x33, 0x9b, 0x85, 0xf7,
    ];

    unsafe {
        let create_fac: CreateFactory1Fn = std::mem::transmute(f_create);
        let mut fac: *mut c_void = std::ptr::null_mut();
        if create_fac(&iid_factory1, &mut fac) != 0 || fac.is_null() {
            return ("unknown".into(), Err("CreateDXGIFactory1 failed".into()));
        }
        let fac_vtbl = &*(*(fac as *const *const [*const c_void; 16]));
        // IDXGIFactory1::EnumAdapters1 is vtable slot 12
        let enum_adapters1: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> i32 =
            std::mem::transmute(fac_vtbl[12]);
        let fac_rel: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(fac_vtbl[2]);

        let mut adapter: *mut c_void = std::ptr::null_mut();
        if enum_adapters1(fac, 0, &mut adapter) != 0 || adapter.is_null() {
            fac_rel(fac);
            return ("none".into(), Err("No DXGI adapter 0".into()));
        }
        let ad_vtbl = &*(*(adapter as *const *const [*const c_void; 12]));
        // IDXGIAdapter1::GetDesc1 is vtable slot 10
        let get_desc1: unsafe extern "system" fn(*mut c_void, *mut AdapterDesc1) -> i32 =
            std::mem::transmute(ad_vtbl[10]);
        let ad_rel: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(ad_vtbl[2]);

        let mut desc: AdapterDesc1 = std::mem::zeroed();
        let gpu_label = if get_desc1(adapter, &mut desc) == 0 {
            let end = desc.desc.iter().position(|&c| c == 0).unwrap_or(128);
            let name = String::from_utf16_lossy(&desc.desc[..end]);
            let vram_mb = desc.ded_vram / (1024 * 1024);
            format!(
                "{name} (VRAM {vram_mb} MiB, ven=0x{:04x}, dev=0x{:04x})",
                desc.vendor_id, desc.device_id
            )
        } else {
            "DXGI Adapter 0".into()
        };

        let create_dev: CreateDeviceFn = std::mem::transmute(d_create);
        let mut dev: *mut c_void = std::ptr::null_mut();
        // D3D_FEATURE_LEVEL_12_0 = 0xC000
        let hr = create_dev(adapter, 0xC000, &iid_device, &mut dev);
        ad_rel(adapter);
        fac_rel(fac);
        if hr != 0 || dev.is_null() {
            return (gpu_label, Err(format!("D3D12CreateDevice(12_0) hr=0x{hr:08x}")));
        }

        let dev_vtbl = &*(*(dev as *const *const [*const c_void; 44]));
        // ID3D12Device::CheckFeatureSupport is vtable slot 13
        let check_feat: unsafe extern "system" fn(*mut c_void, u32, *mut c_void, u32) -> i32 =
            std::mem::transmute(dev_vtbl[13]);
        let dev_rel: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(dev_vtbl[2]);

        // D3D12_FEATURE_SHADER_MODEL (7): query SM 6.6 (0x66)
        let mut sm: u32 = 0x66;
        let sm_ok = check_feat(dev, 7, &mut sm as *mut u32 as *mut c_void, 4) == 0;
        // D3D12_FEATURE_D3D12_OPTIONS1 (18): WaveOps
        let mut opt1: Opt1 = std::mem::zeroed();
        let w_ok = check_feat(dev, 18, &mut opt1 as *mut Opt1 as *mut c_void, std::mem::size_of::<Opt1>() as u32) == 0;
        // D3D12_FEATURE_D3D12_OPTIONS7 (32): MeshShaderTier
        let mut opt7: Opt7 = std::mem::zeroed();
        let m_ok = check_feat(dev, 32, &mut opt7 as *mut Opt7 as *mut c_void, std::mem::size_of::<Opt7>() as u32) == 0;
        dev_rel(dev);

        let sm_str = if sm_ok { format!("SM 6.{}", sm & 0xF) } else { "SM <6.6".into() };
        let wave_str = if w_ok && opt1.wave_ops != 0 {
            format!("Wave {}-{}", opt1.lane_min, opt1.lane_max)
        } else {
            "No WaveOps".into()
        };
        let mesh_str = if m_ok && opt7.mesh_tier > 0 {
            format!("MeshTier {}", opt7.mesh_tier)
        } else {
            "No MeshShader".into()
        };
        (gpu_label, Ok(format!("FL12_0, {sm_str}, {wave_str}, {mesh_str}")))
    }
}

#[cfg(not(windows))]
fn probe_dxgi_and_d3d12_caps() -> (String, Result<String, String>) {
    ("non-Windows".into(), Err("Windows only".into()))
}

#[cfg(windows)]
fn check_pe_loader_minimal(shim_dir: &Path) -> Result<String, String> {
    let mut pe = vec![0u8; 0x2000];
    pe[0..2].copy_from_slice(b"MZ");
    pe[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    pe[0x80..0x84].copy_from_slice(b"PE\0\0");
    pe[0x84..0x86].copy_from_slice(&0x8664u16.to_le_bytes());
    pe[0x86..0x88].copy_from_slice(&1u16.to_le_bytes());
    pe[0x94..0x96].copy_from_slice(&0xF0u16.to_le_bytes());
    pe[0x98..0x9A].copy_from_slice(&0x020Bu16.to_le_bytes());
    pe[0xA8..0xAC].copy_from_slice(&0x1000u32.to_le_bytes());
    pe[0xB0..0xB8].copy_from_slice(&0x1_4000_0000u64.to_le_bytes());
    pe[0xD0..0xD4].copy_from_slice(&0x2000u32.to_le_bytes());
    pe[0xD4..0xD8].copy_from_slice(&0x200u32.to_le_bytes());
    pe[0x104..0x108].copy_from_slice(&16u32.to_le_bytes());
    pe[0x120..0x124].copy_from_slice(&0x1020u32.to_le_bytes());
    pe[0x124..0x128].copy_from_slice(&12u32.to_le_bytes());
    pe[0x188..0x18D].copy_from_slice(b".text");
    pe[0x190..0x194].copy_from_slice(&0x1000u32.to_le_bytes());
    pe[0x194..0x198].copy_from_slice(&0x1000u32.to_le_bytes());
    pe[0x198..0x19C].copy_from_slice(&0x1000u32.to_le_bytes());
    pe[0x19C..0x1A0].copy_from_slice(&0x1000u32.to_le_bytes());
    pe[0x1AC..0x1B0].copy_from_slice(&0x6000_0020u32.to_le_bytes());
    pe[0x1000..0x1006].copy_from_slice(&[0xB8, 42, 0, 0, 0, 0xC3]);
    pe[0x1020..0x1024].copy_from_slice(&0x1000u32.to_le_bytes());
    pe[0x1024..0x1028].copy_from_slice(&0x1006u32.to_le_bytes());
    pe[0x1028..0x102C].copy_from_slice(&0x1030u32.to_le_bytes());
    pe[0x1030] = 0x01;
    let (st, rc) = crate::pe_loader::load_and_run_pe64(&pe, shim_dir, true)?;
    if rc == 42 {
        Ok(format!("rc=42, pdata={}, base=0x{:x}", st.pdata_entries, st.mapped_base))
    } else {
        Err(format!("unexpected rc={rc}"))
    }
}

#[cfg(not(windows))]
fn check_pe_loader_minimal(_shim_dir: &Path) -> Result<String, String> {
    Err("Windows only".into())
}

pub fn run_self_test(shim_dir: &Path) -> SelfTestResult {
    let ver = env!("CARGO_PKG_VERSION");
    let arch = std::env::consts::ARCH;
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0);
    let cpu = std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_else(|_| "unknown".into());
    let (win_ver, ram) = query_memory_and_os();
    let (gpu_label, dx12_caps) = probe_dxgi_and_d3d12_caps();

    let mut passed = 0usize;
    let mut total = 0usize;
    let mut lines = Vec::new();

    // 1. 9 Shim DLLs + key export check
    total += 1;
    let shims: &[(&str, &str)] = &[
        ("xmem.dll", "XMemAllocDefault"),
        ("kernelx.dll", "TitleMemoryStatus"),
        ("d3d12_xs.dll", "D3D12XboxCreateDevice"),
        ("xg_xs.dll", "XGCreateTexture2DComputer"),
        ("xgameruntime.dll", "XTaskQueueCreate"),
        ("dstorage.dll", "DStorageGetFactory"),
        ("GameInput.dll", "GameInputCreate"),
        ("winrt_xs.dll", "WindowsCreateString"),
        ("xaudio2_9.dll", "XAudio2Create"),
    ];
    let mut ok_shims = 0usize;
    let mut missing_shims = Vec::new();
    let mut d3d12_xs_h = 0usize;
    let mut xaudio_h = 0usize;
    for &(dll, sym) in shims {
        #[cfg(windows)]
        let h = load_dll_handle(shim_dir, dll);
        #[cfg(not(windows))]
        let h = 0usize;
        #[cfg(windows)]
        let sym_ok = proc_addr(h, sym) != 0;
        #[cfg(not(windows))]
        let sym_ok = false;
        if h != 0 && sym_ok {
            ok_shims += 1;
            if dll == "d3d12_xs.dll" {
                d3d12_xs_h = h;
            } else if dll == "xaudio2_9.dll" {
                xaudio_h = h;
            }
        } else {
            missing_shims.push(dll);
        }
    }
    if ok_shims == shims.len() {
        passed += 1;
        lines.push(format!("- [x] **Shim DLLs & Exports:** {ok_shims}/{} live", shims.len()));
    } else {
        lines.push(format!(
            "- [ ] **Shim DLLs & Exports:** {ok_shims}/{} live (missing: {})",
            shims.len(),
            missing_shims.join(", ")
        ));
    }

    // 2. Hardware DX12 Device + SM 6.6 + Wave + Mesh Shader Caps
    total += 1;
    match dx12_caps {
        Ok(caps) => {
            passed += 1;
            lines.push(format!("- [x] **Hardware DX12 Caps:** {caps}"));
        }
        Err(e) => lines.push(format!("- [ ] **Hardware DX12 Caps:** FAIL (`{e}`)")),
    }

    // 3. d3d12_xs PresentX + WaitForOriginX
    total += 1;
    #[cfg(windows)]
    {
        let p_present = proc_addr(d3d12_xs_h, "PresentX");
        let p_wait = proc_addr(d3d12_xs_h, "WaitForOriginX");
        if p_present != 0 && p_wait != 0 {
            unsafe {
                let present: unsafe extern "system" fn(*mut c_void, *const c_void, *const c_void) -> i32 =
                    std::mem::transmute(p_present);
                let wait: unsafe extern "system" fn(*mut c_void, u64, u32) -> i32 = std::mem::transmute(p_wait);
                let hr1 = present(std::ptr::null_mut(), std::ptr::null(), std::ptr::null());
                let hr2 = wait(std::ptr::null_mut(), 0, 16667);
                if hr1 == 0 && hr2 == 0 {
                    passed += 1;
                    lines.push("- [x] **d3d12_xs PresentX / WaitForOriginX:** OK".into());
                } else {
                    lines.push(format!("- [ ] **d3d12_xs PresentX:** FAIL (hr1=0x{hr1:x}, hr2=0x{hr2:x})"));
                }
            }
        } else {
            lines.push("- [ ] **d3d12_xs PresentX:** skipped (d3d12_xs.dll not loaded)".into());
        }
    }
    #[cfg(not(windows))]
    lines.push("- [ ] **d3d12_xs PresentX:** skipped (non-Windows)".into());

    // 4. XAudio2.9 live engine check
    total += 1;
    #[cfg(windows)]
    {
        let p_xa = proc_addr(xaudio_h, "XAudio2Create");
        if p_xa != 0 {
            unsafe {
                let xa_create: unsafe extern "system" fn(*mut *mut c_void, u32, u32) -> i32 =
                    std::mem::transmute(p_xa);
                let mut xa: *mut c_void = std::ptr::null_mut();
                let hr = xa_create(&mut xa, 0, 1);
                if hr == 0 && !xa.is_null() {
                    let vtbl = &*(*(xa as *const *const [*const c_void; 4]));
                    let rel: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(vtbl[2]);
                    rel(xa);
                    passed += 1;
                    lines.push("- [x] **XAudio2Create:** OK".into());
                } else {
                    lines.push(format!("- [ ] **XAudio2Create:** FAIL (`0x{hr:08x}`)"));
                }
            }
        } else {
            lines.push("- [ ] **XAudio2Create:** skipped (xaudio2_9.dll not loaded)".into());
        }
    }
    #[cfg(not(windows))]
    lines.push("- [ ] **XAudio2Create:** skipped (non-Windows)".into());

    // 5. XMem Physical Pool
    total += 1;
    let mut pool = crate::xmem::PhysicalPool::default();
    match pool.alloc(16 * 1024 * 1024) {
        Ok(addr) => {
            pool.free(addr);
            passed += 1;
            lines.push(format!("- [x] **XMem 16 MiB Pool:** OK (`0x{addr:x}`)"));
        }
        Err(e) => lines.push(format!("- [ ] **XMem 16 MiB Pool:** FAIL (`{e}`)")),
    }

    // 6. In-process PE64 loader
    total += 1;
    match check_pe_loader_minimal(shim_dir) {
        Ok(msg) => {
            passed += 1;
            lines.push(format!("- [x] **PE32+ Loader:** OK ({msg})"));
        }
        Err(e) => lines.push(format!("- [ ] **PE32+ Loader:** FAIL (`{e}`)")),
    }

    // 7. RDNA2 -> HLSL -> DXBC (d3dcompiler_47.dll)
    total += 1;
    let mut prog = Vec::new();
    prog.extend_from_slice(&((0x3Fu32 << 25) | (0u32 << 17) | (0x01u32 << 9) | 242u32).to_le_bytes());
    prog.extend_from_slice(&((0x3Eu32 << 26) | (12u32 << 4) | 0xFu32).to_le_bytes());
    prog.extend_from_slice(&0u32.to_le_bytes());
    prog.extend_from_slice(&0xBF81_0000u32.to_le_bytes());
    let (stage, hlsl) = d3d12_xs::translate_and_cache_rdna2(&prog, d3d12_xs::rdna2_hlsl::ShaderStage::Vertex);
    #[cfg(windows)]
    match d3d12_xs::compile_hlsl_to_dxbc(&hlsl, stage) {
        Ok(dxbc) => {
            passed += 1;
            lines.push(format!("- [x] **RDNA2 -> DXBC (`d3dcompiler_47.dll`):** OK ({} bytes)", dxbc.len()));
        }
        Err(e) => lines.push(format!("- [ ] **RDNA2 -> DXBC:** FAIL (`{e}`)")),
    }
    #[cfg(not(windows))]
    lines.push("- [ ] **RDNA2 -> DXBC:** skipped (non-Windows)".into());

    // 8. RDNA2 -> SM 6.6 DXIL (VS/PS/CS/Mesh via dxcompiler.dll)
    total += 1;
    #[cfg(windows)]
    {
        let mut stages_ok = 0usize;
        let mut last_err = String::new();
        for st in [
            d3d12_xs::rdna2_hlsl::ShaderStage::Vertex,
            d3d12_xs::rdna2_hlsl::ShaderStage::Pixel,
            d3d12_xs::rdna2_hlsl::ShaderStage::Compute,
            d3d12_xs::rdna2_hlsl::ShaderStage::Mesh,
        ] {
            let (s2, h2) = d3d12_xs::translate_and_cache_rdna2(&prog, st);
            match d3d12_xs::compile_hlsl_to_dxil(&h2, s2) {
                Ok(_) => stages_ok += 1,
                Err(e) => last_err = format!("{}: {e}", st.as_str()),
            }
        }
        if stages_ok == 4 {
            passed += 1;
            lines.push("- [x] **RDNA2 -> SM 6.6 DXIL (`dxcompiler.dll`):** 4/4 stages (VS/PS/CS/Mesh)".into());
        } else {
            lines.push(format!("- [ ] **RDNA2 -> SM 6.6 DXIL:** {stages_ok}/4 (`{last_err}`)"));
        }
    }
    #[cfg(not(windows))]
    lines.push("- [ ] **RDNA2 -> SM 6.6 DXIL:** skipped (non-Windows)".into());

    let status_word = if passed == total { "PASS" } else { "PARTIAL/FAIL" };
    let summary = format!("PercyScarlett v{ver} Self-Test: {passed}/{total} {status_word}");
    let markdown = format!(
        "### Environment\n- **Version:** `v{ver}`\n- **OS / Arch:** `{win_ver}` / `{arch}` ({cores} threads)\n- **CPU:** `{cpu}`\n- **RAM:** `{ram}`\n- **GPU:** `{gpu_label}`\n\n### Self-Test Results ({passed}/{total})\n{}\n\n### Notes / What happened\n_Add any extra details or screenshots below:_\n",
        lines.join("\n")
    );
    let title = format!("[Tester Report] v{ver} ({passed}/{total} {status_word}) — {gpu_label}");
    let issue_url = format!(
        "https://github.com/rpatty-dev/PercyScarlett/issues/new?title={}&body={}",
        url_encode(&title),
        url_encode(&markdown)
    );

    SelfTestResult {
        passed,
        total,
        summary,
        markdown,
        issue_url,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_prefilled_github_issue_url() {
        let res = run_self_test(Path::new("."));
        assert_eq!(res.total, 8);
        assert!(res.passed >= 3);
        assert!(res.issue_url.starts_with("https://github.com/rpatty-dev/PercyScarlett/issues/new?title="));
        assert!(res.issue_url.contains("&body="));
        assert!(!res.issue_url.contains(' '));
        assert!(res.markdown.contains("**GPU:**"));
        assert!(res.markdown.contains("**RAM:**"));
    }
}
