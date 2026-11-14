//! The keep: 32 x 22 m, basement + three floors, a roof terrace around the
//! third-floor block, and the cellar steps outside its north face.
//!
//! Local grid: x 0..32 (west to east), z 0..22 (north to south); world =
//! local + (-16, -30). The north face is part of the compound's outer wall.
//! Floor plans with callouts are in DESIGN.md ("Map: Harrow Keep").

use super::builder::{Builder, Core, Dir, Trim, WALL};
use super::{BASEMENT, GROUND, ROOF, TERRACE, UPPER};
use Dir::*;

/// World position of the keep's local origin.
pub const ORIGIN: (i32, i32) = (-16, -30);

/// Central stair: basement to third floor.
const KEEP_STAIR: Core = Core { min: (14, 0), bottom: BASEMENT, flights: 3, up0: South, first_east: true };
/// West and east stairs: basement to second floor.
const WEST_STAIR: Core = Core { min: (0, 14), bottom: BASEMENT, flights: 2, up0: South, first_east: true };
const EAST_STAIR: Core = Core { min: (28, 14), bottom: BASEMENT, flights: 2, up0: South, first_east: true };
const CORES: [Core; 3] = [KEEP_STAIR, WEST_STAIR, EAST_STAIR];

/// Is floor tile (x, z) of `level` (0 = basement) a stairwell opening?
fn open(level: usize, x: i32, z: i32) -> bool {
    CORES.iter().any(|c| c.is_open(level, x, z))
}

pub fn build(b: &mut Builder) {
    b.at(ORIGIN.0, ORIGIN.1);
    for core in &CORES {
        core.build(b);
    }
    basement(b);
    ground(b);
    upper(b);
    third(b);
    terrace_stairs(b);
    cellar_steps(b);
    dress(b);
    lamps(b);
}

fn basement(b: &mut Builder) {
    let y = BASEMENT;
    let n = Trim::NONE;
    // Outer walls are below ground: no trim.
    b.run((0, 22), East, y, "s4 s4 s4 s4 s4 s4 s4 s4", n);
    b.run((32, 22), North, y, "s4 s4 s4 s4 s4 s2", n);
    b.run((32, 0), West, y, "s4 s4 s2 d s2 s4 s4 s4 s4 s2", n);
    b.run((0, 0), South, y, "s4 s4 s4 s4 s4 s2", n);
    // Oil cellar | cells | stair || undercroft.
    b.run((0, 8), East, y, "s2 d s4 s2 d s2 d s2", n);
    b.run((8, 0), South, y, "p2 p2 p2 p2", n);
    b.run((14, 0), South, y, "s4 s4", n);
    // Stair | cistern, undercroft | cistern / powder store, crypt | powder store.
    b.run((18, 0), South, y, "d s2 s4 d s2 d p2 pd p2 p2", n);
    // Cistern | powder store (soft, with a door) and pump room.
    b.run((18, 10), East, y, "p2 p2 pd p2 p2 d s2", n);
    b.run((28, 10), South, y, "s2 d s4 s4", n);
    b.run((28, 14), East, y, "s2 d", n);
    // Undercroft | west stair, crypt.
    b.run((0, 14), East, y, "s2 d s4 d s4 d s2", n);
    b.run((4, 14), South, y, "s4 s4", n);
    b.floor((0, 0), (32, 22), y, |_, _| Some("floor_stone_2m"));
}

