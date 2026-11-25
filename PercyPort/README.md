# PercyPort

Runs Grand Theft Auto VI (Xbox Series X|S) natively on Windows, from files
the user supplies.

The title's x86-64 code runs unchanged inside our process; PercyPort supplies
the platform underneath it and converts the title's data, ahead of time where
it can and on first use where it must (shaders). See
[docs/architecture.md](docs/architecture.md).

## Status (2026-11-23)

**Not playable yet.** The title boots, runs its menus at its own 30 fps and
loads the world, where rendering fails.

| Stage | State |
|-------|-------|
| Boot, first-boot screens (accessibility, language, settings, legal) | works, 30 fps |
| Intro video with sound | works |
| Start screen, profile and saves | works |
| Loading into the world (about 50 s) | works |
| The world (first mission's intro) | reached; the picture is black and the host GPU fails within seconds |

Tested on one machine: Windows 11, AMD RX 6700 XT.

> **Warning.** Entering the world on a real GPU can currently hang it, and
> Windows then resets the display driver (every monitor goes black for a
> moment). To look further, use a diagnostics build with
> `PERCYPORT_D3D12_WARP=1`, which renders on the software adapter.

What each part does today and what it still needs:

| Subsystem | Today | Needs |
|-----------|-------|-------|
| Loader | Runs the title's executable and its DLLs in our process; crash reports name the faulting code and are kept in `crashes.log` | – |
| Runtime services | One signed-in offline user, licences, packages, saves that survive a kill at any step, text entry dialog | Speech is silent; the dialog and relaunch are untested live |
| Network | Offline by design: the title cannot reach the network | – |
| Files and streaming | Archives and streamed data read, zlib and BCPack decoded in parallel | Loading is limited by writes into title memory |
| Input | XInput pads, keyboard, keyboard stand-in pad | Trigger rumble |
| Audio | Menu, video and loading sound; XMA voices and Opus streams decode | Reverb is silent; Opus results unverified against the title |
| GPU: menus | Draws, compute and presentation on host D3D12; only changed title memory is copied | Frame pacing drifts on one screen in release builds |
| GPU: world | Mesh pipelines, shader-written buffers, descriptor lifetimes are in | The host device is lost and frames are black; frames take seconds; indirect dispatches, real queries, depth bounds, HDR and scaling are missing |
| Shaders | Console GPU code recompiled to DXIL at first use, cached on disk. Menus: all. World: mesh 8,191 of 8,506, vertex/pixel 10,617 of 12,346, compute 1,643 of 2,539 | 64-bit image atomics, merged geometry stages, the ray-tracing builder's shaders; some compute programs need a GPU with 64-lane waves |
| Textures | Console tiled layouts and detiling for the modes the title uses | Render-target metadata planes |
| Series S mode | Runs through the menus to the loading screen | The world, as above |

## Getting started

Requirements: Windows 10/11 x64, a CPU with AVX2, a Direct3D 12 GPU with
shader model 6.6 (and mesh shaders for the world), Rust (pinned in
`rust-toolchain.toml`).

```powershell
cargo xtask install-hooks                # asset-policy check on every commit
copy percyport.example.toml percyport.toml   # then set your paths (outside the repo)
cargo test --workspace                   # needs no game data
cargo run -p percyport -- config           # check your paths
```

Run the title:

```powershell
cargo build --release -p percyport
target\release\percyport.exe run <game dir>\<the title's exe> --execute
```

The window caption shows the framerate. Saves, settings and `crashes.log`
are kept under the data folder (default `%LOCALAPPDATA%\percyport\data`), derived
data under the cache folder.

Input: XInput pads. With none connected the keyboard is a pad while the
title's window has focus:

| Keys | Pad |
|------|-----|
| W A S D / I J K L | left / right stick |
| arrows | D-pad |
| Enter, Backspace, X, Y | A, B, X, Y |
| Q / E, Z / C | shoulders, triggers |
| F / H | stick clicks |
| Esc, Tab | Menu, View |

## Commands

```
percyport inspect <exe|dll> [--imports] [--exports]
percyport census [paths...] [--missing] [--strict]
percyport lib-exports <import.lib> [--list]
percyport shader-info [paths...] [--verbose] [--disasm]
percyport translate [paths...] [--validate] [--pipelines] [--list] [--jobs N]
percyport run <exe> [--execute] [--unimplemented trap|zero] [--strict]
            [--console series-x|series-s] [-- <title arguments>]
percyport config
```

Logging: `PERCYPORT_LOG=percyport_loader=trace,info` (a release build logs
info and above).

## Debugging

A release build has no debugging aids. Build them in with

```powershell
cargo build --profile diag -p percyport --features diagnostics   # target\diag\percyport.exe
```

| Variable | Effect |
|----------|--------|
| `PERCYPORT_LOG=<target>=debug,...` | debug and trace output |
| `PERCYPORT_STACKS=20,60` | every thread's stack at those seconds |
| `PERCYPORT_WATCH=<rva>,...` | logs a title u32 when it changes |
| `PERCYPORT_KEYS=30:Enter,35:X` | presses keys at those seconds, focus or not |
| `PERCYPORT_CRASH_SAVE=<rva>:<len>,...` | saves title memory to the cache folder on a crash |
| `PERCYPORT_CAPTURE=<dir>` | saves presented frames (powers of two, or `PERCYPORT_CAPTURE_FRAMES=<list>`) |
| `PERCYPORT_DUMP_TARGETS=1` | with the two above, saves every render target of those frames |
| `PERCYPORT_D3D12_DEBUG=1` | host debug layer |
| `PERCYPORT_D3D12_DRED=1` | on a GPU failure, the allocation hit and the commands in flight |
| `PERCYPORT_D3D12_WARP=1` | host device on the software adapter (slow; cannot reset the driver) |
| `PERCYPORT_AUDIO_CAPTURE=<file.wav>` | records what the title plays (our process only) |
| `PERCYPORT_XMA_DUMP=<dir>` | saves the first 32 XMA voices' context and input |

Every build appends crashes and host GPU failures to `crashes.log` in the
title's data folder.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
