# HARROW

A grounded, slow, tactical first-person attack-vs-defense shooter built in
Bevy. Private hobby project. Design lives in [DESIGN.md](DESIGN.md); every
external asset is logged in [ASSETS.md](ASSETS.md).

## Build and run

```sh
cargo run                      # Harrow Keep (dev build; first build takes a few minutes)
cargo run -- --map greybox     # the grey-box movement test area
cargo run --release            # optimised build, use this for judging performance
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
| MPFB | 2.0.18 | Character bodies (Blender extension, installed per user by `build_characters.sh --setup`). |

## Controls

| Key | Action |
|---|---|
| Mouse | Look |
| W A S D | Move |
| Left Shift (hold) | Sprint (forward only; stands you up, cancels lean) |
| C | Crouch (toggle) |
| Z | Prone (toggle) |
| Q / E | Lean left / right (toggle) |
| Esc / left click | Release / capture the mouse |
| F1 | Toggle key help |
| F3 | Toggle FPS overlay |
| F4 | Show hit zones and movement capsule |
| F5 | Third-person debug view |
| F12 | Save screenshot to `screenshots/` |

Hold vs. toggle for sprint, crouch and lean is set in the tuning file.

## Tuning

Movement, look, lighting and per-surface look (texture scale and tint) live in
`assets/config/tuning.ron`. Edit and save while the game is running and the
changes apply immediately. A mistake in the file is logged and ignored.

## Asset pipeline

```sh
tools/scripts/build_kit.sh                       # Blender -> assets/models/kit/*.glb
tools/scripts/build_kit.sh --only stone_wall_2m  # rebuild one piece
tools/scripts/build_kit.sh --preview sheet.png   # render all pieces to an image
python3 tools/scripts/fetch_textures.py          # re-download CC0 textures (Poly Haven)
tools/scripts/build_characters.sh --setup        # once: install MPFB 2.0.18 + MakeHuman CC0 assets
tools/scripts/build_characters.sh                # Blender -> assets/models/characters/*.glb
tools/scripts/build_characters.sh --only lark    # rebuild one operator
tools/scripts/build_characters.sh --preview DIR  # also render each operator and some poses to DIR
```

On Windows use `tools/scripts/build_kit.ps1`. Kit conventions are documented
at the top of `tools/blender/kit/pieces.py`; the piece list is in DESIGN.md.

Characters: one config per operator in `tools/blender/characters/operators/`
(body, face, skin, gear; commented). `--setup` changes only your Blender user
configuration (the MPFB extension and its asset folder). On Windows use
`tools/scripts/build_characters.ps1`. See DESIGN.md, "M5 characters".

## Developer flags

```sh
cargo run -- --screenshot out.png              # screenshot after 120 frames, then quit
cargo run -- --screenshot out.png --screenshot-frames 300
cargo run -- --spawn 11,0,4,0,-10              # spawn at x,y,z facing yaw,pitch (degrees)
cargo run -- --auto-move 0,1,sprint            # hold strafe,forward[,sprint]
cargo run -- --stance crouch --lean left       # hold a stance / lean
cargo run -- --third-person --hitboxes         # start with debug views on
cargo run -- --trace-player                    # log position 4x per second, and every footstep
cargo run -- --no-vsync                        # uncapped frame rate (performance checks)
cargo run -- --lineup                          # the four operators in the courtyard, cycling animations
cargo run -- --lineup-state crouch_walk        # ... all holding one state (names in src/characters/lineup.rs)
```

Used for automated checks: e.g. spawn at the stairs, auto-move forward, and
screenshot/log where the player ends up. Automated runs don't grab the mouse
and don't play player sounds.

## Layout

```
src/        game code, one module (and Bevy plugin) per area
assets/     models, textures, audio, shaders loaded by the game
tools/      Blender Python scripts and build helpers
```
