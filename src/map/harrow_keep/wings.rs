//! Buildings around the yards: the barracks (west wall), the granary (east
//! wall), the old hall and the stables (south), the inner walls between the
//! yards, and the paving.

use super::builder::{Builder, Core, Dir, Trim};
use super::{GROUND, TERRACE, UPPER};
use Dir::*;

pub fn build(b: &mut Builder) {
    barracks(b);
    granary(b);
    old_hall(b);
    stables(b);
    yards(b);
}

/// Two-storey barracks on the west wall; its roof carries the wall-walk.
/// Local x 0..12 (west wall at x = 0 is the outer wall), z 0..24.
fn barracks(b: &mut Builder) {
    b.at(-40, -20);
    let (g, u, t) = (GROUND, UPPER, TERRACE);
    let n = Trim::NONE;
    let core = Core { min: (0, 16), bottom: g, flights: 2, up0: North, first_east: false };
    core.build(b);
    // Ground: bunk room, washroom, drill hall.
    b.run((0, 24), East, g, "s2 d s2 W D s2", Trim::GROUND);
    b.run((12, 24), North, g, "W s2 D W s2 W s2 W D W s2 s2", Trim::GROUND);
    b.run((12, 0), West, g, "s2 W s2 W s2 s2", Trim::GROUND);
    b.run((0, 0), South, g, "s4 W s4 s2 D s2 s4 s4", Trim::GROUND);
    b.run((0, 12), East, g, "s2 d p2 pd p2 p2", n);
    b.run((4, 12), South, g, "s2 pd s4 s2 d", n);
    b.run((0, 16), East, g, "s4", n);
    // Upper: dormitory and stores.
    b.run((0, 24), East, u, "s2 W s2 W W s2", Trim::BAND);
    b.run((12, 24), North, u, "W s2 W s2 W s2 W s2 W s2 W s2", Trim::BAND);
    b.run((12, 0), West, u, "s2 W s2 W s2 s2", Trim::BAND);
    b.run((0, 0), South, u, "s4 W s4 W s2 s2 s4 s4", Trim::BAND);
    b.run((4, 14), East, u, "p2 pd p2 p2", n);
    b.run((4, 14), South, u, "s2 s4 s4", n);
    b.run((0, 16), East, u, "d s2", n);
    b.floor((0, 0), (12, 24), g, |_, _| Some("floor_stone_2m"));
    b.floor((0, 0), (12, 24), u, |x, z| (!core.is_open(1, x, z)).then_some("floor_stone_2m"));
    b.floor((0, 0), (12, 24), t, |x, z| match (x, z) {
        _ if core.is_open(2, x, z) => None,
        (8, 6) => Some("floor_hatch_2m"),
        _ => Some("floor_stone_2m"),
    });
    // Roof stairwell rails and battlements (the walk passes along x 0..2).
    b.railing((4, 16), South, t, 3);
    b.railing((2, 16), East, t, 1);
    b.parapets((0, 0), South, t, "! 4 4 4 4 4 !4");
    b.parapets((12, 0), West, t, "4 4 2 !_2");
    b.parapets((0, 24), East, t, "! _2 2 4 4");
    b.parapets((12, 24), North, t, "4 4 4 4 4 4");
    // Bunk room.
    for (x, z) in [(1.0, 2.5), (4.0, 2.5), (8.0, 2.5), (11.0, 2.5), (1.0, 8.5), (4.0, 8.5), (8.0, 8.5), (11.0, 10.0)] {
        b.prop("bunk_bed", x, g, z, 0.0);
    }
    b.prop("table_2m", 6.0, g, 5.5, 0.0);
    b.prop("bench_2m", 6.0, g, 4.8, 0.0);
    // Washroom.
    b.prop("barrel", 0.8, g, 15.2, 0.0);
    b.prop("barrel", 1.5, g, 15.2, 0.0);
    // Drill hall.
    b.prop("weapon_rack", 11.75, g, 15.0, 90.0);
    b.prop("weapon_rack", 9.5, g, 12.3, 180.0);
    b.prop("crate_large", 6.0, g, 22.6, 0.0);
    b.prop("crate_small", 10.6, g, 22.9, 20.0);
    b.prop("sandbags_2m", 7.0, g, 16.2, 0.0);
    // Dormitory and stores.
    for x in [1.0, 4.0, 8.0, 11.0] {
        b.prop("bunk_bed", x, u, 3.0, 0.0);
    }
    b.prop("bunk_bed", 8.0, u, 9.5, 90.0);
    b.prop("shelf_2m", 11.75, u, 17.0, 90.0);
    b.prop("shelf_2m", 11.75, u, 21.0, 90.0);
    b.prop("crate_large", 6.0, u, 22.6, 0.0);
    b.prop("crate_small", 6.0, u + 1.0, 22.6, 30.0);
    b.prop("sandbags_2m", 6.0, t, 10.0, 0.0);
    b.lamp(6.0, 6.0, g, true);
    b.lamp(2.0, 14.0, g, false);
    b.lamp(8.0, 19.0, g, false);
    b.lamp(6.0, 7.0, u, false);
    b.lamp(8.0, 19.0, u, false);
}

