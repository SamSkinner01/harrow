//! Placed defender secondaries: razor wire, ballistic cover and the
//! proximity alarm (hold 4 for the placing time, standing still).
//!
//! - **Razor wire**: a concertina roll laid across the floor ahead of you
//!   (in a doorway). Anyone inside it moves at `slow` speed
//!   ([`GadgetSpeedScale`]) and every footstep in it rattles loudly
//!   ([`Noise`]). Bullets pass through it; explosions and melee break it.
//! - **Ballistic cover**: a waist-high folding steel shield set down ahead
//!   of you, facing where you look. Solid world geometry
//!   ([`SurfaceKind::Hard`]): it stops bullets and blasts, and blocks
//!   movement. Only explosions wear it down.
//! - **Proximity alarm**: a small box stuck to any surface. While an
//!   attacker is within `radius` it beeps (everyone nearby hears it) and
//!   reveals them to defenders every `ping_interval`. Can be shot.
//!
//! Movement reads [`GadgetSpeedScale`] (`player/movement.rs`). It is set
//! after every tick by `slow_in_wire` on every machine that simulates the
//! character (the server for everyone, a client for its own predicted
//! character), so clients predict the slow-down instead of being corrected.

use avian3d::prelude::*;
use bevy::{ecs::system::SystemParam, mesh::TorusMeshBuilder, prelude::*};

use super::{
    DeployedGadget, GadgetAssets, GadgetEvent, GadgetOwner, GadgetStage, Reveal, RevealSource,
    RoundScoped, SecondaryGadget, eye_and_look,
    tuning::GadgetTuning,
};
use crate::{
    combat::{DamageKind, DamageMessage, Dead, Health, Noise, NoiseKind, SurfaceKind, Team},
    config::Tuning,
    destruction::{Explosion, MeleeSwing},
    physics::GameLayer,
    player::{CharacterState, Footstep, Player, PlayerInput},
    round::other_team,
};

pub struct DeployablesPlugin;

impl Plugin for DeployablesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedUpdate,
            (
                (wire_noise, cut_wire_with_melee, damage_covers, alarms_detect)
                    .in_set(GadgetStage::Effects),
            ),
        )
        // Not part of `GadgetSystems`: clients run it for their predicted
        // character (the network orders it before prediction history).
        .add_systems(FixedPostUpdate, slow_in_wire.in_set(GadgetSpeedSystems))
        .add_systems(Update, dress_deployables);
    }
}

/// Movement speed multiplier from gadgets (razor wire); 1.0 = normal.
/// Simulation state, set after every tick by `slow_in_wire` and read by
/// movement the next tick (it multiplies with weapons' `MoveSpeedScale`;
/// no sprinting while it is below 1). Replicated and predicted.
#[derive(Component, Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GadgetSpeedScale(pub f32);

/// Where [`GadgetSpeedScale`] is updated (`FixedPostUpdate`).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct GadgetSpeedSystems;

#[derive(Component, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RazorWire {
    /// Half size: x along the roll, y up, z across.
    pub half: Vec3,
}

#[derive(Component, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct BallisticCover {
    /// Blast damage left before it breaks.
    pub health: f32,
}

#[derive(Component, Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ProximityAlarm {
    /// Seconds until it may beep again.
    pub cooldown: f32,
    /// Someone was in range last tick.
    pub triggered: bool,
}

/// Spatial access for placing things.
#[derive(SystemParam)]
pub struct PlaceTarget<'w, 's> {
    spatial: SpatialQuery<'w, 's>,
}

pub fn place_time(kind: SecondaryGadget, g: &GadgetTuning) -> f32 {
    match kind {
        SecondaryGadget::RazorWire => g.razor_wire.place_time,
        SecondaryGadget::BallisticCover => g.cover.place_time,
        SecondaryGadget::ProximityAlarm => g.alarm.place_time,
        _ => 1.0,
    }
}

pub fn place_hint(kind: SecondaryGadget) -> &'static str {
    match kind {
        SecondaryGadget::ProximityAlarm => "aim at a wall, floor or ceiling close by",
        _ => "needs open floor ahead of you",
    }
}

