//! Persisted user settings: `{config_dir}/kurrency/settings.json`.
//!
//! Plain Rust without GPUI.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::money;

/// Every field has a default, and a value this version does not know (a
/// newer or hand-edited file) falls back to it, so any older file loads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The favourite currencies, in display order.
    pub currencies: Vec<String>,
    /// The currency the amount was last entered in.
    pub source: Option<String>,
    /// The last committed amount, as a plain decimal string.
    pub amount: Option<String>,
    /// Which colour means "up" for rate changes.
    #[serde(deserialize_with = "lenient")]
    pub up_down_colors: UpDownColors,
    /// Draw every animation in its end state (DESIGN §4). GPUI does not
    /// report the system setting, so this is the switch; the
    /// `KURRENCY_REDUCE_MOTION=1` environment variable sets it too.
    #[serde(deserialize_with = "lenient")]
    pub reduce_motion: bool,
    /// System / Light / Dark.
    #[serde(deserialize_with = "lenient")]
    pub theme_mode: ThemeMode,
    /// The theme used in light mode (Paper or Slate).
    #[serde(deserialize_with = "lenient_light")]
    pub theme_light: ThemeId,
    /// The theme used in dark mode (Graphite or Ink).
    #[serde(deserialize_with = "lenient_dark")]
    pub theme_dark: ThemeId,
    #[serde(deserialize_with = "lenient")]
    pub language: Language,
    #[serde(deserialize_with = "lenient")]
    pub number_format: NumberFormat,
    #[serde(deserialize_with = "lenient")]
    pub decimal_places: DecimalPlaces,
    #[serde(deserialize_with = "lenient")]
    pub rate_source: RateSource,
    #[serde(deserialize_with = "lenient")]
    pub refresh_interval: RefreshInterval,
    /// The window's last size and position (logical px).
    #[serde(deserialize_with = "lenient")]
    pub window: Option<SavedWindow>,
}

/// Reads a field, or its default when the value is malformed or unknown.
fn lenient<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned + Default,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

/// A light theme, or Paper.
fn lenient_light<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<ThemeId, D::Error> {
    let theme: Option<ThemeId> = lenient(deserializer)?;
    Ok(theme
        .filter(|t| !t.is_dark())
        .unwrap_or_else(ThemeId::default_light))
}

/// A dark theme, or Graphite.
fn lenient_dark<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<ThemeId, D::Error> {
    let theme: Option<ThemeId> = lenient(deserializer)?;
    Ok(theme
        .filter(|t| t.is_dark())
        .unwrap_or_else(ThemeId::default_dark))
}

/// Settings → Theme mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    /// Follow the system appearance, with the chosen light and dark themes.
    #[default]
    System,
    Light,
    Dark,
}

/// The theme gallery (docs/DESIGN.md §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeId {
    /// Light, the default.
    Paper,
    /// Dark, the default.
    Graphite,
    /// High-contrast dark.
    Ink,
    /// Tinted light.
    Slate,
}

impl ThemeId {
    pub const ALL: [Self; 4] = [Self::Paper, Self::Graphite, Self::Ink, Self::Slate];

    pub(crate) fn default_light() -> Self {
        Self::Paper
    }

    pub(crate) fn default_dark() -> Self {
        Self::Graphite
    }

    pub fn is_dark(self) -> bool {
        matches!(self, Self::Graphite | Self::Ink)
    }

    /// "Paper"
    pub fn label(self) -> &'static str {
        match self {
            Self::Paper => "Paper",
            Self::Graphite => "Graphite",
            Self::Ink => "Ink",
            Self::Slate => "Slate",
        }
    }

    /// The name in `assets/themes/kurrency.json`.
    pub fn theme_name(self) -> &'static str {
        match self {
            Self::Paper => "Kurrency Paper",
            Self::Graphite => "Kurrency Graphite",
            Self::Ink => "Kurrency Ink",
            Self::Slate => "Kurrency Slate",
        }
    }
}

