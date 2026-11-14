//! Objectives: bomb-site pairs and team spawn points for each map.
//! Coordinates match the map code (`src/map/harrow_keep/`, and
//! `src/map/keep.rs` for section A).
//!
//! A map has one or more **site pairs** (two adjacent sites, A and B) and a
//! set of attacker **spawn areas** outside it. Each round the defenders
//! protect one pair: it is chosen when operator selection starts (in order
//! or at random, `round.site_pairs` in the tuning file), together with the
//! attackers' spawn area from that pair's list (`round.attacker_spawns`).
//! [`KeepLayout`] always holds the current choice, so the rest of the round
//! code only ever sees "site A, site B and the spawns".
//!
//! Harrow Keep (DESIGN.md, "Map: Harrow Keep"):
//! 1. BASEMENT: A cistern, B powder store. Defenders in the undercroft.
//! 2. GROUND: A great hall, B armoury. Defenders in the larder.
//! 3. UPPER: A map room, B records. Defenders in the long gallery.
//! 4. TOP: A signal room, B study. Defenders on the third-floor landing.
//!
//! Attackers spawn at one of four ruins about 40 m outside the walls
//! (north, west, east, south). Section A has one pair (storeroom / upper
//! east room) with attackers outside its courtyard gate.
//!
//! Sites are trigger volumes: a character whose feet are inside the box is
//! "in the site". The current pair gets painted floor lines; the HUD labels
//! them. Developer flags: `--sites N` forces pair N (1-based), and
//! `--attack-from N` the Nth spawn area of the pair's list.

use std::f32::consts::{FRAC_PI_2, PI};

use bevy::prelude::*;

use super::{Round, RoundEvent, RoundPhase, RoundSettings};
use crate::{
    combat::Team,
    config::{Rotation, Tuning},
    map::{KitLoaded, MapChoice},
};

pub struct SitesPlugin;

impl Plugin for SitesPlugin {
    fn build(&self, app: &mut App) {
        let objectives = match MapChoice::from_args() {
            MapChoice::Keep => MapObjectives::harrow_keep(),
            MapChoice::SectionA | MapChoice::Greybox => MapObjectives::section_a(),
        };
        let forced = ForcedChoice::from_args(&objectives);
        // The layout exists even when the loop is off, so other code can
        // always read it.
        let layout = objectives.layout(forced.pair.unwrap_or(0), forced.area.unwrap_or(0));
        app.insert_resource(objectives)
            .insert_resource(forced)
            .insert_resource(layout)
            .add_systems(Startup, spawn_safety_floors)
            .add_systems(Update, remove_safety_floors.run_if(resource_added::<KitLoaded>))
            .add_systems(
                Update,
                repaint_site_markings
                    .run_if(round_enabled)
                    .run_if(resource_changed::<KeepLayout>),
            );
    }
}

fn round_enabled(settings: Res<RoundSettings>) -> bool {
    settings.enabled
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SiteId {
    A,
    B,
}

impl SiteId {
    pub fn letter(self) -> &'static str {
        match self {
            SiteId::A => "A",
            SiteId::B => "B",
        }
    }
}

#[derive(Clone, Debug)]
pub struct BombSite {
    pub id: SiteId,
    pub name: &'static str,
    /// Trigger volume for a character's feet.
    pub min: Vec3,
    pub max: Vec3,
    /// Floor height inside the site.
    pub floor: f32,
}

impl BombSite {
    /// A site over the floor area x0..x1, z0..z1 at height `floor` (feet
    /// count from 0.5 m below the floor to 2 m above it).
    fn room(id: SiteId, name: &'static str, x: (f32, f32), z: (f32, f32), floor: f32) -> Self {
        Self {
            id,
            name,
            min: Vec3::new(x.0, floor - 0.5, z.0),
            max: Vec3::new(x.1, floor + 2.0, z.1),
            floor,
        }
    }

    pub fn contains(&self, feet: Vec3) -> bool {
        feet.cmpge(self.min).all() && feet.cmple(self.max).all()
    }

    /// Centre of the floor area, for markers.
    pub fn centre(&self) -> Vec3 {
        let c = (self.min + self.max) / 2.0;
        Vec3::new(c.x, self.floor, c.z)
    }

