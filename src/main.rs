mod gdk;
mod gui;
mod mascot;
mod pe64;
mod pe_loader;
mod self_test;
mod shader_bench;
mod xmem;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

// Hint NVIDIA Optimus & AMD Switchable Graphics to select the discrete high-performance GPU.
#[cfg(windows)]
#[no_mangle]
pub static NvOptimusEnablement: u32 = 1;
#[cfg(windows)]
#[no_mangle]
pub static AmdPowerXpressRequestHighPerformance: u32 = 1;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("inspect") if args.len() >= 3 => match fs::read(&args[2]) {
            Ok(data) => match pe64::parse_pe64(&data) {
                Ok(pe) => print!("{}", gdk::summarize_bringup(&pe)),
                Err(e) => eprintln!("PE error: {e}"),
            },
            Err(e) => eprintln!("Read error: {e}"),
        },
        Some("plan") if args.len() >= 3 => {
            let dir = Path::new(&args[2]);
            let cfg = dir.join("MicrosoftGame.Config");
            if cfg.exists() {
                println!("Found {}", cfg.display());
            } else {
                println!("No MicrosoftGame.Config in {}", dir.display());
            }
            let mut pkg_exports: HashMap<String, HashSet<String>> = HashMap::new();
            let mut parsed = Vec::new();
            if let Ok(rd) = fs::read_dir(dir) {
                for ent in rd.flatten() {
                    let p = ent.path();
                    let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("");
                    if ext.eq_ignore_ascii_case("exe") || ext.eq_ignore_ascii_case("dll") {
                        if let Ok(data) = fs::read(&p) {
                            if let Ok(pe) = pe64::parse_pe64(&data) {
                                if ext.eq_ignore_ascii_case("dll") {
                                    if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                                        pkg_exports.insert(
                                            name.to_ascii_lowercase(),
                                            pe.exports.iter().cloned().collect(),
                                        );
                                    }
                                }
                                parsed.push((p, pe));
                            }
                        }
                    }
                }
            }
            for (p, pe) in &parsed {
                println!("=== {} ===", p.display());
                print!("{}", gdk::summarize_bringup_with_pkg(pe, &pkg_exports));
            }
        }
        Some("probe-mem") => {
            let mut pool = xmem::PhysicalPool::default();
            match pool.alloc(16 * 1024 * 1024) {
                Ok(ptr) => println!(
                    "Allocated 16 MiB Scarlett physical test block at 0x{ptr:x} (active={} B, blocks={})",
                    pool.active_bytes(),
                    pool.active_count()
                ),
                Err(e) => eprintln!("Memory probe failed: {e}"),
            }
        }
        Some("shims") => {
            for s in gdk::SHIM_SPECS {
                println!("{:<18} | {:<10} | {:<24} | {}", s.dll, s.area, s.status, s.role);
            }
        }
        Some("verify-dlls") if args.len() >= 3 => {
            verify_package_dlls(Path::new(&args[2]));
        }
        Some("stubgen") if args.len() >= 4 => {
            run_stubgen(Path::new(&args[2]), Path::new(&args[3]));
        }
        Some("report") if args.len() >= 4 => {
            write_bringup_report(Path::new(&args[2]), Path::new(&args[3]));
        }
        Some("launch") if args.len() >= 3 => {
            #[cfg(windows)]
            {
                let exe_path = Path::new(&args[2]);
                let shim_dir = std::env::current_exe()
                    .ok()
                    .and_then(|p| p.parent().map(|d| d.to_path_buf()))
                    .unwrap_or_else(|| Path::new(".").to_path_buf());
                match pe_loader::launch_suspended_with_overlay(exe_path, &shim_dir) {
                    Ok(pid) => println!("Launched {} with PercyScarlett overlay (PID {pid})", exe_path.display()),
                    Err(e) => eprintln!("Launch failed: {e}"),
                }
            }
        }
        Some("run") if args.len() >= 3 => {
            #[cfg(windows)]
            {
                let exe_path = Path::new(&args[2]);
                let dry_run = args.iter().any(|a| a == "--dry-run");
                let shim_dir = std::env::current_exe()
                    .ok()
                    .and_then(|p| p.parent().map(|d| d.to_path_buf()))
                    .unwrap_or_else(|| Path::new(".").to_path_buf());
                match fs::read(exe_path) {
                    Ok(data) => match pe_loader::load_and_run_pe64(&data, &shim_dir, !dry_run) {
                        Ok((st, rc)) => {
                            let verb = if dry_run { "Dry-run mapped" } else { "Executed" };
                            println!(
                                "{verb} {} (base=0x{:x}, entry=0x{:x}, pdata={}, tls_cb={}, tls_bytes={}, relocs={}, resolved={}, stubbed={}) -> rc={rc}",
                                exe_path.display(),
                                st.mapped_base,
                                st.entry_va,
                                st.pdata_entries,
                                st.tls_callbacks,
                                st.static_tls_bytes,
                                st.relocations_applied,
                                st.resolved_imports,
                                st.stubbed_imports
                            );
                            if !st.missing_imports.is_empty() {
                                println!("Missing imports ({}): {}", st.missing_imports.len(), st.missing_imports.join(", "));
                            }
                        }
                        Err(e) => eprintln!("Run failed: {e}"),
                    },
                    Err(e) => eprintln!("Read error: {e}"),
                }
            }
        }
        Some("bench-shaders") if args.len() >= 3 => {
            let p = Path::new(&args[2]);
            let dxil = args.iter().any(|a| a == "--dxil");
            let limit = args
                .windows(2)
                .find(|w| w[0] == "--limit")
                .and_then(|w| w[1].parse::<usize>().ok())
                .unwrap_or(0);
            shader_bench::run_shader_benchmark(p, limit, dxil);
        }
        Some("self-test") => {
            let exe_dir = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.to_path_buf()))
                .unwrap_or_else(|| Path::new(".").to_path_buf());
            let rt_dir = std::env::var_os("LOCALAPPDATA")
                .map(|b| std::path::PathBuf::from(b).join("PercyScarlett").join("runtime"));
            let shim_dir = if exe_dir.join("d3d12_xs.dll").exists() {
                exe_dir
            } else if let Some(rd) = rt_dir.filter(|p| p.join("d3d12_xs.dll").exists()) {
                rd
            } else {
                exe_dir
            };
            let old_path = std::env::var("PATH").unwrap_or_default();
            std::env::set_var("PATH", format!("{};{old_path}", shim_dir.display()));
            let res = self_test::run_self_test(&shim_dir);
            println!("{}\n\n{}", res.summary, res.markdown);
            let _ = fs::write("percyscarlett_test_report.md", &res.markdown);
            println!("Saved report: percyscarlett_test_report.md");
            println!("Pre-filled GitHub Issue URL:\n{}", res.issue_url);
            if args.iter().any(|a| a == "--open") {
                #[cfg(windows)]
                {
                    let _ = std::process::Command::new("cmd")
                        .args(["/C", "start", "", &res.issue_url])
                        .spawn();
                }
            }
        }
        Some("mascot") => {
            mascot::print_mascot_banner();
        }
        None | Some("gui") => {
            #[cfg(windows)]
            {
                extern "system" {
                    fn FreeConsole() -> i32;
                }
                if args.len() <= 1 {
                    unsafe {
                        FreeConsole();
                    }
                }
            }
            if let Err(e) = gui::run_dashboard() {
                eprintln!("GUI error: {e}");
            }
        }
        _ => {
            mascot::print_mascot_banner();
            println!("Usage:");
            println!("  percyscarlett                     (Launch Xbox Series X Dashboard UI)");
            println!("  percyscarlett gui                 (Launch Xbox Series X Dashboard UI)");
            println!("  percyscarlett inspect <game.exe>");
            println!("  percyscarlett plan <game_dir>");
            println!("  percyscarlett verify-dlls <game_dir>");
            println!("  percyscarlett stubgen <game.exe> <out_dir>");
            println!("  percyscarlett report <game.exe> <report.json>");
            println!("  percyscarlett run <game.exe> [--dry-run]");
            println!("  percyscarlett self-test [--open]");
            println!("  percyscarlett bench-shaders <shaders.rpf> [--dxil] [--limit N]");
            println!("  percyscarlett probe-mem");
            println!("  percyscarlett shims");
        }
    }
}

