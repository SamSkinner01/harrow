//! `--net-trace`: once a second, log what this instance sees (session,
//! round phase and clock, the local player, every other networked player,
//! and on clients the prediction rollback counters). Used by
//! `tools/scripts/net_test.sh` to check that instances agree.

use std::time::Duration;

use bevy::{prelude::*, time::common_conditions::on_timer};
use lightyear::prelude::PredictionMetrics;

use super::NetPlayer;
use crate::{
    combat::{Dead, Health, Team},
    player::LocalPlayer,
    round::Round,
    session::{AppState, NetMode},
};
use crate::{
    destruction::SurfaceChanged,
    weapons::{BulletHit, ShotFired},
};

pub(super) struct NetTracePlugin;

impl Plugin for NetTracePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Counts>().add_systems(
            Update,
            (
                count,
                trace.run_if(on_timer(Duration::from_secs(1))),
                // Mesh data is only in the main world without a renderer.
                footprint
                    .run_if(|mode: Res<NetMode>| matches!(*mode, NetMode::Dedicated { .. }))
                    .run_if(on_timer(Duration::from_secs(10))),
            )
                .chain(),
        );
    }
}

/// Things seen since startup: shots (simulated on the server, forwarded to
/// clients), hits, and destruction (cells knocked out, on every machine).
#[derive(Resource, Default)]
struct Counts {
    shots: u32,
    hits: u32,
    cells: u32,
}

fn count(
    mut counts: ResMut<Counts>,
    mut shots: MessageReader<ShotFired>,
    mut hits: MessageReader<BulletHit>,
    mut changes: MessageReader<SurfaceChanged>,
) {
    counts.shots += shots.read().count() as u32;
    counts.hits += hits.read().count() as u32;
    counts.cells += changes.read().map(|c| c.removed.len() as u32).sum::<u32>();
}

/// Every 10 s on a dedicated server: what the world holds (memory checks).
fn footprint(
    entities: Query<()>,
    meshes: Res<Assets<Mesh>>,
    images: Res<Assets<Image>>,
    server: Res<AssetServer>,
) {
    let mut big: Vec<(usize, String)> = images
        .iter()
        .map(|(id, i)| {
            let path = server.get_path(id).map_or_else(|| format!("{id:?}"), |p| p.to_string());
            (i.data.as_ref().map_or(0, Vec::len), format!("{path} {}x{}", i.width(), i.height()))
        })
        .collect();
    big.sort_by(|a, b| b.0.cmp(&a.0));
    for (bytes, what) in big.iter().take(6) {
        debug!("net-trace: image {} KB {what}", bytes / 1024);
    }
    let mesh_bytes: usize = meshes
        .iter()
        .map(|(_, m)| {
            m.attributes().map(|(_, v)| v.get_bytes().len()).sum::<usize>()
                + m.indices().map_or(0, |i| i.len() * 4)
        })
        .sum();
    let image_bytes: usize = images.iter().map(|(_, i)| i.data.as_ref().map_or(0, Vec::len)).sum();
    info!(
        "net-trace: footprint: {} entities, {} meshes ({} MB), {} images ({} MB)",
        entities.iter().count(),
        meshes.len(),
        mesh_bytes / (1 << 20),
        images.len(),
        image_bytes / (1 << 20)
    );
}

#[allow(clippy::type_complexity)]
fn trace(
    state: Res<State<AppState>>,
    mode: Res<NetMode>,
    round: Res<Round>,
    local: Query<(&Transform, Option<&Team>, Option<&Health>), With<LocalPlayer>>,
    others: Query<(&Transform, &NetPlayer, Option<&Team>, Has<Dead>), Without<LocalPlayer>>,
    // Per-player state is only ever sent to its own player (`PrivateTo`):
    // on a client this must stay 0.
    private: Query<(), (With<NetPlayer>, Without<LocalPlayer>, With<crate::gadgets::SecondaryStock>)>,
    metrics: Option<Res<PredictionMetrics>>,
    server: Option<Res<super::client::ServerInfo>>,
    counts: Res<Counts>,
) {
    let me = local
        .single()
        .map(|(t, team, health)| {
            format!(
                "me {:?} hp {:.0} at ({:.2}, {:.2}, {:.2})",
                team,
                health.map_or(0.0, |h| h.current),
                t.translation.x,
                t.translation.y,
                t.translation.z
            )
        })
        .unwrap_or_else(|_| "no local player".into());
    let mut players: Vec<String> = others
        .iter()
        .map(|(t, player, team, dead)| {
            format!(
                "{}{} {:?} at ({:.2}, {:.2}, {:.2})",
                player.name,
                if dead { " (dead)" } else { "" },
                team,
                t.translation.x,
                t.translation.y,
                t.translation.z
            )
        })
        .collect();
    players.sort();
    let rollbacks = metrics
        .map(|m| format!(" | rollbacks {} ({} ticks)", m.rollbacks, m.rollback_ticks))
        .unwrap_or_default();
    let joined = server
        .map(|s| format!(" on \"{}\" ({})", s.name, s.address))
        .unwrap_or_default();
    info!(
        "net-trace: {:?} {:?}{} | round {} {:?} {:.1}s score {}-{} | {} | others [{}] | shots {} hits {} cells {}{}{}",
        state.get(),
        *mode,
        joined,
        round.number,
        round.phase,
        round.time_left,
        round.score_blue,
        round.score_red,
        me,
        players.join("; "),
        counts.shots,
        counts.hits,
        counts.cells,
        rollbacks,
        if matches!(*mode, NetMode::Client { .. }) {
            format!(" | others' private state {}", private.iter().count())
        } else {
            String::new()
        }
    );
}
