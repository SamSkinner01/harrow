//! Gadget HUD, in the round HUD's style (white text on translucent black):
//! a progress bar under the crosshair while clamping, bolting, re-seating,
//! placing or cooking; a status panel on the right with your gadgets and
//! what your ring cutters / backplates are doing; short gadget notices.
//! Gadget counts for keys 3, 4 and G are on the operator card (round HUD).
//!
//! Presentation only.

use bevy::prelude::*;

use super::{
    GadgetActivity, GadgetEvent, GadgetKind, UniqueGadget,
    backplate::BackplateCharge,
    cctv::CctvCamera,
    cutter::{CutterState, RingCutter},
    drone::Drone,
    remote_view::RemoteView,
};
use crate::{
    combat::Team,
    player::LocalPlayer,
};

pub struct GadgetHudPlugin;

impl Plugin for GadgetHudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_hud)
            .add_systems(Update, (update_progress, update_status, update_notices));
    }
}

const PANEL: Color = Color::srgba(0.0, 0.0, 0.0, 0.45);
const TEXT: Color = Color::srgba(1.0, 1.0, 1.0, 0.9);
const ACCENT: Color = Color::srgb(0.98, 0.70, 0.32);
const ALERT: Color = Color::srgb(1.0, 0.35, 0.25);
const GOOD: Color = Color::srgb(0.6, 0.95, 0.55);

#[derive(Component)]
struct ProgressRoot;
#[derive(Component)]
struct ProgressLabel;
#[derive(Component)]
struct ProgressFill;
#[derive(Component)]
struct StatusText;
#[derive(Component)]
struct NoticeText;

fn font(size: f32) -> TextFont {
    TextFont {
        font_size: FontSize::Px(size),
        ..default()
    }
}

fn spawn_hud(mut commands: Commands) {
    commands.spawn((
        Name::new("Gadget progress"),
        ProgressRoot,
        Node {
            position_type: PositionType::Absolute,
            top: percent(64.0),
            width: percent(100.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(5.0),
            ..default()
        },
        Visibility::Hidden,
        Pickable::IGNORE,
        children![
            (
                ProgressLabel,
                Text::new(""),
                font(15.0),
                TextColor(TEXT),
                Node {
                    padding: UiRect::axes(px(8.0), px(3.0)),
                    ..default()
                },
                BackgroundColor(PANEL),
            ),
            (
                Node {
                    width: px(240.0),
                    height: px(7.0),
                    border: UiRect::all(px(1.0)),
                    ..default()
                },
                BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.5)),
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
                children![(
                    ProgressFill,
                    Node {
                        width: percent(0.0),
                        height: percent(100.0),
                        ..default()
                    },
                    BackgroundColor(ACCENT),
                )],
            ),
        ],
    ));
    commands.spawn((
        Name::new("Gadget notice"),
        NoticeText,
        Text::new(""),
        font(15.0),
        TextColor(TEXT),
        TextLayout::justify(Justify::Center),
        Node {
            position_type: PositionType::Absolute,
            top: percent(70.0),
            width: percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
        Pickable::IGNORE,
    ));
    commands.spawn((
        Name::new("Gadget status"),
        StatusText,
        Text::new(""),
        font(13.0),
        TextColor(TEXT),
        TextLayout::justify(Justify::Right),
        Node {
            position_type: PositionType::Absolute,
            right: px(28.0),
            bottom: px(118.0),
            padding: UiRect::all(px(7.0)),
            ..default()
        },
        BackgroundColor(PANEL),
        Visibility::Hidden,
        Pickable::IGNORE,
    ));
}

fn set_text(text: &mut Text, value: String) {
    if text.0 != value {
        text.0 = value;
    }
}

fn update_progress(
    local: Query<&GadgetActivity, With<LocalPlayer>>,
    mut root: Single<&mut Visibility, With<ProgressRoot>>,
    mut label: Single<&mut Text, With<ProgressLabel>>,
    mut fill: Single<(&mut Node, &mut BackgroundColor), With<ProgressFill>>,
) {
    let progress = local.single().ok().and_then(|a| a.progress());
    let Some((text, p)) = progress else {
        root.set_if_neq(Visibility::Hidden);
        return;
    };
    root.set_if_neq(Visibility::Inherited);
    set_text(&mut label, text);
    fill.0.width = percent((p * 100.0).clamp(0.0, 100.0));
    let color = if p > 0.8 && matches!(local.single(), Ok(GadgetActivity::Cooking { fuse: Some(_), .. })) {
        ALERT
    } else {
        ACCENT
    };
    fill.1.0 = color;
}

