//! Menus: the main menu (play solo, host, join, settings, credits, quit),
//! the connecting screen, the in-game pause menu (Esc), the settings and
//! credits pages, and who owns the mouse.
//!
//! Look: the shared UI style (`ui::style`) over the live map. The main menu
//! is a column on the left of a slow cinematic drift over the keep
//! ([`menu_backdrop`]); other pages are centred cards on a darkened world.
//! The in-game HUD is hidden while a main-menu page is open
//! (`ui::HudLayer`).
//!
//! Pages ([`Page`]) are rebuilt whenever the page changes. Keyboard: Up/Down
//! or Tab move the focus, Enter activates, Left/Right change a setting, Esc
//! goes back (pause menu: resumes).
//!
//! Mouse: free in menus; captured in game (unless an automated run); Esc in
//! game opens the pause menu and frees it; clicking the game while free and
//! not paused captures it again. Gameplay input only reads while captured
//! (`player::input_captured`).

mod credits;
mod settings;
mod settings_screen;
mod widgets;

use bevy::{
    app::AppExit,
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
    },
    prelude::*,
    ui_widgets::{Activate, Button},
    window::{CursorGrabMode, CursorOptions},
};

pub use settings::MenuSettings;
use widgets::{Focus, Nav, PageRoot};

use crate::{
    map::MapChoice,
    net::BUILD_ID,
    player::{AutomatedRun, PlayerCamera},
    session::{AppState, MenuNotice, NetMode, resolve_address},
    settings::{PlayerSettings, SettingsFile},
    ui::style::{self, palette, size},
};

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(MenuSettings::load())
            .init_resource::<Paused>()
            .init_resource::<Page>()
            .init_resource::<Focus>()
            .add_message::<MenuAction>()
            .add_observer(activated)
            .add_systems(OnEnter(AppState::MainMenu), (open(Page::Main), free_cursor, page_from_args).chain())
            .add_systems(OnEnter(AppState::Connecting), (open(Page::Connecting), free_cursor))
            .add_systems(
                OnEnter(AppState::InGame),
                (open(Page::Closed), capture_cursor_unless_automated, page_from_args).chain(),
            )
            .add_systems(
                Update,
                (
                    show_page.run_if(resource_changed::<Page>),
                    (widgets::hover_focus, widgets::move_focus, widgets::focus_colors).chain(),
                    (type_into_fields, show_fields).chain(),
                    (keyboard_actions, run_menu_actions).chain(),
                    update_notice,
                    update_connecting_text.run_if(in_state(AppState::Connecting)),
                    // Every frame: at startup the camera appears after the
                    // first state is entered.
                    menu_backdrop.run_if(not(in_state(AppState::InGame))),
                    recapture_on_click.run_if(in_state(AppState::InGame)),
                ),
            );
        settings_screen::build(app);
        credits::build(app);
    }
}

/// The open menu page.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Page {
    /// In game, no menu.
    #[default]
    Closed,
    Main,
    Connecting,
    Pause,
    Settings,
    Credits,
}

/// The pause menu (or a page opened from it) is up.
#[derive(Resource, Default)]
pub struct Paused(pub bool);

/// `--menu-page settings|credits|pause`: open that page straight away (menu
/// screenshots; `pause` only in game, the others in either).
fn page_from_args(mut page: ResMut<Page>, state: Res<State<AppState>>) {
    let args: Vec<String> = std::env::args().collect();
    let Some(name) = args.iter().position(|a| a == "--menu-page").and_then(|i| args.get(i + 1)) else {
        return;
    };
    let in_game = *state.get() == AppState::InGame;
    *page = match (name.as_str(), in_game) {
        ("settings", _) => Page::Settings,
        ("credits", _) => Page::Credits,
        ("pause", true) => Page::Pause,
        _ => return,
    };
}

fn open(page: Page) -> impl Fn(ResMut<Page>) {
    move |mut current: ResMut<Page>| {
        *current = page;
    }
}

