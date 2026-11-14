//! Objective layout for Harrow Keep, section A: the two bomb sites and the
//! team spawn points. Kept here (keyed to the keep) rather than in the map
//! module; coordinates match `src/map/keep.rs`.
//!
//! - Site A: ground-floor storeroom (x 6..12, z -8..-4).
//! - Site B: upper-floor east room (x 6..12, z -8..0, floor at y 3.2).
//! - Attackers spawn outside the courtyard gate (z 18..22); defenders inside
//!   the building, on both floors.
//!
//! Sites are trigger volumes: a character whose feet are inside the box is
//! "in the site". Each site gets painted floor lines; the HUD labels them.

use bevy::prelude::*;

use super::RoundSettings;
use crate::combat::Team;

pub struct SitesPlugin;

impl Plugin for SitesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_site_markings.run_if(round_enabled));
        // The layout exists even when the loop is off, so other code can
        // always read it.
        app.insert_resource(KeepLayout::section_a());
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
    /// Radians; 0 faces -Z (north, towards the keep).
    pub yaw: f32,
}

/// Bomb sites and spawn points of the current map.
#[derive(Resource, Clone, Debug)]
pub struct KeepLayout {
    pub sites: Vec<BombSite>,
    pub attacker_spawns: Vec<TeamSpawn>,
    pub defender_spawns: Vec<TeamSpawn>,
}

impl KeepLayout {
    pub fn section_a() -> Self {
        // Walls are centred on the grid lines: stone 0.5 m thick, plaster
        // 0.15 m, so rooms are a little smaller than their grid cells.
        let ground = 0.0;
        let upper = 3.2;
        let north = 0.0;
        let south = std::f32::consts::PI;
        let spawn = |x: f32, y: f32, z: f32, yaw: f32| TeamSpawn {
            position: Vec3::new(x, y, z),
            yaw,
        };
        Self {
            sites: vec![
                BombSite {
                    id: SiteId::A,
                    name: "STOREROOM",
                    min: Vec3::new(6.25, ground - 0.5, -7.75),
                    max: Vec3::new(11.75, ground + 2.0, -4.08),
                    floor: ground,
                },
                BombSite {
                    id: SiteId::B,
                    name: "UPPER EAST ROOM",
                    min: Vec3::new(6.08, upper - 0.5, -7.75),
                    max: Vec3::new(11.75, upper + 2.0, -0.25),
                    floor: upper,
                },
            ],
            // Outside the courtyard gate (arch at x 6..10, z 12), facing it.
            attacker_spawns: vec![
                spawn(8.0, ground, 20.0, north),
                spawn(5.5, ground, 19.0, north),
                spawn(10.5, ground, 19.0, north),
                spawn(6.5, ground, 22.0, north),
                spawn(9.5, ground, 22.0, north),
            ],
            // Inside the building, both floors, facing the courtyard.
            defender_spawns: vec![
                spawn(4.5, ground, -3.5, south),
                spawn(9.0, ground, -2.0, south),
                spawn(9.0, upper, -3.0, south),
                spawn(4.0, upper, -2.5, south),
                spawn(8.8, ground, -6.0, south),
            ],
        }
    }

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

/// Painted ochre lines around each site's floor.
fn spawn_site_markings(
    mut commands: Commands,
    layout: Res<KeepLayout>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let paint = materials.add(StandardMaterial {
        base_color: Color::srgb(0.85, 0.62, 0.16),
        perceptual_roughness: 0.9,
        ..default()
    });
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