    /// Nearest point to `p` inside the site's floor area.
    pub fn clamp(&self, p: Vec3) -> Vec3 {
        let margin = Vec3::new(0.2, 0.0, 0.2);
        Vec3::new(
            p.x.clamp(self.min.x + margin.x, self.max.x - margin.x),
            self.floor,
            p.z.clamp(self.min.z + margin.z, self.max.z - margin.z),
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TeamSpawn {
    pub position: Vec3,
    /// Radians; 0 faces -Z (north).
    pub yaw: f32,
}

/// Facing for spawns (radians, 0 = north).
const NORTH: f32 = 0.0;
const SOUTH: f32 = PI;
const EAST: f32 = -FRAC_PI_2;
const WEST: f32 = FRAC_PI_2;

fn spawns(y: f32, yaw: f32, points: &[(f32, f32)]) -> Vec<TeamSpawn> {
    points
        .iter()
        .map(|&(x, z)| TeamSpawn {
            position: Vec3::new(x, y, z),
            yaw,
        })
        .collect()
}

/// Two adjacent bomb sites, defended together, and where the defenders
/// start.
#[derive(Clone, Debug)]
pub struct SitePair {
    pub name: &'static str,
    pub sites: [BombSite; 2],
    pub defender_spawns: Vec<TeamSpawn>,
    /// Indices into [`MapObjectives::areas`] the attackers may start from.
    pub attack_from: Vec<usize>,
}

/// A named attacker spawn area outside the map.
#[derive(Clone, Debug)]
pub struct SpawnArea {
    pub name: &'static str,
    pub spawns: Vec<TeamSpawn>,
}

/// Every site pair and attacker spawn area of the current map.
#[derive(Resource, Clone, Debug)]
pub struct MapObjectives {
    pub pairs: Vec<SitePair>,
    pub areas: Vec<SpawnArea>,
}

impl MapObjectives {
    /// Harrow Keep. World coordinates; the keep spans x -16..16, z -30..-8.
    /// Site boxes stop at the inner faces of the room walls (stone 0.25 m
    /// from the grid line, plaster 0.075 m).
    pub fn harrow_keep() -> Self {
        let (basement, ground, upper, top) = (-3.2, 0.0, 3.2, 6.4);
        let areas = vec![
            SpawnArea {
                name: "NORTH RUIN",
                spawns: spawns(0.0, SOUTH, &[(-1.5, -69.0), (1.5, -69.0), (-4.5, -69.0), (4.5, -69.0), (0.0, -71.5)]),
            },
            SpawnArea {
                name: "WEST RUIN",
                spawns: spawns(0.0, EAST, &[(-79.0, -1.5), (-79.0, 1.5), (-79.0, -4.5), (-79.0, 4.5), (-81.5, 0.0)]),
            },
            SpawnArea {
                name: "EAST RUIN",
                spawns: spawns(0.0, WEST, &[(79.0, 1.5), (79.0, -1.5), (79.0, 4.5), (79.0, -4.5), (81.5, 0.0)]),
            },
            SpawnArea {
                name: "GATE ROAD",
                spawns: spawns(0.0, NORTH, &[(-4.5, 71.0), (4.5, 71.0), (-7.0, 71.0), (7.0, 71.0), (-6.0, 73.5)]),
            },
        ];
        let (n, w, e, s) = (0, 1, 2, 3);
        let pairs = vec![
            SitePair {
                name: "BASEMENT",
                sites: [
                    BombSite::room(SiteId::A, "CISTERN", (2.25, 15.75), (-29.75, -20.2), basement),
                    BombSite::room(SiteId::B, "POWDER STORE", (2.25, 11.75), (-19.85, -8.25), basement),
                ],
                defender_spawns: spawns(
                    basement,
                    EAST,
                    &[(-14.0, -18.0), (-9.0, -17.2), (-5.0, -20.5), (-1.0, -17.5), (0.5, -20.5)],
                ),
                attack_from: vec![n, w, e, s],
            },
            SitePair {
                name: "GROUND",
                sites: [
                    BombSite::room(SiteId::A, "GREAT HALL", (-7.75, 7.75), (-21.85, -14.25), ground),
                    BombSite::room(SiteId::B, "ARMOURY", (-7.85, -2.25), (-29.75, -22.1), ground),
                ],
                defender_spawns: spawns(
                    ground,
                    WEST,
                    &[(5.0, -26.0), (8.0, -24.5), (13.0, -27.0), (13.0, -23.5), (14.5, -25.5)],
                ),
                attack_from: vec![s, w, e, n],
            },
            SitePair {
                name: "UPPER",
                sites: [
                    BombSite::room(SiteId::A, "MAP ROOM", (-15.75, -2.25), (-29.75, -22.25), upper),
                    BombSite::room(SiteId::B, "RECORDS", (2.25, 15.75), (-29.75, -22.25), upper),
                ],
                defender_spawns: spawns(
                    upper,
                    NORTH,
                    &[(-10.0, -19.0), (-5.0, -19.0), (0.0, -19.5), (5.0, -19.0), (10.0, -19.0)],
                ),
                attack_from: vec![w, e, s, n],
            },
            SitePair {
                name: "TOP",
                sites: [
                    BombSite::room(SiteId::A, "SIGNAL ROOM", (-7.75, -2.25), (-29.75, -18.25), top),
                    BombSite::room(SiteId::B, "STUDY", (2.25, 7.75), (-29.75, -18.25), top),
                ],
                defender_spawns: spawns(
                    top,
                    NORTH,
                    &[(-1.0, -23.0), (1.0, -23.0), (-1.0, -20.5), (1.0, -20.5), (0.0, -19.0)],
                ),
                attack_from: vec![e, s, w, n],
            },
        ];
        Self { pairs, areas }
    }

    /// Section A: one pair; the building spans x 0..12, z -8..0.
    pub fn section_a() -> Self {
        let (ground, upper) = (0.0, 3.2);
        Self {
            pairs: vec![SitePair {
                name: "SECTION A",
                sites: [
                    BombSite::room(SiteId::A, "STOREROOM", (6.25, 11.75), (-7.75, -4.08), ground),
                    BombSite::room(SiteId::B, "UPPER EAST ROOM", (6.08, 11.75), (-7.75, -0.25), upper),
                ],
                // Inside the building, both floors, facing the courtyard.
                defender_spawns: vec![
                    TeamSpawn { position: Vec3::new(4.5, ground, -3.5), yaw: SOUTH },
                    TeamSpawn { position: Vec3::new(9.0, ground, -2.0), yaw: SOUTH },
                    TeamSpawn { position: Vec3::new(9.0, upper, -3.0), yaw: SOUTH },
                    TeamSpawn { position: Vec3::new(4.0, upper, -2.5), yaw: SOUTH },
                    TeamSpawn { position: Vec3::new(8.8, ground, -6.0), yaw: SOUTH },
                ],
                attack_from: vec![0],
            }],
            // Outside the courtyard gate (arch at x 6..10, z 12), facing it.
            areas: vec![SpawnArea {
                name: "COURTYARD GATE",
                spawns: spawns(0.0, NORTH, &[(8.0, 20.0), (5.5, 19.0), (10.5, 19.0), (6.5, 22.0), (9.5, 22.0)]),
            }],
        }
    }

    /// The layout for pair `pair` with the attackers in the `area`-th entry
    /// of that pair's spawn list (both wrap around).
    pub fn layout(&self, pair: usize, area: usize) -> KeepLayout {
        let index = pair % self.pairs.len();
        let p = &self.pairs[index];
        let area = &self.areas[p.attack_from[area % p.attack_from.len()]];
        KeepLayout {
            sites: p.sites.to_vec(),
            attacker_spawns: area.spawns.clone(),
            defender_spawns: p.defender_spawns.clone(),
            pair_name: p.name,
            attack_from: area.name,
        }
    }
}

/// Bomb sites and spawn points in play this round (the current site pair).
#[derive(Resource, Clone, Debug)]
pub struct KeepLayout {
    pub sites: Vec<BombSite>,
    pub attacker_spawns: Vec<TeamSpawn>,
    pub defender_spawns: Vec<TeamSpawn>,
    pub pair_name: &'static str,
    /// Name of the attackers' spawn area.
    pub attack_from: &'static str,
}

impl KeepLayout {
    pub fn site_at(&self, feet: Vec3) -> Option<&BombSite> {
        self.sites.iter().find(|s| s.contains(feet))
    }

    pub fn site(&self, id: SiteId) -> Option<&BombSite> {
        self.sites.iter().find(|s| s.id == id)
    }

    pub fn spawns(&self, team: Team) -> &[TeamSpawn] {
        match team {
            Team::Attackers => &self.attacker_spawns,
            Team::Defenders => &self.defender_spawns,
        }
    }
}

impl Rotation {
    fn pick(self, round: u32, len: usize, rng: &mut u64) -> usize {
        match self {
            Rotation::Cycle => round.saturating_sub(1) as usize % len.max(1),
            Rotation::Random => (next_random(rng) % len.max(1) as u64) as usize,
            Rotation::Fixed(n) => (n.max(1) as usize - 1).min(len.saturating_sub(1)),
        }
    }
}

/// xorshift64*: good enough to shuffle sites.
fn next_random(state: &mut u64) -> u64 {
    if *state == 0 {
        *state = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15)
            | 1;
    }
    let mut x = *state;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    *state = x;
    x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11
}

/// `--sites N` / `--attack-from N` (1-based) override the tuning.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct ForcedChoice {
    pub pair: Option<usize>,
    pub area: Option<usize>,
}

impl ForcedChoice {
    fn from_args(objectives: &MapObjectives) -> Self {
        let args: Vec<String> = std::env::args().collect();
        let value = |flag: &str| {
            let raw = args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1))?;
            match raw.parse::<usize>() {
                Ok(n) if n >= 1 => Some(n - 1),
                _ => {
                    warn!("{flag} expects a number from 1, got {raw:?}");
                    None
                }
            }
        };
        let pair = value("--sites");
        if let Some(p) = pair
            && p >= objectives.pairs.len()
        {
            warn!("--sites {}: this map has {} site pairs", p + 1, objectives.pairs.len());
        }
        Self {
            pair,
            area: value("--attack-from"),
        }
    }
}

