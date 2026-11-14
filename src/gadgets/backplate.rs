//! Mortise's backplate charges (DESIGN.md "Starter operators", "M6 part
//! two").
//!
//! Hold 3 (`bolt_time`) looking at the plate side of a reinforced wall or
//! hatch (the defenders' side) to bolt a charge on. They are inert until
//! Mortise taps 3, from anywhere: the most recently bolted charge fires.
//! It blasts through the wall: every attacker gadget mounted on the far
//! side within `gadget_radius` (ring cutters, breach charges) is
//! destroyed, people right behind the wall are hurt a little, and the
//! charge punches a fist-sized [`HardBreach`] hole through the
//! reinforcement: a permanent sightline. Charges can be shot or blown up.

use avian3d::prelude::*;
use bevy::prelude::*;

use super::{
    DeployedGadget, GadgetActivity, GadgetAssets, GadgetButtons, GadgetEvent, GadgetIntent,
    GadgetKind, GadgetOwner, GadgetStage, RoundScoped, UniqueGadget, eye_and_look, gadget_block,
    cutter::RingCutter,
    particles::{Burst, ParticleKind},
    remote_view::RemoteView,
};
use crate::{
    combat::{Dead, Health, Team},
    config::Tuning,
    destruction::{BreachCharge, Explosion, HardBreach, SoftSurface, surface_root},
    physics::GameLayer,
    player::{CharacterState, Player, PlayerInput},
    round::Round,
};

pub struct BackplatePlugin;

impl Plugin for BackplatePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedUpdate,
            (use_backplates.in_set(GadgetStage::Use), drop_orphans.in_set(GadgetStage::Effects)),
        )
        .add_systems(Update, (dress_backplates, fired_looks));
    }
}

/// A charge bolted to a reinforcement plate. Its `Transform` is on the
/// plate, +Z out of it (towards the defenders' side). Replicated to
/// clients (`net/gadget_sync.rs`).
#[derive(Component, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct BackplateCharge {
    #[entities]
    pub owner: Entity,
    /// The reinforced wall or hatch (server only).
    #[serde(skip, default = "super::server_only_entity")]
    pub surface: Entity,
    /// Out of the plate (world), towards the defenders' side.
    pub normal: Vec3,
    /// On the surface's middle plane (world): where the hole goes.
    pub centre: Vec3,
    /// On the far (attackers') face (world).
    pub far: Vec3,
    /// Increasing per placement: "the latest" charge is the highest.
    pub serial: u32,
}

struct Mount {
    at: Vec3,
    centre: Vec3,
    far: Vec3,
    normal: Vec3,
}

