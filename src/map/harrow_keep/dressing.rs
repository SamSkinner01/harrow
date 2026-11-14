//! Scanned props (CC0, Poly Haven) dressing rooms, yards and the desert, on
//! top of the kit furniture placed with each building.

use super::builder::Builder;
use super::{BASEMENT, GROUND, TERRACE, UPPER, keep};

pub fn build(b: &mut Builder) {
    keep_rooms(b);
    compound(b);
    desert(b);
}

/// Keep, in its local grid (x 0..32, z 0..22).
fn keep_rooms(b: &mut Builder) {
    b.at(keep::ORIGIN.0, keep::ORIGIN.1);
    let (bs, g, u, t) = (BASEMENT, GROUND, UPPER, TERRACE);
    // Oil cellar: casks along the west wall.
    for z in [4.0, 5.0, 6.0] {
        b.model("wine_barrel_01", 0.8, bs, z, 0.0, 1.0);
    }
    b.model("ceramic_pot", 7.2, bs, 1.0, 30.0, 1.0);
    // Powder store: field crates, a rack, ammunition.
    b.model("old_military_crate", 22.5, bs, 20.6, 0.0, 1.0);
    b.model("worn_metal_rack", 27.35, bs, 17.5, 90.0, 1.0);
    b.model("ammo_box", 20.0, bs + 1.0, 14.0, 20.0, 1.0);
    b.model("ammo_box", 26.6, bs + 1.0, 20.6, -30.0, 1.0);
    b.model("wooden_military_crate", 24.0, bs, 12.5, 0.0, 1.0);
    // Cistern and crypt.
    b.model("wooden_bucket_01", 24.0, bs, 6.2, 0.0, 1.0);
    b.model("wine_barrel_01", 16.8, bs, 21.0, 0.0, 1.0);
    // Armoury: workbench, racks, crates.
    b.model("WoodenTable_03", 12.8, g, 1.0, 0.0, 1.0);
    b.model("ammo_box", 12.6, g + 0.83, 1.0, 10.0, 1.0);
    b.model("ammo_box", 10.5, g + 0.8, 4.4, 80.0, 1.0);
    b.model("wooden_military_crate", 12.6, g, 6.3, 90.0, 1.0);
    b.model("worn_metal_rack", 8.55, g, 2.5, -90.0, 1.0);
    // Larder and kitchen.
    b.model("ceramic_pot", 19.2, g, 4.0, 0.0, 1.0);
    b.model("ceramic_pot", 19.3, g, 4.8, 50.0, 1.0);
    b.model("brass_pot_01", 30.8, g, 1.8, 0.0, 1.0);
    b.model("brass_pot_01", 29.0, g + 0.8, 9.5, 0.0, 1.0);
    b.model("ceramic_pot", 25.2, g, 9.0, 0.0, 1.0);
    b.model("wine_barrel_01", 21.0, g, 21.2, 0.0, 1.0);
    // Guardroom.
    b.model("folding_wooden_stool", 9.0, g, 20.0, 15.0, 1.0);
    // Map room: the field radio on the west table.
    b.model("vintage_radio_transceiver", 2.0, u + 0.8, 6.6, 180.0, 1.0);
    b.model("folding_wooden_stool", 2.0, u, 7.4, 0.0, 1.0);
    // Records and the infirmary.
    b.model("wooden_bookshelf_worn", 31.45, u, 5.0, -90.0, 1.0);
    b.model("small_wooden_table_01", 8.0, u, 21.3, 0.0, 1.0);
    b.model("medical_box", 8.0, u + 0.53, 21.3, 10.0, 1.0);
    b.model("medical_box", 5.2, u, 15.2, 80.0, 1.0);
    // Signal room and study.
    b.model("vintage_radio_transceiver", 13.0, t + 0.8, 9.2, 90.0, 1.0);
    b.model("ammo_box", 12.6, t + 1.0, 4.6, 0.0, 1.0);
    b.model("small_wooden_table_01", 19.0, t, 4.0, 90.0, 1.0);
    b.model("folding_wooden_stool", 19.7, t, 4.0, 0.0, 1.0);
    b.model("wooden_bookshelf_worn", 23.45, t, 11.0, -90.0, 1.0);
    // Terrace lookouts.
    b.model("folding_wooden_stool", 2.0, t, 12.0, 30.0, 1.0);
    b.model("metal_jerrycan_green", 30.5, t, 21.0, 0.0, 1.0);
    b.model("planter_pot_clay", 14.0, g, 23.2, 0.0, 1.2);
    b.model("planter_pot_clay", 18.0, g, 23.2, 0.0, 1.2);
}