/// At the start of every round (operator selection), pick the site pair and
/// the attackers' spawn area. Runs right after the round flow, before the
/// characters are moved to their spawns.
pub(super) fn choose_site_pair(
    round: Res<Round>,
    tuning: Res<Tuning>,
    objectives: Res<MapObjectives>,
    forced: Res<ForcedChoice>,
    mut layout: ResMut<KeepLayout>,
    mut events: MessageReader<RoundEvent>,
    mut rng: Local<u64>,
) {
    let new_round = events.read().any(|e| {
        matches!(e, RoundEvent::PhaseStarted { phase: RoundPhase::OperatorSelect, .. })
    });
    if !new_round {
        return;
    }
    let t = &tuning.round;
    let pair = forced
        .pair
        .unwrap_or_else(|| t.site_pairs.pick(round.number, objectives.pairs.len(), &mut rng));
    let areas = objectives.pairs[pair % objectives.pairs.len()].attack_from.len();
    let area = forced
        .area
        .unwrap_or_else(|| t.attacker_spawns.pick(round.number, areas, &mut rng));
    *layout = objectives.layout(pair, area);
    info!(
        "round {}: sites {} (A {}, B {}), attackers from {}",
        round.number, layout.pair_name, layout.sites[0].name, layout.sites[1].name, layout.attack_from
    );
}

