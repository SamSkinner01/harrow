//! What bots notice, and what each team knows.
//!
//! - **Sight** (every `senses.sight_interval` per bot, staggered): enemies
//!   inside the bot's view cone (`fov`) and `sight_range`, checked with a
//!   ray from the bot's eye to each of the enemy's hit zones (torso, head,
//!   legs) against world geometry. Walls (soft or stone), floors, closed
//!   barricades and smoke block sight; holes and gaps don't. Someone within
//!   2.5 m is noticed whatever the angle.
//! - **Hearing** (every tick): footsteps ([`Footstep`]) and other sounds
//!   ([`Noise`]: gunfire, reloads, gadgets, explosions) within
//!   `loudness * hearing_range * difficulty.hearing` metres, less on another
//!   floor. Hearing gives a position, not a line of sight.
//! - **Pain**: being shot tells you roughly where from.
//! - **Team knowledge** ([`TeamIntel`]): sightings, sounds, stake pings and
//!   dart reveals ([`Reveal`]) are shared with the whole team for a few
//!   seconds. New contacts become [`Callout`]s ("Enemy spotted: upper
//!   floor").

use avian3d::prelude::*;
use bevy::{platform::collections::HashMap, prelude::*};

use super::{AiSettings, AiSystems, Bot, BotState, angle_diff, angles_to, eye_position};
use crate::{
    combat::{DamageMessage, Dead, Noise, NoiseKind, Team},
    config::Tuning,
    gadgets::{Reveal, RevealSource, SmokeCloud, smoke_blocks},
    physics::GameLayer,
    player::{CharacterState, Footstep, HitZone, Player, PlayerInput},
    round::{Round, RoundPhase, sites::KeepLayout},
};

pub struct SensesPlugin;

impl Plugin for SensesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TeamIntel>()
            .init_resource::<ZoneCache>()
            .add_message::<Callout>()
            .add_systems(
                FixedPreUpdate,
                (cache_zones, forget, hear, feel, take_reveals, see)
                    .chain()
                    .in_set(AiSystems::Senses),
            );
    }
}

/// How a team learned about an enemy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContactKind {
    Seen,
    Heard,
    /// Gunfire heard.
    Shot,
    /// A stake ping or dart reveal.
    Ping,
}

#[derive(Clone, Copy, Debug)]
pub struct Contact {
    pub entity: Option<Entity>,
    pub pos: Vec3,
    /// Fixed-clock seconds.
    pub time: f32,
    pub kind: ContactKind,
}

/// What each team knows about the other (last known positions).
#[derive(Resource, Default)]
pub struct TeamIntel {
    known: [Vec<Contact>; 2],
    /// Last callout time per (team, enemy entity or area).
    called: HashMap<(usize, u64), f32>,
}

fn side(team: Team) -> usize {
    match team {
        Team::Attackers => 0,
        Team::Defenders => 1,
    }
}

impl TeamIntel {
    pub fn contacts(&self, team: Team) -> &[Contact] {
        &self.known[side(team)]
    }

    /// Record a contact; returns true if it's news (not known recently).
    pub fn report(&mut self, team: Team, contact: Contact, news_age: f32) -> bool {
        let list = &mut self.known[side(team)];
        let existing = list.iter_mut().find(|c| match (c.entity, contact.entity) {
            (Some(a), Some(b)) => a == b,
            (None, None) => c.pos.distance(contact.pos) < 2.0,
            _ => false,
        });
        match existing {
            Some(c) => {
                let news = contact.time - c.time > news_age;
                // A sighting beats a sound for the same time.
                if contact.kind == ContactKind::Seen || c.kind != ContactKind::Seen || contact.time - c.time > 1.0 {
                    *c = contact;
                } else {
                    c.time = contact.time;
                }
                news
            }
            None => {
                list.push(contact);
                true
            }
        }
    }

    pub fn clear(&mut self) {
        for list in &mut self.known {
            list.clear();
        }
        self.called.clear();
    }

    /// True (and remembered) if `key` wasn't called out by `team` within
    /// `repeat` seconds.
    fn should_call(&mut self, team: Team, key: u64, now: f32, repeat: f32) -> bool {
        let slot = self.called.entry((side(team), key)).or_insert(f32::MIN);
        if now - *slot < repeat {
            return false;
        }
        *slot = now;
        true
    }
}

