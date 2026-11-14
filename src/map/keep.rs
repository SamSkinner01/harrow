//! Harrow Keep, first section: a two-storey building on the north side of a
//! walled courtyard, assembled from the fortress kit.
//!
//! Map view (north = -Z is up, x to the right), metres:
//!
//! ```text
//!   z=-8  +-----------+-----------+
//!         | stairs    | storeroom |  <- hatch above (upper floor)
//!         | |  hall   +--soft-----+
//!         | |         | room      |
//!   z= 0  +---arch----+--door-----+-----+
//!         |                             |
//!         |         courtyard           |
//!         |                             |
//!   z=12  +-----------gate--------------+
//!        x=0         x=6         x=12  x=16
//! ```
//!
//! Upper floor: two rooms split by a plaster wall, a stairwell with a railing,
//! a wooden floor with a hatch over the storeroom. Roof: battlements.

use std::collections::HashSet;

use bevy::prelude::*;

use super::kit::piece;
use crate::player::SpawnPoint;

pub struct KeepPlugin;

impl Plugin for KeepPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(SpawnPoint {
            position: Vec3::new(8.0, 0.0, 9.0),
            yaw: 0.0,
            pitch: 0.0,
        })
        .add_systems(Startup, (build_section, place_lamps));
    }
}

/// One lantern per room, hanging from the ceiling (slab underside).
fn place_lamps(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    tuning: Res<crate::config::Tuning>,
) {
    let ground_ceiling = GROUND + WALL;
    let upper_ceiling = UPPER + WALL;
    for (x, y, z) in [
        (3.5, ground_ceiling, -4.0), // west hall
        (9.0, ground_ceiling, -2.0), // courtyard-side room
        (9.0, ground_ceiling, -5.6), // storeroom (clear of the hatch)
        (3.5, upper_ceiling, -4.0),  // upper west room
        (9.0, upper_ceiling, -4.0),  // upper east room
    ] {
        super::lamps::spawn_lamp(
            &mut commands,
            &mut meshes,
            &mut materials,
            &tuning,
            Vec3::new(x, y, z),
        );
    }
}

/// Storey heights: floor top of each level.
const GROUND: f32 = 0.0;
const UPPER: f32 = 3.2;
const ROOF: f32 = 6.4;
/// Wall height; the 0.2 m slab layer sits on top.
const WALL: f32 = 3.0;

#[derive(Clone, Copy)]
enum Dir {
    East,
    North,
    West,
    South,
}

impl Dir {
    /// Kit pieces run along +X with their exterior on +Z, i.e. on the right
    /// of the direction of travel (map view). Rotating keeps that true.
    fn rotation(self) -> Quat {
        let quarter = std::f32::consts::FRAC_PI_2;
        Quat::from_rotation_y(match self {
            Dir::East => 0.0,
            Dir::North => quarter,
            Dir::West => 2.0 * quarter,
            Dir::South => 3.0 * quarter,
        })
    }

    fn step(self) -> IVec2 {
        match self {
            Dir::East => IVec2::new(1, 0),
            Dir::North => IVec2::new(0, -1),
            Dir::West => IVec2::new(-1, 0),
            Dir::South => IVec2::new(0, 1),
        }
    }
}

/// What goes with a run of walls.
#[derive(Clone, Copy, Default)]
struct Trim {
    /// Stone band + caps in the slab layer on top of the walls (exterior
    /// walls with a floor or roof above).
    band: bool,
    /// Stone band + caps below the walls, hiding the gap to the desert floor.
    plinth: bool,
}

struct Builder<'w, 's, 'a> {
    commands: Commands<'w, 's>,
    assets: &'a AssetServer,
    /// (x, z, y in cm) of every post/cap/merlon already placed.
    junctions: HashSet<(i32, i32, i32)>,
}

