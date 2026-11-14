# HARROW

A grounded, slow, tactical first-person attack-vs-defense shooter built in
Bevy. Private hobby project. Design lives in [DESIGN.md](DESIGN.md); every
external asset is logged in [ASSETS.md](ASSETS.md).

## Build and run

```sh
cargo run                      # main menu: solo, host, join (dev build; first build takes a few minutes)
cargo run -- --solo            # straight into solo on Harrow Keep, the full map
cargo run -- --map section_a   # the first small section of the keep (M2)
cargo run -- --map greybox     # the grey-box movement test area
cargo run --release            # optimised build, use this for judging performance
```

Multiplayer (host, join, dedicated server): see [Multiplayer](#multiplayer).

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
| Lightyear | 0.31.0 | Networking (M8): netcode over UDP, replication (on bevy_replicon 0.45), prediction, interpolation, native inputs. Targets Bevy 0.20 + Avian 0.8. |
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
| 3 | Unique gadget. Lark, Fathom: press to fire a dart / hammer a stake. Auger: hold on a wall or hatch to clamp a ring cutter, tap to start it, hold at a jammed cutter to re-seat it. Mortise: hold on your reinforcement to bolt a backplate charge, tap (anywhere) to fire the latest |
| 4 | Secondary gadget (the one in your loadout). Breach charge: hold to stick it, press again to set it off. Frag / flashbang / smoke / impact charge: press to ready, release to throw (holding a frag cooks it). Razor wire / ballistic cover / proximity alarm: hold to place |
| G | Recon: tap to look through your drones (attackers) or the CCTV cameras (defenders), tap again to come back; attackers hold G to throw another drone |
| Q / E (recon view) | Previous / next drone or camera |
| W A S D, mouse (recon view) | Drive and turn the drone, tilt its camera; pan and tilt a CCTV camera |
| 5 | Equip / stow the defuser (attackers) |
| 1 / 2 | Pick an operator on the selection screen |
| Esc | Pause menu (frees the mouse); left click in game captures it again |
| 4 (selection screen) | Change the picked operator's secondary gadget |
| F1 | Toggle key help |
| F3 | Toggle FPS overlay |
| F4 | Show hit zones and movement capsule |
| F5 | Third-person debug view |
| F12 | Save screenshot to `screenshots/` |

Hold vs. toggle for sprint, crouch, lean and aiming: Settings (below), or
the tuning file.

## Settings

**Settings** (main menu or pause menu) has controls (mouse sensitivity,
invert Y, field of view, hold/toggle for sprint, crouch, lean and aim),
audio (master and effects volume), graphics (a quality preset: Low,
Medium, High, Ultra; or anti-aliasing, ambient occlusion, shadows, bloom
and contact shadows one by one) and display (windowed/fullscreen, V-sync,
FPS counter). Mouse or keyboard: Up/Down (Tab) to move, Left/Right to
change, Enter, Esc to go back. Changes apply at once and are saved when you
leave the page, in `~/.config/harrow/settings.ron` (Windows:
`%APPDATA%\harrow\settings.ron`), next to the menu's `menu.ron`.

Controls you never change follow the tuning file; once set, a setting wins
over it ("Reset to defaults" hands them back). Volumes multiply the tuning
volumes. Automated runs (`--screenshot`, test scripts) ignore the saved
settings so results don't depend on who runs them.

| Preset | Anti-aliasing | Ambient occlusion | Shadows | Bloom | Contact shadows |
|---|---|---|---|---|---|
| Low | FXAA | off | 1024 px, 2 cascades, 60 m, no lantern shadows | off | off |
| Medium | SMAA | low | 2048 px, 3 cascades, 80 m | on | off |
| High (default) | TAA | medium | 2048 px, 4 cascades, 100 m | on | on |
| Ultra | TAA | high | 4096 px, 4 cascades, 120 m | on | on |

**Credits** (main or pause menu) shows `CREDITS.md`.

## Tuning

Movement, look, lighting (sun, sky, haze, fill light, eye adaptation,
tonemapping and grading, bloom, SSAO: see DESIGN.md, "Look and feel"),
per-surface look (texture scale and tint) and
weapons (damage, recoil, spread, reloads, the weapon models and where they sit
on screen) live in `assets/config/tuning.ron`. Edit and save while the game is running and the
changes apply immediately. A mistake in the file is logged and ignored.
Gadgets, drones and CCTV are under `round.gadgets` (timings, damage, counts,
drone speed and battery, camera pan limits).

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
tools/scripts/build_characters.sh --mocap DIR    # where the CMU BVH clips are (see below)
python3 tools/scripts/fetch_character_textures.py  # re-download CC0 cloth/leather detail maps
tools/scripts/build_sky.sh kiara_5_noon_4k.hdr   # HDRI -> assets/sky/kiara_5_noon.ktx2 (sky cubemap; prints the haze colour)
```

On Windows use `tools/scripts/build_kit.ps1`. Kit conventions are documented
at the top of `tools/blender/kit/pieces.py`; the piece list is in DESIGN.md.

```sh
tools/scripts/import_weapons.sh                    # staged CC0 guns -> assets/models/weapons/{tamarisk,acacia}.glb
                                                   # (and cleaned CC0 frag grenade / C4 -> assets/models/gadgets/)
tools/scripts/import_weapons.sh --only acacia      # one weapon
tools/scripts/import_weapons.sh --preview DIR      # render them instead of exporting
tools/scripts/build_weapons.sh                     # scripted blockouts -> assets/models/weapons/*_blockout.glb
tools/scripts/build_weapons.sh --preview sheet.png # render the blockouts to an image
tools/scripts/build_arms.sh                        # first-person arms -> assets/models/characters/arms_*.glb
tools/scripts/build_arms.sh --only defenders --preview DIR  # one team, plus first-person renders per weapon
```

The realistic weapon models are made from CC0 downloads that are not in the
repository: `import_weapons.sh` reads them from the asset staging folder
(`$HARROW_ASSET_STAGING`, default `~/harrow-assets-staging`); see ASSETS.md.
The blockouts are kept as fallbacks (`build_weapons.ps1` on Windows).
Weapon conventions (axes, origin at the grip, `SIGHT`/`MUZZLE` empties,
`MAGAZINE`/`SLIDE` nodes) are at the top of
`tools/blender/weapons/models.py` and `import_models.py`. To use a different
weapon model, change its `model` block in the tuning file (path, scale,
rotation, sight and muzzle points or node names, magazine/slide node names,
the third-person attach transform, the arms clips) and its `pose`; no code.
The first-person arms are posed on the realistic models (rebuild them with
`build_arms.sh` after changing a model or a weapon's `hip` pose a lot; it
needs MPFB, see `--setup` below).

Characters: one config per operator in `tools/blender/characters/operators/`
(body, face, skin, gear; commented). `--setup` changes only your Blender user
configuration (the MPFB extension and its asset folder). On Windows use
`tools/scripts/build_characters.ps1`. See DESIGN.md, "M5 characters".
The locomotion clips are retargeted from CMU motion capture, which is not
in the repository: the build reads `cmu_mocap_bvh_selection/source/*.bvh`
from the asset staging folder (`--mocap DIR`, default `$HARROW_MOCAP`, else
`~/harrow-assets-staging/mocap`). See DESIGN.md, "Realism pass: character
animation", and ASSETS.md, "Motion capture".

Sound effects (`assets/audio/**/*.ogg`) are cut, layered and encoded from
CC0/CC-BY packs with Blender's bundled Python (no ffmpeg needed):

```sh
blender --background --factory-startup --python tools/audio/build_audio.py -- \
    --staging /path/to/harrow-assets-staging/audio --out assets/audio
```

It prints a table of every file (duration, peak, RMS, channels, rate). Slice
times and layers are fixed in the script; the packs and licences are in
ASSETS.md ("Audio"), credits in CREDITS.md. If a bank's variant count
changes, update `BANKS` in `src/audio/mod.rs`.

## Multiplayer

The same game plays solo, hosts, joins, or runs a headless dedicated
server. The server simulates everything (rounds, damage, gadgets,
destruction); each player's own movement is predicted locally, so it
feels instant, and everyone else is shown slightly in the past for
smoothness. Shots are checked by the server against where targets were
on the shooter's screen (lag compensation). Details: DESIGN.md, "M8
networking".

**Everyone must run the same build** (same git commit). The game checks
on connect and refuses a different build with "Version mismatch".

### From the menu

`cargo run` opens the main menu:

- **Play Solo**: the game as before, no network.
- **Host Game**: you play and others can join you. Pick the UDP port
  (default 5888).
- **Join Game**: type the host's address, `host` or `host:port`
  (`192.168.1.20`, `my-pc.lan:5888`, a Tailscale name...). The last
  address is remembered (`~/.config/harrow/menu.ron` on Linux,
  `%APPDATA%\harrow\menu.ron` on Windows).

In game, **Esc** opens the pause menu (Resume, Settings, Credits, Leave to menu, Quit). If
the connection fails or drops you're taken back to the menu with the
reason ("Server not responding", "Version mismatch", "Server full",
"Disconnected...").

### From the command line

```sh
cargo run -- --host            # host on UDP 5888 (or --host 6000)
cargo run -- --connect 192.168.1.20        # join (port 5888), or --connect host:port
cargo run -- --connect my-pc --name Alice  # name shown to the server
cargo run -- --server          # dedicated server, headless, UDP 5888 (or --server 6000)
cargo run -- --server --config assets/config/server.ron   # with a config file
cargo run -- --solo            # straight into solo (skip the menu)
cargo run -- --key 'secret' --connect my-vps   # a server with a shared secret (or HARROW_KEY=...)
cargo run -- --version         # print the build id and exit
```

Any other flag (`--map`, `--no-round`, `--round-test`, `--screenshot`...)
except `--key` also skips the menu and works in every mode (`--menu` forces the menu).
Join late and you watch (dead) until the next round starts; players are
put on the squad with fewer people as they join, taking a bot's place
(bots fill both squads on a host; see "Bots").

### Dedicated server

`--server` runs with no window, GPU, audio or input devices, logs to
stdout (plain text when not a terminal, so journald is happy) and exits
cleanly on Ctrl+C or SIGTERM. The round loop waits until someone joins.
Settings: `--config file.ron` (see `assets/config/server.ron`: name,
port, map, max players, log level, round loop and timers).

A lean server binary that doesn't link the window system, audio or
gamepad libraries:

```sh
cargo build --release --no-default-features --features dedicated
./target/release/harrow --server --config assets/config/server.ron
```

It needs the `assets/` folder (the map's collision comes from the kit and
prop models): run it from the repository, or set `BEVY_ASSET_ROOT` to the
folder that contains `assets/`. A server never loads real textures: every
image (texture files and the pictures inside `.glb` models) is served as
a 1x1 stand-in, so it needs no `textures/` folder and little memory
(`tools/scripts/server_memory.sh` measures it).

`harrow --version` prints the build (version + git commit): players and
server must match.

### On a VPS (Linux server on the internet)

```sh
tools/scripts/package_server.sh                 # dist/harrow-server-<build>-linux-x86_64.tar.gz
tools/scripts/deploy_server.sh you@your-vps     # copy, install, start, show the log
tools/scripts/deploy_server.sh you@your-vps --port 6000
```

`package_server.sh` builds the lean server inside an Ubuntu 22.04
container (podman or docker; your own Rust toolchain and crates are
mounted, so it builds offline), so the binary runs on Ubuntu 22.04+,
Debian 12+ and newer (glibc 2.35+), and packs it with the assets the
server loads, the example settings, a systemd unit, `install.sh` and
[README-VPS.md](deploy/README-VPS.md) (settings, logs, firewall, keys,
security). `deploy_server.sh` copies it over your SSH access, runs
`sudo ./install.sh` there (system user `harrow`, `/opt/harrow-server`,
settings in `/etc/harrow-server/server.ron`, kept on updates; service
`harrow-server`, started at boot) and prints the firewall commands. A
container image is the other option: `deploy/Dockerfile`.

No container tool? Build on the VPS itself (install Rust with rustup,
plus `gcc`), from a checkout of the same commit:
`cargo build --release --no-default-features --features dedicated`, then
`HARROW_BUILD_COMMIT=<commit> ...` if the checkout has no `.git`.

### Firewall and router

The server (host or dedicated) listens on **UDP** port 5888 (or your
`--host`/`--server` port). Clients need nothing opened.

- **Fedora** (firewalld):
  ```sh
  sudo firewall-cmd --add-port=5888/udp                # until reboot
  sudo firewall-cmd --permanent --add-port=5888/udp    # permanently...
  sudo firewall-cmd --reload                           # ...and apply now
  ```
- **Windows**: the first time you host, Windows Defender Firewall asks
  whether to allow `harrow.exe`: tick **Private networks** (and Public
  only if you need it). Or add the rule yourself in an admin PowerShell:
  `New-NetFirewallRule -DisplayName "Harrow" -Direction Inbound -Protocol UDP -LocalPort 5888 -Action Allow`.

**LAN:** join with the host's local IP (`ip -4 addr` / `ipconfig`).

**Internet:** either forward UDP 5888 on the host's router to the host's
local IP and join with the router's public IP, or (easier, no router
changes) put everyone on the same virtual LAN with
[Tailscale](https://tailscale.com) or [ZeroTier](https://www.zerotier.com)
and join with the host's Tailscale/ZeroTier IP or name.

**Keys:** by default connections use a development key built into the
game: it keeps other programs' packets out but anyone with the game can
join. Give a server a shared secret and only players who know it can
join: `key: Some("...")` in the server's config (or `--key` /
`HARROW_KEY` on the server), and players start the game with
`HARROW_KEY='...'` (the menu's Join uses it) or
`--key '...' --connect host`. A wrong key looks like an unreachable
server ("Server not responding"). There are no accounts and no
anti-cheat: play with people you trust (README-VPS.md, "Security").

### Testing on one machine

```sh
tools/scripts/net_test.sh                              # dedicated server + 2 clients, free play
tools/scripts/net_test.sh --latency 80 --jitter 10 --loss 2   # with a bad network
tools/scripts/net_test.sh --round                      # round loop: phases stay in sync
tools/scripts/net_test.sh --gadgets frag               # gadgets: frag smoke flash drone wire cutter (or all)
```

Two game windows open for about 20 s. Logs and screenshots go to
`/tmp/harrow-net-test` (`OUT=dir` to change). The summary shows who
joined and left, the lag-compensation test shots, what each client sees
(`--net-trace`), shots/hits/destroyed cells on every instance, and with
`--round` the round phases of all three.

Network test flags: `--net-latency MS --net-jitter MS --net-loss PCT`
(simulated on everything that instance receives), `--net-trace` (log
state once a second; on clients it also counts other players' private
gadget state, which must stay 0; on a dedicated server a memory
footprint line every 10 s), `--net-shoot-test` (server fires
lag-compensated test rays between players and logs rewound vs live
results), `--look yaw[,pitch]`, `--auto-fire SECONDS`,
`--screenshot-secs S`, `--exit-after S`.

Gadget scenarios (`--gadgets NAME`, on section A): the server sets them
up with `--net-script` (`equip`, `tp`, `reinforce`, `log` by player
name: `src/net/test_script.rs`); each client presses keys with its own
`--gadget-test` script, which goes to the server as real input, and logs
what it sees. Screenshots: `$OUT/<scenario>-*.png`.

## Bots

Solo and hosted games fill both squads with bots: 5v5 with you by
default. They pick operators, reinforce, barricade, place stakes, hold
angles, push sites, breach, plant and disable like players (rules in
DESIGN.md, "M7 bots"; numbers in the `ai` section of the tuning file).

```sh
cargo run -- --bots 5                 # fewer bots (total; teams stay even: 3v3 with you)
cargo run -- --no-bots                # just you
cargo run -- --difficulty hard        # easy | normal | hard (tuning: ai.difficulty)
cargo run -- --server --bots 10       # a dedicated server only gets bots when asked
cargo run -- --ai-debug               # draw the nav grid, bot paths, goals and aim lines
cargo run -- --ai-player              # a bot drives your character too (watch it play)
cargo run -- --ai-test                # automated bot-only match: logs kills, plants, stuck bots, fps
cargo run -- --ai-test --ai-matches 3 --ai-speed 2 --ai-shots shots/   # 3 matches at 2x, screenshots
cargo run -- --ai-test --ai-spectate --ai-cam front   # chase camera on a bot, from the front
cargo run -- --ai-test --ai-site B   # attackers always go for site B (testing)
cargo run -- --ai-test --ai-no-reinforce   # defenders don't reinforce (to see breach charges used)
cargo run -- --no-round --ai-player --ai-breach-test --ai-spectate   # a bot breaches into site A
cargo run -- --no-round --ai-navtest  # plan every spawn-to-site path, log them, quit
cargo run -- --no-round --ai-navtest --ai-navdump 0.8,-7.6,0.8,0.4   # dump grid columns on a line
```

Bots are off by default in the other automated test modes
(`--round-script`, `--weapon-test`, `--m4`, `--no-round`, `--dummies`,
`--gadget-test`, ...); pass `--bots N` to have them anyway. The kill feed
(left, under the FPS counter) and your team's callouts ("Enemy spotted:
site A (storeroom)") under it.

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
cargo run -- --window 1920x1080 --quality ultra  # window size; graphics preset (low/medium/high/ultra)
cargo run -- --menu --menu-page settings --screenshot s.png  # open a menu page (settings, credits; pause in game)
tools/scripts/look_shots.sh /tmp/look --no-vsync # six reference views + contact sheet + fps (look checks)
cargo run -- --team defenders --operator fathom  # your starting side and operator (solo testing)
cargo run -- --no-round --operator lark          # free play: no round loop; --operator still gives that gadget
cargo run -- --round-test                        # short round timers and a short match (round.test_timers)
cargo run -- --round-script "action@0.5: hold; planted@1: shot out.png; planted@2: quit"
                                               # scripted automated run; commands in src/round/script.rs
cargo run -- --show-all-intel                    # show both teams' gadget reveals (testing)
cargo run -- --secondary frag                    # your secondary gadget: breach, frag, flash, smoke, wire, cover, impact, alarm
cargo run -- --no-round --operator auger --gadget-test "1 tp 7 0 -2.6 0 0; 1.5 hold3 2; 4 tap3; 15 shot cut.png; 15.5 quit"
                                               # scripted gadget checks; commands in src/gadgets/test_script.rs
cargo run -- --sites 2                           # always use site pair 2 (Harrow Keep has 4; see DESIGN.md)
cargo run -- --attack-from 3                     # attackers start from the pair's 3rd spawn area
cargo run -- --no-batch                          # don't merge static map meshes (performance comparison)
cargo run -- --dummies                         # training dummies in the keep (courtyard, behind plaster, behind stone)
cargo run -- --weapon-test                     # scripted weapon checks; logs PASS/FAIL, exit code 1 on failure
cargo run -- --weapon sidearm --ads            # hold a weapon / aim down sights
cargo run -- --hold-fire                       # hold the trigger
cargo run -- --fire-at 90 --reload-at 120      # pull the trigger / press reload at a fixed tick (64 per second)
cargo run -- --screenshot-on-hit out.png --dummies --fire-at 90  # screenshot just after your first hit, then quit
cargo run -- --log-sounds                      # log every sound: event, file, surface, 2D/3D, distance, volume
cargo run -- --log-sounds --no-round --spawn 8,0,18,0,0 --m4 "0.5 move 0 1; 5 quit"
                                               # walk from the desert sand onto the courtyard stone, logging steps
cargo run -- --play-sounds --m4 "..."          # play sounds in an automated run too (to listen to a scripted scene)
cargo run -- --no-viewmodel                    # don't draw the weapon in your hands (look/performance comparisons)
cargo run -- --lineup                          # the four operators in the courtyard, cycling animations
cargo run -- --lineup-state crouch_walk        # ... all holding one state (names in src/characters/lineup.rs)
cargo run -- --lineup-crowd --no-vsync         # 12 operators, for checking what a full match costs
cargo run -- --lineup --lineup-weapons         # ... holding the real rifle (or `--lineup-weapons sidearm`)
```

Used for automated checks: e.g. spawn at the stairs, auto-move forward, and
screenshot/log where the player ends up. Automated runs (`--screenshot`,
`--weapon-test`, `--screenshot-on-hit`, `--m4`, `--round-script`) don't
grab the mouse and play no sounds; add `--log-sounds` to see which sounds
would have played (that's how audio is checked in automated runs).

Maps: `--map keep` (default) is the full Harrow Keep; `--map section_a` the
small M2 section; `--map greybox` the movement test area (no round loop).
Test flags whose positions were written for section A (`--weapon-test`,
`--dummies`, `--lineup*`, `--m4`, `--gadget-test`, `--screenshot-on-hit`)
load section A unless `--map` is given.

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

Gadget test script (`--gadget-test`, same `<seconds> <command>` format;
`src/gadgets/test_script.rs` lists every command): `op`, `secondary`, `tp`,
`look`, `aimat`, `move`, `stance`, `hold3`/`tap3`, `hold4`/`tap4`, `g`,
`next`/`prev`, `drive`, `dlook`, `dface`, `reinforce`, `hardbreach`, `fire`
(a real weapon shot), `shoot`, `aimblade`/`shootblade`, `dummy`, `walker`,
`walker-move`, `explode`, `lamp`, `reset` (a new-round reset), `log`,
`shot`, `quit`. With the round loop, add `--round-script "action@99: log x"`
to keep the operator selection open (otherwise automated runs auto-pick).

```sh
# backplate vs ring cutter: reinforce, bolt a backplate, clamp and start a cutter, fire the backplate
cargo run -- --no-round --operator auger --gadget-test "1.5 tp 7 0 -5.6 180 0; 1.8 reinforce; 2 op mortise; 2.2 hold3 2; 4.6 op auger; 4.7 tp 7 0 -2.4 0 -12; 5 hold3 2; 7.2 tap3; 9 op mortise; 9.2 tap3; 9.6 log; 10 quit"
# drones: throw one, drive it, come back
cargo run -- --no-round --operator lark --gadget-test "1.5 g; 3 drive 0 1; 4.5 drive 0 0; 5 shot drone.png; 5.5 g; 6 quit"
```

## Layout

```
src/        game code, one module (and Bevy plugin) per area
assets/     models, textures, audio, shaders loaded by the game
tools/      Blender Python scripts and build helpers (net tests, server packaging and deploy)
deploy/     dedicated server on a VPS: systemd unit, install script, guide, Dockerfile
```
