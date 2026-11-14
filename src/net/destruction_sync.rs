//! Destruction (M4) on clients: the server's surface edits, replayed.
//!
//! M4's destruction is deterministic: a surface's cells depend only on the
//! edits applied to it, in order (bullet/melee/explosion impacts, hard
//! breaches, reinforcement) from the intact state of the round. So instead
//! of sending cell grids, the server forwards every edit it applied
//! ([`SurfaceEdit`]) and every round reset, in order, on the reliable event
//! channel. Clients feed them into the same M4 messages (`SurfaceImpact`,
//! `HardBreach`, `ReinforceRequest`, `RoundReset`), so the same holes open
//! and the same debris and dust appear. Late joiners get the edit log since
//! the last round reset.
//!
//! Surfaces are named by their placement ([`SurfaceKey`]: position to the
//! millimetre plus the piece name), which is the same on every machine
//! because every machine builds the map from the same kit; entity ids are
//! not.
//!
//! On clients the destruction *actions* (melee, reinforcing, charges,
//! explosions) don't run; their results arrive as edits. Not covered yet:
//! barricades defenders put up and placed breach charges are entities the
//! server spawns at runtime and aren't replicated (their effects on walls
//! are).

use std::hash::{Hash, Hasher};

use bevy::{platform::collections::HashMap, prelude::*};
use lightyear::prelude::{server::ClientOf, *};
use serde::{Deserialize, Serialize};

use super::{client::NetClient, protocol::EventChannel};
use crate::{
    combat::{ImpactKind, SurfaceImpact, SurfaceKind},
    destruction::{HardBreach, ReinforceRequest, surface_root},
    round::RoundReset,
    session::{AppState, is_client, is_server},
};

pub(super) struct DestructionSyncPlugin;

impl Plugin for DestructionSyncPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EditLog>()
            .add_systems(
                PostUpdate,
                (record_edits, catch_up_new_clients)
                    .chain()
                    .run_if(is_server)
                    .before(MessageSystems::Send),
            )
            .add_systems(
                PreUpdate,
                apply_edits.run_if(is_client).after(MessageSystems::Receive),
            )
            .add_systems(OnExit(AppState::InGame), clear_log);
    }
}

/// Which surface: where it stands (millimetres) and what piece it is.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SurfaceKey {
    pub position: IVec3,
    pub name_hash: u64,
}

impl SurfaceKey {
    fn of(transform: &GlobalTransform, name: &Name) -> Self {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        name.as_str().hash(&mut hasher);
        Self {
            position: (transform.translation() * 1000.0).round().as_ivec3(),
            name_hash: hasher.finish(),
        }
    }
}

/// One change to the destructible world, as applied by the server.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum SurfaceEdit {
    /// A new round: every surface back to intact.
    Reset { round: u32 },
    Impact {
        surface: SurfaceKey,
        point: Vec3,
        normal: Vec3,
        direction: Vec3,
        kind: ImpactKind,
    },
    HardBreach {
        surface: SurfaceKey,
        centre: Vec3,
        radius: f32,
    },
    Reinforce { surface: SurfaceKey, from: Vec3 },
}

/// Edits since the last round reset (server), for late joiners.
#[derive(Resource, Default)]
struct EditLog(Vec<SurfaceEdit>);

type ReadyLink = (With<ClientOf>, With<ReplicationSender>);

