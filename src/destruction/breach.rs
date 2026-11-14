//! Breach charge (attacker secondary gadget). Filled in below.

use bevy::prelude::*;

pub struct BreachChargePlugin;

impl Plugin for BreachChargePlugin {
    fn build(&self, _app: &mut App) {}
}

/// A placed breach charge.
#[derive(Component, Debug)]
pub struct BreachCharge {
    pub owner: Entity,
    pub surface: Entity,
}
