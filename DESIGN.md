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
- Not yet: rooftop access, basement, interior lamps (interiors are very dark
  with only window light), a proper sky (a physically based atmosphere was
  tried and looked grey; needs a lighting pass).

## Open questions

- Vault action (see Movement).