/// Where a `kind` would go for this character: floor ahead (wire, cover,
/// facing the character's way) or the surface aimed at (alarm, +Z out).
pub fn place_target(
    kind: SecondaryGadget,
    transform: &Transform,
    state: &CharacterState,
    input: &PlayerInput,
    tuning: &Tuning,
    targets: &PlaceTarget,
) -> Option<Transform> {
    let g = &tuning.round.gadgets;
    let world = SpatialQueryFilter::from_mask(GameLayer::World);
    let spatial = &targets.spatial;
    let yaw = Quat::from_rotation_y(input.yaw);
    let forward = yaw * Vec3::NEG_Z;
    match kind {
        SecondaryGadget::ProximityAlarm => {
            let (eye, look) = eye_and_look(transform, state, input, &tuning.body, tuning.lean.offset);
            let hit = spatial.cast_ray(eye, look, g.alarm.reach, true, &world)?;
            let point = eye + *look * hit.distance;
            let normal = hit.normal.normalize_or(-*look);
            let up = if normal.y.abs() > 0.9 { forward } else { Vec3::Y };
            Some(Transform::from_translation(point + normal * 0.025).looking_to(-normal, up))
        }
        SecondaryGadget::RazorWire | SecondaryGadget::BallisticCover => {
            let distance = match kind {
                SecondaryGadget::RazorWire => g.razor_wire.distance,
                _ => g.cover.distance,
            };
            let feet = transform.translation;
            let ahead = feet + forward * distance;
            // Nothing in the way between you and the spot, at knee height.
            let knee = feet + Vec3::Y * 0.4;
            if let Ok(dir) = Dir3::new(ahead - feet)
                && spatial.cast_ray(knee, dir, distance, true, &world).is_some()
            {
                return None;
            }
            let above = ahead + Vec3::Y * 0.6;
            let hit = spatial.cast_ray(above, Dir3::NEG_Y, 1.2, true, &world)?;
            if hit.normal.y < 0.75 {
                return None;
            }
            let floor = above - Vec3::Y * hit.distance;
            // The middle must be clear (a doorway is fine: wire and cover
            // ends may tuck into the jambs).
            let middle = Collider::sphere(0.12);
            if !spatial.shape_intersections(&middle, floor + Vec3::Y * 0.35, Quat::IDENTITY, &world).is_empty() {
                return None;
            }
            Some(Transform::from_translation(floor).with_rotation(yaw))
        }
        _ => None,
    }
}

/// Put a deployable down at `at` (from [`place_target`]).
pub fn spawn_deployable(
    commands: &mut Commands,
    kind: SecondaryGadget,
    owner: Entity,
    team: Team,
    at: Transform,
    tuning: &Tuning,
) -> Entity {
    let g = &tuning.round.gadgets;
    let owner_info = GadgetOwner {
        entity: Some(owner),
        team,
    };
    match kind {
        SecondaryGadget::RazorWire => {
            let w = &g.razor_wire;
            commands
                .spawn((
                    Name::new("Razor wire"),
                    RazorWire {
                        half: Vec3::new(w.length / 2.0, w.height / 2.0, w.depth / 2.0),
                    },
                    owner_info,
                    DeployedGadget,
                    RoundScoped,
                    // No collider: bullets pass through; explosions (by
                    // distance) and melee (here) break it.
                    Health::new(w.health),
                    at,
                    Visibility::default(),
                    children![super::blast_anchor(Vec3::Y * w.height / 2.0)],
                ))
                .id()
        }
        SecondaryGadget::BallisticCover => commands
            .spawn((
                BallisticCover { health: g.cover.health },
                owner_info,
                RoundScoped,
                at,
                Visibility::default(),
                cover_body(tuning),
            ))
            .id(),
        _ => commands
            .spawn((
                Name::new("Proximity alarm"),
                ProximityAlarm::default(),
                owner_info,
                DeployedGadget,
                RoundScoped,
                Health::new(g.alarm.health),
                at,
                Visibility::default(),
                Collider::cuboid(0.1, 0.1, 0.06),
                Sensor,
                CollisionLayers::new(GameLayer::Hitbox, LayerMask::NONE),
                children![super::blast_anchor(Vec3::Z * 0.03)],
            ))
            .id(),
    }
}

