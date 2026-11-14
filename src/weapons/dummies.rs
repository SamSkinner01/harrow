//! Training dummies (`--dummies`; always on with `--weapon-test`).
//!
//! Static standing targets with [`Health`], `Team::Defenders` and the same
//! head/torso/legs hit zones as a standing player (sizes from
//! `HitZone::shape`, placement matching `player/hitbox.rs` at full height,
//! no lean). Placed for Harrow Keep: three in the courtyard, one in the
//! storeroom behind the ground-floor plaster wall (z = -4) and one in the
//! west hall behind the stone wall at x = 6.
//!
//! Feedback: the dummy flashes red when hit, a health bar floats above it,
//! and it topples over at zero health, standing back up after
//! `dummies.respawn_time` seconds (fixed tick) with full health.

use std::f32::consts::FRAC_PI_2;

use avian3d::prelude::*;
use bevy::{light::NotShadowCaster, prelude::*};

use super::BulletHit;
use crate::{
    combat::{Dead, Health, Team},
    config::Tuning,
    physics::GameLayer,
    player::{HitZone, LocalPlayer},
};

pub struct DummiesPlugin;

impl Plugin for DummiesPlugin {
    fn build(&self, app: &mut App) {
        if !dummies_requested() {
            return;
        }
        app.add_systems(Startup, spawn_dummies)
            .add_systems(FixedUpdate, (knock_down, stand_up).chain())
            .add_systems(Update, (animate_dummies, update_health_bars));
    }
}

/// `--dummies` or `--weapon-test` on the command line.
pub fn dummies_requested() -> bool {
    std::env::args().any(|a| a == "--dummies" || a == "--weapon-test")
}

/// A training target.
#[derive(Component, Debug)]
pub struct TrainingDummy {
    pub label: &'static str,
    /// Seconds spent down (fixed tick), while dead.
    pub down_for: f32,
}

/// Visual state, animated every frame.
#[derive(Component)]
struct DummyLook {
    pivot: Entity,
    material: Handle<StandardMaterial>,
    flash: f32,
    fall: f32,
}

#[derive(Component)]
struct HealthBar {
    dummy: Entity,
    fill: Entity,
    material: Handle<StandardMaterial>,
}

/// Name, feet position, facing yaw (degrees; 0 faces -Z).
const PLACES: &[(&str, Vec3, f32)] = &[
    ("courtyard A", Vec3::new(8.0, 0.0, 4.0), 180.0),
    ("courtyard B", Vec3::new(12.0, 0.0, 2.5), 200.0),
    ("courtyard C", Vec3::new(3.5, 0.0, 3.0), 160.0),
    // Storeroom, behind the ground-floor plaster wall (z = -4, x 6..8).
    ("behind plaster", Vec3::new(7.0, 0.0, -5.8), 180.0),
    // West hall, behind the stone wall at x = 6 (z 0..-2).
    ("behind stone", Vec3::new(4.3, 0.0, -1.0), -90.0),
];

const BAR_WIDTH: f32 = 0.5;
const BAR_HEIGHT: f32 = 0.06;

