//! Loads every operator's .glb at startup and builds one animation graph per
//! operator once it has loaded.
//!
//! The graph is flat: every clip is a child of the root blend node, and
//! clip weights (set per character in `animation.rs`) do all the blending.
//! Bevy normalises the weights per bone over the clips that animate that
//! bone, and masks decide which clips animate which bones:
//!
//! - locomotion clips: the whole body,
//! - `fire`, `reload`: the arms (clavicles to fingers, and the weapon bones)
//!   plus the chest, neck and head (`spine_03`, `neck_01`, `head`: the aim
//!   blades the chest and puts the cheek on the stock; the reload looks at
//!   the magazine),
//! - `lean_left`, `lean_right`: the lower spine only (`spine_01`,
//!   `spine_02`), so a lean and the aim never fight over a bone.

use std::collections::HashMap;

use bevy::{
    animation::AnimationTargetId,
    gltf::{Gltf, GltfNode},
    prelude::*,
};

use crate::combat::Team;

pub(super) struct LibraryPlugin;

impl Plugin for LibraryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CharacterLibrary>()
            .add_systems(Update, build_rigs);
    }
}

/// The four starter operators (see DESIGN.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Operator {
    Auger,
    Lark,
    Mortise,
    Fathom,
}

impl Operator {
    pub const ALL: [Operator; 4] = [
        Operator::Auger,
        Operator::Lark,
        Operator::Mortise,
        Operator::Fathom,
    ];

    /// File and config name (`tools/blender/characters/operators/<id>.toml`).
    pub fn id(self) -> &'static str {
        match self {
            Operator::Auger => "auger",
            Operator::Lark => "lark",
            Operator::Mortise => "mortise",
            Operator::Fathom => "fathom",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Operator::Auger => "Auger",
            Operator::Lark => "Lark",
            Operator::Mortise => "Mortise",
            Operator::Fathom => "Fathom",
        }
    }

    pub fn team(self) -> Team {
        match self {
            Operator::Auger | Operator::Lark => Team::Attackers,
            Operator::Mortise | Operator::Fathom => Team::Defenders,
        }
    }

    pub fn model_path(self) -> String {
        format!("models/characters/{}.glb", self.id())
    }
}

/// Animation clips in every operator's .glb, by glTF animation name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Clip {
    Idle,
    Walk,
    Run,
    CrouchIdle,
    CrouchWalk,
    ProneIdle,
    ProneCrawl,
    LeanLeft,
    LeanRight,
    Fire,
    Reload,
}

impl Clip {
    pub const COUNT: usize = 11;
    pub const ALL: [Clip; Clip::COUNT] = [
        Clip::Idle,
        Clip::Walk,
        Clip::Run,
        Clip::CrouchIdle,
        Clip::CrouchWalk,
        Clip::ProneIdle,
        Clip::ProneCrawl,
        Clip::LeanLeft,
        Clip::LeanRight,
        Clip::Fire,
        Clip::Reload,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Clip::Idle => "idle",
            Clip::Walk => "walk",
            Clip::Run => "run",
            Clip::CrouchIdle => "crouch_idle",
            Clip::CrouchWalk => "crouch_walk",
            Clip::ProneIdle => "prone_idle",
            Clip::ProneCrawl => "prone_crawl",
            Clip::LeanLeft => "lean_left",
            Clip::LeanRight => "lean_right",
            Clip::Fire => "fire",
            Clip::Reload => "reload",
        }
    }

    fn mask(self) -> u64 {
        match self {
            Clip::Fire | Clip::Reload => group(LOWER) | group(SPINE),
            Clip::LeanLeft | Clip::LeanRight => group(LOWER) | group(UPPER) | group(ARMS),
            _ => 0,
        }
    }

    /// Movement speed the game uses this clip at, m/s (if it's locomotion):
    /// the walk, sprint, crouch and prone speeds. Blending between clips is
    /// keyed to these. Also the fallback for the authored speed, which the
    /// `harrow_clips` extra in the .glb overrides.
    pub fn game_speed(self) -> f32 {
        match self {
            Clip::Walk => 3.4,
            Clip::Run => 5.6,
            Clip::CrouchWalk => 1.9,
            Clip::ProneCrawl => 0.8,
            _ => 0.0,
        }
    }
}

// Mask groups. A node with a group's bit set does not animate that group.
const LOWER: u32 = 0;
/// `spine_01`, `spine_02` (lean).
const SPINE: u32 = 1;
const ARMS: u32 = 2;
/// `spine_03`, `neck_01`, `head` (aim and reload, with the arms).
const UPPER: u32 = 3;

fn group(g: u32) -> u64 {
    1 << g
}

/// Which mask group a bone belongs to (same split as `anim.py`).
fn bone_group(name: &str) -> u32 {
    const ARM_PREFIXES: [&str; 10] = [
        "clavicle", "upperarm", "lowerarm", "hand", "thumb", "index", "middle", "ring", "pinky",
        "weapon",
    ];
    if name == "spine_03" || name == "neck_01" || name == "head" {
        UPPER
    } else if name.starts_with("spine") {
        SPINE
    } else if ARM_PREFIXES.iter().any(|p| name.starts_with(p)) {
        ARMS
    } else {
        LOWER
    }
}