fn ground(b: &mut Builder) {
    let y = GROUND;
    let n = Trim::NONE;
    let outer = Trim::GROUND;
    b.run((0, 22), East, y, "s2 d s2 W s2 W s2 a4 s2 W s2 W s2 W s2", outer);
    b.run((32, 22), North, y, "s4 s4 s4 s2 W s2 D s2", outer);
    b.run((32, 0), West, y, "s2 W s2 W s2 s2 s2 s4 s2 W s2 s2 d W s2", outer);
    b.run((0, 0), South, y, "s2 W s2 s2 s2 D W s4 s4", outer);
    // North rooms | great hall band.
    b.run((0, 8), East, y, "s2 a4 s2 p2 pd p2 s2 d p2 pd p2 s2 d s4", n);
    // Postern | armoury (soft), gallery | great hall, gallery | guardroom.
    b.run((8, 0), South, y, "p2 p2 p2 p2 s2 a4 s2 d s4", n);
    b.run((14, 0), South, y, "s4 s4", n);
    b.run((18, 0), South, y, "s4 s4", n);
    // West stair | gallery; west stair north wall.
    b.run((4, 14), South, y, "s4 s2 d", n);
    b.run((0, 14), East, y, "s4", n);
    // Great hall | guardroom, entrance, scullery.
    b.run((8, 16), East, y, "s4 s2 a4 s2 pd p2", n);
    b.run((12, 16), South, y, "p2 pd p2", n);
    b.run((20, 16), South, y, "p2 pd p2", n);
    // Great hall | kitchen | scullery; east stair.
    b.run((24, 8), South, y, "s2 d s2 s2", n);
    b.run((24, 16), East, y, "d s2", n);
    b.run((28, 14), South, y, "s2 s4 d", n);
    b.run((28, 14), East, y, "s4", n);
    // Floor: wood (with hatches down to the basement) in the larder, the
    // kitchen and one tile of the great hall.
    b.floor((0, 0), (32, 22), y, |x, z| {
        if open(1, x, z) {
            None
        } else if [(26, 2), (26, 10), (14, 12)].contains(&(x, z)) {
            Some("floor_hatch_2m")
        } else if (22..30).contains(&x) && (2..6).contains(&z) {
            Some("floor_wood_2m")
        } else {
            Some("floor_stone_2m")
        }
    });
}

fn upper(b: &mut Builder) {
    let y = UPPER;
    let n = Trim::NONE;
    let outer = Trim::BAND;
    b.run((0, 22), East, y, "s2 W s2 W s2 W s2 W W s2 s2 W s2 W s2 s2", outer);
    b.run((32, 22), North, y, "s4 s1 d s1 s4 s2 W s2 W s2", outer);
    b.run((32, 0), West, y, "s2 W s2 W s2 W s2 s4 s2 W s2 W s2 W s2", outer);
    b.run((0, 0), South, y, "s2 W s2 W s2 W s2 s4 s4", outer);
    // Map room, stair, records | long gallery.
    b.run((0, 8), East, y, "s2 d s2 p2 pd p2 s2 s4 s2 p2 pd p2 s2 d s2", n);
    b.run((14, 0), South, y, "d s2 s4", n);
    b.run((18, 0), South, y, "d s2 s4", n);
    // Chart alcove in the map room.
    b.run((6, 0), South, y, "p2 p2", n);
    // Long gallery | stairs, infirmary, chapel, mess.
    b.run((0, 14), East, y, "d s2 s2 pd p2 p2 s2 a4 s2 s2 p2 pd p2 s2 d", n);
    b.run((4, 14), South, y, "s4 s4", n);
    b.run((12, 14), South, y, "p2 pd p2 p2", n);
    b.run((22, 14), South, y, "s2 d s2 s2", n);
    b.run((28, 14), South, y, "s4 s4", n);
    // Floor: wood over the great hall and most of the armoury, with hatches.
    b.floor((0, 0), (32, 22), y, |x, z| {
        if open(2, x, z) {
            None
        } else if (x, z) == (14, 10) || (x, z) == (10, 4) {
            Some("floor_hatch_2m")
        } else if (8..24).contains(&x) && (8..16).contains(&z) || (8..14).contains(&x) && (4..8).contains(&z) {
            Some("floor_wood_2m")
        } else {
            Some("floor_stone_2m")
        }
    });
}

fn third(b: &mut Builder) {
    let y = TERRACE;
    let n = Trim::NONE;
    let outer = Trim::BAND;
    // The third-floor block, x 8..24, z 0..12.
    b.run((8, 12), East, y, "s2 W s2 s1 D s1 s2 W s2", outer);
    b.run((24, 12), North, y, "s2 D p2 W s2 s2", outer);
    b.run((24, 0), West, y, "s2 W s2 s4 s2 W s2", outer);
    b.run((8, 0), South, y, "s2 s2 W p2 D s2", outer);
    b.run((14, 0), South, y, "s4 s2 d p2 p2", n);
    b.run((18, 0), South, y, "s4 s2 d p2 p2", n);
    // Floor of the third floor and the terrace: hatches in both rooms and
    // on the terrace over the map room, records and chapel.
    b.floor((0, 0), (32, 22), y, |x, z| {
        if open(3, x, z) {
            None
        } else if [(10, 2), (20, 2), (2, 2), (28, 2), (16, 16)].contains(&(x, z)) {
            Some("floor_hatch_2m")
        } else {
            Some("floor_stone_2m")
        }
    });
    // Terrace battlements, with gaps where the north wall-walks come in
    // (both north corners) and the terrace stairs arrive (east).
    b.parapets((0, 22), East, y, "4 4 4 4 4 4 4 4");
    b.parapets((32, 22), North, y, "4 4 4 _2 4 2 !_2");
    b.parapets((32, 0), West, y, "! _2 2 4");
    b.parapets((8, 0), West, y, "4 2 !_2");
    b.parapets((0, 0), South, y, "! _2 2 4 4 4 4 2");
    // Roof of the third-floor block.
    b.floor((8, 0), (24, 12), ROOF, |_, _| Some("floor_stone_2m"));
    b.parapets((8, 12), East, ROOF, "4 4 4 4");
    b.parapets((24, 12), North, ROOF, "4 4 4");
    b.parapets((24, 0), West, ROOF, "4 4 4 4");
    b.parapets((8, 0), South, ROOF, "4 4 4");
}

