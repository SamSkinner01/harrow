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

## M3 weapons

All numbers live in the `weapons` section of `assets/config/tuning.ron`;
these are the rules (code: `src/weapons/`).

**Loadout (placeholder until operators, M6):** every character carries a
primary and a sidearm. Both are original designs:
- **TAMARISK**, assault rifle: full auto, 30-round curved magazine, compact
  open reflex sight, skeletal stock, octagonal sand-coloured handguard.
- **ACACIA**, sidearm: semi-auto (one shot per click, rate-capped),
  15-round magazine, two-tone, three-dot iron sights.
Weapons spawn full with a round chambered (30+1, 15+1).

**Firing.** Hitscan from the true eye position (the same point the camera
uses, including the lean offset, so leaning round a corner shoots from where
you see). Direction = look angles + recoil + random spread. The fire rate is
exact over time (no rounding to the 64 Hz tick). You can't fire or aim while
sprinting or for a moment after.

**Damage model.** Each weapon has base damage per bullet, falling off
linearly between two distances to a minimum share. Body region multiplies
it: **any bullet to the head kills** (like the feel target; can be switched
to a multiplier), torso 1x, legs reduced. Bullets stop in the first body
they hit (no over-penetration through people). Friendly fire is on.
Operator armour (M6) scales body and leg damage when it lands; head shots
kill through any armour. Deployed gadgets (hit volumes with health but no
body region) take the bullet's base damage and stop it.

**Penetration.** Soft surfaces (plaster, wooden floors, hatches, crates,
barricades: anything the kit marks soft) let bullets through; each one costs
a share of the damage, up to a maximum number of soft layers. Hard surfaces
(stone, and anything without a surface kind) stop the bullet. Every surface
a bullet enters sends a `SurfaceImpact` (kind `Bullet`) at the entry point
with the bullet's direction; destruction (M4) uses these to open holes.
Head shots through a soft wall still kill.

**Recoil.** Each weapon has a fixed, learnable pattern: shot N of a burst
always kicks the view by the same amount (mostly up, wandering left and
right later in the spray), repeating the pattern's tail on long sprays.
Kicks are smaller while aimed. The view kick is shown by the camera. After a
short pause the view drifts back to where you aimed and the pattern starts
over. Visual weapon kick is separate and cosmetic.

**Spread.** Bullets land randomly inside a cone: wide from the hip, tight
when aimed. Moving widens it (scaled by speed); crouching and prone tighten
it. Sustained fire adds bloom (only part of it while aimed), which shrinks
once you stop. The first shot after a pause is much more accurate than the
rest. The random spread is seeded by the shot count, so the same inputs
always give the same shots (needed for server-side hit checks, M8).

**Aim down sights.** Right mouse, hold or toggle (tuning). Aiming takes a
fraction of a second, zooms the view per weapon, slows walking, and lines
the weapon's sight up with the eye. No aiming while reloading or sprinting.

**Reloads and switching.** A tactical reload (rounds left) keeps one in the
chamber (magazine + 1); an empty reload is slower and gives a full
magazine. Pulling the trigger on empty clicks and starts a reload (option).
Switching weapons lowers the current one, then raises the other; it cancels
a reload. Keys 1 / 2 or the mouse wheel. With the defuser in hand (M6) the
weapon is lowered: no firing, aiming or reloading.

**Noise.** Every shot and every reload start is a gameplay `Noise`
(gunfire is loud, reloads quiet; loudness in the tuning file), so detection
gadgets and later bots hear them.

**Feedback.** HUD ammo (magazine / reserve), weapon name, reload progress,
and a hit marker (white X on body hits, larger red X on head shots, held
longer on kills). Bullet impacts show a short dust puff and chips (sparks on
stone); nothing permanent. Training dummies (`--dummies`) flash red when
hit, show a health bar, fall over at zero and stand up again after a few
seconds.

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
| 3 | Unique gadget: press (darts, stakes); hold to place, tap to trigger (ring cutter, backplates) | gadgets (M6) |
| 4 | Secondary gadget: only the loadout's one answers (breach charge, grenades, wire...); cycles the secondary on the selection screen | gadgets / destruction |
| G | Recon: tap to look through your drones (attackers) or the CCTV (defenders), tap again to leave; hold to throw another drone | gadgets (M6) |
| Q / E (recon view) | Previous / next drone or camera (lean stays frozen) | gadgets |
| W A S D, mouse (recon view) | Drive and turn the drone; pan and tilt a camera | gadgets |
| 5 | Defuser (attackers) | round (M6) |
| F (hold) | Interact: reinforce, barricade, plant/disable defuser | destruction / round |
| Mouse wheel | Cycle weapon | weapons |
| Esc | Pause menu (frees the mouse) | menu (M8) |
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

- Prep: defenders set up and may use gadgets and CCTV. Attackers stay at
  their spawn (tunable) and scout with recon drones (G); no other attacker
  gadgets.
- Action: attackers may use gadgets and plant. The clock running out
  without a plant is a defender win (even mid-plant).
- Planted: the clock is replaced by the defuser's timer. It completing is
  an attacker win; defenders disabling it is a defender win.
- Wipes: if every member of a team is dead, the other team wins, except
  that a wiped attacking team does not lose once the defuser is planted
  (defenders must still disable it). A team with no members at all (solo
  play) is never "wiped". Anything with a `Team` and `Health` counts as a
  member (players, bots, training dummies). Dead characters can't walk.

**Bomb sites.** Each round uses one site pair of the map (Harrow Keep has
four; see "Map: Harrow Keep"). Each site is a trigger volume (your feet
must be inside), marked with painted ochre floor lines and a HUD label with
distance. On section A, site A is the ground-floor storeroom and site B the
upper-floor east room; attackers spawn outside the courtyard gate,
defenders inside the building on both floors.

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
sidearm, and the shared secondary picked with 4 on the selection screen).

**Gadget availability.** Defenders from prep on; attackers from the action
phase on; nobody during selection or after the round. Recon (drones, CCTV)
in prep, action and planted for both sides. The ring cutter, backplate
charges, shared secondaries, drones and CCTV are described in "M6 part
two" below.

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

**Not yet:** overtime, a dead-player spectator view. (Other characters
are drawn by the M7 body system.)

## M6 part two: gadgets, drones and CCTV (implemented 2026-11-14)

Rules as built; every number is in `round.gadgets` in the tuning file. Code:
`src/gadgets/` (one file per gadget; `mod.rs` explains the data flow). Every
player action goes through an intent (`GadgetIntent`: keys 3 and 4 held;
`DroneIntent`: G held, Q/E, drive axis, look angles) and every gadget's
state is a plain component, so bots and the network can drive and
replicate them. Using a gadget is refused (with a HUD notice) while dead,
while looking through a drone or camera, or outside the phases above.

**Keys 3 and 4: tap or hold.** A press shorter than `tap_time` (0.25 s) is
a tap; longer is a hold. Placing anything means holding the key while
standing still and looking at a valid spot; a progress bar shows under the
crosshair, and letting go, moving or looking away cancels it.

