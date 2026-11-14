//! HUD basics: the shared style ([`style`]), the HUD layer, a centre dot and
//! a key-help panel (F1). Menus are in `src/menu/`.
//!
//! Every root UI node is moved under one [`HudLayer`] node, which is only
//! visible in game: the HUDs of the round, weapons, gadgets and destruction
//! all disappear behind the main menu without knowing about it. Menu pages
//! (and anything else that must stay up) carry [`NotHud`].

pub mod style;

use bevy::prelude::*;

use crate::{player::LocalPlayer, session::AppState, weapons::AimView};
use style::{palette, size, space};

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(style::UiStylePlugin)
            .add_systems(PreStartup, spawn_hud_layer)
            .add_systems(Startup, (spawn_crosshair, spawn_help))
            .add_systems(
                Update,
                (toggle_help, hide_crosshair_when_aiming, adopt_hud_roots, show_hud_in_game),
            );
    }
}

/// Parent of every HUD element; hidden outside the game.
#[derive(Component)]
pub struct HudLayer;

/// A root UI node that is not part of the HUD (menu pages).
#[derive(Component)]
pub struct NotHud;

fn spawn_hud_layer(mut commands: Commands) {
    commands.spawn((
        Name::new("HUD"),
        HudLayer,
        Node {
            position_type: PositionType::Absolute,
            width: percent(100.0),
            height: percent(100.0),
            ..default()
        },
        Pickable::IGNORE,
        Visibility::Hidden,
    ));
}

/// New root UI nodes join the HUD layer.
#[allow(clippy::type_complexity)]
fn adopt_hud_roots(
    layer: Single<Entity, With<HudLayer>>,
    roots: Query<Entity, (Added<Node>, Without<ChildOf>, Without<HudLayer>, Without<NotHud>)>,
    mut commands: Commands,
) {
    for root in &roots {
        commands.entity(root).insert(ChildOf(*layer));
    }
}

fn show_hud_in_game(state: Res<State<AppState>>, mut layer: Single<&mut Visibility, With<HudLayer>>) {
    let wanted = if *state.get() == AppState::InGame { Visibility::Inherited } else { Visibility::Hidden };
    layer.set_if_neq(wanted);
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
                BackgroundColor(palette::TEXT),
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

#[derive(Component)]
struct HelpHint;

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
        // Monospace: the keys are laid out in columns.
        style::mono(size::SMALL),
        TextColor(palette::TEXT),
        Node {
            position_type: PositionType::Absolute,
            top: px(space::EDGE),
            right: px(space::EDGE),
            padding: UiRect::all(px(10.0)),
            border: UiRect::top(px(2.0)),
            ..default()
        },
        BorderColor::all(palette::ACCENT),
        BackgroundColor(palette::PANEL),
        Visibility::Hidden,
    ));
    commands.spawn((
        Name::new("Help hint"),
        HelpHint,
        Text::new("F1  controls"),
        style::text(size::CAPTION),
        TextColor(palette::FAINT),
        Node {
            position_type: PositionType::Absolute,
            top: px(space::EDGE),
            right: px(space::EDGE),
            ..default()
        },
    ));
}

fn toggle_help(
    keys: Res<ButtonInput<KeyCode>>,
    mut panel: Single<&mut Visibility, (With<HelpPanel>, Without<HelpHint>)>,
    mut hint: Single<&mut Visibility, With<HelpHint>>,
) {
    if keys.just_pressed(KeyCode::F1) {
        let show = **panel == Visibility::Hidden;
        **panel = if show { Visibility::Inherited } else { Visibility::Hidden };
        **hint = if show { Visibility::Hidden } else { Visibility::Inherited };
    }
}
