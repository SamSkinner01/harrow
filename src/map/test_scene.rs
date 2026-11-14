//! M0 test scene: a sandy ground plane, a few boxes, a small roofed shelter
//! (to check deep interior shadows), a hard desert sun and a pale sky.
//!
//! Lighting is physically based: the sun uses real-world lux and the camera
//! uses a matching sunlight exposure, so values carry over to the real map.

use bevy::{light::CascadeShadowConfigBuilder, prelude::*};

/// Pale, slightly warm desert sky.
const SKY: Color = Color::srgb(0.66, 0.74, 0.82);

pub struct TestScenePlugin;

impl Plugin for TestScenePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(SKY))
            .insert_resource(GlobalAmbientLight {
                // Sky fill. Kept low on purpose so shade reads as deep shadow.
                color: Color::srgb(0.75, 0.82, 1.0),
                brightness: 2500.0,
                ..default()
            })
            .add_systems(Startup, (spawn_lighting, spawn_geometry, spawn_camera));
    }
}

fn spawn_lighting(mut commands: Commands) {
    commands.spawn((
        DirectionalLight {
            color: Color::srgb(1.0, 0.95, 0.86),
            illuminance: light_consts::lux::DIRECT_SUNLIGHT,
            shadow_maps_enabled: true,
            ..default()
        },
        // Afternoon sun, ~43 degrees up, from the front-right of the camera.
        Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, 0.55, -0.75, 0.0)),
        CascadeShadowConfigBuilder {
            first_cascade_far_bound: 12.0,
            maximum_distance: 90.0,
            ..default()
        }
        .build(),
    ));
}

fn spawn_geometry(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let matte = |color: Color| StandardMaterial {
        base_color: color,
        perceptual_roughness: 0.92,
        ..default()
    };
    let sand = materials.add(matte(Color::srgb(0.76, 0.66, 0.49)));
    let plaster = materials.add(matte(Color::srgb(0.84, 0.77, 0.65)));
    let ochre = materials.add(matte(Color::srgb(0.70, 0.50, 0.29)));
    let wood = materials.add(matte(Color::srgb(0.47, 0.36, 0.24)));
    let stone = materials.add(matte(Color::srgb(0.58, 0.55, 0.50)));

    // Ground.
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(2000.0, 2000.0))),
        MeshMaterial3d(sand),
    ));

    // Loose boxes: (size, position, yaw, material).
    let boxes = [
        (Vec3::new(1.2, 1.2, 1.2), Vec3::new(2.5, 0.6, 1.0), 0.3, wood.clone()),
        (Vec3::new(1.0, 0.8, 1.6), Vec3::new(3.6, 0.4, 2.6), -0.2, wood.clone()),
        (Vec3::new(2.0, 3.0, 2.0), Vec3::new(-4.0, 1.5, -3.0), 0.0, stone.clone()),
        (Vec3::new(0.8, 0.8, 0.8), Vec3::new(1.2, 0.4, 3.4), 0.8, ochre.clone()),
        (Vec3::new(4.0, 1.0, 0.6), Vec3::new(-1.5, 0.5, 4.5), 0.0, stone),
    ];
    for (size, pos, yaw, mat) in boxes {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::from_size(size))),
            MeshMaterial3d(mat),
            Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(yaw)),
        ));
    }

    // Small open-fronted shelter: back wall, two side walls and a roof.
    // Shelter interior is 4 m wide x 3 m deep x 2.8 m tall, centred at (6, 0, -4).
    let (cx, cz) = (6.0, -4.0);
    let wall = 0.3;
    let parts = [
        // back wall
        (Vec3::new(4.0 + 2.0 * wall, 2.8, wall), Vec3::new(cx, 1.4, cz - 1.5 - wall / 2.0)),
        // left wall
        (Vec3::new(wall, 2.8, 3.0), Vec3::new(cx - 2.0 - wall / 2.0, 1.4, cz)),
        // right wall
        (Vec3::new(wall, 2.8, 3.0), Vec3::new(cx + 2.0 + wall / 2.0, 1.4, cz)),
        // roof, with a slight overhang at the front
        (
            Vec3::new(4.0 + 2.0 * wall, wall, 3.0 + wall + 0.6),
            Vec3::new(cx, 2.8 + wall / 2.0, cz + 0.3 - wall / 2.0),
        ),
    ];
    for (size, pos) in parts {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::from_size(size))),
            MeshMaterial3d(plaster.clone()),
            Transform::from_translation(pos),
        ));
    }
    // A crate inside the shelter, to judge how dark the interior shade is.
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::from_size(Vec3::splat(0.9)))),
        MeshMaterial3d(ochre),
        Transform::from_xyz(cx + 0.8, 0.45, cz - 0.7),
    ));
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        // Matches the physically based sun above.
        bevy::camera::Exposure::SUNLIGHT,
        Transform::from_xyz(-6.0, 3.2, 12.0).looking_at(Vec3::new(2.0, 1.0, -1.0), Vec3::Y),
        // Dusty haze that fades the distant ground into the sky colour.
        DistanceFog {
            color: SKY,
            directional_light_color: Color::srgba(1.0, 0.93, 0.80, 0.35),
            directional_light_exponent: 20.0,
            falloff: FogFalloff::from_visibility(900.0),
        },
    ));
}
