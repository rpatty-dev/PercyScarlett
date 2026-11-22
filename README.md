# PercyScarlett

**Clean-room Rust translation layer, GDK shim suite, and PE32+ inspection toolkit for running unencrypted Xbox Series X|S (Scarlett) executables natively on Windows x64 — built specifically for Codename G6.**

PercyScarlett is written 100% from scratch in pure Rust with zero third-party emulator code, zero proprietary SDK headers, and zero external runtime bloat (standard Win32 FFI only).

---

## Live Project Status

![PercyScarlett Live Subsystem Readiness](docs/status.svg)

---

## Why PercyScarlett Leads Toward Codename G6 on PC

Unlike offline static recompilers that require ~350 GB of extracted intermediate assets or uncompiled skeletons, PercyScarlett runs **directly against the original unencrypted build directory** using Windows x64 native execution + lightweight Rust `cdylib` import shims:

| Capability | PercyScarlett (Pure Rust) | Offline Recompilers |
| :--- | :--- | :--- |
| **Disk Overhead** | **0 GB** (NTFS junctions + hardlinks) | ~350 GB extracted intermediates |
| **CPU Execution** | **Native x86-64** (`machine=0x8664`) | Native / static recompile |
| **Shipped DLLs Live-Loaded** | **6/6 Xbox/GDK sidecar DLLs passing `LoadLibraryW` on Win11** | Untested |
| **Compiled Shim DLLs** | **6 Rust `cdylib` DLLs built & tested** (`xmem`, `d3d12_xs`, `winrt_xs`, `xgameruntime`, `dstorage`, `GameInput`) | Skeleton / Not started |
| **End-to-End PE32+ Guest Test** | **Verified** (`scarlett_guest.exe` executes all 6 shims live) | None |
| **Day-1 Missing Import Recovery** | **Automated** (`percyscarlett stubgen` generates Rust `cdylib` crates) | Manual |

---

## Empirical Proof: Codename G6 Build Verification

> Full binary breakdown & IAT routing architecture: **[docs/CODENAME_G6_PROOF.md](docs/CODENAME_G6_PROOF.md)**

### 1. Live `LoadLibraryW` Verification Across All Shipped Xbox Sidecar DLLs (`6/6 PASS`)
Running `percyscarlett verify-dlls` against the unencrypted **Codename G6** build directory on Windows 11 x64 loads every shipped Xbox/GDK sidecar DLL directly against PercyScarlett's compiled shims (`xmem.dll`, `d3d12_xs.dll`) and host `System32` (`XAudio2_9.dll`, `WINHTTP.dll`, `CRYPT32.dll`):

```text
[PASS] amd_fidelityfx_loader_xbox.dll loaded at 0x7ffc03a90000
[PASS] amd_fidelityfx_upscaler_xbox.dll loaded at 0x7ffb96450000
[PASS] Bink2Scarlett.dll loaded at 0x7ffba5720000
[PASS] libHttpClient.GDK.dll loaded at 0x7ffb94cd0000
[PASS] XboxBvhBuilder.dll loaded at 0x7ffb94cf0000
[PASS] XCurl.dll loaded at 0x7ffba57b0000
DLL verification: 6 passed, 0 failed
```

### 2. End-to-End Synthetic Scarlett PE32+ Guest Execution (`PASS`)
`tests/e2e_bringup.rs` compiles a real x86-64 PE32+ executable (`scarlett_guest.exe`) dynamically linked against all 6 PercyScarlett shim DLLs, inspects it with `percyscarlett inspect`, runs `percyscarlett stubgen`, and executes it live on Windows 11 x64:

- **`xmem.dll` (`crates/xmem`):** `XMemGetWorkingSetStatistics` (reports 4 GiB avail / 8 GiB total), `XMemAlloc` (64 KiB read/write), `XMemFree`, `XMemVirtualQuery`, plus `PERCY_XMEM_LOG` call tracing.
- **`d3d12_xs.dll` (`crates/d3d12_xs`):** `D3D12SerializeRootSignature` with Scarlett upper flag `0x0100_0000` masked cleanly (`S_OK`) + live `D3D12CreateDevice` forwarding to host `d3d12.dll`.
- **`winrt_xs.dll` (`crates/winrt_xs`):** `RoInitialize`, `WindowsCreateString`, `WindowsGetStringLen`, `WindowsGetStringRawBuffer`, `WindowsDeleteString`.
- **`xgameruntime.dll` (`crates/xgameruntime`):** `XTaskQueueCreate`, `XUserGetId`, `XTaskQueueCloseHandle`.
- **`dstorage.dll` (`crates/dstorage`):** `DStorageSetConfiguration`, `DStorageSetConfiguration1`, `DStorageGetFactory`.
- **`GameInput.dll` (`crates/gameinput`):** `GameInputCreate`.

### 3. Live Physical Memory Pool Probe (`PASS`)
```text
Allocated 16 MiB Scarlett physical test block at 0x1eb59000000 (active=16777216 B, blocks=1)
```

