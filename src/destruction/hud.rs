//! Presentation for the local player: a progress bar while reinforcing,
//! barricading or placing a charge, a one-line gadget count, and a small
//! camera jab when swinging melee (applied on top of the player camera after
//! it has been placed, so the camera code is untouched).

use bevy::{prelude::*, transform::TransformSystems};

use super::{Activity, BreachCharge, BreachCharges, Reinforcements, melee::MeleeState};
use crate::{combat::Team, player::LocalPlayer};

pub struct DestructionHudPlugin;

impl Plugin for DestructionHudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_hud)
            .add_systems(Update, update_hud)
            .add_systems(
                PostUpdate,
                melee_jab.before(TransformSystems::Propagate),
            );
    }
}

#[derive(Component)]
struct StatusText;

#[derive(Component)]
struct ProgressBar;

#[derive(Component)]
struct ProgressFill;

#[derive(Component)]
struct GadgetText;

fn spawn_hud(mut commands: Commands) {
    let font = |size: f32| TextFont {
        font_size: FontSize::Px(size),
        ..default()
    };
    commands.spawn((
        Name::new("Destruction HUD"),
        Node {
            position_type: PositionType::Absolute,
            bottom: px(28.0),
            width: percent(100.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(6.0),
            ..default()
        },
        Pickable::IGNORE,
        children![
            (StatusText, Text::new(""), font(16.0), TextColor(Color::srgba(1.0, 1.0, 1.0, 0.9))),
            (
                ProgressBar,
                Node {
                    width: px(220.0),
                    height: px(6.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
                Visibility::Hidden,
                children![(
                    ProgressFill,
                    Node {
                        width: percent(0.0),
                        height: percent(100.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.95, 0.85, 0.55, 0.95)),
                )],
            ),
            (GadgetText, Text::new(""), font(13.0), TextColor(Color::srgba(1.0, 1.0, 1.0, 0.7))),
        ],
    ));
}

#[allow(clippy::type_complexity)]
fn update_hud(
    player: Query<(Entity, &Activity, &Reinforcements, &BreachCharges, &Team), With<LocalPlayer>>,
    charges: Query<&BreachCharge>,
    mut status: Query<&mut Text, (With<StatusText>, Without<GadgetText>)>,
    mut gadget: Query<&mut Text, (With<GadgetText>, Without<StatusText>)>,
    mut bar: Query<&mut Visibility, With<ProgressBar>>,
    mut fill: Query<&mut Node, With<ProgressFill>>,
) {
    let Ok((entity, activity, reinforcements, breach, team)) = player.single() else {
        return;
    };
    let (label, progress) = match *activity {
        Activity::Idle => ("", None),
        Activity::Reinforcing { progress, .. } => ("Reinforcing", Some(progress)),
        Activity::Barricading { progress, .. } => ("Barricading", Some(progress)),
        Activity::PlacingCharge { progress, .. } => ("Placing breach charge", Some(progress)),
    };
    for mut text in &mut status {
        if text.0 != label {
            text.0 = label.to_string();
        }
    }
    for mut visibility in &mut bar {
        visibility.set_if_neq(if progress.is_some() { Visibility::Inherited } else { Visibility::Hidden });
    }
    if let Some(p) = progress {
        for mut node in &mut fill {
            node.width = percent(p.clamp(0.0, 1.0) * 100.0);
        }
    }
    let line = match team {
        Team::Defenders => format!(
            "V melee   F reinforce ({} left) / barricade",
            reinforcements.0
        ),
        Team::Attackers if charges.iter().any(|c| c.owner == entity) => {
            "V melee   4 detonate breach charge".to_string()
        }
        Team::Attackers => format!("V melee   4 (hold) breach charge ({} left)", breach.0),
    };
    for mut text in &mut gadget {
        if text.0 != line {
            text.0 = line.clone();
        }
    }
}

/// Seconds a melee jab lasts.
const JAB_TIME: f32 = 0.22;

/// A quick forward jab and tilt of the camera on every swing.
fn melee_jab(
    player: Query<&MeleeState, With<LocalPlayer>>,
    // The player camera (not a child camera such as a weapon view).
    mut camera: Query<&mut Transform, (With<Camera3d>, Without<ChildOf>)>,
    time: Res<Time>,
    mut jab: Local<(u32, f32)>,
) {
    let Ok(melee) = player.single() else { return };
    if melee.swings != jab.0 {
        *jab = (melee.swings, 0.0);
    } else {
        jab.1 += time.delta_secs();
    }
    if jab.0 == 0 || jab.1 >= JAB_TIME {
        return;
    }
    let x = jab.1 / JAB_TIME;
    // Fast out, slower back.
    let k = if x < 0.3 { x / 0.3 } else { 1.0 - (x - 0.3) / 0.7 };
    let k = k * k * (3.0 - 2.0 * k);
    for mut transform in &mut camera {
        let forward = transform.forward();
        transform.translation += forward * (0.07 * k);
        transform.rotation *= Quat::from_rotation_x(-0.045 * k) * Quat::from_rotation_z(0.03 * k);
    }
}
