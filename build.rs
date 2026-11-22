use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=assets/app.rc");
    println!("cargo:rerun-if-changed=assets/mascot.ico");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_env == "gnu" {
        let obj = out_dir.join("app_res.o");
        if let Ok(st) = Command::new("windres")
            .args(["assets/app.rc", "-O", "coff", "-o"])
            .arg(&obj)
            .status()
        {
            if st.success() {
                println!("cargo:rustc-link-arg={}", obj.display());
            }
        }
    }
}
