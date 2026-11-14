# HARROW design doc

Living document. Sections marked **PROPOSED** are not agreed yet.

## Feel target

Tense, slow, deliberate: holding angles, leaning, breaching, scouting before
committing, gadgets that change how a room plays.

**Reference:** Rainbow Six Siege as it played around Y3S4 (Wind Bastion,
Dec 2018). We match its *rules, pacing and counter-play structure* as closely
as possible: one life per round, soft vs. reinforced walls, hard breach vs.
denial, prep-phase scouting.

**Originality line:** names, operators and their gadgets, map layouts, UI,
logos, audio and art are all our own. A gadget may fill the same *role* as one
in the reference (e.g. "opens reinforced walls"), but must work differently.

## Mode and round flow (decided 2026-11-13)

- 5v5, attack vs. defense, one life per round. Bot-filled until multiplayer.
- **Prep phase:** defenders reinforce walls and hatches, place barricades and
  gadgets; attackers scout with recon throwbots.
- **Action phase:** attackers breach and push the objective.
- **Objective:** defenders protect one of two bomb sites. Attackers plant a
  defuser device and must defend it until it completes. Defenders win by
  disabling it, wiping the attackers, or running out the clock.
- Starting timings (tune in config): prep 45 s, action 3:00, plant 7 s,
  defuser run time 45 s, disable 7 s.
- Round end: objective, team wipe, or timer.

## Movement (M1)

All numbers live in `assets/config/tuning.ron`; these are the rules.

- Stances: stand / crouch / prone. You can't stand up (or crouch from prone)
  unless the full height fits; pressing the key again cancels the request.
- Lean left/right: camera tilts and shifts sideways; the head and upper torso
  hit zones move with it, so leaning exposes you. Leaning stops at walls. No
  lean while prone.
- Sprint: forward only. Starting a sprint stands you up and straightens a
  lean; leaning stops a sprint.
- No jumping (matches the feel target). **PROPOSED:** a vault action for
  low walls and windows, later.