/// Ballistic cover as world geometry: its name (destruction keys surfaces
/// by name and place), a static body and the shield's collider. Clients
/// add the same to replicated covers (`net/gadget_sync.rs`), so their
/// predicted movement collides with it and bullet impacts find it.
pub(crate) fn cover_body(tuning: &Tuning) -> impl Bundle {
    let c = &tuning.round.gadgets.cover;
    let (half_w, h) = (c.width / 2.0, c.height);
    let wing = half_w * 0.36;
    let centre_half = half_w - wing;
    let wing_angle = 0.45_f32;
    let wing_pos = |s: f32| {
        Vec3::new(s * (centre_half + wing * wing_angle.cos() / 2.0), h / 2.0, wing * wing_angle.sin() / 2.0)
    };
    let parts = vec![
        (Vec3::new(0.0, h / 2.0, 0.0), Quat::IDENTITY, Collider::cuboid(centre_half * 2.0, h, 0.03)),
        (wing_pos(1.0), Quat::from_rotation_y(-wing_angle), Collider::cuboid(wing, h, 0.03)),
        (wing_pos(-1.0), Quat::from_rotation_y(wing_angle), Collider::cuboid(wing, h, 0.03)),
    ];
    (
        Name::new("Ballistic cover"),
        RigidBody::Static,
        Collider::compound(parts),
        SurfaceKind::Hard,
    )
}

/// Inside a wire roll's box (character feet / body).
fn in_wire(wire: &RazorWire, at: &GlobalTransform, point: Vec3, margin: f32) -> bool {
    let local = at.affine().inverse().transform_point3(point) - Vec3::Y * wire.half.y;
    local.x.abs() <= wire.half.x + margin && local.z.abs() <= wire.half.z + margin && local.y.abs() <= wire.half.y + 0.6
}

/// After every tick: everyone standing in wire is slowed next tick. Runs
/// wherever the character is simulated (server; a client for its own
/// predicted character, also during rollback replays), from replicated
/// wire placements, so prediction agrees with the server. Uses `Transform`
/// (not `GlobalTransform`, which lags a frame for a wire placed this tick).
#[allow(clippy::type_complexity)]
fn slow_in_wire(
    tuning: Res<Tuning>,
    wires: Query<(&RazorWire, &Transform), Without<Dead>>,
    mut people: Query<(&Transform, &mut GadgetSpeedScale), (With<Player>, With<PlayerInput>, Without<RazorWire>)>,
) {
    let slow = tuning.round.gadgets.razor_wire.slow;
    let radius = tuning.body.radius;
    for (feet, mut scale) in &mut people {
        let inside = wires
            .iter()
            .any(|(w, at)| in_wire(w, &GlobalTransform::from(*at), feet.translation, radius));
        let wanted = if inside { slow } else { 1.0 };
        if scale.0 != wanted {
            scale.0 = wanted;
        }
    }
}

/// Footsteps in wire are loud.
fn wire_noise(
    tuning: Res<Tuning>,
    wires: Query<(&RazorWire, &GlobalTransform), Without<Dead>>,
    mut footsteps: MessageReader<Footstep>,
    mut noise: MessageWriter<Noise>,
    mut events: MessageWriter<GadgetEvent>,
) {
    let t = &tuning.round.gadgets.razor_wire;
    let radius = tuning.body.radius;
    for step in footsteps.read() {
        if let Some((_, at)) = wires.iter().find(|(w, at)| in_wire(w, at, step.position, radius)) {
            noise.write(Noise {
                source: Some(step.entity),
                position: step.position,
                loudness: t.noise,
                kind: NoiseKind::Other,
            });
            events.write(GadgetEvent::WireRattle {
                by: step.entity,
                position: at.translation(),
            });
        }
    }
}

/// Melee swings cut wire: the swing reaches from the eyes along the look.
fn cut_wire_with_melee(
    tuning: Res<Tuning>,
    mut swings: MessageReader<MeleeSwing>,
    attackers: Query<(&Transform, &CharacterState, &PlayerInput)>,
    wires: Query<(Entity, &RazorWire, &GlobalTransform), Without<Dead>>,
    mut damage: MessageWriter<DamageMessage>,
) {
    let range = tuning.destruction.melee.range;
    for swing in swings.read() {
        let Ok((transform, state, input)) = attackers.get(swing.attacker) else {
            continue;
        };
        let (eye, look) = eye_and_look(transform, state, input, &tuning.body, tuning.lean.offset);
        for (wire, w, at) in &wires {
            // Sample along the swing.
            let hit = (0..=8).any(|i| in_wire(w, at, eye + *look * (range * i as f32 / 8.0), 0.15));
            if hit {
                damage.write(DamageMessage {
                    target: wire,
                    amount: tuning.round.gadgets.razor_wire.melee_damage,
                    kind: DamageKind::Melee,
                    zone: None,
                    source: Some(swing.attacker),
                });
                break;
            }
        }
    }
}

