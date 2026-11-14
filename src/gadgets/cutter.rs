//! Auger's ring cutter (DESIGN.md "Starter operators", "M6 part two").
//!
//! Hold 3 (`clamp_time`) looking at a wall or hatch cover, soft or
//! reinforced, to clamp a cutter on; tap 3 to start the most recently
//! clamped one. A wall cutter always sits with its centre `cut_height`
//! above the bottom of the wall, so the plug is a walk-through opening; a
//! hatch cutter sits on the middle of the cover (from above).
//!
//! While cutting (`cut_time`) it is very loud: a [`Noise`] of kind
//! `Breach` every `noise_interval` (loud enough to saturate seismic
//! stakes), dust shed from the wall, the view shaking nearby. At
//! `expose_at` the blade breaks through to the far side as a small
//! [`CutterBlade`] target orbiting the cut line: any damage to it jams the
//! cutter, and Auger must hold 3 at the cutter for `reseat_time` (standing
//! still at the wall) to get it going again. The cutter body itself can be
//! shot or blown up.
//!
//! When the cut completes: a [`HardBreach`] of `cut_radius` through the
//! surface (plate included), and the plug falls inward, knocking down
//! (prone) and hurting anyone standing right behind it.

use std::f32::consts::{FRAC_PI_2, TAU};

use avian3d::prelude::*;
use bevy::{mesh::TorusMeshBuilder, prelude::*};

use super::{
    DeployedGadget, GadgetActivity, GadgetAssets, GadgetButtons, GadgetEvent, GadgetIntent,
    GadgetKind, GadgetOwner, GadgetStage, RoundScoped, UniqueGadget, eye_and_look, gadget_block,
    particles::{Burst, ParticleKind},
    remote_view::RemoteView,
};
use crate::{
    combat::{DamageKind, DamageMessage, Dead, Health, Noise, NoiseKind, Team},
    config::Tuning,
    destruction::{HardBreach, SoftSurface, SurfaceRole, surface_root},
    physics::GameLayer,
    player::{CharacterState, Player, PlayerInput, Stance},
    round::Round,
};

pub struct CutterPlugin;

impl Plugin for CutterPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedUpdate,
            (
                use_ring_cutters.in_set(GadgetStage::Use),
                run_cutters.in_set(GadgetStage::Simulate),
                (jam_cutters, drop_orphans).in_set(GadgetStage::Effects),
            ),
        )
        .add_systems(Update, (dress_cutters, animate_cutters, animate_plugs).chain());
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CutterState {
    /// Clamped on, waiting for Auger to start it (tap 3).
    Ready,
    Cutting,
    /// The blade was hit; Auger must re-seat it.
    Jammed,
}

/// A clamped ring cutter. Its `Transform` sits at the cut centre on the
/// near face, local +Z out of the surface (towards Auger's side), local X
/// and Y along the face.
#[derive(Component, Clone, Debug)]
pub struct RingCutter {
    pub owner: Entity,
    /// Root of the wall or hatch it is clamped to.
    pub surface: Entity,
    pub state: CutterState,
    /// 0..1.
    pub progress: f32,
    /// Ring angle, radians; the blade is at this angle on the cut circle.
    pub angle: f32,
    /// Out of the surface on the cutter's side (world).
    pub normal: Vec3,
    /// Cut centre on the near and far faces (world).
    pub near: Vec3,
    pub far: Vec3,
    /// Along the face (world): local X and Y of the cutter.
    pub axis_u: Vec3,
    pub axis_v: Vec3,
    pub radius: f32,
    /// Half size of the frame and the blade's path along `axis_u` /
    /// `axis_v` (the cut circle clipped to the panel between its posts).
    pub extent: Vec2,
    /// The blade has broken through to the far side.
    pub exposed: bool,
    pub noise_timer: f32,
    /// Increasing per placement: "the latest" cutter is the highest.
    pub serial: u32,
    pub reinforced: bool,
    pub hatch: bool,
}

impl RingCutter {
    /// Where the blade is (world), on the far face.
    pub fn blade_position(&self) -> Vec3 {
        self.far
            + self.axis_u * (self.angle.cos() * self.extent.x)
            + self.axis_v * (self.angle.sin() * self.extent.y)
            - self.normal * 0.03
    }
}

/// The exposed blade on the far side: shoot it to jam the cutter.
#[derive(Component, Clone, Copy, Debug)]
pub struct CutterBlade {
    pub cutter: Entity,
}

/// Where a cutter would clamp on.
struct Mount {
    near: Vec3,
    far: Vec3,
    normal: Vec3,
    axis_u: Vec3,
    axis_v: Vec3,
    extent: Vec2,
    reinforced: bool,
    hatch: bool,
}

