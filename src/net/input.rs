//! Networked input: what a client sends the server every tick.
//!
//! [`NetInput`] bundles every intent component a character has. Lightyear
//! sends it every tick with redundancy (lost packets are covered by the
//! next ones), buffers it on the server for the tick it belongs to, and
//! replays it on the client during rollbacks.
//!
//! Flow per fixed tick:
//! - **Client**, `WriteClientInputs`: the local player's intent components
//!   (written from the keyboard in `PreUpdate`, exactly as in solo) are
//!   copied into `ActionState<NetInput>`.
//! - **Server**, after `UpdateActionState`: `ActionState<NetInput>` of each
//!   remote character is copied back into its intent components, so the
//!   simulation can't tell a remote player from a local one.
//! - **Client during rollback**: the buffered movement input is restored
//!   into `PlayerInput` for each replayed tick.
//!
//! Intents come in two kinds:
//! - *Held state* (`DestructionIntent`, `PlayerInput`): copied as-is.
//!   Register more with [`sync_intent`] (one line per intent component).
//! - *Latched presses* (M6's `RoundIntent::equip_defuser`,
//!   `GadgetIntent::use_unique`): the simulation clears them once handled,
//!   which a client never does, so they travel as press counters and the
//!   server re-latches when a counter moves. Loss- and repeat-proof.

use bevy::{
    ecs::{component::Mutable, entity::MapEntities},
    prelude::*,
};
use lightyear::{
    input::native::prelude::{ActionState, InputMarker},
    prelude::{
        client::input::InputSystems as ClientInputSystems,
        server::input::InputSystems as ServerInputSystems,
    },
};
use serde::{Deserialize, Serialize};

use super::Replay;
use crate::{
    destruction::DestructionIntent,
    gadgets::GadgetIntent,
    player::{Lean, LocalPlayer, PlayerInput, Stance},
    round::RoundIntent,
    session::is_authority,
    weapons::WeaponInput,
};

/// Everything a player wants this tick.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default, Reflect)]
pub struct NetInput {
    pub movement: MoveInput,
    /// M6 round loop: F, 5, operator picks.
    pub round: RoundButtons,
    /// M6 gadgets: 3.
    pub gadget: GadgetButtons,
    /// M4 destruction: V, F, 4 (held state).
    pub destruction: DestructionIntent,
    /// M3 weapons: trigger, aim, reload, slot (held state and press
    /// counters; the weapon simulation detects presses itself).
    pub weapon: WeaponInput,
    // A new feature's intent: add a field here and one `sync_intent` line
    // in `NetInputPlugin::build`.
}

impl MapEntities for NetInput {
    fn map_entities<E: EntityMapper>(&mut self, _mapper: &mut E) {}
}

/// [`PlayerInput`] on the wire.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default, Reflect)]
pub struct MoveInput {
    pub axis: Vec2,
    pub yaw: f32,
    pub pitch: f32,
    pub sprint: bool,
    pub stance: Stance,
    pub lean: Lean,
}

impl From<&PlayerInput> for MoveInput {
    fn from(input: &PlayerInput) -> Self {
        Self {
            axis: input.move_axis,
            yaw: input.yaw,
            pitch: input.pitch,
            sprint: input.sprint,
            stance: input.stance,
            lean: input.lean,
        }
    }
}

impl MoveInput {
    fn apply(&self, input: &mut PlayerInput) {
        input.move_axis = self.axis;
        input.yaw = self.yaw;
        input.pitch = self.pitch;
        input.sprint = self.sprint;
        input.stance = self.stance;
        input.lean = self.lean;
    }
}

/// [`RoundIntent`] on the wire. Presses are counters (see module docs).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default, Reflect)]
pub struct RoundButtons {
    pub interact: bool,
    pub equip_presses: u8,
    pub pick_presses: u8,
    /// Operator index of the latest pick.
    pub pick: u8,
}

/// [`GadgetIntent`] on the wire.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default, Reflect)]
pub struct GadgetButtons {
    pub unique_presses: u8,
}

/// Server side: the last press counters seen from a client.
#[derive(Component, Default, Debug)]
pub(super) struct SeenPresses {
    equip: u8,
    pick: u8,
    unique: u8,
}

/// Where client input is captured (client, `FixedPreUpdate`).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CaptureInputSystems;

/// Where received input is applied to intent components (`FixedPreUpdate`).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ApplyInputSystems;

pub(super) struct NetInputPlugin;

