# HARROW

A grounded, slow, tactical first-person attack-vs-defense shooter built in
Bevy. Private hobby project. Design lives in [DESIGN.md](DESIGN.md); every
external asset is logged in [ASSETS.md](ASSETS.md).

## Build and run

```sh
cargo run              # dev build (first build takes a few minutes)
cargo run --release    # optimised build, use this for judging performance
```

Use `cargo build -j 4` (or any number) to leave CPU cores free while compiling.

### Requirements

| Tool | Version | Notes |
|---|---|---|
| Rust | 1.97.1 or newer | Install with [rustup](https://rustup.rs). Minimum required by Bevy 0.20. |
| Git LFS | any | Stores models, textures and audio. Run `git lfs install` once after installing. |
| Blender | 5.2.4 LTS | Only needed for the asset pipeline (`tools/blender/`). |

**Linux (Fedora):** Bevy needs `alsa-lib-devel systemd-devel wayland-devel
libxkbcommon-devel vulkan-loader` (all present on the dev machine).
Runs natively on Wayland.

**Windows:** install Rust with the MSVC toolchain (rustup's default) and the
"Desktop development with C++" workload from Visual Studio Build Tools. No other
system libraries are needed.

### Blender

Blender is installed per-user (no root) at
`~/.local/opt/blender-5.2.4-linux-x64/`, with `~/.local/bin/blender` linked to it.
Pipeline scripts call `blender`, or `$HARROW_BLENDER` if that is set. On Windows,
set `HARROW_BLENDER` to the full path of `blender.exe`.

## Pinned versions

Bevy's API changes between releases, so everything is pinned with `=` in
`Cargo.toml`. Upgrade only as a deliberate step, all crates together.

| Component | Version | Why this one |
|---|---|---|
| Bevy | 0.20.1 | Latest stable (2026-10-28), supported by every crate below. |
| Avian (`avian3d`) | 0.8.0 | Physics. Chosen over Rapier because Lightyear has first-party Avian integration (`lightyear_avian3d`), which we need for networked prediction. |
| bevy_enhanced_input | 0.27.2 | Input. Context-based actions (on foot / gadget placement / menus) and Lightyear supports it directly (`input_bei` feature). |
| Lightyear | 0.31.0 *(planned, M8)* | Networking. Not added yet; 0.31.0 targets Bevy 0.20 + Avian 0.8 + bevy_enhanced_input 0.27. |
| Blender | 5.2.4 LTS | Asset pipeline. LTS chosen over 5.3.0, which was only days old. |
| MPFB | 2.0.18 *(planned, M5)* | Character bodies. Supports Blender 4.2+. |

## Controls

No gameplay controls yet (M1 adds the first-person controller).

| Key | Action |
|---|---|
| F3 | Toggle FPS overlay |
| F12 | Save screenshot to `screenshots/` |

## Developer flags

```sh
cargo run -- --screenshot out.png              # screenshot after 120 frames, then quit
cargo run -- --screenshot out.png --screenshot-frames 300
```

Used for automated visual checks of a scene.

## Layout

```
src/        game code, one module (and Bevy plugin) per area
assets/     models, textures, audio, shaders loaded by the game
tools/      Blender Python scripts and build helpers
```