impl Builder<'_, '_, '_> {
    fn place(&mut self, name: &str, at: Vec3, rotation: Quat) {
        let bundle = piece(self.assets, name);
        self.commands
            .spawn((bundle, Transform::from_translation(at).with_rotation(rotation)));
    }

    /// Junction piece (post, cap or merlon) at a grid point, once per height.
    fn junction(&mut self, name: &str, p: IVec2, y: f32) {
        if self.junctions.insert((p.x, p.y, (y * 100.0).round() as i32)) {
            self.place(name, Vec3::new(p.x as f32, y, p.y as f32), Quat::IDENTITY);
        }
    }

    fn has_junction(&self, p: IVec2, y: f32) -> bool {
        self.junctions.contains(&(p.x, p.y, (y * 100.0).round() as i32))
    }

    /// A run of wall pieces from grid point `start` in direction `dir`,
    /// with posts at every junction. Piece lengths come from their names.
    fn walls(&mut self, start: (i32, i32), dir: Dir, y: f32, pieces: &[&str], trim: Trim) {
        let mut p = IVec2::new(start.0, start.1);
        self.junction("stone_post", p, y);
        self.trim_junction(p, y, trim);
        for &name in pieces {
            let len = length_of(name);
            let at = Vec3::new(p.x as f32, y, p.y as f32);
            self.place(name, at, dir.rotation());
            if trim.band {
                self.place(&format!("stone_band_{len}m"), at + Vec3::Y * WALL, dir.rotation());
            }
            if trim.plinth {
                self.place(&format!("stone_band_{len}m"), at - Vec3::Y * 0.2, dir.rotation());
            }
            p += dir.step() * len;
            self.junction("stone_post", p, y);
            self.trim_junction(p, y, trim);
        }
    }

    fn trim_junction(&mut self, p: IVec2, y: f32, trim: Trim) {
        if trim.band {
            self.junction("stone_cap", p, y + WALL);
        }
        if trim.plinth {
            self.junction("stone_cap", p, y - 0.2);
        }
    }

    /// Battlements: parapet pieces with merlons at the junctions. Merlons are
    /// skipped where a building post continues upwards.
    fn parapets(&mut self, start: (i32, i32), dir: Dir, y: f32, lengths: &[i32]) {
        let mut p = IVec2::new(start.0, start.1);
        self.merlon(p, y);
        for &len in lengths {
            let at = Vec3::new(p.x as f32, y, p.y as f32);
            self.place(&format!("parapet_{len}m"), at, dir.rotation());
            p += dir.step() * len;
            self.merlon(p, y);
        }
    }

    fn merlon(&mut self, p: IVec2, y: f32) {
        if !self.has_junction(p, y + 0.2) && !self.has_junction(p, y) {
            self.junction("merlon", p, y);
        }
    }

    /// Fill a rectangle with 2 m floor tiles (top at `y`), skipping tiles
    /// whose min corner is in `skip`.
    fn floor(&mut self, min: (i32, i32), max: (i32, i32), y: f32, name: &str, skip: &[(i32, i32)]) {
        for x in (min.0..max.0).step_by(2) {
            for z in (min.1..max.1).step_by(2) {
                if !skip.contains(&(x, z)) {
                    self.place(name, Vec3::new(x as f32, y, z as f32), Quat::IDENTITY);
                }
            }
        }
    }

    fn prop(&mut self, name: &str, x: f32, y: f32, z: f32, yaw_degrees: f32) {
        self.place(
            name,
            Vec3::new(x, y, z),
            Quat::from_rotation_y(yaw_degrees.to_radians()),
        );
    }
}

/// "stone_wall_4m" -> 4.
fn length_of(name: &str) -> i32 {
    name.rsplit('_')
        .next()
        .and_then(|s| s.strip_suffix('m'))
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("kit piece {name:?} has no _<n>m length suffix"))
}