/// A menu button; activating it (click, Enter, or Enter in its text field)
/// sends a [`MenuAction`].
#[derive(Component, Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuButton {
    Solo,
    Host,
    Join,
    Settings,
    Credits,
    Quit,
    Cancel,
    Resume,
    Leave,
    /// Leave a settings or credits page.
    Back,
    /// Settings page: everything back to the defaults.
    ResetSettings,
}

type MenuAction = MenuButton;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum FieldKind {
    Port,
    Address,
}

/// A one-line text box.
#[derive(Component, Debug)]
struct TextField {
    kind: FieldKind,
    value: String,
    focused: bool,
}

#[derive(Component)]
struct FieldText;

#[derive(Component)]
struct NoticeLine;

#[derive(Component)]
struct ConnectingText;

/// Rebuild the open page.
fn show_page(
    page: Res<Page>,
    state: Res<State<AppState>>,
    pages: Query<Entity, With<PageRoot>>,
    mut paused: ResMut<Paused>,
    mut focus: ResMut<Focus>,
    settings: Res<MenuSettings>,
    player: Res<PlayerSettings>,
    tuning: Res<crate::config::Tuning>,
    mut commands: Commands,
) {
    for entity in &pages {
        commands.entity(entity).despawn();
    }
    focus.0 = None;
    let in_game = *state.get() == AppState::InGame;
    paused.0 = in_game && *page != Page::Closed;
    match *page {
        Page::Closed => {}
        Page::Main => spawn_main(&mut commands, &settings),
        Page::Connecting => spawn_connecting(&mut commands),
        Page::Pause => spawn_pause(&mut commands),
        Page::Settings => settings_screen::spawn(&mut commands, &player, &tuning),
        Page::Credits => credits::spawn(&mut commands),
    }
}

fn spawn_main(commands: &mut Commands, settings: &MenuSettings) {
    use widgets::button;
    commands.spawn(widgets::side_screen(
        "Main menu",
        children![(
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexStart,
                row_gap: px(8.0),
                margin: UiRect::left(percent(7.0)),
                ..default()
            },
            children![
                (
                    Text::new("HARROW"),
                    style::bold(size::HERO),
                    TextColor(palette::TEXT),
                    Node {
                        margin: UiRect::bottom(px(-10.0)),
                        ..default()
                    },
                ),
                (
                    Text::new("HARROW KEEP  /  ATTACK AND DEFENCE"),
                    style::bold(size::SMALL),
                    TextColor(palette::ACCENT),
                    Node {
                        margin: UiRect::bottom(px(26.0)),
                        ..default()
                    },
                ),
                (NoticeLine, Text::new(""), style::text(size::BODY), TextColor(palette::ALERT)),
                button(MenuButton::Solo, 0, "Play Solo"),
                row(children![
                    button(MenuButton::Host, 1, "Host Game"),
                    field_label("Port"),
                    field(FieldKind::Port, settings.host_port.to_string(), 96.0),
                ]),
                row(children![
                    button(MenuButton::Join, 2, "Join Game"),
                    field_label("Address"),
                    field(FieldKind::Address, settings.last_address.clone(), 220.0),
                ]),
                button(MenuButton::Settings, 3, "Settings"),
                button(MenuButton::Credits, 4, "Credits"),
                button(MenuButton::Quit, 5, "Quit"),
                (
                    Text::new(format!("build {BUILD_ID}")),
                    style::text(size::CAPTION),
                    TextColor(palette::FAINT),
                    Node {
                        margin: UiRect::top(px(28.0)),
                        ..default()
                    },
                ),
            ],
        )],
    ));
}

fn spawn_connecting(commands: &mut Commands) {
    commands.spawn(widgets::screen(
        "Connecting",
        JustifyContent::Center,
        children![widgets::panel(
            460.0,
            children![
                widgets::title("CONNECTING"),
                (ConnectingText, widgets::body("Connecting...", size::BODY, palette::TEXT)),
                (
                    Node {
                        margin: UiRect::top(px(12.0)),
                        ..default()
                    },
                    children![widgets::button(MenuButton::Cancel, 0, "Cancel")],
                ),
            ]
        )],
    ));
}