/// Settings → Language: the UI language (see `i18n::Lang::resolve`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    System,
    English,
    #[serde(rename = "zh_hans")]
    ChineseSimplified,
}

/// Settings → Number format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NumberFormat {
    /// From the system locale.
    #[default]
    System,
    /// `1,234.56`
    CommaDot,
    /// `1.234,56`
    DotComma,
    /// `1 234,56` (narrow no-break space)
    SpaceComma,
}

impl NumberFormat {
    pub fn style(self, locale: Option<&str>) -> money::NumberStyle {
        match self {
            Self::System => money::NumberStyle::for_locale(locale),
            Self::CommaDot => money::NumberStyle::COMMA_DOT,
            Self::DotComma => money::NumberStyle::DOT_COMMA,
            Self::SpaceComma => money::NumberStyle::SPACE_COMMA,
        }
    }
}

/// Settings → Decimal places.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecimalPlaces {
    /// ISO 4217 minor units.
    #[default]
    CurrencyDefault,
    /// Two more than the minor units.
    ExtraTwo,
}

impl DecimalPlaces {
    pub fn extra(self) -> u32 {
        match self {
            Self::CurrencyDefault => 0,
            Self::ExtraTwo => 2,
        }
    }
}

/// Settings → Source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RateSource {
    /// Frankfurter, then fawazahmed0, then the ECB's XML feed.
    #[default]
    Auto,
    /// The ECB's reference rates only: Frankfurter's ECB feed, then the
    /// ECB's own XML.
    Ecb,
    /// Frankfurter's blend of central banks.
    Frankfurter,
}

/// Settings → Refresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshInterval {
    #[serde(rename = "1h")]
    Hour,
    #[default]
    #[serde(rename = "6h")]
    SixHours,
    #[serde(rename = "12h")]
    TwelveHours,
    Manual,
}

impl RefreshInterval {
    /// Seconds between automatic refreshes; `None` for manual.
    pub fn secs(self) -> Option<u64> {
        match self {
            Self::Hour => Some(3600),
            Self::SixHours => Some(6 * 3600),
            Self::TwelveHours => Some(12 * 3600),
            Self::Manual => None,
        }
    }
}

/// A window's frame in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowGeometry {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// The remembered window: its size, and its position where it is known.
///
/// On Wayland a client cannot read (or choose) its window's position: the
/// compositor places it, and GPUI just reports back the origin it was opened
/// with. So there only the size is saved, and `x`/`y` are left out.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SavedWindow {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f32>,
    pub width: f32,
    pub height: f32,
}

impl SavedWindow {
    /// The whole frame, when the position is known.
    pub fn frame(self) -> Option<WindowGeometry> {
        Some(WindowGeometry {
            x: self.x?,
            y: self.y?,
            width: self.width,
            height: self.height,
        })
    }
}

/// Settings → Up/down colours: By language / Green up / Red up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpDownColors {
    /// Red up in Chinese, Japanese and Korean locales; green up elsewhere.
    #[default]
    ByLanguage,
    GreenUp,
    RedUp,
}

impl UpDownColors {
    /// Whether a rise is shown in green for the UI `locale` (`zh-CN`, `en-US`).
    pub fn green_up(self, locale: Option<&str>) -> bool {
        match self {
            Self::GreenUp => true,
            Self::RedUp => false,
            Self::ByLanguage => {
                let lang = locale
                    .and_then(|l| l.split(['-', '_']).next())
                    .unwrap_or("en")
                    .to_ascii_lowercase();
                !matches!(lang.as_str(), "zh" | "ja" | "ko")
            }
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self::first_launch(None)
    }
}

impl Settings {
    /// First-launch defaults for a system locale such as `en-US`.
    pub(crate) fn first_launch(locale: Option<&str>) -> Self {
        let home = locale.and_then(money::currency_for_locale);
        let currencies = money::default_currency_list(home.as_deref());
        Self {
            source: currencies.first().cloned(),
            amount: Some("100".to_string()),
            currencies,
            up_down_colors: UpDownColors::default(),
            reduce_motion: false,
            theme_mode: ThemeMode::default(),
            theme_light: ThemeId::default_light(),
            theme_dark: ThemeId::default_dark(),
            language: Language::default(),
            number_format: NumberFormat::default(),
            decimal_places: DecimalPlaces::default(),
            rate_source: RateSource::default(),
            refresh_interval: RefreshInterval::default(),
            window: None,
        }
    }