- Footsteps have a loudness tier: sprint 1.0, walk 0.5, crouch-walk 0.12,
  prone 0.04. Bots and detection gadgets (e.g. Fathom's stakes) will use
  these; crouch-walking is the quiet way to move.
- Level rule: characters don't step up ledges, so every staircase and
  threshold needs a ramp collider (visual steps on top).

## Controls (key map)

Every module binds its own keys; this is the shared map so they don't clash.
Hold/toggle options live in the tuning file.

| Key | Action | Owner |
|---|---|---|
| W A S D, mouse | Move, look | player (M1) |
| Shift | Sprint | player |
| C / Z | Crouch / prone | player |
| Q / E | Lean left / right | player |
| Left mouse | Fire | weapons (M3) |
| Right mouse | Aim down sights | weapons |
| R | Reload | weapons |
| 1 / 2 | Primary / sidearm | weapons |
| V | Melee | destruction (M4) |
| 3 | Unique gadget (operator ability) | gadgets (M6) |
| 4 | Secondary gadget (breach charge, razor wire...) | gadgets / destruction |
| 5 | Defuser (attackers) | round (M6) |
| F (hold) | Interact: reinforce, barricade, plant/disable defuser | destruction / round |
| Mouse wheel | Cycle weapon | weapons |
| Esc | Free mouse | player |
| F1-F12 | Debug and help keys | debug / ui |

Input code: each module creates its own input context entity (see
`src/player/input.rs` for the pattern) and copies values into an intent
component on the player, and only reads input while `input_captured`.

## Destruction (decided 2026-11-13: match the reference's Y3S4 rules)

Fortress logic: thick stone and rammed earth are **indestructible**; plaster,
lath and timber partitions are **soft**.

| Surface | Bullets | Shotgun | Melee | Explosives | Hard breach |
|---|---|---|---|---|---|
| Soft wall | small hole per hit; sustained fire opens sightlines | large hole | head-sized hole; 3–4 hits make a vault/crouch hole | large opening | opens |
| Reinforced wall | no effect, no penetration | no effect | no effect | no effect | **only way in** |
| Soft floor / ceiling | small holes (vertical sightlines both ways) | larger holes | no | opens a drop-through hole from above | — |
| Hatch | small holes | breaks boards | repeated melee from above opens it | opens | opens if reinforced |
| Barricade (door/window) | breaks boards | breaks boards | 2 hits clear a door | clears | — |
| Stone / exterior / stairs / pillars | no | no | no | no | no |

- Soft walls keep their outer frame and a bottom lip (you vault through).
  Internal timber studs survive bullets and melee; only explosives break them.
- Bullets penetrate soft surfaces with damage falloff.
- Debris is cosmetic: it never blocks movement and fades out.
- Each defender has **2 reinforcements** (walls or hatches), ~5 s to install,
  interruptible, during prep or action phase. Some soft walls are marked
  non-reinforceable.
- Defenders can barricade any doorway/window (unlimited, ~2 s).

**Implementation (decided 2026-11-13):** soft surfaces are a runtime grid of
~10 cm cells, with the mesh rebuilt around removed cells (cheap: ~300 cells
per 1 x 3 m wall), so bullets, melee and explosives can open holes anywhere.
The Blender kit provides the intact pieces (marked soft), materials and debris;
pre-fractured meshes are used for props and barricades only.

## M4 implementation notes (destruction as built)

Code in `src/destruction/`; every number is in the `destruction` section of
`assets/config/tuning.ron`. The rules above are unchanged; this is how they
are implemented.

**Cells.** Each soft kit piece (plaster walls 1 m / 2 m / door / window,
wooden floor tiles, the hatch tile and cover, door and window barricades) is
replaced at load by a grid of 10 cm cells. Grid lines also fall exactly on
openings, studs and the frame, so cells near those are narrower. A 2 m wall
is about 450 cells, a floor tile about 400. A cell has layers: *skin*
(plaster or boards), *core* (timber studs, joists, battens), *frame* (never
breaks), and on reinforced surfaces *plate* and *brace*. The mesh is the
cells merged into rectangles (with hole edges and wall thickness, the kit's
metre UVs and library materials); the collider is a compound of boxes over
the same rectangles. Only a surface that changed is rebuilt: its collider on
the same fixed tick (holes are walkable and shootable from the next tick),
its mesh once per frame.

**Layout.**
- Walls: 5 cm frame strips at the posts, a 6 cm bottom lip (low enough to
  walk over without a vault) and a top plate never break. Studs (6 cm) stand
  beside openings and are spaced so no bay is wider than 0.7 m, so a 2 m wall
  has one in the middle. (The brief said about 0.5 m; 0.7 m leaves 0.67 m
  bays, so a melee crouch hole fits between studs, since a person needs
  0.6 m.) Door and window walls can't be reinforced; map code can mark others
  with `NotReinforceable`.
- Floors: 4 cm joists at the tile edges and at most 0.5 m apart, so bullets
  and pellets make sightlines but never a drop-through hole. Around the
  hatch, a 10 cm timber ring and the two trimmer joists are frame, so a
  blast never leaves the hatch floating.
- Hatch cover: boards with a cross of battens under them.
- Barricades: the kit's plank layout, planks breaking in 0.3 m pieces, two
  battens behind.

**Impacts.** Every change comes from a `SurfaceImpact` (or a hard breach or
reinforcement), and the result is deterministic: the only randomness is a
hash of the piece's placement, its impact count and the cell.

| Impact | Wall | Floor | Hatch cover | Barricade |
|---|---|---|---|---|
| Bullet | the cell hit, plus a 25% chance for each neighbour; studs and frame unaffected; a hit on a hole's edge chips it | same | same | same (planks) |
| Pellet | 7 cm disc per pellet (pellets overlap into one big hole) | same | same | same |
| Melee | hits within 0.5 m of each other grow one hole (radius 0.14 / 0.20 / 0.26 / 0.32 m); from the 3rd hit it also opens a 0.75 x 1.3 m crouch hole down to the lip; studs stay | nothing | 3rd hit from above breaks it | 1st hit knocks out a 0.5 m chunk, 2nd clears it |
| Explosion (radius r) | disc of radius sqrt(r² - d²), d = blast distance from the wall, studs included; if it reaches within 0.5 m of the lip, the wall below the centre is blown out to the lip (walk-through) | disc (drop-through) | breaks | clears |
| Hard breach | cuts skin, studs, frame and plate in a disc | same | same | same |

Reinforced surfaces ignore everything except hard breaches. A barricade
falls apart once less than 30% of its planks are left.

**Explosions** (the `Explosion` message, also meant for M6's grenades and
impact charges) open every soft surface within the breach radius (a charge
stuck to a surface opens only that surface) and hurt people within the
damage radius: full damage within 25% of it, then falling off with the
square of the distance. Line of sight is checked to each hit zone: stone or a
reinforced wall in between blocks it; each soft wall halves it. The most
exposed zone counts.

**Melee (V).** A 12 cm sphere swept 1.7 m from the eyes (enough to reach a
hatch at your feet), 0.8 s cooldown, 35 damage to people through their hit
zones. Swinging into an existing hole still catches its edges, so it keeps
growing. Feedback is a short camera jab.

**Reinforcing (hold F, defenders).** 5 s looking at a reinforceable wall or
hatch within 1.6 m; moving, looking away, letting go or the round loop
closing the gate cancels it. 2 per defender. The plate goes on the
defender's side and covers existing holes; the piece becomes
`SurfaceKind::Hard`, so bullets stop.

**Barricading (hold F, defenders).** 2 s looking through an empty doorway or
window (stone or plaster) within 2 m. Unlimited; replaces a broken one.

**Breach charge (key 4).** Hold 1 s while looking at a soft wall, a floor or
hatch (from above) or a barricade within 2 m; press 4 again to detonate. One
out at a time, 3 per attacker. On walls it always sits 1.15 m up so the
0.9 m-radius hole is walk-through. 120 damage point blank, 3 m damage
radius. It falls off if its surface breaks.

**Hard breach** (the `HardBreach` message) is the API for M6's Ring Cutter
(radius about 0.5 m) and Backplate Charge (fist-sized).

**Round loop.** Reinforcing and barricading are allowed in prep, action and
planted (and in free play); breach charges whenever attackers may use
gadgets (not during prep). Every new round restores the map: walls, floors
and hatches intact and unreinforced, the map's barricades back up,
defenders' barricades and placed charges gone, 2 reinforcements per
defender, and 3 breach charges for anyone whose secondary gadget is the
soft-breach charge (in free play, every attacker). While a defender is
planting or disabling the defuser, F belongs to the round loop.
Destruction is heard by gadgets and bots as `Noise`: explosions 6,
breaking a hatch or barricade 1.5, melee hits 1, finishing a reinforcement
0.8 (footstep scale).

**Debris** is cosmetic: chunks in the surface's material (each casts one ray
per frame while moving; no colliders) and camera-facing dust puffs, capped at
250 chunks and 60 puffs, oldest first.

