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
use crate::{config::Tuning, player::SpawnPoint};

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
        .add_systems(Startup, build_map);
        // `--no-batch` keeps every kit piece as its own mesh (to compare).
        if !std::env::args().any(|a| a == "--no-batch") {
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
