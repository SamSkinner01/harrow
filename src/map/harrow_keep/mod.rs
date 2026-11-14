//! Harrow Keep, the full map: a walled desert fortress with a three-storey
//! keep (plus basement) on its north wall, a gatehouse, barracks, granary,
//! old hall and stables around three yards, corner towers and a wall-walk,
//! and the desert approaches outside. Layout, callouts and site pairs:
//! DESIGN.md, "Map: Harrow Keep". Bomb sites and spawns:
//! `src/round/sites.rs`.
//!
//! World: x east, z south (north is -Z), y up. The curtain walls' centre
//! lines are x = +-40 and z = +-30. Storeys are 3.2 m; levels below.
//!
//! Everything is assembled at startup from fortress-kit pieces through a
//! [`builder::Builder`]; each building has its own module and local grid.

mod builder;
mod compound;
mod dressing;
mod exterior;
mod keep;
mod wings;

use bevy::prelude::*;

use super::{
    batch::StaticBatching,
    environment::DesertHoles,
    lamps::{LampAssets, spawn_lantern},
};
use crate::{
    config::Tuning,
    gadgets::{CctvLayout, CctvSpot},
    player::SpawnPoint,
};

/// Floor heights.
pub const BASEMENT: f32 = -3.2;
pub const GROUND: f32 = 0.0;
pub const UPPER: f32 = 3.2;
/// Third floor, terraces and the wall-walk.
pub const TERRACE: f32 = 6.4;
pub const ROOF: f32 = 9.6;

pub struct HarrowKeepPlugin;

impl Plugin for HarrowKeepPlugin {
    fn build(&self, app: &mut App) {
        let (kx, kz) = (keep::ORIGIN.0 as f32, keep::ORIGIN.1 as f32);
        app.insert_resource(SpawnPoint {
            // Free play: on the road outside the gate, facing it.
            position: Vec3::new(0.0, 0.0, 44.0),
            yaw: 0.0,
            pitch: 0.0,
        })
        .insert_resource(DesertHoles(vec![
            // The keep's basement.
            Rect::new(kx, kz, kx + 32.0, kz + 22.0),
            // The sunken cellar steps outside its north face.
            Rect::new(kx + 20.0, kz - 6.0, kx + 22.0, kz),
        ]))
        .add_systems(Startup, (build_map, insert_cctv_layout));
        // `--no-batch` keeps every kit piece as its own mesh (to compare);
        // a headless server (no render app) draws nothing, so skips it.
        let renders = app.get_sub_app(bevy::render::RenderApp).is_some();
        if renders && !std::env::args().any(|a| a == "--no-batch") {
            app.init_resource::<StaticBatching>();
        }
    }
}

fn build_map(commands: Commands, assets: Res<AssetServer>, lamp_assets: Res<LampAssets>, tuning: Res<Tuning>) {
    let mut b = builder::Builder::new(commands, &assets);
    keep::build(&mut b);
    compound::build(&mut b);
    wings::build(&mut b);
    exterior::build(&mut b);
    dressing::build(&mut b);
    let pieces = b.pieces;
    let lamps = std::mem::take(&mut b.lamps);
    let mut commands = b.commands;
    for lamp in &lamps {
        spawn_lantern(&mut commands, &lamp_assets, &tuning, lamp.ceiling, lamp.shadows);
    }
    let shadowed = lamps.iter().filter(|l| l.shadows).count();
    info!(
        "Harrow Keep: {pieces} kit pieces, {} lamps ({shadowed} with shadows)",
        lamps.len()
    );
}

/// Defender CCTV spots: three per site pair (both sites and the way in
/// between them) and four outside. Lens positions are just under the
/// ceilings, in room corners. Yaw in degrees: 0 faces north (-Z), 90 west.
/// Inserted at startup so it replaces the gadgets' default layout before
/// the cameras spawn (`PostStartup`).
fn insert_cctv_layout(mut commands: Commands) {
    let spot = |name, x, y, z, yaw, pitch| CctvSpot {
        name,
        position: Vec3::new(x, y, z),
        yaw,
        pitch,
    };
    commands.insert_resource(CctvLayout {
        spots: vec![
            // Basement.
            spot("CISTERN (1A)", 15.5, -0.45, -29.5, 125.0, -20.0),
            spot("POWDER STORE (1B)", 11.5, -0.45, -8.5, 35.0, -20.0),
            spot("UNDERCROFT", -15.5, -0.45, -21.5, 253.0, -15.0),
            // Ground floor.
            spot("GREAT HALL (2A)", 7.5, 2.7, -14.5, 65.0, -18.0),
            spot("ARMOURY (2B)", -2.5, 2.7, -29.5, 142.0, -24.0),
            spot("ENTRANCE", 3.5, 2.7, -8.5, 45.0, -20.0),
            // Second floor.
            spot("MAP ROOM (3A)", -2.5, 5.9, -22.5, 62.0, -18.0),
            spot("RECORDS (3B)", 15.5, 5.9, -22.5, 59.0, -18.0),
            spot("LONG GALLERY", 15.5, 5.9, -21.5, 101.0, -12.0),
            // Third floor.
            spot("SIGNAL ROOM (4A)", -7.5, 9.1, -18.5, 335.0, -22.0),
            spot("STUDY (4B)", 7.5, 9.1, -18.5, 25.0, -22.0),
            spot("LANDING", 1.5, 9.1, -18.5, 10.0, -25.0),
            // Outside: the keep's facades and the gate passage.
            spot("MAIN YARD", 3.0, 5.0, -7.6, 180.0, -12.0),
            spot("WELL COURT", -16.4, 5.0, -12.0, 120.0, -15.0),
            spot("EAST YARD", 16.4, 5.0, -24.0, 260.0, -15.0),
            spot("GATE", 1.5, 2.6, 26.3, 180.0, -8.0),
        ],
    });
}
