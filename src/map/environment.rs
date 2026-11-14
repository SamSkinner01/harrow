//! Desert environment: hard sun, pale sky, dusty haze and endless sand.
//!
//! Lighting is physically based: the sun uses real-world lux and every 3D
//! camera gets a matching sunlight exposure, so values carry over to the
//! real map.

use avian3d::prelude::*;
use bevy::{
    asset::RenderAssetUsages,
    camera::Exposure,
    curve::cubic_splines::LinearSpline,
    light::CascadeShadowConfigBuilder,
    mesh::Indices,
    post_process::auto_exposure::{AutoExposure, AutoExposureCompensationCurve, AutoExposurePlugin},
    prelude::*,
    render::render_resource::PrimitiveTopology,
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

/// Rectangles (world x/z) where the desert floor is left out, e.g. over a
/// basement or a sunken stair. Insert before `Startup`; the floor's visible
/// surface and its collider both skip them.
#[derive(Resource, Clone, Default, Debug)]
pub struct DesertHoles(pub Vec<Rect>);

fn spawn_desert_floor(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    library: Res<MaterialLibrary>,
    holes: Option<Res<DesertHoles>>,
) {
    const HALF: f32 = 1000.0;
    let holes = holes.map(|h| h.0.clone()).unwrap_or_default();
    let material = library.get("sand").expect("sand material");
    for rect in desert_pieces(HALF, &holes) {
        let (min, max) = (rect.min, rect.max);
        // UVs in metres from the world origin, so dunes and other sand
        // pieces (kit UVs are world metres too) line up with the floor.
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(
                Mesh::ATTRIBUTE_POSITION,
                vec![[min.x, 0.0, min.y], [max.x, 0.0, min.y], [max.x, 0.0, max.y], [min.x, 0.0, max.y]],
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; 4])
            .with_inserted_attribute(
                Mesh::ATTRIBUTE_UV_0,
                vec![[min.x, min.y], [max.x, min.y], [max.x, max.y], [min.x, max.y]],
            )
            .with_inserted_indices(Indices::U32(vec![0, 2, 1, 0, 3, 2]));
        mesh.generate_tangents().ok();
        commands.spawn((
            Name::new("Desert floor"),
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material.clone()),
            Transform::from_xyz(0.0, DESERT_FLOOR_Y, 0.0),
        ));
        // Collision: a thick slab whose top is the desert floor.
        let size = max - min;
        let centre = (min + max) / 2.0;
        commands.spawn((
            RigidBody::Static,
            Collider::cuboid(size.x, 1.0, size.y),
            Transform::from_xyz(centre.x, DESERT_FLOOR_Y - 0.5, centre.y),
        ));
    }
}

/// Split the square floor (-half..half) into rectangles that cover it
/// except for `holes`: a grid on every hole edge, empty cells dropped and
/// neighbouring cells in a row merged.
fn desert_pieces(half: f32, holes: &[Rect]) -> Vec<Rect> {
    let mut xs = vec![-half, half];
    let mut zs = vec![-half, half];
    for hole in holes {
        xs.extend([hole.min.x, hole.max.x]);
        zs.extend([hole.min.y, hole.max.y]);
    }
    for v in [&mut xs, &mut zs] {
        v.retain(|c| c.abs() <= half);
        v.sort_by(f32::total_cmp);
        v.dedup();
    }
    let mut pieces = Vec::new();
    for z in zs.windows(2) {
        let mut run: Option<Rect> = None;
        for x in xs.windows(2) {
            let cell = Rect::new(x[0], z[0], x[1], z[1]);
            let empty = holes.iter().any(|h| h.contains(cell.center()));
            match (&mut run, empty) {
                (Some(r), false) => r.max.x = cell.max.x,
                (None, false) => run = Some(cell),
                (Some(_), true) => pieces.extend(run.take()),
                (None, true) => {}
            }
        }
        pieces.extend(run);
    }
    pieces
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