/// Two-storey granary on the east wall, with a timber loft; its roof carries
/// the wall-walk. Local x 0..12 (east wall at x = 12 is the outer wall).
fn granary(b: &mut Builder) {
    b.at(28, -20);
    let (g, u, t) = (GROUND, UPPER, TERRACE);
    let n = Trim::NONE;
    let core = Core { min: (8, 16), bottom: g, flights: 2, up0: North, first_east: true };
    core.build(b);
    // Ground: granary hall, mill room, sack store.
    b.run((0, 24), East, g, "s2 D s2 W d s2", Trim::GROUND);
    b.run((12, 24), North, g, "s4 s4 s2 D s4 W s4 s2", Trim::GROUND);
    b.run((12, 0), West, g, "s2 W s2 W s2 s2", Trim::GROUND);
    b.run((0, 0), South, g, "s2 W s2 D W s2 s2 W s2 D s2 W", Trim::GROUND);
    b.run((0, 12), East, g, "p2 pd p2 p2 s2 d", n);
    b.run((8, 12), South, g, "pd p2 s4 s2 d", n);
    b.run((8, 16), East, g, "s4", n);
    // Upper: loft and office.
    b.run((0, 24), East, u, "s2 W s2 W s2 s2", Trim::BAND);
    b.run((12, 24), North, u, "s4 s4 s2 W s4 W s4 s2", Trim::BAND);
    b.run((12, 0), West, u, "s2 W s2 W s2 s2", Trim::BAND);
    b.run((0, 0), South, u, "s2 W s2 W s2 W s2 W s2 W s2 W", Trim::BAND);
    b.run((0, 12), East, u, "p2 p2 pd p2 s2 d", n);
    b.run((8, 12), South, u, "p2 pd s4 s4", n);
    b.run((8, 16), East, u, "s2 d", n);
    b.floor((0, 0), (12, 24), g, |_, _| Some("floor_stone_2m"));
    b.floor((0, 0), (12, 24), u, |x, z| match (x, z) {
        _ if core.is_open(1, x, z) => None,
        (2, 4) | (6, 8) => Some("floor_hatch_2m"),
        _ if (0..8).contains(&x) && (2..10).contains(&z) => Some("floor_wood_2m"),
        _ => Some("floor_stone_2m"),
    });
    b.floor((0, 0), (12, 24), t, |x, z| (!core.is_open(2, x, z)).then_some("floor_stone_2m"));
    b.railing((8, 16), South, t, 3);
    b.railing((8, 16), East, t, 1);
    b.parapets((12, 24), North, t, "! 4 4 4 4 4 !4");
    b.parapets((12, 0), West, t, "! _2 2 4 4");
    b.parapets((0, 24), East, t, "4 4 2 !_2");
    b.parapets((0, 0), South, t, "4 4 4 4 4 4");
    // Granary hall: sacks and crates.
    for (x, z) in [(2.0, 1.0), (2.0, 3.0), (6.0, 1.0), (10.0, 6.0), (10.0, 8.0)] {
        b.prop("sandbags_2m", x, g, z, if x > 9.0 { 90.0 } else { 0.0 });
    }
    b.prop("crate_large", 5.5, g, 6.0, 0.0);
    b.prop("crate_large", 6.8, g, 6.0, 0.0);
    b.prop("crate_small", 6.1, g + 1.0, 6.0, 10.0);
    b.prop("crate_large", 2.0, g, 9.6, 30.0);
    for (x, z) in [(11.0, 1.0), (11.0, 1.7), (10.3, 1.0)] {
        b.prop("barrel", x, g, z, 0.0);
    }
    // Mill room.
    b.prop("table_2m", 4.0, g, 18.0, 0.0);
    b.prop("crate_large", 1.0, g, 22.8, 0.0);
    b.prop("cart", 4.0, g, 14.6, 0.0);
    // Sack store.
    b.prop("crate_small", 9.0, g, 13.0, 0.0);
    // Loft and office.
    b.prop("sandbags_2m", 1.0, u, 11.0, 0.0);
    b.prop("crate_large", 10.0, u, 3.0, 0.0);
    b.prop("table_2m", 3.0, u, 19.0, 90.0);
    b.prop("shelf_2m", 0.25, u, 21.0, -90.0);
    b.prop("sandbags_2m", 6.0, t, 12.0, 0.0);
    b.lamp(6.0, 6.0, g, true);
    b.lamp(4.0, 18.0, g, false);
    b.lamp(6.0, 6.0, u, false);
    b.lamp(4.0, 18.0, u, false);
}

