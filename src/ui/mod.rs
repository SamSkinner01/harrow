//! HUD and menus. For now: a centre dot and a key-help panel (F1).

use bevy::prelude::*;

use crate::{player::LocalPlayer, weapons::AimView};

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (spawn_crosshair, spawn_help))
            .add_systems(Update, (toggle_help, hide_crosshair_when_aiming));
    }
}

fn spawn_crosshair(mut commands: Commands) {
    // Full-screen container that centres the dot.
    commands.spawn((
        Name::new("Crosshair"),
        Crosshair,
        Node {
            width: percent(100.0),
            height: percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        Pickable::IGNORE,
        children![(
            // Dark outline so the dot reads on bright sand and dark interiors.
            Node {
                width: px(6.0),
                height: px(6.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            children![(
                Node {
                    width: px(4.0),
                    height: px(4.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.9)),
            )],
        )],
    ));
}

#[derive(Component)]
struct Crosshair;

/// The sight replaces the centre dot when aiming down sights.
fn hide_crosshair_when_aiming(
    aim: Option<Single<&AimView, With<LocalPlayer>>>,
    mut crosshair: Single<&mut Visibility, With<Crosshair>>,
) {
    let aiming = aim.is_some_and(|a| a.aim > 0.5);
    let wanted = if aiming { Visibility::Hidden } else { Visibility::Inherited };
    if **crosshair != wanted {
        **crosshair = wanted;
    }
}

#[derive(Component)]
struct HelpPanel;

const HELP: &str = "\
WASD  move        Shift  sprint
C  crouch         Z  prone
Q / E  lean       Esc  pause menu
LMB  fire         RMB  aim
R  reload         1 / 2  weapons
V  melee          F  interact (hold)
3  gadget         4  secondary gadget
5  defuser        G  drones / cameras
F1  this help     F3  FPS
F4  hit zones     F5  third person
F12  screenshot";

fn spawn_help(mut commands: Commands) {
    commands.spawn((
        Name::new("Help panel"),
        HelpPanel,
        Text::new(HELP),
        TextFont {
            font_size: FontSize::Px(14.0),
            ..default()
        },
        TextColor(Color::srgba(1.0, 1.0, 1.0, 0.85)),
        Node {
            position_type: PositionType::Absolute,
            top: px(12.0),
            right: px(12.0),
            padding: UiRect::all(px(8.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
    ));
}

fn toggle_help(keys: Res<ButtonInput<KeyCode>>, mut panel: Single<&mut Visibility, With<HelpPanel>>) {
    if keys.just_pressed(KeyCode::F1) {
        **panel = match **panel {
            Visibility::Hidden => Visibility::Inherited,
            _ => Visibility::Hidden,
        };
    }
}