/// Exterior stairs up the east face: yard -> landing (second-floor door)
/// -> terrace.
fn terrace_stairs(b: &mut Builder) {
    b.uphill("stairs_ext_3_2m", (32.0, 21.6), GROUND, North);
    b.uphill("stair_landing_2m", (32.0, 16.8), GROUND, North);
    b.uphill("stair_base_3_2m", (32.0, 14.8), GROUND, North);
    b.uphill("stairs_ext_3_2m", (32.0, 14.8), UPPER, North);
    b.uphill("stair_pier_2m", (32.0, 10.0), GROUND, North);
    b.uphill("stair_landing_end_2m", (32.0, 10.0), UPPER, North);
}

/// Sunken steps from the desert down to the cistern's north door.
fn cellar_steps(b: &mut Builder) {
    let y = BASEMENT;
    b.run((20, -6), South, y, "s4 s2", Trim::NONE);
    b.run((22, 0), North, y, "s2 s4", Trim::NONE);
    b.parapets((20, -6), South, y + WALL, "4 2");
    b.parapets((22, 0), North, y + WALL, "2 4");
    b.uphill("stairs_3_2m", (20.0, -1.2), y, North);
    b.floor((20, -2), (22, 0), y, |_, _| Some("floor_stone_2m"));
}

/// Furniture, cover and clutter, room by room.
fn dress(b: &mut Builder) {
    let (bs, g, u, t) = (BASEMENT, GROUND, UPPER, TERRACE);
    // --- Basement ---
    // Oil cellar.
    for (x, z) in [(1.0, 1.0), (1.7, 1.0), (1.0, 1.7), (6.8, 6.6), (6.1, 7.0), (2.4, 1.0)] {
        b.prop("barrel", x, bs, z, 0.0);
    }
    b.prop("shelf_2m", 5.0, bs, 0.25, 180.0);
    b.prop("crate_large", 6.4, bs, 3.0, 10.0);
    // Cells.
    b.prop("bench_2m", 12.6, bs, 2.0, 90.0);
    b.prop("barrel", 9.0, bs, 1.0, 0.0);
    b.prop("crate_small", 9.2, bs, 6.8, 20.0);
    // Cistern (site): piers, sandbags and crates.
    for (x, z) in [(22.5, 3.5), (27.5, 3.5), (22.5, 7.0), (27.5, 7.0)] {
        b.prop("pier", x, bs, z, 0.0);
    }
    b.prop("sandbags_2m", 25.0, bs, 5.3, 0.0);
    b.prop("crate_large", 30.3, bs, 1.3, 0.0);
    b.prop("crate_small", 30.3, bs + 1.0, 1.3, 15.0);
    b.prop("crate_large", 19.6, bs, 8.4, 15.0);
    b.prop("barrel", 31.0, bs, 8.9, 0.0);
    b.prop("barrel", 30.3, bs, 8.9, 0.0);
    // Undercroft: a row of piers down the middle.
    for x in [5.0, 9.0, 13.0] {
        b.prop("pier", x, bs, 11.0, 0.0);
    }
    b.prop("crate_large", 1.0, bs, 12.9, 0.0);
    b.prop("crate_small", 1.2, bs, 9.2, 30.0);
    // Powder store (site): crate stacks and kegs.
    for (x, z, big) in [(20.0, 12.2, true), (20.0, 14.0, true), (25.5, 12.0, false), (23.0, 17.0, true), (26.6, 20.6, true)] {
        b.prop(if big { "crate_large" } else { "crate_small" }, x, bs, z, 0.0);
    }
    b.prop("crate_small", 20.0, bs + 1.0, 12.2, 15.0);
    b.prop("crate_small", 23.0, bs + 1.0, 17.0, -10.0);
    for (x, z) in [(26.9, 15.5), (26.9, 16.2), (26.2, 15.8), (19.2, 20.9), (19.9, 20.9)] {
        b.prop("barrel", x, bs, z, 0.0);
    }
    // Pump room.
    b.prop("generator", 30.0, bs, 11.0, 90.0);
    // Crypt: tombs and piers.
    for (x, z) in [(7.0, 17.0), (7.0, 19.5), (14.0, 17.0), (14.0, 19.5)] {
        b.prop("tomb", x, bs, z, 0.0);
    }
    b.prop("pier", 10.5, bs, 18.0, 0.0);
    // --- Ground floor ---
    // Postern hall.
    b.prop("bench_2m", 1.0, g, 6.0, 90.0);
    b.prop("crate_large", 6.6, g, 1.4, 0.0);
    b.prop("crate_small", 6.6, g + 1.0, 1.4, 25.0);
    // Armoury (site).
    b.prop("weapon_rack", 9.2, g, 0.25, 180.0);
    b.prop("weapon_rack", 13.75, g, 4.0, 90.0);
    b.prop("table_2m", 10.5, g, 4.6, 90.0);
    b.prop("crate_large", 9.0, g, 7.0, 0.0);
    b.prop("crate_small", 12.9, g, 7.2, 10.0);
    // Larder.
    b.prop("shelf_2m", 21.0, g, 0.25, 180.0);
    b.prop("shelf_2m", 27.0, g, 0.25, 180.0);
    for (x, z) in [(19.0, 1.0), (19.0, 1.7), (19.7, 1.0), (30.9, 1.0)] {
        b.prop("barrel", x, g, z, 0.0);
    }
    b.prop("sandbags_2m", 25.0, g, 7.0, 0.0);
    // West gallery.
    b.prop("rug_2x3", 4.0, g, 11.0, 0.0);
    b.prop("bench_2m", 1.0, g, 9.5, 90.0);
    b.prop("crate_small", 6.4, g, 20.6, 0.0);
    // Great hall (site): long tables, benches, rugs.
    for x in [12.0, 20.0] {
        b.prop("table_2m", x - 1.0, g, 11.0, 0.0);
        b.prop("table_2m", x + 1.0, g, 11.0, 0.0);
        b.prop("bench_2m", x - 1.0, g, 10.2, 0.0);
        b.prop("bench_2m", x + 1.0, g, 11.8, 0.0);
    }
    b.prop("rug_2x3", 17.5, g, 13.0, 90.0);
    b.prop("crate_large", 22.8, g, 14.6, 0.0);
    b.prop("crate_large", 9.3, g, 14.6, 0.0);
    // Guardroom.
    b.prop("table_2m", 10.0, g, 19.0, 90.0);
    b.prop("weapon_rack", 8.25, g, 20.0, -90.0);
    // Entrance.
    b.prop("rug_2x3", 16.0, g, 19.0, 0.0);
    b.prop("crate_large", 13.3, g, 20.5, 0.0);
    // Kitchen.
    b.prop("hearth", 31.75, g, 11.0, 90.0);
    b.prop("table_2m", 29.5, g, 9.5, 0.0);
    b.prop("barrel", 24.9, g, 13.2, 0.0);
    // Scullery.
    b.prop("table_2m", 24.0, g, 21.2, 0.0);
    for (x, z) in [(21.0, 17.0), (21.7, 17.0), (27.0, 17.2)] {
        b.prop("barrel", x, g, z, 0.0);
    }
    // --- Second floor ---
    // Map room (site): map table, chart shelves, radio.
    b.prop("table_2m", 9.0, u, 2.6, 0.0);
    b.prop("table_2m", 11.0, u, 2.6, 0.0);
    b.prop("rug_2x3", 10.0, u, 1.6, 90.0);
    b.prop("shelf_2m", 2.0, u, 0.25, 180.0);
    b.prop("shelf_2m", 13.0, u, 0.25, 180.0);
    b.prop("table_2m", 2.0, u, 6.6, 0.0);
    b.prop("crate_large", 7.0, u, 1.0, 0.0);
    // Records (site): free-standing shelf rows.
    for x in [21.0, 25.0, 29.0] {
        b.prop("shelf_2m", x, u, 3.0, 0.0);
        b.prop("shelf_2m", x, u, 3.0, 180.0);
    }
    b.prop("table_2m", 24.0, u, 6.5, 0.0);
    b.prop("crate_small", 31.0, u, 7.0, 0.0);
    // Long gallery.
    b.prop("rug_2x3", 4.5, u, 11.0, 90.0);
    b.prop("rug_2x3", 27.5, u, 11.0, 90.0);
    b.prop("bench_2m", 5.0, u, 13.2, 0.0);
    // Infirmary.
    for z in [17.2, 20.0] {
        b.prop("bunk_bed", 5.2, u, z, 0.0);
        b.prop("bunk_bed", 10.8, u, z, 0.0);
    }
    // Chapel: benches in rows and a table as altar.
    for z in [16.6, 18.0, 19.4] {
        b.prop("bench_2m", 15.0, u, z, 0.0);
        b.prop("bench_2m", 19.0, u, z, 0.0);
    }
    b.prop("table_2m", 17.0, u, 21.0, 0.0);
    // Mess.
    b.prop("table_2m", 25.0, u, 18.0, 90.0);
    b.prop("bench_2m", 24.2, u, 18.0, 90.0);
    b.prop("bench_2m", 25.8, u, 18.0, 90.0);
    // --- Third floor and terrace ---
    // Signal room (site): radios, generator.
    b.prop("table_2m", 10.0, t, 1.0, 0.0);
    b.prop("radio_set", 10.0, t + 0.8, 1.0, 0.0);
    b.prop("table_2m", 13.0, t, 9.0, 90.0);
    b.prop("generator", 9.4, t, 10.6, 0.0);
    b.prop("crate_large", 12.6, t, 4.6, 0.0);
    // Study (site).
    b.prop("table_2m", 21.0, t, 6.0, 90.0);
    b.prop("shelf_2m", 23.75, t, 2.0, 90.0);
    b.prop("shelf_2m", 18.25, t, 11.0, -90.0);
    b.prop("rug_2x3", 21.0, t, 9.0, 0.0);
    b.prop("crate_small", 19.0, t, 1.0, 0.0);
    // Terrace cover.
    b.prop("sandbags_2m", 4.0, t, 18.0, 0.0);
    b.prop("sandbags_2m", 28.0, t, 18.0, 0.0);
    b.prop("sandbags_2m", 16.0, t, 14.5, 0.0);
    b.prop("crate_large", 2.0, t, 9.0, 0.0);
    b.prop("crate_large", 29.5, t, 12.0, 0.0);
}

