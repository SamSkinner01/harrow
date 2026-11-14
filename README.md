# HARROW

A grounded, slow, tactical first-person attack-vs-defense shooter built in
Bevy. Private hobby project. Design lives in [DESIGN.md](DESIGN.md); every
external asset is logged in [ASSETS.md](ASSETS.md).

## Build and run

```sh
cargo run                      # Harrow Keep, the full map (dev build; first build takes a few minutes)
cargo run -- --map section_a   # the first small section of the keep (M2)
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
| Left mouse | Fire (hold for full auto) |
| Right mouse | Aim down sights (hold; toggle option in the tuning file) |
| R | Reload |
| 1 / 2, mouse wheel | Primary (TAMARISK rifle) / sidearm (ACACIA pistol) |
| V | Melee (breaks soft walls, hatches from above, barricades) |
| F (hold) | Plant the defuser in a site (attackers) / disable it (defenders). Defenders elsewhere: reinforce the wall or hatch you look at (5 s), or barricade an empty doorway/window (2 s) |
| 3 | Unique gadget (operator ability) |
| 4 (hold, then press) | Breach charge: hold to stick it on a soft wall, floor or barricade, press again to set it off |
| 5 | Equip / stow the defuser (attackers) |
| 1 / 2 | Pick an operator on the selection screen |
| Esc / left click | Release / capture the mouse |
| F1 | Toggle key help |
| F3 | Toggle FPS overlay |
| F4 | Show hit zones and movement capsule |
| F5 | Third-person debug view |
| F12 | Save screenshot to `screenshots/` |

Hold vs. toggle for sprint, crouch, lean and aiming is set in the tuning file.

## Tuning

Movement, look, lighting, per-surface look (texture scale and tint) and
weapons (damage, recoil, spread, reloads, the weapon models and where they sit
on screen) live in `assets/config/tuning.ron`. Edit and save while the game is running and the
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
python3 tools/scripts/fetch_character_textures.py  # re-download CC0 cloth/leather detail maps
```

On Windows use `tools/scripts/build_kit.ps1`. Kit conventions are documented
at the top of `tools/blender/kit/pieces.py`; the piece list is in DESIGN.md.

```sh
tools/scripts/build_weapons.sh                     # Blender -> assets/models/weapons/*.glb
tools/scripts/build_weapons.sh --only tamarisk     # rebuild one weapon
tools/scripts/build_weapons.sh --preview sheet.png # render the weapons to an image
```

(`build_weapons.ps1` on Windows.) Weapon conventions (axes, origin at the
grip, `SIGHT`/`MUZZLE` empties, `MAGAZINE`/`SLIDE` nodes) are at the top of
`tools/blender/weapons/models.py`. To use a different weapon model, change
its `model` block in the tuning file (path, scale, rotation, sight and muzzle
points or node names, magazine/slide node names) and its `pose`; no code.

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
cargo run -- --team defenders --operator fathom  # your starting side and operator (solo testing)
cargo run -- --no-round --operator lark          # free play: no round loop; --operator still gives that gadget
cargo run -- --round-test                        # short round timers and a short match (round.test_timers)
cargo run -- --round-script "action@0.5: hold; planted@1: shot out.png; planted@2: quit"
                                               # scripted automated run; commands in src/round/script.rs
cargo run -- --show-all-intel                    # show both teams' gadget reveals (testing)
cargo run -- --sites 2                           # always use site pair 2 (Harrow Keep has 4; see DESIGN.md)
cargo run -- --attack-from 3                     # attackers start from the pair's 3rd spawn area
cargo run -- --no-batch                          # don't merge static map meshes (performance comparison)
cargo run -- --dummies                         # training dummies in the keep (courtyard, behind plaster, behind stone)
cargo run -- --weapon-test                     # scripted weapon checks; logs PASS/FAIL, exit code 1 on failure
cargo run -- --weapon sidearm --ads            # hold a weapon / aim down sights
cargo run -- --hold-fire                       # hold the trigger
cargo run -- --fire-at 90 --reload-at 120      # pull the trigger / press reload at a fixed tick (64 per second)
cargo run -- --screenshot-on-hit out.png --dummies --fire-at 90  # screenshot just after your first hit, then quit
cargo run -- --dump-weapon-sounds sounds/      # also write the synthesised weapon sounds as .wav files
cargo run -- --no-viewmodel                    # don't draw the weapon in your hands (look/performance comparisons)
cargo run -- --lineup                          # the four operators in the courtyard, cycling animations
cargo run -- --lineup-state crouch_walk        # ... all holding one state (names in src/characters/lineup.rs)
cargo run -- --lineup-crowd --no-vsync         # 12 operators, for checking what a full match costs
```

Used for automated checks: e.g. spawn at the stairs, auto-move forward, and
screenshot/log where the player ends up. Automated runs (`--screenshot`,
`--weapon-test`, `--screenshot-on-hit`) don't grab the mouse and don't play
player or weapon sounds.

Maps: `--map keep` (default) is the full Harrow Keep; `--map section_a` the
small M2 section; `--map greybox` the movement test area (no round loop).
Test flags whose positions were written for section A (`--weapon-test`,
`--dummies`, `--lineup*`, `--m4`, `--screenshot-on-hit`) load section A
unless `--map` is given.

The round loop (maps with bomb sites) starts with operator selection. In
`--screenshot` runs everyone is auto-picked; `--round-script` runs pick
with `pick N` or let the timer run out, can spawn stand-in characters
(`walker`), plant (`plant A`), place gadgets (`dart`, `stake`), shoot,
take screenshots (`shot`) and quit, and log the round state once a second.

Destruction test script (`src/destruction/debug.rs` lists every command):
a timed list of `<seconds> <command>` separated by `;`.

```sh
# shoot the plaster wall, take a screenshot, quit
cargo run -- --no-round --spawn 7,0,-2.3,0,0 --m4 "0 lamp 7.5 2.2 -2 3e7; 1 fire bullet 40 12; 3 shot out.png; 3.5 quit"
# reinforce as a defender (hold F 5.6 s), shoot it, then hard-breach it
cargo run -- --no-round --spawn 7,0,-2.6,0,0 --m4 "0.5 team defenders; 1 interact 5.6; 7 fire bullet 30 10; 9 hardbreach 0.5; 10 log; 10.5 quit"
```

Use `--no-round` for free play (otherwise the round loop respawns and
locks the player). Commands: `team`, `look`, `tp`, `move`, `stance`, `fire bullet|pellet`,
`melee`, `interact`, `gadget`, `explode`, `hardbreach`, `reinforce`, `lamp`,
`dummy`, `log` (surfaces, player, rebuild cost, FPS), `shot`, `quit`.

## Layout

```
src/        game code, one module (and Bevy plugin) per area
assets/     models, textures, audio, shaders loaded by the game
tools/      Blender Python scripts and build helpers
```
