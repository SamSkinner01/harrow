//! Menus: the main menu (play solo, host, join, quit), the connecting
//! screen, the in-game pause menu (Esc), and who owns the mouse.
//!
//! Functional styling in the HUD's look (white text on translucent black);
//! a polish pass can restyle it. The map stays loaded behind the main menu
//! as its backdrop.
//!
//! Mouse: free in menus; captured in game (unless an automated run); Esc in
//! game opens the pause menu and frees it; clicking the game while free and
//! not paused captures it again. Gameplay input only reads while captured
//! (`player::input_captured`).

mod settings;

use bevy::{
    app::AppExit,
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
    },
    picking::hover::Hovered,
    prelude::*,
    ui::Pressed,
    ui_widgets::{Activate, Button},
    window::{CursorGrabMode, CursorOptions},
};

pub use settings::MenuSettings;

use crate::{
    net::BUILD_ID,
    player::{AutomatedRun, PlayerCamera},
    session::{AppState, MenuNotice, NetMode, resolve_address},
};

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(MenuSettings::load())
            .init_resource::<Paused>()
            .add_message::<MenuAction>()
            .add_observer(activated)
            .add_systems(
                OnEnter(AppState::MainMenu),
                (spawn_main_menu, free_cursor),
            )
            .add_systems(
                OnEnter(AppState::Connecting),
                (spawn_connecting, free_cursor),
            )
            .add_systems(OnEnter(AppState::InGame), capture_cursor_unless_automated)
            .add_systems(OnExit(AppState::InGame), close_pause)
            .add_systems(
                Update,
                (
                    button_colors,
                    (type_into_fields, show_fields).chain(),
                    run_menu_actions,
                    update_notice,
                    update_connecting_text.run_if(in_state(AppState::Connecting)),
                    // Every frame: at startup the camera appears after the
                    // first state is entered.
                    menu_backdrop.run_if(not(in_state(AppState::InGame))),
                    (toggle_pause, recapture_on_click).run_if(in_state(AppState::InGame)),
                ),
            );
    }
}

const PANEL: Color = Color::srgba(0.0, 0.0, 0.0, 0.62);
const BACKDROP: Color = Color::srgba(0.0, 0.0, 0.0, 0.35);
const TEXT: Color = Color::srgba(1.0, 1.0, 1.0, 0.92);
const DIM: Color = Color::srgba(1.0, 1.0, 1.0, 0.6);
const ALERT: Color = Color::srgb(1.0, 0.55, 0.35);
const BUTTON: Color = Color::srgba(1.0, 1.0, 1.0, 0.08);
const BUTTON_HOVER: Color = Color::srgba(1.0, 1.0, 1.0, 0.18);
const BUTTON_PRESS: Color = Color::srgba(0.98, 0.70, 0.32, 0.45);
const FIELD: Color = Color::srgba(0.0, 0.0, 0.0, 0.5);
const FIELD_FOCUS: Color = Color::srgba(0.25, 0.2, 0.1, 0.8);

fn font(size: f32) -> TextFont {
    TextFont {
        font_size: FontSize::Px(size),
        ..default()
    }
}

/// A menu button; activating it (click, or Enter in its text field) sends
/// a [`MenuAction`].
#[derive(Component, Message, Clone, Copy, Debug, PartialEq, Eq)]
enum MenuButton {
    Solo,
    Host,
    Join,
    Quit,
    Cancel,
    Resume,
    Leave,
}

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

#[derive(Component)]
struct PauseMenu;

/// The pause menu is open.
#[derive(Resource, Default)]
pub struct Paused(pub bool);

fn panel(children: impl Bundle) -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            row_gap: px(10.0),
            padding: UiRect::all(px(24.0)),
            min_width: px(420.0),
            max_width: px(620.0),
            ..default()
        },
        BackgroundColor(PANEL),
        children,
    )
}

