//! The settings page (from the main menu and the pause menu): controls,
//! audio, graphics and display, edited live; saved when you leave the page
//! (`crate::settings`).
//!
//! Every row is `label   < value >`: click the arrows or the row, or focus
//! it with the keyboard and use Left/Right (Enter = next value).

use bevy::{picking::hover::Hovered, prelude::*, ui_widgets::{Activate, Button}};

use super::{
    MenuButton,
    widgets::{self, Focus, Focusable, Nav},
};
use crate::{
    config::Tuning,
    settings::{Aa, Ao, PlayerSettings, Press, Quality, Shadows},
    ui::style::{self, palette, size},
};

pub fn build(app: &mut App) {
    app.add_observer(row_activated).add_systems(
        Update,
        (step_with_keys, show_values.run_if(resource_changed::<PlayerSettings>)),
    );
}

/// One setting.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    Sensitivity,
    InvertY,
    Fov,
    Sprint,
    Crouch,
    Lean,
    Aim,
    Master,
    Effects,
    Quality,
    AntiAliasing,
    AmbientOcclusion,
    Shadows,
    Bloom,
    ContactShadows,
    Display,
    Vsync,
    ShowFps,
}

impl Row {
    fn label(self) -> &'static str {
        match self {
            Row::Sensitivity => "Mouse sensitivity",
            Row::InvertY => "Invert mouse Y",
            Row::Fov => "Field of view (vertical)",
            Row::Sprint => "Sprint",
            Row::Crouch => "Crouch",
            Row::Lean => "Lean",
            Row::Aim => "Aim down sights",
            Row::Master => "Master volume",
            Row::Effects => "Effects volume",
            Row::Quality => "Quality preset",
            Row::AntiAliasing => "Anti-aliasing",
            Row::AmbientOcclusion => "Ambient occlusion",
            Row::Shadows => "Shadows",
            Row::Bloom => "Bloom",
            Row::ContactShadows => "Contact shadows",
            Row::Display => "Display",
            Row::Vsync => "V-sync",
            Row::ShowFps => "Show FPS",
        }
    }

    /// The value as shown (effective value: the setting, or the tuning
    /// file's when the setting is unset).
    fn value(self, s: &PlayerSettings, t: &Tuning) -> String {
        let on_off = |b: bool| if b { "On" } else { "Off" }.to_string();
        let press = |p: Option<Press>, file: crate::config::ButtonMode| {
            match p.unwrap_or(file.into()) {
                Press::Hold => "Hold",
                Press::Toggle => "Toggle",
            }
            .to_string()
        };
        match self {
            Row::Sensitivity => format!("{:.3}", s.sensitivity.unwrap_or(t.look.sensitivity)),
            Row::InvertY => on_off(s.invert_y.unwrap_or(t.look.invert_y)),
            Row::Fov => format!("{:.0}°", s.fov.unwrap_or(t.look.fov)),
            Row::Sprint => press(s.sprint, t.controls.sprint),
            Row::Crouch => press(s.crouch, t.controls.crouch),
            Row::Lean => press(s.lean, t.controls.lean),
            Row::Aim => press(s.aim, t.weapons.ads_mode),
            Row::Master => format!("{:.0}%", s.master_volume * 100.0),
            Row::Effects => format!("{:.0}%", s.effects_volume * 100.0),
            Row::Quality => s.quality.label().to_string(),
            Row::AntiAliasing => match s.graphics.anti_aliasing {
                Aa::Off => "Off",
                Aa::Fxaa => "FXAA",
                Aa::Smaa => "SMAA",
                Aa::Taa => "TAA",
            }
            .to_string(),
            Row::AmbientOcclusion => match s.graphics.ambient_occlusion {
                Ao::Off => "Off",
                Ao::Low => "Low",
                Ao::Medium => "Medium",
                Ao::High => "High",
            }
            .to_string(),
            Row::Shadows => match s.graphics.shadows {
                Shadows::Low => "Low",
                Shadows::Medium => "Medium",
                Shadows::High => "High",
                Shadows::Ultra => "Ultra",
            }
            .to_string(),
            Row::Bloom => on_off(s.graphics.bloom),
            Row::ContactShadows => on_off(s.graphics.contact_shadows),
            Row::Display => if s.fullscreen { "Fullscreen" } else { "Windowed" }.to_string(),
            Row::Vsync => on_off(s.vsync),
            Row::ShowFps => on_off(s.show_fps),
        }
    }

    /// Change the value one step (`dir` = +1 / -1).
    fn step(self, dir: i32, s: &mut PlayerSettings, t: &Tuning) {
        fn cycle<T: Copy + PartialEq>(options: &[T], current: T, dir: i32) -> T {
            let i = options.iter().position(|o| *o == current).unwrap_or(0) as i32;
            options[(i + dir).rem_euclid(options.len() as i32) as usize]
        }
        let toggle = |p: Option<Press>, file: crate::config::ButtonMode| {
            Some(match p.unwrap_or(file.into()) {
                Press::Hold => Press::Toggle,
                Press::Toggle => Press::Hold,
            })
        };
        let d = dir as f32;
        match self {
            Row::Sensitivity => {
                let v = s.sensitivity.unwrap_or(t.look.sensitivity) + 0.005 * d;
                s.sensitivity = Some((v.clamp(0.005, 0.3) * 1000.0).round() / 1000.0);
            }
            Row::InvertY => s.invert_y = Some(!s.invert_y.unwrap_or(t.look.invert_y)),
            Row::Fov => {
                let v = s.fov.unwrap_or(t.look.fov) + 2.0 * d;
                s.fov = Some(v.round().clamp(50.0, 100.0));
            }
            Row::Sprint => s.sprint = toggle(s.sprint, t.controls.sprint),
            Row::Crouch => s.crouch = toggle(s.crouch, t.controls.crouch),
            Row::Lean => s.lean = toggle(s.lean, t.controls.lean),
            Row::Aim => s.aim = toggle(s.aim, t.weapons.ads_mode),
            Row::Master => s.master_volume = ((s.master_volume + 0.05 * d) * 20.0).round().clamp(0.0, 20.0) / 20.0,
            Row::Effects => s.effects_volume = ((s.effects_volume + 0.05 * d) * 20.0).round().clamp(0.0, 20.0) / 20.0,
            Row::Quality => {
                let current = if s.quality == Quality::Custom { Quality::High } else { s.quality };
                let next = if s.quality == Quality::Custom {
                    current
                } else {
                    cycle(&Quality::PRESETS, current, dir)
                };
                s.set_quality(next);
            }
            Row::AntiAliasing => {
                s.graphics.anti_aliasing = cycle(&[Aa::Off, Aa::Fxaa, Aa::Smaa, Aa::Taa], s.graphics.anti_aliasing, dir);
                s.match_preset();
            }
            Row::AmbientOcclusion => {
                s.graphics.ambient_occlusion =
                    cycle(&[Ao::Off, Ao::Low, Ao::Medium, Ao::High], s.graphics.ambient_occlusion, dir);
                s.match_preset();
            }
            Row::Shadows => {
                s.graphics.shadows = cycle(
                    &[Shadows::Low, Shadows::Medium, Shadows::High, Shadows::Ultra],
                    s.graphics.shadows,
                    dir,
                );
                s.match_preset();
            }
            Row::Bloom => {
                s.graphics.bloom = !s.graphics.bloom;
                s.match_preset();
            }
            Row::ContactShadows => {
                s.graphics.contact_shadows = !s.graphics.contact_shadows;
                s.match_preset();
            }
            Row::Display => s.fullscreen = !s.fullscreen,
            Row::Vsync => s.vsync = !s.vsync,
            Row::ShowFps => s.show_fps = !s.show_fps,
        }
    }
}

