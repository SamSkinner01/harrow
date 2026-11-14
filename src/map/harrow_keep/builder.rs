//! Placement helpers: everything in Harrow Keep is put down through a
//! [`Builder`], which spawns fortress-kit pieces on the 1 m grid.
//!
//! Each building works in its own local grid (`Builder::at` sets the
//! origin); junction posts, caps and merlons are de-duplicated in world
//! space, so runs from different buildings can share a corner.
//!
//! Wall runs use a small spec language, one token per piece, laid from a
//! grid point in a direction with the exterior (+Z of the kit) on the right:
//!
//! | token | piece |
//! |---|---|
//! | `s1` `s2` `s4` | stone wall 1/2/4 m |
//! | `d` / `D` | stone doorway / with an open door leaf (on the left side) |
//! | `w` / `W` | stone window / with open shutters (on the right side) |
//! | `a2` `a4` | stone arch 2/4 m |
//! | `r4` | wide arcade arch 4 m |
//! | `p1` `p2` `pd` `pw` | plaster (soft) wall 1/2 m, door, window |
//! | `_1` `_2` `_4` | no wall for that many metres (posts still go in) |

use std::collections::HashSet;

use bevy::prelude::*;

use crate::map::kit::piece;

/// Wall height; the 0.2 m slab layer sits on top.
pub const WALL: f32 = 3.0;
/// Floor-to-floor height.
pub const STOREY: f32 = 3.2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    East,
    North,
    West,
    South,
}

impl Dir {
    /// Kit pieces run along +X with their exterior on +Z, i.e. on the right
    /// of the direction of travel (map view, north up). Rotating keeps that.
    pub fn rotation(self) -> Quat {
        let quarter = std::f32::consts::FRAC_PI_2;
        Quat::from_rotation_y(match self {
            Dir::East => 0.0,
            Dir::North => quarter,
            Dir::West => 2.0 * quarter,
            Dir::South => 3.0 * quarter,
        })
    }

    pub fn step(self) -> IVec2 {
        match self {
            Dir::East => IVec2::new(1, 0),
            Dir::North => IVec2::new(0, -1),
            Dir::West => IVec2::new(-1, 0),
            Dir::South => IVec2::new(0, 1),
        }
    }

    pub fn opposite(self) -> Dir {
        match self {
            Dir::East => Dir::West,
            Dir::North => Dir::South,
            Dir::West => Dir::East,
            Dir::South => Dir::North,
        }
    }

    /// The direction on your right when facing this way (map view).
    pub fn right(self) -> Dir {
        match self {
            Dir::North => Dir::East,
            Dir::East => Dir::South,
            Dir::South => Dir::West,
            Dir::West => Dir::North,
        }
    }

    /// Rotation for stairs-like pieces (lane x 0..2, rising towards -Z) so
    /// they rise towards `self`. The lane then lies on the right of the
    /// origin when facing uphill.
    pub fn uphill(self) -> Quat {
        self.right().rotation()
    }
}

/// What goes with a run of walls.
#[derive(Clone, Copy, Default)]
pub struct Trim {
    /// Stone band + caps in the slab layer on top of the walls, exterior
    /// half (where no floor tile covers it).
    pub band: bool,
    /// Stone band + caps below the walls on the exterior face, hiding the
    /// gap to the ground.
    pub plinth: bool,
    /// The same on the interior half / face (free-standing walls).
    pub band_in: bool,
    pub plinth_in: bool,
}

impl Trim {
    pub const NONE: Trim = Trim { band: false, plinth: false, band_in: false, plinth_in: false };
    pub const BAND: Trim = Trim { band: true, ..Trim::NONE };
    pub const GROUND: Trim = Trim { band: true, plinth: true, ..Trim::NONE };
    /// Free-standing wall: band and plinth on both faces.
    pub const FREE: Trim = Trim { band: true, plinth: true, band_in: true, plinth_in: true };
    /// Ground-floor wall with paving (not a floor tile) on its inner side.
    pub const OPEN: Trim = Trim { band: true, plinth: true, band_in: false, plinth_in: true };
}

/// A lantern to hang once the build is done.
#[derive(Clone, Copy, Debug)]
pub struct LampSpot {
    /// Ceiling (slab underside) point, world space.
    pub ceiling: Vec3,
    /// Casts shadows (costs a shadow map); use for bigger rooms only.
    pub shadows: bool,
}

pub struct Builder<'w, 's, 'a> {
    pub commands: Commands<'w, 's>,
    assets: &'a AssetServer,
    /// World position of the current building's local grid origin.
    origin: IVec2,
    /// World (x, z, y) in cm of every post / cap / merlon placed.
    junctions: HashSet<(i32, i32, i32)>,
    pub lamps: Vec<LampSpot>,
    pub pieces: usize,
}