/// Server: turn this frame's edits into [`SurfaceEdit`]s, log and send them.
#[allow(clippy::too_many_arguments)]
fn record_edits(
    mut resets: MessageReader<RoundReset>,
    mut impacts: MessageReader<SurfaceImpact>,
    mut breaches: MessageReader<HardBreach>,
    mut reinforcements: MessageReader<ReinforceRequest>,
    colliders: Query<&avian3d::prelude::ColliderOf>,
    surfaces: Query<(&GlobalTransform, &Name), With<SurfaceKind>>,
    mut log: ResMut<EditLog>,
    mut links: Query<&mut MessageSender<SurfaceEdit>, ReadyLink>,
) {
    let key = |entity: Entity| {
        let root = surface_root(entity, &colliders);
        surfaces.get(root).ok().map(|(t, n)| SurfaceKey::of(t, n))
    };
    let mut edits = Vec::new();
    if let Some(reset) = resets.read().last() {
        log.0.clear();
        edits.push(SurfaceEdit::Reset { round: reset.round });
    }
    for impact in impacts.read() {
        if let Some(surface) = key(impact.collider) {
            edits.push(SurfaceEdit::Impact {
                surface,
                point: impact.point,
                normal: impact.normal,
                direction: impact.direction,
                kind: impact.kind,
            });
        }
    }
    for breach in breaches.read() {
        if let Some(surface) = key(breach.surface) {
            edits.push(SurfaceEdit::HardBreach {
                surface,
                centre: breach.centre,
                radius: breach.radius,
            });
        }
    }
    for request in reinforcements.read() {
        if let Some(surface) = key(request.surface) {
            edits.push(SurfaceEdit::Reinforce {
                surface,
                from: request.from,
            });
        }
    }
    if edits.is_empty() {
        return;
    }
    for mut sender in &mut links {
        for edit in &edits {
            sender.send::<EventChannel>(edit.clone());
        }
    }
    log.0.extend(edits.into_iter().filter(|e| !matches!(e, SurfaceEdit::Reset { .. })));
}

/// Server: a client that just joined gets the round's edits so far.
fn catch_up_new_clients(
    log: Res<EditLog>,
    mut links: Query<&mut MessageSender<SurfaceEdit>, (With<ClientOf>, Added<ReplicationSender>)>,
) {
    for mut sender in &mut links {
        for edit in &log.0 {
            sender.send::<EventChannel>(edit.clone());
        }
        if !log.0.is_empty() {
            info!("net: sent {} destruction edits to a new client", log.0.len());
        }
    }
}

fn clear_log(mut log: ResMut<EditLog>) {
    log.0.clear();
}

/// Client: replay the server's edits through the destruction module.
#[allow(clippy::too_many_arguments)]
fn apply_edits(
    mut links: Query<&mut MessageReceiver<SurfaceEdit>, With<NetClient>>,
    surfaces: Query<(Entity, &GlobalTransform, &Name), With<SurfaceKind>>,
    mut index: Local<(usize, HashMap<SurfaceKey, Entity>)>,
    mut resets: MessageWriter<RoundReset>,
    mut impacts: MessageWriter<SurfaceImpact>,
    mut breaches: MessageWriter<HardBreach>,
    mut reinforcements: MessageWriter<ReinforceRequest>,
) {
    for mut receiver in &mut links {
        for edit in receiver.receive() {
            let mut find = |key: &SurfaceKey| {
                // (Re)build the key -> entity index when surfaces appear.
                let count = surfaces.iter().len();
                if index.0 != count || !index.1.contains_key(key) {
                    index.0 = count;
                    index.1.clear();
                    index
                        .1
                        .extend(surfaces.iter().map(|(e, t, n)| (SurfaceKey::of(t, n), e)));
                }
                let found = index.1.get(key).copied();
                if found.is_none() {
                    debug!("net: no local surface for {key:?}");
                }
                found
            };
            match edit {
                SurfaceEdit::Reset { round } => {
                    resets.write(RoundReset { round });
                }
                SurfaceEdit::Impact {
                    surface,
                    point,
                    normal,
                    direction,
                    kind,
                } => {
                    if let Some(collider) = find(&surface) {
                        impacts.write(SurfaceImpact {
                            collider,
                            point,
                            normal,
                            direction,
                            kind,
                            source: None,
                        });
                    }
                }
                SurfaceEdit::HardBreach {
                    surface,
                    centre,
                    radius,
                } => {
                    if let Some(entity) = find(&surface) {
                        breaches.write(HardBreach {
                            surface: entity,
                            centre,
                            radius,
                            source: None,
                        });
                    }
                }
                SurfaceEdit::Reinforce { surface, from } => {
                    if let Some(entity) = find(&surface) {
                        reinforcements.write(ReinforceRequest {
                            surface: entity,
                            from,
                            by: None,
                        });
                    }
                }
            }
        }
    }
}
