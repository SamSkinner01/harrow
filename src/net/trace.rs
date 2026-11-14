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

pub(super) struct NetTracePlugin;

impl Plugin for NetTracePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, trace.run_if(on_timer(Duration::from_secs(1))));
    }
}

#[allow(clippy::type_complexity)]
fn trace(
    state: Res<State<AppState>>,
    mode: Res<NetMode>,
    round: Res<Round>,
    local: Query<(&Transform, Option<&Team>, Option<&Health>), With<LocalPlayer>>,
    others: Query<(&Transform, &NetPlayer, Option<&Team>, Has<Dead>), Without<LocalPlayer>>,
    metrics: Option<Res<PredictionMetrics>>,
    server: Option<Res<super::client::ServerInfo>>,
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
        "net-trace: {:?} {:?}{} | round {} {:?} {:.1}s score {}-{} | {} | others [{}]{}",
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
        rollbacks
    );
}