fn key(p: IVec2, y: f32) -> (i32, i32, i32) {
    (p.x * 100, p.y * 100, (y * 100.0).round() as i32)
}

impl<'w, 's, 'a> Builder<'w, 's, 'a> {
    pub fn new(commands: Commands<'w, 's>, assets: &'a AssetServer) -> Self {
        Self {
            commands,
            assets,
            origin: IVec2::ZERO,
            junctions: HashSet::new(),
            lamps: Vec::new(),
            pieces: 0,
        }
    }

    /// Work in a local grid whose (0, 0) is world (x, z).
    pub fn at(&mut self, x: i32, z: i32) {
        self.origin = IVec2::new(x, z);
    }

    pub fn world(&self, x: f32, y: f32, z: f32) -> Vec3 {
        Vec3::new(x + self.origin.x as f32, y, z + self.origin.y as f32)
    }

    /// Place a kit piece at a local position.
    pub fn place(&mut self, name: &str, local: Vec3, rotation: Quat) -> Entity {
        let at = self.world(local.x, local.y, local.z);
        self.pieces += 1;
        self.commands
            .spawn((piece(self.assets, name), Transform::from_translation(at).with_rotation(rotation)))
            .id()
    }

    /// A prop at local (x, y, z), turned `yaw` degrees (counter-clockwise
    /// seen from above).
    pub fn prop(&mut self, name: &str, x: f32, y: f32, z: f32, yaw: f32) -> Entity {
        self.place(name, Vec3::new(x, y, z), Quat::from_rotation_y(yaw.to_radians()))
    }

    /// A scanned prop (`assets/models/props/`) at local (x, y, z), turned
    /// `yaw` degrees and scaled uniformly.
    pub fn model(&mut self, name: &str, x: f32, y: f32, z: f32, yaw: f32, scale: f32) {
        let transform = Transform::from_translation(self.world(x, y, z))
            .with_rotation(Quat::from_rotation_y(yaw.to_radians()))
            .with_scale(Vec3::splat(scale));
        self.commands.spawn(crate::map::props::prop(self.assets, name, transform));
    }

    /// Junction piece (post, cap or merlon) at a local grid point, once per
    /// world position and height.
    pub fn junction(&mut self, name: &str, p: IVec2, y: f32) {
        if self.junctions.insert(key(p + self.origin, y)) {
            self.place(name, Vec3::new(p.x as f32, y, p.y as f32), Quat::IDENTITY);
        }
    }

    pub fn has_junction(&self, p: IVec2, y: f32) -> bool {
        self.junctions.contains(&key(p + self.origin, y))
    }

    /// A run of walls from local grid point `start` going `dir`, with posts
    /// at every junction. See the module docs for the spec tokens.
    pub fn run(&mut self, start: (i32, i32), dir: Dir, y: f32, spec: &str, trim: Trim) {
        let mut p = IVec2::new(start.0, start.1);
        self.post(p, y, trim);
        for token in spec.split_whitespace() {
            let (name, len, extra): (Option<&str>, i32, Option<&str>) = match token {
                "s1" => (Some("stone_wall_1m"), 1, None),
                "s2" => (Some("stone_wall_2m"), 2, None),
                "s4" => (Some("stone_wall_4m"), 4, None),
                "d" => (Some("stone_wall_door_2m"), 2, None),
                "D" => (Some("stone_wall_door_2m"), 2, Some("door_leaf")),
                "w" => (Some("stone_wall_window_2m"), 2, None),
                "W" => (Some("stone_wall_window_2m"), 2, Some("shutters_2m")),
                "a2" => (Some("stone_wall_arch_2m"), 2, None),
                "a4" => (Some("stone_wall_arch_4m"), 4, None),
                "r4" => (Some("arcade_arch_4m"), 4, None),
                "p1" => (Some("soft_wall_1m"), 1, None),
                "p2" => (Some("soft_wall_2m"), 2, None),
                "pd" => (Some("soft_wall_door_2m"), 2, None),
                "pw" => (Some("soft_wall_window_2m"), 2, None),
                "_1" => (None, 1, None),
                "_2" => (None, 2, None),
                "_4" => (None, 4, None),
                other => panic!("unknown wall token {other:?} in {spec:?}"),
            };
            let at = Vec3::new(p.x as f32, y, p.y as f32);
            let rotation = dir.rotation();
            if let Some(name) = name {
                self.place(name, at, rotation);
                if let Some(extra) = extra {
                    self.place(extra, at, rotation);
                }
                self.trim_piece(at, dir, len, trim);
            }
            p += dir.step() * len;
            self.post(p, y, trim);
        }
    }

