//! Interior lighting: hanging lanterns with warm spot lights.
//!
//! Spot lights pointing down (rather than point lights) give one shadow map
//! each, so a lamp lights its own room without bleeding through walls.
//! Shadow maps cost a render pass each, so on the full map only lamps in
//! the larger rooms cast shadows; the rest are dimmer and shorter-ranged so
//! their light hardly reaches through walls (`lighting` tuning section).
//! Intensities are game values, not real-world ones: with the camera exposed
//! for desert sun, a real 100 W bulb would be invisible.
//!
//! All lanterns share one set of meshes and materials ([`LampAssets`]), so
//! they batch.

use bevy::prelude::*;

use crate::config::Tuning;

pub struct LampsPlugin;

impl Plugin for LampsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LampAssets>().add_systems(
            PreUpdate,
            apply_lamp_tuning.run_if(resource_changed::<Tuning>),
        );
    }
}

/// Marks a lamp's light so tuning changes can reach it.
#[derive(Component)]
struct Lamp {
    shadows: bool,
}

/// Shared lantern meshes and materials.
#[derive(Resource)]
pub struct LampAssets {
    rod: Handle<Mesh>,
    plate: Handle<Mesh>,
    glass: Handle<Mesh>,
    glow: Handle<StandardMaterial>,
    iron: Handle<StandardMaterial>,
}

const ROD: f32 = 0.35;

impl FromWorld for LampAssets {
    fn from_world(world: &mut World) -> Self {
        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        let rod = meshes.add(Cuboid::new(0.02, ROD, 0.02));
        let plate = meshes.add(Cuboid::new(0.16, 0.03, 0.16));
        let glass = meshes.add(Cuboid::new(0.12, 0.18, 0.12));
        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
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
        Self { rod, plate, glass, glow, iron }
    }
}

/// Spawn a lantern hanging from `ceiling`; `shadows` decides whether its
/// light gets a shadow map.
pub fn spawn_lantern(commands: &mut Commands, assets: &LampAssets, tuning: &Tuning, ceiling: Vec3, shadows: bool) {
    commands
        .spawn((
            Name::new("Lamp"),
            Transform::from_translation(ceiling),
            Visibility::default(),
        ))
        .with_children(|lamp| {
            lamp.spawn((
                Mesh3d(assets.rod.clone()),
                MeshMaterial3d(assets.iron.clone()),
                Transform::from_xyz(0.0, -ROD / 2.0, 0.0),
            ));
            lamp.spawn((
                Mesh3d(assets.plate.clone()),
                MeshMaterial3d(assets.iron.clone()),
                Transform::from_xyz(0.0, -ROD, 0.0),
            ));
            lamp.spawn((
                Mesh3d(assets.glass.clone()),
                MeshMaterial3d(assets.glow.clone()),
                Transform::from_xyz(0.0, -ROD - 0.105, 0.0),
                // The glass glows; it shouldn't block its own light.
                bevy::light::NotShadowCaster,
            ));
            lamp.spawn((
                Lamp { shadows },
                lamp_light(tuning, shadows),
                Transform::from_xyz(0.0, -ROD - 0.25, 0.0).looking_to(Vec3::NEG_Y, Vec3::Z),
            ));
        });
}

fn lamp_light(tuning: &Tuning, shadows: bool) -> SpotLight {
    let l = &tuning.lighting;
    let (r, g, b) = l.lamp_colour;
    // Unshadowed lamps light through walls, so keep them dimmer and short.
    let (intensity, range) = if shadows {
        (l.lamp_intensity, l.lamp_range)
    } else {
        (l.lamp_intensity * l.unshadowed_lamp_scale, l.lamp_range * l.unshadowed_lamp_scale)
    };
    SpotLight {
        color: Color::srgb(r, g, b),
        intensity,
        range,
        radius: 0.06,
        outer_angle: 1.25,
        inner_angle: 0.5,
        shadow_maps_enabled: shadows,
        ..default()
    }
}

fn apply_lamp_tuning(tuning: Res<Tuning>, mut lamps: Query<(&mut SpotLight, &Lamp)>) {
    for (mut light, lamp) in &mut lamps {
        *light = lamp_light(&tuning, lamp.shadows);
    }
}
