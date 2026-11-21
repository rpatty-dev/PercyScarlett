# PercyScarlett

**PercyScarlett** is an original, clean-room, MIT-licensed **Xbox Series X|S (`Scarlett`) bring-up & translation layer** written in pure **Rust** for Windows 10/11 (x64), designed to pair with [PercyStar](https://github.com/rpatty-dev/PercyStar).

Because Xbox Series X|S runs on an **AMD Zen 2 x86-64 CPU** and a Windows-derived Game OS, guest x86-64 instructions can execute natively on Windows once the Scarlett-specific runtime, memory aliasing, storage, audio, input, and `d3d12_xs` graphics ABI are translated.

---

## Roadmap & What We Plan to Build

| Phase | Subsystem | Plan | Status |
| :--- | :--- | :--- | :--- |
| **Phase 1** | **PE32+ & GDK Manifest Inspector (`src/pe64.rs`)** | Parse x64 PE headers, sections, import/delay-load DLL tables, `MicrosoftGame.Config`, and `layout_*.xml` package layouts to map every required Scarlett API before launch | **Implemented** |
| **Phase 2** | **Unified Memory & `kernelx` Shim (`src/xmem.rs`)** | Translate `XMemAllocPhysical` / `XMemMapAllocation` / 64 KiB & 4 MiB page aliasing using Win10/11 `VirtualAlloc2` & `MapViewOfFile3` | **Prototype + Tests** |
| **Phase 3** | **GDK Runtime Shims (`src/gdk.rs`)** | Clean-room `xgameruntime`, `GameInput` (XInput/KB), `XAudio2_9`, `dstorage` (GDeflate/BCPack), and `winrt_xs` stubs + telemetry | **Planned / Registry** |
| **Phase 4** | **`d3d12_xs` & `xg_xs` Graphics Proxy** | Proxy `ID3D12DeviceX` / `CommandQueueX` / `CommandListX` (`PresentX`, `DispatchMesh`, swizzle layouts) onto desktop DX12 / `wgpu` | **Planned** |
| **Phase 5** | **RDNA2 / `SGD5` Shader Translation** | Pair with PercyStar's `rdna2` + `sgd5` decoder/lifter to translate precompiled GFX10.3 shaders to DXIL/SPIR-V | **Paired w/ PercyStar** |

---

## Quick Start

```bash
cargo test
cargo run -- inspect path\to\game.exe
cargo run -- plan path\to\game_folder
cargo run -- probe-mem
cargo run -- shims
```

## Legal & Clean-Room Notice
PercyScarlett contains **100% original MIT-licensed Rust code**. It contains **no** proprietary binaries, encryption keys, firmware, or copyrighted assets, and never decrypts retail `.xvc`/`.xvd` packages.