**Performance** (dev build): rebuilding one surface takes about 0.05 ms for
the mesh (worst seen 0.14 ms) and 0.03 ms for the collider. With roughly 400
impacts spread over every soft surface in the keep, the frame rate stayed at
about 350 fps uncapped (330-370 before). Watch: very ragged surfaces mean
more collider boxes (still well under 100 per surface), and moving debris
costs one ray each per frame (capped).

**Not done yet:** sounds; holes are blocky 10 cm steps (a decal or edge
jitter could soften them); crates aren't destructible; no vault action, so
melee holes are crouch holes.

## Operator structure (approved 2026-11-13)

- Armour/speed split: 1/3, 2/2 or 3/1.
- Loadout: primary + sidearm, **one unique gadget**, and a choice of one shared
  secondary gadget.
- Shared attacker secondaries: soft-breach charge (x3, remote), frag,
  flashbang, smoke.
- Shared defender secondaries: razor wire, ballistic cover (deployable
  waist-high shield), impact charge (thrown, opens soft surfaces),
  proximity alarm.
- Recon: each attacker has 2 recon throwbots (two-wheeled, can't jump, climbs
  low steps, can be shot). Defenders get fixed CCTV cameras at map-defined spots.

## Starter operators (approved 2026-11-13; callsigns are placeholders)

