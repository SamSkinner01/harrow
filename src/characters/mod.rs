//! Character models (M5): operators built by the Blender/MPFB pipeline
//! (`tools/scripts/build_characters.sh` -> `assets/models/characters/*.glb`)
//! and their animation graphs.
//!
//! Spawning: [`spawn_character`] (or the [`character`] bundle) spawns an
//! operator's model with its feet at the transform's origin, facing the
//! transform's forward (-Z). Nothing here touches the local player or the
//! camera.
//!
//! Animating: write [`CharacterAnim`] (speed, stance, lean, aiming, firing,
//! reloading) every frame; bots and the third-person view will do that.
//! The graph blends the locomotion clips by speed and stance, overrides the
//! arms for aiming/firing/reloading and the spine for leaning. See
//! `animation.rs` for the details.
//!
//! Once a model is ready it gets [`CharacterParts`]: its animation player
//! and every bone entity by name (e.g. `head`, `spine_03`, `weapon_r`).
//!
//! `--lineup` spawns all four operators in the keep courtyard cycling
//! through their animations (`--lineup-state <name>` holds one state).

mod animation;
mod library;
mod lineup;
mod setup;

use bevy::{gltf::GltfAssetLabel, prelude::*, world_serialization::WorldAssetRoot};

pub use animation::CharacterAnim;
pub use library::Operator;
pub use setup::{CharacterMaterials, CharacterParts};
pub(crate) use setup::add_mip_chain;

pub struct CharactersPlugin;

impl Plugin for CharactersPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            library::LibraryPlugin,
            setup::SetupPlugin,
            animation::AnimationPlugin,
            lineup::LineupPlugin,
        ));
    }
}

/// An operator's model. The root entity of the spawned glTF scene.
#[derive(Component, Clone, Copy, Debug)]
#[require(CharacterAnim)]
pub struct Character {
    pub operator: Operator,
    /// Show the placeholder carbine held in the right hand (attached to the
    /// `weapon_r` bone). Hide it once real weapon models attach there.
    pub prop_weapon: bool,
}

/// Components for spawning `operator` (add a `Transform`).
pub fn character(assets: &AssetServer, operator: Operator) -> impl Bundle {
    (
        Name::new(format!("Operator {}", operator.name())),
        Character {
            operator,
            prop_weapon: true,
        },
        WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(operator.model_path()))),
    )
}

/// Spawn `operator` at `transform` (feet at the origin, facing -Z).
pub fn spawn_character(
    commands: &mut Commands,
    assets: &AssetServer,
    operator: Operator,
    transform: Transform,
) -> Entity {
    commands
        .spawn((character(assets, operator), transform))
        .id()
}