**Auger's ring cutter (x2).** Hold 3 (1.5 s) on a wall or a hatch cover,
soft or reinforced (hatches from above), to clamp it on; tap 3 to start the
latest clamped cutter (so it can go on quietly and be started with the
team). On a wall the frame centres itself between the posts with the cut
1.0 m up; on a hatch it sits in the middle of the cover. It cuts for 10 s:
a `Noise` of kind `Breach` at loudness 5 every 0.5 s (enough to saturate
seismic stakes), sparks and dust at the head, dust shed from the top of
the wall on both sides, and the view shakes within 4 m. Model: an oval
olive guide track on four clamp feet sized to the panel, a hub and yellow
motor, a steel drive ring that spins, and a cutting head with a spinning
saw disc that runs round the track three times per cut.
- At 50% the blade breaks through: a glowing cut line appears on the far
  side and the blade (a white-hot tooth with sparks, hit sphere 0.12 m)
  travels round it at about 2 m/s. It has 1 health: any damage (bullet or
  blast) jams the cutter, and Auger must hold 3 at the cutter for 4 s,
  standing still at the wall, to re-seat it. Progress is kept.
- The cutter body (60 health, hit volume on the hub) can be shot or blown
  up.
- Finished: a `HardBreach` of radius 1.0 at the cut centre (skin, studs,
  frame, plate and braces), clipped by the stone posts, so a 2 m panel
  becomes a walk-through opening; on a hatch the whole cover goes. The
  plug falls inward (tips over flat, or drops through the hatch), and
  anyone within 1.5 m behind it, inside the cut, is knocked prone and
  takes 25 damage.

**Mortise's backplate charges (x3).** Hold 3 (1.5 s) on the plate side of
a reinforced wall or hatch to bolt one on; tap 3, from anywhere, fires the
latest. It blasts through the wall: every enemy ring cutter and every
breach charge on the far side within 1.2 m is destroyed, a small blast (no
breach, 1.2 m, 20 damage) hits whoever is right behind the wall, and a
`HardBreach` of radius 0.1 punches a fist-sized hole through the
reinforcement (a permanent sightline). Charges have 30 health and fall off
when their wall is no longer reinforced (a new round).

**Shared secondaries (key 4).** The character's `Loadout.secondary`
decides who answers the key: the soft-breach charge stays destruction's
(`BreachCharges`); the rest are here, counted in `SecondaryStock`. Pick
the secondary with 4 on the selection screen (it cycles the picked
operator's options) or with `--secondary <name>`. Counts: frag 2,
flashbang 3, smoke 2, razor wire 2, ballistic cover 1, impact charge 2,
proximity alarm 2; refilled every round.
- Thrown gadgets: press 4 to ready one, release to throw (13 m/s plus a
  lob). They are Avian rigid bodies (4.5 cm spheres, bouncy, swept CCD)
  on their own collision layer: they bounce off the world but never block
  people or stop bullets.
- **Frag** (attackers): the 3 s fuse starts on the press, so holding 4
  cooks it (the bar turns red near the end); held too long it goes off in
  your hand. An `Explosion` of 140 damage falling to zero at 4.5 m, with a
  0.25 m breach radius (small holes in soft walls and floors; reinforced
  surfaces are untouched).
- **Flashbang** (attackers): bangs 1.6 s after the throw. Everyone with a
  clear line of sight within 10 m (any wall blocks it; smoke weakens it)
  is blinded for up to 5 s (full-screen white, fading over the last 40%)
  and deafened for up to 7 s. Strength = distance falloff x how directly
  they were looking at it (0.25 when looking away). Deafness is the
  `HearingMuffled` flag (gadget sounds use it; the global volume is
  lowered as a bridge) plus a ringing tone.
- **Smoke** (attackers): after 1.2 s it pours a cloud of 3.5 m radius for
  12 s, then thins out over 3 s: up to 46 large camera-facing puffs per
  cloud (140 in total), pulled in from walls so they don't show through
  them, opaque enough to block sight. `smoke_blocks(a, b, ..)` answers
  sight-line questions for bots.
- **Impact charge** (defenders): thrown flat and fast; on the first
  contact an `Explosion` with a 0.75 m breach radius (opens soft walls,
  floors, hatches and barricades) and only 25 damage within 1.5 m.
- **Razor wire** (defenders): hold 4 (1.5 s) to lay a 1.7 m concertina roll
  across the floor 1 m ahead (doorways are fine). Anyone inside moves at
  35% speed (no sprint), and every footstep in it is a loud `Noise` (1.2)
  with a rattle. Bullets pass through; 80 health against blasts and melee
  (45 per swing).
- **Ballistic cover** (defenders): hold 4 (1.5 s) to set a waist-high (1 m)
  folding steel shield 0.9 m ahead, facing where you look: a centre plate,
  two wings angled back, a vision block. It is world geometry
  (`SurfaceKind::Hard`): it stops bullets, shields people from blasts and
  blocks movement. Only explosions wear it down (150 blast damage).
- **Proximity alarm** (defenders): hold 4 (0.5 s) aiming at any surface
  within 2 m. While an attacker is within 3 m (and 2 m up or down) it
  beeps every second (everyone nearby hears it) and reveals them to
  defenders (a red bracket through walls). 15 health.

**Recon drones (attackers, x2).** Tap G to connect to your latest drone, or
to throw one if none is out; hold G to throw another; tap G to come back;
Q/E switch between your drones. Allowed in prep (from the spawn), action
and planted. A thrown drone tumbles and bounces until it settles, then
drives: W/S forward/back (3.2 / 1.8 m/s), A/D and the mouse turn it, the
mouse tilts the camera. It can't jump; it climbs ledges up to 9 cm and
ramps up to 40 degrees (it drives up the stairs), and its body is a 0.1 m
sphere, so it fits through any hole a little wider than 0.2 m. Its battery
lasts 240 s of driving; its motor is a faint `Noise`. Defenders can see it
(olive body, two wheels, a camera eye, a green light) and shoot it (40
health, hit sensor on the bullet layer). The view is a separate camera:
the player and weapon cameras are switched off (`Camera::is_active`) and
the drone camera becomes the UI camera so the HUD stays. A tinted, grainy,
scan-lined, vignetted overlay shows the drone number, battery, signal bars
(falling off over 45 m), distance and drones in reserve, a reticle and key
hints. A drone that breaks cuts the feed: a burst of static, then your
next drone, or your body.

**CCTV (defenders).** Six fixed cameras on Harrow Keep section A: the
courtyard facade, the west hall, the east room and the storeroom (site A),
the upper west room and the upper east room (site B). Spots live in
`CctvLayout` (a map can insert its own). Tap G to watch, Q/E to switch,
the mouse and A/D pan (55 degrees) and tilt (30 degrees), G to leave. The
overlay shows the camera number and name, a clock and a blinking REC.
Attackers can see them (red light) and shoot or blow them up (10 health);
a camera lost while watching shows static and moves to the next. Every
round brings them all back.

**Looking through a drone or camera** leaves the body standing still: its
intents are frozen (no walking, turning, firing, gadgets or interact), it
carries the round's `MovementLock`, and it can still be shot. The view
ends when the target breaks, you die, a new round starts or the phase no
longer allows it.

**Round reset.** Every `RoundReset` removes thrown, placed and cosmetic
gadget entities (cutters, backplates, grenades, smoke, wire, covers,
alarms, drones, plugs), refills secondary stock and drones, respawns the
CCTV cameras and ends every remote view. Unique gadget counts refill when
prep equips the operators.

**How blasts reach gadgets.** Destruction's explosion damage measures to a
target's hit zones (or 1 m above it). Gadgets carry a collider-less
`HitZone` child ("blast anchor") at their centre, so wall and ceiling
gadgets take blast damage correctly; bullets and melee never see it.
Ballistic cover takes blast damage in `deployables.rs` instead (it is solid
world geometry and would block its own line of sight).

**Not yet:** picking deployables back up, viewing teammates' drones, CCTV
spots for the full-size map (it needs its own `CctvLayout`), real sounds
(all gadget sounds are synthesised placeholders).

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
  meshes (short hair is a scalp tint under the headgear). Real weapon
  models attach to `weapon_r` with the prop carbine hidden (M7
  `characters/body.rs`).

## M7 bots (implemented 2026-11-14)

Bots fill both squads: 5v5 by default (you plus 4 team mates against 5).
Code: `src/ai/` (one file per concern, overview in `ai/mod.rs`) and the
third-person bodies in `src/characters/body.rs`. Every number is in the
`ai` section at the end of the tuning file.

**Rules bots follow.** A bot is an ordinary character (health, team,
squad, operator, loadout, weapons, hit zones) whose controller writes the
same intent components a player's keyboard does (`PlayerInput`,
`WeaponInput`, `DestructionIntent`, `RoundIntent`, `GadgetIntent`). It
never touches simulation state, so recoil, spread, reload times, the
plant timer and destruction apply to it exactly as to a person, and a
server runs bots like any other simulation. They only run where the
simulation runs (`session::simulating`; never on a client). They don't
fire before the action phase (attackers are held at their spawn in prep).