### AUGER: attacker, hard breacher (armour 2 / speed 2)
**Ring Cutter** x2. Clamps to any wall or hatch, reinforced or soft, and cuts a
1 m circular plug in ~10 s. Very loud: the whole wall shudders and sheds dust,
audible two rooms away. Halfway through, the blade breaks through on the
defender side and can be shot; a hit jams it and Auger must re-seat it (4 s,
exposed). When the cut finishes, the plug falls inward and knocks down anyone
directly behind it.
*Different from the reference:* not an instant charge or a hand torch. It's a
slow machine that defenders can fight through the wall while it works.

### LARK: attacker, scout (armour 1 / speed 3)
**Sensor Darts** x4, fired from a wrist launcher, stick to any surface. For 8 s
each dart listens: every sound a defender makes within 6 m (footsteps above
crouch-walk, reloads, gadget deploys, doors) shows to all attackers as a
ripple marker at its source, through walls and floors. Defenders hear a sharp
hiss when one lands nearby and can shoot it.
*Counter-play:* freeze, crouch-walk, delay the reload.

### MORTISE: defender, anchor / anti-breach (armour 3 / speed 1)
**Backplate Charges** x3. Bolted to the *defender* side of a reinforced wall or
hatch. Inert until Mortise triggers it (remote, from anywhere); it then blasts
outward and destroys any attacker gadget within ~1 m on the far side. Cost: it
punches a fist-sized hole in your own reinforcement, giving attackers a
permanent sightline.
*Different from the reference:* one-shot and manual, and it weakens the wall
it protects. Timing it against a slow Ring Cutter is the core mind game.
(The brief's "blocks and damages anyone touching it" idea moves to the shared
razor wire.)

### FATHOM: defender, roamer (armour 1 / speed 3)
**Seismic Stakes** x3, hammered into any floor. Passive: any attacker walking
or running within 6 m, including the floor directly above or below, sends a
location ping to all defenders with an audio tick. Crouch-walking doesn't
trigger it. Stakes are visible and can be shot. Loud events nearby (Ring
Cutter, explosions) saturate a stake for 10 s, so a breach also masks
footsteps.

## M6 round loop (implemented 2026-11-13)

Rules as built; every number is in the `round` section of the tuning file.
Code: `src/round/` (loop, sites, defuser, operators, HUD) and
`src/gadgets/` (darts, stakes, intel).

**Match.** Two squads (yours and the other) play rounds; each round one
squad attacks and the other defends. Sides swap every few rounds (tunable;
every round by default). First squad to the target number of round wins
takes the match; after the result banner a new match starts. No overtime
yet.

**Round.** Operator selection (keys 1/2; auto-pick of your last or the
first operator when the timer runs out) -> prep -> action -> planted (if the
defuser goes down) -> round end. At the start of every round everyone is
healed and revived, players go to their side's spawns, deployed gadgets and
defusers are cleared, and a `RoundReset` message goes out for other systems.
Nobody can walk during selection.

- Prep: defenders set up and may use gadgets. Attackers wait at their spawn
  (tunable) because recon drones aren't built yet; no attacker gadgets.
- Action: attackers may use gadgets and plant. The clock running out
  without a plant is a defender win (even mid-plant).
- Planted: the clock is replaced by the defuser's timer. It completing is
  an attacker win; defenders disabling it is a defender win.
