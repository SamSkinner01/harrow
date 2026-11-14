//! Gameplay events for presentation, forwarded from the server.
//!
//! The HUD and sounds react to [`RoundEvent`] and [`GadgetEvent`]
//! messages, and the intel display to [`Reveal`]s. On a client the
//! simulation that writes them doesn't run, so the server forwards each one
//! (reliable, in order) and the client writes it locally: presentation code
//! works unchanged. Entity references are mapped to the client's entities.
//! Reveals only go to the team they are for.

use bevy::{ecs::entity::MapEntities, prelude::*};
use lightyear::prelude::{server::ClientOf, *};

use super::{client::NetClient, protocol::EventChannel, server::ClientInfo};
use crate::{
    combat::Team,
    gadgets::{GadgetEvent, Reveal},
    round::RoundEvent,
    session::{is_client, is_server},
    weapons::{BulletHit, ShotFired, WeaponEvent},
};

pub(super) struct EventsPlugin;

impl Plugin for EventsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            (
                forward_events,
                forward::<ShotFired>,
                forward::<WeaponEvent>,
                forward::<BulletHit>,
            )
                .run_if(is_server)
                .before(MessageSystems::Send),
        )
        .add_systems(
            PreUpdate,
            (
                receive::<RoundEvent>,
                receive::<GadgetEvent>,
                receive::<Reveal>,
                receive::<ShotFired>,
                receive::<WeaponEvent>,
                receive::<BulletHit>,
            )
                .run_if(is_client)
                .after(MessageSystems::Receive),
        );
    }
}

type ReadyLink = (With<ClientOf>, With<ReplicationSender>);

#[allow(clippy::type_complexity)]
fn forward_events(
    mut round_events: MessageReader<RoundEvent>,
    mut gadget_events: MessageReader<GadgetEvent>,
    mut reveals: MessageReader<Reveal>,
    mut links: Query<
        (
            &ClientInfo,
            &mut MessageSender<RoundEvent>,
            &mut MessageSender<GadgetEvent>,
            &mut MessageSender<Reveal>,
        ),
        ReadyLink,
    >,
    teams: Query<&Team>,
) {
    let round: Vec<_> = round_events.read().cloned().collect();
    let gadget: Vec<_> = gadget_events.read().cloned().collect();
    let intel: Vec<_> = reveals.read().cloned().collect();
    if round.is_empty() && gadget.is_empty() && intel.is_empty() {
        return;
    }
    for (info, mut round_sender, mut gadget_sender, mut reveal_sender) in &mut links {
        for event in &round {
            round_sender.send::<EventChannel>(event.clone());
        }
        for event in &gadget {
            gadget_sender.send::<EventChannel>(event.clone());
        }
        let team = info.character.and_then(|c| teams.get(c).ok());
        for reveal in intel.iter().filter(|r| Some(&r.for_team) == team) {
            reveal_sender.send::<EventChannel>(reveal.clone());
        }
    }
}

/// Forward every `M` to all clients.
fn forward<M: bevy::prelude::Message + Clone>(
    mut messages: MessageReader<M>,
    mut links: Query<&mut MessageSender<M>, ReadyLink>,
) {
    let all: Vec<M> = messages.read().cloned().collect();
    if all.is_empty() {
        return;
    }
    for mut sender in &mut links {
        for message in &all {
            sender.send::<EventChannel>(message.clone());
        }
    }
}

/// Write every received `M` locally. (A link only gets a receiver for a
/// message type once the first one arrives, hence one system per type.)
fn receive<M: bevy::prelude::Message>(
    mut links: Query<&mut MessageReceiver<M>, With<NetClient>>,
    mut writer: MessageWriter<M>,
) {
    for mut receiver in &mut links {
        writer.write_batch(receiver.receive());
    }
}

fn map(entity: &mut Entity, mapper: &mut impl EntityMapper) {
    *entity = mapper.get_mapped(*entity);
}

fn map_opt(entity: &mut Option<Entity>, mapper: &mut impl EntityMapper) {
    if let Some(entity) = entity {
        map(entity, mapper);
    }
}

impl MapEntities for RoundEvent {
    fn map_entities<E: EntityMapper>(&mut self, mapper: &mut E) {
        match self {
            RoundEvent::OperatorLocked { entity, .. }
            | RoundEvent::DefuserAssigned { entity }
            | RoundEvent::DefuserEquipped { entity, .. }
            | RoundEvent::PlantRefused { entity, .. }
            | RoundEvent::PlantStarted { entity, .. }
            | RoundEvent::PlantInterrupted { entity, .. }
            | RoundEvent::DisableStarted { entity }
            | RoundEvent::DisableInterrupted { entity, .. }
            | RoundEvent::Disabled { entity }
            | RoundEvent::DefuserPickedUp { entity } => map(entity, mapper),
            RoundEvent::Planted { entity, .. } => map_opt(entity, mapper),
            RoundEvent::PhaseStarted { .. }
            | RoundEvent::DefuserDropped { .. }
            | RoundEvent::RoundWon { .. }
            | RoundEvent::MatchWon { .. } => {}
        }
    }
}

impl MapEntities for GadgetEvent {
    fn map_entities<E: EntityMapper>(&mut self, mapper: &mut E) {
        match self {
            GadgetEvent::DartFired { by }
            | GadgetEvent::StakePlaced { by, .. }
            | GadgetEvent::Refused { by, .. } => map(by, mapper),
            GadgetEvent::StakePing { stake, .. } | GadgetEvent::StakeSaturated { stake } => {
                map(stake, mapper)
            }
            GadgetEvent::Destroyed { entity, .. } => map(entity, mapper),
            GadgetEvent::DartStuck { .. }
            | GadgetEvent::DartLost
            | GadgetEvent::DartExpired { .. } => {}
        }
    }
}

impl MapEntities for ShotFired {
    fn map_entities<E: EntityMapper>(&mut self, mapper: &mut E) {
        map(&mut self.shooter, mapper);
    }
}

impl MapEntities for WeaponEvent {
    fn map_entities<E: EntityMapper>(&mut self, mapper: &mut E) {
        map(&mut self.shooter, mapper);
    }
}

impl MapEntities for BulletHit {
    fn map_entities<E: EntityMapper>(&mut self, mapper: &mut E) {
        map(&mut self.shooter, mapper);
        map(&mut self.target, mapper);
    }
}

impl MapEntities for Reveal {
    fn map_entities<E: EntityMapper>(&mut self, mapper: &mut E) {
        map_opt(&mut self.revealed, mapper);
    }
}
