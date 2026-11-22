# Codename G6 — Technical Proof & Bring-Up Architecture

This document records empirical verification from PercyScarlett's PE32+ scanner and runtime probes against an unencrypted **Codename G6** Xbox Series X|S (Scarlett) GDK build, demonstrating why the build can execute natively on Windows x64 once the missing main executable is paired with PercyScarlett's GDK shim DLLs.

---

## 1. Proof of Native x86-64 PE32+ Compatibility

Scanning the 16 shipped DLLs and `MicrosoftGame.Config` in the Codename G6 build with `percyscarlett plan` confirms:

1. **Standard Windows PE32+ Format (`0x8664`):**
   Every binary uses standard `PE32+` (`machine = 0x8664`, `base = 0x180000000`) with normal `.text`, `.rdata`, `.data`, `.pdata`, `.reloc` sections (plus the `.xbld` marker on Xbox-built modules). **No CPU instruction translation or hypervisor is required** — Windows x64 executes the `.text` code natively.
2. **Host Win32 & MSVC CRT Resolution:**
   All 16 sidecar DLLs link against standard Windows API sets (`api-ms-win-core-*`, `api-ms-win-crt-*`, `WINHTTP.dll`, `CRYPT32.dll`) and standard MSVC 140 runtime DLLs (`vcruntime140.dll`, `msvcp140.dll`, `concrt140.dll`, `vcomp140.dll`, `vccorlib140.dll`) that already run on Windows 10/11 x64.
3. **Minimal GDK Surface Across Sidecar Modules:**
   Across all 16 sidecar DLLs, only **3 Xbox/GDK shim DLLs** are imported (9 total function imports):

| Shipped Module | Section Profile | Imports / Exports | Required PercyScarlett Shims |
| :--- | :--- | :--- | :--- |
| `amd_fidelityfx_upscaler_xbox.dll` | 7 sections (incl. `.xbld`) | 24 imports / 5 exports | `d3d12_xs.dll` (1), `xmem.dll` (3) |
| `Bink2Scarlett.dll` | 6 sections (incl. `.xbld`) | 20 imports / 74 exports | `xmem.dll` (2), `XAudio2_9.dll` (1) |
| `XboxBvhBuilder.dll` | 7 sections (incl. `.xbld`) | 21 imports / 12 exports | `d3d12_xs.dll` (1), `xmem.dll` (1) |
| `amd_fidelityfx_loader_xbox.dll` | 6 sections | 9 imports / 5 exports | Host Win32 + MSVC only |
| `libHttpClient.GDK.dll` | 12 sections (incl. `.xbld`) | 14 imports / 129 exports | Host Win32 + MSVC only |
| `XCurl.dll` | 7 sections (incl. `.xbld`) | 33 imports / 61 exports | Host Win32 (`WINHTTP`, `CRYPT32`) only |
| `concrt140.dll`, `msvcp140*.dll`, `vcruntime140*.dll`, `vccorlib140.dll`, `vcomp140.dll` | 6–7 sections | Standard CRT | Host Win32 only |

---

## 2. Verified Runtime Primitives & Live DLL Loading (Windows 11 x64)

- **Live Windows `LoadLibraryW` Verification (`percyscarlett verify-dlls`):**
  All 6 shipped non-CRT Xbox/Scarlett sidecar DLLs in the Codename G6 build load cleanly on Windows 11 x64 using PercyScarlett's compiled `xmem.dll` and `d3d12_xs.dll` shims + host `System32`:
  ```text
  [PASS] amd_fidelityfx_loader_xbox.dll loaded at 0x7ffbfa7b0000
  [PASS] amd_fidelityfx_upscaler_xbox.dll loaded at 0x7ffb89170000
  [PASS] Bink2Scarlett.dll loaded at 0x7ffba2b80000
  [PASS] libHttpClient.GDK.dll loaded at 0x7ffb89140000
  [PASS] XboxBvhBuilder.dll loaded at 0x7ffb89160000
  [PASS] XCurl.dll loaded at 0x7ffbb1ee0000
  DLL verification: 6 passed, 0 failed
  ```
- **Compiled `xmem.dll` (`crates/xmem`):**
  Exports `XMemAlloc`, `XMemFree`, `XMemAllocDefault`, `XMemFreeDefault`, `XMemAllocPhysical`, `XMemFreePhysical`, `XMemGetWorkingSetStatistics`, and `XMemVirtualQuery` backed by Win32 `VirtualAlloc` / `VirtualFree` / `VirtualQueryEx` (with `PERCY_XMEM_LOG` call tracing).
- **Compiled `d3d12_xs.dll` (`crates/d3d12_xs`):**
  Exports `D3D12CreateDevice`, `D3D12SerializeRootSignature`, `D3D12SerializeVersionedRootSignature`, `D3D12CreateRootSignatureDeserializer`, `D3D12CreateVersionedRootSignatureDeserializer`, `D3D12GetDebugInterface`, and `D3D12EnableExperimentalFeatures` forwarding to host `d3d12.dll` (masking Scarlett-specific root signature flags cleanly).
- **Auto-Stub Generator (`percyscarlett stubgen <exe> <out_dir>`):**
  Automatically generates compilable Rust `cdylib` shim crates for any remaining GDK DLL imports discovered when the main `.exe` drops.

---

## 3. How Codename G6 Boots When the `.exe` Drops

```
  Codename G6 (.exe + 16 DLLs + MicrosoftGame.Config)
        │
        ├──► Standard Win32 / API Sets / WINHTTP / CRYPT32 ──► Host Windows 10/11 x64
        ├──► MSVC 140 Runtime DLLs                         ──► Packaged / Host CRT
        └──► Xbox / Scarlett GDK Imports (IAT)             ──► PercyScarlett Shims (Overlay Mount)
                 ├── xmem.dll / kernelx.dll  ──► Win32 VirtualAlloc Physical Pool (xmem.rs)
                 ├── d3d12_xs.dll / xg_xs.dll──► PC D3D12 Proxy + Tile Swizzle Bridge
                 ├── xgameruntime.dll        ──► XTaskQueue / XAsync / XUser / XGameSave
                 ├── dstorage.dll            ──► DirectStorage + GDeflate / BCPack
                 ├── XAudio2_9.dll           ──► PC XAudio2 / WASAPI Voice Sink
                 ├── GameInput.dll           ──► XInput / RawInput Translation
                 └── winrt_xs.dll            ──► RoInitialize / HSTRING Activation
```

### Day-1 Bring-Up Sequence
1. Place the unencrypted Codename G6 `.exe` into the source build directory and run the non-destructive overlay mount (junctions for asset folders, hardlinks for root binaries).
2. Run `percyscarlett inspect <MountDir>\<CodenameG6.exe>` to enumerate every exact GDK import ordinal and function name required by the executable.
3. Drop the compiled PercyScarlett shim DLLs (`xmem.dll`, `d3d12_xs.dll`, `xgameruntime.dll`, etc.) into the mount directory so the Windows PE loader binds the `.exe` IAT directly to PercyScarlett without touching the original build files.
