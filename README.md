<p align="center">
  <img src="assets/mascot.png" width="210" alt="PercyScarlett Harlequin Bunny Xbox Mascot" />
</p>

<h1 align="center">PercyScarlett</h1>

<p align="center">
  <a href="https://rpatty-dev.github.io/PercyScarlett/"><strong>Live Proof & Benchmark Site (GitHub Pages)</strong></a> ·
  <a href="docs/CODENAME_G6_PROOF.md"><strong>Technical Bring-Up Architecture</strong></a> ·
  <a href="https://github.com/sponsors/rpatty-dev"><strong>♥ Support / GitHub Sponsors</strong></a>
</p>

**Clean-room Rust translation layer, GDK shim suite, and PE32+ inspection toolkit for running unencrypted Xbox Series X|S (Scarlett) executables natively on Windows x64 — built specifically for TheBig6 😉.**

Pure Rust · Zero third-party emulator code · Zero proprietary SDK headers · `0 GB` duplicate disk overhead.

---

## How to Test (Takes 10 Seconds — No Game Files Needed!)

> **Want to help test PercyScarlett on your PC? Just do this:**
>
> 1. **Download & double-click [`Run_SelfTest_And_Report.bat`](https://github.com/rpatty-dev/PercyScarlett/releases/latest/download/Run_SelfTest_And_Report.bat)**
> 2. **Your browser will open automatically** with your test results already typed in.
> 3. **Click the green `Submit new issue` button on GitHub.** That's it!

*(Or in the app: open `percyscarlett.exe` -> click **Run Self-Test** -> click **Submit GitHub Issue**.)*

---

## Live Status & Surface Coverage

![PercyScarlett Live Subsystem Readiness](docs/status.svg)

![PercyScarlett Per-Item Surface Coverage Map](docs/progress_grid.svg)

---

## Why PercyScarlett

| Capability | PercyScarlett | Offline Recompilers |
| :--- | :--- | :--- |
| **Disk Overhead** | **0 GB** (NTFS junctions + hardlinks) | ~350 GB extracted intermediates |
| **CPU Execution** | **Native x86-64** (`machine=0x8664`) | Native / static recompile |
| **Shipped Sidecar DLLs** | **6/6 live-loaded on Win11 (`LoadLibraryW`)** | Untested |
| **Compiled Shim DLLs** | **9 Rust `cdylib` DLLs** (`28/28` tests passing, `0` warnings) | Skeleton / Incomplete |
| **PE32+ Loader** | **In-process `.reloc`, sanitized `.pdata`, static `.tls` (`gs:[0x58]`), TLS callbacks & missing-import audit** | Basic mapper |
| **Unmodified Scarlett Samples** | **6/6 Scarlett GDKX samples booted & running live** (`SimpleCompute`, `SimpleTriangle`, `SimpleLighting`, `SimpleTexture`, `SimpleBezier`, `SimpleInstancing`) | Untested / Partial |
| **Shader Recompiler** | **Strict clean-room RDNA2 -> HLSL (fails on unknown ops) -> SM 6.6 DXIL (`dxcompiler.dll`)** (`489/500` = **97.8%** on `--limit 500`; `3,060/21,522` full corpus; **2/2 hardware GPU compute reference tests bit-exact**) | Partial / External |
| **System Tray (`-headless`)** | **`percyscarlett -headless` detaches console & lives in Windows System Tray** (`Shell_NotifyIconW`) | CLI only |
| **Day-1 Crash Telemetry** | **Built-in VEH fault logger** + `PERCY_XMEM_LOG` + `PERCY_D3D12_LOG` + `report.json` | External debugger required |
| **Xbox VFS & Auto-Stubs** | **`G:/T:/S:` rewrite** + `percyscarlett stubgen` | Manual |

---

## Verified Proof Against TheBig6 😉

> Full binary breakdown & IAT routing: **[docs/CODENAME_G6_PROOF.md](docs/CODENAME_G6_PROOF.md)**

1. **6/6 Shipped Xbox Sidecar DLLs Load Live on Windows 11 (`percyscarlett verify-dlls`)**
   ```text
   [PASS] amd_fidelityfx_loader_xbox.dll loaded at 0x7ffc14430000
   [PASS] amd_fidelityfx_upscaler_xbox.dll loaded at 0x7ffbfb310000
   [PASS] Bink2Scarlett.dll loaded at 0x7ffc143f0000
   [PASS] libHttpClient.GDK.dll loaded at 0x7ffb95710000
   [PASS] XboxBvhBuilder.dll loaded at 0x7ffbfb300000
   [PASS] XCurl.dll loaded at 0x7ffc14410000
   DLL verification: 6 passed, 0 failed
   ```
2. **End-to-End PE32+ Guest & Hardware Bring-Up (`tests/e2e_bringup.rs` + `src/pe_loader.rs`)**
   Compiles and executes a real PE32+ binary (`scarlett_guest.exe`) dynamically linked against all **9** shim DLLs (`xmem`, `d3d12_xs`, `winrt_xs`, `xgameruntime`, `dstorage`, `GameInput`, `kernelx`, `xg_xs`, `xaudio2_9`), validating real hardware `D3D12XboxCreateDevice`, `WaitForOriginX` + `PresentX`, `XAudio2Create`, `XMemAlloc*`, `D3D12SerializeRootSignature` (Scarlett flag masking), `HSTRING`, `XTaskQueue` / `XUser`, `DStorageGetFactory`, `GameInputCreate`, `XGCreateTexture2DComputer`, plus in-process PE32+ mapping with `.reloc`, `.pdata` (`RtlAddFunctionTable`), and TLS callbacks (`percyscarlett run`).
3. **Physical Memory Pool (`percyscarlett probe-mem`)**
   ```text
   Allocated 16 MiB Scarlett physical test block at 0x1eb59000000 (active=16777216 B, blocks=1)
   ```
4. **Strict RDNA2 → SM 6.6 DXIL & Hardware GPU Compute Reference (`d3d12_xs::rdna2_hlsl` + `compute_exec.rs`)**
   Zero silent fallbacks: `lift_to_hlsl_strict` rejects any unrecognized instruction (`strict_lift_rejects_unknown_opcodes`). Decodes GFX10.3 + Scarlett GFX10.1 (`VOP1`/`VOP2`/`VOPC`/`VOP3`/`VOP3P`/`VINTRP`/`SOP1`/`SOP2`/`SOPK`/`SOPC`/`SOPP`/`SMEM`/`DS`/`EXP`/`MUBUF`) and verifies real D3D12 hardware GPU compute execution (`Dispatch`) bit-for-bit against CPU reference:
   ```text
   PS09 Shaders (full corpus): total=21522, strict_lifted=3060/21522 (14.2%) in 13.55s
   Stages: VS=1345 PS=8710 CS=3905 Mesh=7498 RT=64
   DXIL SM 6.6 (dxcompiler.dll, --limit 500): ok=489/500 (97.8%), fail=11
   test compute_exec::tests::executes_lifted_rdna2_compute_shader_on_hardware_gpu_and_matches_reference ... ok
   test compute_exec::tests::executes_rdna2_loop_with_sopp_branch_on_hardware_gpu ... ok
   ```
5. **6/6 Unmodified Scarlett GDKX Samples + `SimpleTriangleDesktop.exe` Rendered Live (`percyscarlett run`)**
   With `DeviceXProxy` x64 `ID3D12Device` forwarders, `DxgiDeviceProxy` (`IDXGIDevice`/`1`/`2`), `XSystemGetDeviceType() = 7` (Xbox Series X), `XGameRuntimeInitialize`, `RegisterAppStateChangeNotification`, and `pe_loader` `GetModuleFileNameW/A` + `GetModuleHandleW/A` virtualization:
   ```text
   SimpleCompute_GDKX.exe: BOOTED & RUNNING
   SimpleTriangle_GDKX.exe: BOOTED & RUNNING
   SimpleLighting_GDKX.exe: BOOTED & RUNNING
   SimpleTexture_GDKX.exe: BOOTED & RUNNING
   SimpleBezier_GDKX.exe: BOOTED & RUNNING
   SimpleInstancing_GDKX.exe: BOOTED & RUNNING
   SimpleTriangleDesktop.exe: 1296x759 D3D12 window rendered live (percyscarlett run, 0 crashes)
   ```
   <p align="center">
     <img src="docs/assets/simple_triangle_desktop.png" width="760" alt="SimpleTriangleDesktop.exe rendered live via percyscarlett run" />
   </p>
   Sample lifted Mesh shader:
   ```hlsl
   // PercyScarlett RDNA2 -> HLSL (ms, 5 insns, 1 exports)
   s[0] = g_cbuf.Load(16u);
   s[1] = g_cbuf.Load(20u);
   v[1] = 0x3F800000u;
   v[2] = asuint(asfloat(v[1]) + asfloat(v[0]));
   /* exp_prim v[0,0,0] */
   // s_endpgm
   ```

5. **Live Hardware GDK Sample Execution ([samples/scarlett_sample.rs](samples/scarlett_sample.rs))**
   Compiles and launches a clean-room GDK sample executable against PercyScarlett's compiled DLLs via `percyscarlett launch`, creating a real hardware `ID3D12Device` + `ID3D12CommandQueue` + Win32 `HWND` + `IDXGISwapChain1`, compiling RDNA2 VS/PS to SM 6.6 DXIL live, initializing `XAudio2Create`, and presenting 10 DXGI frames via `PresentX`:
   ```text
   [ScarlettSample] XSystemGetDeviceType = 4
   [ScarlettSample] XAudio2Create OK (0x1f29576c3e0)
   [ScarlettSample] D3D12XboxCreateDevice OK (0x1f2957ecd00)
   [ScarlettSample] ID3D12CommandQueue created (0x1f2979be090)
   [ScarlettSample] Auto-HWND + IDXGISwapChain1 created!
   [ScarlettSample] RDNA2->VS_6_6 DXIL compiled (7132 bytes)
   [ScarlettSample] RDNA2->PS_6_6 DXIL compiled (4592 bytes)
   [ScarlettSample] Presented 10 real DXGI frames via PresentX!
   Launched target\release\scarlett_sample.exe with PercyScarlett overlay (PID 15256)
   ```

<details>
<summary>Full Verbatim <code>cargo test --workspace</code> Output (17/17 Passing, 0 Warnings)</summary>

```text
     Running unittests src\lib.rs (target\debug\deps\d3d12_xs.exe)
running 5 tests
test tests::device_x_proxy_wraps_and_releases_cleanly ... ok
test rdna2_hlsl::tests::decodes_rdna2_alu_smem_lds_and_detects_mesh_stage ... ok
test tests::resolves_host_d3d12_exports_on_windows ... ok
test tests::compiles_rdna2_to_sm66_dxil_and_presents_frames ... ok
test tests::disk_shader_cache_roundtrip ... ok
test result: ok. 5 passed; 0 failed

     Running unittests src\lib.rs (target\debug\deps\dstorage.exe)
test tests::dstorage_factory_and_queue_memory_copy_work ... ok

     Running unittests src\lib.rs (target\debug\deps\GameInput.exe)
test tests::gameinput_create_and_reading_work ... ok

     Running unittests src\lib.rs (target\debug\deps\kernelx.exe)
test tests::title_memory_status_works ... ok

     Running unittests src\main.rs (target\debug\deps\percyscarlett.exe)
running 4 tests
test pe64::tests::parses_minimal_pe64_header ... ok
test pe_loader::tests::maps_relocates_registers_pdata_and_executes_pe64_in_process ... ok
test xmem::tests::allocates_and_frees_physical_pool_blocks ... ok
test gdk::tests::resolves_kernel32_gettickcount_on_windows ... ok

     Running tests\e2e_bringup.rs
test scarlett_guest_exe_end_to_end_bringup ... ok

     Running unittests src\lib.rs (target\debug\deps\winrt_xs.exe)
test tests::hstring_create_read_delete_roundtrip ... ok

     Running unittests src\lib.rs (target\debug\deps\xaudio2_9.exe)
test tests::xaudio2_create_mastering_and_source_voice_work ... ok

     Running unittests src\lib.rs (target\debug\deps\xg_xs.exe)
test tests::format_block_info_and_computer_work ... ok

     Running unittests src\lib.rs (target\debug\deps\xgameruntime.exe)
test tests::task_queue_and_xuser_work ... ok

     Running unittests src\lib.rs (target\debug\deps\xmem.exe)
test tests::xmem_alloc_free_and_stats_work ... ok
```

</details>

---

## Workspace Crates

| Crate | Output | Implemented Surface |
| :--- | :--- | :--- |
| Root (`src/`) | `percyscarlett.exe` | PE32+ parser, in-process PE32+ loader (`run`), `bench-shaders`, `inspect`, `plan`, `verify-dlls`, `stubgen`, `probe-mem`, `shims` |
| `crates/xmem` | `xmem.dll` | `XMemAlloc*`, `XMemFree*`, `XMemVirtualQuery`, working set stats, VEH crash logger |
| `crates/kernelx` | `kernelx.dll` | `XMemAlloc*`, `XMemGetAuxiliaryTitleMemory`, `AllocateTitlePhysicalPages`, `TitleMemoryStatus`, `SetThreadName` |
| `crates/d3d12_xs` | `d3d12_xs.dll` | `D3D12XboxCreateDevice`, `D3D12CreateDevice`, `PresentX`/`WaitForOriginX`/`RegisterFrameEventX`, `CommandQueueXProxy`, SM 6.6 DXIL (`dxcompiler.dll`) + DXBC, root sig flag masking |
| `crates/xg_xs` | `xg_xs.dll` | `XGGetFormatBlockInfo`, `XGComputeTexture2DLayout`, `XGTileTexture2D`, `XGUntileTexture2D`, `XGCreateTexture2DComputer` |
| `crates/xaudio2_9` | `xaudio2_9.dll` | `XAudio2Create`, `XAudio2CreateWithVersionInfo`, `IXAudio2`, `IXAudio2MasteringVoice`, `IXAudio2SourceVoice` |
| `crates/xgameruntime` | `xgameruntime.dll` | `XTaskQueue*`, `XAsync*`, `XUser*`, `Xbl*` offline auth, `XGameSaveInitializeProvider` |
| `crates/dstorage` | `dstorage.dll` | `DStorageGetFactory` (`IDStorageFactoryX` / `QueueX` / `FileX`), `G:/T:/S:` VFS rewrite |
| `crates/winrt_xs` | `winrt_xs.dll` | `RoInitialize`, `RoUninitialize`, `RoGetActivationFactory`, `Windows*String` (`HSTRING`) |
| `crates/gameinput` | `GameInput.dll` | `GameInputCreate` |

*(Extended console audio/OS recognition for `chad.dll`, `xapu.dll`, `AcpHal.dll`, and `xcrdapi.dll` is built into `src/gdk.rs` + `stubgen`.)*

---

## Quick Start (Windows 10/11 x64)

### Build
```powershell
git clone https://github.com/rpatty-dev/PercyScarlett.git
cd PercyScarlett
cargo build --release --workspace
```
*(If using `x86_64-pc-windows-gnu`, ensure `mingw64\bin` is on `PATH`.)*

### Day-1 Bring-Up
```powershell
# 1. Inspect executable imports & auto-generate any missing shim stubs
.\target\release\percyscarlett.exe inspect "C:\path\to\game.exe"
.\target\release\percyscarlett.exe stubgen "C:\path\to\game.exe" ".\generated_stubs"

# 2. Verify all build DLLs against staged target\release\*.dll
.\target\release\percyscarlett.exe plan "C:\path\to\build_dir"
.\target\release\percyscarlett.exe verify-dlls "C:\path\to\build_dir"
```

### Non-Destructive Overlay Mount
1. Create a mount directory (e.g. `D:\PercyScarlett_Mount`).
2. Junction (`mklink /J`) large asset subdirectories and hardlink (`mklink /H`) root files.
3. Copy `target\release\*.dll` into the mount directory, optionally set `PERCY_VFS_G`, `PERCY_XMEM_LOG`, `PERCY_CRASH_LOG`, `PERCY_D3D12_WRAP=1`, and `PERCY_D3D12_LOG`, and launch.

---

## Tests (`22/22 passing`)

```powershell
cargo test --workspace
```

- `e2e_bringup` (1): Synthetic PE32+ `scarlett_guest.exe` build, `inspect`, `stubgen`, and live execution across all **9** shim DLLs (including real hardware `D3D12XboxCreateDevice`, `WaitForOriginX`, `PresentX`, `XAudio2Create`)
- `percyscarlett` (7): Dashboard semver + PowerShell path quoting, PE32+ parser, in-process PE32+ mapper + sanitized `.pdata` `RtlAddFunctionTable` + static `.tls` (`gs:[0x58]`) + missing-import audit (`pe_loader`), Win32 `VirtualAlloc` pool, live `kernel32.dll` forwarding
- `d3d12_xs` (6): Host `d3d12.dll` symbol forwarding, `ID3D12DeviceX` COM wrapper, RDNA2 ALU/SMEM/LDS/Mesh + `SOPP` CFG branches + Wave64 intrinsics + `s_code_end` lifter, live `d3dcompiler_47.dll` `DXBC` compilation, and live `dxcompiler.dll` SM 6.6 `DXIL` compilation (`VS`/`PS`/`CS`/`Mesh`/`RT`) + `PresentX`/`CommandQueueXProxy`
- `xmem` (1), `kernelx` (1), `xg_xs` (1), `xaudio2_9` (1), `xgameruntime` (1), `dstorage` (1), `winrt_xs` (1), `GameInput` (1): C-ABI exports, COM vtables, `HSTRING`, VFS rewrite, `XGameSave` disk persistence, KBM->Gamepad `GameInput`, `XAudio2Create` voices, and offline GDK runtime checks

---

## Legal Notice

PercyScarlett is an independent, clean-room interoperability research project licensed under the **[PercyLicense (Non-Commercial Attribution & Copyleft v1.2)](LICENSE)**.
It contains no proprietary Xbox/Microsoft SDK headers, keys, or binaries, and does **not** decrypt, bypass, or strip retail DRM or encrypted `.xvc`/`.xvd` packages. Users must supply their own lawfully obtained, unencrypted executables.

The Dashboard font **PercyPRO** is Inter Display 4.1 by The Inter Project Authors, renamed and bundled under the [SIL Open Font License 1.1](assets/fonts/OFL.txt). The PercyLicense does not apply to it.

---

## Support & Donations

PercyScarlett is free and built out of passion for clean-room Xbox Series X|S & Windows compatibility research. If you'd ever like to kindly support ongoing development, hardware testing, and releases, **[GitHub Sponsors (@rpatty-dev)](https://github.com/sponsors/rpatty-dev)** keeps everything private and goes straight toward the project — no pressure at all, running the 10-second Self-Test helps hugely too!