**Population.** `--bots N` (total; default 9 solo or hosting, 0 on a
dedicated server), `--no-bots`, `--difficulty easy|normal|hard`. Bots
are spawned when a session starts (`OnEnter(InGame)`), split so squads are
even with the local player on Blue, and pick operators on the selection
screen, spreading each side over the roster (with two operators per side,
duplicates are unavoidable at 5v5). Bots carry `NetPlayer` (ids from
`0xB07_0000_0000`), so clients see, place and draw them like players. On a
host, a joining human takes a bot's place in their squad (a dead bot
first, never the defuser carrier); a squad left short gets a bot back
between rounds. Bots don't count towards the server's player limit.

### Navigation

Generated from the level's colliders once the map has loaded (no
hand-placed data), so a new map just works:

- **Grid:** columns every 0.4 m over the area of play (every site pair
  and spawn area of the map, 6 m margin; on a map without objectives,
  every finite world collider). Built once the map reports its kit pieces
  loaded (`map::KitLoaded`). A sphere (r 0.1 m, so bullet holes don't
  punch gaps) is swept down each column through every collider; each
  upward-facing surface no steeper than `movement.max_slope` is a
  candidate floor (up to 6 levels per column: basement to roof).
  The floor's height and slope at the column centre come from a ray (the
  sphere's contact normal is skewed at edges).
- **Clearance:** an upright capsule (r 0.22, a bit under the 0.3 m body so
  1 m doorways always contain a cell) lifted 12 cm off the floor must fit
  standing, else crouching (crouch-only cell). A cell blocked only by an
  intact barricade is kept and flagged: attackers path through it and
  break it, defenders don't.
- **Links** to the 8 neighbours: the height change must lie between the
  two floors' slope predictions plus a 9 cm step (so stair ramps link up
  and down but their sides are ledges), and three horizontal rays (knee,
  chest, head) between the cells must be clear (thin walls, railings,
  ceilings). Where a neighbour has no floor at this height but one up to
  4.5 m below and a body-sized sphere falls through, a one-way **drop**
  link is added (open hatches, blown floors, walking off a ledge). Plain
  soft walls get two-way **breach** links (for attackers carrying breach
  charges). Only the biggest walk-connected area counts as "the map" for
  tactics (no hold spots on crate tops).
- **Updates:** big holes (explosions, melee crouch holes), broken hatches
  and barricades, new or removed surfaces (defender barricades) and round
  resets mark their area dirty; it is re-sampled and re-linked a little at
  a time (1.5 ms a frame) after colliders are rebuilt. Paths that cross a
  changed area are re-planned.
- **Cost:** section A's grid (73 x 107 columns, ~8,300 nodes) builds in
  ~150 ms; the full Harrow Keep (439 x 391 columns over the keep and its
  four spawn ruins, ~187,000 nodes on five levels) in ~4.5 s, spread over
  frames (6 ms each) while the round loop waits for it.
- **Paths:** A* (metres walked, crouch-only x1.8, barricade +6 m, drop
  +1.5 m, breach +14 m, plus per-caller penalties; the distance estimate
  weighted x1.1: ~15% fewer nodes searched on the big map, for paths
  that came out the same in the spawn-to-site tests). One bot's search runs at a time, spread over
  ticks at 4,000 nodes a tick (~1 ms in a dev build), and none while an
  area of the grid is being patched (it is unlinked until done). Paths are
  then string-pulled along walk links but never
  across edge cells, so paths keep off walls and go through the middle of
  doorways. Local avoidance keeps bots 0.9 m apart (sidestepping to the
  right head-on) and attackers moving carefully keep stack spacing.
- **Stuck:** no progress for 1.5 s while trying to move is logged and
  counted; the bot re-plans, then sidesteps, then hits whatever is in front.
- `--ai-debug` draws the grid (green standing, yellow crouch-only, red
  barricaded, blue drops, magenta breaches), bot paths, goals and aim
  lines. `--ai-navtest` plans every spawn-to-site path and logs them;
  `--ai-navdump x0,z0,x1,z1` dumps the columns along a line.

### Senses and team knowledge

- **Sight** (10 times a second per bot, staggered): enemies inside the
  view cone (`fov`) and `sight_range`, with a ray from the eye to each hit
  zone. Stone, soft walls, floors, closed barricades and smoke block it;
  holes and gaps don't. Someone within 2.5 m is noticed at any angle.
- **Hearing** (every tick): footsteps and `Noise` within
  `loudness x 22 m x difficulty.hearing` (walk 11 m, sprint 22 m,
  crouch-walk 2.6 m, gunshot 66 m), halved on another floor.
