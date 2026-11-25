<p align="center">
  <img src="assets/mascot.png" width="200" alt="PercyScarlett mascot" />
</p>

<h1 align="center">PercyScarlett</h1>

<p align="center">
  <a href="https://rpatty-dev.github.io/PercyScarlett/"><strong>Project site</strong></a> ·
  <a href="docs/TheBig6.md"><strong>TheBig6 😉 readiness</strong></a> ·
  <a href="CHANGELOG.md"><strong>Changelog</strong></a> ·
  <a href="https://github.com/sponsors/rpatty-dev"><strong>♥ Sponsor</strong></a>
</p>

Rust translation layer that runs unencrypted Xbox Series X|S (Scarlett) GDK titles natively on Windows 10/11 x64. The title's x86-64 code runs as-is; PercyScarlett maps it in-process and supplies the console-only pieces: D3D12.X, XMem, the Xbox runtime, input, audio hardware, storage and networking.

**v1.0: TheBig6 😉 reaches its opening prison scene, lit and textured, on an 8 GB card. Start here: [docs/PROLOGUE.md](docs/PROLOGUE.md).**

```powershell
.\scripts\play-prologue.ps1 -Game "D:\path\to\your\dump"
```

The runtime in `runtime/victory` is built on VIctory, fxceless' victory-fork and ScarlettPort: see [CREDITS.md](CREDITS.md).

---

## Proof: Minecraft (Xbox Series X|S build) on Windows

![Minecraft Scarlett build in-world on PercyScarlett](docs/proof/minecraft_ingame_percy.png)

| Main menu | Loading |
| :---: | :---: |
| ![Main menu](docs/proof/minecraft_xsx_main_menu.png) | ![Loading](docs/proof/minecraft_xsx_loading_render.png) |

- Unmodified Scarlett exe, offline local player, world created, saved and reloaded.
- Main menu renders at several hundred FPS. In-world is ~45-50 FPS on an RTX 2060 SUPER before the v0.9 VRAM mirror (verification of the mirror is in progress).
- Known issue: about half of launches still hit an intermittent heap corruption during asset loading. Relaunch if it closes during the loading screen.

### Status

| Area | State | Where |
| :--- | :--- | :--- |
| Retail Xbox Series X\|S game | Minecraft (Scarlett build, unmodified exe): boots, main menu, creates/saves/reloads a world, in-game | screenshots above |
| Loader | Maps the title in-process: relocations, `.pdata`, guest static TLS, import routing, user-path virtualization | `src/pe_loader.rs` |
| Graphics | D3D12.X command lists replayed onto PC D3D12, console present into a DXGI swap chain | `crates/d3d12_xs/src/{replay,console_list,present}.rs` |
| Shaders | Minecraft ships DXIL, so its shaders run as-is. Retail RDNA2/GFX10 shaders (VS + PS) are lifted to SM 6.6 HLSL (`hlsl_xbox.rs`), compiled via DXC, and bound into live PC graphics PSOs (`gfx_pso.rs`, `desc_mirror.rs`) | `crates/d3d12_xs/src/{hlsl_xbox,dxbc_xbox,gfx_pso,desc_mirror}.rs` |
| TheBig6 😉 | v1.0: reaches the opening prison scene, lit and textured, on the Series S profile with an 8 GB card (about 2-3 fps on an RTX 2060 SUPER). Menus, Bink2 video, save data and world streaming work. Shaded areas are still too dark | `runtime/victory`, `docs/PROLOGUE.md` |

---

## TheBig6 😉: the opening prison scene