fn write_bringup_report(exe_path: &Path, out_json: &Path) {
    let Ok(data) = fs::read(exe_path) else {
        eprintln!("Failed to read {}", exe_path.display());
        return;
    };
    let Ok(pe) = pe64::parse_pe64(&data) else {
        eprintln!("Invalid PE32+: {}", exe_path.display());
        return;
    };
    let mut shims = Vec::new();
    let mut total_funcs = 0usize;
    for imp in &pe.imports {
        total_funcs += imp.functions.len();
        if let Some(s) = gdk::SHIM_SPECS.iter().find(|s| s.dll.eq_ignore_ascii_case(&imp.name)) {
            shims.push(format!(
                "    {{\"dll\":\"{}\",\"area\":\"{}\",\"status\":\"{}\",\"imports\":{}}}",
                imp.name, s.area, s.status, imp.functions.len()
            ));
        }
    }
    let json = format!(
        "{{\n  \"target\": \"{}\",\n  \"machine\": \"0x{:04x}\",\n  \"entry_rva\": \"0x{:x}\",\n  \"import_dlls\": {},\n  \"import_functions\": {},\n  \"matched_shims\": [\n{}\n  ]\n}}\n",
        exe_path.file_name().and_then(|s| s.to_str()).unwrap_or("unknown"),
        pe.machine,
        pe.entry_rva,
        pe.imports.len(),
        total_funcs,
        shims.join(",\n")
    );
    if fs::write(out_json, json).is_ok() {
        println!("Wrote {}", out_json.display());
    }
}