fn spawn_dummies(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    tuning: Res<Tuning>,
) {
    let height = tuning.body.stand_height;
    let canvas = Color::srgb(0.62, 0.52, 0.36);
    let base_mesh = meshes.add(Cylinder::new(0.3, 0.05));
    let visor_mesh = meshes.add(Cuboid::new(0.16, 0.05, 0.04));
    let bar_mesh = meshes.add(Rectangle::new(BAR_WIDTH, BAR_HEIGHT));
    let bar_back = materials.add(StandardMaterial {
        base_color: Color::srgba(0.0, 0.0, 0.0, 0.7),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    let dark = materials.add(StandardMaterial {
        base_color: Color::srgb(0.08, 0.08, 0.08),
        perceptual_roughness: 0.6,
        ..default()
    });

    for &(label, feet, yaw) in PLACES {
        let material = materials.add(StandardMaterial {
            base_color: canvas,
            perceptual_roughness: 0.95,
            ..default()
        });
        let fill_material = materials.add(StandardMaterial {
            base_color: Color::srgb(0.2, 0.9, 0.2),
            unlit: true,
            ..default()
        });

        let dummy = commands
            .spawn((
                Name::new(format!("Training dummy ({label})")),
                TrainingDummy {
                    label,
                    down_for: 0.0,
                },
                Health::new(tuning.weapons.dummies.health),
                Team::Defenders,
                Transform::from_translation(feet).with_rotation(Quat::from_rotation_y(yaw.to_radians())),
                Visibility::default(),
                RigidBody::Static,
            ))
            .id();

        // Hit zones: same shapes and standing placement as a player's.
        for (zone, y) in [
            (HitZone::Head, height - 0.14),
            (HitZone::Torso, height - 0.63),
            (HitZone::Legs, 0.4),
        ] {
            let (radius, length) = zone.shape();
            commands.spawn((
                Name::new(format!("{zone:?} hit zone")),
                zone,
                Collider::capsule(radius, length),
                Sensor,
                CollisionLayers::new(GameLayer::Hitbox, LayerMask::NONE),
                Transform::from_xyz(0.0, y, 0.0),
                ChildOf(dummy),
            ));
        }

        // Visible body on a pivot at the feet, so it can topple.
        let pivot = commands
            .spawn((Transform::default(), Visibility::default(), ChildOf(dummy)))
            .id();
        let parts: [(Handle<Mesh>, Handle<StandardMaterial>, Transform); 5] = [
            (
                meshes.add(Capsule3d::new(0.15, 0.5)),
                material.clone(),
                Transform::from_xyz(0.0, 0.4, 0.0),
            ),
            (
                meshes.add(Capsule3d::new(0.2, 0.3)),
                material.clone(),
                Transform::from_xyz(0.0, height - 0.63, 0.0),
            ),
            (
                meshes.add(Sphere::new(0.12)),
                material.clone(),
                Transform::from_xyz(0.0, height - 0.14, 0.0),
            ),
            (
                visor_mesh.clone(),
                dark.clone(),
                Transform::from_xyz(0.0, height - 0.12, -0.105),
            ),
            (base_mesh.clone(), dark.clone(), Transform::from_xyz(0.0, 0.025, 0.0)),
        ];
        for (mesh, mat, transform) in parts {
            commands.spawn((Mesh3d(mesh), MeshMaterial3d(mat), transform, ChildOf(pivot)));
        }

        // Health bar above the head (turned to face the player each frame).
        let bar = commands
            .spawn((
                Transform::from_xyz(0.0, height + 0.22, 0.0),
                Visibility::default(),
                ChildOf(dummy),
            ))
            .id();
        commands.spawn((
            Mesh3d(bar_mesh.clone()),
            MeshMaterial3d(bar_back.clone()),
            Transform::from_xyz(0.0, 0.0, -0.002),
            NotShadowCaster,
            ChildOf(bar),
        ));
        let fill = commands
            .spawn((
                Mesh3d(bar_mesh.clone()),
                MeshMaterial3d(fill_material.clone()),
                Transform::default(),
                NotShadowCaster,
                ChildOf(bar),
            ))
            .id();
        commands.entity(bar).insert(HealthBar {
            dummy,
            fill,
            material: fill_material,
        });
        commands.entity(dummy).insert(DummyLook {
            pivot,
            material,
            flash: 0.0,
            fall: 0.0,
        });
    }
    info!("spawned {} training dummies", PLACES.len());
}

/// Fixed tick: a dummy that just died stops being shootable.
fn knock_down(
    dummies: Query<(&TrainingDummy, &Children), Added<Dead>>,
    zones: Query<(), With<HitZone>>,
    mut commands: Commands,
) {
    for (dummy, children) in &dummies {
        info!("dummy '{}' is down", dummy.label);
        for child in children.iter().filter(|c| zones.contains(*c)) {
            commands.entity(child).insert(ColliderDisabled);
        }
    }
}

/// Fixed tick: after `respawn_time` down, stand up with full health.
fn stand_up(
    mut dummies: Query<(Entity, &mut TrainingDummy, &mut Health, &Children), With<Dead>>,
    zones: Query<(), With<HitZone>>,
    tuning: Res<Tuning>,
    time: Res<Time>,
    mut commands: Commands,
) {
    for (entity, mut dummy, mut health, children) in &mut dummies {
        dummy.down_for += time.delta_secs();
        if dummy.down_for < tuning.weapons.dummies.respawn_time {
            continue;
        }
        dummy.down_for = 0.0;
        health.max = tuning.weapons.dummies.health;
        health.current = health.max;
        commands.entity(entity).remove::<Dead>();
        for child in children.iter().filter(|c| zones.contains(*c)) {
            commands.entity(child).remove::<ColliderDisabled>();
        }
        info!("dummy '{}' is back up", dummy.label);
    }
}

/// Every frame: red flash on hits, topple when dead.
fn animate_dummies(
    mut dummies: Query<(Entity, &mut DummyLook, Has<Dead>)>,
    mut pivots: Query<&mut Transform>,
    mut hits: MessageReader<BulletHit>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    for hit in hits.read() {
        if let Ok((_, mut look, _)) = dummies.get_mut(hit.target) {
            look.flash = 1.0;
        }
    }
    for (_, mut look, dead) in &mut dummies {
        let target = if dead { 1.0 } else { 0.0 };
        let speed = if dead { 4.0 } else { 2.0 };
        let fall = look.fall.move_towards(target, speed * dt);
        if fall != look.fall {
            look.fall = fall;
            if let Ok(mut pivot) = pivots.get_mut(look.pivot) {
                // Fall backwards (away from the face, which looks down -Z).
                let eased = fall * fall;
                pivot.rotation = Quat::from_rotation_x(eased * FRAC_PI_2 * 0.95);
            }
        }
        if look.flash > 0.0 {
            look.flash = (look.flash - dt / 0.18).max(0.0);
            if let Some(mut material) = materials.get_mut(&look.material) {
                // Alpha 0: emissive not scaled by camera exposure, so it reads
                // the same in sun and shade.
                let f = look.flash;
                material.emissive = LinearRgba::new(0.9 * f, 0.05 * f, 0.02 * f, 0.0);
            }
        }
    }
}

/// Health bars: length and colour from health, turned to face the player.
fn update_health_bars(
    mut bars: Query<(&HealthBar, &mut Transform, &GlobalTransform)>,
    dummies: Query<(&Health, &GlobalTransform), With<TrainingDummy>>,
    mut fills: Query<&mut Transform, (Without<HealthBar>, Without<LocalPlayer>)>,
    player: Option<Single<&Transform, (With<LocalPlayer>, Without<HealthBar>)>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(player) = player else { return };
    for (bar, mut transform, global) in &mut bars {
        let Ok((health, dummy_global)) = dummies.get(bar.dummy) else {
            continue;
        };
        // Face the player: world yaw towards them, minus the dummy's own yaw.
        let to_player = (player.translation - global.translation()).with_y(0.0);
        if to_player.length_squared() > 1e-4 {
            let world_yaw = to_player.x.atan2(to_player.z);
            let (dummy_yaw, _, _) = dummy_global.rotation().to_euler(EulerRot::YXZ);
            transform.rotation = Quat::from_rotation_y(world_yaw - dummy_yaw);
        }
        let fraction = (health.current / health.max.max(1.0)).clamp(0.0, 1.0);
        if let Ok(mut fill) = fills.get_mut(bar.fill) {
            fill.scale = Vec3::new(fraction.max(1e-3), 1.0, 1.0);
            fill.translation.x = -BAR_WIDTH * (1.0 - fraction) / 2.0;
        }
        if let Some(mut material) = materials.get_mut(&bar.material) {
            let colour = Color::srgb(0.9, 0.15, 0.1).mix(&Color::srgb(0.2, 0.9, 0.2), fraction);
            if material.base_color != colour {
                material.base_color = colour;
            }
        }
    }
}
