#[cfg(windows)]
#[test]
fn scarlett_guest_exe_end_to_end_bringup() {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let exe_path = PathBuf::from(env!("CARGO_BIN_EXE_percyscarlett"));
    let target_dir = exe_path.parent().expect("bin parent");

    let is_release = target_dir
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s == "release")
        .unwrap_or(false);
    let mut build_args = vec!["build", "-p", "xmem", "-p", "d3d12_xs"];
    if is_release {
        build_args.push("--release");
    }
    let status = Command::new("cargo")
        .args(&build_args)
        .current_dir(&manifest_dir)
        .status()
        .expect("cargo build -p xmem -p d3d12_xs");
    assert!(status.success(), "cargo build -p xmem -p d3d12_xs failed");

    let xmem_dll = target_dir.join("xmem.dll");
    let d3d12_dll = target_dir.join("d3d12_xs.dll");
    assert!(xmem_dll.exists(), "missing {}", xmem_dll.display());
    assert!(d3d12_dll.exists(), "missing {}", d3d12_dll.display());

    let work = std::env::temp_dir().join(format!("percyscarlett_e2e_{}", std::process::id()));
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work).unwrap();

    // Stage compiled shim DLLs into isolated guest directory.
    fs::copy(&xmem_dll, work.join("xmem.dll")).unwrap();
    fs::copy(&d3d12_dll, work.join("d3d12_xs.dll")).unwrap();

    // Synthesize a real PE32+ executable importing xmem.dll + d3d12_xs.dll.
    let guest_src = work.join("scarlett_guest.rs");
    let guest_exe = work.join("scarlett_guest.exe");
    fs::write(
        &guest_src,
        r#"
#[link(name = "xmem", kind = "dylib")]
extern "system" {
    fn XMemAlloc(size: usize, attributes: u64) -> *mut core::ffi::c_void;
    fn XMemFree(ptr: *mut core::ffi::c_void, attributes: u64);
    fn XMemGetWorkingSetStatistics(stats: *mut [u64; 8]) -> i32;
}

#[link(name = "d3d12_xs", kind = "dylib")]
extern "system" {
    fn D3D12SerializeRootSignature(
        desc: *const RootSigDesc,
        version: u32,
        blob: *mut usize,
        err: *mut usize,
    ) -> i32;
    fn D3D12CreateDevice(
        adapter: usize,
        min_fl: u32,
        riid: *const [u8; 16],
        out_dev: *mut usize,
    ) -> i32;
}

#[repr(C)]
struct RootSigDesc {
    num_params: u32,
    _pad0: u32,
    params: usize,
    num_samplers: u32,
    _pad1: u32,
    samplers: usize,
    flags: u32,
    _pad2: u32,
}

fn main() {
    unsafe {
        let mut stats = [0u64; 8];
        assert_eq!(XMemGetWorkingSetStatistics(&mut stats), 1);
        assert!(stats[1] > 0 && stats[2] >= stats[1]);

        let p = XMemAlloc(64 * 1024, 0x10);
        assert!(!p.is_null());
        core::ptr::write_bytes(p as *mut u8, 0xAB, 4096);
        XMemFree(p, 0x10);

        // Empty root signature with Scarlett-specific upper flag bit (0x0100_0000)
        // that d3d12_xs must mask before forwarding to host d3d12.dll.
        let desc = RootSigDesc {
            num_params: 0,
            _pad0: 0,
            params: 0,
            num_samplers: 0,
            _pad1: 0,
            samplers: 0,
            flags: 0x0100_0000,
            _pad2: 0,
        };
        let mut blob = 0usize;
        let mut err = 0usize;
        let hr = D3D12SerializeRootSignature(&desc, 1, &mut blob, &mut err);
        assert_eq!(hr, 0, "D3D12SerializeRootSignature failed: 0x{hr:08x}");
        assert_ne!(blob, 0);

        // Probe D3D12CreateDevice forwarding (IID_ID3D12Device) with null out_dev to verify forwarding != E_NOTIMPL.
        let iid_device: [u8; 16] = [
            0xEB, 0x6F, 0x98, 0x18, 0x1B, 0x4A, 0x62, 0x4C,
            0xBA, 0x52, 0xD7, 0x19, 0x1D, 0xAB, 0x2A, 0xEF,
        ];
        let hr_dev = D3D12CreateDevice(0, 0xB000, &iid_device, core::ptr::null_mut());
        assert_ne!(hr_dev, 0x8000_4001u32 as i32, "D3D12CreateDevice must forward, not E_NOTIMPL");
    }
}
"#,
    )
    .unwrap();

    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let compile = Command::new(&rustc)
        .args([
            "--edition",
            "2021",
            "-L",
            target_dir.to_str().unwrap(),
            guest_src.to_str().unwrap(),
            "-o",
            guest_exe.to_str().unwrap(),
        ])
        .output()
        .expect("run rustc for synthetic guest");
    assert!(
        compile.status.success(),
        "rustc failed:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );

    // 1. Inspect synthetic PE32+ executable
    let inspect = Command::new(&exe_path)
        .args(["inspect", guest_exe.to_str().unwrap()])
        .output()
        .expect("percyscarlett inspect");
    assert!(inspect.status.success());
    let inspect_out = String::from_utf8_lossy(&inspect.stdout);
    assert!(inspect_out.contains("xmem.dll"), "inspect missing xmem.dll:\n{inspect_out}");
    assert!(inspect_out.contains("d3d12_xs.dll"), "inspect missing d3d12_xs.dll:\n{inspect_out}");

    // 2. Auto-stubgen check
    let stubs_dir = work.join("stubs");
    let stubgen = Command::new(&exe_path)
        .args(["stubgen", guest_exe.to_str().unwrap(), stubs_dir.to_str().unwrap()])
        .output()
        .expect("percyscarlett stubgen");
    assert!(stubgen.status.success());

    // 3. Execute guest PE32+ binary against compiled xmem.dll + d3d12_xs.dll
    let xmem_log = work.join("xmem_calls.log");
    let run = Command::new(&guest_exe)
        .current_dir(&work)
        .env("PERCY_XMEM_LOG", &xmem_log)
        .output()
        .expect("run scarlett_guest.exe");
    assert!(
        run.status.success(),
        "scarlett_guest.exe failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );

    let log_text = fs::read_to_string(&xmem_log).expect("read PERCY_XMEM_LOG");
    assert!(log_text.contains("XMemGetWorkingSetStatistics"));
    assert!(log_text.contains("XMemAlloc(size=0x10000"));
    assert!(log_text.contains("XMemFree("));

    let _ = fs::remove_dir_all(&work);
    let _ = Path::new("");
}