/// Where a cutter aimed from `eye` along `dir` would sit on `surface`.
fn cutter_mount(surface: &SoftSurface, gt: &GlobalTransform, eye: Vec3, dir: Vec3, tuning: &Tuning) -> Option<Mount> {
    let t = &tuning.round.gadgets.ring_cutter;
    let hatch = match surface.role() {
        SurfaceRole::Wall => false,
        SurfaceRole::Hatch => true,
        _ => return None,
    };
    if surface.broken {
        return None;
    }
    let mount = surface.mount(gt, eye, dir, t.reach)?;
    if hatch && mount.normal.y < 0.7 {
        // Hatches only from above.
        return None;
    }
    let plane = surface.spec.plane;
    let inverse = gt.affine().inverse();
    let (mut uv, _) = plane.split_local(inverse.transform_point3(mount.point));
    let (min, max) = (surface.grid.min(), surface.grid.max());
    // The frame sits in the middle of the panel (between the posts, or on
    // the hatch cover); the cut circle is clipped by whatever is around.
    let half = (max - min) / 2.0;
    uv.x = (min.x + max.x) / 2.0;
    let extent = if hatch {
        uv.y = (min.y + max.y) / 2.0;
        Vec2::new(t.cut_radius.min(half.x - 0.02), t.cut_radius.min(half.y - 0.02))
    } else {
        uv.y = t.cut_height;
        Vec2::new(t.cut_radius.min(half.x - 0.06), t.cut_radius)
    };
    let side = surface.side_of(gt, eye);
    let (w0, w1) = surface.spec.skin_depth;
    // Outer face on each side, plate and braces included.
    let face = |s: f32| -> f32 {
        let skin = if s > 0.0 { w1 } else { w0 };
        match surface.reinforced {
            Some(r) if r.side * s > 0.0 => {
                let (_, brace) = surface.plate_depth(s, &tuning.destruction);
                if s > 0.0 { brace.1 } else { brace.0 }
            }
            _ => skin,
        }
    };
    let near = gt.transform_point(plane.to_local(uv.x, uv.y, face(side)));
    let far = gt.transform_point(plane.to_local(uv.x, uv.y, face(-side)));
    let normal = mount.normal;
    let up = if hatch {
        (eye - near).with_y(0.0).normalize_or(Vec3::Z)
    } else {
        Vec3::Y
    };
    let axis_v = (up - normal * up.dot(normal)).normalize_or(Vec3::Y);
    let axis_u = axis_v.cross(normal).normalize_or(Vec3::X);
    Some(Mount {
        near,
        far,
        normal,
        axis_u,
        axis_v,
        extent,
        reinforced: mount.reinforced,
        hatch,
    })
}