/// The old hall: one tall storey with an arcade screening the shrine at its
/// east end.
fn old_hall(b: &mut Builder) {
    b.at(-30, 12);
    let g = GROUND;
    b.run((0, 12), East, g, "s2 W s2 d s2 W s2 s2", Trim::GROUND);
    b.run((16, 12), North, g, "s2 W s2 D s2 W", Trim::GROUND);
    b.run((16, 0), West, g, "s2 W s2 W s2 D s2 W", Trim::GROUND);
    b.run((0, 0), South, g, "s2 W s2 d s2 W", Trim::GROUND);
    b.run((12, 0), South, g, "r4 r4 r4", Trim::NONE);
    b.floor((0, 0), (16, 12), g, |_, _| Some("floor_stone_2m"));
    b.floor((0, 0), (16, 12), UPPER, |_, _| Some("floor_stone_2m"));
    b.parapets((0, 12), East, UPPER, "4 4 4 4");
    b.parapets((16, 12), North, UPPER, "4 4 4");
    b.parapets((16, 0), West, UPPER, "4 4 4 4");
    b.parapets((0, 0), South, UPPER, "4 4 4");
    for (x, z) in [(4.0, 4.0), (8.0, 4.0), (4.0, 8.0), (8.0, 8.0)] {
        b.prop("pier", x, g, z, 0.0);
    }
    for z in [2.0, 6.0, 10.0] {
        b.prop("bench_2m", 2.0, g, z, 90.0);
        b.prop("bench_2m", 10.0, g, z, 90.0);
    }
    b.prop("rug_2x3", 6.0, g, 6.0, 0.0);
    b.prop("table_2m", 14.5, g, 9.0, 90.0);
    b.prop("barrel", 15.2, g, 1.0, 0.0);
    b.prop("crate_large", 14.4, g, 10.8, 0.0);
    b.lamp(6.0, 6.0, g, true);
    b.lamp(14.0, 6.0, g, false);
}

