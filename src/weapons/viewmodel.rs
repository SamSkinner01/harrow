//! First-person weapon view ("viewmodel"). Presentation only.
//!
//! The weapon is drawn by a second camera that renders only render layer
//! [`VIEWMODEL_LAYER`], after the world camera, without clearing colour and
//! with its own (cleared) depth. So the weapon is always drawn on top and
//! never clips into walls, and its field of view is independent of the
//! player's FOV setting and ADS zoom.
//!
//! The weapon camera sits at the true eye position with the world view's
//! rotation (look + recoil + lean tilt), so lights hit the weapon from the
//! right directions:
//! - the world's sun only lights layer 0, so a matching shadowless sun on the
//!   weapon layer follows it, dimmed when a ray from the eye to the sun is
//!   blocked (in shade / indoors). Cheaper than a second set of shadow
//!   cascades for the weapon camera.
//! - the muzzle flash light is on both layers (lights the weapon and walls).
//!
//! On top of the hip/ADS pose: sway from mouse look, bob in step with the
//! footsteps, a sprint pose, recoil kick, a procedural reload (tilt, drop
//! the magazine, insert a new one, rack the slide/bolt when empty) and a
//! lower/raise animation when switching.

use std::f32::consts::{PI, TAU};

use avian3d::prelude::*;
use bevy::{
    asset::RenderAssetUsages,
    math::Affine3A,
    camera::visibility::RenderLayers,
    gltf::{GltfAssetLabel, GltfMaterialName},
    light::NotShadowCaster,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    world_serialization::{WorldAssetRoot, WorldInstanceReady},
};

use super::{
    AimView, ShotFired, Slot, WeaponAction, WeaponEvent, WeaponEventKind, WeaponState,
    materials::WeaponMaterials, view::ease,
};
use crate::{
    config::{Tuning, WeaponDef},
    physics::GameLayer,
    player::{CharacterState, Footstep, LocalPlayer, PlayerInput, Stance},
};

/// Render layer of the weapon in your hands.
pub const VIEWMODEL_LAYER: usize = 1;

pub struct ViewmodelPlugin;

impl Plugin for ViewmodelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ViewmodelAnim>()
            .add_systems(Startup, (create_flash_assets, spawn_viewmodel).chain())
            .add_observer(prepare_weapon_model)
            .add_systems(
                Update,
                (pose_viewmodel, update_muzzle_flash, follow_sun).chain(),
            );
    }
}

#[derive(Component)]
struct ViewmodelCamera;

/// Parent of the weapon models, posed every frame.
#[derive(Component)]
struct ViewmodelRig;

/// One weapon model in the rig.
#[derive(Component)]
struct WeaponModel {
    slot: Slot,
    info: Option<ModelInfo>,
}

/// Read from the model's named nodes once it has loaded. Positions are in
/// model space (before the tuning's scale and rotation).
#[derive(Clone, Copy)]
struct ModelInfo {
    sight: Option<Vec3>,
    muzzle: Option<Vec3>,
    magazine: Option<AnimatedPart>,
    slide: Option<AnimatedPart>,
    /// Muzzle flash, a child of the rig (so the model's scale doesn't
    /// affect it).
    flash: Entity,
}

/// A node the viewmodel moves (magazine, slide).
#[derive(Clone, Copy)]
struct AnimatedPart {
    entity: Entity,
    home: Transform,
    /// Turns a model-space offset into the node's parent space (the node
    /// may be nested under rotated or scaled nodes).
    to_local: Mat3,
}

/// Where a weapon's model sits in the rig: its tuning scale and rotation.
fn model_transform(def: &WeaponDef) -> Transform {
    let m = &def.model;
    let (yaw, pitch, roll) = m.rotation;
    Transform {
        translation: Vec3::ZERO,
        rotation: Quat::from_euler(
            EulerRot::YXZ,
            yaw.to_radians(),
            pitch.to_radians(),
            roll.to_radians(),
        ),
        scale: Vec3::splat(m.scale.max(1e-4)),
    }
}

