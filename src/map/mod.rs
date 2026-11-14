//! Map loading, spawn points and bomb sites.
//!
//! For now: the desert environment (sun, sky, haze, ground) and a grey-box
//! test area for the player controller. Harrow Keep itself is assembled from
//! the Blender fortress kit in M2.

mod environment;
mod greybox;

use bevy::prelude::*;

pub struct MapPlugin;

impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((environment::EnvironmentPlugin, greybox::GreyboxPlugin));
    }
}