/// The value text of a row.
#[derive(Component)]
struct ValueText(Row);

/// An arrow button: step `row` by `dir`.
#[derive(Component, Clone, Copy)]
struct Stepper {
    row: Row,
    dir: i32,
}

const LEFT: [&[Row]; 2] = [
    &[Row::Sensitivity, Row::InvertY, Row::Fov, Row::Sprint, Row::Crouch, Row::Lean, Row::Aim],
    &[Row::Master, Row::Effects],
];
const RIGHT: [&[Row]; 2] = [
    &[
        Row::Quality,
        Row::AntiAliasing,
        Row::AmbientOcclusion,
        Row::Shadows,
        Row::Bloom,
        Row::ContactShadows,
    ],
    &[Row::Display, Row::Vsync, Row::ShowFps],
];

pub fn spawn(commands: &mut Commands, settings: &PlayerSettings, tuning: &Tuning) {
    let mut nav = 0;
    let screen = commands
        .spawn(widgets::screen("Settings", JustifyContent::Center, ()))
        .id();
    let panel = commands
        .spawn((widgets::panel(940.0, ()), ChildOf(screen)))
        .id();
    commands.spawn((widgets::title("SETTINGS"), ChildOf(panel)));
    let columns = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                column_gap: px(36.0),
                ..default()
            },
            ChildOf(panel),
        ))
        .id();
    for (groups, headings) in [(LEFT, ["CONTROLS", "AUDIO"]), (RIGHT, ["GRAPHICS", "DISPLAY"])] {
        let column = commands
            .spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    flex_grow: 1.0,
                    flex_basis: px(0.0),
                    row_gap: px(3.0),
                    ..default()
                },
                ChildOf(columns),
            ))
            .id();
        for (rows, heading) in groups.iter().zip(headings) {
            commands.spawn((widgets::heading(heading), ChildOf(column)));
            for &row in *rows {
                spawn_row(commands, column, row, nav, settings, tuning);
                nav += 1;
            }
        }
    }
    commands.spawn((
        widgets::body(
            "Unchanged controls follow assets/config/tuning.ron. Saved when you leave this page.",
            size::SMALL,
            palette::FAINT,
        ),
        Node {
            margin: UiRect::top(px(10.0)),
            ..default()
        },
        ChildOf(panel),
    ));
    let buttons = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                column_gap: px(12.0),
                margin: UiRect::top(px(8.0)),
                ..default()
            },
            ChildOf(panel),
        ))
        .id();
    commands.spawn((widgets::button(MenuButton::Back, nav + 1, "Back"), ChildOf(buttons)));
    commands.spawn((widgets::button(MenuButton::ResetSettings, nav, "Reset to defaults"), ChildOf(buttons)));
}

