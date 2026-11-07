//! UI languages: English and Simplified Chinese (`locales/en.yml`,
//! `locales/zh-CN.yml` at the repository root, via rust-i18n).
//!
//! Both crates' strings go through [`tr!`], which reads the language set with
//! [`set`]. gpui-kit's own strings (input menus, the palette's empty state)
//! follow it too: [`set`] also sets rust-i18n's global locale, and the app
//! hands [`backend`] to gpui-kit so that the `gpui_component` keys in our
//! locale files override the kit's.
//!
//! Tests (and the app's tests, through the `test-support` feature) keep the
//! language per thread, so a test that switches to Chinese never changes the
//! strings another test sees.

use std::borrow::Cow;

use crate::settings::Language;

/// A UI language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    #[default]
    En,
    ZhCn,
}

impl Lang {
    /// The rust-i18n locale (`en`, `zh-CN`).
    pub fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::ZhCn => "zh-CN",
        }
    }

    /// The language for Settings → Language, given the system locale.
    pub fn resolve(setting: Language, system: Option<&str>) -> Self {
        match setting {
            Language::System => Self::for_locale(system),
            Language::English => Self::En,
            Language::ChineseSimplified => Self::ZhCn,
        }
    }

    /// The UI language for a BCP 47 / POSIX locale: Simplified Chinese for
    /// `zh`, `zh-CN`, `zh-Hans`, `zh-SG` (`zh_CN.UTF-8`), English otherwise.
    // TODO(i18n): Traditional Chinese (zh-TW, zh-HK, zh-MO, zh-Hant) gets
    // English until there is a zh-Hant locale.
    pub(crate) fn for_locale(locale: Option<&str>) -> Self {
        let Some(locale) = locale.and_then(|l| l.split(['.', '@']).next()) else {
            return Self::En;
        };
        let mut parts = locale.split(['-', '_']).map(str::to_ascii_lowercase);
        if parts.next().as_deref() != Some("zh") {
            return Self::En;
        }
        let rest: Vec<String> = parts.collect();
        let has = |subtag: &str| rest.iter().any(|p| p == subtag);
        if has("hans") {
            Self::ZhCn
        } else if has("hant") || has("tw") || has("hk") || has("mo") {
            Self::En
        } else {
            Self::ZhCn
        }
    }

    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::ZhCn,
            _ => Self::En,
        }
    }
}

/// Looks up a key in the current UI language: `tr!("toast.undo")`,
/// `tr!("toast.removed", code = code)`. Returns a `String`; a missing key
/// returns the key itself, like rust-i18n's `t!`.
///
/// Exported so that the app's strings, which share the locale files, are
/// looked up the same way. (`rust_i18n::t!` itself only reaches the table of
/// the crate it is written in.)
#[macro_export]
macro_rules! tr {
    ($key:expr) => {
        $crate::i18n::translate($key, &[], &[])
    };
    ($key:expr, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::i18n::translate(
            $key,
            &[$(::core::stringify!($name)),+],
            &[$(::std::format!("{}", $value)),+],
        )
    };
}
pub use crate::tr;

/// What [`tr!`] expands to: `key` in the current language, with each
/// `%{name}` replaced by its value.
#[doc(hidden)]
pub fn translate(key: &str, names: &[&str], values: &[String]) -> String {
    let text = crate::_rust_i18n_try_translate(lang().code(), key).unwrap_or(Cow::Borrowed(key));
    if names.is_empty() {
        text.into_owned()
    } else {
        rust_i18n::replace_patterns(&text, names, values)
    }
}

/// The compiled locale files. The app extends gpui-kit's strings with it
/// (what `rust_i18n::extend!` does within one crate), and its tests read it.
pub fn backend() -> &'static dyn rust_i18n::Backend {
    crate::_rust_i18n_backend()
}

#[cfg(not(any(test, feature = "test-support")))]
static CURRENT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    static CURRENT: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

/// The current UI language.
pub fn lang() -> Lang {
    #[cfg(not(any(test, feature = "test-support")))]
    let value = CURRENT.load(std::sync::atomic::Ordering::Relaxed);
    #[cfg(any(test, feature = "test-support"))]
    let value = CURRENT.with(std::cell::Cell::get);
    Lang::from_u8(value)
}

/// Switches the UI language (live: the next frame draws in it).
pub fn set(lang: Lang) {
    let value = lang as u8;
    #[cfg(not(any(test, feature = "test-support")))]
    {
        CURRENT.store(value, std::sync::atomic::Ordering::Relaxed);
        rust_i18n::set_locale(lang.code());
    }
    #[cfg(any(test, feature = "test-support"))]
    CURRENT.with(|current| current.set(value));
}

/// Runs `f` in `lang` (tests).
#[cfg(test)]
pub(crate) fn with_lang<T>(lang: Lang, f: impl FnOnce() -> T) -> T {
    let before = self::lang();
    set(lang);
    let out = f();
    set(before);
    out
}

