mod gdk;
mod pe64;
mod xmem;

use std::fs;
use std::path::Path;

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
            if let Ok(rd) = fs::read_dir(dir) {
                for ent in rd.flatten() {
                    let p = ent.path();
                    if p.extension().and_then(|s| s.to_str()).map(|s| s.eq_ignore_ascii_case("exe")).unwrap_or(false) {
                        if let Ok(data) = fs::read(&p) {
                            if let Ok(pe) = pe64::parse_pe64(&data) {
                                println!("=== {} ===", p.display());
                                print!("{}", gdk::summarize_bringup(&pe));
                            }
                        }
                    }
                }
            }
        }
        Some("probe-mem") => {
            let mut pool = xmem::PhysicalPool::default();
            match pool.alloc(16 * 1024 * 1024) {
                Ok(ptr) => println!("Allocated 16 MiB Scarlett physical test block at 0x{ptr:x} (active={} B)", pool.active_bytes()),
                Err(e) => eprintln!("Memory probe failed: {e}"),
            }
        }
        Some("shims") => {
            for s in gdk::SHIM_SPECS {
                println!("{:<18} | {:<10} | {:<24} | {}", s.dll, s.area, s.status, s.role);
            }
        }
        _ => {
            println!("PercyScarlett v0.1.0 — Clean-Room Rust Scarlett Bring-Up Layer");
            println!("Usage:");
            println!("  percyscarlett inspect <game.exe>");
            println!("  percyscarlett plan <game_dir>");
            println!("  percyscarlett probe-mem");
            println!("  percyscarlett shims");
        }
    }
}