fn screen(marker: impl Bundle, children: impl Bundle) -> impl Bundle {
    (
        marker,
        Node {
            position_type: PositionType::Absolute,
            width: percent(100.0),
            height: percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(BACKDROP),
        GlobalZIndex(10),
        children,
    )
}

type MenuAction = MenuButton;

fn button(kind: MenuButton, label: &str) -> impl Bundle {
    (
        kind,
        Button,
        Hovered::default(),
        Node {
            padding: UiRect::axes(px(16.0), px(8.0)),
            justify_content: JustifyContent::Center,
            min_width: px(150.0),
            ..default()
        },
        BackgroundColor(BUTTON),
        children![(Text::new(label), font(19.0), TextColor(TEXT))],
    )
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
            ..default()
        },
        BackgroundColor(FIELD),
        children![(FieldText, Text::new(""), font(18.0), TextColor(TEXT))],
    )
}

fn row(children: impl Bundle) -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(10.0),
            ..default()
        },
        children,
    )
}

fn spawn_main_menu(mut commands: Commands, settings: Res<MenuSettings>) {
    commands.spawn(screen(
        (Name::new("Main menu"), DespawnOnExit(AppState::MainMenu)),
        children![panel(children![
            (
                Text::new("HARROW"),
                font(46.0),
                TextColor(TEXT),
                TextLayout::justify(Justify::Center),
            ),
            (NoticeLine, Text::new(""), font(15.0), TextColor(ALERT)),
            button(MenuButton::Solo, "Play Solo"),
            row(children![
                button(MenuButton::Host, "Host Game"),
                (Text::new("Port"), font(16.0), TextColor(DIM)),
                field(FieldKind::Port, settings.host_port.to_string(), 90.0),
            ]),
            row(children![
                button(MenuButton::Join, "Join Game"),
                (Text::new("Address"), font(16.0), TextColor(DIM)),
                field(FieldKind::Address, settings.last_address.clone(), 210.0),
            ]),
            button(MenuButton::Quit, "Quit"),
            (
                Text::new(format!("build {BUILD_ID}")),
                font(12.0),
                TextColor(DIM),
                TextLayout::justify(Justify::Right),
            ),
        ])],
    ));
}

fn spawn_connecting(mut commands: Commands) {
    commands.spawn(screen(
        (Name::new("Connecting"), DespawnOnExit(AppState::Connecting)),
        children![panel(children![
            (
                ConnectingText,
                Text::new("Connecting..."),
                font(22.0),
                TextColor(TEXT)
            ),
            button(MenuButton::Cancel, "Cancel"),
        ])],
    ));
}

fn spawn_pause(commands: &mut Commands) {
    commands.spawn(screen(
        (Name::new("Pause menu"), PauseMenu),
        children![panel(children![
            (Text::new("PAUSED"), font(30.0), TextColor(TEXT)),
            button(MenuButton::Resume, "Resume"),
            button(MenuButton::Leave, "Leave to menu"),
            button(MenuButton::Quit, "Quit"),
        ])],
    ));
}

/// The main menu looks over the courtyard at the keep.
fn menu_backdrop(mut camera: Query<&mut Transform, With<PlayerCamera>>) {
    let view = Transform::from_xyz(15.5, 5.5, 21.0).looking_at(Vec3::new(6.0, 2.0, -3.0), Vec3::Y);
    for mut transform in &mut camera {
        transform.set_if_neq(view);
    }
}