---

## Workspace Architecture

| Crate / Module | Output | Purpose | Status |
| :--- | :--- | :--- | :--- |
| Root (`src/`) | `percyscarlett.exe` | Zero-copy PE32+ parser, `plan`, `inspect`, `verify-dlls`, `stubgen`, `probe-mem`, `shims` | **Verified** |
| `crates/xmem` | `xmem.dll` | Scarlett physical/virtual memory pool (`XMemAlloc*`, `XMemFree*`, stats, `VirtualQuery`) | **Live-DLL Verified** |
| `crates/d3d12_xs` | `d3d12_xs.dll` | D3D12 Xbox proxy (`D3D12CreateDevice`, root signature flag masking, deserializer, debug) | **Live-DLL Verified** |
| `crates/winrt_xs` | `winrt_xs.dll` | WinRT `RoInitialize` / `HSTRING` creation, inspection & cleanup | **Live-DLL Verified** |
| `crates/xgameruntime` | `xgameruntime.dll` | `XTaskQueue`, `XAsync`, `XUserGetId` GDK runtime shims | **Live-DLL Verified** |
| `crates/dstorage` | `dstorage.dll` | DirectStorage configuration & factory shim | **Live-DLL Verified** |
| `crates/gameinput` | `GameInput.dll` | `GameInputCreate` controller shim | **Live-DLL Verified** |

---

## Requirements

- **OS:** Windows 10 / Windows 11 x64
- **Toolchain:** Rust (`cargo` / `rustc`) targeting `x86_64-pc-windows-gnu` or `x86_64-pc-windows-msvc`

---

## Build & Setup

```powershell
git clone https://github.com/rpatty-dev/PercyScarlett.git
cd PercyScarlett

# Build CLI + all 6 shim DLLs in release mode
cargo build --release --workspace
```

*(If using the `x86_64-pc-windows-gnu` toolchain, ensure `mingw64\bin` is in `PATH` before running `cargo`.)*

---

## Day-1 Bring-Up Instructions

### 1. Inspect the Executable & Auto-Generate Missing Stubs
```powershell
.\target\release\percyscarlett.exe inspect "C:\path\to\game.exe"
.\target\release\percyscarlett.exe stubgen "C:\path\to\game.exe" ".\generated_stubs"
```

### 2. Scan & Verify All Build DLLs Live on Windows
```powershell
.\target\release\percyscarlett.exe plan "C:\path\to\build_dir"
.\target\release\percyscarlett.exe verify-dlls "C:\path\to\build_dir"
```

### 3. Non-Destructive Overlay Mount Workflow
Keep the original build directory untouched when testing shim DLLs:
1. Create a separate mount directory (e.g. `D:\PercyScarlett_Mount`).
2. Create directory junctions (`mklink /J`) for large read-only asset subdirectories (`data`, `common`, `x64`, etc.).
3. Hardlink (`mklink /H`) root binaries and `MicrosoftGame.Config` into the mount directory.
4. Copy `target\release\*.dll` into the mount directory, run `verify-dlls`, set `PERCY_XMEM_LOG=xmem_calls.log`, and launch.

---

## Tests (`10/10 passing`, `0` compiler warnings)

Run the full workspace test suite:

```powershell
cargo test --workspace
```

- `e2e_bringup::scarlett_guest_exe_end_to_end_bringup` — Compiles & executes synthetic PE32+ `scarlett_guest.exe` against all 6 compiled shim DLLs + runs `inspect` & `stubgen`
- `pe64::tests::parses_minimal_pe64_header` — Validates PE32+ header parsing, machine `0x8664`, sections, and entrypoint RVA
- `xmem::tests::allocates_and_frees_physical_pool_blocks` — Validates Win32 `VirtualAlloc` pool allocation & release
- `gdk::tests::resolves_kernel32_gettickcount_on_windows` — Validates live `kernel32.dll` symbol forwarding
- `xmem::tests::xmem_alloc_free_and_stats_work` — Validates `xmem.dll` C ABI exports
- `d3d12_xs::tests::resolves_host_d3d12_exports_on_windows` — Validates `d3d12_xs.dll` forwarding to host `d3d12.dll`
- `winrt_xs::tests::hstring_create_read_delete_roundtrip` — Validates `HSTRING` lifecycle
- `xgameruntime::tests::task_queue_and_xuser_work` — Validates `XTaskQueue` & `XUserGetId`
- `dstorage::tests::dstorage_config_and_null_factory_guard` — Validates DirectStorage configuration & null guards
- `gameinput::tests::gameinput_null_guard` — Validates `GameInputCreate` null guard

---

## Legal Notice

PercyScarlett is an independent, clean-room interoperability research project licensed under the [MIT License](LICENSE).
It contains no proprietary Xbox/Microsoft SDK headers, keys, or binaries, and does **not** decrypt, bypass, or strip retail DRM or encrypted `.xvc`/`.xvd` packages. Users must supply their own lawfully obtained, unencrypted executables.
