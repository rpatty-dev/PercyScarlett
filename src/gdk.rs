use std::collections::{HashMap, HashSet};
use std::ffi::CString;

use crate::pe64::Pe64Summary;

pub struct ScarlettShimSpec {
    pub dll: &'static str,
    pub area: &'static str,
    pub role: &'static str,
    pub status: &'static str,
}

pub const SHIM_SPECS: &[ScarlettShimSpec] = &[
    ScarlettShimSpec { dll: "xmem.dll", area: "Memory", role: "XMemAlloc, XMemFree, XMemVirtualQuery, working set stats", status: "Prototype (xmem.rs)" },
    ScarlettShimSpec { dll: "kernelx.dll", area: "Memory/OS", role: "XMemAllocPhysical, 64K/4M aliasing, title memory", status: "Prototype (xmem.rs)" },
    ScarlettShimSpec { dll: "d3d12_xs.dll", area: "Graphics", role: "ID3D12DeviceX, CommandListX, PresentX, DispatchMesh", status: "Planned" },
    ScarlettShimSpec { dll: "xg_xs.dll", area: "Graphics", role: "Scarlett texture/buffer tile swizzle & layout", status: "Paired w/ PercyStar tex.rs" },
    ScarlettShimSpec { dll: "xgameruntime.dll", area: "GDK", role: "QueryApiImpl, XTaskQueue, XAsync, XUser, XGameSave", status: "Planned" },
    ScarlettShimSpec { dll: "dstorage.dll", area: "IO", role: "DirectStorage queue + GDeflate / BCPack decompression", status: "Planned" },
    ScarlettShimSpec { dll: "XAudio2_9.dll", area: "Audio", role: "XAudio2 + XMA2 / Opus voice playback", status: "Planned" },
    ScarlettShimSpec { dll: "GameInput.dll", area: "Input", role: "XInput + keyboard/mouse translation", status: "Planned" },
    ScarlettShimSpec { dll: "winrt_xs.dll", area: "Runtime", role: "WinRT RoInitialize / HSTRING / Xbox factories", status: "Planned" },
];

pub fn summarize_bringup(pe: &Pe64Summary) -> String {
    summarize_bringup_with_pkg(pe, &HashMap::new())
}

pub fn summarize_bringup_with_pkg(pe: &Pe64Summary, pkg_exports: &HashMap<String, HashSet<String>>) -> String {
    let sec_names: Vec<&str> = pe.sections.iter().map(|s| s.name.as_str()).collect();
    let mut out = format!(
        "PE32+ machine=0x{:04x} entry=0x{:x} base=0x{:x} subsystem={} sections={} [{}] imports={} exports={}\n",
        pe.machine,
        pe.entry_rva,
        pe.image_base,
        pe.subsystem,
        pe.sections.len(),
        sec_names.join(","),
        pe.imports.len(),
        pe.exports.len()
    );

    let mut missing_surface: Vec<String> = Vec::new();
    for imp in &pe.imports {
        let kind = if imp.delay_loaded { " [DELAY]" } else { "" };
        let key = imp.name.to_ascii_lowercase();
        let tag = if let Some(s) = SHIM_SPECS.iter().find(|s| s.dll.eq_ignore_ascii_case(&imp.name)) {
            format!("[SHIM: {} - {}]", s.area, s.status)
        } else if let Some(exports) = pkg_exports.get(&key) {
            let unres: Vec<&str> = imp
                .functions
                .iter()
                .filter(|f| !f.starts_with("ord#") && !exports.contains(f.as_str()))
                .map(String::as_str)
                .collect();
            for f in &unres {
                missing_surface.push(format!("{}!{f}", imp.name));
            }
            if unres.is_empty() {
                "[PACKAGE]".to_string()
            } else {
                format!("[PACKAGE PARTIAL: {} missing]", unres.len())
            }
        } else {
            match check_host_dll(&imp.name, &imp.functions) {
                HostCheck::Resolved => "[HOST WIN32]".to_string(),
                HostCheck::Partial(unres) => {
                    for f in &unres {
                        missing_surface.push(format!("{}!{f}", imp.name));
                    }
                    format!("[HOST PARTIAL: {} missing]", unres.len())
                }
                HostCheck::NotFound => {
                    for f in &imp.functions {
                        missing_surface.push(format!("{}!{f}", imp.name));
                    }
                    "[MISSING]".to_string()
                }
            }
        };
        out.push_str(&format!("  {} ({} funcs){kind} {tag}\n", imp.name, imp.functions.len()));
    }
    if !missing_surface.is_empty() {
        out.push_str(&format!("  -> Missing / unshimmed symbols ({}):\n", missing_surface.len()));
        for sym in missing_surface.iter().take(25) {
            out.push_str(&format!("     - {sym}\n"));
        }
        if missing_surface.len() > 25 {
            out.push_str(&format!("     ... +{} more\n", missing_surface.len() - 25));
        }
    }
    out
}

