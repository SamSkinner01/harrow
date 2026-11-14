//! `--lineup-weapons [sidearm]`: puts the real weapon model (primary, or
//! the sidearm) in every lineup operator's hands, at the `weapon_r` bone with
//! the tuning's `model.third_person` transform, and hides the placeholder
//! carbine. A visual check of the attach transforms for the third-person
//! bodies (`characters/body.rs` does the same for bots and remote players).
//! Combine with `--lineup` / `--lineup-state <name>`.

use bevy::{
    gltf::GltfAssetLabel, prelude::*, world_serialization::WorldAssetRoot,
};

use super::{Slot, third_person_transform};
use crate::{
    characters::{Character, CharacterParts},
    config::Tuning,
};

pub(super) struct ThirdPersonCheckPlugin;

impl Plugin for ThirdPersonCheckPlugin {
    fn build(&self, app: &mut App) {
        let args: Vec<String> = std::env::args().collect();
        let Some(i) = args.iter().position(|a| a == "--lineup-weapons") else {
            return;
        };
        let slot = match args.get(i + 1).map(String::as_str) {
            Some("sidearm") => Slot::Sidearm,
            _ => Slot::Primary,
        };
        app.insert_resource(LineupWeapon(slot))
            .add_systems(Update, attach_weapons);
    }
}

#[derive(Resource)]
struct LineupWeapon(Slot);

#[derive(Component)]
struct HoldsWeapon;

fn attach_weapons(
    mut characters: Query<(Entity, &mut Character, &CharacterParts), Without<HoldsWeapon>>,
    slot: Res<LineupWeapon>,
    tuning: Res<Tuning>,
    server: Res<AssetServer>,
    mut commands: Commands,
) {
    let def = slot.0.def(&tuning);
    for (entity, mut character, parts) in &mut characters {
        let Some(bone) = parts.bone("weapon_r") else {
            continue;
        };
        character.prop_weapon = false;
        commands.spawn((
            Name::new(format!("Third-person {}", def.name)),
            WorldAssetRoot(server.load(GltfAssetLabel::Scene(0).from_asset(def.model.path.clone()))),
            third_person_transform(def),
            ChildOf(bone),
        ));
        commands.entity(entity).insert(HoldsWeapon);
        info!(
            "lineup: {} on {:?}'s weapon_r at {:?}",
            def.name, character.operator, def.model.third_person
        );
    }
}
