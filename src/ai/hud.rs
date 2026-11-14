//! Kill feed (everyone's kills, top left) and team callouts (your team's
//! bots calling out what they see and hear, under it). Presentation only.
//! Every kill and callout is also logged.

use bevy::prelude::*;

use super::senses::Callout;
use crate::{
    combat::{Died, Team},
    player::LocalPlayer,
    round::{RoundEvent, operators::Operator},
    weapons::{BulletHit, Slot, WeaponState},
};

pub struct AiHudPlugin;

impl Plugin for AiHudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Feeds>()
            .add_systems(Startup, spawn_feeds)
            .add_systems(Update, (collect_kills, collect_callouts, show_feeds).chain());
    }
}

const KILLS_SHOWN: usize = 5;
const CALLOUTS_SHOWN: usize = 4;
const KILL_TIME: f32 = 7.0;
const CALLOUT_TIME: f32 = 5.0;
const FRIEND: Color = Color::srgb(0.55, 0.82, 1.0);
const FOE: Color = Color::srgb(1.0, 0.62, 0.38);
const TEXT: Color = Color::srgba(1.0, 1.0, 1.0, 0.9);

struct Line {
    text: String,
    color: Color,
    at: f32,
}

#[derive(Resource, Default)]
struct Feeds {
    kills: Vec<Line>,
    callouts: Vec<Line>,
    /// Kills per round, for the log.
    pub round_kills: u32,
}

#[derive(Component)]
struct KillFeed;

#[derive(Component)]
struct CalloutFeed;

fn spawn_feeds(mut commands: Commands) {
    let column = |top: f32| Node {
        position_type: PositionType::Absolute,
        left: px(12.0),
        top: px(top),
        flex_direction: FlexDirection::Column,
        row_gap: px(3.0),
        ..default()
    };
    commands.spawn((Name::new("Kill feed"), KillFeed, column(44.0), Pickable::IGNORE));
    commands.spawn((Name::new("Callout feed"), CalloutFeed, column(190.0), Pickable::IGNORE));
}

/// "Name (OPERATOR)", or "You".
fn label(entity: Entity, names: &Query<(&Name, Option<&Operator>, Has<LocalPlayer>)>) -> String {
    match names.get(entity) {
        Ok((_, _, true)) => "You".into(),
        Ok((name, Some(op), _)) => format!("{} ({})", name.as_str(), op.0.info().callsign),
        Ok((name, None, _)) => name.as_str().to_string(),
        Err(_) => "?".into(),
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_kills(
    mut died: MessageReader<Died>,
    mut hits: MessageReader<BulletHit>,
    mut events: MessageReader<RoundEvent>,
    names: Query<(&Name, Option<&Operator>, Has<LocalPlayer>)>,
    teams: Query<&Team>,
    weapons: Query<&WeaponState>,
    local: Query<&Team, With<LocalPlayer>>,
    tuning: Res<crate::config::Tuning>,
    mut feeds: ResMut<Feeds>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs();
    let lethal: Vec<BulletHit> = hits.read().filter(|h| h.lethal).cloned().collect();
    let my_team = local.single().ok().copied();
    for event in events.read() {
        if let RoundEvent::PhaseStarted { phase: crate::round::RoundPhase::OperatorSelect, .. } = event {
            feeds.round_kills = 0;
            feeds.kills.clear();
        }
    }
    for death in died.read() {
        // Only characters (gadgets die too).
        if !names.contains(death.entity) || teams.get(death.entity).is_err() {
            continue;
        }
        let victim = label(death.entity, &names);
        let hit = lethal.iter().find(|h| h.target == death.entity);
        let (killer, weapon) = match death.killer {
            Some(k) if k == death.entity => ("".to_string(), "fell".to_string()),
            Some(k) => {
                let slot = weapons.get(k).map(|w| w.active).unwrap_or(Slot::Primary);
                let weapon = if hit.is_some() { slot.def(&tuning).name.clone() } else { "melee/blast".into() };
                (label(k, &names), weapon)
            }
            None => (String::new(), "down".into()),
        };
        let head = if hit.is_some_and(|h| h.zone == Some(crate::player::HitZone::Head)) { "  HEADSHOT" } else { "" };
        let friendly = death.killer.is_some_and(|k| teams.get(k).ok() == teams.get(death.entity).ok());
        let text = if killer.is_empty() {
            format!("{victim} {weapon}")
        } else if friendly {
            format!("{killer}  [{weapon}]  {victim}  (team kill){head}")
        } else {
            format!("{killer}  [{weapon}]  {victim}{head}")
        };
        info!("kill: {text}");
        feeds.round_kills += 1;
        let victim_team = teams.get(death.entity).ok().copied();
        let color = if victim_team == my_team { FOE } else { FRIEND };
        feeds.kills.push(Line { text, color, at: now });
        if feeds.kills.len() > KILLS_SHOWN {
            feeds.kills.remove(0);
        }
    }
}

fn collect_callouts(
    mut callouts: MessageReader<Callout>,
    names: Query<&Name>,
    local: Query<&Team, With<LocalPlayer>>,
    mut feeds: ResMut<Feeds>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs();
    let my_team = local.single().ok().copied();
    for c in callouts.read() {
        let who = c.from.and_then(|e| names.get(e).ok()).map(|n| format!("{}: ", n.as_str())).unwrap_or_default();
        info!("callout ({:?}): {who}{}", c.team, c.text);
        if Some(c.team) != my_team {
            continue;
        }
        feeds.callouts.push(Line { text: format!("{who}{}", c.text), color: TEXT, at: now });
        if feeds.callouts.len() > CALLOUTS_SHOWN {
            feeds.callouts.remove(0);
        }
    }
}

fn show_feeds(
    mut feeds: ResMut<Feeds>,
    kill_feed: Single<Entity, With<KillFeed>>,
    callout_feed: Single<Entity, (With<CalloutFeed>, Without<KillFeed>)>,
    children: Query<&Children>,
    time: Res<Time>,
    mut shown: Local<(usize, usize, u32)>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs();
    let before = (feeds.kills.len(), feeds.callouts.len());
    feeds.kills.retain(|l| now - l.at < KILL_TIME);
    feeds.callouts.retain(|l| now - l.at < CALLOUT_TIME);
    // Rebuild the lines only when something changed.
    let stamp = feeds.kills.last().map_or(0, |l| l.at.to_bits()) ^ feeds.callouts.last().map_or(0, |l| l.at.to_bits());
    let key = (feeds.kills.len(), feeds.callouts.len(), stamp);
    if key == *shown && before == (feeds.kills.len(), feeds.callouts.len()) {
        return;
    }
    *shown = key;
    for (root, lines) in [(*kill_feed, &feeds.kills), (*callout_feed, &feeds.callouts)] {
        if let Ok(old) = children.get(root) {
            for child in old.iter() {
                commands.entity(child).despawn();
            }
        }
        for line in lines.iter() {
            commands.spawn((
                ChildOf(root),
                Text::new(line.text.clone()),
                TextFont {
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextColor(line.color),
                Node {
                    padding: UiRect::axes(px(6.0), px(2.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
            ));
        }
    }
}
