//! Bodies for other players: the M5 operator models, animated from the
//! networked state.
//!
//! Every networked character that isn't ours (on a client: interpolated
//! remote players; on a host: the clients' characters) gets its operator's
//! model as a child (feet at the character's origin, facing its yaw), and
//! [`CharacterAnim`] is driven every frame from replicated/interpolated
//! state: ground speed, stance, lean. The model follows the round's
//! operator choice (one default per side before anyone picks) and hides
//! while the character is dead.
//!
//! Hook for later: aiming/firing/reloading come from M3's replicated weapon
//! state; weapons attach to the model's `weapon_r` bone (`CharacterParts`);
//! head pitch can come from the replicated `NetAim`.

use bevy::prelude::*;

use super::NetPlayer;
use crate::{
    characters::{Character, CharacterAnim, Operator as Model, spawn_character},
    combat::{Dead, Team},
    player::{CharacterState, LocalPlayer},
    round::operators::{Operator, OperatorId},
};

pub(super) struct BodiesPlugin;

impl Plugin for BodiesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (attach_bodies, animate_bodies).chain());
    }
}

/// On a character: its model entity and which operator it shows.
#[derive(Component)]
struct RemoteBody {
    model: Entity,
    shows: Model,
}

fn model_for(operator: Option<&Operator>, team: Option<&Team>) -> Model {
    match (operator.map(|o| o.0), team) {
        (Some(OperatorId::Auger), _) => Model::Auger,
        (Some(OperatorId::Lark), _) => Model::Lark,
        (Some(OperatorId::Mortise), _) => Model::Mortise,
        (Some(OperatorId::Fathom), _) => Model::Fathom,
        (None, Some(Team::Defenders)) => Model::Mortise,
        (None, _) => Model::Auger,
    }
}

/// Give remote characters a model, swap it when their operator changes,
/// and remove it if the character turns out to be ours.
#[allow(clippy::type_complexity)]
fn attach_bodies(
    characters: Query<
        (
            Entity,
            Option<&Operator>,
            Option<&Team>,
            Option<&RemoteBody>,
            Has<LocalPlayer>,
        ),
        With<NetPlayer>,
    >,
    assets: Res<AssetServer>,
    mut commands: Commands,
) {
    for (entity, operator, team, body, local) in &characters {
        if local {
            if let Some(body) = body {
                commands.entity(body.model).try_despawn();
                commands.entity(entity).remove::<RemoteBody>();
            }
            continue;
        }
        let wanted = model_for(operator, team);
        if body.is_some_and(|b| b.shows == wanted) {
            continue;
        }
        if let Some(body) = body {
            commands.entity(body.model).try_despawn();
        }
        let model = spawn_character(&mut commands, &assets, wanted, Transform::IDENTITY);
        commands.entity(model).insert(ChildOf(entity));
        commands.entity(entity).insert((
            RemoteBody {
                model,
                shows: wanted,
            },
            Visibility::default(),
        ));
    }
}

/// Drive the animation graph from the networked state.
#[allow(clippy::type_complexity)]
fn animate_bodies(
    characters: Query<(&RemoteBody, &Transform, Option<&CharacterState>, Has<Dead>)>,
    mut models: Query<(&mut CharacterAnim, &mut Visibility), With<Character>>,
    mut last: Local<bevy::platform::collections::HashMap<Entity, Vec3>>,
    time: Res<Time>,
) {
    let dt = time.delta_secs().max(1e-4);
    for (body, transform, state, dead) in &characters {
        let Ok((mut anim, mut visibility)) = models.get_mut(body.model) else {
            continue;
        };
        visibility.set_if_neq(if dead {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        });
        // Ground speed from how far the (interpolated) body moved.
        let at = transform.translation;
        let previous = last.insert(body.model, at).unwrap_or(at);
        let speed = (at - previous).with_y(0.0).length() / dt;
        anim.speed = anim.speed + (speed - anim.speed) * (1.0 - (-12.0 * dt).exp());
        if let Some(state) = state {
            anim.stance = state.stance;
            anim.lean = state.lean;
        }
    }
}
