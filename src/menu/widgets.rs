//! Menu building blocks in the shared UI style (`ui::style`): screens,
//! panels, headings, buttons and text fields, plus keyboard/mouse focus.
//!
//! Focus: every focusable element has a [`Nav`] index (its order on the
//! page). Up/Down (or W/S) move the focus, Enter/Space activate it,
//! Left/Right change a settings row; hovering with the mouse moves the focus
//! too. The focused element is highlighted like a hovered one.

use bevy::{picking::hover::Hovered, prelude::*, ui::Pressed, ui_widgets::Button};

use crate::ui::{
    NotHud,
    style::{self, palette, size},
};

/// Order of a focusable element on the current page.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Nav(pub u32);

/// Which [`Nav`] element has the keyboard focus.
#[derive(Resource, Default, Debug)]
pub struct Focus(pub Option<u32>);

/// Gives an element the focusable look (background follows hover, press
/// and focus).
#[derive(Component)]
pub struct Focusable;

/// Every element of the open menu page (despawned when the page changes).
#[derive(Component)]
pub struct PageRoot;

/// Full-screen layer over the game: a scrim darkening the world, content
/// laid out by `justify` (start = left column, center = centred).
pub fn screen(name: &'static str, justify: JustifyContent, children: impl Bundle) -> impl Bundle {
    (
        Name::new(name),
        PageRoot,
        // Not part of the HUD layer (which is hidden behind the main menu).
        NotHud,
        Node {
            position_type: PositionType::Absolute,
            width: percent(100.0),
            height: percent(100.0),
            justify_content: justify,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(palette::SCRIM),
        GlobalZIndex(10),
        children,
    )
}

/// A full-screen layer whose left side is dark and right side clear (the
/// main menu, so the keep shows on the right).
pub fn side_screen(name: &'static str, children: impl Bundle) -> impl Bundle {
    (
        Name::new(name),
        PageRoot,
        // Not part of the HUD layer (which is hidden behind the main menu).
        NotHud,
        Node {
            position_type: PositionType::Absolute,
            width: percent(100.0),
            height: percent(100.0),
            justify_content: JustifyContent::FlexStart,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundGradient::from(LinearGradient::to_right(vec![
            ColorStop::percent(Color::srgba(0.03, 0.025, 0.02, 0.86), 0.0),
            ColorStop::percent(Color::srgba(0.03, 0.025, 0.02, 0.62), 32.0),
            ColorStop::percent(Color::srgba(0.03, 0.025, 0.02, 0.0), 62.0),
        ])),
        GlobalZIndex(10),
        children,
    )
}

/// A panel: a column on a dark translucent card with a thin top accent.
pub fn panel(width: f32, children: impl Bundle) -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            row_gap: px(8.0),
            padding: UiRect::new(px(28.0), px(28.0), px(22.0), px(24.0)),
            width: px(width),
            max_width: percent(94.0),
            max_height: percent(94.0),
            border: UiRect::top(px(2.0)),
            ..default()
        },
        BorderColor::all(palette::ACCENT),
        BackgroundColor(Color::srgba(0.045, 0.038, 0.03, 0.88)),
        children,
    )
}

/// A page title (accent colour, bold).
pub fn title(text: impl Into<String>) -> impl Bundle {
    (
        Text::new(text),
        style::bold(size::TITLE),
        TextColor(palette::ACCENT),
        Node {
            margin: UiRect::bottom(px(6.0)),
            ..default()
        },
    )
}

/// A section heading inside a page.
pub fn heading(text: impl Into<String>) -> impl Bundle {
    (
        Text::new(text),
        style::bold(size::SMALL),
        TextColor(palette::DIM),
        Node {
            margin: UiRect::top(px(10.0)),
            padding: UiRect::bottom(px(3.0)),
            border: UiRect::bottom(px(1.0)),
            ..default()
        },
        BorderColor::all(palette::BUTTON_EDGE),
    )
}

/// Plain text at a size from the type scale.
pub fn body(text: impl Into<String>, px: f32, color: Color) -> impl Bundle {
    (Text::new(text), style::text(px), TextColor(color))
}

/// A focusable, clickable menu button carrying `action`.
pub fn button<A: Component>(action: A, nav: u32, label: impl Into<String>) -> impl Bundle {
    (
        action,
        Button,
        Hovered::default(),
        Focusable,
        Nav(nav),
        Node {
            padding: UiRect::axes(px(18.0), px(9.0)),
            justify_content: JustifyContent::FlexStart,
            align_items: AlignItems::Center,
            min_width: px(220.0),
            border: UiRect::left(px(3.0)),
            ..default()
        },
        BorderColor::all(Color::NONE),
        BackgroundColor(palette::BUTTON),
        children![(Text::new(label), style::bold(size::LARGE), TextColor(palette::TEXT))],
    )
}

/// Background and accent edge of focusable elements: pressed, hovered or
/// keyboard-focused.
#[allow(clippy::type_complexity)]
pub fn focus_colors(
    focus: Res<Focus>,
    mut items: Query<(&Nav, Option<&Hovered>, Has<Pressed>, &mut BackgroundColor, &mut BorderColor), With<Focusable>>,
) {
    for (nav, hovered, pressed, mut background, mut border) in &mut items {
        let focused = focus.0 == Some(nav.0);
        let hover = hovered.is_some_and(|h| h.0);
        let wanted = if pressed {
            palette::BUTTON_PRESS
        } else if hover || focused {
            palette::BUTTON_HOVER
        } else {
            palette::BUTTON
        };
        if background.0 != wanted {
            background.0 = wanted;
        }
        let edge = if focused || hover { palette::ACCENT } else { Color::NONE };
        let edges = BorderColor::all(edge);
        if *border != edges {
            *border = edges;
        }
    }
}

/// The mouse moves the focus to what it hovers.
pub fn hover_focus(items: Query<(&Nav, &Hovered), Changed<Hovered>>, mut focus: ResMut<Focus>) {
    for (nav, hovered) in &items {
        if hovered.0 && focus.0 != Some(nav.0) {
            focus.0 = Some(nav.0);
        }
    }
}

/// Up/Down (and Tab) move the focus through the page's elements.
pub fn move_focus(keys: Res<ButtonInput<KeyCode>>, items: Query<&Nav>, mut focus: ResMut<Focus>) {
    let up = keys.just_pressed(KeyCode::ArrowUp);
    let down = keys.any_just_pressed([KeyCode::ArrowDown, KeyCode::Tab]);
    if !(up || down) {
        return;
    }
    let mut order: Vec<u32> = items.iter().map(|n| n.0).collect();
    order.sort_unstable();
    order.dedup();
    if order.is_empty() {
        return;
    }
    let at = focus.0.and_then(|f| order.iter().position(|&n| n == f));
    let next = match (at, down) {
        (None, true) => 0,
        (None, false) => order.len() - 1,
        (Some(i), true) => (i + 1) % order.len(),
        (Some(i), false) => (i + order.len() - 1) % order.len(),
    };
    focus.0 = Some(order[next]);
}
