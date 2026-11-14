//! The desert outside the walls: the road to the gate, dunes on the horizon,
//! rocks for cover on the approaches, and the ruined walls that shelter the
//! attacker spawns from the fortress (no spawn peeking from the walls).

use super::builder::{Builder, Dir, Trim};
use super::GROUND;
use Dir::*;

pub fn build(b: &mut Builder) {
    b.at(0, 0);
    shelters(b);
    tracks(b);
    dunes(b);
    rocks(b);
}

/// U-shaped ruins (3 m stone walls), open away from the fortress. The
/// attacker spawns stand inside them; see `src/round/sites.rs`.
fn shelters(b: &mut Builder) {
    let g = GROUND;
    let free = Trim::FREE;
    // North ruin, spawns at z = -70.
    b.run((-8, -66), East, g, "s4 s4 s4 s4", free);
    b.run((-8, -66), North, g, "s4", free);
    b.run((8, -66), North, g, "s2", free);
    // West ruin, spawns at x = -80.
    b.run((-76, -8), South, g, "s4 s4 s4 s4", free);
    b.run((-76, -8), West, g, "s4", free);
    b.run((-76, 8), West, g, "s2", free);
    // East ruin, spawns at x = 80.
    b.run((76, -8), South, g, "s4 s4 s4 s4", free);
    b.run((76, -8), East, g, "s2", free);
    b.run((76, 8), East, g, "s4", free);
    // South ruin: the old outer gate on the road, spawns at z = 72.
    b.run((-10, 68), East, g, "s4 s4 a4 s4 s4", free);
    b.run((-10, 68), South, g, "s4", free);
    b.run((10, 68), South, g, "s2", free);
    for (x, z) in [(-9.0, -63.5), (9.5, -68.5), (-73.5, 9.0), (73.5, -9.0), (-12.0, 70.0), (12.5, 66.0)] {
        b.prop("rubble_pile", x, g, z, 30.0);
    }
    for (x, z) in [(3.0, -64.5), (-74.5, -3.0), (74.5, 3.0), (6.0, 66.5)] {
        b.prop("rubble_small", x, g, z, 0.0);
    }
    // Crates and a cart left at the spawns.
    b.prop("cart", -5.0, g, -71.5, 80.0);
    b.prop("crate_large", -80.5, g, 5.5, 15.0);
    b.prop("crate_large", 81.0, g, -5.0, -10.0);
    b.prop("cart", 7.0, g, 73.0, 10.0);
}

/// Gravel tracks: the road to the gate from the south, a cross track, and
/// the path to the postern from the north.
fn tracks(b: &mut Builder) {
    const Y: f32 = -0.012;
    for i in 0..10 {
        b.place("track_8m", bevy::math::Vec3::new(0.0, Y, 34.0 + 8.0 * i as f32), South.rotation());
    }
    for i in 0..20 {
        let x = -80.0 + 8.0 * i as f32;
        b.place("track_8m", bevy::math::Vec3::new(x, Y - 0.002, 52.0), East.rotation());
    }
    for i in 0..4 {
        b.place("track_8m", bevy::math::Vec3::new(-11.0, Y, -66.0 + 8.0 * i as f32), South.rotation());
    }
}

/// Big dunes, well outside the fighting area.
fn dunes(b: &mut Builder) {
    let g = GROUND;
    for (x, z, yaw) in [
        (-60.0, -60.0, 30.0),
        (55.0, -62.0, -20.0),
        (-95.0, 10.0, 80.0),
        (95.0, -15.0, 95.0),
        (-60.0, 70.0, -35.0),
        (62.0, 72.0, 25.0),
        (0.0, -105.0, 5.0),
        (-30.0, 110.0, 10.0),
        (35.0, 115.0, -10.0),
        (-120.0, -50.0, 50.0),
        (125.0, 45.0, -60.0),
        (-115.0, 60.0, 20.0),
        (110.0, -80.0, 15.0),
    ] {
        b.prop("dune_large", x, g, z, yaw);
    }
    for (x, z, yaw) in [
        (-30.0, -58.0, 10.0),
        (30.0, -56.0, -15.0),
        (-62.0, -32.0, 70.0),
        (-60.0, 32.0, 100.0),
        (60.0, -30.0, 110.0),
        (64.0, 30.0, 75.0),
        (-30.0, 58.0, -5.0),
        (32.0, 60.0, 20.0),
    ] {
        b.prop("dune_small", x, g, z, yaw);
    }
}

/// Boulders: cover on the approaches, 10-25 m out from the walls.
fn rocks(b: &mut Builder) {
    let g = GROUND;
    for (name, x, z, yaw) in [
        ("rock_large", -20.0, -46.0, 0.0),
        ("rock_medium", -17.5, -44.5, 40.0),
        ("rock_large", 21.0, -45.0, 110.0),
        ("rock_small", 23.0, -47.0, 0.0),
        ("rock_medium", -4.0, -48.0, 70.0),
        ("rock_large", -54.0, -18.0, 200.0),
        ("rock_medium", -52.0, -16.0, 10.0),
        ("rock_large", -55.0, 19.0, 60.0),
        ("rock_small", -52.5, 21.0, 0.0),
        ("rock_medium", -50.0, 2.0, 140.0),
        ("rock_large", 54.0, -17.0, 30.0),
        ("rock_medium", 51.5, -15.5, 0.0),
        ("rock_large", 53.0, 18.0, 250.0),
        ("rock_small", 50.5, 16.0, 0.0),
        ("rock_medium", 50.0, -2.0, 90.0),
        ("rock_large", -19.0, 47.0, 160.0),
        ("rock_medium", -16.5, 45.5, 20.0),
        ("rock_large", 18.0, 46.0, 300.0),
        ("rock_small", 20.0, 44.5, 0.0),
        ("rock_medium", 9.0, 40.0, 45.0),
        ("rock_medium", -45.0, -40.0, 10.0),
        ("rock_medium", 46.0, 38.0, 80.0),
        ("rock_small", -44.0, 37.0, 0.0),
        ("rock_small", 44.0, -36.0, 0.0),
    ] {
        b.prop(name, x, g, z, yaw);
    }
}
