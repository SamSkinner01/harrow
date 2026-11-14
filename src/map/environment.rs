//! Desert environment: hard sun, pale sky, dusty haze and endless sand.
//!
//! Lighting is physically based: the sun uses real-world lux and every 3D
//! camera gets a matching sunlight exposure, so values carry over to the
//! real map.

use avian3d::prelude::*;
use bevy::{
    camera::Exposure,
    curve::cubic_splines::LinearSpline,
    light::CascadeShadowConfigBuilder,
    mesh::VertexAttributeValues,
    post_process::auto_exposure::{AutoExposure, AutoExposureCompensationCurve, AutoExposurePlugin},
    prelude::*,
};

use super::materials::MaterialLibrary;
use crate::config::Tuning;

/// Pale, slightly warm desert sky.
const SKY: Color = Color::srgb(0.66, 0.74, 0.82);

/// Top of the open-desert ground. Slightly below 0 so play areas built at
/// y = 0 sit on top of it without z-fighting.
pub const DESERT_FLOOR_Y: f32 = -0.02;

pub struct EnvironmentPlugin;

impl Plugin for EnvironmentPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(AutoExposurePlugin)
            .insert_resource(ClearColor(SKY))
            .insert_resource(GlobalAmbientLight {
                // Sky fill (bluish). Brightness comes from the tuning file.
                color: Color::srgb(0.75, 0.82, 1.0),
                ..default()
            })
            .add_systems(Startup, (spawn_sun, spawn_desert_floor))
            .add_systems(
                PreUpdate,
                apply_lighting_tuning.run_if(resource_changed::<Tuning>),
            )
            .add_observer(configure_camera);
    }
}

fn spawn_sun(mut commands: Commands) {
    commands.spawn((
        Name::new("Sun"),
        DirectionalLight {
            color: Color::srgb(1.0, 0.95, 0.86),
            illuminance: light_consts::lux::DIRECT_SUNLIGHT, // see tuning
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
    library: Res<MaterialLibrary>,
) {
    const SIZE: f32 = 2000.0;
    let mut mesh = Plane3d::default().mesh().size(SIZE, SIZE).build();
    // Plane UVs run 0..1 across the whole plane; the material library expects
    // metres.
    if let Some(VertexAttributeValues::Float32x2(uvs)) = mesh.attribute_mut(Mesh::ATTRIBUTE_UV_0) {
        for uv in uvs {
            uv[0] *= SIZE;
            uv[1] *= SIZE;
        }
    }
    mesh.generate_tangents().ok();
    commands.spawn((
        Name::new("Desert floor"),
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(library.get("sand").expect("sand material")),
        Transform::from_xyz(0.0, DESERT_FLOOR_Y, 0.0),
    ));
    // Collision: a thick slab whose top is the desert floor.
    commands.spawn((
        RigidBody::Static,
        Collider::cuboid(SIZE, 1.0, SIZE),
        Transform::from_xyz(0.0, DESERT_FLOOR_Y - 0.5, 0.0),
    ));
}

/// Sun, ambient, exposure and eye adaptation from the `lighting` section of
/// the tuning file.
fn apply_lighting_tuning(
    tuning: Res<Tuning>,
    mut suns: Query<&mut DirectionalLight>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut cameras: Query<(&mut Exposure, Option<&mut AutoExposure>)>,
    mut curves: ResMut<Assets<AutoExposureCompensationCurve>>,
) {
    let l = &tuning.lighting;
    for mut sun in &mut suns {
        sun.illuminance = l.sun;
    }
    ambient.brightness = l.ambient;
    let curve = curves.add(adaptation_curve(l.adaptation_threshold, l.adaptation_max));
    for (mut exposure, auto) in &mut cameras {
        exposure.ev100 = l.exposure;
        if let Some(mut auto) = auto {
            auto.compensation_curve = curve.clone();
        }
    }
}

/// Eye adaptation that only ever brightens, and by at most `max_boost`
/// stops. Bevy's auto exposure targets `curve(avg) - avg` stops of
/// correction on top of the base exposure (avg = average log2 luminance), so:
/// at or above `threshold` the curve is y = x (no change); just below it the
/// curve is flat (brighten towards the threshold); far below it the curve is
/// y = x + max_boost (capped boost).
fn adaptation_curve(threshold: f32, max_boost: f32) -> AutoExposureCompensationCurve {
    let boost = max_boost.max(0.01);
    let (lo, hi) = (-16.0, 8.0);
    let points = [
        Vec2::new(lo, lo + boost),
        Vec2::new(threshold - boost, threshold),
        Vec2::new(threshold, threshold),
        Vec2::new(hi, hi),
    ];
    AutoExposureCompensationCurve::from_curve(LinearSpline::new(points))
        .expect("adaptation curve is monotonic")
}

/// Every 3D camera gets sunlight exposure, eye adaptation and the dusty haze.
fn configure_camera(
    add: On<Add<Camera3d>>,
    mut commands: Commands,
    tuning: Res<Tuning>,
    mut curves: ResMut<Assets<AutoExposureCompensationCurve>>,
) {
    let l = &tuning.lighting;
    commands.entity(add.entity).insert((
        Exposure { ev100: l.exposure },
        AutoExposure {
            range: -16.0..=8.0,
            speed_brighten: 1.5,
            speed_darken: 2.5,
            compensation_curve: curves
                .add(adaptation_curve(l.adaptation_threshold, l.adaptation_max)),
            ..default()
        },
        DistanceFog {
            color: SKY,
            directional_light_color: Color::srgba(1.0, 0.93, 0.80, 0.35),
            directional_light_exponent: 20.0,
            falloff: FogFalloff::from_visibility(900.0),
        },
    ));
}