    /// Removes duplicate and malformed codes, keeping the first occurrence.
    pub(crate) fn normalize(&mut self) {
        let mut seen = Vec::new();
        self.currencies.retain(|code| {
            let ok = code.len() == 3
                && code.chars().all(|c| c.is_ascii_uppercase())
                && !seen.contains(code);
            if ok {
                seen.push(code.clone());
            }
            ok
        });
    }
}

pub fn default_path() -> Option<PathBuf> {
    crate::paths::config_dir().map(|dir| dir.join("settings.json"))
}

/// Loads settings, or the first-launch defaults when there are none.
pub fn load(path: Option<&Path>, locale: Option<&str>) -> Settings {
    let loaded = path.and_then(|p| std::fs::read(p).ok()).and_then(|bytes| {
        match serde_json::from_slice::<Settings>(&bytes) {
            Ok(settings) => Some(settings),
            Err(err) => {
                log::warn!("ignoring unreadable settings: {err}");
                None
            }
        }
    });
    let mut settings = loaded.unwrap_or_else(|| Settings::first_launch(locale));
    settings.normalize();
    settings
}

pub fn save(path: &Path, settings: &Settings) -> anyhow::Result<()> {
    let json = serde_json::to_vec_pretty(settings)?;
    crate::rates::cache::write_atomic(path, &json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_launch_uses_locale() {
        let s = Settings::first_launch(Some("zh-CN"));
        assert_eq!(s.currencies, ["CNY", "USD", "EUR", "GBP", "JPY"]);
        assert_eq!(s.source.as_deref(), Some("CNY"));
        let s = Settings::first_launch(Some("en-GB"));
        assert_eq!(s.currencies, ["GBP", "USD", "EUR", "JPY", "CNY"]);
        let s = Settings::first_launch(None);
        assert_eq!(s.currencies, ["USD", "EUR", "GBP", "JPY", "CNY"]);
    }

    #[test]
    fn up_down_colors_follow_the_language() {
        let by_lang = UpDownColors::ByLanguage;
        assert!(by_lang.green_up(Some("en-US")));
        assert!(by_lang.green_up(None));
        assert!(!by_lang.green_up(Some("zh-CN")));
        assert!(!by_lang.green_up(Some("zh_TW")));
        assert!(UpDownColors::GreenUp.green_up(Some("zh-CN")));
        assert!(!UpDownColors::RedUp.green_up(Some("en-GB")));
    }

    #[test]
    fn normalize_dedupes_and_drops_junk() {
        let mut s = Settings {
            currencies: vec!["USD".into(), "eur".into(), "USD".into(), "JPY".into()],
            source: None,
            amount: None,
            ..Settings::default()
        };
        s.normalize();
        assert_eq!(s.currencies, ["USD", "JPY"]);
    }

    #[test]
    fn round_trip_and_missing_fields() {
        let dir = std::env::temp_dir().join(format!("kurrency-settings-{}", std::process::id()));
        let path = dir.join("settings.json");
        assert_eq!(load(Some(&path), Some("ja-JP")).currencies[0], "JPY");
        let s = Settings {
            currencies: vec!["HKD".into(), "USD".into()],
            source: Some("USD".into()),
            amount: Some("42.5".into()),
            up_down_colors: UpDownColors::RedUp,
            reduce_motion: true,
            theme_mode: ThemeMode::Dark,
            theme_light: ThemeId::Slate,
            theme_dark: ThemeId::Ink,
            language: Language::ChineseSimplified,
            number_format: NumberFormat::SpaceComma,
            decimal_places: DecimalPlaces::ExtraTwo,
            rate_source: RateSource::Ecb,
            refresh_interval: RefreshInterval::Manual,
            window: Some(SavedWindow {
                x: Some(10.),
                y: Some(20.),
                width: 380.,
                height: 600.,
            }),
        };
        save(&path, &s).unwrap();
        assert_eq!(load(Some(&path), None), s);
        // Wayland: the size only.
        let size_only = Settings {
            window: Some(SavedWindow {
                x: None,
                y: None,
                width: 380.,
                height: 600.,
            }),
            ..s.clone()
        };
        save(&path, &size_only).unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(!saved.contains("\"x\""), "{saved}");
        assert_eq!(load(Some(&path), None), size_only);
        assert_eq!(size_only.window.unwrap().frame(), None);
        std::fs::write(&path, br#"{"currencies":["SGD"]}"#).unwrap();
        let partial = load(Some(&path), None);
        assert_eq!(partial.currencies, ["SGD"]);
        assert_eq!(partial.up_down_colors, UpDownColors::ByLanguage);
        assert!(!partial.reduce_motion);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn old_files_migrate_and_bad_values_fall_back() {
        // A v2.1 file: no new keys at all.
        let v21 = br#"{"currencies":["USD","EUR"],"source":"USD","amount":"100",
            "up_down_colors":"red_up","reduce_motion":true}"#;
        let s: Settings = serde_json::from_slice(v21).unwrap();
        assert_eq!(s.up_down_colors, UpDownColors::RedUp);
        assert!(s.reduce_motion);
        assert_eq!(s.theme_mode, ThemeMode::System);
        assert_eq!(
            (s.theme_light, s.theme_dark),
            (ThemeId::Paper, ThemeId::Graphite)
        );
        assert_eq!(s.number_format, NumberFormat::System);
        assert_eq!(s.decimal_places, DecimalPlaces::CurrencyDefault);
        assert_eq!(s.rate_source, RateSource::Auto);
        assert_eq!(s.refresh_interval, RefreshInterval::SixHours);
        assert_eq!(s.refresh_interval.secs(), Some(6 * 3600));
        assert_eq!(s.window, None);

        // Unknown values, wrong types and a theme in the wrong slot keep
        // their defaults without losing the rest of the file.
        let odd = br#"{"currencies":["JPY"],"theme_mode":"sepia","theme_light":"ink",
            "theme_dark":"slate","number_format":42,"refresh_interval":"1h",
            "rate_source":"ecb","language":"zh_hans","window":{"x":"left"},
            "up_down_colors":null}"#;
        let s: Settings = serde_json::from_slice(odd).unwrap();
        assert_eq!(s.currencies, ["JPY"]);
        assert_eq!(s.theme_mode, ThemeMode::System);
        assert_eq!(
            (s.theme_light, s.theme_dark),
            (ThemeId::Paper, ThemeId::Graphite)
        );
        assert_eq!(s.number_format, NumberFormat::System);
        assert_eq!(s.refresh_interval, RefreshInterval::Hour);
        assert_eq!(s.rate_source, RateSource::Ecb);
        assert_eq!(s.language, Language::ChineseSimplified);
        assert_eq!(s.window, None);
        assert_eq!(s.up_down_colors, UpDownColors::ByLanguage);

        // The stored names.
        let json = serde_json::to_value(Settings::default()).unwrap();
        assert_eq!(json["refresh_interval"], "6h");
        assert_eq!(json["theme_light"], "paper");
        assert_eq!(json["number_format"], "system");
        assert_eq!(json["decimal_places"], "currency_default");
    }

    #[test]
    fn format_choices() {
        use crate::money::NumberStyle;
        assert_eq!(
            NumberFormat::DotComma.style(Some("en-US")),
            NumberStyle::DOT_COMMA
        );
        assert_eq!(
            NumberFormat::System.style(Some("de-DE")),
            NumberStyle::DOT_COMMA
        );
        assert_eq!(NumberFormat::System.style(None), NumberStyle::COMMA_DOT);
        assert_eq!(RefreshInterval::Manual.secs(), None);
    }
}