fn button_colors(
    mut buttons: Query<(&Hovered, Has<Pressed>, &mut BackgroundColor), With<MenuButton>>,
) {
    for (hovered, pressed, mut color) in &mut buttons {
        let wanted = if pressed {
            BUTTON_PRESS
        } else if hovered.0 {
            BUTTON_HOVER
        } else {
            BUTTON
        };
        if color.0 != wanted {
            color.0 = wanted;
        }
    }
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

#[allow(clippy::too_many_arguments)]
fn run_menu_actions(
    mut actions: MessageReader<MenuAction>,
    mut fields: Query<&mut TextField>,
    mut keys: MessageReader<KeyboardInput>,
    mut settings: ResMut<MenuSettings>,
    mut notice: ResMut<MenuNotice>,
    mut mode: ResMut<NetMode>,
    mut paused: ResMut<Paused>,
    pause_menus: Query<Entity, With<PauseMenu>>,
    mut next: ResMut<NextState<AppState>>,
    state: Res<State<AppState>>,
    mut exit: MessageWriter<AppExit>,
    mut cursor: Query<&mut CursorOptions>,
    mut commands: Commands,
) {
    let mut pressed: Vec<MenuButton> = actions.read().copied().collect();
    // Enter in a field presses its button.
    for key in keys.read() {
        if key.state == ButtonState::Pressed && key.logical_key == Key::Enter {
            for field in &fields {
                if field.focused {
                    pressed.push(match field.kind {
                        FieldKind::Port => MenuButton::Host,
                        FieldKind::Address => MenuButton::Join,
                    });
                }
            }
        }
    }
    let field_value = |kind: FieldKind, fields: &Query<&mut TextField>| {
        fields
            .iter()
            .find(|f| f.kind == kind)
            .map(|f| f.value.trim().to_string())
            .unwrap_or_default()
    };
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
            (MenuButton::Cancel, AppState::Connecting) => {
                notice.0 = Some("Cancelled".into());
                next.set(AppState::MainMenu);
            }
            (MenuButton::Resume, AppState::InGame) => {
                set_paused(false, &mut paused, &pause_menus, &mut cursor, &mut commands);
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

fn update_connecting_text(
    mode: Res<NetMode>,
    time: Res<Time>,
    mut texts: Query<&mut Text, With<ConnectingText>>,
) {
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
    fields: Query<(&TextField, &Children, &mut BackgroundColor), Changed<TextField>>,
    mut texts: Query<&mut Text, With<FieldText>>,
) {
    let mut fields = fields;
    for (field, children, mut background) in &mut fields {
        background.0 = if field.focused { FIELD_FOCUS } else { FIELD };
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

fn free_cursor(mut cursor: Query<&mut CursorOptions>) {
    for mut cursor in &mut cursor {
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
    }
}

fn capture_cursor_unless_automated(
    automated: Option<Res<AutomatedRun>>,
    mut cursor: Query<&mut CursorOptions>,
    mut paused: ResMut<Paused>,
) {
    paused.0 = false;
    if automated.is_some() {
        return;
    }
    for mut cursor in &mut cursor {
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
    }
}

fn set_paused(
    open: bool,
    paused: &mut Paused,
    menus: &Query<Entity, With<PauseMenu>>,
    cursor: &mut Query<&mut CursorOptions>,
    commands: &mut Commands,
) {
    paused.0 = open;
    for menu in menus {
        commands.entity(menu).despawn();
    }
    if open {
        spawn_pause(commands);
    }
    for mut cursor in cursor.iter_mut() {
        cursor.grab_mode = if open {
            CursorGrabMode::None
        } else {
            CursorGrabMode::Locked
        };
        cursor.visible = open;
    }
}

fn toggle_pause(
    keys: Res<ButtonInput<KeyCode>>,
    mut paused: ResMut<Paused>,
    menus: Query<Entity, With<PauseMenu>>,
    mut cursor: Query<&mut CursorOptions>,
    mut commands: Commands,
) {
    if keys.just_pressed(KeyCode::Escape) {
        let open = !paused.0;
        set_paused(open, &mut paused, &menus, &mut cursor, &mut commands);
    }
}

/// Clicking the game while the mouse is free (alt-tab) captures it again.
fn recapture_on_click(
    mouse: Res<ButtonInput<MouseButton>>,
    paused: Res<Paused>,
    mut cursor: Query<&mut CursorOptions>,
) {
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

fn close_pause(
    mut paused: ResMut<Paused>,
    menus: Query<Entity, With<PauseMenu>>,
    mut commands: Commands,
) {
    paused.0 = false;
    for menu in &menus {
        commands.entity(menu).despawn();
    }
}
