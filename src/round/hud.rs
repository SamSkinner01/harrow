//! Round HUD, in the same style as `src/ui` (white text on translucent black
//! panels): score, round, phase and timer at the top; operator card bottom
//! left; objective prompt with a progress bar under the crosshair; short
//! notices; site and defuser labels that follow the world; the operator
//! selection screen; the round-end and match-end banners.
//!
//! Presentation only: reads the round state, never changes it.

use bevy::prelude::*;

use super::{
    InteractionKind, ObjectiveInteraction, Round, RoundEvent, RoundPhase, Squad, team_label,
    defuser::{DefuserCarrier, DefuserDevice, DroppedDefuser, in_reach},
    operators::{Loadout, Operator, OperatorChoice, OperatorId},
    sites::KeepLayout,
};
use crate::{
    combat::{Dead, Team},
    config::Tuning,
    gadgets::{GadgetEvent, UniqueGadget},
    player::LocalPlayer,
};

pub struct RoundHudPlugin;

impl Plugin for RoundHudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Notice>()
            .add_systems(Startup, spawn_hud)
            .add_systems(
                Update,
                (
                    collect_notices,
                    update_top_bar,
                    update_operator_card,
                    update_prompt,
                    update_notice,
                    update_banner,
                    update_select_panel,
                    update_world_labels,
                    show_hud,
                ),
            );
    }
}

const PANEL: Color = Color::srgba(0.0, 0.0, 0.0, 0.45);
const TEXT: Color = Color::srgba(1.0, 1.0, 1.0, 0.9);
const DIM: Color = Color::srgba(1.0, 1.0, 1.0, 0.65);
const ATTACK: Color = Color::srgb(0.98, 0.70, 0.32);
const DEFEND: Color = Color::srgb(0.48, 0.80, 0.98);
const ALERT: Color = Color::srgb(1.0, 0.35, 0.25);
const SITE: Color = Color::srgb(0.95, 0.75, 0.25);

fn side_color(team: Team) -> Color {
    match team {
        Team::Attackers => ATTACK,
        Team::Defenders => DEFEND,
    }
}

#[derive(Component)]
struct HudRoot;
#[derive(Component)]
struct ScoreText;
#[derive(Component)]
struct PhaseText;
#[derive(Component)]
struct CardText;
#[derive(Component)]
struct PromptText;
#[derive(Component)]
struct PromptBar;
#[derive(Component)]
struct PromptFill;
#[derive(Component)]
struct NoticeText;
#[derive(Component)]
struct Banner;
#[derive(Component)]
struct BannerTitle;
#[derive(Component)]
struct BannerDetail;
#[derive(Component)]
struct SelectPanel;
#[derive(Component)]
struct SelectText;

/// A label pinned to a world position.
#[derive(Component, Clone, Copy, PartialEq)]
enum WorldLabel {
    Site(usize),
    Defuser,
    Dropped,
}

const LABEL_WIDTH: f32 = 220.0;
/// World labels hide when you are closer than this (metres): you can see it.
const LABEL_MIN_DISTANCE: f32 = 2.5;

#[derive(Component)]
struct LabelText;

/// A short message under the prompt, e.g. why you can't plant here.
#[derive(Resource, Default)]
struct Notice {
    text: String,
    color: Color,
    shown_at: f32,
}

fn font(size: f32) -> TextFont {
    TextFont {
        font_size: FontSize::Px(size),
        ..default()
    }
}