fn spawn_row(commands: &mut Commands, column: Entity, row: Row, nav: u32, settings: &PlayerSettings, tuning: &Tuning) {
    let arrow = |dir: i32, glyph: &'static str| {
        (
            Stepper { row, dir },
            Button,
            Hovered::default(),
            Node {
                width: px(26.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            children![(Text::new(glyph), style::bold(size::BODY), TextColor(palette::ACCENT))],
        )
    };
    commands.spawn((
        row,
        Button,
        Hovered::default(),
        Focusable,
        Nav(nav),
        Node {
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            padding: UiRect::new(px(10.0), px(4.0), px(4.0), px(4.0)),
            border: UiRect::left(px(3.0)),
            ..default()
        },
        BorderColor::all(Color::NONE),
        BackgroundColor(palette::BUTTON),
        ChildOf(column),
        children![
            (Text::new(row.label()), style::text(size::BODY), TextColor(palette::TEXT)),
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    ..default()
                },
                children![
                    arrow(-1, "<"),
                    (
                        ValueText(row),
                        Text::new(row.value(settings, tuning)),
                        style::bold(size::BODY),
                        TextColor(palette::TEXT),
                        TextLayout::justify(Justify::Center),
                        Node {
                            width: px(104.0),
                            justify_content: JustifyContent::Center,
                            ..default()
                        },
                    ),
                    arrow(1, ">"),
                ],
            ),
        ],
    ));
}

/// Clicking an arrow steps its row; clicking the row itself steps forward.
fn row_activated(
    trigger: On<Activate>,
    steppers: Query<&Stepper>,
    rows: Query<&Row>,
    mut settings: ResMut<PlayerSettings>,
    tuning: Res<Tuning>,
) {
    let (row, dir) = if let Ok(stepper) = steppers.get(trigger.entity) {
        (stepper.row, stepper.dir)
    } else if let Ok(row) = rows.get(trigger.entity) {
        (*row, 1)
    } else {
        return;
    };
    row.step(dir, &mut settings, &tuning);
}

/// Left/Right (and Enter) on the focused row.
fn step_with_keys(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<Focus>,
    rows: Query<(&Nav, &Row)>,
    mut settings: ResMut<PlayerSettings>,
    tuning: Res<Tuning>,
) {
    let dir = if keys.just_pressed(KeyCode::ArrowLeft) {
        -1
    } else if keys.any_just_pressed([KeyCode::ArrowRight, KeyCode::Enter, KeyCode::NumpadEnter]) {
        1
    } else {
        return;
    };
    let Some(at) = focus.0 else {
        return;
    };
    if let Some((_, row)) = rows.iter().find(|(nav, _)| nav.0 == at) {
        row.step(dir, &mut settings, &tuning);
    }
}

fn show_values(settings: Res<PlayerSettings>, tuning: Res<Tuning>, mut values: Query<(&ValueText, &mut Text)>) {
    for (value, mut text) in &mut values {
        let shown = value.0.value(&settings, &tuning);
        if text.0 != shown {
            text.0 = shown;
        }
    }
}