fn verify_package_dlls(dir: &Path) {
    #[cfg(windows)]
    {
        extern "system" {
            fn LoadLibraryW(name: *const u16) -> usize;
            fn FreeLibrary(module: usize) -> i32;
            fn GetLastError() -> u32;
        }
        let shim_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| Path::new(".").to_path_buf());
        let old_path = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", format!("{};{};{old_path}", shim_dir.display(), dir.display()));

        let skip = ["vcruntime140.dll", "vcruntime140_1.dll", "msvcp140.dll", "msvcp140_1.dll", "msvcp140_2.dll", "msvcp140_atomic_wait.dll", "msvcp140_codecvt_ids.dll", "concrt140.dll", "vccorlib140.dll", "vcomp140.dll"];
        let mut ok_count = 0usize;
        let mut fail_count = 0usize;
        if let Ok(rd) = fs::read_dir(dir) {
            for ent in rd.flatten() {
                let p = ent.path();
                if p.extension().and_then(|s| s.to_str()).map(|s| s.eq_ignore_ascii_case("dll")).unwrap_or(false) {
                    let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
                    if skip.iter().any(|s| s.eq_ignore_ascii_case(name)) {
                        continue;
                    }
                    let mut wide: Vec<u16> = p.as_os_str().to_string_lossy().encode_utf16().collect();
                    wide.push(0);
                    let h = unsafe { LoadLibraryW(wide.as_ptr()) };
                    if h != 0 {
                        println!("[PASS] {name} loaded at 0x{h:x}");
                        unsafe { FreeLibrary(h) };
                        ok_count += 1;
                    } else {
                        let err = unsafe { GetLastError() };
                        println!("[FAIL] {name} GetLastError={err}");
                        fail_count += 1;
                    }
                }
            }
        }
        println!("DLL verification: {ok_count} passed, {fail_count} failed (shims from {})", shim_dir.display());
    }
    #[cfg(not(windows))]
    {
        let _ = dir;
        eprintln!("verify-dlls is Windows-only");
    }
}

fn run_stubgen(pe_path: &Path, out_dir: &Path) {
    let Ok(data) = fs::read(pe_path) else {
        eprintln!("Failed to read {}", pe_path.display());
        return;
    };
    let Ok(pe) = pe64::parse_pe64(&data) else {
        eprintln!("Failed to parse PE32+ {}", pe_path.display());
        return;
    };
    let missing = gdk::collect_missing_by_dll(&pe, &HashMap::new());
    if missing.is_empty() {
        println!("No missing DLLs/symbols for {}", pe_path.display());
        return;
    }
    for (dll, funcs) in &missing {
        let dir = out_dir.join(dll.trim_end_matches(".dll").trim_end_matches(".DLL"));
        let _ = fs::create_dir_all(dir.join("src"));
        let (cargo, lib) = gdk::generate_rust_stub_crate(dll, funcs);
        let _ = fs::write(dir.join("Cargo.toml"), cargo);
        let _ = fs::write(dir.join("src/lib.rs"), lib);
        println!("Generated stub crate for {dll} ({} funcs) -> {}", funcs.len(), dir.display());
    }
}
