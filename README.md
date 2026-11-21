# PercyScarlett

**Clean-room Rust translation layer and PE32+ / GDK inspection toolkit for running unencrypted Xbox One (Durango) and Xbox Series X|S (Scarlett) executables on Windows x64.**

PercyScarlett is written from scratch in pure Rust with zero third-party emulator code, zero proprietary SDK headers, and minimal dependencies (standard `windows-sys` bindings only).

---

## Live Project Status

![PercyScarlett Live Subsystem Readiness](docs/status.svg)

---

## Architecture & Roadmap

| Module | Purpose | Current State |
| :--- | :--- | :--- |
| `src/pe64.rs` | Zero-copy PE32+ parser: sections, import table, DLLs, and GDK dependency classification | **Verified** |
| `src/xmem.rs` | Xbox physical/virtual memory pool prototype (`XMemAlloc` / `XMemFree` / `VirtualAlloc`) | **Unit-Tested** |
| `src/gdk.rs` | GDK runtime shim registry (`d3d12_xs`, `xgameruntime`, `dstorage`, `XAudio2_9`, `GameInput`, `winrt_xs`, `kernelx`) + `kernel32` resolution | **Unit-Tested** |
| `d3d12_xs.dll` / `xg_xs.dll` | Direct3D 12 Xbox proxy & texture swizzle translation to PC D3D12 | Planned |
| `xgameruntime.dll` / `dstorage.dll` | GDK runtime lifecycle, async storage, and `winrt_xs` HSTRING activation | Planned |
| `XAudio2_9.dll` / `GameInput.dll` | Audio voice routing and controller input shims | Planned |

---

## Requirements

- **OS:** Windows 10 / Windows 11 x64
- **Toolchain:** Rust (`cargo` / `rustc`) targeting `x86_64-pc-windows-gnu` or `x86_64-pc-windows-msvc`

---

## Build & Setup

```powershell
git clone https://github.com/rpatty-dev/PercyScarlett.git
cd PercyScarlett

# Debug build
cargo build

# Release build
cargo build --release
```

*(If using the `x86_64-pc-windows-gnu` toolchain, ensure `mingw64\bin` is in `PATH` before running `cargo`.)*

---

## Usage Instructions

### 1. Inspect a PE32+ Executable
Parses headers, sections, imported DLLs, and highlights Xbox/GDK imports alongside their planned PercyScarlett shims:

```powershell
cargo run --release -- inspect "C:\path\to\game.exe"
```

### 2. Scan an Unencrypted Build Directory
Scans a build folder for `.exe` targets, `MicrosoftGame.config` manifests, and local GDK/DLL dependencies without modifying the source directory:

```powershell
cargo run --release -- plan "C:\path\to\build_dir"
```

### 3. Probe Memory Pool & Shim Registry
Verify local Windows physical memory allocation and `kernel32` symbol resolution:

```powershell
cargo run --release -- probe-mem
cargo run --release -- shims
```

### 4. Non-Destructive Overlay Mount Workflow
To keep your original build folder clean when testing shim DLLs:
1. Create a separate mount directory (e.g., `D:\PercyScarlett_Mount`).
2. Create directory junctions (`mklink /J`) for large read-only asset subdirectories (`data`, `common`, `x64`, etc.).
3. Hardlink (`mklink /H`) or copy root binaries and `MicrosoftGame.config` into the mount directory.
4. Place compiled PercyScarlett shim DLLs into the mount directory and run `inspect` / `plan` against the mount path.

---

## Tests

Run the full Rust test suite:

```powershell
cargo test
```

### Covered Unit Tests (`3/3 passing`)
- `pe64::tests::parses_minimal_pe64_header` — Validates PE32+ DOS/NT header parsing, machine type (`0x8664`), section count, and entrypoint RVA.
- `xmem::tests::allocates_and_frees_physical_pool_blocks` — Validates Win32 `VirtualAlloc` pool allocation, block alignment, and `XMemFree` release.
- `gdk::tests::resolves_kernel32_gettickcount_on_windows` — Validates live `GetModuleHandleA` / `GetProcAddress` forwarding against `kernel32.dll`.

---

## Legal Notice

PercyScarlett is an independent, clean-room interoperability research project licensed under the [MIT License](LICENSE).
It contains no proprietary Xbox/Microsoft SDK headers, keys, or binaries, and does **not** decrypt, bypass, or strip retail DRM or encrypted `.xvc`/`.xvd` packages. Users must supply their own lawfully obtained, unencrypted executables.
