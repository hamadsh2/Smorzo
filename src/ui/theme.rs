// SPDX-License-Identifier: GPL-3.0-only
//! Design tokens from the design skill. Every color, size and radius in the UI comes
//! from here; components never hard-code a value.
//!
//! Colors come in themes the user picks in Settings (Warm stone by default). A
//! component asks for a *token* (`c(ACCENT)`), and `c` looks it up in the theme that
//! is active right now, so switching a theme just redraws the window.
//!
//! Python analogy: a constants module (`theme.py`) whose color names are keys into a
//! dict of palettes, plus an `apply()` that pushes the same values into the component
//! library's global theme so its widgets don't keep their default (blue/grey) look.

use std::sync::atomic::{AtomicU8, Ordering::Relaxed};

use gpui::{App, Hsla, Pixels, px, rgb};
use gpui_component::{Theme as UiTheme, ThemeMode};

/// A color role. Which hex it is depends on the active theme.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tok {
    Bg,
    Surface,
    Elevated,
    Border,
    TextPrimary,
    TextSecondary,
    Accent,
    AccentHover,
    /// Text and icons on an `accent` fill.
    OnAccent,
    Warning,
    MeterPeak,
}

pub const BG: Tok = Tok::Bg;
pub const SURFACE: Tok = Tok::Surface;
pub const ELEVATED: Tok = Tok::Elevated;
pub const BORDER: Tok = Tok::Border;
pub const TEXT_PRIMARY: Tok = Tok::TextPrimary;
pub const TEXT_SECONDARY: Tok = Tok::TextSecondary;
pub const ACCENT: Tok = Tok::Accent;
pub const ACCENT_HOVER: Tok = Tok::AccentHover;
pub const ON_ACCENT: Tok = Tok::OnAccent;
pub const WARNING: Tok = Tok::Warning;
/// Meter peak-hold tick and the top ~10% of the scale.
pub const METER_PEAK: Tok = Tok::MeterPeak;

/// One theme's colors (`0xRRGGBB`).
pub struct Palette {
    pub dark: bool,
    pub bg: u32,
    pub surface: u32,
    pub elevated: u32,
    pub border: u32,
    pub text_primary: u32,
    pub text_secondary: u32,
    pub accent: u32,
    pub accent_hover: u32,
    pub on_accent: u32,
    pub warning: u32,
    pub meter_peak: u32,
}

impl Palette {
    pub fn get(&self, t: Tok) -> u32 {
        match t {
            Tok::Bg => self.bg,
            Tok::Surface => self.surface,
            Tok::Elevated => self.elevated,
            Tok::Border => self.border,
            Tok::TextPrimary => self.text_primary,
            Tok::TextSecondary => self.text_secondary,
            Tok::Accent => self.accent,
            Tok::AccentHover => self.accent_hover,
            Tok::OnAccent => self.on_accent,
            Tok::Warning => self.warning,
            Tok::MeterPeak => self.meter_peak,
        }
    }
}

/// The themes the user can pick from, in the order Settings shows them.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ThemeName {
    /// Warm neutrals with sage: the original look, and the default.
    #[default]
    WarmStone,
    /// Cool blue-black with a soft teal.
    Midnight,
    /// Warm black with ember amber.
    InkEmber,
    /// Light: warm paper with a deeper sage.
    Paper,
}

impl ThemeName {
    pub const ALL: [ThemeName; 4] = [
        ThemeName::WarmStone,
        ThemeName::Midnight,
        ThemeName::InkEmber,
        ThemeName::Paper,
    ];

    /// The name saved in the settings file.
    pub fn id(self) -> &'static str {
        match self {
            ThemeName::WarmStone => "warm-stone",
            ThemeName::Midnight => "midnight",
            ThemeName::InkEmber => "ink-ember",
            ThemeName::Paper => "paper",
        }
    }

    /// Unknown names (e.g. from a newer version) fall back to the default.
    pub fn from_id(id: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|t| t.id() == id)
            .unwrap_or_default()
    }

    pub fn label(self) -> &'static str {
        match self {
            ThemeName::WarmStone => "Warm stone",
            ThemeName::Midnight => "Midnight",
            ThemeName::InkEmber => "Ink",
            ThemeName::Paper => "Paper",
        }
    }

    pub fn palette(self) -> &'static Palette {
        &PALETTES[self as usize]
    }
}