/// Explosions wear ballistic cover down (it shields the blast's own line
/// of sight, so it is handled here rather than by destruction's damage).
fn damage_covers(
    mut explosions: MessageReader<Explosion>,
    mut covers: Query<(Entity, &mut BallisticCover, &GlobalTransform)>,
    spatial: SpatialQuery,
    tuning: Res<Tuning>,
    mut events: MessageWriter<GadgetEvent>,
    mut commands: Commands,
) {
    let full = tuning.destruction.explosion.full_damage_fraction;
    let world = SpatialQueryFilter::from_mask(GameLayer::World);
    for blast in explosions.read() {
        if blast.max_damage <= 0.0 || blast.damage_radius <= 0.0 {
            continue;
        }
        for (entity, mut cover, at) in &mut covers {
            let centre = at.translation() + Vec3::Y * 0.5;
            let offset = centre - blast.centre;
            let distance = (offset.length() - 0.4).max(0.0);
            if distance > blast.damage_radius {
                continue;
            }
            // Walls in between (not the cover itself) stop it.
            let filter = world.clone().with_excluded_entities([entity]);
            if let Ok(dir) = Dir3::new(offset)
                && spatial.cast_ray(blast.centre, dir, offset.length(), true, &filter).is_some()
            {
                continue;
            }
            let x = distance / blast.damage_radius;
            let falloff = if x <= full { 1.0 } else { (1.0 - (x - full) / (1.0 - full)).powi(2) };
            cover.health -= blast.max_damage * falloff;
            if cover.health <= 0.0 {
                events.write(GadgetEvent::Destroyed {
                    entity,
                    position: at.translation(),
                });
                commands.entity(entity).despawn();
            }
        }
    }
}

/// Attackers near an alarm are revealed to defenders; it beeps.
#[allow(clippy::type_complexity)]
fn alarms_detect(
    tuning: Res<Tuning>,
    time: Res<Time>,
    mut alarms: Query<(Entity, &mut ProximityAlarm, &GlobalTransform, &GadgetOwner), Without<Dead>>,
    people: Query<(Entity, &Transform, &Team), (With<Player>, Without<Dead>)>,
    mut reveals: MessageWriter<Reveal>,
    mut noise: MessageWriter<Noise>,
    mut events: MessageWriter<GadgetEvent>,
) {
    let t = &tuning.round.gadgets.alarm;
    for (entity, mut alarm, at, owner) in &mut alarms {
        alarm.cooldown = (alarm.cooldown - time.delta_secs()).max(0.0);
        let p = at.translation();
        let intruders: Vec<(Entity, Vec3)> = people
            .iter()
            .filter(|(_, _, team)| **team == other_team(owner.team))
            .filter_map(|(e, feet, _)| {
                let body = feet.translation + Vec3::Y * 0.9;
                let d = body - p;
                (d.with_y(0.0).length() <= t.radius && d.y.abs() <= t.vertical_reach).then_some((e, body))
            })
            .collect();
        alarm.triggered = !intruders.is_empty();
        if !alarm.triggered || alarm.cooldown > 0.0 {
            continue;
        }
        alarm.cooldown = t.ping_interval;
        for (intruder, body) in &intruders {
            reveals.write(Reveal {
                for_team: owner.team,
                position: *body,
                source: RevealSource::ProximityAlarm,
                revealed: Some(*intruder),
            });
        }
        noise.write(Noise {
            source: None,
            position: p,
            loudness: 0.8,
            kind: NoiseKind::Gadget,
        });
        events.write(GadgetEvent::AlarmTriggered { alarm: entity, position: p });
    }
}

// ---- Models -----------------------------------------------------------------

#[derive(Resource)]
struct DeployableLook {
    loop_mesh: Handle<Mesh>,
    wire: Handle<StandardMaterial>,
    lens_glow: Handle<StandardMaterial>,
}