/// Stables: an arcaded lean-to against the south wall, east of the gate.
fn stables(b: &mut Builder) {
    b.at(16, 22);
    let y = GROUND;
    b.run((18, 0), West, y, "r4 r4 r4 r4 s2", Trim::OPEN);
    b.run((0, 0), South, y, "s4 s2 s1", Trim::OPEN);
    b.run((18, 2), North, y, "s2", Trim::OPEN);
    // Cobbled floor and roof run in under the curtain wall behind.
    b.paving((0, 0), (18, 8));
    b.floor((0, 0), (18, 8), UPPER, |_, _| Some("floor_stone_2m"));
    b.parapets((18, 0), West, UPPER, "4 4 4 4 2");
    for x in [4, 8, 12] {
        b.railing((x, 1), South, y, 2);
    }
    b.prop("sandbags_2m", 2.0, y, 5.6, 0.0);
    b.prop("sandbags_2m", 6.0, y, 5.6, 0.0);
    b.prop("barrel", 10.6, y, 5.8, 0.0);
    b.prop("crate_large", 14.0, y, 5.5, 20.0);
    b.prop("cart", 15.0, y, 2.8, 70.0);
    b.prop("crate_small", 9.4, y, 1.2, 0.0);
}

/// Inner walls between the yards, the well and the paving.
fn yards(b: &mut Builder) {
    b.at(0, 0);
    // West wall: well court | main yard, arch at z 0..4.
    b.run((-16, -8), South, GROUND, "s4 s4 a4 s4 s4", Trim::FREE);
    // East wall: main yard | east yard, arch at z 4..8.
    b.run((16, -8), South, GROUND, "s4 s4 s4 a4 s4 s4 s4 s2", Trim::FREE);
    // Paving: keep forecourt, the gate road, the well court square.
    b.paving((-16, -8), (16, 0));
    b.paving((-4, 0), (4, 26));
    b.paving((-26, -18), (-18, -10));
    b.prop("well", -22.0, GROUND, -14.0, 0.0);
    // Main yard: stalls, carts and crates for cover.
    b.prop("market_stall", -10.0, GROUND, 6.0, 90.0);
    b.prop("market_stall", 10.0, GROUND, 10.0, -90.0);
    b.prop("cart", -9.0, GROUND, 16.0, 20.0);
    b.prop("crate_large", 7.0, GROUND, 18.0, 0.0);
    b.prop("crate_large", 8.3, GROUND, 18.4, 15.0);
    b.prop("crate_small", 7.5, GROUND + 1.0, 18.1, 40.0);
    b.prop("sandbags_2m", -6.0, GROUND, -4.0, 0.0);
    b.prop("sandbags_2m", 6.0, GROUND, -4.0, 0.0);
    b.prop("barrel", 12.0, GROUND, 2.0, 0.0);
    b.prop("barrel", 12.6, GROUND, 2.4, 0.0);
    b.prop("rubble_pile", 13.0, GROUND, 23.0, 0.0);
    b.prop("market_stall", -11.0, GROUND, 22.0, 90.0);
    // Well court.
    b.prop("cart", -25.0, GROUND, -24.0, -20.0);
    b.prop("crate_large", -19.5, GROUND, -21.5, 0.0);
    b.prop("crate_small", -20.6, GROUND, -21.2, 10.0);
    b.prop("sandbags_2m", -24.0, GROUND, -4.0, 0.0);
    b.prop("rubble_small", -20.0, GROUND, 6.0, 0.0);
    b.prop("barrel", -27.0, GROUND, 9.0, 0.0);
    b.prop("market_stall", -34.0, GROUND, 9.0, 0.0);
    // East yard.
    b.prop("cart", 22.0, GROUND, -4.0, 100.0);
    b.prop("crate_large", 24.0, GROUND, -14.0, 0.0);
    b.prop("crate_large", 25.3, GROUND, -14.4, 25.0);
    b.prop("sandbags_2m", 22.0, GROUND, 12.0, 0.0);
    b.prop("rubble_pile", 34.0, GROUND, 14.0, 0.0);
    b.prop("barrel", 26.4, GROUND, 6.6, 0.0);
    b.prop("barrel", 27.0, GROUND, 7.1, 0.0);
    b.prop("crate_large", 30.0, GROUND, -25.0, 10.0);
    // Back lanes behind the old hall.
    b.prop("crate_large", -36.0, GROUND, 20.0, 0.0);
    b.prop("rubble_small", -24.0, GROUND, 26.5, 0.0);
}