/// In `ThemeName` order. Contrast is checked in the tests below.
static PALETTES: [Palette; 4] = [
    Palette {
        dark: true,
        bg: 0x1C1B1A,
        surface: 0x252422,
        elevated: 0x2E2C29,
        border: 0x3A3733,
        text_primary: 0xE8E4DC,
        text_secondary: 0xA39E94,
        accent: 0x8FA89A,
        accent_hover: 0xA3BBAE,
        on_accent: 0x1C1B1A,
        warning: 0xC48A72,
        meter_peak: 0xC9B79C,
    },
    Palette {
        dark: true,
        bg: 0x121519,
        surface: 0x1A1E24,
        elevated: 0x252B34,
        border: 0x313844,
        text_primary: 0xE4E8EE,
        text_secondary: 0x98A1AE,
        accent: 0x7FB3C4,
        accent_hover: 0x97C3D1,
        on_accent: 0x121519,
        warning: 0xD08C76,
        meter_peak: 0xD9B97E,
    },
    Palette {
        dark: true,
        bg: 0x171412,
        surface: 0x211D1A,
        elevated: 0x2C2622,
        border: 0x3A332D,
        text_primary: 0xEEE6DC,
        text_secondary: 0xABA094,
        accent: 0xD49A6A,
        accent_hover: 0xDFAE85,
        on_accent: 0x171412,
        warning: 0xD47F74,
        meter_peak: 0xE6CC9A,
    },
    Palette {
        dark: false,
        bg: 0xF4F1EA,
        surface: 0xFBF9F5,
        elevated: 0xECE7DE,
        border: 0xDDD7CC,
        text_primary: 0x2B2925,
        text_secondary: 0x66615A,
        accent: 0x557263,
        accent_hover: 0x4A6557,
        on_accent: 0xFBF9F5,
        warning: 0xA4634E,
        meter_peak: 0x8F7650,
    },
];

/// The active theme (`ThemeName as u8`). Read on every draw; written when the user picks.
static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn current() -> ThemeName {
    ThemeName::ALL
        .get(CURRENT.load(Relaxed) as usize)
        .copied()
        .unwrap_or_default()
}

/// A token's color in the active theme.
pub fn c(t: Tok) -> Hsla {
    rgb(current().palette().get(t)).into()
}

/// Pick the theme to start with (before `apply`).
pub fn init(name: ThemeName) {
    CURRENT.store(name as u8, Relaxed);
}

/// Switch themes: recolor the component library and redraw every window.
pub fn set(name: ThemeName, cx: &mut App) {
    CURRENT.store(name as u8, Relaxed);
    apply(cx);
    match_title_bars();
    cx.refresh_windows();
}

/// Make the windows' title bars dark or light like the active theme.
pub fn match_title_bars() {
    let dark = current().palette().dark;
    for w in super::win::gpui_windows() {
        super::win::set_dark_title_bar(w, dark);
    }
}

// Type scale (px).
pub const CAPTION: f32 = 11.0;
pub const LABEL: f32 = 12.0;
pub const BODY: f32 = 13.0;
pub const HEADING: f32 = 18.0;

// Spacing scale (px): 4 / 8 / 12 / 16 / 24 / 32 only.
pub const SP_4: Pixels = px(4.0);
pub const SP_8: Pixels = px(8.0);
pub const SP_12: Pixels = px(12.0);
pub const SP_16: Pixels = px(16.0);
pub const SP_24: Pixels = px(24.0);

// Radius.
pub const RADIUS_CONTROL: Pixels = px(8.0);
pub const RADIUS_PANEL: Pixels = px(12.0);

/// Font: Inter, embedded in the exe (`assets.rs`), registered at start by `load_fonts`.
pub const FONT: &str = "Inter";