/// Where a backplate charge aimed from `eye` would sit on `surface`: only
/// the plate side of a reinforced wall or hatch.
fn backplate_mount(surface: &SoftSurface, gt: &GlobalTransform, eye: Vec3, dir: Vec3, tuning: &Tuning) -> Option<Mount> {
    let reinforcement = surface.reinforced?;
    let side = surface.side_of(gt, eye);
    if side * reinforcement.side <= 0.0 {
        return None;
    }
    let mount = surface.mount(gt, eye, dir, tuning.round.gadgets.backplate.reach)?;
    let plane = surface.spec.plane;
    let (uv, _) = plane.split_local(gt.affine().inverse().transform_point3(mount.point));
    let (min, max) = (surface.grid.min(), surface.grid.max());
    let uv = uv.clamp(min + 0.12, max - 0.12);
    let (_, brace) = surface.plate_depth(side, &tuning.destruction);
    let outer = if side > 0.0 { brace.1 } else { brace.0 };
    let (w0, w1) = surface.spec.skin_depth;
    let far_w = if side > 0.0 { w0 } else { w1 };
    Some(Mount {
        at: gt.transform_point(plane.to_local(uv.x, uv.y, outer + side * 0.03)),
        centre: gt.transform_point(plane.to_local(uv.x, uv.y, (w0 + w1) / 2.0)),
        far: gt.transform_point(plane.to_local(uv.x, uv.y, far_w)),
        normal: mount.normal,
    })
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn use_backplates(
    round: Res<Round>,
    tuning: Res<Tuning>,
    time: Res<Time>,
    spatial: SpatialQuery,
    colliders: Query<&ColliderOf>,
    surfaces: Query<(&SoftSurface, &GlobalTransform)>,
    mut users: Query<
        (
            Entity,
            &Transform,
            &CharacterState,
            &PlayerInput,
            &Team,
            &mut GadgetIntent,
            &mut UniqueGadget,
            &GadgetButtons,
            &mut GadgetActivity,
            Has<Dead>,
            Has<RemoteView>,
        ),
        With<Player>,
    >,
    charges: Query<(Entity, &BackplateCharge)>,
    cutters: Query<(Entity, &RingCutter, &GadgetOwner)>,
    breach_charges: Query<(Entity, &BreachCharge, &GlobalTransform)>,
    mut events: MessageWriter<GadgetEvent>,
    (mut breaches, mut explosions): (MessageWriter<HardBreach>, MessageWriter<Explosion>),
    mut serial: Local<u32>,
    mut commands: Commands,
) {
    let t = &tuning.round.gadgets.backplate;
    let dt = time.delta_secs();
    let filter = SpatialQueryFilter::from_mask(GameLayer::World);
    for (entity, transform, state, input, team, mut intent, mut gadget, buttons, mut activity, dead, viewing) in
        &mut users
    {
        if gadget.kind != GadgetKind::BackplateCharge {
            continue;
        }
        intent.use_unique = false;
        let block = gadget_block(dead, round.phase, *team, viewing);
        let still = input.move_axis.length_squared() < 0.01;
        let (eye, look) = eye_and_look(transform, state, input, &tuning.body, tuning.lean.offset);
        let aim = || {
            let hit = spatial.cast_ray(eye, look, t.reach + 0.3, true, &filter)?;
            let root = surface_root(hit.entity, &colliders);
            let (surface, gt) = surfaces.get(root).ok()?;
            Some((root, backplate_mount(surface, gt, eye, *look, &tuning)?))
        };
        let button = buttons.unique;

        match *activity {
            GadgetActivity::Bolting { surface, progress } => {
                let target = aim().filter(|(root, _)| *root == surface);
                match target {
                    Some((root, mount)) if button.held && still && block.is_none() && gadget.remaining > 0 => {
                        let progress = progress + dt / t.bolt_time.max(0.01);
                        if progress < 1.0 {
                            *activity = GadgetActivity::Bolting { surface, progress };
                        } else {
                            *activity = GadgetActivity::Idle;
                            gadget.remaining -= 1;
                            *serial += 1;
                            commands.spawn((
                                Name::new("Backplate charge"),
                                BackplateCharge {
                                    owner: entity,
                                    surface: root,
                                    normal: mount.normal,
                                    centre: mount.centre,
                                    far: mount.far,
                                    serial: *serial,
                                },
                                GadgetOwner {
                                    entity: Some(entity),
                                    team: *team,
                                },
                                DeployedGadget,
                                RoundScoped,
                                Health::new(t.health),
                                Transform::from_translation(mount.at).looking_to(-mount.normal, Vec3::Y),
                                Visibility::default(),
                                Collider::cuboid(0.22, 0.22, 0.08),
                                Sensor,
                                CollisionLayers::new(GameLayer::Hitbox, LayerMask::NONE),
                                children![super::blast_anchor(Vec3::Z * 0.05)],
                            ));
                            events.write(GadgetEvent::BackplateSet { by: entity, position: mount.at });
                        }
                    }
                    _ => *activity = GadgetActivity::Idle,
                }
            }
            GadgetActivity::Idle if button.pressed => {
                if let Some(reason) = block {
                    events.write(GadgetEvent::Refused { by: entity, reason: reason.into() });
                    continue;
                }
                if gadget.remaining > 0
                    && let Some((surface, _)) = aim()
                {
                    *activity = GadgetActivity::Bolting { surface, progress: 0.0 };
                }
            }
            _ => {}
        }

        if !button.tapped || block.is_some() {
            continue;
        }
        // Tap: fire the latest charge.
        let Some((charge_entity, charge)) = charges
            .iter()
            .filter(|(_, c)| c.owner == entity)
            .max_by_key(|(_, c)| c.serial)
        else {
            let reason = if gadget.remaining > 0 {
                "hold 3 on the plate side of a reinforced wall to bolt a charge on"
            } else {
                "no backplate charge placed"
            };
            events.write(GadgetEvent::Refused { by: entity, reason: reason.into() });
            continue;
        };
        let reach = t.gadget_radius;
        let mut destroyed = 0;
        for (cutter_entity, cutter, owner) in &cutters {
            let far_side = cutter.normal.dot(charge.normal) < 0.0;
            if owner.team != *team && far_side && cutter.near.distance(charge.far) <= reach + 0.1 {
                events.write(GadgetEvent::Destroyed {
                    entity: cutter_entity,
                    position: cutter.near,
                });
                commands.entity(cutter_entity).despawn();
                destroyed += 1;
            }
        }
        // Only attackers carry breach charges (destruction's loadout rule).
        for (breach_entity, _, at) in &breach_charges {
            let far_side = (at.translation() - charge.centre).dot(charge.normal) < 0.0;
            if far_side && at.translation().distance(charge.far) <= reach {
                events.write(GadgetEvent::Destroyed {
                    entity: breach_entity,
                    position: at.translation(),
                });
                commands.entity(breach_entity).despawn();
                destroyed += 1;
            }
        }
        breaches.write(HardBreach {
            surface: charge.surface,
            centre: charge.centre,
            radius: t.hole_radius,
            source: Some(entity),
        });
        // A small blast on the far side for whoever is crouched there.
        explosions.write(Explosion {
            centre: charge.far - charge.normal * 0.15,
            breach_radius: 0.0,
            damage_radius: t.blast_radius,
            max_damage: t.blast_damage,
            source: Some(entity),
            attached_to: None,
        });
        // Flash and soot: `fired_looks`, from the event (on clients too).
        events.write(GadgetEvent::BackplateFired {
            position: charge.far,
            normal: charge.normal,
            destroyed,
        });
        commands.entity(charge_entity).despawn();
    }
}

/// Charges on a surface that no longer exists, or is no longer reinforced
/// (a new round restored it), fall off.
fn drop_orphans(
    charges: Query<(Entity, &BackplateCharge)>,
    surfaces: Query<&SoftSurface>,
    mut commands: Commands,
) {
    for (entity, charge) in &charges {
        if surfaces.get(charge.surface).is_ok_and(|s| !s.is_reinforced()) || !surfaces.contains(charge.surface) {
            commands.entity(entity).despawn();
        }
    }
}

/// The blast out of the far face of the wall (presentation, from the event
/// so clients show it too).
fn fired_looks(mut events: MessageReader<GadgetEvent>, mut bursts: MessageWriter<Burst>) {
    for event in events.read() {
        let &GadgetEvent::BackplateFired { position: far, normal, .. } = event else {
            continue;
        };
        bursts.write(Burst::new(ParticleKind::Flash, far - normal * 0.1).count(2).size(0.3, 1.2));
        bursts.write(
            Burst::new(ParticleKind::Soot, far - normal * 0.2)
                .toward(-normal, 0.6)
                .count(8)
                .speed(2.0)
                .size(0.3, 1.4),
        );
        bursts.write(
            Burst::new(ParticleKind::Spark, far - normal * 0.05)
                .toward(-normal, 0.7)
                .count(14)
                .speed(6.0),
        );
    }
}

/// A flat steel box with bolts, a coil of det-cord and a green ready light.
fn dress_backplates(new: Query<Entity, Added<BackplateCharge>>, assets: Res<GadgetAssets>, mut commands: Commands) {
    let a = &*assets;
    for entity in &new {
        commands.entity(entity).with_children(|c| {
            c.spawn(super::part(a, &a.olive, Vec3::ZERO, Vec3::new(0.24, 0.24, 0.05)));
            c.spawn(super::part(a, &a.dark_metal, Vec3::new(0.0, 0.0, 0.03), Vec3::new(0.16, 0.16, 0.02)));
            for (x, y) in [(-0.1, -0.1), (0.1, -0.1), (-0.1, 0.1), (0.1, 0.1)] {
                c.spawn((
                    Mesh3d(a.unit_cylinder.clone()),
                    MeshMaterial3d(a.steel.clone()),
                    Transform::from_xyz(x, y, 0.03)
                        .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2))
                        .with_scale(Vec3::new(0.025, 0.02, 0.025)),
                ));
            }
            c.spawn(super::part(a, &a.rubber, Vec3::new(0.0, -0.13, 0.02), Vec3::new(0.05, 0.05, 0.04)));
            c.spawn((
                Mesh3d(a.unit_sphere.clone()),
                MeshMaterial3d(a.green_light.clone()),
                Transform::from_xyz(0.06, 0.06, 0.045).with_scale(Vec3::splat(0.02)),
            ));
        });
    }
}
