# VIctory fork: the Xbox Series build of GTA VI, running on a Windows PC

<p align="center">
<img width="100%" alt="in the world on a PC: the dormitory, lit by the game's own lights" src="https://github.com/user-attachments/assets/d889d2cf-dc29-4e98-b832-dc43efca7d71" />
<br/><sub>In the world on an RTX 3060 (2026-11-24). Real-time, through this fork; no emulator, no game files in this repo.</sub>
</p>

<p align="center">
<img alt="status: in the world, 2-3 fps" src="https://img.shields.io/badge/status-in%20the%20world%2C%202--3%20fps-orange" />
<img alt="platform" src="https://img.shields.io/badge/platform-Windows%2011%20%7C%20D3D12-blue" />
<img alt="license" src="https://img.shields.io/badge/license-GPL--3.0--or--later-green" />
<img alt="last update" src="https://img.shields.io/github/last-commit/brololikeptputtingmypassword/victory-fork-BESTegolol" />
</p>

A fork of [fxceless/victory-fork](https://github.com/fxceless/victory-fork), which is itself a
fork of [VIctory](https://github.com/Marvisak/VIctory): a compatibility layer that runs an
Xbox Series title natively on Windows (Direct3D 12) from files the user supplies. The console's
GPU programs (GFX10 machine code) are translated to DXIL; the console's system calls are
answered on Windows.
This fork pushes on two things: **the world drawing completely** and **frame rate**.
Jump to: [current state](#current-state-updated-2026-11-25) ·
[what changed](#what-this-fork-changed) · [switches](#recommended-switches) ·
[next](#next) · [getting started](#getting-started).
If this helps you, a star helps others find it.

**This repository contains no game files, code, shaders, dumps or keys.** You need your own
copy of the game. Not affiliated with or endorsed by the game's developer or publisher.
Licensed GPL-3.0-or-later, like everything it is built on.

## Current state (updated 2026-11-25)

Tested on: Windows 11, AMD Ryzen 7 5700X, NVIDIA RTX 3060 12 GB, 32 GB RAM.

Screenshots from play sessions (in the world, ~8 minutes after launch; 1-3 fps):

<img width="49%" alt="cell, dialogue" src="https://github.com/user-attachments/assets/5f704e91-0fc9-4344-8223-87e6975a661b" /> <img width="49%" alt="yard gate" src="https://github.com/user-attachments/assets/afc23132-8f6d-4ca9-b7b0-f542a8e1a8c2" />
<img width="49%" alt="visitor window: green surfaces and blocky shadows still wrong" src="https://github.com/user-attachments/assets/398367c9-db70-40f0-abd0-0ad27ec4a03b" /> <img width="49%" alt="sunset through the visiting room window" src="https://github.com/user-attachments/assets/5f7004e7-a4bd-41be-94b9-8dd92d29b17b" />

Visible still wrong: some surfaces bright green, blocky shadow edges, light in shadow too
dark, and walls/floors that are late to appear while the world streams in.

| Area | State |
|------|-------|
| Boot, menus | works |
| World geometry (walls, floors, ceilings, eyes) | **now drawn**: four pixel programs that were dropped (and their draws with them) now translate (see below); some pieces are late while the world streams in |
| Shader translation (offline check over 2,124 programs saved from a play session) | vertex+pixel 888/888, mesh 165/165, compute 1,066/1,071 before today's merge (re-check in progress) |
| Ray-traced lighting (the game builds its own BVH in 53 compute programs) | **50 of 53 builder pipelines now made on a 32-lane NVIDIA GPU** (four were refused for 64-lane waves): the builder runs only when all are there, so light in shadow is still dark |
| Frame rate in the world | **about 2-3 fps** (1.3 fps with the old defaults; see the switch table) - CPU-bound: the card is idle about half of every frame |
| Loading | slow: about 5 minutes to the world on a warm cache |
| Start screen | stutters: frames wait up to 500 ms for pipelines still being made (`VICTORY_PIPELINE_WAIT_MS`), while ~1,400 are compiled in the background |

## What this fork changed

1. **Image descriptors used only for their size** (`crates/victory-shader/src/emit/lower.rs`).
   Some pixel programs load an 8-dword image descriptor only to compute the texture's width,
   height and base level, never sampling it. The translator refused these ("a descriptor read
   only as data"), the whole program failed and every draw using it was dropped - that is
   where walls, floors and eyes went. Such a load is now bound as a 2D texture so the size
   math reads real values. `VICTORY_SIZE_ONLY_DESC=0` restores the old behaviour.
2. **Shader cache keyed by the translator's content** (`crates/victory-gpu/build.rs`,
   `shader_cache.rs`): the cache survives rebuilds that do not change the translator, so a
   rebuild no longer means a slow first launch.
3. **Configurable pipeline workers** (`VICTORY_PIPELINE_WORKERS`, `pipeline/make.rs`): more
   threads compile pipelines during loading screens.
4. **Console 64-lane compute programs on 32-lane GPUs: LDS in lane branches**
   (`lds_meetings` in `lower.rs`). Such a program runs as two host waves that meet at group
   barriers around its shared-memory (LDS) accesses; a lane branch holding both an LDS read
   and a write was refused, and NVIDIA refuses the 64-lane fallback. Now accepted when the
   read and an atomic use one lane-uniform address with disjoint bytes (counters and
   allocators), or the read and write both use lane-uniform addresses (a one-lane section).
   `VICTORY_LDS_LOOSE=1` accepts the remaining cases too (a test switch: two host waves may
   then see each other's LDS accesses out of the console's order).
   `VICTORY_HOST_WAVE=32` makes offline `victory translate` stand in for a 32-lane GPU.
5. **Translation gaps** (the second session's branch, merged): `v_cvt_f32_f64` and wave
   sums of doubles, `v_fma_mixlo_f16` over interpolation halves, whole-wave sections whose
   ends the compiler merged, a last form for 32-lane hosts, and a retry of cross-lane code
   with every lane present; signature rows in element order and one interpolation mode per
   row (six programs the DXIL validator refused).
6. **Descriptor tables shared between lists** (`descriptor/host.rs`, `draw.rs`): a table
   without constant buffers is keyed by what its entries name (view record and generation)
   and made once for every list that binds it; about 43% of tables are now taken instead of
   made. `VICTORY_SHARED_TABLES=0` turns it off.
7. **Card-side copy buffers from 1 MB to 3 GB** (`shadow.rs`): snapshots fell from
   100-230 ms to about 3 ms a frame and copy-back from 190-480 ms to about 50 ms.
8. **No blending into integer render targets** (`pipeline/host.rs`): D3D12 refuses such a
   pipeline, so a mesh program writing a `uint` target beside blended colour had none.
9. **Merged** (2026-11-25): upstream's depth bounds test, texture refetch for characters'
   skin and hair, and faster defaults; from
   the VItoriaBr fork (GPL-3.0) a fix for interpolation first halves read
   outside their pair, and `VICTORY_SHADER_LOOP_LIMIT` in release builds.

## Recommended switches

The copy-back thread (`VICTORY_RELAY=2`), wide copy-back and the larger card buffers are
now on by default; `Play GTA VI.bat` also sets `VICTORY_PIPELINE_WORKERS=14` and
`VICTORY_TEXTURE_NO_WAIT=1`. Earlier measurements, with an automatic A/B runner (each setup 7 minutes, frames averaged after loading):

| Setup | avg fps | ms/frame |
|-------|---------|----------|
| `VICTORY_RELAY=2 VICTORY_PIPELINE_WORKERS=14 VICTORY_TEXTURE_NO_WAIT=1` | 1.34 | 867 |
| + `VICTORY_WIDE_BACK=1` | **2.11** | **488** |
| + `VICTORY_CARD_BUFFERS_MIN_MB=4` | 1.59 | 642 |
| + `VICTORY_INPLACE=1` | 1.56 | 816 |

`VICTORY_TEXTURE_NO_WAIT=1` lets texture refreshes read through the kernel instead of waiting
for title memory remaps.

## How the work is done

- **Find what fails.** `VICTORY_SHADER_SAVE=<dir>` saves every program the title submits during
  play. `victory translate <dir> --list --validate --pipelines --jobs N` translates them all
  offline and lists every failure by reason; `victory shader-info <file> --disasm` shows one.
- **Fix, then check for regressions** by translating the whole saved set before and after
  (each fix has an off switch so before/after runs come from the same build).
- **Measure speed** from the per-second counters the host logs (preparing / copies / GPU /
  copy-back / heap passes), with runs started and stopped by script and pad input
  scripted with `VICTORY_KEYS`.
- **Frame captures**: RenderDoc 1.47 cannot capture this title in the world on a 12 GB card -
  saving the initial state of ~18,600 resources runs out of memory and ends the game.
- Done with [Claude Code](https://claude.com/claude-code); two sessions work in parallel
  (one on translation gaps in a separate worktree, one on speed and testing).

## Next

- **Precompiled pipelines.** The game first asks for its pipelines through two console device
  methods that no VIctory fork implements (0x1c8 graphics, 0x218 compute; over 16,000 and
  2,000 calls at boot), and every call fails. Each piece of precompiled code opens with its
  source shader's hash, and the same shaders later arrive as ordinary containers, so the game
  appears to fall back to compiling those; making the precompiled ones directly could cut
  loading time. Decoded so far (`VICTORY_PIPE_DUMP=<dir>` saves them): a register block, the
  vertex input layout, each stage's GFX10 code in GPU memory and a resource table per stage.
  `VICTORY_PRECOMPILED=1` makes the compute ones from known containers (few are known at boot).
- The last ray-tracing builder programs on 32-lane GPUs ("a lane mask exchanged / a first-lane
  read where not every lane runs"), then a run with `VICTORY_BVH_BUILDER=1` for light in shadow.
- Start-screen stutter: a shorter pipeline wait, and pipelines a frame needs ahead of the
  background prewarm queue.
- Loading time, and textures held back by the VRAM budget (~12,000 waiting in the world).
- Frame time: copy-back and the per-frame heap-buffer pass are the biggest CPU costs.
- Green surfaces; dark scenes (no ray-traced bounce light until the builder runs); three
  64-lane compute programs (light/fog volume among them) still refused on 32-lane GPUs.

---

*Below: the notes of the fork this one is based on (fxceless/victory-fork), unchanged.*

# VIctory fork (work in progress)

A work-in-progress fork of [VIctory](https://github.com/Marvisak/VIctory), a
compatibility layer that runs an Xbox Series title natively on Windows from
files the user supplies. This fork follows upstream
`main` and adds fixes aimed at one goal: getting the game's world to render
correctly and playably on one particular PC.

**This repository contains no game files, code or assets.** You need your own
copy of the game. It is not affiliated with or endorsed by the game's
developer or publisher.

## Credits

- **[VIctory](https://github.com/Marvisak/VIctory)** by Marvisak and its
  contributors is the base of everything here: the loader, the runtime
  services, the Direct3D 12 back end and the shader recompiler. Licensed
  GPL-3.0-or-later; so is this fork. Upstream's history is kept intact below
  the single commit that carries this fork's changes.
- **[XCL](https://github.com/ivall/xcl)** by ivall and its contributors
  (GPL-2.0-only) was used as a reference for how the same title behaves under
  another layer. Fixes inspired by it were written anew in VIctory's code.
- This fork's changes were developed with [Claude Code](https://claude.com/claude-code).

## Status of this fork (2026-11-25)

**Not playable yet: 2.5 to 4 frames per second, and shadowed areas are black.**
Tested on one machine: Windows 11, Intel i7-14700KF, NVIDIA RTX 4070 12 GB,
32 GB RAM. The Series X profile is used throughout.

| Stage | State in this fork |
|-------|--------------------|
| Boot, menus, start screen | works, 30 fps (as upstream) |
| Opening cutscene | plays from start to finish, in order, lit: title cards, credits cards, night and dusk shots |
| Saves | the game autosaves a checkpoint and loads it again |
| First gameplay scene (the prologue) | renders: characters, textures, sky, sunlit exterior; HUD, pause menu and pad input work |
| Light in shadow | missing: the game's ray-traced bounce light and ray-traced lamp and sun shadows do not run yet, so anything the sun does not hit directly is black |
| Frame rate in the world | 2-4 fps: 420-450 ms per frame by default, 240-350 ms with the newer switches (`VICTORY_RELAY=2`, or `VICTORY_LATE_WAIT=3 VICTORY_WIDE_BACK=1 VICTORY_CARD_BUFFERS_MIN_MB=1 VICTORY_CARD_BUFFERS_MB=1024`); it started at 3-6 seconds per frame |
| Stability | no driver hang or device loss in normal runs (a hang introduced by the wave64 update was found and fixed: its barrier form for 64-lane programs is now off unless `VICTORY_WAVE_WIDE=1`) |

### What this fork changed (summary)

Rendering:
- Meshlet culling read its scene buffers as the wrong kind of view, so the world drew almost nothing; fixed.
- Per-draw descriptor tables could run out, after which draws used slots never made for them and the card hung; fixed.
- Light packed by the game's compute shaders was read back through empty copies (sky, probes, fog); fixed for every mip level and layer.
- The fog reconstruction, shadow-mask, hair, blend-shape skinning, fire and several material programs did not translate; fixed.
- Gathers at a named mip level (with and without an offset) read level 0; fixed.
- Implicit-level samples outside pixel shaders produced invalid DXIL in 730 compute programs; fixed.
- Rectangle lists, depth and stencil twins for depth textures written by compute, texture read-backs.
- Ray tracing, in progress and off by default: the game's own structure-builder programs translate, title memory can be read by raw address from shaders, the hardware ray/box and ray/triangle test is emulated in DXIL (checked against a reference on the GPU), and 59 of the 64 trace programs translate. Nothing is lit by it yet. See [docs/fork-ray-tracing.md](docs/fork-ray-tracing.md).

Speed and memory:
- Waits for memory values, write-watch queries and page queries no longer walk page tables per call.
- Large title buffers get a copy in card memory, with shader writes found by hashing on the card.
- Submissions can be sent ahead of the wait for the card (`VICTORY_LATE_WAIT`), with a relay thread that lands results as the card finishes (`VICTORY_RELAY`): in a first five-minute run the card's idle time behind the submitting thread fell from about 136 to 30 ms per frame.
- Title memory can be opened as a graphics heap so that buffers are used in place instead of copied (`VICTORY_INPLACE`, first stage only; a probe confirmed the approach on the test GPU).
- The bindless texture queue no longer uploads stale views; the view table no longer grows without bound; the pipeline library is mapped instead of read into memory.
- An intermittent device loss was traced to the pipeline library file being replaced while mapped; fixed.

- Translated shaders get clean-up passes over the finished module (dead code, constant folding, reuse of values, branch facts); the pack's host instruction count falls by about 43 %, worth about 20 ms of GPU time per frame on the test machine (`VICTORY_SHADER_OPT=0` turns them off).

Most of the newer speed changes sit behind environment switches that are off
by default (`VICTORY_LATE_WAIT`, `VICTORY_RELAY`, `VICTORY_WIDE_BACK`,
`VICTORY_CARD_BUFFERS_MIN_MB`, `VICTORY_INPLACE`, `VICTORY_BVH_BUILDER`, ...).
Each is described in a comment where it is read.

### Known problems

- Everything in shadow is black (see above).
- The interior of the first gameplay room is sometimes not drawn after the cutscene hands over to gameplay.
- Around 24 GB of memory is in use during play.
- A few style lints are switched off for this fork in the workspace `Cargo.toml` until the newer code is tidied.
- Many changes here are specific to this title and this GPU, and some are marked `shortcut:` in the code where they approximate what the console does.

Everything below this line is upstream VIctory's own README, unchanged; its
status section describes upstream, not this fork.

---

# VIctory

Runs the title (Xbox Series X|S) natively on Windows, from files
the user supplies.

The title's x86-64 code runs unchanged inside our process; VIctory supplies
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
| The world (first mission's intro) | reached and runs with indirect draws; its title card renders, the scene after it is still dark; 0.5-1.8 s per frame |

Tested on one machine: Windows 11, AMD RX 6700 XT.

> **Warning.** Entering the world on a real GPU can currently hang it, and
> Windows then resets the display driver (every monitor goes black for a
> moment). To look further, use a diagnostics build with
> `VICTORY_D3D12_WARP=1`, which renders on the software adapter.

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
| GPU: world | Mesh pipelines, shader-written buffers, descriptor lifetimes, indirect execution (arguments clamped), pipelines made by worker threads and kept on disk | Most of the scene is still missing (untranslated shaders drop draws); frames take seconds; real queries, depth bounds, HDR and scaling are missing |
| Shaders | Console GPU code recompiled to DXIL at first use, cached on disk. Menus: all. World: mesh 8,191 of 8,506, vertex/pixel 10,617 of 12,346, compute 1,643 of 2,539 | 64-bit image atomics, merged geometry stages, the ray-tracing builder's shaders; some compute programs need a GPU with 64-lane waves |
| Textures | Console tiled layouts and detiling for the modes the title uses | Render-target metadata planes |
| Series S mode | Runs through the menus to the loading screen | The world, as above |

## Getting started

Requirements: Windows 10/11 x64, a CPU with AVX2, a Direct3D 12 GPU with
shader model 6.6 (and mesh shaders for the world), Rust (pinned in
`rust-toolchain.toml`).

The title also needs several GiB of available Windows commit at startup.
Check **Committed** under **Task Manager → Performance → Memory**. If
`crashes.log` reports Windows error 1455, close other applications or increase
the Windows paging file size, then retry.

```powershell
cargo xtask install-hooks                # asset-policy check on every commit
copy victory.example.toml victory.toml   # then set your paths (outside the repo)
cargo test --workspace                   # needs no game data
cargo run -p victory -- config           # check your paths
```

Run the title:

```powershell
cargo build --release -p victory
target\release\victory.exe run <game dir>\<the title's exe> --execute
```

The window caption shows the framerate. Saves, settings and `crashes.log`
are kept under the data folder (default `%LOCALAPPDATA%\victory\data`), derived
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
victory inspect <exe|dll> [--imports] [--exports]
victory census [paths...] [--missing] [--strict]
victory lib-exports <import.lib> [--list]
victory shader-info [paths...] [--verbose] [--disasm]
victory translate [paths...] [--validate] [--pipelines] [--list] [--jobs N]
victory run <exe> [--execute] [--unimplemented trap|zero] [--strict]
            [--console series-x|series-s] [-- <title arguments>]
victory config
```

Logging: `VICTORY_LOG=victory_loader=trace,info` (a release build logs
info and above).

## Debugging

A release build has no debugging aids. Build them in with

```powershell
cargo build --profile diag -p victory --features diagnostics   # target\diag\victory.exe
```

| Variable | Effect |
|----------|--------|
| `VICTORY_LOG=<target>=debug,...` | debug and trace output |
| `VICTORY_STACKS=20,60` | every thread's stack at those seconds |
| `VICTORY_WATCH=<rva>,...` | logs a title u32 when it changes |
| `VICTORY_KEYS=30:Enter,35:X` | presses keys at those seconds, focus or not |
| `VICTORY_CRASH_SAVE=<rva>:<len>,...` | saves title memory to the cache folder on a crash |
| `VICTORY_CAPTURE=<dir>` | saves presented frames (powers of two, or `VICTORY_CAPTURE_FRAMES=<list>`) |
| `VICTORY_DUMP_TARGETS=1` | with the two above, saves every render target of those frames |
| `VICTORY_D3D12_DEBUG=1` | host debug layer |
| `VICTORY_D3D12_GBV=1` | with the above, GPU-based validation (bad descriptor use reported, not faulted; slow) |
| `VICTORY_D3D12_DRED=1` | on a GPU failure, the allocation hit and the commands in flight |
| `VICTORY_PIPELINE_WAIT_MS=24` | reduce the per-frame wait for background pipeline creation from the default of 500 ms. A draw can be omitted until its pipeline is ready |
| `VICTORY_SKIP_PIPELINES=<hash>,...` | omit draws and dispatches, including indirect ones, whose pipeline contains a listed 16-digit shader hash |
| `VICTORY_GPU_TIMING=1` | with the above, each list of a submission run and timed on its own; slow lists logged with their slowest commands |
| `VICTORY_D3D12_WARP=1` | host device on the software adapter (slow; cannot reset the driver) |
| `VICTORY_SHADER_LOOP_LIMIT=<n>` | translated shader loops stop after `n` iterations per lane instead of 2^20 (own shader cache entries); if a lower `n` makes world frames much faster, loops hitting the limit are what makes them slow |
| `VICTORY_AUDIO_CAPTURE=<file.wav>` | records what the title plays (our process only) |
| `VICTORY_XMA_DUMP=<dir>` | saves the first 32 XMA voices' context and input |

Every build appends crashes and host GPU failures to `crashes.log` in the
title's data folder.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