#[allow(clippy::type_complexity)]
fn update_status(
    local: Query<(Entity, &Team, Option<&UniqueGadget>, Has<RemoteView>), With<LocalPlayer>>,
    cutters: Query<&RingCutter>,
    backplates: Query<&BackplateCharge>,
    drones: Query<&Drone>,
    cameras: Query<(), With<CctvCamera>>,
    mut status: Single<(&mut Text, &mut Visibility), With<StatusText>>,
) {
    let Ok((me, team, gadget, viewing)) = local.single() else {
        return;
    };
    let mut lines = Vec::new();
    if let Some(g) = gadget {
        match g.kind {
            GadgetKind::RingCutter => {
                let mut mine: Vec<&RingCutter> = cutters.iter().filter(|c| c.owner == me).collect();
                mine.sort_by_key(|c| c.serial);
                for (i, c) in mine.iter().enumerate() {
                    let what = match c.state {
                        CutterState::Ready => "READY  (tap 3 to start)".to_string(),
                        CutterState::Cutting => format!("CUTTING  {:.0}%", c.progress * 100.0),
                        CutterState::Jammed => format!("JAMMED at {:.0}%  (hold 3 at it)", c.progress * 100.0),
                    };
                    lines.push(format!("Ring cutter {}: {what}", i + 1));
                }
            }
            GadgetKind::BackplateCharge => {
                let set = backplates.iter().filter(|b| b.owner == me).count();
                if set > 0 {
                    lines.push(format!("Backplate charges set: {set}  (tap 3 fires the latest)"));
                }
            }
            _ => {}
        }
        if lines.is_empty() && g.remaining > 0 && !viewing {
            lines.push(format!("{}: {}", g.kind.name(), g.kind.hint()));
        }
    }
    match team {
        Team::Attackers => {
            let out = drones.iter().filter(|d| d.owner == me).count();
            if out > 0 {
                lines.push(format!("Drones out: {out}  (G to {})", if viewing { "exit" } else { "connect" }));
            }
        }
        Team::Defenders => {
            let live = cameras.iter().count();
            if live > 0 && !viewing {
                lines.push(format!("Cameras: {live}  (G to watch)"));
            }
        }
    }
    let (text, visibility) = &mut *status;
    visibility.set_if_neq(if lines.is_empty() { Visibility::Hidden } else { Visibility::Inherited });
    set_text(text, lines.join("\n"));
}

#[allow(clippy::too_many_arguments)]
fn update_notices(
    mut events: MessageReader<GadgetEvent>,
    local: Query<(Entity, &Team), With<LocalPlayer>>,
    cutters: Query<&RingCutter>,
    time: Res<Time>,
    mut notice: Single<(&mut Text, &mut TextColor), With<NoticeText>>,
    mut shown: Local<(String, Color, f32)>,
) {
    let now = time.elapsed_secs();
    let Ok((me, team)) = local.single() else { return };
    let mine = |cutter: Entity| cutters.get(cutter).is_ok_and(|c| c.owner == me);
    for event in events.read() {
        let post = match event {
            GadgetEvent::CutterJammed { cutter, .. } if mine(*cutter) => {
                Some(("Ring cutter JAMMED: hold 3 at it to re-seat".into(), ALERT))
            }
            GadgetEvent::CutterJammed { .. } if *team == Team::Defenders => {
                Some(("Ring cutter jammed".into(), GOOD))
            }
            GadgetEvent::CutterExposed { cutter, .. } if mine(*cutter) => {
                Some(("Blade through: defenders can jam it".into(), ACCENT))
            }
            GadgetEvent::CutterExposed { .. } if *team == Team::Defenders => {
                Some(("A ring cutter blade is through: shoot it".into(), ALERT))
            }
            GadgetEvent::CutterBreached { knocked_down, .. } => Some((
                if *knocked_down > 0 {
                    format!("Ring cutter is through ({knocked_down} knocked down)")
                } else {
                    "Ring cutter is through".into()
                },
                ACCENT,
            )),
            GadgetEvent::BackplateFired { destroyed, .. } => Some((
                format!("Backplate fired: {destroyed} gadget(s) destroyed"),
                if *destroyed > 0 { GOOD } else { TEXT },
            )),
            GadgetEvent::Flashed { entity, .. } if *entity == me => Some(("FLASHED".into(), ALERT)),
            GadgetEvent::AlarmTriggered { .. } if *team == Team::Defenders => {
                Some(("Proximity alarm: attacker close".into(), ALERT))
            }
            GadgetEvent::ViewLeft { by, lost: true } if *by == me => Some(("Feed lost".into(), ALERT)),
            _ => None,
        };
        if let Some((text, color)) = post {
            *shown = (text, color, now);
        }
    }
    const SHOW: f32 = 2.5;
    let age = now - shown.2;
    let (text, color) = &mut *notice;
    if shown.0.is_empty() || age > SHOW {
        set_text(text, String::new());
        return;
    }
    set_text(text, shown.0.clone());
    color.0 = shown.1.with_alpha((SHOW - age).min(1.0));
}