/// A team message (HUD feed for the local player's team, and the log).
#[derive(Message, Clone, Debug)]
pub struct Callout {
    pub team: Team,
    pub from: Option<Entity>,
    pub text: String,
}

/// One enemy a bot can see right now.
#[derive(Clone, Copy, Debug)]
pub struct Sighting {
    pub entity: Entity,
    pub feet: Vec3,
    /// Visible zone centres (head, torso, legs).
    pub zones: [Option<Vec3>; 3],
    pub distance: f32,
    /// At the edge of the view when first noticed.
    pub peripheral: bool,
}

/// Hit zone positions of every character this tick: [head, torso, legs].
#[derive(Resource, Default)]
pub struct ZoneCache {
    pub zones: HashMap<Entity, [Vec3; 3]>,
}

pub fn zone_index(zone: HitZone) -> usize {
    match zone {
        HitZone::Head => 0,
        HitZone::Torso => 1,
        HitZone::Legs => 2,
    }
}

fn cache_zones(mut cache: ResMut<ZoneCache>, zones: Query<(&HitZone, &ChildOf, &GlobalTransform)>) {
    cache.zones.clear();
    for (zone, parent, gt) in &zones {
        let entry = cache.zones.entry(parent.parent()).or_insert([Vec3::ZERO; 3]);
        entry[zone_index(*zone)] = gt.translation();
    }
}

/// Drop stale and dead contacts.
fn forget(
    mut intel: ResMut<TeamIntel>,
    dead: Query<(), With<Dead>>,
    tuning: Res<Tuning>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs();
    let s = &tuning.ai.senses;
    for list in &mut intel.known {
        list.retain(|c| {
            let max = if c.kind == ContactKind::Seen { s.remember_seen } else { s.remember_heard };
            now - c.time <= max && c.entity.is_none_or(|e| !dead.contains(e))
        });
    }
}

/// A sound one bot heard.
#[derive(Clone, Copy, Debug)]
pub struct Heard {
    pub pos: Vec3,
    pub time: f32,
    pub loudness: f32,
    pub source: Option<Entity>,
}

#[allow(clippy::too_many_arguments)]
fn hear(
    mut steps: MessageReader<Footstep>,
    mut noises: MessageReader<Noise>,
    mut bots: Query<(&Transform, &Team, &mut BotState), (With<Bot>, Without<Dead>)>,
    teams: Query<&Team, With<Player>>,
    mut intel: ResMut<TeamIntel>,
    mut callouts: MessageWriter<Callout>,
    layout: Option<Res<KeepLayout>>,
    spatial: SpatialQuery,
    settings: Res<AiSettings>,
    tuning: Res<Tuning>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs();
    let s = &tuning.ai.senses;
    let hearing = tuning.ai.preset(settings.level(&tuning)).hearing;
    let mut sounds: Vec<(Option<Entity>, Vec3, f32, bool)> = steps
        .read()
        .map(|f| (Some(f.entity), f.position + Vec3::Y * 0.2, f.loudness, false))
        .collect();
    sounds.extend(noises.read().map(|n| {
        (n.source, n.position, n.loudness, matches!(n.kind, NoiseKind::Gunfire | NoiseKind::Explosion))
    }));
    if sounds.is_empty() {
        return;
    }
    for (source, pos, loudness, gunfire) in sounds {
        let source_team = source.and_then(|e| teams.get(e).ok()).copied();
        let mut heard_by: [bool; 2] = [false; 2];
        for (transform, team, mut state) in &mut bots {
            if source_team == Some(*team) || source.is_some() && source_team.is_none() {
                continue;
            }
            let feet = transform.translation;
            let mut range = loudness * s.hearing_range * hearing;
            if (pos.y - feet.y).abs() > 2.2 {
                range *= s.other_floor_hearing;
            }
            if pos.distance(feet) > range {
                continue;
            }
            let louder = state.senses.heard.is_none_or(|h| now - h.time > 1.0 || loudness >= h.loudness);
            if louder {
                state.senses.heard = Some(Heard { pos, time: now, loudness, source });
            }
            heard_by[side(*team)] = true;
        }
        // Team knowledge from what was heard.
        for team in [Team::Attackers, Team::Defenders] {
            if !heard_by[side(team)] || source_team.is_none() {
                continue;
            }
            let kind = if gunfire { ContactKind::Shot } else { ContactKind::Heard };
            let news = intel.report(team, Contact { entity: source, pos, time: now, kind }, s.callout_repeat);
            if news && loudness >= 0.45 {
                let area = area_name(pos, layout.as_deref(), &spatial);
                let key = area_key(&area);
                if intel.should_call(team, key, now, s.callout_repeat) {
                    let what = if gunfire { "Gunfire" } else { "Movement heard" };
                    callouts.write(Callout { team, from: None, text: format!("{what}: {area}") });
                }
            }
        }
    }
}

