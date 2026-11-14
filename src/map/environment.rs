//! Desert environment: hard sun, pale sky, dusty haze and endless sand.
//!
//! Lighting is physically based: the sun uses real-world lux and every 3D
//! camera gets a matching sunlight exposure, so values carry over to the
//! real map.

use avian3d::prelude::*;
use bevy::{camera::Exposure, light::CascadeShadowConfigBuilder, prelude::*};

/// Pale, slightly warm desert sky.
const SKY: Color = Color::srgb(0.66, 0.74, 0.82);

/// Top of the open-desert ground. Slightly below 0 so play areas built at
/// y = 0 sit on top of it without z-fighting.
pub const DESERT_FLOOR_Y: f32 = -0.02;

pub struct EnvironmentPlugin;

impl Plugin for EnvironmentPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(SKY))
            .insert_resource(GlobalAmbientLight {
                // Sky fill. Kept low on purpose so shade reads as deep shadow.
                color: Color::srgb(0.75, 0.82, 1.0),
                brightness: 2500.0,
                ..default()
            })
            .add_systems(Startup, (spawn_sun, spawn_desert_floor))
            .add_observer(configure_camera);
    }
}

fn spawn_sun(mut commands: Commands) {
    commands.spawn((
        Name::new("Sun"),
        DirectionalLight {
            color: Color::srgb(1.0, 0.95, 0.86),
            illuminance: light_consts::lux::DIRECT_SUNLIGHT,
            shadow_maps_enabled: true,
            ..default()
        },
        // Afternoon sun, ~43 degrees up.
        Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, 0.55, -0.75, 0.0)),
        CascadeShadowConfigBuilder {
            first_cascade_far_bound: 8.0,
            maximum_distance: 90.0,
            ..default()
        }
        .build(),
    ));
}

fn spawn_desert_floor(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    const SIZE: f32 = 2000.0;
    commands.spawn((
        Name::new("Desert floor"),
        Mesh3d(meshes.add(Plane3d::default().mesh().size(SIZE, SIZE))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.76, 0.66, 0.49),
            perceptual_roughness: 0.92,
            ..default()
        })),
        Transform::from_xyz(0.0, DESERT_FLOOR_Y, 0.0),
    ));
    // Collision: a thick slab whose top is the desert floor.
    commands.spawn((
        RigidBody::Static,
        Collider::cuboid(SIZE, 1.0, SIZE),
        Transform::from_xyz(0.0, DESERT_FLOOR_Y - 0.5, 0.0),
    ));
}

/// Every 3D camera gets sunlight exposure and the dusty haze.
fn configure_camera(add: On<Add<Camera3d>>, mut commands: Commands) {
    commands.entity(add.entity).insert((
        Exposure::SUNLIGHT,
        DistanceFog {
            color: SKY,
            directional_light_color: Color::srgba(1.0, 0.93, 0.80, 0.35),
            directional_light_exponent: 20.0,
            falloff: FogFalloff::from_visibility(900.0),
        },
    ));
}