fn spawn_pause(commands: &mut Commands) {
    use widgets::button;
    commands.spawn(widgets::screen(
        "Pause menu",
        JustifyContent::Center,
        children![widgets::panel(
            360.0,
            children![
                widgets::title("PAUSED"),
                button(MenuButton::Resume, 0, "Resume"),
                button(MenuButton::Settings, 1, "Settings"),
                button(MenuButton::Credits, 2, "Credits"),
                button(MenuButton::Leave, 3, "Leave to menu"),
                button(MenuButton::Quit, 4, "Quit"),
            ]
        )],
    ));
}

fn row(children: impl Bundle) -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(12.0),
            ..default()
        },
        children,
    )
}

fn field_label(text: &str) -> impl Bundle {
    (Text::new(text), style::text(size::SMALL), TextColor(palette::DIM))
}

fn field(kind: FieldKind, value: String, width: f32) -> impl Bundle {
    (
        TextField {
            kind,
            value,
            focused: false,
        },
        Button,
        Node {
            padding: UiRect::axes(px(10.0), px(7.0)),
            width: px(width),
            border: UiRect::bottom(px(1.0)),
            ..default()
        },
        BorderColor::all(palette::DIM),
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
        children![(FieldText, Text::new(""), style::text(size::BODY), TextColor(palette::TEXT))],
    )
}

/// A button was clicked: menu buttons act, text fields take the focus.
fn activated(
    trigger: On<Activate>,
    buttons: Query<&MenuButton>,
    mut fields: Query<&mut TextField>,
    mut actions: MessageWriter<MenuAction>,
) {
    if let Ok(button) = buttons.get(trigger.entity) {
        actions.write(*button);
    }
    if let Ok(kind) = fields.get(trigger.entity).map(|f| f.kind) {
        for mut field in &mut fields {
            field.focused = field.kind == kind;
        }
    }
}

/// Enter activates the focused button (or presses Host / Join from their
/// text field); Esc goes back.
#[allow(clippy::too_many_arguments)]
fn keyboard_actions(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<Focus>,
    mut page: ResMut<Page>,
    state: Res<State<AppState>>,
    buttons: Query<(&Nav, &MenuButton)>,
    fields: Query<&TextField>,
    mut actions: MessageWriter<MenuAction>,
    mut cursor: Query<&mut CursorOptions>,
) {
    if keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space]) {
        let typing = fields.iter().find(|f| f.focused);
        if let Some(field) = typing {
            if !keys.just_pressed(KeyCode::Space) {
                actions.write(match field.kind {
                    FieldKind::Port => MenuButton::Host,
                    FieldKind::Address => MenuButton::Join,
                });
            }
        } else if let Some(at) = focus.0
            && let Some((_, button)) = buttons.iter().find(|(nav, _)| nav.0 == at)
        {
            actions.write(*button);
        }
    }
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    let in_game = *state.get() == AppState::InGame;
    let target = match (*page, in_game) {
        (Page::Settings | Page::Credits, true) => Page::Pause,
        (Page::Settings | Page::Credits, false) => Page::Main,
        (Page::Closed, true) => Page::Pause,
        (Page::Pause, true) => Page::Closed,
        (Page::Connecting, _) => {
            actions.write(MenuButton::Cancel);
            return;
        }
        _ => return,
    };
    if target != *page {
        set_cursor_free(in_game && target != Page::Closed, in_game, &mut cursor);
        *page = target;
    }
}

/// In game: free the mouse while a page is open, capture it when closed.
fn set_cursor_free(free: bool, in_game: bool, cursor: &mut Query<&mut CursorOptions>) {
    if !in_game {
        return;
    }
    for mut cursor in cursor.iter_mut() {
        cursor.grab_mode = if free { CursorGrabMode::None } else { CursorGrabMode::Locked };
        cursor.visible = free;
    }
}