fn build_section(commands: Commands, assets: Res<AssetServer>) {
    let mut b = Builder {
        commands,
        assets: &assets,
        junctions: HashSet::new(),
    };
    use Dir::*;
    let exterior = Trim {
        band: true,
        plinth: true,
    };
    let facade = Trim {
        band: true,
        plinth: false, // stands on courtyard paving
    };
    let interior = Trim::default();

    // ---- Building, ground floor (x 0..12, z -8..0) --------------------------
    // Exterior traced counter-clockwise so the outside is always on the right.
    b.walls((0, 0), East, GROUND, &[
        "stone_wall_window_2m", "stone_wall_arch_2m", "stone_wall_2m",
        "stone_wall_window_2m", "stone_wall_door_2m", "stone_wall_window_2m",
    ], facade);
    b.walls((12, 0), North, GROUND, &[
        "stone_wall_2m", "stone_wall_window_2m", "stone_wall_2m", "stone_wall_door_2m",
    ], exterior);
    b.walls((12, -8), West, GROUND, &[
        "stone_wall_4m", "stone_wall_window_2m", "stone_wall_2m", "stone_wall_window_2m",
        "stone_wall_2m",
    ], exterior);
    b.walls((0, -8), South, GROUND, &["stone_wall_4m", "stone_wall_2m", "stone_wall_window_2m"], exterior);
    // Interior: stone wall splitting west hall from the east rooms, and a
    // plaster wall between the east rooms.
    b.walls((6, 0), North, GROUND, &["stone_wall_2m", "stone_wall_door_2m", "stone_wall_4m"], interior);
    b.walls((6, -4), East, GROUND, &["soft_wall_2m", "soft_wall_door_2m", "soft_wall_2m"], interior);
    b.floor((0, -8), (12, 0), GROUND, "floor_stone_2m", &[]);
    // Stairs along the west wall, top step at the stairwell's north edge.
    b.place("stairs_3_2m", Vec3::new(0.0, GROUND, -1.2), Quat::IDENTITY);

    // ---- Building, upper floor ---------------------------------------------
    b.walls((0, 0), East, UPPER, &[
        "stone_wall_window_2m", "stone_wall_window_2m", "stone_wall_2m",
        "stone_wall_window_2m", "stone_wall_2m", "stone_wall_window_2m",
    ], Trim { band: true, plinth: false });
    b.walls((12, 0), North, UPPER, &[
        "stone_wall_2m", "stone_wall_window_2m", "stone_wall_2m", "stone_wall_window_2m",
    ], Trim { band: true, plinth: false });
    b.walls((12, -8), West, UPPER, &[
        "stone_wall_4m", "stone_wall_window_2m", "stone_wall_2m", "stone_wall_window_2m",
        "stone_wall_2m",
    ], Trim { band: true, plinth: false });
    b.walls((0, -8), South, UPPER, &["stone_wall_4m", "stone_wall_2m", "stone_wall_window_2m"],
        Trim { band: true, plinth: false });
    b.walls((6, 0), North, UPPER, &[
        "soft_wall_2m", "soft_wall_door_2m", "soft_wall_2m", "soft_wall_2m",
    ], interior);
    // Stone floor, open over the stairs, wooden over the storeroom (soft,
    // with a hatch).
    let stairwell = [(0, -6), (0, -4), (0, -2)];
    let storeroom = [(6, -8), (8, -8), (10, -8), (6, -6), (8, -6), (10, -6)];
    let mut skip = stairwell.to_vec();
    skip.extend(storeroom);
    b.floor((0, -8), (12, 0), UPPER, "floor_stone_2m", &skip);
    for &(x, z) in &storeroom {
        let name = if (x, z) == (8, -8) { "floor_hatch_2m" } else { "floor_wood_2m" };
        b.prop(name, x as f32, UPPER, z as f32, 0.0);
    }
    b.prop("hatch_cover", 8.0, UPPER, -8.0, 0.0);
    for z in [0.0, -2.0, -4.0] {
        b.place("railing_2m", Vec3::new(2.0, UPPER, z), North.rotation());
    }

    // ---- Roof -----------------------------------------------------------------
    b.floor((0, -8), (12, 0), ROOF, "floor_stone_2m", &[]);
    b.parapets((0, 0), East, ROOF, &[2, 2, 2, 2, 2, 2]);
    b.parapets((12, 0), North, ROOF, &[2, 2, 2, 2]);
    b.parapets((12, -8), West, ROOF, &[4, 2, 2, 2, 2]);
    b.parapets((0, -8), South, ROOF, &[4, 2, 2]);

    // ---- Courtyard (x 0..16, z 0..12) -----------------------------------------
    b.floor((0, 0), (16, 12), GROUND, "floor_stone_2m", &[]);
    let wall = Trim { band: false, plinth: true };
    b.walls((0, 0), South, GROUND, &["stone_wall_4m", "stone_wall_4m", "stone_wall_4m"], wall);
    b.walls((0, 12), East, GROUND, &[
        "stone_wall_4m", "stone_wall_2m", "stone_wall_arch_4m", "stone_wall_2m", "stone_wall_4m",
    ], wall);
    b.walls((16, 12), North, GROUND, &["stone_wall_4m", "stone_wall_4m", "stone_wall_4m"], wall);
    b.walls((16, 0), West, GROUND, &["stone_wall_4m"], wall);
    b.parapets((0, 0), South, WALL, &[4, 4, 4]);
    b.parapets((0, 12), East, WALL, &[4, 2, 4, 2, 4]);
    b.parapets((16, 12), North, WALL, &[4, 4, 4]);
    b.parapets((16, 0), West, WALL, &[4]);

    // ---- Props ------------------------------------------------------------------
    // Courtyard cover.
    b.prop("sandbags_2m", 5.0, GROUND, 5.0, 0.0);
    b.prop("sandbags_2m", 11.5, GROUND, 7.5, 90.0);
    b.prop("crate_large", 2.5, GROUND, 9.5, 10.0);
    b.prop("crate_small", 2.5, 1.0, 9.5, 35.0);
    b.prop("crate_small", 3.7, GROUND, 10.4, -5.0);
    b.prop("crate_large", 13.8, GROUND, 2.3, 0.0);
    // Defenders have barricaded the east room's courtyard door and the
    // storeroom's outer door.
    b.prop("barricade_door", 9.0, GROUND, 0.0, 0.0);
    b.place("barricade_door", Vec3::new(12.0, GROUND, -7.0), North.rotation());
    // Storeroom and upper rooms.
    b.prop("crate_large", 7.2, GROUND, -7.0, 0.0);
    b.prop("crate_large", 10.8, GROUND, -5.0, 15.0);
    b.prop("crate_small", 7.2, 1.0, -7.0, 20.0);
    b.prop("crate_small", 10.6, UPPER, -1.2, 0.0);
    b.prop("sandbags_2m", 3.5, UPPER, -6.8, 0.0);
}