/// Aim at a surface a cutter can clamp on: (surface root, mount).
fn aim_mount(
    eye: Vec3,
    look: Dir3,
    spatial: &SpatialQuery,
    colliders: &Query<&ColliderOf>,
    surfaces: &Query<(&SoftSurface, &GlobalTransform)>,
    tuning: &Tuning,
) -> Option<(Entity, Mount)> {
    let filter = SpatialQueryFilter::from_mask(GameLayer::World);
    let reach = tuning.round.gadgets.ring_cutter.reach;
    let hit = spatial.cast_ray(eye, look, reach + 0.3, true, &filter)?;
    let root = surface_root(hit.entity, colliders);
    let (surface, gt) = surfaces.get(root).ok()?;
    Some((root, cutter_mount(surface, gt, eye, *look, tuning)?))
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn use_ring_cutters(
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
    mut cutters: Query<(Entity, &mut RingCutter)>,
    mut events: MessageWriter<GadgetEvent>,
    mut serial: Local<u32>,
    mut commands: Commands,
) {
    let t = &tuning.round.gadgets.ring_cutter;
    let dt = time.delta_secs();
    for (entity, transform, state, input, team, mut intent, mut gadget, buttons, mut activity, dead, viewing) in
        &mut users
    {
        if gadget.kind != GadgetKind::RingCutter {
            continue;
        }
        // The cutter uses tap/hold, not the latched press.
        intent.use_unique = false;
        let block = gadget_block(dead, round.phase, *team, viewing);
        let still = input.move_axis.length_squared() < 0.01;
        let (eye, look) = eye_and_look(transform, state, input, &tuning.body, tuning.lean.offset);
        let button = buttons.unique;

        match *activity {
            GadgetActivity::Clamping { surface, progress } => {
                let target = aim_mount(eye, look, &spatial, &colliders, &surfaces, &tuning)
                    .filter(|(root, _)| *root == surface);
                match target {
                    Some((root, mount)) if button.held && still && block.is_none() && gadget.remaining > 0 => {
                        let progress = progress + dt / t.clamp_time.max(0.01);
                        if progress < 1.0 {
                            *activity = GadgetActivity::Clamping { surface, progress };
                        } else {
                            *activity = GadgetActivity::Idle;
                            gadget.remaining -= 1;
                            *serial += 1;
                            let cutter = spawn_cutter(&mut commands, entity, *team, root, &mount, *serial, t.health);
                            events.write(GadgetEvent::CutterClamped {
                                by: entity,
                                cutter,
                                position: mount.near,
                                reinforced: mount.reinforced,
                            });
                        }
                    }
                    _ => *activity = GadgetActivity::Idle,
                }
            }
            GadgetActivity::Reseating { cutter, progress } => {
                let ok = button.held
                    && still
                    && block.is_none()
                    && cutters.get(cutter).is_ok_and(|(_, c)| {
                        c.state == CutterState::Jammed && looking_at(eye, *look, c.near, t.reach + 0.4)
                    });
                if !ok {
                    *activity = GadgetActivity::Idle;
                } else {
                    let progress = progress + dt / t.reseat_time.max(0.01);
                    if progress < 1.0 {
                        *activity = GadgetActivity::Reseating { cutter, progress };
                    } else {
                        *activity = GadgetActivity::Idle;
                        if let Ok((_, mut c)) = cutters.get_mut(cutter) {
                            c.state = CutterState::Cutting;
                        }
                        events.write(GadgetEvent::CutterReseated { cutter });
                    }
                }
            }
            GadgetActivity::Idle if button.pressed => {
                if let Some(reason) = block {
                    events.write(GadgetEvent::Refused { by: entity, reason });
                    continue;
                }
                let jammed = cutters.iter().find(|(_, c)| {
                    c.owner == entity
                        && c.state == CutterState::Jammed
                        && looking_at(eye, *look, c.near, t.reach + 0.4)
                });
                if let Some((cutter, _)) = jammed {
                    *activity = GadgetActivity::Reseating { cutter, progress: 0.0 };
                } else if gadget.remaining > 0
                    && let Some((surface, _)) = aim_mount(eye, look, &spatial, &colliders, &surfaces, &tuning)
                {
                    *activity = GadgetActivity::Clamping { surface, progress: 0.0 };
                }
            }
            _ => {}
        }

        // A tap starts the latest cutter that is ready.
        if button.tapped && block.is_none() {
            let ready = cutters
                .iter_mut()
                .filter(|(_, c)| c.owner == entity && c.state == CutterState::Ready)
                .max_by_key(|(_, c)| c.serial);
            match ready {
                Some((cutter, mut c)) => {
                    c.state = CutterState::Cutting;
                    events.write(GadgetEvent::CutterStarted { cutter });
                }
                None => {
                    let reason = if gadget.remaining > 0 {
                        "hold 3 on a wall or hatch to clamp a ring cutter"
                    } else {
                        "no ring cutter ready"
                    };
                    events.write(GadgetEvent::Refused { by: entity, reason });
                }
            }
        }
    }
}

/// `target` is within `reach` of `eye` and roughly in the middle of the view.
fn looking_at(eye: Vec3, look: Vec3, target: Vec3, reach: f32) -> bool {
    let to = target - eye;
    to.length() <= reach && to.normalize_or_zero().dot(look) > 0.8
}

fn spawn_cutter(
    commands: &mut Commands,
    owner: Entity,
    team: Team,
    surface: Entity,
    mount: &Mount,
    serial: u32,
    health: f32,
) -> Entity {
    let up = mount.axis_v;
    commands
        .spawn((
            Name::new("Ring cutter"),
            RingCutter {
                owner,
                surface,
                state: CutterState::Ready,
                progress: 0.0,
                angle: FRAC_PI_2,
                normal: mount.normal,
                near: mount.near,
                far: mount.far,
                axis_u: mount.axis_u,
                axis_v: mount.axis_v,
                radius: 0.0,
                extent: mount.extent,
                exposed: false,
                noise_timer: 0.0,
                serial,
                reinforced: mount.reinforced,
                hatch: mount.hatch,
            },
            GadgetOwner {
                entity: Some(owner),
                team,
            },
            DeployedGadget,
            RoundScoped,
            Health::new(health),
            Transform::from_translation(mount.near).looking_to(-mount.normal, up),
            Visibility::default(),
            // The hub and motor stick out of the wall: that is what can be shot.
            Collider::sphere(0.2),
            Sensor,
            CollisionLayers::new(GameLayer::Hitbox, LayerMask::NONE),
        ))
        .id()
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn run_cutters(
    tuning: Res<Tuning>,
    time: Res<Time>,
    mut cutters: Query<(Entity, &mut RingCutter)>,
    surfaces: Query<(), With<SoftSurface>>,
    mut blades: Query<(&CutterBlade, &mut Transform)>,
    mut people: Query<(Entity, &Transform, &CharacterState, &mut PlayerInput), (With<Player>, Without<Dead>, Without<CutterBlade>)>,
    mut noise: MessageWriter<Noise>,
    mut breaches: MessageWriter<HardBreach>,
    mut damage: MessageWriter<DamageMessage>,
    mut events: MessageWriter<GadgetEvent>,
    mut bursts: MessageWriter<Burst>,
    mut commands: Commands,
) {
    let t = &tuning.round.gadgets.ring_cutter;
    let dt = time.delta_secs();
    for (entity, mut cutter) in &mut cutters {
        cutter.radius = t.cut_radius;
        if !surfaces.contains(cutter.surface) {
            commands.entity(entity).despawn();
            continue;
        }
        if cutter.state != CutterState::Cutting {
            continue;
        }
        cutter.progress += dt / t.cut_time.max(0.1);
        cutter.angle = (cutter.angle + dt * t.revolutions * TAU / t.cut_time.max(0.1)).rem_euclid(TAU);
        cutter.noise_timer -= dt;
        if cutter.noise_timer <= 0.0 {
            cutter.noise_timer = t.noise_interval;
            noise.write(Noise {
                source: Some(cutter.owner),
                position: cutter.near,
                loudness: t.noise,
                kind: NoiseKind::Breach,
            });
        }
        if !cutter.exposed && cutter.progress >= t.expose_at {
            cutter.exposed = true;
            let at = cutter.blade_position();
            commands.spawn((
                Name::new("Ring cutter blade"),
                CutterBlade { cutter: entity },
                RoundScoped,
                Health::new(t.blade_health),
                Transform::from_translation(at),
                Visibility::default(),
                Collider::sphere(0.12),
                Sensor,
                CollisionLayers::new(GameLayer::Hitbox, LayerMask::NONE),
            ));
            events.write(GadgetEvent::CutterExposed { cutter: entity, position: at });
        }
        if cutter.progress < 1.0 {
            continue;
        }

        // Through: cut the plug out and drop it on whoever is behind.
        breaches.write(HardBreach {
            surface: cutter.surface,
            centre: cutter.near,
            radius: cutter.radius,
            source: Some(cutter.owner),
        });
        let behind = -cutter.normal;
        let mut knocked = 0;
        for (person, feet, state, mut input) in &mut people {
            let points = [
                feet.translation + Vec3::Y * 0.2,
                feet.translation + Vec3::Y * state.height / 2.0,
                feet.translation + Vec3::Y * state.height,
            ];
            let hit = points.iter().any(|p| {
                let rel = *p - cutter.far;
                let depth = rel.dot(behind);
                let lateral = (rel - behind * depth).length();
                (0.0..=t.knockdown_depth).contains(&depth) && lateral <= cutter.radius
            });
            if hit {
                knocked += 1;
                input.stance = Stance::Prone;
                damage.write(DamageMessage {
                    target: person,
                    amount: t.knockdown_damage,
                    kind: DamageKind::Gadget,
                    zone: None,
                    source: Some(cutter.owner),
                });
            }
        }
        bursts.write(
            Burst::new(ParticleKind::Dust, cutter.far + behind * 0.3)
                .toward(behind, 0.8)
                .count(24)
                .speed(1.5)
                .size(0.4, 1.6)
                .life(2.5)
                .jitter(cutter.radius),
        );
        commands.spawn((
            Name::new("Cut plug"),
            FallingPlug {
                normal: cutter.normal,
                axis_u: cutter.axis_u,
                axis_v: cutter.axis_v,
                far: cutter.far,
                radius: cutter.radius,
                extent: cutter.extent,
                thickness: (cutter.near - cutter.far).length(),
                hatch: cutter.hatch,
                metal: cutter.reinforced,
                age: 0.0,
            },
            RoundScoped,
            Transform::from_translation(cutter.far),
            Visibility::default(),
        ));
        events.write(GadgetEvent::CutterBreached {
            cutter: entity,
            position: cutter.far,
            knocked_down: knocked,
        });
        commands.entity(entity).despawn();
    }
    // Blades follow the ring.
    for (blade, mut transform) in &mut blades {
        if let Ok((_, cutter)) = cutters.get(blade.cutter) {
            transform.translation = cutter.blade_position();
        }
    }
}

/// Any damage to an exposed blade jams its cutter.
fn jam_cutters(
    tuning: Res<Tuning>,
    mut blades: Query<(Entity, &CutterBlade, &mut Health)>,
    mut cutters: Query<&mut RingCutter>,
    mut events: MessageWriter<GadgetEvent>,
    mut commands: Commands,
) {
    for (entity, blade, mut health) in &mut blades {
        if health.current >= health.max {
            continue;
        }
        *health = Health::new(tuning.round.gadgets.ring_cutter.blade_health);
        commands.entity(entity).remove::<Dead>();
        if let Ok(mut cutter) = cutters.get_mut(blade.cutter)
            && cutter.state == CutterState::Cutting
        {
            cutter.state = CutterState::Jammed;
            events.write(GadgetEvent::CutterJammed {
                cutter: blade.cutter,
                position: cutter.blade_position(),
            });
        }
    }
}

/// Blades whose cutter is gone.
fn drop_orphans(blades: Query<(Entity, &CutterBlade)>, cutters: Query<(), With<RingCutter>>, mut commands: Commands) {
    for (entity, blade) in &blades {
        if !cutters.contains(blade.cutter) {
            commands.entity(entity).despawn();
        }
    }
}

// ---- Presentation ---------------------------------------------------------

/// The visible machine (child of the cutter), jittered while cutting.
#[derive(Component)]
struct CutterVisual;

/// The drive ring: spins with the cut.
#[derive(Component)]
struct CutterSpinner;

/// Arm from the hub to the cutting head (follows the oval track).
#[derive(Component)]
struct CutterArm;

/// The cutting head on the track.
#[derive(Component)]
struct CutterHead;

/// The head's glow and its little saw disc (lit / spinning while cutting).
#[derive(Component)]
struct HeadGlow;
#[derive(Component)]
struct SawDisc;

/// Glowing cut line on the far side (shown once through).
#[derive(Component)]
struct FarKerf;

/// Model meshes and materials. Rings are unit-sized and scaled per cutter.
#[derive(Resource)]
struct CutterLook {
    guide_ring: Handle<Mesh>,
    spin_ring: Handle<Mesh>,
    kerf: Handle<Mesh>,
    plug_plaster: Handle<StandardMaterial>,
    plug_metal: Handle<StandardMaterial>,
    plug_mesh: Handle<Mesh>,
    yellow: Handle<StandardMaterial>,
    kerf_glow: Handle<StandardMaterial>,
}

/// Point on the oval track at `angle` (cutter local XY).
fn on_track(extent: Vec2, angle: f32) -> Vec3 {
    Vec3::new(angle.cos() * extent.x, angle.sin() * extent.y, 0.0)
}

#[allow(clippy::too_many_arguments)]
fn dress_cutters(
    new_cutters: Query<(Entity, &RingCutter), Added<RingCutter>>,
    new_blades: Query<Entity, Added<CutterBlade>>,
    new_plugs: Query<(Entity, &FallingPlug), Added<FallingPlug>>,
    assets: Res<GadgetAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut look: Local<Option<CutterLook>>,
    mut commands: Commands,
) {
    if new_cutters.is_empty() && new_blades.is_empty() && new_plugs.is_empty() {
        return;
    }
    let look = look.get_or_insert_with(|| CutterLook {
        // Tube radius about 3 cm on a 1 m ring.
        guide_ring: meshes.add(TorusMeshBuilder::new(0.97, 1.03).major_resolution(56).minor_resolution(8)),
        spin_ring: meshes.add(TorusMeshBuilder::new(0.975, 1.0).major_resolution(48).minor_resolution(6)),
        kerf: meshes.add(TorusMeshBuilder::new(0.99, 1.01).major_resolution(56).minor_resolution(4)),
        plug_plaster: materials.add(StandardMaterial {
            base_color: Color::srgb(0.78, 0.72, 0.62),
            perceptual_roughness: 0.95,
            ..default()
        }),
        plug_metal: materials.add(StandardMaterial {
            base_color: Color::srgb(0.42, 0.43, 0.44),
            metallic: 0.8,
            perceptual_roughness: 0.5,
            ..default()
        }),
        plug_mesh: meshes.add(Cylinder::new(1.0, 1.0)),
        yellow: materials.add(StandardMaterial {
            base_color: Color::srgb(0.85, 0.62, 0.08),
            metallic: 0.3,
            perceptual_roughness: 0.55,
            ..default()
        }),
        kerf_glow: materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.45, 0.1),
            emissive: LinearRgba::rgb(6.0, 1.8, 0.2),
            unlit: true,
            ..default()
        }),
    });
    let a = &*assets;
    // Torus meshes lie flat (axis Y); stand them up into the cutter's XY
    // plane. Local Z of the torus becomes the cutter's -Y.
    let upright = Quat::from_rotation_x(FRAC_PI_2);
    let oval = |e: Vec2| Vec3::new(e.x, 1.0, e.y);
    for (entity, cutter) in &new_cutters {
        let thickness = (cutter.near - cutter.far).length();
        let e = cutter.extent;
        let track = e + Vec2::splat(0.065);
        let drive = e.min_element() - 0.06;
        commands.entity(entity).with_children(|root| {
            root.spawn((CutterVisual, Transform::default(), Visibility::default()))
                .with_children(|v| {
                    // Oval guide track held off the wall on four clamp feet.
                    v.spawn((
                        Mesh3d(look.guide_ring.clone()),
                        MeshMaterial3d(a.olive.clone()),
                        Transform::from_xyz(0.0, 0.0, 0.07).with_rotation(upright).with_scale(oval(track)),
                    ));
                    for k in 0..4 {
                        let ang = k as f32 * FRAC_PI_2 + FRAC_PI_2 / 2.0;
                        let foot = on_track(track, ang);
                        v.spawn(super::part(a, &a.dark_metal, foot + Vec3::Z * 0.035, Vec3::new(0.09, 0.09, 0.07)));
                        v.spawn(super::part(a, &a.steel, foot + Vec3::Z * 0.075, Vec3::new(0.04, 0.04, 0.02)));
                        // Strut from the hub to the foot.
                        let length = foot.length();
                        v.spawn((
                            Mesh3d(a.unit_cube.clone()),
                            MeshMaterial3d(a.olive.clone()),
                            Transform::from_translation(foot / 2.0 + Vec3::Z * 0.13)
                                .with_rotation(Quat::from_rotation_z(foot.y.atan2(foot.x)))
                                .with_scale(Vec3::new(length, 0.045, 0.03)),
                        ));
                    }
                    // Hub, motor and handles.
                    v.spawn((
                        Mesh3d(a.unit_cylinder.clone()),
                        MeshMaterial3d(a.dark_metal.clone()),
                        Transform::from_xyz(0.0, 0.0, 0.13)
                            .with_rotation(upright)
                            .with_scale(Vec3::new(0.2, 0.14, 0.2)),
                    ));
                    v.spawn(super::part(a, &look.yellow, Vec3::new(0.0, 0.0, 0.27), Vec3::new(0.24, 0.2, 0.16)));
                    v.spawn(super::part(a, &a.rubber, Vec3::new(0.0, 0.16, 0.27), Vec3::new(0.3, 0.035, 0.035)));
                    v.spawn(super::part(a, &a.rubber, Vec3::new(0.0, -0.16, 0.27), Vec3::new(0.3, 0.035, 0.035)));
                    v.spawn((
                        Mesh3d(a.unit_sphere.clone()),
                        MeshMaterial3d(a.red_light.clone()),
                        Transform::from_xyz(0.09, 0.07, 0.355).with_scale(Vec3::splat(0.025)),
                    ));
                    // The drive ring spins on the hub.
                    v.spawn((CutterSpinner, Transform::from_xyz(0.0, 0.0, 0.1), Visibility::default()))
                        .with_children(|s| {
                            s.spawn((
                                Mesh3d(look.spin_ring.clone()),
                                MeshMaterial3d(a.steel.clone()),
                                Transform::from_rotation(upright).with_scale(Vec3::splat(drive)),
                            ));
                            for k in 0..3 {
                                let ang = k as f32 * TAU / 3.0;
                                s.spawn((
                                    Mesh3d(a.unit_cube.clone()),
                                    MeshMaterial3d(a.steel.clone()),
                                    Transform::from_translation(Vec3::new(ang.cos(), ang.sin(), 0.0) * drive / 2.0)
                                        .with_rotation(Quat::from_rotation_z(ang))
                                        .with_scale(Vec3::new(drive, 0.025, 0.015)),
                                ));
                            }
                        });
                    // Arm and cutting head, placed every frame.
                    v.spawn((
                        CutterArm,
                        Mesh3d(a.unit_cube.clone()),
                        MeshMaterial3d(a.dark_metal.clone()),
                        Transform::default(),
                    ));
                    v.spawn((CutterHead, Transform::default(), Visibility::default()))
                        .with_children(|h| {
                            h.spawn(super::part(a, &a.dark_metal, Vec3::new(0.0, 0.0, 0.04), Vec3::new(0.1, 0.08, 0.08)));
                            h.spawn((
                                SawDisc,
                                Mesh3d(a.unit_cylinder.clone()),
                                MeshMaterial3d(a.steel.clone()),
                                Transform::from_rotation(upright).with_scale(Vec3::new(0.11, 0.008, 0.11)),
                            ));
                            h.spawn((
                                HeadGlow,
                                super::part(a, &a.hot, Vec3::new(0.0, 0.0, -0.01), Vec3::new(0.05, 0.03, 0.02)),
                                Visibility::Hidden,
                            ));
                        });
                });
            // Glowing cut line on the far side, shown once the blade is through.
            root.spawn((
                FarKerf,
                Mesh3d(look.kerf.clone()),
                MeshMaterial3d(look.kerf_glow.clone()),
                Transform::from_xyz(0.0, 0.0, -thickness - 0.005)
                    .with_rotation(upright)
                    .with_scale(oval(e)),
                Visibility::Hidden,
            ));
        });
    }
    for entity in &new_blades {
        commands.entity(entity).with_children(|b| {
            b.spawn((
                Mesh3d(a.unit_cube.clone()),
                MeshMaterial3d(a.hot.clone()),
                Transform::from_scale(Vec3::new(0.06, 0.06, 0.06)),
            ));
            b.spawn((
                Mesh3d(a.unit_cube.clone()),
                MeshMaterial3d(a.steel.clone()),
                Transform::from_scale(Vec3::new(0.14, 0.02, 0.05)),
            ));
        });
    }
    for (entity, plug) in &new_plugs {
        let material = if plug.metal { look.plug_metal.clone() } else { look.plug_plaster.clone() };
        commands.entity(entity).with_children(|p| {
            p.spawn((
                Mesh3d(look.plug_mesh.clone()),
                MeshMaterial3d(material),
                // Cylinder axis (Y) along the surface normal: the parent
                // turns it. Squashed to the opening's oval.
                Transform::from_scale(Vec3::new(plug.extent.x * 0.97, plug.thickness, plug.extent.y * 0.97)),
            ));
        });
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn animate_cutters(
    cutters: Query<(&RingCutter, &Children)>,
    mut visuals: Query<(&mut Transform, &Children), With<CutterVisual>>,
    mut parts: ParamSet<(
        Query<&mut Transform, (With<CutterSpinner>, Without<CutterVisual>)>,
        Query<&mut Transform, (With<CutterArm>, Without<CutterVisual>)>,
        Query<(&mut Transform, &Children), (With<CutterHead>, Without<CutterVisual>)>,
        Query<&mut Transform, (With<SawDisc>, Without<CutterVisual>)>,
    )>,
    mut glows: Query<&mut Visibility, (With<HeadGlow>, Without<FarKerf>)>,
    mut kerfs: Query<&mut Visibility, (With<FarKerf>, Without<HeadGlow>)>,
    blade_places: Query<(&CutterBlade, &GlobalTransform)>,
    time: Res<Time>,
    mut bursts: MessageWriter<Burst>,
    mut dust_timer: Local<f32>,
    mut saw_angle: Local<f32>,
) {
    let now = time.elapsed_secs();
    *dust_timer -= time.delta_secs();
    *saw_angle = (*saw_angle + time.delta_secs() * 40.0).rem_euclid(TAU);
    let puff = *dust_timer <= 0.0;
    if puff {
        *dust_timer = 0.12;
    }
    for (cutter, children) in &cutters {
        let cutting = cutter.state == CutterState::Cutting;
        let head = on_track(cutter.extent, cutter.angle);
        for child in children.iter() {
            if let Ok(mut v) = kerfs.get_mut(child) {
                v.set_if_neq(if cutter.exposed { Visibility::Inherited } else { Visibility::Hidden });
            }
            let grandchildren: Vec<Entity> = match visuals.get_mut(child) {
                Ok((mut transform, grandchildren)) => {
                    transform.translation = if cutting {
                        let k = now * 61.0;
                        Vec3::new((k * 1.3).sin(), (k * 1.7).cos(), (k * 2.3).sin()) * 0.004
                    } else {
                        Vec3::ZERO
                    };
                    grandchildren.iter().collect()
                }
                Err(_) => continue,
            };
            for &g in &grandchildren {
                if let Ok(mut spin) = parts.p0().get_mut(g) {
                    spin.rotation = Quat::from_rotation_z(cutter.angle * 2.0);
                }
                if let Ok(mut arm) = parts.p1().get_mut(g) {
                    let length = head.length();
                    *arm = Transform::from_translation(head / 2.0 + Vec3::Z * 0.06)
                        .with_rotation(Quat::from_rotation_z(head.y.atan2(head.x)))
                        .with_scale(Vec3::new(length, 0.04, 0.03));
                }
                let mut head_children = Vec::new();
                if let Ok((mut at, kids)) = parts.p2().get_mut(g) {
                    at.translation = head + Vec3::Z * 0.02;
                    at.rotation = Quat::from_rotation_z(head.y.atan2(head.x));
                    head_children.extend(kids.iter());
                }
                for h in head_children {
                    if let Ok(mut v) = glows.get_mut(h) {
                        v.set_if_neq(if cutting { Visibility::Inherited } else { Visibility::Hidden });
                    }
                    if cutting && let Ok(mut disc) = parts.p3().get_mut(h) {
                        disc.rotation = Quat::from_rotation_z(*saw_angle) * Quat::from_rotation_x(FRAC_PI_2);
                    }
                }
            }
        }
        if cutting && puff {
            let head_world = cutter.near + cutter.axis_u * head.x + cutter.axis_v * head.y + cutter.normal * 0.05;
            bursts.write(
                Burst::new(ParticleKind::Spark, head_world)
                    .toward(cutter.normal + Vec3::Y * 0.3, 0.7)
                    .count(5),
            );
            bursts.write(Burst::new(ParticleKind::Dust, head_world).toward(cutter.normal, 0.9).count(1).speed(0.4));
            // The whole wall sheds dust from its top on both sides.
            if !cutter.hatch {
                let top = cutter.near + Vec3::Y * (cutter.radius + 1.0);
                let side = cutter.axis_u * ((now * 7.3).sin() * cutter.extent.x);
                bursts.write(Burst::new(ParticleKind::Dust, top + side).toward(-Vec3::Y, 0.4).count(1).speed(0.3));
                let far_top = cutter.far + Vec3::Y * (cutter.radius + 1.0) - side;
                bursts.write(Burst::new(ParticleKind::Dust, far_top).toward(-Vec3::Y, 0.4).count(1).speed(0.3));
            }
        }
    }
    if puff {
        for (blade, at) in &blade_places {
            if let Ok((cutter, _)) = cutters.get(blade.cutter)
                && cutter.state == CutterState::Cutting
            {
                bursts.write(
                    Burst::new(ParticleKind::Spark, at.translation())
                        .toward(-cutter.normal + Vec3::Y * 0.2, 0.8)
                        .count(4),
                );
            }
        }
    }
}

/// The cut-out plug falling away from the wall (or through the hatch).
#[derive(Component, Clone, Debug)]
pub struct FallingPlug {
    normal: Vec3,
    axis_u: Vec3,
    axis_v: Vec3,
    far: Vec3,
    radius: f32,
    extent: Vec2,
    thickness: f32,
    hatch: bool,
    metal: bool,
    age: f32,
}

/// Seconds a plug lies on the floor before sinking away.
const PLUG_REST: f32 = 12.0;

fn animate_plugs(
    mut plugs: Query<(Entity, &mut FallingPlug, &mut Transform)>,
    time: Res<Time>,
    mut commands: Commands,
) {
    for (entity, mut plug, mut transform) in &mut plugs {
        plug.age += time.delta_secs();
        let behind = -plug.normal;
        // Cylinder axis (Y) along the normal.
        let base = Quat::from_rotation_arc(Vec3::Y, plug.normal);
        if plug.hatch {
            // Drops straight down through the floor opening.
            let drop = (0.5 * 9.8 * plug.age * plug.age).min(3.0 - plug.thickness);
            transform.translation = plug.far + behind * (plug.thickness / 2.0) - Vec3::Y * drop;
            transform.rotation = base;
        } else {
            // Tips over its bottom edge and falls flat.
            let f = (plug.age / 0.6).min(1.0);
            let angle = FRAC_PI_2 * f * f;
            let pivot = plug.far - plug.axis_v * plug.radius;
            let tip = Quat::from_axis_angle(plug.axis_u, -angle);
            let offset = plug.axis_v * plug.radius + behind * (plug.thickness / 2.0);
            transform.translation = pivot + tip * offset;
            transform.rotation = tip * base;
        }
        if plug.age > PLUG_REST {
            transform.translation.y -= (plug.age - PLUG_REST) * 0.3;
        }
        if plug.age > PLUG_REST + 1.5 {
            commands.entity(entity).despawn();
        }
    }
}