#[allow(clippy::too_many_arguments)]
fn run_menu_actions(
    mut actions: MessageReader<MenuAction>,
    mut fields: Query<&mut TextField>,
    mut settings: ResMut<MenuSettings>,
    mut notice: ResMut<MenuNotice>,
    mut mode: ResMut<NetMode>,
    mut page: ResMut<Page>,
    mut next: ResMut<NextState<AppState>>,
    state: Res<State<AppState>>,
    mut exit: MessageWriter<AppExit>,
    mut cursor: Query<&mut CursorOptions>,
    mut player: ResMut<PlayerSettings>,
    file: Res<SettingsFile>,
) {
    let field_value = |kind: FieldKind, fields: &Query<&mut TextField>| {
        fields
            .iter()
            .find(|f| f.kind == kind)
            .map(|f| f.value.trim().to_string())
            .unwrap_or_default()
    };
    let in_game = *state.get() == AppState::InGame;
    let pressed: Vec<MenuButton> = actions.read().copied().collect();
    for button in pressed {
        match (button, *state.get()) {
            (MenuButton::Solo, AppState::MainMenu) => {
                notice.0 = None;
                *mode = NetMode::Offline;
                next.set(AppState::InGame);
            }
            (MenuButton::Host, AppState::MainMenu) => {
                match field_value(FieldKind::Port, &fields).parse::<u16>() {
                    Ok(port) if port > 0 => {
                        notice.0 = None;
                        settings.host_port = port;
                        settings.save();
                        *mode = NetMode::Host { port };
                        next.set(AppState::InGame);
                    }
                    _ => notice.0 = Some("The port must be a number from 1 to 65535".into()),
                }
            }
            (MenuButton::Join, AppState::MainMenu) => {
                let raw = field_value(FieldKind::Address, &fields);
                match resolve_address(&raw) {
                    Ok(server) => {
                        notice.0 = None;
                        settings.last_address = raw;
                        settings.save();
                        *mode = NetMode::Client { server };
                        next.set(AppState::Connecting);
                    }
                    Err(err) => notice.0 = Some(err),
                }
            }
            (MenuButton::Settings, _) => *page = Page::Settings,
            (MenuButton::Credits, _) => *page = Page::Credits,
            (MenuButton::Back, _) => {
                if *page == Page::Settings {
                    file.save(&player);
                }
                *page = if in_game { Page::Pause } else { Page::Main };
            }
            (MenuButton::ResetSettings, _) => {
                *player = PlayerSettings {
                    // Keep the display mode: resetting shouldn't jump the
                    // window around.
                    fullscreen: player.fullscreen,
                    ..default()
                };
                // Rebuild the page with the new values.
                page.set_changed();
            }
            (MenuButton::Cancel, AppState::Connecting) => {
                notice.0 = Some("Cancelled".into());
                next.set(AppState::MainMenu);
            }
            (MenuButton::Resume, AppState::InGame) => {
                set_cursor_free(false, true, &mut cursor);
                *page = Page::Closed;
            }
            (MenuButton::Leave, AppState::InGame) => {
                notice.0 = None;
                next.set(AppState::MainMenu);
            }
            (MenuButton::Quit, _) => {
                exit.write(AppExit::Success);
            }
            _ => {}
        }
        for mut field in &mut fields {
            field.focused = false;
        }
    }
}

fn update_notice(notice: Res<MenuNotice>, mut lines: Query<&mut Text, With<NoticeLine>>) {
    for mut text in &mut lines {
        let value = notice.0.clone().unwrap_or_default();
        if text.0 != value {
            text.0 = value;
        }
    }
}