fn lamps(b: &mut Builder) {
    let (bs, g, u, t) = (BASEMENT, GROUND, UPPER, TERRACE);
    for (x, z, y, shadows) in [
        (4.0, 4.0, bs, false),   // oil cellar
        (11.0, 4.0, bs, false),  // cells
        (25.0, 5.2, bs, true),   // cistern
        (25.0, 1.5, bs, false),  // cistern north
        (7.0, 11.0, bs, true),   // undercroft west
        (15.5, 11.0, bs, false), // undercroft east
        (23.0, 14.8, bs, true),  // powder store
        (30.0, 12.0, bs, false), // pump room
        (11.0, 18.0, bs, true),  // crypt
        (4.0, 4.0, g, false),    // postern hall
        (11.0, 4.0, g, true),    // armoury
        (25.0, 4.0, g, false),   // larder
        (4.0, 12.0, g, false),   // west gallery
        (12.0, 12.0, g, true),   // great hall
        (20.0, 12.0, g, false),  // great hall east
        (10.0, 19.0, g, false),  // guardroom
        (16.0, 18.5, g, false),  // entrance
        (28.0, 11.0, g, false),  // kitchen
        (24.0, 19.0, g, false),  // scullery
        (9.0, 4.5, u, true),     // map room
        (24.0, 4.5, u, true),    // records
        (8.0, 11.0, u, false),   // long gallery west
        (24.0, 11.0, u, false),  // long gallery east
        (8.0, 18.0, u, false),   // infirmary
        (17.0, 18.0, u, true),   // chapel
        (25.0, 18.0, u, false),  // mess
        (11.0, 6.0, t, true),    // signal room
        (21.0, 6.0, t, true),    // study
        (16.0, 10.0, t, false),  // landing
    ] {
        b.lamp(x, z, y, shadows);
    }
}