- **Pain:** being shot reveals roughly where from.
- **Team knowledge:** sightings, sounds, stake pings and dart reveals are
  shared by the team for 5-8 s. New contacts become callouts ("Enemy
  spotted: site A (storeroom)", "Movement heard: upper floor north"),
  shown in the HUD to your team and logged. Place names are worked out
  from the sites and the floor (map-independent).

### Behaviour

- **Defenders, prep:** anchors (most) and `roamers` (Fathoms first) share
  jobs: reinforce the reinforceable walls and hatches around the sites (2
  each, standing on the site side, hatches from below), Fathoms hammer
  stakes 3 m outside the entrances, then anchors barricade the site
  doorways. Unfinished jobs carry on for 20 s into the action phase.
- **Defenders, action:** anchors hold *hold spots* (inside a site, 2.5-10 m
  from an entrance with a line of sight to it, preferring cover and an
  off-angle), crouching sometimes, swapping spots every 30-50 s when
  quiet, turning towards anything heard or pinged nearby. Roamers move
  between indoor spots away from the sites and go after fresh contacts.
- **Attackers:** wait at their spawn in prep while a target site is
  picked and a route planned. In action they split into two stacks; the
  second avoids the first's route where the map allows. They run while
  far, then walk within `careful_range`, checking corners (a fan of rays
  finds depth jumps; they look past the nearest one) and pre-aiming the
  openings on their path. Barricades on the path are meleed down; Augers
  with breach charges can take breach links (stick the charge, back off,
  wait for team mates to clear, set it off, go through). Larks fire sensor
  darts at the site on the way in. The carrier plants as soon as it is
  inside a site and no enemy is in sight; the others take hold spots
  covering the entrances, then cover the defuser. A dropped defuser is
  fetched by the nearest attacker.
- **Planted:** the nearest defender goes to disable it (holds F once in
  reach and no enemy in sight); the others cover around the device.
- **Everyone:** breaks off to cover (a nearby spot out of the threat's
  sight) below `retreat_health` in a fight, reloading there.

### Combat

- **Target:** the visible enemy already being fought, else the nearest
  (weighted by how far the view must turn).
- **Reaction:** first shot no sooner than `reaction_time` (+/- jitter,
  more at the edge of the view, halved when a lost target reappears).
  Someone appearing where the bot was already aiming (a held angle, a
  pre-aimed doorway) gets 30% less reaction time and a third of the aim
  error; within 15 degrees, 15% and 60%.
- **Aim:** the view turns at `turn_speed` towards the chosen zone (head
  with `head_aim` chance, else torso, else whatever shows) plus an error
  starting at `aim_error` degrees that settles over `aim_settle` seconds;
  a moving target drags the aim (`tracking_error`). The trigger is only
  pulled when the aim is on the zone.
- **Recoil:** the look angles pull against the weapon's recoil by
  `recoil_control`, so long sprays still climb.
- **Bursts:** `burst_close` rounds up close to `burst_far` at 25 m, with
  `burst_pause` between; the sidearm is clicked.
- **Stance:** aim down sights when fighting; crouch (`crouch_chance`) when
  holding still; lean out from beside a wall (`lean_chance`).
- **Ammo:** reload when empty (switch to the sidearm instead up close),
  top up after the fight.
- **Friendly fire:** no shot with a team mate in the line of fire.
- **Through walls:** never at someone it can't see, except, rarely
  (`wallbang_chance`, 0 on easy), a short burst at an enemy it just heard
  behind a soft wall; the line is checked first and must cross only soft
  surfaces. Shots at targets are spot-checked for stone in the way (logged
  as `stone-shots`, which stays 0).

| Preset | Reaction | Aim error / settle | Turn | Recoil control | Head | FOV / range | Wallbang |
|---|---|---|---|---|---|---|---|
| easy | 0.65 s | 7 deg / 0.6 s | 200 deg/s | 45% | 10% | 100 deg / 40 m | never |
| normal | 0.4 s | 4.5 deg / 0.4 s | 300 deg/s | 70% | 30% | 110 deg / 60 m | 3%/s |
| hard | 0.25 s | 2.5 deg / 0.25 s | 420 deg/s | 88% | 55% | 120 deg / 80 m | 12%/s |

### HUD

A kill feed (left, under the FPS counter: killer, weapon, victim, headshot; your side's
deaths in orange) and your team's callouts under it. On a client the
feed is built from the server's forwarded hits.

### Third-person bodies (`characters/body.rs`)

Every `Player` without `LocalPlayer` (and without the opt-out `NoBody`)
gets its operator model as a child, swapped when its round operator or
side changes; a dedicated server draws none. Each frame it reads only:
the (interpolated) transform, `CharacterState` (stance from height, lean),
the drawn ground speed (or `LinearVelocity`), `PlayerInput.pitch` or
`net::NetAim.pitch` (spine bend), `WeaponState` (active weapon, aiming,
firing, reloading, lowered while holstering or holding the defuser) and
`Dead` (tips over backwards, stands up when revived). The prop carbine is
hidden and the active slot's real weapon `.glb` (the tuning file's
`weapons.<slot>.model`, the same as the first-person view) is attached to
the `weapon_r` bone (+90 degrees about X: the bone's +Y runs along the
barrel). Shots show a muzzle flash, a flicker of light and a tracer on
every other shot. So a remote player only needs those components
replicated; nothing else is required.

### Tests and cost

`--ai-test` plays bot-only matches (the local player is a bot too) with
`ai.test_timers`, logging every kill, plant, defuse, reinforcement,
breach and stuck bot, a summary per round and match, and the frame rate;
`--ai-matches N`, `--ai-speed X` (game clock speed), `--ai-site A|B`
(force the attack target), `--ai-no-reinforce` (defenders leave walls
soft), `--ai-breach-test` (free play: breach into site A), `--ai-shots DIR`
(screenshots from a chase camera on a bot that is fighting), and
`--ai-spectate`. Bot cost is measured every frame and logged every 10 s
(`ai: cost ...`).

**Not yet:** bots don't use the ring cutter, backplate charges, drones,
CCTV, razor wire or proximity alarms (the M6 part two gadgets), and never
go prone or vault. Breach charges only open plain soft walls into a site,
which the keep's defenders usually reinforce in prep, so in full matches
they are rare (`--ai-no-reinforce` / `--ai-breach-test` show them).
Callouts are made on the server only (not forwarded to clients yet).
Place names in callouts are generic (site, floor, compass); a map could
provide named areas later.

## M8 networking (implemented 2026-11-14)

Lightyear 0.31 (netcode over UDP; replication on bevy_replicon), at the
64 Hz fixed tick. Code: `src/session.rs` (app flow), `src/net/`
(networking), `src/menu/` (menus). How to play it: README "Multiplayer".

### Modes and app flow

`AppState`: `MainMenu` -> (`Connecting`) -> `InGame` -> back. The map is
built once at startup and stays loaded (it is the menu backdrop);
everything belonging to a session (characters, round state, gadgets,
defusers, links) is created on entering `InGame` and removed on leaving.

| Mode (`NetMode`) | Simulates | Local player | Window |
|---|---|---|---|
| `Offline` (solo) | everything | yes | yes |
| `Host` (listen server) | everything | yes | yes |
| `Dedicated` | everything | no | no (no GPU, audio, input) |
| `Client` | its own movement only (predicted) | yes (replicated) | yes |

Solo, host and dedicated run the *same* gameplay code; a host is solo
plus a UDP server, and its own character is an ordinary local player that
also gets replicated. The authority's fixed-tick simulation is gated with
`session::simulating` (`RoundSystems`, `GadgetSystems`,
`WeaponSystems::Simulate`, `DestructionSystems::{Actions, Explosions}`,
`CombatSystems`), so a client never decides rounds, damage, shots or
destruction.

### Connecting

