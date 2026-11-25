# Reaching the opening prison scene

PercyScarlett v1.0 runs TheBig6 😉 from your own dump into its first scene: the prison yard, lit and textured, on an 8 GB NVIDIA card.

## What you need

- Windows 10 or 11 and a D3D12 GPU with 8 GB of VRAM (tested: RTX 2060 SUPER 8 GB, 16 GB RAM)
- Rust (`rustup`; the toolchain in `runtime/victory/rust-toolchain.toml` installs itself)
- Your own dump of the game, with `game_scarlett_master.exe` in its folder. An NVMe drive helps. Verify the big archives after copying: a bad copy of `levels.rpf` gives a black world and a crash.

## Run it

```powershell
.\scripts\play-prologue.ps1 -Game "D:\path\to\your\dump"
```

The first run builds the runtime (a few minutes) and downloads the D3D12 Agility SDK. Press Enter through the logos and menus.

The first visit to a scene compiles its GPU pipelines: the picture can be black or have missing pieces for a few minutes. The pipeline cache is kept on disk (`%LOCALAPPDATA%\victory\cache`), so later runs start straight away.

## What to expect

| Profile | Speed (RTX 2060 SUPER) | Notes |
| :--- | :--- | :--- |
| Series S (default) | about 2-3 fps | recommended; fits an 8 GB card |
| Series X (`-Console series-x`) | about 1 fps | needs more VRAM; spills into system RAM on 8 GB cards |

Known issues:
- Shaded areas and interiors are too dark: ray-traced lighting is off, and shaders that need 64-lane waves do not run on NVIDIA.
- Speed is far from playable. Close other GPU and RAM heavy apps.

## How the Series S black screen was fixed

On the Series S profile the game renders at 720p and upscales to 1080p with the console build of AMD FSR 4. Its pipelines come from a console-only call (`DeserializeComputePipelineStateX`) that has no PC equivalent, so the upscaled picture stayed black. The runtime now takes over the upscale dispatch (`crates/victory-hle/src/modules/ffx.rs`) and records a plain stretch of the colour image into the output (`crates/victory-gpu/src/stretch.rs`). Everything else in the game's FSR DLL runs as it is.

## Useful switches

| Variable | Effect |
| :--- | :--- |
| `VICTORY_LEVELS_ON_DEMAND=1` | load texture levels on demand, about 3 GB less VRAM (set by the script) |
| `VICTORY_DISPLAY_MODE=1920x1080` | tell the game it is on a 1080p TV (Series X profile) |
| `VICTORY_PIPELINE_WORKERS=<n>` | threads that build GPU pipelines |
| `VICTORY_WATCHDOG=0` | stop holding the game's own stall watchdog |
| `VICTORY_TEXTURE_BUDGET_MB=<n>` | raise the texture memory cap (more textures, slower when VRAM spills) |
