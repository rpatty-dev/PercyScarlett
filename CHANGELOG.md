# Changelog

## v0.9.0 — 2026-11-23

First retail Scarlett title running: Minecraft (Xbox Series X|S build) reaches the main menu and plays in-world. Pipeline ready for TheBig6 😉.

### New
- **Library** tab (now the start page): add a package folder before its exe exists, Check (plain PE + unresolved-import report), Register, Launch, per-game options (VRAM mirror, audio, offline sign-in token, D3D12 debug layer, save folder) and a filtered run log.
- Identity overlay generates placeholder store art when a dump lacks the images `MicrosoftGame.Config` references (registration used to fail).
- `d3d12_xs`: D3D12.X command-list proxy with inline record replay (draws, dispatches, root parameters, vertex/index buffers, PSOs), console present into a DXGI flip swap chain, barrier and resolve fixes.
- `d3d12_xs` VRAM mirror: console UMA buffers get a VRAM twin; CPU writes are tracked per 64 KiB page and copied before each direct-queue submit, so a discrete GPU no longer fetches geometry over PCIe.
- `xcurl`: offline XCurl (loopback transfers, PlayFab offline login reply, empty Xbox Live collections).
- `acphal` / `xapu`: audio control processor and APU memory, enough for console audio middleware to initialise.
- `xgameruntime`: offline XUser (local signed-in user, gamertag, optional synthetic token), persistent local storage, local XGameSave provider, device association, networking hints.
- `gameinput`: virtual gamepad driven by a script file.
- Loader: file-system virtualization for console user paths (directories, attributes, enumeration, moves, deletes, disk space) including the C/C++ runtime import tables, so saves are written.
- Loader crash log: true stack unwinding for access violations, fast-fail and heap corruption (with the heap failure record and memory around the bad block).

### Fixed
- Random boot abort: titles load `xgameruntime.dll` with `LOAD_LIBRARY_SEARCH_SYSTEM32`, bypassing the shim; the loader now always redirects it.
- Device-association callbacks are delivered through the title's task queue instead of a raw thread.

### Known issues
- Intermittent heap corruption during asset loading (about half of launches). Relaunch if the title closes on the loading screen.
- VRAM mirror is new; disable it per game in Library → Options if a title misrenders.

### Housekeeping
- Version 0.9.0. Self-test script moved to `scripts/`, TheBig6 notes to `docs/TheBig6.md`. README rewritten.

## v0.4.0
- Six unmodified Scarlett GDKX samples boot; strict RDNA2 → DXIL lifter; `-headless` tray mode.