impl Plugin for NetInputPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(
            FixedPreUpdate,
            (
                CaptureInputSystems.in_set(ClientInputSystems::WriteClientInputs),
                ApplyInputSystems
                    .after(ClientInputSystems::BufferClientInputs)
                    .after(ServerInputSystems::UpdateActionState),
            ),
        )
        .add_systems(
            FixedPreUpdate,
            (
                capture_local_input.in_set(CaptureInputSystems),
                (apply_movement_input, apply_buttons.run_if(is_authority))
                    .in_set(ApplyInputSystems),
            ),
        );
        // Held-state intents: one line each.
        sync_intent::<DestructionIntent>(app, |input| &mut input.destruction);
        sync_intent::<WeaponInput>(app, |input| &mut input.weapon);
    }
}

/// Network a held-state intent component `C` stored in field `field` of
/// [`NetInput`]: captured from the local player on clients, applied to
/// every character with received input on the server.
pub fn sync_intent<C>(app: &mut App, field: fn(&mut NetInput) -> &mut C)
where
    C: Component<Mutability = Mutable> + Clone + PartialEq,
{
    app.add_systems(
        FixedPreUpdate,
        (
            (move |replay: Replay,
                   mut players: Query<
                (&C, &mut ActionState<NetInput>),
                (With<LocalPlayer>, With<InputMarker<NetInput>>),
            >| {
                if replay.active() {
                    return;
                }
                for (intent, mut action) in &mut players {
                    let slot = field(&mut action.0);
                    if *slot != *intent {
                        *slot = intent.clone();
                    }
                }
            })
            .in_set(CaptureInputSystems),
            (move |mut players: Query<
                (&mut C, &mut ActionState<NetInput>),
                Without<InputMarker<NetInput>>,
            >| {
                for (mut intent, mut action) in &mut players {
                    let value = field(&mut action.bypass_change_detection().0);
                    if *intent != *value {
                        *intent = value.clone();
                    }
                }
            })
            .run_if(is_authority)
            .in_set(ApplyInputSystems),
        ),
    );
}

/// Client: the local player's intents -> `ActionState<NetInput>`.
fn capture_local_input(
    replay: Replay,
    mut players: Query<
        (
            &PlayerInput,
            Option<&mut RoundIntent>,
            Option<&mut GadgetIntent>,
            &mut ActionState<NetInput>,
        ),
        (With<LocalPlayer>, With<InputMarker<NetInput>>),
    >,
) {
    // Rollback replays restore buffered input instead.
    if replay.active() {
        return;
    }
    for (input, round, gadget, mut action) in &mut players {
        let net = &mut action.0;
        net.movement = MoveInput::from(input);
        if let Some(mut round) = round {
            net.round.interact = round.interact;
            // The client never simulates the round loop, so it clears the
            // latches itself once they are on the wire.
            if std::mem::take(&mut round.equip_defuser) {
                net.round.equip_presses = net.round.equip_presses.wrapping_add(1);
            }
            if let Some(pick) = round.pick_operator.take() {
                net.round.pick = pick;
                net.round.pick_presses = net.round.pick_presses.wrapping_add(1);
            }
        }
        if let Some(mut gadget) = gadget
            && std::mem::take(&mut gadget.use_unique)
        {
            net.gadget.unique_presses = net.gadget.unique_presses.wrapping_add(1);
        }
    }
}

/// Server: received movement input -> `PlayerInput`. Client: during
/// rollback the restored (buffered) input -> `PlayerInput` of the predicted
/// local player (outside rollbacks this copies the same values back).
fn apply_movement_input(
    replay: Replay,
    mut players: Query<(
        &ActionState<NetInput>,
        &mut PlayerInput,
        Has<InputMarker<NetInput>>,
    )>,
) {
    for (action, mut input, local) in &mut players {
        // Our own input is already in `PlayerInput`, except while replaying.
        if local && !replay.active() {
            continue;
        }
        action.0.movement.apply(&mut input);
    }
}

/// Server: press counters -> latched intents.
fn apply_buttons(
    mut players: Query<
        (
            &ActionState<NetInput>,
            &mut SeenPresses,
            Option<&mut RoundIntent>,
            Option<&mut GadgetIntent>,
        ),
        Without<InputMarker<NetInput>>,
    >,
) {
    for (action, mut seen, round, gadget) in &mut players {
        let net = &action.0;
        if let Some(mut round) = round {
            if round.interact != net.round.interact {
                round.interact = net.round.interact;
            }
            if seen.equip != net.round.equip_presses {
                seen.equip = net.round.equip_presses;
                round.equip_defuser = true;
            }
            if seen.pick != net.round.pick_presses {
                seen.pick = net.round.pick_presses;
                round.pick_operator = Some(net.round.pick);
            }
        }
        if let Some(mut gadget) = gadget
            && seen.unique != net.gadget.unique_presses
        {
            seen.unique = net.gadget.unique_presses;
            gadget.use_unique = true;
        }
    }
}