/// Being shot: you know roughly where it came from.
fn feel(
    mut damage: MessageReader<DamageMessage>,
    mut bots: Query<(&Team, &mut BotState), With<Bot>>,
    sources: Query<(&Transform, &Team), With<Player>>,
    mut intel: ResMut<TeamIntel>,
    tuning: Res<Tuning>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs();
    for hit in damage.read() {
        let Ok((team, mut state)) = bots.get_mut(hit.target) else { continue };
        let Some(source) = hit.source else { continue };
        let Ok((at, source_team)) = sources.get(source) else { continue };
        if source_team == team {
            continue;
        }
        state.senses.hurt = Some((source, at.translation, now));
        let contact = Contact { entity: Some(source), pos: at.translation, time: now, kind: ContactKind::Shot };
        intel.report(*team, contact, tuning.ai.senses.callout_repeat);
    }
}

/// Stake pings and dart reveals go to the team they're for.
fn take_reveals(
    mut reveals: MessageReader<Reveal>,
    mut intel: ResMut<TeamIntel>,
    mut callouts: MessageWriter<Callout>,
    layout: Option<Res<KeepLayout>>,
    spatial: SpatialQuery,
    tuning: Res<Tuning>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs();
    let repeat = tuning.ai.senses.callout_repeat;
    for reveal in reveals.read() {
        let contact = Contact { entity: reveal.revealed, pos: reveal.position, time: now, kind: ContactKind::Ping };
        let news = intel.report(reveal.for_team, contact, repeat);
        let key = reveal.revealed.map_or(0, |e| e.to_bits());
        if news && intel.should_call(reveal.for_team, key ^ 0x5157, now, repeat) {
            let area = area_name(reveal.position, layout.as_deref(), &spatial);
            let what = match reveal.source {
                RevealSource::SeismicStake => "Stake ping",
                RevealSource::SensorDart => "Dart picked up movement",
                RevealSource::ProximityAlarm => "Proximity alarm",
            };
            callouts.write(Callout { team: reveal.for_team, from: None, text: format!("{what}: {area}") });
        }
    }
}