fn spawn_hud(mut commands: Commands, layout: Res<KeepLayout>) {
    let root = commands
        .spawn((
            Name::new("Round HUD"),
            HudRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100.0),
                height: percent(100.0),
                ..default()
            },
            Pickable::IGNORE,
            Visibility::Hidden,
        ))
        .id();

    // Top: score line and phase/timer.
    commands.spawn((
        ChildOf(root),
        Node {
            position_type: PositionType::Absolute,
            top: px(10.0),
            width: percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
        children![(
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                padding: UiRect::axes(px(14.0), px(6.0)),
                ..default()
            },
            BackgroundColor(PANEL),
            children![
                (ScoreText, Text::new(""), font(15.0), TextColor(TEXT)),
                (PhaseText, Text::new(""), font(22.0), TextColor(TEXT)),
            ],
        )],
    ));

    // Bottom left: operator card.
    commands.spawn((
        ChildOf(root),
        CardText,
        Text::new(""),
        font(14.0),
        TextColor(TEXT),
        Node {
            position_type: PositionType::Absolute,
            left: px(12.0),
            bottom: px(12.0),
            padding: UiRect::all(px(8.0)),
            ..default()
        },
        BackgroundColor(PANEL),
    ));

    // Centre, below the crosshair: prompt, progress bar, notice.
    commands.spawn((
        ChildOf(root),
        Node {
            position_type: PositionType::Absolute,
            top: percent(57.0),
            width: percent(100.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(6.0),
            ..default()
        },
        children![
            (
                PromptText,
                Text::new(""),
                font(17.0),
                TextColor(TEXT),
                TextLayout::justify(Justify::Center),
                Node {
                    padding: UiRect::axes(px(10.0), px(4.0)),
                    ..default()
                },
                BackgroundColor(PANEL),
            ),
            (
                PromptBar,
                Node {
                    width: px(260.0),
                    height: px(8.0),
                    border: UiRect::all(px(1.0)),
                    ..default()
                },
                BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.5)),
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
                children![(
                    PromptFill,
                    Node {
                        width: percent(0.0),
                        height: percent(100.0),
                        ..default()
                    },
                    BackgroundColor(ATTACK),
                )],
            ),
            (NoticeText, Text::new(""), font(14.0), TextColor(TEXT)),
        ],
    ));

    // Upper middle: round/match result banner.
    commands.spawn((
        ChildOf(root),
        Banner,
        Node {
            position_type: PositionType::Absolute,
            top: percent(24.0),
            width: percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
        Visibility::Hidden,
        children![(
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                padding: UiRect::axes(px(36.0), px(12.0)),
                row_gap: px(4.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            children![
                (BannerTitle, Text::new(""), font(38.0), TextColor(TEXT)),
                (
                    BannerDetail,
                    Text::new(""),
                    font(17.0),
                    TextColor(TEXT),
                    TextLayout::justify(Justify::Center),
                ),
            ],
        )],
    ));

    // Centre: operator selection.
    commands.spawn((
        ChildOf(root),
        SelectPanel,
        Node {
            position_type: PositionType::Absolute,
            width: percent(100.0),
            height: percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        Visibility::Hidden,
        children![(
            SelectText,
            Text::new(""),
            font(16.0),
            TextColor(TEXT),
            Node {
                padding: UiRect::all(px(16.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
        )],
    ));

    // World labels: one per site, the defuser, a dropped defuser.
    let labels = (0..layout.sites.len())
        .map(WorldLabel::Site)
        .chain([WorldLabel::Defuser, WorldLabel::Dropped]);
    for label in labels {
        commands.spawn((
            ChildOf(root),
            label,
            Node {
                position_type: PositionType::Absolute,
                width: px(LABEL_WIDTH),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Visibility::Hidden,
            children![(
                LabelText,
                Text::new(""),
                font(14.0),
                TextColor(SITE),
                TextLayout::justify(Justify::Center),
                Node {
                    padding: UiRect::axes(px(5.0), px(1.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
            )],
        ));
    }
}

/// Only touch the text when it changes, so the UI isn't re-laid out every frame.
fn set_text(text: &mut Mut<Text>, value: String) {
    if text.0 != value {
        text.0 = value;
    }
}

fn set_visible(visibility: &mut Mut<Visibility>, visible: bool) {
    visibility.set_if_neq(if visible { Visibility::Inherited } else { Visibility::Hidden });
}

fn clock(seconds: f32) -> String {
    let s = seconds.max(0.0).ceil() as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Hidden when there is no round loop, unless free play has an operator.
fn show_hud(
    round: Res<Round>,
    operator: Query<(), (With<LocalPlayer>, With<Operator>)>,
    mut root: Single<&mut Visibility, With<HudRoot>>,
) {
    set_visible(&mut root, round.phase != RoundPhase::Inactive || !operator.is_empty());
}

type LocalView<'a> = (&'a Team, Option<&'a Squad>);

fn update_top_bar(
    round: Res<Round>,
    local: Query<LocalView, With<LocalPlayer>>,
    mut score: Single<(&mut Text, &mut TextColor), (With<ScoreText>, Without<PhaseText>)>,
    mut phase: Single<(&mut Text, &mut TextColor), With<PhaseText>>,
) {
    let (team, squad) = local
        .single()
        .map(|(t, s)| (*t, s.copied().unwrap_or(Squad::Blue)))
        .unwrap_or((Team::Attackers, Squad::Blue));
    let mine = round.score_of(squad);
    let theirs = round.score_of(squad.other());
    let side = if team == Team::Attackers { "ATTACKING" } else { "DEFENDING" };
    let line = if round.phase == RoundPhase::Inactive {
        side.to_string()
    } else {
        format!("YOU {mine}  :  {theirs} THEM      ROUND {}  -  {side}", round.number)
    };
    set_text(&mut score.0, line);
    score.1.0 = side_color(team).mix(&TEXT, 0.4);

    let (label, color) = match round.phase {
        RoundPhase::Planted => (format!("DEFUSER  {}", clock(round.time_left)), ALERT),
        RoundPhase::Inactive | RoundPhase::RoundEnd | RoundPhase::MatchEnd => {
            (round.phase.label().to_string(), TEXT)
        }
        p => (format!("{}  {}", p.label(), clock(round.time_left)), TEXT),
    };
    set_text(&mut phase.0, label);
    phase.1.0 = color;
}

fn update_operator_card(
    round: Res<Round>,
    tuning: Res<Tuning>,
    local: Query<
        (
            &Team,
            Option<&Operator>,
            Option<&UniqueGadget>,
            Option<&DefuserCarrier>,
            Option<&Loadout>,
        ),
        With<LocalPlayer>,
    >,
    mut card: Single<(&mut Text, &mut Visibility), With<CardText>>,
) {
    let Ok((team, operator, gadget, carrier, loadout)) = local.single() else {
        return;
    };
    let Some(Operator(op)) = operator.filter(|_| round.phase != RoundPhase::OperatorSelect) else {
        set_visible(&mut card.1, false);
        return;
    };
    set_visible(&mut card.1, true);
    let info = op.info();
    let numbers = op.numbers(&tuning.round);
    let mut lines = vec![
        format!("{}  -  {}  ({})", info.callsign, info.role, team_label(*team)),
        format!("Armour {}/3   Speed {}/3", numbers.armour, numbers.speed),
    ];
    if let Some(l) = loadout {
        lines.push(format!("[1] {}  [2] {}", l.primary, l.sidearm));
    }
    if let Some(g) = gadget {
        let built = if g.kind.is_built() { "" } else { "  (not built yet)" };
        lines.push(format!("[3] {}  {}/{}{built}", g.kind.name(), g.remaining, g.max));
    }
    if let Some(l) = loadout {
        lines.push(format!("[4] {}  (not built yet)", l.secondary.name()));
    }
    if let Some(c) = carrier {
        lines.push(format!(
            "[5] Defuser  {}",
            if c.equipped { "IN HAND" } else { "carried" }
        ));
    }
    set_text(&mut card.0, lines.join("\n"));
}

#[allow(clippy::type_complexity)]
fn update_prompt(
    round: Res<Round>,
    layout: Res<KeepLayout>,
    tuning: Res<Tuning>,
    local: Query<
        (
            &Transform,
            &Team,
            Has<Dead>,
            Option<&ObjectiveInteraction>,
            Option<&DefuserCarrier>,
        ),
        With<LocalPlayer>,
    >,
    devices: Query<(&Transform, &DefuserDevice), Without<LocalPlayer>>,
    dropped: Query<(), With<DroppedDefuser>>,
    mut prompt: Single<(&mut Text, &mut Visibility), With<PromptText>>,
    mut bar: Single<&mut Visibility, (With<PromptBar>, Without<PromptText>)>,
    mut fill: Single<(&mut Node, &mut BackgroundColor), With<PromptFill>>,
) {
    let Ok((transform, team, dead, interaction, carrier)) = local.single() else {
        return;
    };
    let feet = transform.translation;
    let device = devices.iter().find(|(_, d)| !d.disabled);
    let attacker = *team == Team::Attackers;
    let mut progress: Option<(f32, Color)> = None;

    let text = if dead {
        "You are down".to_string()
    } else if let Some(i) = interaction {
        let (verb, color) = match i.kind {
            InteractionKind::Plant => ("Planting defuser", ATTACK),
            InteractionKind::Disable => ("Disabling defuser", DEFEND),
        };
        progress = Some((i.progress, color));
        format!("{verb}  {:.0}%", (i.progress * 100.0).min(100.0))
    } else {
        match (round.phase, attacker) {
            (RoundPhase::Prep, true) => "Prep: hold at spawn (recon drones not built yet)".into(),
            (RoundPhase::Prep, false) => "Prep: set up your defences".into(),
            (RoundPhase::Action, true) => match (carrier, layout.site_at(feet)) {
                (Some(_), Some(site)) => format!("Hold F to plant the defuser (site {})", site.id.letter()),
                (Some(_), None) => "Take the defuser to site A or B".into(),
                (None, _) if !dropped.is_empty() => "Pick up the dropped defuser".into(),
                (None, _) => "Cover the defuser carrier".into(),
            },
            (RoundPhase::Action, false) => "Defend sites A and B".into(),
            (RoundPhase::Planted, false) => match device {
                Some((at, _)) if in_reach(feet, at.translation, tuning.round.disable_reach) => {
                    "Hold F to disable the defuser".into()
                }
                _ => "Find and disable the defuser".into(),
            },
            (RoundPhase::Planted, true) => match device {
                Some((_, d)) if d.disabler.is_some() => {
                    progress = Some((d.disable_progress, ALERT));
                    format!("Defuser is being disabled!  {:.0}%", d.disable_progress * 100.0)
                }
                _ => "Defend the defuser".into(),
            },
            _ => String::new(),
        }
    };
    set_visible(&mut prompt.1, !text.is_empty());
    set_text(&mut prompt.0, text);
    set_visible(&mut bar, progress.is_some());
    if let Some((p, color)) = progress {
        fill.0.width = percent((p * 100.0).clamp(0.0, 100.0));
        fill.1.0 = color;
    }
}

fn collect_notices(
    mut notice: ResMut<Notice>,
    mut round_events: MessageReader<RoundEvent>,
    mut gadget_events: MessageReader<GadgetEvent>,
    local: Query<(Entity, &Team, &Transform), With<LocalPlayer>>,
    tuning: Res<Tuning>,
    time: Res<Time>,
) {
    let Ok((me, my_team, at)) = local.single() else {
        return;
    };
    let mut post = |text: String, color: Color| {
        notice.text = text;
        notice.color = color;
        notice.shown_at = time.elapsed_secs();
    };
    for event in round_events.read() {
        match event {
            RoundEvent::PlantRefused { entity, reason } if *entity == me => {
                post(format!("Can't plant: {reason}"), ALERT)
            }
            RoundEvent::DefuserEquipped { entity, equipped } if *entity == me => post(
                if *equipped { "Defuser in hand".into() } else { "Defuser stowed".into() },
                TEXT,
            ),
            RoundEvent::PlantStarted { entity, .. } | RoundEvent::DisableStarted { entity }
                if *entity == me =>
            {
                post(String::new(), TEXT)
            }
            RoundEvent::DefuserAssigned { entity } if *entity == me => {
                post("You carry the defuser [5]".into(), ATTACK)
            }
            RoundEvent::PlantInterrupted { entity, reason, .. }
            | RoundEvent::DisableInterrupted { entity, reason, .. }
                if *entity == me =>
            {
                post(format!("Interrupted ({reason:?})"), ALERT)
            }
            RoundEvent::Planted { site, .. } => {
                post(format!("Defuser planted at site {}", site.letter()), ALERT)
            }
            RoundEvent::DefuserDropped { .. } => post("Defuser dropped".into(), ALERT),
            _ => {}
        }
    }
    for event in gadget_events.read() {
        match event {
            GadgetEvent::Refused { by, reason } if *by == me => {
                post(format!("Gadget: {reason}"), ALERT)
            }
            GadgetEvent::DartStuck { position, team }
                if team != my_team
                    && position.distance(at.translation) <= tuning.round.darts.hiss_radius =>
            {
                post("A sensor dart hissed nearby".into(), ALERT)
            }
            GadgetEvent::StakePing { team, .. } if team == my_team => {
                post("Stake: movement detected".into(), DEFEND)
            }
            GadgetEvent::StakeSaturated { .. } if *my_team == Team::Defenders => {
                post("Stake saturated by noise".into(), DIM)
            }
            _ => {}
        }
    }
}

fn update_notice(
    notice: Res<Notice>,
    time: Res<Time>,
    mut text: Single<(&mut Text, &mut TextColor), With<NoticeText>>,
) {
    const SHOW: f32 = 2.5;
    let age = time.elapsed_secs() - notice.shown_at;
    if notice.text.is_empty() || age > SHOW {
        set_text(&mut text.0, String::new());
        return;
    }
    set_text(&mut text.0, notice.text.clone());
    text.1.0 = notice.color.with_alpha((SHOW - age).min(1.0));
}

fn update_banner(
    round: Res<Round>,
    local: Query<Option<&Squad>, With<LocalPlayer>>,
    mut banner: Single<&mut Visibility, With<Banner>>,
    mut title: Single<(&mut Text, &mut TextColor), (With<BannerTitle>, Without<BannerDetail>)>,
    mut detail: Single<&mut Text, With<BannerDetail>>,
) {
    let squad = local.single().ok().flatten().copied().unwrap_or(Squad::Blue);
    let score = format!("YOU {}  :  {} THEM", round.score_of(squad), round.score_of(squad.other()));
    match (round.phase, round.result, round.match_winner) {
        (RoundPhase::RoundEnd, Some(result), _) => {
            let won = round.squad_on(result.winner) == squad;
            set_visible(&mut banner, true);
            set_text(&mut title.0, if won { "ROUND WON" } else { "ROUND LOST" }.into());
            title.1.0 = if won { Color::srgb(0.6, 0.95, 0.55) } else { ALERT };
            set_text(
                &mut detail,
                format!("{} WIN  -  {}\n{score}", team_label(result.winner), result.reason.label()),
            );
        }
        (RoundPhase::MatchEnd, _, Some(winner)) => {
            let won = winner == squad;
            set_visible(&mut banner, true);
            set_text(&mut title.0, if won { "MATCH WON" } else { "MATCH LOST" }.into());
            title.1.0 = if won { Color::srgb(0.6, 0.95, 0.55) } else { ALERT };
            set_text(&mut detail, format!("{score}\nA new match starts shortly"));
        }
        _ => set_visible(&mut banner, false),
    }
}

fn update_select_panel(
    round: Res<Round>,
    tuning: Res<Tuning>,
    local: Query<(&Team, &OperatorChoice), With<LocalPlayer>>,
    mut panel: Single<&mut Visibility, With<SelectPanel>>,
    mut text: Single<&mut Text, With<SelectText>>,
) {
    let Ok((team, choice)) = local.single() else {
        return;
    };
    let open = round.phase == RoundPhase::OperatorSelect;
    set_visible(&mut panel, open);
    if !open {
        return;
    }
    let side = if *team == Team::Attackers { "ATTACKING" } else { "DEFENDING" };
    let mut lines = vec![
        format!("ROUND {}  -  YOU ARE {side}", round.number),
        format!("Choose your operator  (auto-pick in {} s)", round.time_left.max(0.0).ceil()),
        String::new(),
    ];
    for (i, op) in OperatorId::for_side(*team).enumerate() {
        let info = op.info();
        let n = op.numbers(&tuning.round);
        let mark = if choice.locked == Some(op) { ">" } else { " " };
        let built = if info.gadget.is_built() { "" } else { "  (not built yet)" };
        lines.push(format!("{mark} [{}] {:<8} {}", i + 1, info.callsign, info.role));
        lines.push(format!(
            "      Armour {}  Speed {}   {} x{}{built}",
            n.armour,
            n.speed,
            info.gadget.name(),
            n.gadgets
        ));
    }
    if let Some(first) = OperatorId::for_side(*team).next() {
        let options: Vec<_> = first.info().secondaries.iter().map(|g| g.name()).collect();
        lines.push(String::new());
        lines.push(format!("Secondary gadgets: {}", options.join(", ")));
    }
    if let Some(op) = choice.locked {
        lines.push(String::new());
        lines.push(format!("Locked in: {}", op.info().callsign));
    }
    set_text(&mut text, lines.join("\n"));
}

#[allow(clippy::type_complexity)]
fn update_world_labels(
    round: Res<Round>,
    layout: Res<KeepLayout>,
    camera: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    local: Query<(&Transform, &Team), With<LocalPlayer>>,
    devices: Query<(&GlobalTransform, &DefuserDevice)>,
    dropped: Query<&GlobalTransform, With<DroppedDefuser>>,
    mut labels: Query<(&WorldLabel, &mut Node, &mut Visibility, &Children)>,
    mut texts: Query<(&mut Text, &mut TextColor), With<LabelText>>,
) {
    let (Ok((camera, eye)), Ok((me, team))) = (camera.single(), local.single()) else {
        return;
    };
    let planted = devices
        .iter()
        .find(|(_, d)| !d.disabled)
        .map(|(t, d)| (t.translation(), d.site));
    let live = round.phase.is_live();
    for (label, mut node, mut visibility, children) in &mut labels {
        let target = match *label {
            WorldLabel::Site(i) if live && planted.is_none() => layout.sites.get(i).map(|s| {
                (s.centre() + Vec3::Y * 1.3, format!("{}  {}", s.id.letter(), s.name), SITE)
            }),
            WorldLabel::Defuser if live => planted
                .map(|(p, site)| (p + Vec3::Y * 0.6, format!("DEFUSER ({})", site.letter()), ALERT)),
            WorldLabel::Dropped if live && *team == Team::Attackers => dropped
                .iter()
                .next()
                .map(|t| (t.translation() + Vec3::Y * 0.5, "DROPPED DEFUSER".to_string(), ATTACK)),
            _ => None,
        };
        let screen = target
            .filter(|(world, _, _)| me.translation.distance(*world) >= LABEL_MIN_DISTANCE)
            .and_then(|(world, name, c)| {
                camera
                    .world_to_viewport(eye, world)
                    .ok()
                    .map(|p| (p, world, name, c))
            });
        let Some((p, world, name, c)) = screen else {
            set_visible(&mut visibility, false);
            continue;
        };
        set_visible(&mut visibility, true);
        let distance = me.translation.distance(world);
        if let Some(mut text) = children.first().and_then(|&c| texts.get_mut(c).ok()) {
            set_text(&mut text.0, format!("{name}  {distance:.0} m"));
            text.1.0 = c;
        }
        node.left = px(p.x - LABEL_WIDTH / 2.0);
        node.top = px(p.y - 10.0);
    }
}
