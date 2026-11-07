//! The Kurrency themes (docs/DESIGN.md §5): Paper and
//! Slate for light mode, Graphite and Ink for dark, with Settings → Theme
//! choosing the mode (System / Light / Dark) and the theme for each mode.

use std::borrow::Cow;
use std::rc::Rc;

use gpui_kit::component::{ActiveTheme as _, Theme, ThemeMode, ThemeRegistry};
use gpui_kit::{Anchor, App, Hsla, Window, px, rgb};
use kurrency_core::settings::{self, Settings, ThemeId};

use crate::layout::metrics::{PAD_X, WIDTH};

const THEMES: &str = crate::assets::embed!(str: "themes/kurrency.json");

/// Registers the bundled Inter and the Kurrency themes, and applies the
/// default pair (Paper and Graphite) following the system appearance until
/// the settings are read.
pub(crate) fn init(cx: &mut App) {
    // The bundled Inter and its CJK fallback, never whatever the system has
    // installed. (Inflated here, on first use.)
    if let Err(err) = cx.text_system().add_fonts(
        crate::assets::FONTS
            .iter()
            .chain(crate::assets::CJK_FONTS.iter())
            .map(|data| Cow::Borrowed(data.as_slice()))
            .collect(),
    ) {
        log::error!("could not load the bundled Inter: {err:#}");
    }
    if let Err(err) = ThemeRegistry::global_mut(cx).load_themes_from_str(THEMES) {
        log::error!("could not load the Kurrency themes: {err:#}");
        return;
    }
    Theme::update(cx, |theme| {
        // Error toasts fit the narrow window. (The notification layer has no
        // height, so bottom placements are off-screen; the undo toast is the
        // app's own.)
        theme.notification.width = px(WIDTH - 2. * PAD_X);
        theme.notification.placement = Anchor::TopCenter;
    });
    apply(&Settings::default(), None, cx);
}

/// Applies Settings → Theme: the light and dark themes, then the mode.
/// `System` follows the window's appearance.
pub(crate) fn apply(settings: &Settings, window: Option<&mut Window>, cx: &mut App) {
    let registry = ThemeRegistry::global(cx);
    let config = |id: ThemeId| {
        registry
            .themes()
            .get(id.theme_name())
            .cloned()
            .map(|config| {
                let mut config = (*config).clone();
                config.font_family = Some(crate::assets::FONT_FAMILY.into());
                Rc::new(config)
            })
    };
    let light = config(settings.theme_light);
    let dark = config(settings.theme_dark);
    Theme::update(cx, |theme| {
        if let Some(light) = light {
            theme.light_theme = light;
        }
        if let Some(dark) = dark {
            theme.dark_theme = dark;
        }
    });
    match settings.theme_mode {
        settings::ThemeMode::System => Theme::sync_system_appearance(window, cx),
        settings::ThemeMode::Light => Theme::change(ThemeMode::Light, window, cx),
        settings::ThemeMode::Dark => Theme::change(ThemeMode::Dark, window, cx),
    }
}

/// Re-applies light or dark after the system appearance changes.
pub(crate) fn sync_with_system(window: &mut Window, cx: &mut App) {
    Theme::sync_system_appearance(Some(window), cx);
}

/// The core colours of a theme as `0xRRGGBB`, which the gallery previews
/// draw with; `assets/themes/kurrency.json` must agree (tested).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Palette {
    pub bg: u32,
    pub card: u32,
    pub hairline: u32,
    pub text: u32,
    pub muted: u32,
    pub accent: u32,
    pub hover: u32,
    pub green: u32,
    pub red: u32,
    pub amber: u32,
    /// The opacity of an amount's decimals.
    pub decimals: f32,
    /// The accent tint behind a selected quick-amount chip (0 = none).
    pub accent_fill: f32,
}

