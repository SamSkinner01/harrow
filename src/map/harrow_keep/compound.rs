//! The outer works: curtain walls with their wall-walk (6.4 m), the four
//! corner towers, the gatehouse and the rampart stairs. World coordinates:
//! the curtain's centre lines are x = +-40 and z = +-30.

use super::builder::{Builder, Core, Dir, Trim};
use super::{GROUND, ROOF, TERRACE, UPPER};
use Dir::*;

pub fn build(b: &mut Builder) {
    b.at(0, 0);
    curtains(b);
    for tower in &TOWERS {
        tower.build(b);
    }
    gatehouse(b);
    rampart_stairs(b);
}

/// Curtain runs, traced with the outside on the right. They end against
/// tower, keep and wing walls.
fn curtains(b: &mut Builder) {
    b.at(0, 0);
    b.curtain((-16, -30), West, &[4, 4, 4, 4, 2]);
    b.curtain((34, -30), West, &[4, 4, 4, 4, 2]);
    b.curtain((-40, -24), South, &[4]);
    b.curtain((-40, 4), South, &[4, 4, 4, 4, 4]);
    b.curtain((40, 24), North, &[4, 4, 4, 4, 4]);
    b.curtain((40, -20), North, &[4]);
    b.curtain((-34, 30), East, &[4, 4, 4, 4, 4, 4, 4]);
    b.curtain((6, 30), East, &[4, 4, 4, 4, 4, 4, 4]);
}

