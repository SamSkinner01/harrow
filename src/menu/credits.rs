//! The credits page: `CREDITS.md` (compiled in), so every attribution is
//! visible in game. Mouse wheel or Up/Down... scroll it; Esc or Back leaves.

use bevy::{
    input::mouse::{MouseScrollUnit, MouseWheel},
    prelude::*,
};

use super::{MenuButton, widgets};
use crate::ui::style::{self, palette, size};

const CREDITS: &str = include_str!("../../CREDITS.md");

pub fn build(app: &mut App) {
    app.add_systems(Update, scroll_credits);
}

/// The scrolling list.
#[derive(Component)]
struct CreditsList;

pub fn spawn(commands: &mut Commands) {
    let screen = commands
        .spawn(widgets::screen("Credits", JustifyContent::Center, ()))
        .id();
    let panel = commands
        .spawn((widgets::panel(760.0, ()), ChildOf(screen)))
        .id();
    commands.spawn((widgets::title("CREDITS"), ChildOf(panel)));
    let list = commands
        .spawn((
            CreditsList,
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(2.0),
                max_height: vh(62.0),
                overflow: Overflow::scroll_y(),
                padding: UiRect::right(px(8.0)),
                ..default()
            },
            ScrollPosition::default(),
            ChildOf(panel),
        ))
        .id();
    for line in markdown_lines(CREDITS) {
        let bundle = match line {
            Line::Heading(text) => (
                Text::new(text),
                style::bold(size::BODY),
                TextColor(palette::ACCENT),
                Node {
                    margin: UiRect::top(px(10.0)),
                    ..default()
                },
            ),
            Line::Item(text) => (
                Text::new(format!("    {text}")),
                style::text(size::SMALL),
                TextColor(palette::TEXT),
                Node::default(),
            ),
            Line::Para(text) => (
                Text::new(text),
                style::text(size::SMALL),
                TextColor(palette::DIM),
                Node::default(),
            ),
        };
        commands.spawn((bundle, ChildOf(list)));
    }
    commands.spawn((
        widgets::body("Mouse wheel or arrow keys to scroll.", size::CAPTION, palette::FAINT),
        Node {
            margin: UiRect::top(px(8.0)),
            ..default()
        },
        ChildOf(panel),
    ));
    let buttons = commands
        .spawn((
            Node {
                margin: UiRect::top(px(6.0)),
                ..default()
            },
            ChildOf(panel),
        ))
        .id();
    commands.spawn((widgets::button(MenuButton::Back, 0, "Back"), ChildOf(buttons)));
}

enum Line {
    Heading(String),
    Item(String),
    Para(String),
}

/// Just enough Markdown for CREDITS.md: headings, bullet items, and
/// paragraphs (joined lines); links show their text.
fn markdown_lines(source: &str) -> Vec<Line> {
    let mut out = Vec::new();
    let mut para = String::new();
    let flush = |para: &mut String, out: &mut Vec<Line>| {
        if !para.is_empty() {
            out.push(Line::Para(std::mem::take(para)));
        }
    };
    for raw in source.lines() {
        let line = strip_links(raw.trim());
        if line.is_empty() {
            flush(&mut para, &mut out);
        } else if let Some(heading) = line.strip_prefix('#') {
            flush(&mut para, &mut out);
            let text = heading.trim_start_matches('#').trim();
            // The page title says it already.
            if !raw.starts_with("# ") {
                out.push(Line::Heading(text.to_uppercase()));
            }
        } else if let Some(item) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            flush(&mut para, &mut out);
            out.push(Line::Item(item.to_string()));
        } else if line.starts_with('|') {
            // Table rows: cells joined, separators dropped.
            flush(&mut para, &mut out);
            let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
            if !cells.iter().all(|c| c.chars().all(|ch| ch == '-' || ch == ':')) {
                out.push(Line::Item(cells.join("  /  ")));
            }
        } else {
            if !para.is_empty() {
                para.push(' ');
            }
            para.push_str(&line);
        }
    }
    flush(&mut para, &mut out);
    out
}

/// `[text](url)` -> `text`; backticks and emphasis markers dropped.
fn strip_links(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(start) = rest.find('[') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match (after.find("]("), after.find(')')) {
            (Some(mid), Some(end)) if mid < end => {
                out.push_str(&after[..mid]);
                rest = &after[end + 1..];
            }
            _ => {
                out.push('[');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out.replace(['`', '*'], "")
}

fn scroll_credits(
    mut wheel: MessageReader<MouseWheel>,
    keys: Res<ButtonInput<KeyCode>>,
    mut lists: Query<(&mut ScrollPosition, &ComputedNode), With<CreditsList>>,
) {
    let mut delta = 0.0;
    for event in wheel.read() {
        delta -= match event.unit {
            MouseScrollUnit::Line => event.y * 24.0,
            MouseScrollUnit::Pixel => event.y,
        };
    }
    if keys.pressed(KeyCode::ArrowDown) {
        delta += 8.0;
    }
    if keys.pressed(KeyCode::ArrowUp) {
        delta -= 8.0;
    }
    if delta == 0.0 {
        return;
    }
    for (mut scroll, computed) in &mut lists {
        let max = ((computed.content_size() - computed.size()) * computed.inverse_scale_factor()).y.max(0.0);
        scroll.y = (scroll.y + delta).clamp(0.0, max);
    }
}
