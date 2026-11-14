//! Weapon HUD: ammo (magazine / reserve), weapon name, reload progress, and
//! a hit marker in the centre (white for body hits, red and larger for head
//! shots, with a short hold on kills).

use bevy::prelude::*;

use super::{BulletHit, WeaponAction, WeaponState};
use crate::{
    config::Tuning,
    player::LocalPlayer,
    ui::style::{self, palette, size, space},
};

pub struct WeaponHudPlugin;

impl Plugin for WeaponHudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_hud)
            .add_systems(Update, (update_ammo, update_hit_marker));
    }
}

#[derive(Component)]
struct WeaponName;

#[derive(Component)]
struct MagazineText;

#[derive(Component)]
struct ReserveText;

#[derive(Component)]
struct ReloadBar;

#[derive(Component)]
struct ReloadFill;

#[derive(Component)]
struct HitMarker;

#[derive(Component)]
struct HitMarkerArm;

#[derive(Default)]
struct MarkerState {
    timer: f32,
    head: bool,
    lethal: bool,
}

// Shared HUD style (`ui::style`).
const TEXT: Color = palette::TEXT;
const DIM: Color = palette::DIM;
const LOW: Color = palette::ATTACK;
const EMPTY: Color = palette::ALERT;
const BAR_WIDTH: f32 = 150.0;

fn spawn_hud(mut commands: Commands) {
    // Bottom right: weapon name, ammo, reload bar.
    commands.spawn((
        Name::new("Weapon HUD"),
        Node {
            position_type: PositionType::Absolute,
            right: px(space::EDGE + 6.0),
            bottom: px(space::EDGE),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::FlexEnd,
            row_gap: px(2.0),
            ..default()
        },
        Pickable::IGNORE,
        children![
            (WeaponName, Text::new(""), style::bold(size::SMALL), TextColor(DIM)),
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Baseline,
                    column_gap: px(6.0),
                    ..default()
                },
                children![
                    (MagazineText, Text::new(""), style::bold(size::TITLE + 6.0), TextColor(TEXT)),
                    (ReserveText, Text::new(""), style::text(size::LARGE), TextColor(DIM)),
                ],
            ),
            (
                ReloadBar,
                Node {
                    width: px(BAR_WIDTH),
                    height: px(4.0),
                    ..default()
                },
                BackgroundColor(palette::BAR_TRACK),
                Visibility::Hidden,
                children![(
                    ReloadFill,
                    Node {
                        width: px(0.0),
                        height: percent(100.0),
                        ..default()
                    },
                    BackgroundColor(palette::ACCENT),
                )],
            ),
        ],
    ));

    // Centre: hit marker, four short diagonal strokes around the crosshair.
    let marker = commands
        .spawn((
            Name::new("Hit marker"),
            HitMarker,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100.0),
                height: percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            Visibility::Hidden,
            Pickable::IGNORE,
        ))
        .id();
    let frame = commands
        .spawn((
            Node {
                width: px(28.0),
                height: px(28.0),
                ..default()
            },
            UiTransform::default(),
            ChildOf(marker),
        ))
        .id();
    for (x, y, angle) in [
        (3.0, 6.0, 45.0),
        (15.0, 6.0, -45.0),
        (3.0, 20.0, -45.0),
        (15.0, 20.0, 45.0),
    ] {
        commands.spawn((
            HitMarkerArm,
            Node {
                position_type: PositionType::Absolute,
                left: px(x),
                top: px(y),
                width: px(10.0),
                height: px(2.0),
                ..default()
            },
            UiTransform {
                rotation: Rot2::degrees(angle),
                ..default()
            },
            BackgroundColor(Color::WHITE),
            ChildOf(frame),
        ));
    }
}

#[allow(clippy::type_complexity)]
fn update_ammo(
    player: Option<Single<&WeaponState, With<LocalPlayer>>>,
    mut name: Single<&mut Text, (With<WeaponName>, Without<MagazineText>, Without<ReserveText>)>,
    mut magazine: Single<
        (&mut Text, &mut TextColor),
        (With<MagazineText>, Without<WeaponName>, Without<ReserveText>),
    >,
    mut reserve: Single<&mut Text, (With<ReserveText>, Without<WeaponName>, Without<MagazineText>)>,
    mut bar: Single<&mut Visibility, With<ReloadBar>>,
    mut fill: Single<&mut Node, With<ReloadFill>>,
    tuning: Res<Tuning>,
) {
    let Some(state) = player else { return };
    let def = state.active.def(&tuning);
    let ammo = state.ammo();
    let set = |text: &mut Text, value: String| {
        if text.0 != value {
            text.0 = value;
        }
    };
    let label = match state.action {
        WeaponAction::Reloading { .. } => format!("{}  RELOADING", def.name),
        _ => def.name.clone(),
    };
    set(&mut name, label);
    let (text, colour) = &mut *magazine;
    set(text, ammo.magazine.to_string());
    let wanted = if ammo.magazine == 0 {
        EMPTY
    } else if ammo.magazine * 4 <= def.magazine {
        LOW
    } else {
        TEXT
    };
    if colour.0 != wanted {
        colour.0 = wanted;
    }
    set(&mut reserve, format!("/ {}", ammo.reserve));

    let reloading = matches!(state.action, WeaponAction::Reloading { .. });
    let visible = if reloading { Visibility::Inherited } else { Visibility::Hidden };
    if **bar != visible {
        **bar = visible;
    }
    if reloading {
        fill.width = px(BAR_WIDTH * state.action.progress());
    }
}

fn update_hit_marker(
    player: Option<Single<Entity, With<LocalPlayer>>>,
    mut hits: MessageReader<BulletHit>,
    mut marker: Single<(&mut Visibility, &Children), With<HitMarker>>,
    mut frames: Query<&mut UiTransform, Without<HitMarkerArm>>,
    mut arms: Query<&mut BackgroundColor, With<HitMarkerArm>>,
    mut state: Local<MarkerState>,
    tuning: Res<Tuning>,
    time: Res<Time>,
) {
    let Some(me) = player else { return };
    let me = *me;
    let duration = tuning.weapons.effects.hit_marker_time;
    for hit in hits.read().filter(|h| h.shooter == me) {
        let head = hit.zone == Some(crate::player::HitZone::Head);
        // A head shot or kill isn't overwritten by a body hit on the same frame.
        state.head = head || (state.timer > 0.0 && state.head);
        state.lethal = hit.lethal || (state.timer > 0.0 && state.lethal);
        state.timer = if hit.lethal { duration * 2.0 } else { duration };
    }
    let (visibility, children) = &mut *marker;
    let wanted = if state.timer > 0.0 {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if **visibility != wanted {
        **visibility = wanted;
    }
    if state.timer <= 0.0 {
        return;
    }
    let fade = (state.timer / duration).min(1.0);
    let colour = if state.head {
        Color::srgba(1.0, 0.2, 0.15, fade)
    } else {
        Color::srgba(1.0, 1.0, 1.0, 0.9 * fade)
    };
    let scale = if state.head || state.lethal { 1.3 } else { 1.0 };
    for frame in children.iter() {
        if let Ok(mut transform) = frames.get_mut(frame) {
            transform.scale = Vec2::splat(scale);
        }
    }
    for mut arm in &mut arms {
        arm.0 = colour;
    }
    state.timer -= time.delta_secs();
}
