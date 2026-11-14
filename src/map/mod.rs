//! Map loading, spawn points and bomb sites.
//!
//! For now this only holds the M0 lighting test scene; Harrow Keep itself is
//! assembled from the Blender fortress kit in M2.

mod test_scene;

use bevy::prelude::*;

pub struct MapPlugin;

impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(test_scene::TestScenePlugin);
    }
}
