//! Interior lighting: hanging lanterns with warm, shadow-casting spot lights.
//!
//! Spot lights pointing down (rather than point lights) give one shadow map
//! each, so a lamp lights its own room without bleeding through walls.
//! Brightness, colour and range come from the `lighting` tuning section.
//! Intensities are game values, not real-world ones: with the camera exposed
//! for desert sun, a real 100 W bulb would be invisible.

use bevy::prelude::*;

use crate::config::Tuning;

pub struct LampsPlugin;

impl Plugin for LampsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PreUpdate,
            apply_lamp_tuning.run_if(resource_changed::<Tuning>),
        );
    }
}

/// Marks a lamp's light so tuning changes can reach it.
#[derive(Component)]
struct Lamp;

/// Spawn a lantern hanging from a ceiling at `ceiling` (the slab underside).
pub fn spawn_lamp(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    tuning: &Tuning,
    ceiling: Vec3,
) {
    let glow = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.85, 0.6),
        emissive: LinearRgba::rgb(1.0, 0.72, 0.42) * 40_000.0,
        ..default()
    });
    let iron = materials.add(StandardMaterial {
        base_color: Color::srgb(0.08, 0.08, 0.08),
        perceptual_roughness: 0.6,
        metallic: 1.0,
        ..default()
    });
    let rod = 0.35;
    commands
        .spawn((
            Name::new("Lamp"),
            Transform::from_translation(ceiling),
            Visibility::default(),
        ))
        .with_children(|lamp| {
            lamp.spawn((
                Mesh3d(meshes.add(Cuboid::new(0.02, rod, 0.02))),
                MeshMaterial3d(iron.clone()),
                Transform::from_xyz(0.0, -rod / 2.0, 0.0),
            ));
            lamp.spawn((
                Mesh3d(meshes.add(Cuboid::new(0.16, 0.03, 0.16))),
                MeshMaterial3d(iron),
                Transform::from_xyz(0.0, -rod, 0.0),
            ));
            lamp.spawn((
                Mesh3d(meshes.add(Cuboid::new(0.12, 0.18, 0.12))),
                MeshMaterial3d(glow),
                Transform::from_xyz(0.0, -rod - 0.105, 0.0),
                // The glass glows; it shouldn't block its own light.
                bevy::light::NotShadowCaster,
            ));
            lamp.spawn((
                Lamp,
                SpotLight {
                    shadow_maps_enabled: true,
                    ..lamp_light(tuning)
                },
                Transform::from_xyz(0.0, -rod - 0.25, 0.0)
                    .looking_to(Vec3::NEG_Y, Vec3::Z),
            ));
        });
}

fn lamp_light(tuning: &Tuning) -> SpotLight {
    let l = &tuning.lighting;
    let (r, g, b) = l.lamp_colour;
    SpotLight {
        color: Color::srgb(r, g, b),
        intensity: l.lamp_intensity,
        range: l.lamp_range,
        radius: 0.06,
        outer_angle: 1.25,
        inner_angle: 0.5,
        shadow_maps_enabled: true,
        ..default()
    }
}

fn apply_lamp_tuning(tuning: Res<Tuning>, mut lamps: Query<&mut SpotLight, With<Lamp>>) {
    for mut light in &mut lamps {
        *light = lamp_light(&tuning);
    }
}