/// One operator's animation graph and clip data.
pub(super) struct OperatorRig {
    pub graph: Handle<AnimationGraph>,
    pub nodes: [AnimationNodeIndex; Clip::COUNT],
    /// Authored ground speed per clip (0 if not locomotion).
    pub speeds: [f32; Clip::COUNT],
}

impl OperatorRig {
    pub fn node(&self, clip: Clip) -> AnimationNodeIndex {
        self.nodes[clip as usize]
    }

    pub fn speed(&self, clip: Clip) -> f32 {
        self.speeds[clip as usize]
    }
}

#[derive(Resource)]
pub(super) struct CharacterLibrary {
    gltfs: HashMap<Operator, Handle<Gltf>>,
    pub rigs: HashMap<Operator, OperatorRig>,
}

impl FromWorld for CharacterLibrary {
    fn from_world(world: &mut World) -> Self {
        let server = world.resource::<AssetServer>();
        let gltfs = Operator::ALL
            .iter()
            .map(|&op| (op, server.load(op.model_path())))
            .collect();
        Self {
            gltfs,
            rigs: HashMap::new(),
        }
    }
}

fn build_rigs(
    mut library: ResMut<CharacterLibrary>,
    server: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    nodes: Res<Assets<GltfNode>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    for operator in Operator::ALL {
        if library.rigs.contains_key(&operator) {
            continue;
        }
        let handle = &library.gltfs[&operator];
        if !server.is_loaded_with_dependencies(handle) {
            continue;
        }
        let Some(gltf) = gltfs.get(handle) else {
            continue;
        };
        match build_rig(gltf, &nodes, &mut graphs) {
            Ok(rig) => {
                info!("{}: animation graph ready", operator.name());
                library.rigs.insert(operator, rig);
            }
            Err(err) => {
                error!("{}: {err}", operator.model_path());
                // Don't retry every frame; the model stays in its bind pose.
                library.gltfs.remove(&operator);
                return;
            }
        }
    }
}

fn build_rig(
    gltf: &Gltf,
    nodes: &Assets<GltfNode>,
    graphs: &mut Assets<AnimationGraph>,
) -> Result<OperatorRig, String> {
    let mut graph = AnimationGraph::new();
    let root = graph.root;
    let mut indices = Vec::with_capacity(Clip::COUNT);
    for clip in Clip::ALL {
        let handle = gltf
            .named_animations
            .get(clip.name())
            .ok_or_else(|| format!("missing animation {:?}", clip.name()))?;
        indices.push(graph.add_clip_with_mask(handle.clone(), clip.mask(), 1.0, root));
    }

    // Mask groups need each bone's animation target id, which is the path
    // of node names from the scene root (as the glTF loader builds it).
    let by_id: HashMap<AssetId<GltfNode>, &GltfNode> = gltf
        .nodes
        .iter()
        .filter_map(|h| nodes.get(h).map(|n| (h.id(), n)))
        .collect();
    let children: Vec<AssetId<GltfNode>> = by_id
        .values()
        .flat_map(|n| n.children.iter().map(|c| c.id()))
        .collect();
    let mut stack: Vec<(AssetId<GltfNode>, Vec<Name>)> = by_id
        .keys()
        .filter(|id| !children.contains(id))
        .map(|&id| (id, Vec::new()))
        .collect();
    let mut bones = 0;
    while let Some((id, mut path)) = stack.pop() {
        let Some(node) = by_id.get(&id) else { continue };
        path.push(Name::new(node.name.clone()));
        let target = AnimationTargetId::from_names(path.iter());
        graph.add_target_to_mask_group(target, bone_group(&node.name));
        bones += 1;
        for child in &node.children {
            stack.push((child.id(), path.clone()));
        }
    }
    if bones < 50 {
        return Err(format!(
            "only {bones} nodes found; is this a character .glb?"
        ));
    }

    let mut speeds = Clip::ALL.map(Clip::game_speed);
    if let Some(extras) = gltf
        .named_nodes
        .get("rig")
        .and_then(|h| nodes.get(h))
        .and_then(|n| n.extras.as_ref())
    {
        read_clip_speeds(&extras.value, &mut speeds);
    }

    Ok(OperatorRig {
        graph: graphs.add(graph),
        nodes: indices.try_into().expect("one node per clip"),
        speeds,
    })
}

/// Reads authored speeds from the rig's `harrow_clips` extra (written by
/// `tools/blender/characters/anim.py`). glTF extras are JSON, which RON
/// can read as maps.
fn read_clip_speeds(extras: &str, speeds: &mut [f32; Clip::COUNT]) {
    let outer: HashMap<String, ron::Value> = match ron::from_str(extras) {
        Ok(v) => v,
        Err(err) => {
            warn!("could not read the rig's extras: {err}");
            return;
        }
    };
    let Some(ron::Value::String(clips)) = outer.get("harrow_clips") else {
        return;
    };
    let clips: HashMap<String, HashMap<String, ron::Value>> = match ron::from_str(clips) {
        Ok(v) => v,
        Err(err) => {
            warn!("could not read harrow_clips: {err}");
            return;
        }
    };
    for clip in Clip::ALL {
        if let Some(ron::Value::Number(n)) = clips.get(clip.name()).and_then(|c| c.get("speed")) {
            speeds[clip as usize] = n.into_f64() as f32;
        }
    }
}