/// Invisible floor under a spawn point while the map is still loading.
#[derive(Component)]
struct SafetyFloor;

/// The first round starts on the first tick, before the kit pieces (and
/// their colliders) have loaded, so players placed on an upper floor or in
/// the basement would fall through it. A small collider under every spawn
/// point holds them until the map is in.
fn spawn_safety_floors(mut commands: Commands, objectives: Res<MapObjectives>) {
    let points = objectives
        .pairs
        .iter()
        .flat_map(|p| &p.defender_spawns)
        .chain(objectives.areas.iter().flat_map(|a| &a.spawns));
    for spawn in points {
        commands.spawn((
            SafetyFloor,
            avian3d::prelude::RigidBody::Static,
            avian3d::prelude::Collider::cuboid(1.0, 0.1, 1.0),
            Transform::from_translation(spawn.position - Vec3::Y * 0.05),
        ));
    }
}

fn remove_safety_floors(floors: Query<Entity, With<SafetyFloor>>, mut commands: Commands) {
    for floor in &floors {
        commands.entity(floor).despawn();
    }
}

/// Marks the painted lines of the current pair.
#[derive(Component)]
struct SiteMarkings;

/// Painted ochre lines around each site of the current pair.
fn repaint_site_markings(
    mut commands: Commands,
    layout: Res<KeepLayout>,
    old: Query<Entity, With<SiteMarkings>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut paint: Local<Option<Handle<StandardMaterial>>>,
) {
    for entity in &old {
        commands.entity(entity).despawn();
    }
    let paint = paint
        .get_or_insert_with(|| {
            materials.add(StandardMaterial {
                base_color: Color::srgb(0.85, 0.62, 0.16),
                perceptual_roughness: 0.9,
                ..default()
            })
        })
        .clone();
    const WIDTH: f32 = 0.08;
    const INSET: f32 = 0.15;
    for site in &layout.sites {
        let (min, max) = (site.min + Vec3::splat(INSET), site.max - Vec3::splat(INSET));
        let y = site.floor + 0.006;
        let size = max - min;
        let centre = (min + max) / 2.0;
        let edges = [
            // (centre, size x, size z)
            (Vec3::new(centre.x, y, min.z), size.x, WIDTH),
            (Vec3::new(centre.x, y, max.z), size.x, WIDTH),
            (Vec3::new(min.x, y, centre.z), WIDTH, size.z),
            (Vec3::new(max.x, y, centre.z), WIDTH, size.z),
        ];
        let parent = commands
            .spawn((
                Name::new(format!("Site {} markings", site.id.letter())),
                SiteMarkings,
                Transform::default(),
                Visibility::default(),
            ))
            .id();
        for (at, sx, sz) in edges {
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(sx, 0.01, sz))),
                MeshMaterial3d(paint.clone()),
                Transform::from_translation(at),
                ChildOf(parent),
            ));
        }
    }
}
