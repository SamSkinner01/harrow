//! First-person arms: gloved, sleeved forearms and hands holding the weapon
//! in your hands. Presentation only, drawn with the weapon by the weapon
//! camera (render layer [`VIEWMODEL_LAYER`]).
//!
//! The arms are built by `tools/scripts/build_arms.sh` (Blender + MPFB, the
//! operators' own skeleton, uniform and glove colours) into
//! `assets/models/characters/arms_{attackers,defenders}.glb`; you see the
//! pair for your team. Each file has three clips per weapon, named after
//! the weapon's `model.arms` in the tuning file:
//!
//! - `<arms>_hold`: both hands on the weapon,
//! - `<arms>_reload`, `<arms>_reload_empty`: the support hand takes the
//!   magazine out, fetches a new one and seats it (and racks the charging
//!   handle or slide on an empty reload).
//!
//! The rig node's `harrow_arms` extra says where the arms' scene sits
//! relative to each weapon's grip origin. The arms are a child of the weapon
//! rig, so aiming, sway, bob, recoil, sprinting, switching and the reload
//! tilt all move them with the weapon. The reload clips follow the same
//! magazine path as `viewmodel.rs` and are scrubbed to the reload's
//! progress, so the left hand carries the magazine at any reload time.

use std::collections::HashMap;

use bevy::{
    camera::visibility::{NoFrustumCulling, RenderLayers},
    gltf::{Gltf, GltfAssetLabel, GltfNode},
    light::NotShadowCaster,
    prelude::*,
    world_serialization::{WorldAssetRoot, WorldInstanceReady},
};

use super::{
    WeaponAction, WeaponState,
    viewmodel::{VIEWMODEL_LAYER, ViewmodelRig},
};
use crate::{
    characters::CharacterMaterials,
    combat::Team,
    config::Tuning,
    player::LocalPlayer,
};

pub(super) struct ArmsPlugin;

impl Plugin for ArmsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load_arms)
            .add_observer(prepare_arms)
            .add_systems(Update, (spawn_arms, build_rigs, drive_arms).chain());
    }
}

/// Arms per team.
const FILES: [(Team, &str); 2] = [
    (Team::Attackers, "models/characters/arms_attackers.glb"),
    (Team::Defenders, "models/characters/arms_defenders.glb"),
];

#[derive(Resource)]
struct ArmsAssets {
    files: Vec<ArmsFile>,
}

struct ArmsFile {
    team: Team,
    path: &'static str,
    gltf: Handle<Gltf>,
    rig: Option<ArmsRig>,
    failed: bool,
}

/// One arms file's animation graph, clips and weapon placements.
struct ArmsRig {
    graph: Handle<AnimationGraph>,
    /// Clip name -> (graph node, length in seconds).
    clips: HashMap<String, (AnimationNodeIndex, f32)>,
    /// `model.arms` name -> the arms' transform relative to the weapon's
    /// grip origin.
    placements: HashMap<String, Transform>,
}

/// The root of one team's arms scene (child of the weapon rig).
#[derive(Component)]
struct ArmsModel {
    file: usize,
    /// The entity with the `AnimationPlayer`, once the scene has spawned.
    player: Option<Entity>,
    bound: bool,
}

fn load_arms(mut commands: Commands, server: Res<AssetServer>) {
    let files = FILES
        .iter()
        .map(|&(team, path)| ArmsFile {
            team,
            path,
            gltf: server.load(path),
            rig: None,
            failed: false,
        })
        .collect();
    commands.insert_resource(ArmsAssets { files });
}

/// Once the weapon rig exists, put both teams' arms on it (hidden until
/// they're posed).
fn spawn_arms(
    mut commands: Commands,
    rig: Option<Single<Entity, With<ViewmodelRig>>>,
    assets: Res<ArmsAssets>,
    server: Res<AssetServer>,
    mut spawned: Local<bool>,
) {
    let Some(rig) = rig else { return };
    if *spawned {
        return;
    }
    *spawned = true;
    for (index, file) in assets.files.iter().enumerate() {
        commands.spawn((
            Name::new(format!("First-person arms {:?}", file.team)),
            ArmsModel {
                file: index,
                player: None,
                bound: false,
            },
            CharacterMaterials,
            WorldAssetRoot(server.load(GltfAssetLabel::Scene(0).from_asset(file.path))),
            Transform::default(),
            Visibility::Hidden,
            RenderLayers::layer(VIEWMODEL_LAYER),
            ChildOf(*rig),
        ));
    }
}

/// Weapon layer, no shadows, and no frustum culling (a skinned mesh's
/// bounds are those of its rest pose, which is nowhere near the posed arms).
fn prepare_arms(
    ready: On<WorldInstanceReady>,
    mut models: Query<&mut ArmsModel>,
    children: Query<&Children>,
    players: Query<(), With<AnimationPlayer>>,
    meshes: Query<(), With<Mesh3d>>,
    mut commands: Commands,
) {
    let Ok(mut model) = models.get_mut(ready.entity) else {
        return;
    };
    let layer = RenderLayers::layer(VIEWMODEL_LAYER);
    for entity in children.iter_descendants(ready.entity) {
        commands.entity(entity).insert(layer.clone());
        if meshes.contains(entity) {
            commands.entity(entity).insert((NotShadowCaster, NoFrustumCulling));
        }
        if players.contains(entity) {
            model.player = Some(entity);
        }
    }
    if model.player.is_none() {
        error!("first-person arms: no animation player in the model");
    }
}