/// A rig-space offset (metres, in the weapon's frame: -Z towards the
/// muzzle) as an offset in a part's local space.
fn part_offset(part: &AnimatedPart, model: &Transform, rig_offset: Vec3) -> Vec3 {
    let model_space = model.rotation.inverse() * rig_offset / model.scale.x;
    part.to_local * model_space
}

/// Sight point and muzzle of a model in rig space: from the tuning if set,
/// else from the model's nodes.
fn sight_and_muzzle(def: &WeaponDef, info: Option<&ModelInfo>) -> (Vec3, Vec3) {
    let model = model_transform(def);
    let pick = |tuned: Option<(f32, f32, f32)>, node: Option<Vec3>, fallback: Vec3| {
        tuned.map(Vec3::from).or(node).unwrap_or(fallback)
    };
    let sight = pick(def.model.sight, info.and_then(|i| i.sight), Vec3::new(0.0, 0.1, 0.0));
    let muzzle = pick(def.model.muzzle, info.and_then(|i| i.muzzle), Vec3::new(0.0, 0.05, -0.6));
    (model.transform_point(sight), model.transform_point(muzzle))
}

#[derive(Component)]
struct MuzzleFlashLight;

#[derive(Component)]
struct ViewmodelSun;

#[derive(Resource)]
struct FlashAssets {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

/// Animation state carried between frames.
#[derive(Resource, Default)]
struct ViewmodelAnim {
    prev_look: Option<Vec2>,
    /// Weapon lag behind mouse look, degrees (yaw, pitch).
    sway: Vec2,
    bob_cycle: f32,
    bob_amount: f32,
    sprint: f32,
    /// Recoil kick, 0 = rest; jumps on each shot and settles.
    kick: f32,
    slide_kick: f32,
    /// Seconds of muzzle flash left, and for which weapon.
    flash: f32,
    flash_slot: Option<Slot>,
    flash_roll: f32,
    flash_scale: f32,
    /// 0..1, how much direct sun reaches the weapon.
    sun: f32,
    rng: u32,
}

fn create_flash_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    // A soft star: bright core with four flame spikes, alpha in the colour
    // (additive blending).
    const SIZE: usize = 64;
    let mut data = Vec::with_capacity(SIZE * SIZE * 4);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let u = (x as f32 + 0.5) / SIZE as f32 * 2.0 - 1.0;
            let v = (y as f32 + 0.5) / SIZE as f32 * 2.0 - 1.0;
            let r = (u * u + v * v).sqrt();
            let angle = v.atan2(u);
            let spikes = (angle * 4.0).cos().abs().powf(6.0);
            let core = (1.0 - r * 2.2).clamp(0.0, 1.0);
            let flame = (1.0 - r / (0.45 + 0.55 * spikes)).clamp(0.0, 1.0).powf(1.5);
            let i = (core + flame * 0.8).clamp(0.0, 1.0);
            data.extend_from_slice(&[
                (255.0 * i) as u8,
                (230.0 * i * i) as u8,
                (170.0 * i * i * i) as u8,
                255,
            ]);
        }
    }
    let image = Image::new(
        Extent3d {
            width: SIZE as u32,
            height: SIZE as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    let material = materials.add(StandardMaterial {
        base_color: Color::linear_rgb(3.0, 2.0, 1.2),
        base_color_texture: Some(images.add(image)),
        alpha_mode: AlphaMode::Add,
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    commands.insert_resource(FlashAssets {
        mesh: meshes.add(Rectangle::new(1.0, 1.0)),
        material,
    });
}

fn spawn_viewmodel(mut commands: Commands, assets: Res<AssetServer>, tuning: Res<Tuning>) {
    let layer = RenderLayers::layer(VIEWMODEL_LAYER);
    let camera = commands
        .spawn((
            Name::new("Weapon camera"),
            ViewmodelCamera,
            Camera3d::default(),
            Camera {
                // After the world camera, on top of it.
                order: 1,
                // Keep the world image; depth is still cleared per camera.
                clear_color: ClearColorConfig::None,
                ..default()
            },
            Projection::Perspective(PerspectiveProjection {
                fov: tuning.weapons.view.fov.to_radians(),
                near: 0.01,
                far: 10.0,
                ..default()
            }),
            layer.clone(),
        ))
        .id();
    let rig = commands
        .spawn((
            Name::new("Weapon rig"),
            ViewmodelRig,
            Transform::default(),
            Visibility::default(),
            ChildOf(camera),
        ))
        .id();
    for slot in Slot::ALL {
        let def = slot.def(&tuning);
        let path = def.model.path.clone();
        commands.spawn((
            Name::new(format!("Weapon model {}", def.name)),
            WeaponModel { slot, info: None },
            WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path))),
            model_transform(def),
            Visibility::Hidden,
            layer.clone(),
            ChildOf(rig),
        ));
    }
    commands.spawn((
        Name::new("Weapon sun"),
        ViewmodelSun,
        DirectionalLight {
            illuminance: tuning.lighting.sun,
            shadow_maps_enabled: false,
            color: Color::srgb(1.0, 0.95, 0.86),
            ..default()
        },
        Transform::default(),
        layer.clone(),
    ));
    commands.spawn((
        Name::new("Muzzle flash light"),
        MuzzleFlashLight,
        PointLight {
            intensity: tuning.weapons.effects.muzzle_light,
            range: 8.0,
            radius: 0.05,
            color: Color::srgb(1.0, 0.75, 0.45),
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::default(),
        Visibility::Hidden,
        // World and weapon.
        RenderLayers::from_layers(&[0, VIEWMODEL_LAYER]),
    ));
}

/// Once a weapon model has spawned: move it to the weapon layer, swap in the
/// weapon materials, find its named nodes, add the muzzle flash.
#[allow(clippy::too_many_arguments)]
fn prepare_weapon_model(
    ready: On<WorldInstanceReady>,
    mut models: Query<(&mut WeaponModel, &ChildOf)>,
    children: Query<&Children>,
    parents: Query<&ChildOf>,
    names: Query<&Name>,
    transforms: Query<&Transform>,
    material_names: Query<&GltfMaterialName>,
    library: Res<WeaponMaterials>,
    flash: Res<FlashAssets>,
    tuning: Res<Tuning>,
    mut commands: Commands,
) {
    let Ok((mut model, rig)) = models.get_mut(ready.entity) else {
        return;
    };
    let rig = rig.parent();
    let root = ready.entity;
    let def = model.slot.def(&tuning).clone();
    let m = &def.model;
    // Transform of `entity` relative to the model root (nodes may be nested).
    let model_space = |entity: Entity| {
        let mut affine = Affine3A::IDENTITY;
        let mut current = entity;
        while current != root {
            let local = transforms.get(current).copied().unwrap_or_default();
            affine = local.compute_affine() * affine;
            match parents.get(current) {
                Ok(parent) => current = parent.parent(),
                Err(_) => break,
            }
        }
        affine
    };
    let part = |entity: Entity| {
        let parent = parents.get(entity).map(|p| p.parent()).unwrap_or(root);
        let to_parent = if parent == root {
            Affine3A::IDENTITY
        } else {
            model_space(parent)
        };
        AnimatedPart {
            entity,
            home: transforms.get(entity).copied().unwrap_or_default(),
            to_local: Mat3::from(to_parent.matrix3).inverse(),
        }
    };

    let layer = RenderLayers::layer(VIEWMODEL_LAYER);
    let (mut sight, mut muzzle, mut magazine, mut slide) = (None, None, None, None);
    for entity in children.iter_descendants(root) {
        commands.entity(entity).insert((layer.clone(), NotShadowCaster));
        // Our blockouts name materials from the weapon set; a model with
        // its own (textured) materials keeps them.
        if let Ok(material) = material_names.get(entity)
            && let Some(handle) = library.get(&material.0)
        {
            commands.entity(entity).insert(MeshMaterial3d(handle));
        }
        let Ok(name) = names.get(entity) else { continue };
        let name = name.as_str();
        if name == m.sight_node {
            sight = Some(model_space(entity).translation.into());
        } else if name == m.muzzle_node {
            muzzle = Some(model_space(entity).translation.into());
        } else if m.magazine_node.as_deref() == Some(name) {
            magazine = Some(part(entity));
        } else if m.slide_node.as_deref() == Some(name) {
            slide = Some(part(entity));
        }
    }
    if sight.is_none() && m.sight.is_none() {
        warn!("{}: no `{}` node and no `sight` in the tuning; ADS will be off", m.path, m.sight_node);
    }
    if muzzle.is_none() && m.muzzle.is_none() {
        warn!("{}: no `{}` node and no `muzzle` in the tuning", m.path, m.muzzle_node);
    }
    // Muzzle flash: a camera-facing star plus three crossed flame quads
    // along the barrel. On the rig, placed at the muzzle every frame.
    let flash_root = commands
        .spawn((
            Name::new(format!("Muzzle flash {}", def.name)),
            Transform::default(),
            Visibility::Hidden,
            layer.clone(),
            ChildOf(rig),
        ))
        .id();
    let quad = |transform: Transform| {
        (
            Mesh3d(flash.mesh.clone()),
            MeshMaterial3d(flash.material.clone()),
            transform,
            layer.clone(),
            NotShadowCaster,
            ChildOf(flash_root),
        )
    };
    commands.spawn(quad(
        Transform::from_xyz(0.0, 0.0, -0.02).with_scale(Vec3::splat(0.17)),
    ));
    for i in 0..3 {
        let around = Quat::from_rotation_z(i as f32 * PI / 3.0);
        // Rectangle faces +Z; turn it to lie along the barrel (-Z).
        let along = around * Quat::from_rotation_y(PI / 2.0);
        commands.spawn(quad(
            Transform::from_xyz(0.0, 0.0, -0.12)
                .with_rotation(along)
                .with_scale(Vec3::new(0.26, 0.09, 1.0)),
        ));
    }
    model.info = Some(ModelInfo {
        sight,
        muzzle,
        magazine,
        slide,
        flash: flash_root,
    });
    info!(
        "weapon model {} ready: sight {sight:?}, muzzle {muzzle:?}, magazine node {}, slide node {}",
        m.path,
        magazine.is_some(),
        slide.is_some()
    );
}

/// 0 -> 1 -> 0 over `start..end` (smooth), for animation phases.
fn bump(t: f32, start: f32, end: f32) -> f32 {
    let x = ((t - start) / (end - start)).clamp(0.0, 1.0);
    (x * PI).sin()
}

/// 0 -> 1 over `start..end` (smooth).
fn ramp(t: f32, start: f32, end: f32) -> f32 {
    ease((t - start) / (end - start))
}

/// Step length for the current gait (same as the footstep code).
fn stride(state: &CharacterState, tuning: &Tuning) -> f32 {
    let f = &tuning.footsteps;
    if state.sprinting {
        return f.sprint_stride;
    }
    match state.stance {
        Stance::Stand => f.walk_stride,
        Stance::Crouch => f.crouch_stride,
        Stance::Prone => f.prone_stride,
    }
}

type LocalParts<'a> = (
    Entity,
    &'a Transform,
    &'a PlayerInput,
    &'a CharacterState,
    &'a LinearVelocity,
    &'a WeaponState,
    &'a AimView,
);

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn pose_viewmodel(
    player: Single<LocalParts, With<LocalPlayer>>,
    mut camera: Single<
        &mut Transform,
        (With<ViewmodelCamera>, Without<LocalPlayer>, Without<ViewmodelRig>),
    >,
    mut rig: Single<
        &mut Transform,
        (With<ViewmodelRig>, Without<LocalPlayer>, Without<ViewmodelCamera>),
    >,
    mut models: Query<
        (&WeaponModel, &mut Visibility, &mut Transform),
        (Without<LocalPlayer>, Without<ViewmodelCamera>, Without<ViewmodelRig>),
    >,
    mut parts: Query<
        &mut Transform,
        (
            Without<LocalPlayer>,
            Without<ViewmodelCamera>,
            Without<ViewmodelRig>,
            Without<WeaponModel>,
        ),
    >,
    mut anim: ResMut<ViewmodelAnim>,
    mut shots: MessageReader<ShotFired>,
    mut steps: MessageReader<Footstep>,
    tuning: Res<Tuning>,
    time: Res<Time>,
    fixed_time: Res<Time<Fixed>>,
) {
    let (me, body, look, character, velocity, state, aim_view) = player.into_inner();
    let dt = time.delta_secs().max(1e-4);
    let view = &tuning.weapons.view;
    let slot = state.active;
    let def = slot.def(&tuning);

    // --- Camera at the true eye, like the world camera ---------------------
    let (height, lean) = character.interpolated(fixed_time.overstep_fraction());
    let right = Quat::from_rotation_y(look.yaw) * Vec3::X;
    camera.translation = body.translation
        + Vec3::Y * (height - tuning.body.eye_from_top)
        + right * (lean * tuning.lean.offset);
    camera.rotation = Quat::from_euler(
        EulerRot::YXZ,
        look.yaw + aim_view.yaw,
        look.pitch + aim_view.pitch,
        -lean * tuning.lean.angle.to_radians(),
    );

    // --- Shots ----------------------------------------------------------------
    for shot in shots.read().filter(|s| s.shooter == me) {
        anim.kick = (anim.kick + 1.0).min(1.6);
        anim.slide_kick = 1.0;
        anim.flash = tuning.weapons.effects.muzzle_flash_time;
        anim.flash_slot = Some(shot.slot);
        anim.rng = anim.rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        anim.flash_roll = (anim.rng >> 8) as f32 / (1u32 << 24) as f32 * TAU;
        anim.flash_scale = 0.8 + 0.4 * ((anim.rng & 0xff) as f32 / 255.0);
    }
    let settle = |value: f32, rate: f32| value * (-rate * dt).exp();
    anim.kick = settle(anim.kick, 14.0);
    anim.slide_kick = settle(anim.slide_kick, 28.0);

    // --- Sway from mouse look ---------------------------------------------------
    let look_now = Vec2::new(look.yaw, look.pitch);
    let delta = match anim.prev_look {
        Some(prev) => {
            let mut d = look_now - prev;
            // Yaw wraps at 0/360.
            d.x = (d.x + PI).rem_euclid(TAU) - PI;
            d
        }
        None => Vec2::ZERO,
    };
    anim.prev_look = Some(look_now);
    let steadiness = 1.0_f32.lerp(view.ads_steadiness, aim_view.aim);
    // Lags behind the turn: turning right swings the weapon left.
    let target = (-delta * (180.0 / PI) / dt * view.sway).clamp_length_max(view.sway_max);
    anim.sway = target.lerp(anim.sway, (-12.0 * dt).exp());

    // --- Bob, in step with the footsteps ----------------------------------------
    let speed = velocity.0.with_y(0.0).length();
    let bob_target = if character.grounded {
        (speed / tuning.movement.walk_speed).clamp(0.0, 1.3)
    } else {
        0.0
    };
    anim.bob_amount += (bob_target - anim.bob_amount) * (1.0 - (-10.0 * dt).exp());
    anim.bob_cycle = (anim.bob_cycle + speed * dt / stride(character, &tuning)).rem_euclid(2.0);
    if steps.read().any(|step| step.entity == me) {
        anim.bob_cycle = anim.bob_cycle.round().rem_euclid(2.0);
    }
    let side_wave = (PI * anim.bob_cycle).sin() * anim.bob_amount;
    let up_wave = 0.5 * (TAU * anim.bob_cycle).cos() * anim.bob_amount;

    // --- Sprint pose --------------------------------------------------------------
    let sprint_target = if character.sprinting { 1.0 } else { 0.0 };
    anim.sprint += (sprint_target - anim.sprint) * (1.0 - (-8.0 * dt).exp());
    let sprint = ease(anim.sprint);

    // --- Pose ---------------------------------------------------------------------
    let info = models
        .iter()
        .find(|(m, ..)| m.slot == slot)
        .and_then(|(m, ..)| m.info);
    let (sight, _) = sight_and_muzzle(def, info.as_ref());
    let pose = &def.pose;
    let hip = Vec3::new(pose.hip.0, pose.hip.1, -pose.hip.2);
    let ads = Vec3::new(-sight.x, -sight.y, -pose.ads_distance - sight.z);
    let aim = aim_view.aim;

    let mut position = hip.lerp(ads, aim);
    let mut rotation = Quat::IDENTITY;

    // Sway: rotate and shift slightly against the turn.
    let sway = anim.sway * steadiness;
    rotation *= Quat::from_euler(EulerRot::YXZ, sway.x.to_radians(), sway.y.to_radians(), 0.0);
    position += Vec3::new(sway.x, sway.y, 0.0) * (PI / 180.0 * 0.04);

    // Bob: the weapon rides up as the head dips onto each foot.
    let bob = steadiness;
    position += Vec3::new(side_wave * view.bob_side, up_wave * view.bob_height, 0.0) * bob;
    rotation *= Quat::from_rotation_z(side_wave * 0.02 * bob);

    // Sprint: tilt the weapon down and across the body.
    position += Vec3::new(-0.05, -0.04, 0.04) * sprint;
    rotation *= Quat::from_euler(EulerRot::YXZ, 0.55 * sprint, -0.35 * sprint, 0.25 * sprint);

    // Recoil kick: back and muzzle up (less when aimed, to keep the sight
    // picture readable).
    let kick = anim.kick * 1.0_f32.lerp(0.5, aim);
    position.z += pose.kick_back * kick;
    rotation *= Quat::from_rotation_x((pose.kick_up * kick).to_radians());

    // Reload and switching.
    let mut magazine_drop = 0.0;
    let mut slide_back = 0.0;
    match state.action {
        WeaponAction::Reloading { elapsed, duration, empty } => {
            let t = (elapsed / duration.max(1e-3)).clamp(0.0, 1.0);
            // Cant the weapon and bring it in.
            let tilt = bump(t, 0.0, 1.0).powf(0.6);
            // Lift it into view, muzzle up, rolled so the magazine well shows.
            position += Vec3::new(-0.08, 0.1, 0.06) * tilt;
            rotation *= Quat::from_euler(EulerRot::YXZ, 0.35 * tilt, 0.25 * tilt, -0.5 * tilt);
            // Old magazine out (and gone), new one in.
            magazine_drop = if t < 0.55 {
                ramp(t, 0.12, 0.32)
            } else {
                1.0 - ramp(t, 0.55, 0.75)
            };
            // Seat it with a little jolt.
            position.y += 0.012 * bump(t, 0.74, 0.8);
            if empty {
                slide_back = bump(t, 0.82, 0.94);
                position.z += 0.015 * bump(t, 0.82, 0.94);
            }
        }
        WeaponAction::Holstering { elapsed, duration, .. } => {
            let t = ease(elapsed / duration.max(1e-3));
            position.y -= 0.25 * t;
            rotation *= Quat::from_rotation_x(-0.7 * t);
        }
        WeaponAction::Drawing { elapsed, duration } => {
            let t = 1.0 - ease(elapsed / duration.max(1e-3));
            position.y -= 0.25 * t;
            rotation *= Quat::from_rotation_x(-0.7 * t);
        }
        WeaponAction::Ready => {}
    }
    rig.translation = position;
    rig.rotation = rotation;

    // --- Models: show the active one, animate its parts ----------------------------
    for (model, mut visibility, mut transform) in &mut models {
        let wanted = if model.slot == slot {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
        // Scale and rotation from the tuning (applies on save).
        let placed = model_transform(model.slot.def(&tuning));
        if *transform != placed {
            *transform = placed;
        }
    }
    let model = model_transform(def);
    if let Some(info) = info {
        if let Some(part) = info.magazine
            && let Ok(mut transform) = parts.get_mut(part.entity)
        {
            let drop = part_offset(&part, &model, Vec3::NEG_Y * 0.35 * magazine_drop);
            transform.translation = part.home.translation + drop;
            transform.scale = if magazine_drop > 0.95 {
                Vec3::ZERO
            } else {
                part.home.scale
            };
        }
        if let Some(part) = info.slide
            && let Ok(mut transform) = parts.get_mut(part.entity)
        {
            // Locked back on an empty magazine until the reload racks it.
            let locked = state.ammo().magazine == 0
                && !matches!(state.action, WeaponAction::Reloading { elapsed, duration, .. } if elapsed / duration > 0.88);
            let back = if locked { 1.0 } else { anim.slide_kick.max(slide_back) };
            transform.translation = part.home.translation + part_offset(&part, &model, Vec3::Z * 0.022 * back);
        }
    }
}

/// Muzzle flash quads and light, a few frames per shot.
#[allow(clippy::type_complexity)]
fn update_muzzle_flash(
    models: Query<&WeaponModel>,
    mut flashes: Query<(&mut Visibility, &mut Transform, &GlobalTransform), Without<MuzzleFlashLight>>,
    light: Single<(&mut Visibility, &mut Transform, &mut PointLight), With<MuzzleFlashLight>>,
    camera: Single<&GlobalTransform, With<ViewmodelCamera>>,
    spatial: SpatialQuery,
    mut anim: ResMut<ViewmodelAnim>,
    mut events: MessageReader<WeaponEvent>,
    tuning: Res<Tuning>,
    time: Res<Time>,
) {
    // A weapon switch ends any flash.
    if events.read().any(|e| e.kind == WeaponEventKind::Holster) {
        anim.flash = 0.0;
    }
    let showing = anim.flash > 0.0;
    let (mut light_visibility, mut light_transform, mut point) = light.into_inner();
    let mut light_at = None;
    for model in &models {
        let Some(info) = model.info else { continue };
        let Ok((mut visibility, mut transform, global)) = flashes.get_mut(info.flash) else {
            continue;
        };
        let on = showing && anim.flash_slot == Some(model.slot);
        let wanted = if on { Visibility::Inherited } else { Visibility::Hidden };
        if *visibility != wanted {
            *visibility = wanted;
        }
        if on {
            // The flash hangs off the rig; the weapon points down -Z there.
            let (_, muzzle) = sight_and_muzzle(model.slot.def(&tuning), Some(&info));
            transform.translation = muzzle;
            transform.rotation = Quat::from_rotation_z(anim.flash_roll);
            transform.scale = Vec3::splat(anim.flash_scale);
            light_at = Some(global.translation());
        }
    }
    anim.flash = (anim.flash - time.delta_secs()).max(0.0);

    // The light goes at the muzzle, but never through a wall in front of you.
    let wanted = match light_at {
        Some(muzzle) => {
            let eye = camera.translation();
            let to_muzzle = muzzle - eye;
            let distance = to_muzzle.length();
            let mut at = muzzle;
            if let Ok(dir) = Dir3::new(to_muzzle) {
                let filter = SpatialQueryFilter::from_mask(GameLayer::World);
                if let Some(hit) = spatial.cast_ray(eye, dir, distance, true, &filter) {
                    at = eye + *dir * (hit.distance - 0.1).max(0.0);
                }
            }
            light_transform.translation = at;
            point.intensity = tuning.weapons.effects.muzzle_light;
            Visibility::Inherited
        }
        None => Visibility::Hidden,
    };
    if *light_visibility != wanted {
        *light_visibility = wanted;
    }
}

/// Keep the weapon-layer sun aligned with the world sun, and dim it in shade.
#[allow(clippy::type_complexity)]
fn follow_sun(
    world_sun: Query<&GlobalTransform, (With<DirectionalLight>, Without<ViewmodelSun>)>,
    sun: Single<(&mut Transform, &mut DirectionalLight), With<ViewmodelSun>>,
    camera: Single<&GlobalTransform, With<ViewmodelCamera>>,
    spatial: SpatialQuery,
    mut anim: ResMut<ViewmodelAnim>,
    tuning: Res<Tuning>,
    time: Res<Time>,
) {
    let (mut transform, mut light) = sun.into_inner();
    let Some(world) = world_sun.iter().next() else {
        return;
    };
    transform.rotation = world.rotation();
    let to_sun = world.back();
    let filter = SpatialQueryFilter::from_mask(GameLayer::World);
    let lit = spatial
        .cast_ray(camera.translation(), to_sun, 500.0, true, &filter)
        .is_none();
    let target = if lit { 1.0 } else { 0.0 };
    anim.sun += (target - anim.sun) * (1.0 - (-8.0 * time.delta_secs()).exp());
    let illuminance = tuning.lighting.sun * anim.sun;
    if (light.illuminance - illuminance).abs() > 1.0 {
        light.illuminance = illuminance;
    }
}
