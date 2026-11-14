//! The one look shared by every HUD and menu: palette, type scale, fonts
//! and spacing. HUD modules use these instead of their own colours and
//! sizes, so everything on screen reads as one design.
//!
//! - Type: Noto Sans Condensed (Medium for text, SemiBold for headings,
//!   SIL OFL 1.1, `assets/fonts/`), embedded in the binary; tabular text
//!   that lines up in columns (operator card, selection screen, key help)
//!   uses the monospace default font. Sizes come from [`size`], six steps.
//! - Colour: warm off-white text on warm near-black translucent panels,
//!   sand/ochre accent; attackers ochre, defenders a muted sky blue.

use bevy::{asset::uuid_handle, prelude::*};

/// Colours. Text alphas are baked in; panels are translucent.
pub mod palette {
    use bevy::prelude::Color;

    /// Panels behind HUD text.
    pub const PANEL: Color = Color::srgba(0.055, 0.047, 0.035, 0.58);
    /// Lighter panels (labels in the world, notices).
    pub const PANEL_SOFT: Color = Color::srgba(0.055, 0.047, 0.035, 0.38);
    /// Full-screen menu backdrop over the world.
    pub const SCRIM: Color = Color::srgba(0.03, 0.025, 0.02, 0.55);
    /// Main text: warm off-white.
    pub const TEXT: Color = Color::srgba(0.96, 0.93, 0.86, 0.95);
    /// Secondary text.
    pub const DIM: Color = Color::srgba(0.96, 0.93, 0.86, 0.62);
    /// Hints, build ids.
    pub const FAINT: Color = Color::srgba(0.96, 0.93, 0.86, 0.38);
    /// Sand/ochre accent: titles, progress fills, focus.
    pub const ACCENT: Color = Color::srgb(0.87, 0.67, 0.38);
    pub const ATTACK: Color = Color::srgb(0.93, 0.62, 0.30);
    pub const DEFEND: Color = Color::srgb(0.52, 0.74, 0.90);
    pub const ALERT: Color = Color::srgb(0.94, 0.36, 0.26);
    pub const GOOD: Color = Color::srgb(0.62, 0.86, 0.52);
    /// Bomb-site labels.
    pub const SITE: Color = Color::srgb(0.95, 0.78, 0.38);
    /// Progress bars: track and edge.
    pub const BAR_TRACK: Color = Color::srgba(0.0, 0.0, 0.0, 0.55);
    pub const BAR_EDGE: Color = Color::srgba(0.96, 0.93, 0.86, 0.45);
    /// Buttons (menus).
    pub const BUTTON: Color = Color::srgba(0.96, 0.93, 0.86, 0.07);
    pub const BUTTON_HOVER: Color = Color::srgba(0.87, 0.67, 0.38, 0.28);
    pub const BUTTON_PRESS: Color = Color::srgba(0.87, 0.67, 0.38, 0.55);
    pub const BUTTON_EDGE: Color = Color::srgba(0.96, 0.93, 0.86, 0.16);
}

/// Type scale, logical pixels.
pub mod size {
    /// Build id, fine print.
    pub const CAPTION: f32 = 12.0;
    /// Secondary HUD text, labels, help.
    pub const SMALL: f32 = 14.0;
    /// Prompts, notices, card text, menu body.
    pub const BODY: f32 = 16.0;
    /// Buttons, the phase timer, section headings.
    pub const LARGE: f32 = 21.0;
    /// Screen titles, the ammo count.
    pub const TITLE: f32 = 30.0;
    /// Round banners.
    pub const DISPLAY: f32 = 44.0;
    /// The main menu's wordmark.
    pub const HERO: f32 = 96.0;
}

/// Spacing, logical pixels.
pub mod space {
    /// Gap between the screen edge and HUD panels.
    pub const EDGE: f32 = 18.0;
    /// Padding inside panels.
    pub const PAD: f32 = 8.0;
}

/// Noto Sans Condensed Medium (text).
pub const FONT: Handle<Font> = uuid_handle!("6a1c3f0e-6b0e-4d2a-9a51-0d1f3c5e7a11");
/// Noto Sans Condensed SemiBold (headings, buttons, numbers).
pub const FONT_BOLD: Handle<Font> = uuid_handle!("6a1c3f0e-6b0e-4d2a-9a51-0d1f3c5e7a12");

/// Text in the UI font.
pub fn text(size: f32) -> TextFont {
    TextFont {
        font: FONT.into(),
        font_size: FontSize::Px(size),
        ..default()
    }
}

/// Headings, buttons and big numbers.
pub fn bold(size: f32) -> TextFont {
    TextFont {
        font: FONT_BOLD.into(),
        font_size: FontSize::Px(size),
        ..default()
    }
}

/// Monospace, for text laid out in columns with spaces.
pub fn mono(size: f32) -> TextFont {
    TextFont {
        font_size: FontSize::Px(size),
        ..default()
    }
}

/// Puts the embedded fonts behind [`FONT`] and [`FONT_BOLD`].
pub struct UiStylePlugin;

impl Plugin for UiStylePlugin {
    fn build(&self, app: &mut App) {
        // A dedicated server draws nothing; its build doesn't carry the fonts.
        #[cfg(feature = "client")]
        app.add_systems(PreStartup, insert_fonts);
    }
}

#[cfg(feature = "client")]
fn insert_fonts(fonts: Option<ResMut<Assets<Font>>>) {
    let Some(mut fonts) = fonts else {
        return;
    };
    let medium = include_bytes!("../../assets/fonts/NotoSans-CondensedMedium.ttf");
    let semibold = include_bytes!("../../assets/fonts/NotoSans-CondensedSemiBold.ttf");
    for (handle, bytes) in [(&FONT, &medium[..]), (&FONT_BOLD, &semibold[..])] {
        if let Err(err) = fonts.insert(handle, Font::from_bytes(bytes.to_vec())) {
            warn!("UI font: {err}");
        }
    }
}
