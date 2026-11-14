//! Weapon material set: one shared `StandardMaterial` per material name used
//! by the Blender weapon scripts (`tools/blender/weapons/models.py`,
//! `MATERIALS`). Like the kit's material library, the `.glb` only *names* a
//! material and the game swaps in the version defined here.
//!
//! Weapons are untextured blockouts, so these are plain PBR values. The sight
//! dots are unlit so they read in dark rooms.

use std::collections::HashMap;

use bevy::prelude::*;

pub struct WeaponMaterialsPlugin;

impl Plugin for WeaponMaterialsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WeaponMaterials>();
    }
}

#[derive(Resource)]
pub struct WeaponMaterials(HashMap<&'static str, Handle<StandardMaterial>>);

impl WeaponMaterials {
    pub fn get(&self, name: &str) -> Option<Handle<StandardMaterial>> {
        self.0.get(name).cloned()
    }
}

fn pbr(rgb: (f32, f32, f32), metallic: f32, roughness: f32) -> StandardMaterial {
    StandardMaterial {
        base_color: Color::srgb(rgb.0, rgb.1, rgb.2),
        metallic,
        perceptual_roughness: roughness,
        ..default()
    }
}

impl FromWorld for WeaponMaterials {
    fn from_world(world: &mut World) -> Self {
        let mut assets = world.resource_mut::<Assets<StandardMaterial>>();
        let mut set = HashMap::new();
        let mut add = |name: &'static str, material: StandardMaterial| {
            set.insert(name, assets.add(material));
        };
        add("weapon_steel", pbr((0.24, 0.245, 0.25), 0.35, 0.45));
        add("weapon_dark", pbr((0.1, 0.1, 0.105), 0.3, 0.55));
        add("weapon_polymer_sand", pbr((0.46, 0.38, 0.27), 0.0, 0.72));
        add("weapon_polymer_black", pbr((0.11, 0.11, 0.11), 0.0, 0.6));
        add("weapon_rubber", pbr((0.04, 0.04, 0.04), 0.0, 0.92));
        add(
            "weapon_glass",
            StandardMaterial {
                base_color: Color::srgba(0.7, 0.8, 0.85, 0.06),
                perceptual_roughness: 0.05,
                reflectance: 0.9,
                alpha_mode: AlphaMode::Blend,
                ..default()
            },
        );
        add(
            "weapon_dot",
            StandardMaterial {
                base_color: Color::linear_rgb(4.0, 0.08, 0.04),
                unlit: true,
                ..default()
            },
        );
        add(
            "weapon_sight_white",
            StandardMaterial {
                base_color: Color::srgb(0.8, 0.95, 0.75),
                unlit: true,
                ..default()
            },
        );
        Self(set)
    }
}