impl Palette {
    pub(crate) fn of(id: ThemeId) -> Self {
        match id {
            ThemeId::Paper => Self {
                bg: 0xFAFAF8,
                card: 0xFFFFFF,
                hairline: 0xECECE8,
                text: 0x18181B,
                muted: 0x8A8A90,
                accent: 0x3A5A57,
                hover: 0xF3F3F0,
                green: 0x4E8F67,
                red: 0xC0574F,
                amber: 0xB8893B,
                decimals: DECIMALS_OPACITY,
                accent_fill: 0.,
            },
            ThemeId::Graphite => Self {
                bg: 0x0F0F11,
                card: 0x17171A,
                hairline: 0x232327,
                text: 0xEDEDEF,
                muted: 0x7C7C84,
                accent: 0x9DB5B2,
                hover: 0x18181B,
                green: 0x4E8F67,
                red: 0xC0574F,
                amber: 0xB8893B,
                decimals: DECIMALS_OPACITY,
                accent_fill: 0.,
            },
            // High-contrast dark: pure black, pure white text with brighter
            // decimals, muted text and hairlines, a brighter accent, and
            // saturated up/down colours.
            ThemeId::Ink => Self {
                bg: 0x000000,
                card: 0x0A0A0A,
                hairline: 0x2E2E33,
                text: 0xFFFFFF,
                muted: 0xA1A1AA,
                accent: 0xB8D4D0,
                hover: 0x141417,
                green: 0x2FD07E,
                red: 0xFF5C52,
                amber: 0xF2B544,
                decimals: 0.75,
                accent_fill: 0.,
            },
            // Tinted light: a cool blue-grey wash through the background,
            // hairlines and muted text, and the Kurrency Slate accent used
            // more generously (tinted selected chip, focus halo).
            ThemeId::Slate => Self {
                bg: 0xEEF2F3,
                card: 0xF8FAFA,
                hairline: 0xD8E0E2,
                text: 0x14201F,
                muted: 0x5F6F6E,
                accent: 0x3A5A57,
                hover: 0xE3EAEC,
                green: 0x3D8159,
                red: 0xB9483F,
                amber: 0xA97A2C,
                decimals: 0.55,
                accent_fill: 0.12,
            },
        }
    }
}

/// The Kurrency design tokens (DESIGN §5), read from the active theme so
/// every view (list, card, detail page) agrees.
#[derive(Clone, Copy)]
pub(crate) struct Tokens {
    pub bg: Hsla,
    /// The anchor card's surface.
    pub card: Hsla,
    pub hairline: Hsla,
    pub text: Hsla,
    pub muted: Hsla,
    /// Kurrency Slate: focus, the selected chip, the freshness dot.
    pub accent: Hsla,
    pub hover: Hsla,
    pub green: Hsla,
    pub red: Hsla,
    /// Stale rates.
    pub amber: Hsla,
    /// The opacity of an amount's decimals.
    pub decimals: f32,
    /// The accent tint behind a selected quick-amount chip (0 = none).
    pub accent_fill: f32,
    pub dark: bool,
}

impl Tokens {
    pub(crate) fn get(cx: &App) -> Self {
        let theme = cx.theme();
        let name = theme.theme_name();
        let palette = ThemeId::ALL
            .into_iter()
            .find(|id| id.theme_name() == name.as_ref())
            .map(Palette::of);
        Self {
            bg: theme.background,
            card: theme.group_box,
            hairline: theme.border,
            text: theme.foreground,
            muted: theme.muted_foreground,
            accent: theme.ring,
            hover: theme.list_hover,
            green: theme.success,
            red: theme.danger,
            amber: theme.warning,
            decimals: palette.map_or(DECIMALS_OPACITY, |p| p.decimals),
            accent_fill: palette.map_or(0., |p| p.accent_fill),
            dark: theme.is_dark(),
        }
    }

    /// A theme's tokens without applying it (the gallery previews).
    pub(crate) fn of(id: ThemeId) -> Self {
        let p = Palette::of(id);
        let c = |hex: u32| -> Hsla { rgb(hex).into() };
        Self {
            bg: c(p.bg),
            card: c(p.card),
            hairline: c(p.hairline),
            text: c(p.text),
            muted: c(p.muted),
            accent: c(p.accent),
            hover: c(p.hover),
            green: c(p.green),
            red: c(p.red),
            amber: c(p.amber),
            decimals: p.decimals,
            accent_fill: p.accent_fill,
            dark: id.is_dark(),
        }
    }