pub fn collect_missing_by_dll(
    pe: &Pe64Summary,
    pkg_exports: &HashMap<String, HashSet<String>>,
) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    for imp in &pe.imports {
        let key = imp.name.to_ascii_lowercase();
        if SHIM_SPECS.iter().any(|s| s.dll.eq_ignore_ascii_case(&imp.name)) {
            continue;
        }
        if let Some(exports) = pkg_exports.get(&key) {
            let unres: Vec<String> = imp
                .functions
                .iter()
                .filter(|f| !f.starts_with("ord#") && !exports.contains(f.as_str()))
                .cloned()
                .collect();
            if !unres.is_empty() {
                out.push((imp.name.clone(), unres));
            }
        } else {
            match check_host_dll(&imp.name, &imp.functions) {
                HostCheck::Resolved => {}
                HostCheck::Partial(unres) => out.push((imp.name.clone(), unres)),
                HostCheck::NotFound => out.push((imp.name.clone(), imp.functions.clone())),
            }
        }
    }
    out
}

pub fn generate_rust_stub_crate(dll_name: &str, funcs: &[String]) -> (String, String) {
    let stem = dll_name.strip_suffix(".dll").or_else(|| dll_name.strip_suffix(".DLL")).unwrap_or(dll_name);
    let crate_name = stem.to_ascii_lowercase().replace(|c: char| !c.is_ascii_alphanumeric() && c != '_', "_");
    let cargo = format!(
        "[package]\nname = \"{crate_name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\nname = \"{crate_name}\"\ncrate-type = [\"cdylib\"]\n"
    );
    let mut lib = String::from(
        "use std::fs::OpenOptions;\nuse std::io::Write;\n\nfn log_stub(name: &str, a0: usize, a1: usize, a2: usize, a3: usize) {\n    if let Ok(p) = std::env::var(\"PERCY_STUB_LOG\") {\n        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(p) {\n            let _ = writeln!(f, \"{name}(0x{a0:x}, 0x{a1:x}, 0x{a2:x}, 0x{a3:x})\");\n        }\n    }\n    if let Ok(ab) = std::env::var(\"PERCY_STUB_ABORT\") {\n        if !ab.is_empty() && name.contains(&ab) {\n            std::process::abort();\n        }\n    }\n}\n\n"
    );
    for f in funcs {
        if f.starts_with("ord#") {
            continue;
        }
        lib.push_str(&format!(
            "#[no_mangle]\npub unsafe extern \"system\" fn {f}(a0: usize, a1: usize, a2: usize, a3: usize) -> usize {{\n    log_stub(\"{dll_name}!{f}\", a0, a1, a2, a3);\n    0\n}}\n\n"
        ));
    }
    (cargo, lib)
}

enum HostCheck {
    Resolved,
    Partial(Vec<String>),
    NotFound,
}

#[cfg(windows)]
fn check_host_dll(dll: &str, funcs: &[String]) -> HostCheck {
    extern "system" {
        fn LoadLibraryExA(name: *const i8, file: usize, flags: u32) -> usize;
        fn FreeLibrary(module: usize) -> i32;
        fn GetProcAddress(module: usize, name: *const i8) -> usize;
    }
    const DONT_RESOLVE_DLL_REFERENCES: u32 = 0x0000_0001;
    let Ok(c_dll) = CString::new(dll) else {
        return HostCheck::NotFound;
    };
    let hmod = unsafe { LoadLibraryExA(c_dll.as_ptr(), 0, DONT_RESOLVE_DLL_REFERENCES) };
    if hmod == 0 {
        return HostCheck::NotFound;
    }
    let mut missing = Vec::new();
    for f in funcs {
        if f.starts_with("ord#") {
            continue;
        }
        if let Ok(c_fn) = CString::new(f.as_str()) {
            let addr = unsafe { GetProcAddress(hmod, c_fn.as_ptr()) };
            if addr == 0 {
                missing.push(f.clone());
            }
        }
    }
    unsafe { FreeLibrary(hmod) };
    if missing.is_empty() {
        HostCheck::Resolved
    } else {
        HostCheck::Partial(missing)
    }
}

#[cfg(not(windows))]
fn check_host_dll(_dll: &str, _funcs: &[String]) -> HostCheck {
    HostCheck::NotFound
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_kernel32_gettickcount_on_windows() {
        if cfg!(windows) {
            assert!(matches!(
                check_host_dll("kernel32.dll", &["GetTickCount64".into()]),
                HostCheck::Resolved
            ));
            assert!(matches!(
                check_host_dll("definitely_not_a_real_scarlett_dll.dll", &["Foo".into()]),
                HostCheck::NotFound
            ));
        }
    }
}