/// Yards, gatehouse and walls, in world coordinates.
fn compound(b: &mut Builder) {
    b.at(0, 0);
    let (g, u, t) = (GROUND, UPPER, TERRACE);
    // Well court.
    b.model("wooden_bucket_01", -20.9, g, -13.2, 0.0, 1.0);
    b.model("stone_fire_pit", -22.0, g, 2.5, 0.0, 1.0);
    b.model("quiver_tree_02_lowpoly", -26.0, g, -6.0, 40.0, 2.2);
    b.model("planter_pot_clay", -18.5, g, -18.5, 0.0, 1.3);
    // Main yard: trees, pots by the stalls, water cans.
    b.model("quiver_tree_02_lowpoly", -12.0, g, -2.0, 0.0, 2.4);
    b.model("quiver_tree_02_lowpoly", 12.5, g, 15.0, 120.0, 2.0);
    b.model("brass_pot_01", -8.6, g, 4.9, 0.0, 1.0);
    b.model("ceramic_pot", -8.6, g, 7.2, 20.0, 1.0);
    b.model("brass_pot_01", 8.4, g, 11.2, 0.0, 1.0);
    b.model("metal_jerrycan_green", 6.6, g, 17.0, 30.0, 1.0);
    // Gatehouse guard room and gate room.
    b.model("folding_wooden_stool", 5.0, g, 29.5, 0.0, 1.0);
    b.model("wooden_military_crate", -3.5, u, 27.0, 0.0, 1.0);
    b.model("ammo_box", -3.5, u + 0.46, 27.0, 15.0, 1.0);
    // Stables.
    b.model("wooden_bucket_01", 19.0, g, 26.5, 0.0, 1.0);
    b.model("wooden_bucket_01", 23.2, g, 27.0, 0.0, 1.0);
    b.model("metal_jerrycan_green", 30.5, g, 27.6, 80.0, 1.0);
    // East yard and granary.
    b.model("old_military_crate", 25.5, g, -6.0, 90.0, 1.0);
    b.model("quiver_tree_02_lowpoly", 25.0, g, 18.0, 200.0, 2.2);
    b.model("old_military_crate", 36.0, g, -17.5, 0.0, 1.0);
    // Barracks drill hall.
    b.model("wooden_military_crate", -33.0, g, 1.5, 0.0, 1.0);
    // Wall-walk posts: a stool and an ammunition box here and there.
    for (x, z) in [(-28.0, -29.0), (26.0, -29.0), (-18.0, 30.0), (20.0, 30.0)] {
        b.model("folding_wooden_stool", x, t, z, 40.0, 1.0);
        b.model("ammo_box", x + 0.8, t, z, 0.0, 1.0);
    }
}

/// Rocks, dead trees and camp clutter outside the walls.
fn desert(b: &mut Builder) {
    b.at(0, 0);
    let g = GROUND - 0.05;
    for (x, z, yaw, scale) in [
        (-24.0, -50.0, 20.0, 1.0),
        (24.5, -50.5, 140.0, 1.1),
        (-57.0, 10.0, 70.0, 1.2),
        (57.5, -8.0, 200.0, 1.0),
        (-23.0, 57.0, 300.0, 1.1),
        (25.0, 57.5, 10.0, 1.0),
        (-48.0, -44.0, 90.0, 1.3),
        (49.0, 44.0, 250.0, 1.2),
    ] {
        b.model("namaqualand_boulder_04_lowpoly", x, g, z, yaw, scale);
    }
    for (x, z, yaw) in [
        (-8.0, -50.0, 0.0),
        (13.0, -53.0, 60.0),
        (-50.0, -6.0, 120.0),
        (51.0, 7.0, 30.0),
        (-11.0, 47.0, 200.0),
        (9.0, 47.5, 160.0),
        (-36.0, -38.0, 80.0),
        (38.0, 37.0, 10.0),
    ] {
        b.model("namaqualand_boulder_02_lowpoly", x, g, z, yaw, 1.2);
    }
    for (x, z, yaw, scale) in [
        (-35.0, -45.0, 0.0, 2.6),
        (40.0, 48.0, 90.0, 2.3),
        (-46.0, 30.0, 200.0, 2.8),
        (60.0, -40.0, 300.0, 2.4),
        (-62.0, 46.0, 45.0, 2.0),
    ] {
        b.model("quiver_tree_02_lowpoly", x, GROUND, z, yaw, scale);
    }
    b.model("dead_quiver_trunk", 30.0, GROUND, -42.0, 0.0, 1.3);
    b.model("dead_quiver_trunk", -42.0, GROUND, 50.0, 70.0, 1.1);
    // Attacker spawns: camp clutter.
    b.model("stone_fire_pit", 84.0, GROUND, 0.0, 0.0, 1.0);
    b.model("old_military_crate", 6.0, GROUND, -72.5, 10.0, 1.0);
    b.model("metal_jerrycan_green", -8.5, GROUND, 72.5, 30.0, 1.0);
    b.model("metal_jerrycan_green", -81.0, GROUND, -6.0, 100.0, 1.0);
    b.model("wooden_military_crate", -82.0, GROUND, 3.0, 90.0, 1.0);
}