fn build_rigs(
    mut assets: ResMut<ArmsAssets>,
    server: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    nodes: Res<Assets<GltfNode>>,
    clips: Res<Assets<AnimationClip>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    for file in &mut assets.files {
        if file.rig.is_some() || file.failed || !server.is_loaded_with_dependencies(&file.gltf) {
            continue;
        }
        let Some(gltf) = gltfs.get(&file.gltf) else { continue };
        let mut graph = AnimationGraph::new();
        let root = graph.root;
        let mut indices = HashMap::new();
        for (name, handle) in &gltf.named_animations {
            let length = clips.get(handle).map_or(0.0, |c| c.duration());
            indices.insert(name.to_string(), (graph.add_clip(handle.clone(), 1.0, root), length));
        }
        let placements = gltf
            .named_nodes
            .get("rig")
            .and_then(|h| nodes.get(h))
            .and_then(|n| n.extras.as_ref())
            .map(|e| read_placements(&e.value))
            .unwrap_or_default();
        if placements.is_empty() {
            error!("{}: no harrow_arms placements on the rig node", file.path);
            file.failed = true;
            continue;
        }
        info!(
            "{}: {} clips, arms for {:?}",
            file.path,
            indices.len(),
            placements.keys().collect::<Vec<_>>()
        );
        file.rig = Some(ArmsRig {
            graph: graphs.add(graph),
            clips: indices,
            placements,
        });
    }
}

/// The rig's `harrow_arms` extra (JSON, written by
/// `tools/blender/characters/arms.py`): per weapon, the arms' translation
/// and rotation (x, y, z, w) relative to the weapon's grip origin.
fn read_placements(extras: &str) -> HashMap<String, Transform> {
    let mut out = HashMap::new();
    let outer: HashMap<String, ron::Value> = match ron::from_str(extras) {
        Ok(v) => v,
        Err(err) => {
            warn!("arms: could not read the rig's extras: {err}");
            return out;
        }
    };
    let Some(ron::Value::String(text)) = outer.get("harrow_arms") else {
        return out;
    };
    let parsed: HashMap<String, HashMap<String, Vec<f32>>> = match ron::from_str(text) {
        Ok(v) => v,
        Err(err) => {
            warn!("arms: could not read harrow_arms: {err}");
            return out;
        }
    };
    for (weapon, fields) in parsed {
        let (Some(t), Some(r)) = (fields.get("translation"), fields.get("rotation")) else {
            continue;
        };
        if t.len() != 3 || r.len() != 4 {
            continue;
        }
        let rotation = Quat::from_xyzw(r[0], r[1], r[2], r[3]).normalize();
        out.insert(
            weapon,
            Transform::from_xyz(t[0], t[1], t[2]).with_rotation(rotation),
        );
    }
    out
}

/// Every frame: show your team's arms, placed for the weapon in hand, in
/// the hold pose or scrubbed through the reload.
fn drive_arms(
    local: Option<Single<(&WeaponState, Option<&Team>), With<LocalPlayer>>>,
    mut models: Query<(&mut ArmsModel, &mut Transform, &mut Visibility)>,
    mut players: Query<&mut AnimationPlayer>,
    assets: Res<ArmsAssets>,
    tuning: Res<Tuning>,
    mut commands: Commands,
) {
    let Some(local) = local else { return };
    let (state, team) = local.into_inner();
    let team = team.copied().unwrap_or(Team::Attackers);
    let def = state.active.def(&tuning);
    for (mut model, mut transform, mut visibility) in &mut models {
        let file = &assets.files[model.file];
        let rig = file.rig.as_ref();
        let arms = def.model.arms.as_deref();
        let placement = rig.zip(arms).and_then(|(rig, arms)| rig.placements.get(arms));
        let show = file.team == team && placement.is_some() && model.player.is_some();
        let wanted = if show { Visibility::Inherited } else { Visibility::Hidden };
        if *visibility != wanted {
            *visibility = wanted;
        }
        let (Some(rig), Some(arms), Some(placement), Some(player_entity)) =
            (rig, arms, placement, model.player)
        else {
            continue;
        };
        if *transform != *placement {
            *transform = *placement;
        }
        let Ok(mut player) = players.get_mut(player_entity) else {
            continue;
        };
        if !model.bound {
            commands
                .entity(player_entity)
                .insert(AnimationGraphHandle(rig.graph.clone()));
            for &(node, _) in rig.clips.values() {
                player.play(node).set_weight(0.0).pause();
            }
            model.bound = true;
        }
        // Which clip, and where in it.
        let (clip, at) = match state.action {
            WeaponAction::Reloading { elapsed, duration, empty } => (
                if empty { format!("{arms}_reload_empty") } else { format!("{arms}_reload") },
                (elapsed / duration.max(1e-3)).clamp(0.0, 1.0),
            ),
            _ => (format!("{arms}_hold"), 0.0),
        };
        let wanted = rig.clips.get(&clip).or_else(|| rig.clips.get(&format!("{arms}_hold")));
        for (name, &(node, length)) in &rig.clips {
            let on = wanted.is_some_and(|&(n, _)| n == node);
            if let Some(active) = player.animation_mut(node) {
                active.set_weight(if on { 1.0 } else { 0.0 });
                if on {
                    let time = if name.ends_with("_hold") { 0.0 } else { at * length };
                    active.seek_to(time);
                }
            }
        }
    }
}