/// An 8 x 8 m corner tower: ground, middle and wall-walk levels and a roof
/// with battlements, served by a stair core in its outer half.
struct Tower {
    /// World min corner.
    min: (i32, i32),
    core: Core,
    /// Wall specs per level (ground, middle, walk): south (from the SW
    /// corner going east), east (from SE going north), north (from NE going
    /// west), west (from NW going south).
    walls: [[&'static str; 4]; 3],
    lamp: (f32, f32),
}

const TOWERS: [Tower; 4] = [
    // North-west.
    Tower {
        min: (-42, -32),
        core: Core { min: (0, 0), bottom: GROUND, flights: 3, up0: North, first_east: false },
        walls: [
            ["s4 d s2", "s2 W s4", "s2 d s4", "s4 s4"],
            ["s4 W s2", "s2 W s4", "s2 W s4", "s2 W s4"],
            ["s1 d s1 W s2", "s2 W s1 d s1", "s2 W s4", "s2 W s4"],
        ],
        lamp: (6.0, 4.0),
    },
    // North-east.
    Tower {
        min: (34, -32),
        core: Core { min: (4, 0), bottom: GROUND, flights: 3, up0: North, first_east: true },
        walls: [
            ["s2 d s4", "s4 s4", "s4 d s2", "s4 W s2"],
            ["s2 W s4", "s4 W s2", "s4 W s2", "s4 W s2"],
            ["s2 W s1 d s1", "s4 W s2", "s4 W s2", "s1 d s1 W s2"],
        ],
        lamp: (2.0, 4.0),
    },
    // South-west.
    Tower {
        min: (-42, 24),
        core: Core { min: (0, 0), bottom: GROUND, flights: 3, up0: South, first_east: false },
        walls: [
            ["s4 d s2", "s4 W s2", "s2 d s4", "s4 s4"],
            ["s4 W s2", "s4 W s2", "s2 W s4", "s4 W s2"],
            ["s4 W s2", "s1 d s1 W s2", "s2 W s1 d s1", "s4 W s2"],
        ],
        lamp: (6.0, 4.0),
    },
    // South-east.
    Tower {
        min: (34, 24),
        core: Core { min: (4, 0), bottom: GROUND, flights: 3, up0: South, first_east: true },
        walls: [
            ["s2 d s4", "s4 s4", "s4 d s2", "s4 s4"],
            ["s2 W s4", "s2 W s4", "s4 W s2", "s2 W s4"],
            ["s2 W s4", "s2 W s4", "s1 d s1 W s2", "s2 W s1 d s1"],
        ],
        lamp: (2.0, 4.0),
    },
];

impl Tower {
    fn build(&self, b: &mut Builder) {
        b.at(self.min.0, self.min.1);
        let core = self.core;
        core.build(b);
        for (level, faces) in self.walls.iter().enumerate() {
            let y = core.level_y(level);
            let trim = if level == 0 { Trim::GROUND } else { Trim::BAND };
            b.run((0, 8), East, y, faces[0], trim);
            b.run((8, 8), North, y, faces[1], trim);
            b.run((8, 0), West, y, faces[2], trim);
            b.run((0, 0), South, y, faces[3], trim);
        }
        for level in 0..=3 {
            let y = core.level_y(level);
            b.floor((0, 0), (8, 8), y, |x, z| (!core.is_open(level, x, z)).then_some("floor_stone_2m"));
        }
        // On the walk level the inner lane is open to the room beside it.
        let rail_z = if core.up0 == North { 0 } else { 2 };
        b.railing((4, rail_z), South, TERRACE, 3);
        b.parapets((0, 8), East, ROOF, "4 4");
        b.parapets((8, 8), North, ROOF, "4 4");
        b.parapets((8, 0), West, ROOF, "4 4");
        b.parapets((0, 0), South, ROOF, "4 4");
        let (lx, lz) = self.lamp;
        b.lamp(lx, lz, GROUND, false);
        b.lamp(lx, lz, TERRACE, false);
        // A little clutter in the room half.
        let room_x = if core.first_east { 1.0 } else { 7.0 };
        b.prop("crate_large", room_x, GROUND, 6.8, 10.0);
        b.prop("barrel", room_x, GROUND, 1.2, 0.0);
        b.prop("sandbags_2m", room_x + if core.first_east { 1.0 } else { -1.0 }, TERRACE, 4.0, 90.0);
    }
}

/// Gatehouse astride the south wall: a vaulted passage between two guard
/// rooms, the gate room above, and a roof on the wall-walk.
fn gatehouse(b: &mut Builder) {
    b.at(-6, 26);
    let (g, u, t) = (GROUND, UPPER, TERRACE);
    b.run((0, 8), East, g, "s2 W a4 W s2", Trim::GROUND);
    b.run((12, 8), North, g, "W s2 s4", Trim::GROUND);
    b.run((12, 0), West, g, "s2 d a4 s2 d", Trim::GROUND);
    b.run((0, 0), South, g, "s4 s2 W", Trim::GROUND);
    b.run((4, 0), South, g, "s4 d s2", Trim::NONE);
    b.run((8, 8), North, g, "s2 s2 d s2", Trim::NONE);
    b.run((0, 8), East, u, "s2 W W W W s2", Trim::BAND);
    b.run((12, 8), North, u, "W s2 s4", Trim::BAND);
    b.run((12, 0), West, u, "s2 W W W W s2", Trim::BAND);
    b.run((0, 0), South, u, "s4 s2 W", Trim::BAND);
    // Stairs: west guard room up to the gate room; gate room up to the
    // roof along the south wall, clear of the wall-walk crossing the roof
    // (z 2.5..5 here).
    b.uphill("stairs_3_2m", (0.0, 6.8), g, North);
    b.uphill("stairs_3_2m", (3.2, 6.0), u, East);
    b.railing((2, 2), South, u, 3);
    b.railing((2, 6), East, t, 3);
    b.railing((2, 6), South, t, 1);
    b.floor((0, 0), (12, 8), g, |_, _| Some("floor_stone_2m"));
    b.floor((0, 0), (12, 8), u, |x, z| match (x, z) {
        (0, 2) | (0, 4) | (0, 6) => None,
        // Murder hole over the gate passage.
        (6, 4) => Some("floor_hatch_2m"),
        _ => Some("floor_stone_2m"),
    });
    b.floor((0, 0), (12, 8), t, |x, z| match (x, z) {
        (2, 6) | (4, 6) | (6, 6) => None,
        _ => Some("floor_stone_2m"),
    });
    // Battlements; gaps where the wall-walks come in from east and west.
    b.parapets((0, 8), East, t, "4 4 4");
    b.parapets((12, 8), North, t, "2 _4 2");
    b.parapets((12, 0), West, t, "4 4 4");
    b.parapets((0, 0), South, t, "2 _4 2");
    // Guard rooms and gate room.
    b.prop("table_2m", 10.0, g, 5.5, 90.0);
    b.prop("bench_2m", 11.4, g, 5.5, 90.0);
    b.prop("weapon_rack", 11.75, g, 1.8, 90.0);
    b.prop("crate_large", 3.0, g, 1.2, 0.0);
    b.prop("barrel", 3.3, g, 7.2, 0.0);
    b.prop("crate_large", 4.5, u, 1.0, 0.0);
    b.prop("crate_small", 11.0, u, 6.8, 15.0);
    b.prop("sandbags_2m", 9.0, u, 1.0, 0.0);
    b.prop("sandbags_2m", 3.0, t, 1.2, 0.0);
    b.prop("sandbags_2m", 7.0, t, 1.2, 0.0);
    b.lamp(2.0, 4.0, g, false);
    b.lamp(10.0, 4.0, g, false);
    b.lamp(6.0, 4.0, u, true);
}

/// Stone stairs against the inner face of the curtain, from the yards up to
/// the wall-walk: two flights with a landing, the top landing beside the
/// walk. `start` is the toe end of the lane's wall-side edge.
fn rampart_stair(b: &mut Builder, start: (f32, f32), up: Dir) {
    let step = |d: f32| match up {
        East => (start.0 + d, start.1),
        West => (start.0 - d, start.1),
        North => (start.0, start.1 - d),
        South => (start.0, start.1 + d),
    };
    b.uphill("stairs_ext_3_2m", step(0.0), GROUND, up);
    b.uphill("stair_landing_2m", step(4.8), GROUND, up);
    b.uphill("stair_base_3_2m", step(6.8), GROUND, up);
    b.uphill("stairs_ext_3_2m", step(6.8), UPPER, up);
    b.uphill("stair_pier_2m", step(11.6), GROUND, up);
    b.uphill("stair_landing_end_2m", step(11.6), UPPER, up);
}

fn rampart_stairs(b: &mut Builder) {
    b.at(0, 0);
    // Main yard, west of the gatehouse, up to the south wall.
    rampart_stair(b, (-8.0, 28.75), West);
    // Well court and east yard, up to the north walls.
    rampart_stair(b, (-32.6, -28.75), East);
    rampart_stair(b, (20.0, -28.75), East);
}