    /// The up or down colour, per the up/down colours setting.
    pub(crate) fn change(&self, up: bool, green_up: bool) -> Hsla {
        if up == green_up { self.green } else { self.red }
    }
}

/// Opacities from the typography table (Ink draws its decimals brighter:
/// [`Palette::decimals`]).
pub(crate) const SYMBOL_OPACITY: f32 = 0.45;
pub(crate) const DECIMALS_OPACITY: f32 = 0.55;
/// The tinted background of a change chip.
pub(crate) const CHIP_TINT: f32 = 0.10;

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    fn themes() -> Vec<Value> {
        let file: Value = serde_json::from_str(THEMES).unwrap();
        file["themes"].as_array().unwrap().clone()
    }

    #[test]
    fn every_theme_has_every_token() {
        let themes = themes();
        assert_eq!(themes.len(), ThemeId::ALL.len());
        let keys = |theme: &Value| {
            let mut keys: Vec<String> = theme["colors"]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect();
            keys.sort();
            keys
        };
        let paper = keys(&themes[0]);
        assert!(paper.len() > 40);
        for theme in &themes {
            assert_eq!(keys(theme), paper, "{}", theme["name"]);
            for (key, value) in theme["colors"].as_object().unwrap() {
                let hex = value.as_str().unwrap();
                assert!(
                    hex.starts_with('#') && matches!(hex.len(), 7 | 9),
                    "{} {key}: {hex}",
                    theme["name"]
                );
                assert!(u32::from_str_radix(&hex[1..], 16).is_ok());
            }
        }
    }

    #[test]
    fn the_palettes_match_the_theme_file() {
        let themes = themes();
        for id in ThemeId::ALL {
            let theme = themes
                .iter()
                .find(|t| t["name"] == id.theme_name())
                .unwrap_or_else(|| panic!("{} is not in the theme file", id.theme_name()));
            assert_eq!(theme["mode"], if id.is_dark() { "dark" } else { "light" });
            let p = Palette::of(id);
            let color = |key: &str| theme["colors"][key].as_str().unwrap().to_string();
            let hex = |v: u32| format!("#{v:06X}");
            for (key, value) in [
                ("background", p.bg),
                ("group_box.background", p.card),
                ("border", p.hairline),
                ("foreground", p.text),
                ("muted.foreground", p.muted),
                ("ring", p.accent),
                ("list.hover.background", p.hover),
                ("success.background", p.green),
                ("danger.background", p.red),
                ("warning.background", p.amber),
            ] {
                assert_eq!(color(key), hex(value), "{} {key}", id.label());
            }
        }
    }

    /// WCAG relative luminance of `0xRRGGBB`.
    fn luminance(hex: u32) -> f64 {
        let channel = |shift: u32| {
            let c = f64::from((hex >> shift) & 0xFF) / 255.;
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
    }

    fn contrast(a: u32, b: u32) -> f64 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    /// WCAG contrast on each theme's background and card: text ≥ 4.5 (AA;
    /// ≥ 7, AAA, in practice) and muted text ≥ 3, and ≥ 4.5 for Ink, the
    /// high-contrast one.
    #[test]
    fn text_contrast() {
        for id in ThemeId::ALL {
            let p = Palette::of(id);
            let (text, muted) = (contrast(p.text, p.bg), contrast(p.muted, p.bg));
            let (text_card, muted_card) = (contrast(p.text, p.card), contrast(p.muted, p.card));
            assert!(text >= 4.5 && text_card >= 4.5, "{} text", id.label());
            assert!(text >= 7., "{} text {text:.2}", id.label());
            assert!(muted >= 3. && muted_card >= 3., "{} muted", id.label());
        }
        // Ink is the high-contrast one.
        let (ink, graphite) = (Palette::of(ThemeId::Ink), Palette::of(ThemeId::Graphite));
        assert!(contrast(ink.text, ink.bg) > contrast(graphite.text, graphite.bg));
        assert!(contrast(ink.muted, ink.bg) >= 4.5);
        assert!(contrast(ink.hairline, ink.bg) > contrast(graphite.hairline, graphite.bg));
    }
}