/// Line of sight to enemies, staggered between bots.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn see(
    mut bots: Query<
        (Entity, &mut Bot, &mut BotState, &Transform, &CharacterState, &PlayerInput, &Team),
        Without<Dead>,
    >,
    enemies: Query<(Entity, &Transform, &Team), (With<Player>, Without<Dead>)>,
    clouds: Query<&SmokeCloud>,
    cache: Res<ZoneCache>,
    mut intel: ResMut<TeamIntel>,
    mut callouts: MessageWriter<Callout>,
    layout: Option<Res<KeepLayout>>,
    spatial: SpatialQuery,
    settings: Res<AiSettings>,
    tuning: Res<Tuning>,
    time: Res<Time>,
    round: Res<Round>,
) {
    let now = time.elapsed_secs();
    if !(round.phase.is_live() || round.phase == RoundPhase::Inactive) {
        for (_, _, mut state, ..) in &mut bots {
            state.senses.visible.clear();
        }
        return;
    }
    let d = tuning.ai.preset(settings.level(&tuning));
    let interval = tuning.ai.senses.sight_interval.max(0.02);
    let filter = SpatialQueryFilter::from_mask(GameLayer::World);
    let half_fov = (d.fov * 0.5).to_radians();
    for (me, mut bot, mut state, transform, body, input, team) in &mut bots {
        if now < bot.next_sense {
            continue;
        }
        // Stagger: spread bots over the interval.
        bot.next_sense = if bot.next_sense == 0.0 {
            now + interval * (bot.index % 8) as f32 / 8.0
        } else {
            now + interval
        };
        let eye = eye_position(transform.translation, input.yaw, body.height, body.lean, &tuning);
        let previous: Vec<(Entity, bool)> =
            state.senses.visible.iter().map(|s| (s.entity, s.peripheral)).collect();
        state.senses.visible.clear();
        for (enemy, at, enemy_team) in &enemies {
            if enemy == me || enemy_team == team {
                continue;
            }
            let Some(zones) = cache.zones.get(&enemy) else { continue };
            // Not placed yet (first frames).
            if zones[1] == Vec3::ZERO {
                continue;
            }
            let to = zones[1] - eye;
            let distance = to.length();
            if distance > d.sight_range {
                continue;
            }
            let (yaw, pitch) = angles_to(to);
            let off = angle_diff(yaw, input.yaw).abs();
            let in_view = off <= half_fov && (pitch - input.pitch).abs() <= 1.0;
            if !in_view && distance > 2.5 {
                continue;
            }
            let mut visible = [None; 3];
            for i in [1, 0, 2] {
                let target = zones[i];
                let delta = target - eye;
                let Ok(dir) = Dir3::new(delta) else { continue };
                let length = delta.length();
                let blocked = spatial
                    .cast_ray(eye, dir, length, true, &filter)
                    .is_some_and(|hit| hit.distance < length - 0.12)
                    || smoke_blocks(eye, target, clouds.iter(), &tuning.round.gadgets.smoke);
                if !blocked {
                    visible[i] = Some(target);
                }
            }
            if visible.iter().all(Option::is_none) {
                continue;
            }
            let peripheral = previous
                .iter()
                .find(|(e, _)| *e == enemy)
                .map_or(off > 35f32.to_radians(), |(_, p)| *p);
            state.senses.visible.push(Sighting {
                entity: enemy,
                feet: at.translation,
                zones: visible,
                distance,
                peripheral,
            });
            let contact = Contact { entity: Some(enemy), pos: at.translation, time: now, kind: ContactKind::Seen };
            let repeat = tuning.ai.senses.callout_repeat;
            let news = intel.report(*team, contact, repeat);
            if news && intel.should_call(*team, enemy.to_bits(), now, repeat) {
                let area = area_name(at.translation, layout.as_deref(), &spatial);
                // One "spotted" per area at a time, however many there are.
                if !intel.should_call(*team, area_key(&area) ^ 0x51, now, repeat * 0.5) {
                    continue;
                }
                callouts.write(Callout {
                    team: *team,
                    from: Some(me),
                    text: format!("Enemy spotted: {area}"),
                });
            }
        }
        state.senses.visible.sort_by(|a, b| a.distance.total_cmp(&b.distance));
    }
}

/// A rough, map-independent name for a place: a bomb site, near one, or
/// the floor (outside / ground floor / upper floor / roof) and direction.
pub fn area_name(pos: Vec3, layout: Option<&KeepLayout>, spatial: &SpatialQuery) -> String {
    if let Some(layout) = layout {
        if let Some(site) = layout.site_at(pos + Vec3::Y * 0.1) {
            return format!("site {} ({})", site.id.letter(), site.name.to_lowercase());
        }
        for site in &layout.sites {
            let c = site.centre();
            if (pos.y - c.y).abs() < 1.6 && pos.with_y(0.0).distance(c.with_y(0.0)) < 6.5 {
                return format!("near site {}", site.id.letter());
            }
        }
    }
    let filter = SpatialQueryFilter::from_mask(GameLayer::World);
    let covered = spatial
        .cast_ray(pos + Vec3::Y * 1.0, Dir3::Y, 12.0, true, &filter)
        .is_some();
    let centre = layout
        .map(|l| {
            let sum: Vec3 = l.sites.iter().map(|s| s.centre()).sum();
            sum / l.sites.len().max(1) as f32
        })
        .unwrap_or(Vec3::ZERO);
    if !covered {
        if pos.y > 4.5 {
            return "roof".into();
        }
        let d = pos - centre;
        let dir = if d.x.abs() > d.z.abs() {
            if d.x > 0.0 { "east" } else { "west" }
        } else if d.z > 0.0 {
            "south"
        } else {
            "north"
        };
        return format!("outside {dir}");
    }
    let floor = if pos.y < 1.6 {
        "ground floor"
    } else if pos.y < 4.8 {
        "upper floor"
    } else {
        "top floor"
    };
    let d = pos - centre;
    let dir = if d.x.abs() > d.z.abs() {
        if d.x > 0.0 { "east" } else { "west" }
    } else if d.z > 0.0 {
        "south"
    } else {
        "north"
    };
    format!("{floor} {dir}")
}

fn area_key(area: &str) -> u64 {
    area.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3))
}