1. Download `PercyScarlett_v1.0.0_runtime_Win64.zip` from the [latest release](https://github.com/rpatty-dev/PercyScarlett/releases/latest) and unzip it.
2. Run `play.bat "D:\path\to\your\dump"` (the folder with `game_scarlett_master.exe`). Or from source: `.\scripts\play-prologue.ps1 -Game "D:\path\to\your\dump"`.
3. Press Enter through the logos and menus. The first visit to a scene compiles its GPU pipelines, so the picture can be black or incomplete for a few minutes; later runs use the cache.

| Profile | Speed (RTX 2060 SUPER 8 GB) | Notes |
| :--- | :--- | :--- |
| Series S (default) | about 2-3 fps | lit world, fits an 8 GB card |
| Series X | about 1 fps | more VRAM, spills into system RAM on 8 GB cards |

What changed in v1.0: the Series S world was black because the game upscales with the console build of AMD FSR 4, whose pipelines have no PC equivalent. The upscale is now a stretch of the colour image, so the lit picture reaches the screen. Next up: shaded areas and interiors (ray-traced lighting) and speed.

Full guide, switches and known issues: [docs/PROLOGUE.md](docs/PROLOGUE.md). Older notes: [docs/TheBig6.md](docs/TheBig6.md).

---

## Using it

1. Download the latest release zip and run `percyscarlett.exe`.
2. **Library → Add game folder…** and pick the folder that holds `MicrosoftGame.Config`.
3. **Check** confirms the exe is a plain PE (not encrypted) and lists any import that still needs a shim.
4. **Register** creates the identity overlay and registers it with the PC Gaming Runtime (GDK `wdapp`; Developer Mode required).
5. **Launch**. Per-game options (VRAM mirror, audio, offline sign-in token, D3D12 debug layer, save folder) sit next to the run log.

Command line equivalents: `percyscarlett register <exe>`, `percyscarlett launch <exe>`, `percyscarlett inspect <exe>`.

PercyScarlett never decrypts, patches or bypasses licensing. Use decrypted dumps of games you own.

---

## What's inside

| Crate | Provides |
| :--- | :--- |
| `runtime/victory` | The runtime that runs TheBig6 😉: Xbox Series runtime, RDNA2 shader translator and D3D12 layer (based on VIctory and victory-fork), with the Series S upscale fix |
| `PercyPort` | Earlier Rust layer: tiled page-pool VRAM aliasing, Series S overlay, watchdog handling |
| `src/pe_loader.rs` | In-process PE32+ loader: relocations, `.pdata`, guest static TLS, IAT routing, file-system virtualization (`G:`/`T:`/user paths), crash logging with true unwinding |
| `crates/d3d12_xs` | D3D12.X device/list/queue proxy, inline command-record replay, console presents into a DXGI swap chain, UMA buffer VRAM mirror, RDNA2 → DXIL shader lifter |
| `crates/xgameruntime` | Runtime shim over the PC Gaming Runtime: offline XUser, persistent local storage, XGameSave, device association, networking hints |
| `crates/xcurl` | Offline XCurl (loopback transfers, empty Xbox Live replies) |
| `crates/acphal`, `crates/xapu` | Audio control processor and APU memory for console audio middleware |
| `crates/gameinput` | GameInput with a scriptable virtual gamepad |
| `crates/xmem`, `crates/kernelx` | XMem pools and console kernel extras |
| `crates/dstorage`, `crates/xg_xs`, `crates/xaudio2_9`, `crates/winrt_xs` | DirectStorage, XG texture layouts, XAudio2, WinRT surface |

Self-test (no game needed): download [`Run_SelfTest_And_Report.bat`](https://github.com/rpatty-dev/PercyScarlett/releases/latest/download/Run_SelfTest_And_Report.bat), or in the app **Settings → Run Self-Test**. It opens a pre-filled GitHub issue with your results.

---

## Build

```
cargo build --release --workspace
cargo test --workspace --release
```

Rust stable (x86_64-pc-windows-gnu), Windows 10/11 x64, Microsoft GDK for `wdapp`. `dxcompiler.dll` / `dxil.dll` ship in the release zip.

---

## Credits & licence

- `runtime/victory` is built on [VIctory](https://github.com/Marvisak/VIctory) by Marvisak and contributors, [victory-fork](https://github.com/fxceless/victory-fork) by fxceless, and the Series S upscale fix from [ScarlettPort](https://github.com/zigzagboi1/scarlettport) by zigzagboi1. Full list in [CREDITS.md](CREDITS.md).
- XCurl offline transfer logic ported from [OpenScarlett](https://github.com/californiaAlmonds/OpenScarlett) (MIT, © 2026 OpenScarlett Contributors).
- AMD RDNA2 ISA opcode table from AMD's MIT-licensed machine-readable ISA.
- Console bottom-of-pipe write and memory-wait handling ported from [xcl](https://github.com/ivall/xcl) (GPL-2.0).
- PercyPRO font under the SIL OFL (`assets/fonts/OFL.txt`).

PercyScarlett is released under the GNU General Public License v2.0 only ([LICENSE](LICENSE)). Not affiliated with Microsoft, Xbox, Mojang or any publisher.