#[allow(clippy::too_many_arguments)]
fn dress_deployables(
    wires: Query<(Entity, &RazorWire), Added<RazorWire>>,
    covers: Query<Entity, Added<BallisticCover>>,
    alarms: Query<Entity, Added<ProximityAlarm>>,
    assets: Res<GadgetAssets>,
    tuning: Res<Tuning>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut look: Local<Option<DeployableLook>>,
    mut commands: Commands,
) {
    if wires.is_empty() && covers.is_empty() && alarms.is_empty() {
        return;
    }
    let look = look.get_or_insert_with(|| DeployableLook {
        loop_mesh: meshes.add(TorusMeshBuilder::new(0.262, 0.274).major_resolution(20).minor_resolution(4)),
        wire: materials.add(StandardMaterial {
            base_color: Color::srgb(0.62, 0.62, 0.6),
            metallic: 0.9,
            perceptual_roughness: 0.35,
            ..default()
        }),
        lens_glow: materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.1, 0.05),
            emissive: LinearRgba::rgb(4.0, 0.2, 0.1),
            ..default()
        }),
    });
    let a = &*assets;
    for (entity, wire) in &wires {
        let length = wire.half.x * 2.0;
        let loops = (length / 0.11).round() as i32;
        let scale_y = wire.half.y * 2.0 / 0.54;
        let scale_z = wire.half.z * 2.0 / 0.54;
        commands.entity(entity).with_children(|c| {
            for i in 0..loops {
                let x = -wire.half.x + (i as f32 + 0.5) * length / loops as f32;
                let tilt = if i % 2 == 0 { 0.32 } else { -0.32 };
                // Torus axis Y -> along the roll (X), leaning alternately.
                c.spawn((
                    Mesh3d(look.loop_mesh.clone()),
                    MeshMaterial3d(look.wire.clone()),
                    Transform::from_xyz(x, wire.half.y, 0.0)
                        .with_rotation(Quat::from_rotation_y(tilt) * Quat::from_rotation_z(std::f32::consts::FRAC_PI_2))
                        .with_scale(Vec3::new(scale_y, 1.0, scale_z)),
                ));
            }
            for x in [-wire.half.x, wire.half.x] {
                c.spawn(super::part(a, &a.dark_metal, Vec3::new(x, 0.3, 0.0), Vec3::new(0.025, 0.6, 0.025)));
            }
        });
    }
    let c = &tuning.round.gadgets.cover;
    for entity in &covers {
        let (half_w, h) = (c.width / 2.0, c.height);
        let wing = half_w * 0.36;
        let centre_half = half_w - wing;
        let wing_angle = 0.45_f32;
        commands.entity(entity).with_children(|p| {
            p.spawn(super::part(a, &a.olive, Vec3::new(0.0, h / 2.0, 0.0), Vec3::new(centre_half * 2.0, h, 0.03)));
            for s in [1.0_f32, -1.0] {
                let at = Vec3::new(s * (centre_half + wing * wing_angle.cos() / 2.0), h / 2.0, wing * wing_angle.sin() / 2.0);
                p.spawn((
                    Mesh3d(a.unit_cube.clone()),
                    MeshMaterial3d(a.olive.clone()),
                    Transform::from_translation(at)
                        .with_rotation(Quat::from_rotation_y(-s * wing_angle))
                        .with_scale(Vec3::new(wing, h * 0.97, 0.03)),
                ));
                // Folding leg behind.
                p.spawn((
                    Mesh3d(a.unit_cube.clone()),
                    MeshMaterial3d(a.dark_metal.clone()),
                    Transform::from_xyz(s * centre_half * 0.6, h * 0.3, 0.22)
                        .with_rotation(Quat::from_rotation_x(0.75))
                        .with_scale(Vec3::new(0.035, h * 0.7, 0.035)),
                ));
            }
            // Vision block and a carry handle.
            p.spawn(super::part(a, &a.glass, Vec3::new(0.0, h - 0.13, -0.017), Vec3::new(0.26, 0.07, 0.01)));
            p.spawn(super::part(a, &a.dark_metal, Vec3::new(0.0, h - 0.13, 0.0), Vec3::new(0.32, 0.11, 0.035)));
            p.spawn(super::part(a, &a.rubber, Vec3::new(0.0, h + 0.03, 0.03), Vec3::new(0.22, 0.03, 0.03)));
            // Edge trim.
            p.spawn(super::part(a, &a.dark_metal, Vec3::new(0.0, h, 0.0), Vec3::new(centre_half * 2.0, 0.03, 0.04)));
        });
    }
    for entity in &alarms {
        commands.entity(entity).with_children(|p| {
            p.spawn(super::part(a, &a.olive, Vec3::new(0.0, 0.0, -0.005), Vec3::new(0.09, 0.09, 0.04)));
            p.spawn((
                Mesh3d(a.unit_sphere.clone()),
                MeshMaterial3d(look.lens_glow.clone()),
                Transform::from_xyz(0.0, 0.012, 0.018).with_scale(Vec3::new(0.035, 0.035, 0.02)),
            ));
            p.spawn(super::part(a, &a.rubber, Vec3::new(0.035, 0.06, -0.005), Vec3::new(0.008, 0.07, 0.008)));
            p.spawn(super::part(a, &a.glass, Vec3::new(0.0, -0.025, 0.016), Vec3::new(0.05, 0.015, 0.005)));
        });
    }
}
