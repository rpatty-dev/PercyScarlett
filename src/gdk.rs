use crate::pe64::Pe64Summary;

pub struct ScarlettShimSpec {
    pub dll: &'static str,
    pub area: &'static str,
    pub role: &'static str,
    pub status: &'static str,
}

pub const SHIM_SPECS: &[ScarlettShimSpec] = &[
    ScarlettShimSpec { dll: "kernelx.dll", area: "Memory/OS", role: "XMemAllocPhysical, 64K/4M aliasing, title memory", status: "Prototype (xmem.rs)" },
    ScarlettShimSpec { dll: "d3d12_xs.dll", area: "Graphics", role: "ID3D12DeviceX, CommandListX, PresentX, DispatchMesh", status: "Planned" },
    ScarlettShimSpec { dll: "xg_xs.dll", area: "Graphics", role: "Scarlett texture/buffer tile swizzle & layout", status: "Paired w/ PercyStar tex.rs" },
    ScarlettShimSpec { dll: "xgameruntime.dll", area: "GDK", role: "XTaskQueue, XAsync, XUser, XGameSave", status: "Planned" },
    ScarlettShimSpec { dll: "dstorage.dll", area: "IO", role: "DirectStorage queue + GDeflate / BCPack decompression", status: "Planned" },
    ScarlettShimSpec { dll: "XAudio2_9.dll", area: "Audio", role: "XAudio2 + XMA2 / Opus voice playback", status: "Planned" },
    ScarlettShimSpec { dll: "GameInput.dll", area: "Input", role: "XInput + keyboard/mouse translation", status: "Planned" },
    ScarlettShimSpec { dll: "winrt_xs.dll", area: "Runtime", role: "WinRT RoInitialize / HSTRING / Xbox factories", status: "Planned" },
];

pub fn summarize_bringup(pe: &Pe64Summary) -> String {
    let mut out = format!(
        "PE32+ machine=0x{:04x} entry=0x{:x} base=0x{:x} subsystem={} sections={} imports={}\n",
        pe.machine, pe.entry_rva, pe.image_base, pe.subsystem, pe.sections.len(), pe.imports.len()
    );
    for imp in &pe.imports {
        let tag = SHIM_SPECS
            .iter()
            .find(|s| s.dll.eq_ignore_ascii_case(&imp.name))
            .map(|s| format!("[SHIM: {} - {}]", s.area, s.status))
            .unwrap_or_else(|| "[HOST WIN32]".to_string());
        out.push_str(&format!("  {} ({} funcs) {tag}\n", imp.name, imp.functions.len()));
    }
    out
}