- Wipes: if every member of a team is dead, the other team wins, except
  that a wiped attacking team does not lose once the defuser is planted
  (defenders must still disable it). A team with no members at all (solo
  play) is never "wiped". Anything with a `Team` and `Health` counts as a
  member (players, bots, training dummies). Dead characters can't walk.

**Bomb sites.** Site A is the ground-floor storeroom, site B the upper-floor
east room. Each is a trigger volume (your feet must be inside), marked with
painted ochre floor lines and a HUD label with distance. Attackers spawn
outside the courtyard gate, defenders inside the building on both floors.

**Defuser.** One attacker carries it (handed out at prep; you first if you
attack). Key 5 equips/stows it; holding F inside a site plants it (and
equips it). Planting is interrupted, with progress lost, by letting go of F,
leaving the site, dying, or the phase ending; you can't walk while
planting. Defenders disable it by holding F next to it (one at a time, same
interruptions, also can't walk). A carrier who dies drops it; a living
attacker who walks over it picks it up. The device is an original
twin-canister unit with an amber read-out and a blinking mast; it beeps
every second, faster near the end.

**Operators.** Selection gives each character its operator for the round:
armour rating -> damage taken (`Armour` in combat, applied to every hit),
speed rating -> movement multiplier (`OperatorSpeed`, waiting for movement to
read it), the unique gadget and its count, and loadout slots (rifle,
sidearm, one shared secondary; data only).

**Gadget availability.** Defenders from prep on; attackers from the action
phase on; nobody during selection or after the round. Auger's ring cutter
and Mortise's backplate charges are data only until destruction (M4)
provides reinforced walls and the hard-breach API. Shared secondaries are
data only.

**Lark's sensor darts (as built).** Key 3 fires a dart from the wrist; it
flies fast with a little drop and sticks to the first world surface. While
listening it reveals every defender sound above crouch-walk loudness within
its radius (footsteps, and `Noise` such as reloads or gadget use) to all
attackers as a cyan ripple at the source, drawn through walls and floors;
then it goes quiet and disappears. Defenders within earshot hear a hiss when
one lands (and get a notice). Darts can be shot.

**Fathom's seismic stakes (as built).** Key 3 hammers a stake into the
floor just ahead (needs a floor). Every attacker footstep at walk or run
loudness within the radius, on the same floor or the one directly above or
below, pings defenders: an orange diamond at the walker's position, drawn
through walls, with a tick sound. Crouch-walking and prone don't trigger
it. Pings are rate-limited per attacker and stake. Any `Noise` loud enough
nearby (explosions, the ring cutter; gunfire is tunable) blinds the stake
for a while; its light turns amber. Stakes can be shot.

**Intel display.** Reveals are team data (`Reveal` messages); the HUD only
draws your team's (`--show-all-intel` draws both, for testing).

**Not yet:** recon drones (attacker prep), ring cutter and backplate
charges, shared secondaries, overtime, a dead-player spectator view,
character models for other players.

## M5 characters

Built by `tools/scripts/build_characters.sh` from one config per operator
(`tools/blender/characters/operators/<id>.toml`) into
`assets/models/characters/<id>.glb`. Spawned and animated by
`src/characters/`.

- **Body:** MPFB 2.0.18, driven headless through its Python service API:
  macro sliders (gender, age, muscle, weight, proportions) and face
  targets from the config, on MakeHuman's CC0 base mesh. Scaled so the top
  of the head is at `height_m` (1.72–1.79 m; helmets add ~4 cm, so every
  operator stands ~1.8 m like the player capsule). Feet at the origin,
  facing game -Z.
- **Skeleton:** MPFB's `game_engine` rig: 53 deforming bones with
  Unreal-mannequin-style names (`pelvis`, `spine_01..03`, `neck_01`,
  `head`, `clavicle_l`, `upperarm_l`, `lowerarm_l`, `hand_l`, fingers,
  `thigh_l`, `calf_l`, `foot_l`, `ball_l`), plus `weapon_r` (rifle socket,
  child of `hand_r`) and `weapon_mag`. Chosen over Rigify, whose generated
  rig has hundreds of control and mechanism bones that would have to be
  stripped for export; the naming also suits retargeting common game
  animation sets (and mocap) later.
- **Uniform:** not a separate mesh. The body is split by bone weights into
  shirt, trousers, gloves and boots (clean plane-cut seams), pushed out
  slightly and vertex-coloured. Sleeves can be rolled; gloves can be
  fingerless. Hair is a buzz-cut tint on the scalp.
- **Gear:** scripted mid-detail blockouts, fitted to each body by casting
  rays at it, and skinned from the nearest body surface (hard items ride
  one bone). Menu (config `[gear]`):

  | Slot | Options |
  |---|---|
  | helmet | `highcut` (rails, mount, counterweight, chin strap), `full` (ear coverage, jaw guard, raised visor), `cap`, `none` |
  | headset | ear cups and boom mic (headband when there's no helmet) |
  | eyewear | `goggles_up` (on the helmet), `glasses`, `none` |
  | carrier | `chest_rig` (no plates; X-harness), `plate` (front/back plates, cummerbund), `heavy` (plus side plates, collar, shoulder guards, groin flap), `none` |
  | pouches | `mags_triple`, `admin`, `radio_left`, `dump_left`, `grenades`, `med_pouch`, `hydration`, `thigh_holster`; signature kit: `breach_pack` (Auger's Ring Cutter disc), `dart_launcher` (Lark's wrist launcher), `charge_cases` (Mortise), `stake_quiver` (Fathom) |
  | gloves / boots | `full`/`fingerless`/`none`; `high`/`low` (sole and collar added) |
  | pads, neck, sleeves | `knees`, `elbows`; `gaiter`; `long`/`rolled` |

- **Team read:** attackers wear olive and coyote with tan helmets, goggles
  and backpacks; defenders wear slate, charcoal and near-black, no
  backpacks, and have the visibly different headgear (Mortise's full
  helmet and jaw guard, Fathom's soft cap). More armour = more plates.

  | Operator | Body | Kit |
  |---|---|---|
  | Auger (A, 2/2) | male, mid-30s, solid | high-cut helmet, goggles, plate carrier, breach pack with the cutter disc |
  | Lark (A, 1/3) | female, early 20s, light | high-cut helmet, goggles, chest rig, hydration pack, wrist launcher, rolled sleeves |
  | Mortise (D, 3/1) | male, 40s, big | full helmet with jaw guard and visor, heavy carrier, charge cases, elbow and knee pads |
  | Fathom (D, 1/3) | female, late 20s, lean | soft cap, glasses, gaiter, chest rig, stake quiver, thigh holster |

- **Materials (realism pass):** seven per operator, so ~7 draw calls:
  `skin` (1024 px CC0 MakeHuman/Mindfront skin times vertex colour),
  `uniform`, `webbing`, `leather` (vertex colour plus tiling CC0 cloth and
  leather detail maps: weave/grain normals and roughness), `hard` (helmet
  shells, hardware, lenses, carbine; smoother, untextured), `eyes`, and
  `brows` (eyebrow + eyelash atlas, alpha cut-out). Detail maps are shared
  by everyone and loaded once by the game from `assets/textures/characters/`
  (skin pores/roughness from the Mindfront skin; Poly Haven poplin, twill
  and leather); per-operator colour textures are embedded in the .glb.
- **Budget:** ~26–27k triangles per operator (body ~21k, gear 3.7–5.2k,
  carbine 0.35k, eyes/brows/lashes 0.7k), 55 bones. Ten on screen is
  ~270k skinned triangles and ~70 draw calls plus shadows: fine on a
  mid-range PC. Measured (dev build, `--no-vsync`): the keep alone 343 fps
  (2.9 ms), with 12 animated operators (`--lineup-crowd`) 274 fps
  (3.65 ms), so ~0.06 ms per character. **LOD plan:** the build already decimates the body by bone region;
  a second export at ~10k body triangles (gear unchanged) would make LOD1
  for distant characters.
- **Builds are reproducible:** rebuilding produces byte-identical .glb
  files, so unchanged operators don't churn Git LFS.
- **Animations:** scripted placeholders (no CC0 set without a login fits
  this skeleton), 30 fps, in place, posed in armature axes with our own
  two-bone IK: `idle`, `walk` (authored for 3.4 m/s), `run` (5.6),
  `crouch_idle`, `crouch_walk` (1.9), `prone_idle`, `prone_crawl` (0.8),
  `lean_left`/`lean_right` (spine and head pose), `fire` (aim plus one
  recoil kick, looped at the rate of fire), `reload` (2.4 s, the magazine
  comes out in the left hand). The game blends them by stance and speed
  and overrides the arms (aim, fire, reload) and spine (lean).
- **Hit zones (later):** the fixed capsules in `src/player/hitbox.rs` can
  follow bones: head on `head`, torso between `spine_01` and `spine_03`
  (or `neck_01`), legs on `thigh_*`/`calf_*`.
- **Mocap plan:** when CC0/permissive mocap (e.g. CMU BVH) is staged,
  retarget it onto `game_engine` in `anim.py` by mapping its joints to
  these bone names and baking per operator; the clip names, masks and the
  game side stay the same.
- **Not yet:** prone aiming and reloading, strafe/backpedal clips, hair
  meshes (short hair is a scalp tint under the headgear), real weapon
  models (they attach to `weapon_r`, and should hide the prop carbine).

## Fortress kit (M2)

Generated by `tools/blender/kit/pieces.py`; rebuild with
`tools/scripts/build_kit.sh`. Exact conventions are in that file's header.

- 1 m grid; a storey is 3.2 m (3.0 m walls on a 0.2 m slab).
- **Hard (indestructible):** stone walls 0.5 m thick (1/2/4 m, door, window,
  arches 2 m and 4 m), posts at every junction, slab-layer bands and caps on
  exterior walls, parapets with merlons on roofs, stone floors, stairs.
- **Soft (destructible in M4):** plaster walls 0.15 m thick (1/2 m, door,
  window), wooden floors, hatch tiles and hatch covers, crates, barricades
  (door/window). Crates and barricades also come pre-broken.
- Sandbags and railings: cover only for now.
- Stairs: 16 steps, 1.5 m wide; the stairwell above needs 1.2 m of headroom
  at its start, so the floor opening is 6 m long for a 4.8 m flight.

## Map: Harrow Keep

Original, fictional sun-bleached desert fortress: thick plaster-and-stone
walls, courtyards, narrow stairwells, rooftop access, hatches, a basement.
The reference season shipped its own desert-fortress map; Harrow Keep's
layout must not be modelled on it.

**Section A (M2):** the first playable piece, in `src/map/keep.rs`.
- A two-storey building (12 x 8 m) on the north side of a walled courtyard
  (16 x 12 m) with an arched gate to the south.
- Ground floor: a west hall with an arched door to the courtyard and stairs
  up along the west wall; a stone wall to the east rooms; a plaster wall
  between the courtyard-side room and a storeroom. Both outer doors of the
  east rooms are barricaded.
- Upper floor: two rooms split by a plaster wall; a railed stairwell; a
  wooden floor with a hatch over the storeroom.
- Roof with battlements (no access yet).
- Lighting: one hanging lantern per room (warm, shadowed spot light) and
  capped eye adaptation (the view brightens by up to 2 stops in dark
  interiors; outdoors is untouched). Both tunable under `lighting`.
- Not yet: rooftop access, basement, a proper sky (a physically based
  atmosphere was tried and looked grey; needs a lighting pass).

## Open questions

- Vault action (see Movement).