fn update_connecting_text(mode: Res<NetMode>, time: Res<Time>, mut texts: Query<&mut Text, With<ConnectingText>>) {
    let NetMode::Client { server } = *mode else {
        return;
    };
    let dots = ".".repeat(1 + (time.elapsed_secs() * 2.0) as usize % 3);
    for mut text in &mut texts {
        text.0 = format!("Connecting to {server}{dots}");
    }
}

fn type_into_fields(mut keys: MessageReader<KeyboardInput>, mut fields: Query<&mut TextField>) {
    for key in keys.read() {
        if key.state != ButtonState::Pressed {
            continue;
        }
        for mut field in &mut fields {
            if !field.focused {
                continue;
            }
            match &key.logical_key {
                Key::Backspace => {
                    field.value.pop();
                }
                Key::Character(text) => {
                    let allowed = |c: char| match field.kind {
                        FieldKind::Port => c.is_ascii_digit(),
                        FieldKind::Address => c.is_ascii_alphanumeric() || ".:-_[]".contains(c),
                    };
                    let add: String = text.chars().filter(|c| allowed(*c)).collect();
                    if field.value.len() + add.len() <= 64 {
                        field.value.push_str(&add);
                    }
                }
                _ => {}
            }
        }
    }
}

fn show_fields(
    mut fields: Query<(&TextField, &Children, &mut BorderColor), Changed<TextField>>,
    mut texts: Query<&mut Text, With<FieldText>>,
) {
    for (field, children, mut border) in &mut fields {
        *border = BorderColor::all(if field.focused { palette::ACCENT } else { palette::DIM });
        for child in children.iter() {
            if let Ok(mut text) = texts.get_mut(child) {
                text.0 = if field.focused {
                    format!("{}|", field.value)
                } else {
                    field.value.clone()
                };
            }
        }
    }
}

/// Behind the menus: a slow drift over the map. On Harrow Keep the camera
/// sweeps back and forth across the south side of the compound (the sun
/// behind it), looking at the keep.
fn menu_backdrop(mut camera: Query<&mut Transform, With<PlayerCamera>>, time: Res<Time<Real>>) {
    let t = time.elapsed_secs();
    let view = match MapChoice::from_args() {
        MapChoice::Keep => {
            let target = Vec3::new(0.0, 3.0, -16.0);
            // Bearing of the camera seen from the target: 0 north, 90 east.
            let bearing = (165.0 + 28.0 * (t * std::f32::consts::TAU / 110.0).sin()).to_radians();
            let radius = 64.0 + 6.0 * (t * std::f32::consts::TAU / 70.0).sin();
            let height = 22.0 + 4.0 * (t * std::f32::consts::TAU / 85.0).cos();
            let at = target + Vec3::new(bearing.sin() * radius, height, -bearing.cos() * radius);
            Transform::from_translation(at).looking_at(target, Vec3::Y)
        }
        _ => {
            let sway = 2.0 * (t * std::f32::consts::TAU / 40.0).sin();
            Transform::from_xyz(15.5 + sway, 5.5, 21.0).looking_at(Vec3::new(6.0, 2.0, -3.0), Vec3::Y)
        }
    };
    for mut transform in &mut camera {
        transform.set_if_neq(view);
    }
}

fn free_cursor(mut cursor: Query<&mut CursorOptions>) {
    for mut cursor in &mut cursor {
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
    }
}

fn capture_cursor_unless_automated(automated: Option<Res<AutomatedRun>>, mut cursor: Query<&mut CursorOptions>) {
    if automated.is_some() {
        return;
    }
    for mut cursor in &mut cursor {
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
    }
}

/// Clicking the game while the mouse is free (alt-tab) captures it again.
fn recapture_on_click(mouse: Res<ButtonInput<MouseButton>>, paused: Res<Paused>, mut cursor: Query<&mut CursorOptions>) {
    if paused.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for mut cursor in &mut cursor {
        if cursor.grab_mode == CursorGrabMode::None {
            cursor.grab_mode = CursorGrabMode::Locked;
            cursor.visible = false;
        }
    }
}