    fn trim_piece(&mut self, at: Vec3, dir: Dir, len: i32, trim: Trim) {
        let band = format!("stone_band_{len}m");
        if trim.band {
            self.place(&band, at + Vec3::Y * WALL, dir.rotation());
        }
        if trim.plinth {
            self.place(&band, at - Vec3::Y * 0.2, dir.rotation());
        }
        // The other face: the same band run backwards from the far end.
        let step = dir.step() * len;
        let far = at + Vec3::new(step.x as f32, 0.0, step.y as f32);
        let back = dir.opposite().rotation();
        if trim.band_in {
            self.place(&band, far + Vec3::Y * WALL, back);
        }
        if trim.plinth_in {
            self.place(&band, far - Vec3::Y * 0.2, back);
        }
    }

    fn post(&mut self, p: IVec2, y: f32, trim: Trim) {
        self.junction("stone_post", p, y);
        if trim.band || trim.band_in {
            self.junction("stone_cap", p, y + WALL);
        }
        if trim.plinth || trim.plinth_in {
            self.junction("stone_cap", p, y - 0.2);
        }
    }

    /// Battlements along a roof edge: parapet pieces with merlons at the
    /// junctions. Tokens: `1` `2` `4` (parapet), `_2` `_4` (gap); a leading
    /// `!` on a token skips the merlon at the junction where it *ends*, and
    /// a lone `!` first skips the starting one. Merlons are also skipped
    /// where a wall post continues upwards.
    pub fn parapets(&mut self, start: (i32, i32), dir: Dir, y: f32, spec: &str) {
        let mut p = IVec2::new(start.0, start.1);
        let mut tokens = spec.split_whitespace().peekable();
        if tokens.peek() == Some(&"!") {
            tokens.next();
        } else {
            self.merlon(p, y);
        }
        for token in tokens {
            let (skip_end, body) = match token.strip_prefix('!') {
                Some(rest) => (true, rest),
                None => (false, token),
            };
            let (gap, len) = match body {
                "1" => (false, 1),
                "2" => (false, 2),
                "4" => (false, 4),
                "_1" => (true, 1),
                "_2" => (true, 2),
                "_4" => (true, 4),
                other => panic!("unknown parapet token {other:?} in {spec:?}"),
            };
            if !gap {
                let at = Vec3::new(p.x as f32, y, p.y as f32);
                self.place(&format!("parapet_{len}m"), at, dir.rotation());
            }
            p += dir.step() * len;
            if !skip_end {
                self.merlon(p, y);
            }
        }
    }

    fn merlon(&mut self, p: IVec2, y: f32) {
        if !self.has_junction(p, y + 0.2) && !self.has_junction(p, y) {
            self.junction("merlon", p, y);
        }
    }

    /// Fill a rectangle (local, min corner inclusive) with 2 m floor tiles
    /// whose top is at `y`. `pick` returns the piece for each tile (by its
    /// min corner), or `None` to leave a hole. Hatch tiles get a cover.
    pub fn floor(&mut self, min: (i32, i32), max: (i32, i32), y: f32, pick: impl Fn(i32, i32) -> Option<&'static str>) {
        for x in (min.0..max.0).step_by(2) {
            for z in (min.1..max.1).step_by(2) {
                if let Some(name) = pick(x, z) {
                    let at = Vec3::new(x as f32, y, z as f32);
                    self.place(name, at, Quat::IDENTITY);
                    if name == "floor_hatch_2m" {
                        self.place("hatch_cover", at, Quat::IDENTITY);
                    }
                }
            }
        }
    }

    /// Courtyard paving: 4 m tiles where they fit, 2 m along the edges.
    /// Paving sits 1 cm below building floors so it can run under walls
    /// and plinths without flickering.
    pub fn paving(&mut self, min: (i32, i32), max: (i32, i32)) {
        const Y: f32 = -0.01;
        let (w, d) = (max.0 - min.0, max.1 - min.1);
        let (w4, d4) = (w - w % 4, d - d % 4);
        for x in (0..w).step_by(2) {
            for z in (0..d).step_by(2) {
                let big = x < w4 && z < d4;
                if big && x % 4 == 0 && z % 4 == 0 {
                    let at = Vec3::new((min.0 + x) as f32, Y, (min.1 + z) as f32);
                    self.place("floor_stone_4m", at, Quat::IDENTITY);
                } else if !big {
                    let at = Vec3::new((min.0 + x) as f32, Y, (min.1 + z) as f32);
                    self.place("floor_stone_2m", at, Quat::IDENTITY);
                }
            }
        }
    }

    /// A stairs-like piece (lane x 0..2, rising towards -Z: stairs, landings,
    /// bases) placed so it rises towards `up`, with `base` the bottom end of
    /// the lane's left edge (facing uphill), in local coordinates.
    pub fn uphill(&mut self, name: &str, base: (f32, f32), y: f32, up: Dir) -> Entity {
        self.place(name, Vec3::new(base.0, y, base.1), up.uphill())
    }