netcode with a fixed protocol id and a **development key** (every build
has it; it filters stray packets but isn't a secret). Token address
checks are off, so clients can use any address that reaches the server
(LAN IP, public IP, VPN name). Handshake: the client sends `Hello` (build
id = crate version + git commit, name); the server answers `Welcome`
(server name, map, movement tuning) or `Refused` ("Version mismatch",
"Server full") and disconnects. Only after `Welcome` does the link get
`ReplicationSender`, so mismatched builds never exchange world data. The
client times out after 12 s ("Server not responding") and goes back to
the menu on any disconnect with the reason.

Teams: each client joins the squad with fewer players (the host is Blue)
and plays that squad's side. Joining in operator selection or free play
puts you straight in; joining mid-round makes you a spectator (`Dead`,
no `Health`, so you don't count for wipes) until the next round. Leaving
removes the character (dropping the defuser if it had it). A dedicated
server idles its round loop while empty.

### Input

`NetInput` (`net/input.rs`) carries every intent component of a
character: `PlayerInput` (move axis, look angles, sprint, stance, lean),
`RoundIntent` (F, 5, operator picks), `GadgetIntent` (3),
`DestructionIntent` (V, F, 4), `WeaponInput` (trigger, aim, reload,
slot). The client copies its intents in every tick; Lightyear sends them
with redundancy (the last 10 ticks in every packet) plus the client's
interpolation delay (for lag compensation); the server copies them back
into the same components, so gameplay code can't tell a remote player
from a local one. Held state is copied as-is; M6's latched presses
(`equip_defuser`, `pick_operator`, `use_unique`) travel as press counters,
which survive loss and repeats.

### Prediction (your own character)

Movement (`player/movement.rs`) is deterministic given input and state,
and every input to it is networked: `SimPosition` (the feet at the end of
the tick; `Transform` itself is eased between ticks for rendering, so it
can't be sent), `LinearVelocity` and `CharacterState` are replicated and
predicted (small rollback tolerances for floats); `MovementLock` and
`MoveSpeedScale` are replicated; the server's movement tuning replaces
the client's (`net/tuning_sync.rs`). When the server's state for a tick
differs from the prediction, Lightyear restores it and replays the ticks
since with the buffered inputs. Replays don't re-emit footsteps or input,
and local mouse/keyboard is read after replays so no input is lost. Under
80 ms simulated RTT, 10 ms jitter and 2 % loss a moving client had no
rollbacks after the initial one.

Not predicted (yet): weapon fire. The server fires; the client sees its
shots (tracer, sounds, hit marker) and weapon state (ammo, reload, recoil
view) one round trip later. Predicting `WeaponState` with rollback is the
next step for snappier shooting.

### Interpolation (everyone else)

Other characters are shown ~100 ms in the past, blended between server
updates: `SimPosition`, `NetAim` (yaw/pitch), and `CharacterState`
(height, lean). The generic third-person body system
(`characters/body.rs`, M7; see "M7 bots") gives them their M5 operator
model, drives `CharacterAnim` every frame and bends the spine to the look
pitch, reading it from `NetAim` where a character has no `PlayerInput`. Hit zones exist on every machine, posed from the (predicted
or interpolated) state.

### What is replicated

- Characters: `Player`, `NetPlayer` (id, name), the movement state above,
  `Health`, `Team`, `Squad`, `Dead`, `MovementLock`, `Operator`,
  `OperatorChoice`, `UniqueGadget`, `DefuserCarrier`,
  `ObjectiveInteraction`, `WeaponState`, `MoveSpeedScale`. Hit zones and
  models are local children (`DisableReplicateHierarchy`).
- Match: `Round` as a `RoundSync` component on one entity; clients copy
  it into their `Round` resource, so HUD, sounds and `RoundPhase` work
  unchanged.
- Entities the server spawns: planted and dropped defusers, sensor darts,
  seismic stakes (state + `Transform`); clients attach the same models.
- Events, forwarded in order: `RoundEvent`, `GadgetEvent`, `ShotFired`,
  `WeaponEvent`, `BulletHit` to everyone; `Reveal` only to the team it's
  for.
- Destruction: M4 is deterministic, so the server forwards every surface
  edit it applies (impacts, hard breaches, reinforcement, round resets)
  keyed by the piece's placement, and clients replay them through M4's own
  code: identical holes, debris and dust. Late joiners get the round's
  edit log. Not yet: barricades and breach charges placed at runtime
  (their effect on walls is synced, the entities aren't).
- Gadgets (M6 part two, `net/gadget_sync.rs`), below.

### Gadgets over the network (M8 part two, 2026-11-14)

The server runs every gadget's simulation, as in solo; clients show it.

- **World objects, everyone sees them:** ring cutters (state, progress,
  angle, the exposed blade as its own entity), backplate charges, grenades
  in flight, smoke clouds, razor wire, ballistic cover, proximity alarms,
  drones and CCTV cameras, each with its `Transform`. Grenades and drones
  carry `InterpolationTarget`, so their `Transform` is interpolated like
  other players (and their model stays hidden for the ~100 ms until
  interpolation starts). The cut-out plug is sent once (`replicate_once`)
  and falls on each client by itself; alarms are sent once too (their
  beeps are events). Clients pour their own smoke puffs from the cloud's
  state. The gadget model code dresses everything on `Added<..>`, on every
  machine. Server-only entity fields (the wall a cutter or backplate sits
  on: map entities aren't networked) are `#[serde(skip)]`.
- **Ballistic cover is world geometry on clients too:** they add the same
  body (`gadgets::cover_body`: name, static collider, `SurfaceKind::Hard`),
  so predicted movement bumps into it and bullet impacts on it (surface
  edits keyed by name and place) find it.
- **CCTV:** the cameras exist on every machine from startup (map spots).
  A client drops its own while connected and uses the server's (their
  health and destruction are the server's); they come back at the menu.
- **Per-player state, sent only to its player:** `Flashed` (blind/deaf:
  the white-out and muffle happen on that client only), `Loadout`,
  `SecondaryStock`, `BreachCharges`, `ReconStock`, `GadgetActivity` (HUD
  progress bars; sent without its server-side entity targets),
  `RemoteView`, `ViewSerial`. A replicon visibility filter (`PrivateTo`,
  the owning link, on every client's character) hides that scope from
  everyone else; `--net-trace` counts other players' private state on
  clients (must be 0). Clients don't recompute what a loadout implies
  (stock, breach charges): they get the server's.
- **One-off happenings:** forwarded `GadgetEvent`s (sounds, notices). The
  blast, flashbang and backplate visuals and the cut-through dust are
  drawn from the events (or the plug's arrival), so clients get them too.
  Events can name an entity that hasn't been replicated yet (a drone
  thrown this tick): on the client that field is a placeholder.
- **Remote views:** the server puts `RemoteView` on the viewer; it counts
  as a `MovementLock` reason in the round flow. The viewer's client
  freezes its own body exactly like the server (`FrozenInput`, so the
  predicted character holds still and no shots go out), and the server
  re-freezes after applying a client's input, which would otherwise undo
  it. Drone look angles are absolute: a client resets them when a new
  view arrives and echoes `ViewSerial` (bumped by the server on every view
  change) in its input; the server ignores angles meant for an older view.
  Driving is server-side: the owner sees its drone move one round trip
  plus the interpolation delay late (prediction is a later step); looking
  around is immediate.
- **Razor wire is predicted:** `GadgetSpeedScale` is set after every tick
  (`FixedPostUpdate`, before prediction history) on every machine that
  simulates the character, from the replicated wire placements and the
  server's wire tuning (sent in `Welcome`); movement multiplies it in and
  allows no sprint below 1. It is replicated with `.predict()`, so a
  rollback restores it. Walking through wire adds no rollbacks: none at
  all on a clean link; with 80 ms RTT and 10 ms jitter a full-speed walk
  rolls back a few times with or without wire (`net_test.sh --gadgets
  wire|walk --latency 80 --jitter 10`; movement prediction under jitter,
  not the wire).
- **Tests:** `tools/scripts/net_test.sh --gadgets frag|smoke|flash|drone|
  wire|cutter|all` (server `--net-script` sets up; clients drive
  themselves with `--gadget-test`).

### Lag compensation

The server keeps ~300 ms (20 ticks) of every character's pose (feet, yaw,
stance height, lean) in `PoseHistory`. For a client's shot it rebuilds
all hit zones at that client's view time (the shot's tick minus the
interpolation delay it reported, clamped to the history) with the same
`zone_pose` the live zones use, and the bullet trace (`weapons/
ballistics.rs`, with penetration) tests those instead of the live zone
colliders; walls are tested as they are now. Solo, the host's own shots
and bots use the live zones. API: `net::lag_comp::LagCompensation`
(`view_time`, `cast_ray`, `with_rewound_hitboxes`, `pose_at`).
`--net-shoot-test` logs rewound vs live results between two players.

### Plugging in a feature

- **Intent**: a component written by input, read by the fixed tick. Add
  a field to `NetInput` and one `sync_intent::<C>(app, |i| &mut i.field)`
  line. Use held state or press counters, never "cleared by the sim".
- **State shown to players**: `app.component::<C>().replicate()` in
  `net/protocol.rs` (`Serialize + Deserialize`; mark entity fields
  `#[entities]`). Insert such components with `insert_if_new` in `Add`
  observers, so a replicated value isn't overwritten on clients.
- **State the local player's movement reads**: also `.predict()`.
- **Simulation**: put it in a system set and gate the set with
  `session::simulating` in `NetPlugin` (clients must not decide outcomes).
- **One-off happenings** (sounds, effects): forward the message
  (`net/events.rs`, `forward::<M>`/`receive::<M>`).
- **Entities the server spawns**: `replicate_spawned::<Marker>` in
  `net/round_sync.rs`, plus an observer that attaches the model on clients.

### Dedicated server

Same binary with `--server` (Bevy without window/audio/gamepad plugins,
the render plugin with no backend, a 64 Hz loop), or a lean build without
those libraries (`--no-default-features --features dedicated`). Config
file: `--config` (RON). Logs to stdout; SIGTERM exits cleanly.

**Lean assets.** Without a render world Bevy keeps every decoded image in
RAM. A headless server reads `assets/` through `LeanAssetReader`
(`net/dedicated.rs`): files under `textures/` and the images inside
`.glb` models become a 1x1 stand-in, everything else (kit and prop
meshes: collision; character skeletons and animations; tuning) loads
normally. Soft surfaces build no render meshes without a render app
(like the map's static batching). Memory (RSS, idle): dev build,
section A 755 -> 277 MB, full map 1136 -> 522 MB; release lean build
(the VPS bundle) 213 MB on section A, 435-490 MB on the full map
(`tools/scripts/server_memory.sh`).

**Keys.** `netcode_key(secret)`: no secret, the development key; a
secret (server config `key`, `--key`, `HARROW_KEY`) becomes the netcode
private key through BLAKE3 key derivation. Clients make their own connect
tokens, so it is a shared secret: only clients with it can connect.

**Packaging.** `tools/scripts/package_server.sh` builds the lean server
in an Ubuntu 22.04 container (glibc 2.35, so it runs on Ubuntu 22.04+ and
Debian 12+), and packs it with the server's assets (no textures), the
example config, a systemd unit and an install script
(`deploy/`). `deploy_server.sh user@host` installs it over SSH.
`build.rs` takes `HARROW_BUILD_COMMIT` when there's no git repository.

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
- Map pieces (full map, `tools/blender/kit/map_pieces.py`): curtain walls
  1/2/4 m and posts (3 m thick, paved walk at 6.4 m, crenellated parapet),
  wide arcade arch, free-standing pier, 4 m paving, exterior stairs with a
  sloping side wall plus base, pier and landings, open door leaf, window
  shutters, ceiling beam, table, bench, shelf, bunk bed, barrel, weapon
  rack, hearth, well, market stall, cart, rubble, rocks, dunes, rug, tomb,
  generator, radio set, gravel track, and a 1 cm cap slab for roof
  corners left open for the wall-walk. All hard (indestructible).

## Map: Harrow Keep

Original, fictional sun-bleached desert fortress: thick plaster-and-stone
walls, courtyards, narrow stairwells, rooftop access, hatches, a basement.
The reference season shipped its own desert-fortress map; Harrow Keep's
layout must not be modelled on it (and isn't: a walled compound of separate
buildings around three yards, with a tall keep on the north wall and all
four site pairs stacked in it, basement to third floor).

`--map keep` (the default). Code: `src/map/harrow_keep/` (one file per
part), sites and spawns in `src/round/sites.rs`. World: x east, z south
(north is -Z on these plans), 1 m grid, storeys 3.2 m.

### Overview

- **Compound:** curtain walls on x = +-40, z = +-30 (3 m thick, wall-walk
  at 6.4 m with crenellated parapets): 77 x 57 m inside the walls, about
  84 x 66 m with the corner towers. The desert outside is part of the map
  (attacker spawns 40 m out).
- **The keep** (32 x 22 m, x -16..16, z -30..-8): basement (-3.2), ground,
  second floor (3.2), and a third-floor block (6.4, 16 x 12 m) standing on a
  roof terrace that wraps it on three sides. Its north face is part of the
  outer wall. All four site pairs are in the keep.
- **Gatehouse** (south wall): vaulted gate passage between two guard rooms,
  gate room above, roof on the wall-walk.
- **Barracks** (west wall) and **granary** (east wall): two storeys each;
  their roofs carry the wall-walk.
- **Old hall** (south-west, one tall storey with an arcade) and **stables**
  (arcaded lean-to on the south wall).
- **Four corner towers** (8 x 8 m, three levels + roof lookout) link the
  wall-walks.
- **Yards:** inner walls with arches split the compound into the main yard
  (centre), the well court (west) and the east yard. Paving in front of the
  keep, along the gate road and round the well; the rest is packed sand.

```text
Compound: 1 char = 1 m east-west, 1 line = 2 m north-south; north up.
# walls  =| curtain wall + wall-walk  ! inner wall (gaps = arches)
^ stairs up to the walls / terrace  v cellar steps  O well

                                              CELLAR STEPS
  #########                                      v                            #########
  #NW     #=================########+++++++++++++++++########=================# NE    #
  #       # ^^^^^^^^^^^^^^  #       +               +       #   ^^^^^^^^^^^^^^#       #
  #########    NORTH        #KEEP   + 3F            +       #                 #########
    |          COURT        #       +               +       #                       |
    |                       #       +               +       #^                      |
    #############           #       +++++++++++++++++       #^          #############
    #           #           #                               #^          #           #
    #           #           #                               #^          #           #
    #           #     O     #                               #^          #           #
    #           #           #terrace above                  #^          #           #
    #           #WELL       #################################^          #           #
    #BARRACKS   #COURT      !                               !           #GRANARY    #
    #           #           !                               !           #           #
    #           #           !                               !           #           #
    #           #           !                               !  EAST     #           #
    #           #                                           !  YARD     #           #
    #############                                           !           #############
    |                       !                                                       |
    |                       !         MAIN YARD                                     |
    |                       !                               !                       |
    |                       !                               !                       |
    |         #################                             !                       |
    |  BACK   #               #                             !                       |
    |  LANE   #  OLD HALL     #                             !                       |
    |         #               #                             !                       |
    |         #               #                             !                       |
    |         #################                             ###################     |
  #########                                                 #   STABLES       #########
  #SW     #     SOUTH LANE            #############         ################### SE    #
  #       #            ^^^^^^^^^^^^^^ #  GATE     #                           #       #
  #########===========================#           #===========================#########
                                      #############
```

### Callouts

Short names for comms (keep rooms: B = basement, G = ground, 2 = second, 3 = third floor).

| Area | Callouts |
|---|---|
| Keep B | CELLAR STEPS, OIL CELLAR, CELLS, CISTERN (site 1A), UNDERCROFT, POWDER STORE (site 1B), PUMP ROOM, CRYPT |
| Keep G | POSTERN, ARMOURY (site 2B), LARDER, WEST GALLERY, GREAT HALL (site 2A), GUARDROOM, ENTRANCE, KITCHEN, SCULLERY |
| Keep 2 | CHART ALCOVE, MAP ROOM (site 3A), RECORDS (site 3B), LONG GALLERY, INFIRMARY, CHAPEL, MESS |
| Keep 3 / roof | SIGNAL ROOM (site 4A), STUDY (site 4B), LANDING, WEST / EAST / SOUTH TERRACE |
| Keep stairs | KEEP STAIR (centre, B to 3), WEST STAIR and EAST STAIR (B to 2), TERRACE STAIRS (outside, yard to 2 to terrace) |
| Yards | MAIN YARD, WELL COURT, NORTH COURT, EAST YARD, BACK LANE, SOUTH LANE, STALLS, WELL |
| Gatehouse | GATE (passage), WEST / EAST GUARD, GATE ROOM (murder hole), GATE ROOF |
| Barracks | BUNKROOM, WASHROOM, DRILL HALL, DORMITORY, STORES, BARRACKS ROOF |
| Granary | GRANARY, MILL ROOM, SACK STORE, LOFT, OFFICE, GRANARY ROOF |
| South | OLD HALL, SHRINE, STABLES |
| Walls | NW / NE / SW / SE TOWER, NORTH / WEST / EAST / SOUTH WALL, RAMPART STAIRS (main yard, well court, east yard) |
| Outside | NORTH RUIN, WEST RUIN, EAST RUIN, GATE ROAD and OLD GATE (attacker spawns), the north/south dunes and boulders |

### The keep, floor by floor

Plans render the wall runs from `src/map/harrow_keep/keep.rs` (2 chars =
1 m east-west, 1 line = 1 m north-south; local x 0..32 = world x -16..16,
local z 0..22 = world z -30..-8). `#` stone (hard), `:` plaster (soft,
breachable), `=` window, `.` doorway, gap in a wall = arch, `+` post.
Stair cores are 4 x 8 m switchbacks: two 2 m lanes, a stone spine between
them, landings at both ends.

**Basement (-3.2).** Under the whole keep; walls below ground are stone.
Entries: KEEP STAIR, WEST STAIR, EAST STAIR, CELLAR STEPS (from the desert
north of the keep straight into the cistern), and hatches from the larder
(over the cistern), kitchen (over the powder store) and great hall (over the
undercroft). Soft: cistern / powder store (with a door), oil cellar /
cells, crypt / powder store.

```text
                                       CELLAR
                                       STEPS


+###+#######+###+###+#######+#######+###+...+###+#######+#######+
#               :           #       .                           #
#               +           #       +                           +
#               :           #       #                           #
+ OIL CELLAR    + CELLS     +STAIR  +                           #
#               :           #       #       CISTERN (1A)        #
#               +           #       #                           +
#               :           #       #                           #
+###+...+#######+###+...+###+...+###+                           #
#                                   .                           #
#                                   +:::+:::+...+:::+:::+...+###+
#         UNDERCROFT                #                   #       #
+                                   +                   +PUMP   #
#                                   .                   .       #
+###+...+#######+...+#######+...+###+                   +###+...+
#       #                           :                   #       #
+       #                           +   POWDER          #       #
#       #                           .   STORE (1B)      #       #
#W.STR  +       CRYPT               +                   +E.STR  +
#       #                           :                   #       #
+       #                           +                   #       #
#       #                           :                   #       #
+#######+#######+#######+#######+###+###+#######+#######+#######+
```

**Ground (0).** Entries from outside: ENTRANCE (main arch, south), SW door
(into the west stair), WEST door (west gallery), EAST door (larder),
POSTERN (north). Soft: postern / armoury (no door: breach it), armoury,
larder and scullery sides of the great hall, guardroom and scullery sides
of the entrance. Wooden floor with a hatch in the larder; hatches in the
kitchen and great hall.

```text
+###+===+...+###+###+===+###+#######+###+###+###+===+###+===+###+
#               :           #       #                           #
+               +           #       #                           +
=               :ARMOURY    #       #                           .
+ POSTERN       + (2B)      +STAIR  +     LARDER                +
#               :           #       #                           #
+               +           #       #                           +
#               :           #       #                           =
+###+       +###+:::+...+:::+###+...+:::+...+:::+###+...+#######+
#               #                               #               #
+               +                               +               +
. WEST                  GREAT HALL (2A)         . KITCHEN       #
+ GALLERY                                       +               #
=                                               #               #
+#######+       +                               +       +#######+
#       #       #                               #       #       #
#       #       +#######+###+       +###+...+:::+...+###+       #
#       #       .       :               :               #       #
+W.STR  +       +       +               +               #E.STR  +
#       #       #GUARD  .   ENTRANCE    . SCULLERY      #       #
#       +       #       +               +               +       #
#       .       #       :               :               .       #
+###+...+###+===+###+===+###+       +###+===+###+===+###+===+###+
```

**Second floor (3.2).** No doors from outside except the east one from the
terrace stairs' landing (into the east stair). Map room and records sit
either side of the keep stair's north landing (doors from it); both open
south to the long gallery through plaster walls with doors. The floor over
the great hall and the armoury is timber, with hatches down into both.

```text
+###+===+###+===+###+===+###+#######+###+===+###+===+###+===+###+
#           :               .       .                           #
+CHART      +               +       +                           +
=           :               #       #                           =
+           +               +STAIR  +                           +
# MAP ROOM (3A)             #       #   RECORDS (3B)            #
+                           #       #                           +
=                           #       #                           =
+###+...+###+:::+...+:::+###+#######+###+:::+...+:::+###+...+###+
#                                                               #
+                                                               +
=         LONG GALLERY                                          #
+                                                               #
#                                                               #
+...+###+###+...+:::+:::+###+       +###+###+:::+...+:::+###+...+
#       #               :                   #           #       +
#       #               +                   +           #       .
#       #               .                   .           #       +
+W.STR  + INFIRMARY     +    CHAPEL         + MESS      +E.STR  +
#       #               :                   #           #       #
#       #               +                   +           #       #
#       #               :                   #           #       #
+###+===+###+===+###+===+###+===+===+###+###+===+###+===+###+###+
```

**Third floor and terrace (6.4).** The block holds the signal room and the
study, with the keep stair's top landing between them (plaster to both).
Each site has a door and a breachable plaster section onto its terrace and
windows north and south. The terrace is the 2F roof: battlements, open at
both north corners (wall-walks come in) and on the east side (terrace
stairs arrive). Hatches: in both site rooms (down to the map room and
records) and on the terrace over the map room, records and chapel. The
block's roof (9.6) has battlements but no access.

```text
                +###+===+###+#######+###+===+###+
                #           #       #           #
                +           #       #           +
                #           #STAIR  #           #
                +           +       +           +
                =SIGNAL     #       #STUDY      =
  WEST          +ROOM (4A)  +       +(4B)       + EAST
  TERRACE       :           .       .           : TERRACE
                +           +       +           +
                .           :       :           .
                +           +LANDING+           +
                #           :       :           #
                +###+===+###+#+...+#+###+===+###+




                      SOUTH TERRACE
```

### Site pairs and spawns

One pair per round (`round.site_pairs`: Cycle, Random or Fixed(n);
`--sites N` for testing). Attackers start from one of the pair's spawn
areas (`round.attacker_spawns`; `--attack-from N`). Spawn areas are ruins
about 40 m outside the walls, U-shaped 3 m stone walls that block every
line of sight from the walls, towers and keep windows.

| Pair | Site A | Site B | Defenders spawn | Attacker spawns (in order) |
|---|---|---|---|---|
| 1 BASEMENT | CISTERN (14 x 10) | POWDER STORE (10 x 12) | UNDERCROFT | NORTH RUIN, WEST RUIN, EAST RUIN, GATE ROAD |
| 2 GROUND | GREAT HALL (16 x 8) | ARMOURY (6 x 8) | LARDER | GATE ROAD, WEST, EAST, NORTH |
| 3 UPPER | MAP ROOM (14 x 8) | RECORDS (14 x 8) | LONG GALLERY | WEST, EAST, GATE ROAD, NORTH |
| 4 TOP | SIGNAL ROOM (6 x 12) | STUDY (6 x 12) | LANDING (3F) | EAST, GATE ROAD, WEST, NORTH |

Defender CCTV (16 cameras, `insert_cctv_layout` in
`src/map/harrow_keep/mod.rs`): three per site pair (each site plus the
undercroft, entrance, long gallery or landing) and four outside (main
yard and well court and east yard from the keep's facades, the gate
passage).

Ways in, by side (windows are sightlines only until a vault exists):
- **North:** POSTERN (G, next to the armoury: breach the soft wall),
  CELLAR STEPS (B, into the cistern), NW and NE tower doors up to the
  north wall-walk and onto the terrace.
- **West:** barracks postern (washroom), NW and SW towers, the well court
  to the keep's west door and west stair; the barracks roof hatch.
- **East:** granary postern (sack store), NE and SE towers, the east yard
  to the keep's east door, the terrace stairs (2F door, terrace).
- **South:** the gate passage, the main yard, the keep's main arch and SW
  door; the gatehouse and the rampart stair up to the south wall-walk.

Vertical play: hatches above every site (from the floor above), breach
holes in the timber floors over the great hall and armoury, the cellar
steps and three stairways into the basement, the terrace above the second
floor, and the wall-walks at terrace height linking the towers, barracks
and granary roofs, gatehouse roof and keep terrace.

Sightlines and chokepoints: the gate road and main yard are long and open
(cover: market stalls, carts, crates, sandbags); the wall-walks overlook
every yard; the inner-wall arches, the gate passage and the keep's main
arch are the chokepoints into the keep; inside, the long gallery (2F) and
undercroft (B) are long east-west lanes linking the stairs.

### How it is built

- `builder.rs`: wall runs from a spec string (`"s2 W s2 d p2 pd ..."`:
  stone, windows with shutters, doors with leaves, arches, plaster), floors
  (tile picker for stone/wood/hatch/opening), paving, parapets with
  merlons, curtain runs, stair cores (flights, spine, railings, floor
  openings with the headroom rule), stairs-like pieces placed uphill,
  lamps. Junction posts and caps are shared by world position.
- `keep.rs`, `compound.rs` (curtain, towers, gatehouse, rampart stairs),
  `wings.rs` (barracks, granary, old hall, stables, inner walls, yards),
  `exterior.rs` (spawn ruins, tracks, dunes, rocks), `dressing.rs`
  (scanned props).
- The desert floor leaves holes over the basement and the cellar steps
  (`DesertHoles`); nothing below ground shows from outside.
- Static batching (`src/map/batch.rs`): after load, all hard kit meshes are
  merged per material and 16 m cell (about 6000 meshes into 295) and the
  originals are despawned; soft pieces stay separate for destruction.
  `--no-batch` disables it; a headless server skips it.
- 51 lanterns, 15 of them shadow-casting (larger rooms and sites), and
  those only cast while a camera is within `lighting.lamp_shadow_distance`
  (16 m) and not on a floor above them; the others are dimmer and
  shorter-ranged (`lighting.unshadowed_lamp_scale`).
- Every staircase has a ramp collider; spawn points get temporary floors
  until the map has loaded.

**Checked (2026-11-14):** all 30 flights walked up by script (keep, towers,
gatehouse, barracks, granary, terrace, rampart and cellar steps); six
attacker routes walked segment by segment from the spawn ruins to both
sites of every pair, plus the whole wall-walk ring (towers, barracks roof,
gatehouse roof); round tests plant and win on three different pairs and
the pairs rotate per round; stand-in attackers on every spawn are hidden
from the walls, towers and keep windows. Performance (debug build,
`--no-vsync`, round running, default window, RTX 4070 Ti Super, other
builds running): median 190 to 390 fps over eight views (outside the gate,
yard, tower roof across the compound, terrace, great hall, map room,
basement, signal room).

**Section A (M2)**, `--map section_a`, `src/map/keep.rs`: the first
playable piece. A two-storey building (12 x 8 m) on the north side of a
walled courtyard (16 x 12 m) with an arched gate to the south; stairs up
the west hall; a wooden floor with a hatch over the storeroom; one site
pair (storeroom / upper east room), attackers outside the courtyard gate.

## Open questions

- Vault action (see Movement).