/// The month's name, `1..=12`: `Nov` / `November`, `11月`.
fn month(m: u32, long: bool) -> String {
    let names = if long {
        tr!("date.months_long")
    } else {
        tr!("date.months")
    };
    let ix = m.clamp(1, 12) as usize - 1;
    names
        .split_whitespace()
        .nth(ix)
        .map(str::to_string)
        .unwrap_or_else(|| m.to_string())
}

/// `7 Nov` / `11月7日`, or `7 Nov 2026` / `2026年11月7日` with the year.
pub(crate) fn short_date(y: i32, m: u32, d: u32, with_year: bool) -> String {
    let month = month(m, false);
    if with_year {
        tr!("date.short_year", day = d, month = month, year = y)
    } else {
        tr!("date.short", day = d, month = month)
    }
}

/// `6 November 2026` / `2026年11月6日`.
pub(crate) fn long_date(y: i32, m: u32, d: u32) -> String {
    tr!("date.long", day = d, month = month(m, true), year = y)
}

/// `7 Nov, 19:59` / `11月7日 19:59`, from a formatted date.
pub(crate) fn date_time(date: &str, hour: i64, minute: i64) -> String {
    tr!(
        "date.date_time",
        date = date,
        time = format!("{hour:02}:{minute:02}")
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn keys(locale: &str) -> BTreeSet<String> {
        crate::_rust_i18n_backend()
            .messages_for_locale(locale)
            .unwrap_or_else(|| panic!("no {locale} locale"))
            .into_iter()
            .map(|(key, _)| key.into_owned())
            .collect()
    }

    #[test]
    fn both_locales_have_every_key() {
        let en = keys("en");
        let zh = keys("zh-CN");
        assert!(en.len() > 100, "{}", en.len());
        let missing_zh: Vec<_> = en.difference(&zh).collect();
        let missing_en: Vec<_> = zh.difference(&en).collect();
        assert!(missing_zh.is_empty(), "missing in zh-CN: {missing_zh:?}");
        assert!(missing_en.is_empty(), "missing in en: {missing_en:?}");
    }

    #[test]
    fn placeholders_match_across_locales() {
        let placeholders = |text: &str| -> BTreeSet<String> {
            text.split("%{")
                .skip(1)
                .filter_map(|rest| rest.split_once('}').map(|(name, _)| name.to_string()))
                .collect()
        };
        let backend = crate::_rust_i18n_backend();
        for key in keys("en") {
            let en = backend.translate("en", &key).unwrap();
            let zh = backend.translate("zh-CN", &key).unwrap();
            assert_eq!(placeholders(&en), placeholders(&zh), "{key}");
            assert!(!zh.trim().is_empty(), "{key} is empty in zh-CN");
        }
    }

    #[test]
    fn lookups_follow_the_language() {
        assert_eq!(lang(), Lang::En);
        assert_eq!(tr!("toast.undo"), "Undo");
        assert_eq!(tr!("toast.removed", code = "EUR"), "EUR removed");
        with_lang(Lang::ZhCn, || {
            assert_eq!(tr!("toast.undo"), "撤销");
            assert_eq!(tr!("toast.removed", code = "EUR"), "已移除 EUR");
        });
        assert_eq!(tr!("toast.undo"), "Undo");
    }

    #[test]
    fn languages_from_settings_and_system_locales() {
        let system = |l: &str| Lang::resolve(Language::System, Some(l));
        for zh in [
            "zh",
            "zh-CN",
            "zh_CN.UTF-8",
            "zh-Hans",
            "zh-Hans-CN",
            "zh-Hans-HK",
            "zh-SG",
            "zh_SG",
        ] {
            assert_eq!(system(zh), Lang::ZhCn, "{zh}");
        }
        for en in [
            "en-US", "en", "de-DE", "ja-JP", "zh-TW", "zh_HK", "zh-Hant", "zh-MO", "C", "",
        ] {
            assert_eq!(system(en), Lang::En, "{en}");
        }
        assert_eq!(Lang::resolve(Language::System, None), Lang::En);
        assert_eq!(
            Lang::resolve(Language::ChineseSimplified, Some("en-US")),
            Lang::ZhCn
        );
        assert_eq!(Lang::resolve(Language::English, Some("zh-CN")), Lang::En);
    }

    #[test]
    fn dates_per_language() {
        assert_eq!(short_date(2026, 11, 7, false), "7 Nov");
        assert_eq!(short_date(2019, 10, 14, true), "14 Oct 2019");
        assert_eq!(long_date(2026, 11, 6), "6 November 2026");
        assert_eq!(date_time("7 Nov", 19, 59), "7 Nov, 19:59");
        assert_eq!(date_time("7 Nov", 9, 5), "7 Nov, 09:05");
        with_lang(Lang::ZhCn, || {
            assert_eq!(short_date(2026, 11, 7, false), "11月7日");
            assert_eq!(short_date(2019, 10, 14, true), "2019年10月14日");
            assert_eq!(short_date(2026, 1, 31, false), "1月31日");
            assert_eq!(long_date(2026, 11, 6), "2026年11月6日");
            assert_eq!(date_time("11月7日", 19, 59), "11月7日 19:59");
        });
    }
}