/// Make Inter available to the window. If Windows can't load it, text falls back to the
/// system font; nothing else changes.
pub fn load_fonts(cx: &mut App) {
    let fonts = super::assets::FONTS
        .iter()
        .map(|b| std::borrow::Cow::Borrowed(*b))
        .collect();
    let _ = cx.text_system().add_fonts(fonts);
}

/// Tabular numerals ("tnum"): every digit is equally wide, so CPU %, dB and ms
/// readouts don't jitter as they change. Set once on the window's root; it cascades.
pub fn tabular_numbers() -> gpui::FontFeatures {
    gpui::FontFeatures(std::sync::Arc::new(vec![("tnum".into(), 1)]))
}

/// Point gpui-component's theme at the active theme's tokens.
pub fn apply(cx: &mut App) {
    let mode = if current().palette().dark {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    UiTheme::change(mode, None, cx);
    let t = UiTheme::global_mut(cx);
    t.font_family = FONT.into();
    t.font_size = px(BODY);
    t.radius = RADIUS_CONTROL;
    t.radius_lg = RADIUS_PANEL;
    t.shadow = false;

    let k = &mut t.colors;
    k.background = c(BG);
    k.foreground = c(TEXT_PRIMARY);
    k.border = c(BORDER);
    k.input = c(BORDER);
    k.ring = c(ACCENT);
    k.primary = c(ACCENT);
    k.primary_hover = c(ACCENT_HOVER);
    k.primary_active = c(ACCENT);
    k.primary_foreground = c(ON_ACCENT);
    k.secondary = c(SURFACE);
    k.secondary_hover = c(ELEVATED);
    k.secondary_active = c(ELEVATED);
    k.secondary_foreground = c(TEXT_PRIMARY);
    k.muted = c(SURFACE);
    k.muted_foreground = c(TEXT_SECONDARY);
    // gpui-component's "accent" is its hover fill, i.e. our `elevated`.
    k.accent = c(ELEVATED);
    k.accent_foreground = c(TEXT_PRIMARY);
    k.popover = c(ELEVATED);
    k.popover_foreground = c(TEXT_PRIMARY);
    k.slider_bar = c(ACCENT);
    k.slider_thumb = c(TEXT_PRIMARY);
    k.switch = c(BORDER);
    k.warning = c(WARNING);
    k.window_border = c(BORDER);
    k.title_bar = c(BG);
    k.title_bar_border = c(BORDER);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lum(hex: u32) -> f64 {
        let ch = |v: u32| {
            let v = v as f64 / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * ch(hex >> 16 & 0xFF) + 0.7152 * ch(hex >> 8 & 0xFF) + 0.0722 * ch(hex & 0xFF)
    }

    fn contrast(a: u32, b: u32) -> f64 {
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn every_theme_keeps_text_readable() {
        for t in ThemeName::ALL {
            let p = t.palette();
            for bg in [p.bg, p.surface, p.elevated] {
                assert!(contrast(p.text_primary, bg) >= 7.0, "{t:?} primary");
                assert!(contrast(p.text_secondary, bg) >= 4.5, "{t:?} secondary");
            }
            // Accent and meter marks are UI graphics: 3:1.
            for bg in [p.bg, p.surface] {
                assert!(contrast(p.accent, bg) >= 3.0, "{t:?} accent");
                assert!(contrast(p.meter_peak, bg) >= 3.0, "{t:?} peak");
                assert!(contrast(p.warning, bg) >= 3.0, "{t:?} warning");
            }
            // Selected segment labels and primary buttons: text on accent.
            assert!(contrast(p.on_accent, p.accent) >= 4.5, "{t:?} on accent");
        }
    }

    #[test]
    fn theme_names_round_trip_and_unknown_is_default() {
        for t in ThemeName::ALL {
            assert_eq!(ThemeName::from_id(t.id()), t);
        }
        assert_eq!(ThemeName::from_id("neon"), ThemeName::WarmStone);
    }
}