    /// A 2 m railing from local grid point `start` going `dir`.
    pub fn railing(&mut self, start: (i32, i32), dir: Dir, y: f32, count: i32) {
        let mut p = IVec2::new(start.0, start.1);
        for _ in 0..count {
            self.place("railing_2m", Vec3::new(p.x as f32, y, p.y as f32), dir.rotation());
            p += dir.step() * 2;
        }
    }

    /// A curtain-wall run (3 m thick, walk at 6.4 m) from `start` going
    /// `dir`. Posts only between pieces: the ends butt against towers and
    /// buildings.
    pub fn curtain(&mut self, start: (i32, i32), dir: Dir, lengths: &[i32]) {
        let mut p = IVec2::new(start.0, start.1);
        for (i, &len) in lengths.iter().enumerate() {
            if i > 0 {
                self.place("curtain_post", Vec3::new(p.x as f32, 0.0, p.y as f32), Quat::IDENTITY);
            }
            self.place(
                &format!("curtain_wall_{len}m"),
                Vec3::new(p.x as f32, 0.0, p.y as f32),
                dir.rotation(),
            );
            p += dir.step() * len;
        }
    }

    /// Hang a lantern from the ceiling above local (x, z) of the storey
    /// whose floor is at `floor_y`.
    pub fn lamp(&mut self, x: f32, z: f32, floor_y: f32, shadows: bool) {
        let ceiling = self.world(x, floor_y + WALL, z);
        self.lamps.push(LampSpot { ceiling, shadows });
    }
}

/// A switchback stair core, 4 m wide (two 2 m lanes along local x) and
/// 8 m long (along z): flights alternate between the lanes and directions,
/// with a stone spine wall between the lanes and landings at both ends.
/// The core's own outer walls are drawn by the building.
#[derive(Clone, Copy, Debug)]
pub struct Core {
    /// Local min corner (x, z) of the 4 x 8 m footprint.
    pub min: (i32, i32),
    /// Floor height of the lowest level.
    pub bottom: f32,
    /// Number of flights (levels served = flights + 1).
    pub flights: usize,
    /// Direction of the first flight: North or South.
    pub up0: Dir,
    /// The first flight uses the east lane (x 2..4) rather than the west.
    pub first_east: bool,
}

impl Core {
    pub fn level_y(&self, level: usize) -> f32 {
        self.bottom + level as f32 * STOREY
    }

    fn lane_x(&self, flight: usize) -> i32 {
        let first = if self.first_east { 2 } else { 0 };
        let x = if flight % 2 == 0 { first } else { 2 - first };
        self.min.0 + x
    }

    fn dir(&self, flight: usize) -> Dir {
        if flight % 2 == 0 { self.up0 } else { self.up0.opposite() }
    }

    /// Floor tiles (local min corners) left open at `level` for the flight
    /// arriving there.
    pub fn openings(&self, level: usize) -> Vec<(i32, i32)> {
        if level == 0 || level > self.flights {
            return Vec::new();
        }
        let flight = level - 1;
        let x = self.lane_x(flight);
        let z0 = self.min.1;
        let zs = match self.dir(flight) {
            Dir::North => [2, 4, 6],
            _ => [0, 2, 4],
        };
        zs.iter().map(|&z| (x, z0 + z)).collect()
    }

    /// Is local tile (x, z) open at `level`?
    pub fn is_open(&self, level: usize, x: i32, z: i32) -> bool {
        self.openings(level).contains(&(x, z))
    }

    /// Place flights, spine walls and railings.
    pub fn build(&self, b: &mut Builder) {
        let (cx, cz) = self.min;
        for flight in 0..self.flights {
            let y = self.level_y(flight);
            let x = self.lane_x(flight) as f32;
            match self.dir(flight) {
                Dir::North => b.uphill("stairs_3_2m", (x, cz as f32 + 6.8), y, Dir::North),
                _ => b.uphill("stairs_3_2m", (x + 2.0, cz as f32 + 1.2), y, Dir::South),
            };
            // Spine between the lanes on every level a flight leaves from.
            b.run((cx + 2, cz + 2), Dir::South, y, "s4", Trim::NONE);
        }
        for level in 1..=self.flights {
            let y = self.level_y(level);
            let arriving = self.dir(level - 1);
            let top = level == self.flights;
            // Railing on the lane boundary where the spine doesn't cover
            // the opening.
            match (arriving, top) {
                (Dir::North, false) => b.railing((cx + 2, cz + 6), Dir::South, y, 1),
                (_, false) => b.railing((cx + 2, cz), Dir::South, y, 1),
                (Dir::North, true) => b.railing((cx + 2, cz + 2), Dir::South, y, 3),
                (_, true) => b.railing((cx + 2, cz), Dir::South, y, 3),
            }
        }
    }
}
