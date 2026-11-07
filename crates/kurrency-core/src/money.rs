//! Currency metadata, conversion and formatting.
//!
//! Everything here is exact [`Decimal`] arithmetic (never `f64`) and has no
//! GPUI dependency.

use std::str::FromStr;

use iso_currency::{Country, Currency};
use rust_decimal::{Decimal, RoundingStrategy};

/// The currencies added on first launch after the home currency.
pub(crate) const DEFAULT_CURRENCIES: [&str; 5] = ["USD", "EUR", "GBP", "JPY", "CNY"];

/// Decimal places shown for amounts when ISO 4217 has no minor unit.
const FALLBACK_MINOR_UNITS: u32 = 2;

/// The ISO 4217 minor units (JPY 0, USD 2, KWD 3), or 2 for codes that have
/// none (non-ISO codes and special currencies such as XAU).
pub fn minor_units(code: &str) -> u32 {
    Currency::from_code(code)
        .and_then(Currency::exponent)
        .map(u32::from)
        .unwrap_or(FALLBACK_MINOR_UNITS)
}

/// Whether `code` is a current ISO 4217 currency that people hold, as opposed
/// to a fund, a precious metal, a testing code or a crypto ticker.
pub fn is_regular_currency(code: &str) -> bool {
    Currency::from_code(code).is_some_and(|c| c.flags().is_empty())
}

/// The name in the UI language: Chinese from CLDR in zh-CN, else
/// [`currency_name`] (and the code when neither knows it).
pub fn display_name(code: &str) -> String {
    match crate::i18n::lang() {
        crate::i18n::Lang::ZhCn => match cldr_zh_name(code) {
            Some(name) => name.to_string(),
            None => currency_name(code),
        },
        crate::i18n::Lang::En => currency_name(code),
    }
}

/// The English name, falling back to the code itself.
pub(crate) fn currency_name(code: &str) -> String {
    let short = match code {
        "USD" => Some("US Dollar"),
        "GBP" => Some("British Pound"),
        "CNY" => Some("Chinese Yuan"),
        "KRW" => Some("South Korean Won"),
        "TWD" => Some("New Taiwan Dollar"),
        "CHF" => Some("Swiss Franc"),
        "RUB" => Some("Russian Ruble"),
        _ => None,
    };
    if let Some(name) = short {
        return name.to_string();
    }
    match Currency::from_code(code) {
        Some(c) => title_case(c.name()),
        None => code.to_string(),
    }
}

/// The Simplified Chinese name from Unicode CLDR (`HKD` → `港元`).
pub(crate) fn cldr_zh_name(code: &str) -> Option<&'static str> {
    let names = crate::generated::cldr_zh::NAMES;
    names
        .binary_search_by(|(c, _)| (*c).cmp(code))
        .ok()
        .map(|ix| names[ix].1)
}

/// Everyday Chinese (simplified) names besides CLDR's, for the palette
/// search and smart paste (`港币` → HKD). Alternative names follow the first.
const ZH_NAMES: &[(&str, &[&str])] = &[
    ("USD", &["美元", "美金"]),
    ("EUR", &["欧元"]),
    ("JPY", &["日元", "日币"]),
    ("CNY", &["人民币"]),
    ("GBP", &["英镑"]),
    ("HKD", &["港币", "港元"]),
    ("TWD", &["新台币", "台币"]),
    ("MOP", &["澳门元", "澳门币"]),
    ("KRW", &["韩元"]),
    ("AUD", &["澳元", "澳币"]),
    ("CAD", &["加元", "加币"]),
    ("NZD", &["新西兰元", "纽币"]),
    ("SGD", &["新加坡元", "新币"]),
    ("CHF", &["瑞士法郎", "瑞郎"]),
    ("SEK", &["瑞典克朗"]),
    ("NOK", &["挪威克朗"]),
    ("DKK", &["丹麦克朗"]),
    ("RUB", &["卢布", "俄罗斯卢布"]),
    ("INR", &["印度卢比"]),
    ("THB", &["泰铢"]),
    ("MYR", &["林吉特", "马来西亚林吉特"]),
    ("IDR", &["印尼盾"]),
    ("PHP", &["菲律宾比索"]),
    ("VND", &["越南盾"]),
    ("MXN", &["墨西哥比索"]),
    ("BRL", &["巴西雷亚尔"]),
    ("ZAR", &["南非兰特", "兰特"]),
    ("TRY", &["土耳其里拉"]),
    ("AED", &["迪拉姆", "阿联酋迪拉姆"]),
    ("SAR", &["沙特里亚尔"]),
    ("PLN", &["兹罗提", "波兰兹罗提"]),
    ("CZK", &["捷克克朗"]),
    ("HUF", &["福林", "匈牙利福林"]),
    ("ILS", &["新谢克尔", "以色列新谢克尔"]),
];

/// Every Chinese name of a currency, in any UI language: CLDR's, then the
/// everyday ones (`HKD` → `港元`, `港币`).
pub(crate) fn zh_names(code: &str) -> impl Iterator<Item = &'static str> {
    let everyday = ZH_NAMES
        .iter()
        .find(|(c, _)| *c == code)
        .map_or(&[][..], |(_, names)| names);
    cldr_zh_name(code).into_iter().chain(
        everyday
            .iter()
            .copied()
            .filter(move |n| Some(*n) != cldr_zh_name(code)),
    )
}

/// Every CLDR Simplified Chinese currency name (the app checks that its
/// bundled CJK font covers them).
#[cfg(feature = "test-support")]
pub fn all_zh_names() -> impl Iterator<Item = &'static str> {
    crate::generated::cldr_zh::NAMES
        .iter()
        .map(|(_, name)| *name)
}

/// The currency with this exact Chinese name (`港元`, `港币` → HKD): the
/// everyday names first, then CLDR's current currencies.
pub(crate) fn code_for_zh_name(name: &str) -> Option<&'static str> {
    ZH_NAMES
        .iter()
        .find(|(_, names)| names.contains(&name))
        .map(|(code, _)| *code)
        .or_else(|| {
            crate::generated::cldr_zh::NAMES
                .iter()
                .find(|(code, n)| *n == name && is_regular_currency(code))
                .map(|(code, _)| *code)
        })
}

/// ISO names are sentence case ("Japanese yen"); list rows use title case.
fn title_case(name: &str) -> String {
    name.split(' ')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<String>>()
        .join(" ")
}

/// The region whose flag represents the currency, as a lowercase ISO 3166
/// alpha-2 code (or `eu`): the issuer's region, which is the first two letters
/// of every ISO 4217 code outside the `X..` block.
///
/// `X..` codes are supranational. EUR shows the EU flag, and the guilder of
/// Curaçao and Sint Maarten (XCG, formerly ANG) shows Curaçao, the larger
/// user. The CFA francs (XAF, XOF), the East Caribbean dollar (XCD) and the
/// CFP franc (XPF) span several countries with no shared flag, so they get
/// `None` and the neutral badge, as do SDR, metals and codes outside ISO 4217
/// (crypto tickers).
pub fn flag_region(code: &str) -> Option<String> {
    let region = match code {
        "EUR" => "eu",
        "ANG" | "XCG" => "cw",
        _ if code.starts_with('X') || Currency::from_code(code).is_none() => return None,
        _ => &code[..2],
    };
    Some(region.to_ascii_lowercase())
}

/// The text of the neutral badge for codes without a flag: `¤` for
/// supranational, special and malformed codes, otherwise the code's first two
/// letters (BTC → "BT").
pub fn flag_badge(code: &str) -> String {
    match code.get(..2) {
        Some(prefix)
            if !code.starts_with('X') && prefix.chars().all(|c| c.is_ascii_alphabetic()) =>
        {
            prefix.to_string()
        }
        _ => "¤".to_string(),
    }
}

/// Converts `amount` between two currencies, given each currency's rate
/// against a shared base (units of that currency per one base unit).
///
/// Returns `None` for a zero source rate or on overflow.
pub(crate) fn convert(amount: Decimal, from_rate: Decimal, to_rate: Decimal) -> Option<Decimal> {
    if from_rate.is_zero() {
        return None;
    }
    amount.checked_mul(to_rate)?.checked_div(from_rate)
}

/// Rounds to `dp` decimal places, half away from zero (the usual money rule).
pub fn round_to(value: Decimal, dp: u32) -> Decimal {
    value.round_dp_with_strategy(dp, RoundingStrategy::MidpointAwayFromZero)
}

/// How numbers are written: the grouping and decimal separators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NumberStyle {
    pub(crate) group: char,
    pub decimal: char,
}

impl NumberStyle {
    /// `1,234.56`
    pub(crate) const COMMA_DOT: Self = Self {
        group: ',',
        decimal: '.',
    };
    /// `1.234,56`
    pub const DOT_COMMA: Self = Self {
        group: '.',
        decimal: ',',
    };
    /// `1 234,56`, grouped with a narrow no-break space (U+202F).
    pub const SPACE_COMMA: Self = Self {
        group: '\u{202f}',
        decimal: ',',
    };

    /// The usual style for a BCP 47 / POSIX locale (`de-DE`, `fr_FR.UTF-8`),
    /// from a small table; `1,234.56` when unknown.
    pub(crate) fn for_locale(locale: Option<&str>) -> Self {
        let Some(locale) = locale.and_then(|l| l.split(['.', '@']).next()) else {
            return Self::COMMA_DOT;
        };
        let mut parts = locale.split(['-', '_']);
        let lang = parts.next().unwrap_or_default().to_ascii_lowercase();
        let region = parts
            .find(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_alphabetic()))
            .map(str::to_ascii_uppercase)
            .unwrap_or_default();
        match (lang.as_str(), region.as_str()) {
            // Regional exceptions first.
            ("pt", "PT") => Self::SPACE_COMMA,
            ("pt", _) => Self::DOT_COMMA,
            ("de", "CH" | "LI") | ("it" | "fr", "CH") => Self::COMMA_DOT,
            ("es", "MX" | "US" | "PR" | "GT" | "HN" | "NI" | "PA" | "SV" | "DO" | "PE") => {
                Self::COMMA_DOT
            }
            ("en", "ZA") => Self::SPACE_COMMA,
            (
                "de" | "es" | "it" | "nl" | "id" | "tr" | "da" | "el" | "ro" | "hr" | "sl" | "sr"
                | "vi" | "is" | "ca" | "gl" | "eu" | "az" | "mk" | "bs",
                _,
            ) => Self::DOT_COMMA,
            (
                "fr" | "ru" | "pl" | "cs" | "sk" | "sv" | "nb" | "nn" | "no" | "fi" | "uk" | "hu"
                | "bg" | "lt" | "lv" | "et" | "be" | "kk" | "ka" | "hy" | "uz" | "ky",
                _,
            ) => Self::SPACE_COMMA,
            _ => Self::COMMA_DOT,
        }
    }
}

impl Default for NumberStyle {
    fn default() -> Self {
        Self::COMMA_DOT
    }
}

/// How amounts are shown: the number style and the decimal places
/// (Settings → Number format, Decimal places).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Format {
    pub style: NumberStyle,
    /// Places shown beyond the currency's minor units (0 or 2).
    pub extra_dp: u32,
}

impl Format {
    /// The decimal places shown for `code`.
    pub fn dp(&self, code: &str) -> u32 {
        minor_units(code) + self.extra_dp
    }

    /// `value` in `code`, rounded and grouped (`1.119,50`).
    pub fn amount(&self, code: &str, value: Decimal) -> String {
        format_amount_in(value, self.dp(code), self.style)
    }

    /// The raw number of `value` in `code`, for editing and the clipboard.
    pub fn raw(&self, code: &str, value: Decimal) -> String {
        raw_amount_in(value, self.dp(code), self.style)
    }
}

/// Formats an amount for display: rounded to `dp` places, with `,` grouping
/// and exactly `dp` decimals (`15,832`, `89.56`, `1,234.500`).
#[cfg(any(test, feature = "test-support"))]
pub fn format_amount(value: Decimal, dp: u32) -> String {
    format_amount_in(value, dp, NumberStyle::COMMA_DOT)
}

/// [`format_amount`] in a number style (`1.234,50`, `1 234,50`).
pub(crate) fn format_amount_in(value: Decimal, dp: u32, style: NumberStyle) -> String {
    let mut rounded = round_to(value, dp);
    rounded.rescale(dp);
    let text = rounded.abs().to_string();
    let (int_part, frac_part) = match text.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (text.as_str(), None),
    };
    let mut grouped = String::with_capacity(text.len() + text.len() / 3 + 1);
    if rounded.is_sign_negative() && !rounded.is_zero() {
        grouped.push('-');
    }
    let digits = int_part.len();
    for (i, ch) in int_part.chars().enumerate() {
        if i > 0 && (digits - i) % 3 == 0 {
            grouped.push(style.group);
        }
        grouped.push(ch);
    }
    if let Some(frac) = frac_part {
        grouped.push(style.decimal);
        grouped.push_str(frac);
    }
    grouped
}

/// The raw number for the clipboard: rounded to `dp`, no grouping, no
/// trailing zeros (`15832`, `89.56`, `89.5`).
#[cfg(test)]
pub(crate) fn raw_amount(value: Decimal, dp: u32) -> String {
    raw_amount_in(value, dp, NumberStyle::COMMA_DOT)
}

/// [`raw_amount`] with the style's decimal separator (`89,5`).
pub(crate) fn raw_amount_in(value: Decimal, dp: u32, style: NumberStyle) -> String {
    let text = round_to(value, dp).normalize().to_string();
    if style.decimal == '.' {
        text
    } else {
        text.replace('.', &style.decimal.to_string())
    }
}

/// The home currency for a BCP 47 / POSIX locale such as `en-US`,
/// `zh_CN.UTF-8` or `de-AT`, from its region subtag.
pub(crate) fn currency_for_locale(locale: &str) -> Option<String> {
    let locale = locale.split(['.', '@']).next()?;
    let region = locale
        .split(['-', '_'])
        .skip(1)
        .find(|part| part.len() == 2 && part.chars().all(|c| c.is_ascii_alphabetic()))?
        .to_ascii_uppercase();
    let country = Country::from_str(&region).ok()?;
    Currency::from_country(country)
        .into_iter()
        .find(|c| c.flags().is_empty())
        .map(|c| c.code().to_string())
}

/// The first-launch list: the home currency, then the defaults, deduplicated.
pub(crate) fn default_currency_list(home: Option<&str>) -> Vec<String> {
    let mut list: Vec<String> = Vec::new();
    for code in home.into_iter().chain(DEFAULT_CURRENCIES) {
        if !list.iter().any(|c| c == code) {
            list.push(code.to_string());
        }
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    #[test]
    fn minor_units_from_iso() {
        assert_eq!(minor_units("USD"), 2);
        assert_eq!(minor_units("JPY"), 0);
        assert_eq!(minor_units("KWD"), 3);
        assert_eq!(minor_units("XAU"), 2);
        assert_eq!(minor_units("BTC"), 2);
    }

    #[test]
    fn regular_currencies() {
        assert!(is_regular_currency("USD"));
        assert!(is_regular_currency("CNY"));
        assert!(!is_regular_currency("XAU"));
        assert!(!is_regular_currency("ADA"));
        assert!(!is_regular_currency("BOV"));
    }

    #[test]
    fn names_and_flags() {
        assert_eq!(currency_name("JPY"), "Japanese Yen");
        assert_eq!(currency_name("USD"), "US Dollar");
        assert_eq!(currency_name("HKD"), "Hong Kong Dollar");
        assert_eq!(currency_name("ZZZ"), "ZZZ");
        assert_eq!(flag_region("USD").as_deref(), Some("us"));
        assert_eq!(flag_region("EUR").as_deref(), Some("eu"));
        assert_eq!(flag_region("HKD").as_deref(), Some("hk"));
        assert_eq!(flag_region("XCG").as_deref(), Some("cw"));
        assert_eq!(flag_region("ANG").as_deref(), Some("cw"));
        for code in ["XAF", "XOF", "XCD", "XPF", "XAU", "XDR", "BTC", "ZZZ"] {
            assert_eq!(flag_region(code), None, "{code}");
        }
        assert_eq!(flag_badge("XAF"), "¤");
        assert_eq!(flag_badge("BTC"), "BT");
        assert_eq!(flag_badge(""), "¤");
    }

    #[test]
    fn chinese_names() {
        use crate::i18n::{Lang, with_lang};
        assert_eq!(display_name("HKD"), "Hong Kong Dollar");
        with_lang(Lang::ZhCn, || {
            assert_eq!(display_name("HKD"), "港元");
            assert_eq!(display_name("USD"), "美元");
            assert_eq!(display_name("CHF"), "瑞士法郎");
            // Not in CLDR: the code.
            assert_eq!(display_name("ZZZ"), "ZZZ");
        });
        assert_eq!(zh_names("HKD").collect::<Vec<_>>(), ["港元", "港币"]);
        assert_eq!(zh_names("USD").collect::<Vec<_>>(), ["美元", "美金"]);
        assert_eq!(zh_names("ZZZ").count(), 0);
        assert_eq!(code_for_zh_name("港币"), Some("HKD"));
        assert_eq!(code_for_zh_name("港元"), Some("HKD"));
        assert_eq!(code_for_zh_name("冰岛克朗"), Some("ISK"));
        assert_eq!(code_for_zh_name("德国马克"), None);
    }

    /// Every ISO 4217 code a row or the palette can hold has a CLDR name.
    #[test]
    fn cldr_names_cover_every_listed_code() {
        let names = crate::generated::cldr_zh::NAMES;
        assert!(names.is_sorted_by_key(|(code, _)| *code));
        let mut checked = 0;
        for a in b'A'..=b'Z' {
            for b in b'A'..=b'Z' {
                for c in b'A'..=b'Z' {
                    let code = String::from_utf8(vec![a, b, c]).unwrap();
                    if is_regular_currency(&code) {
                        let name = cldr_zh_name(&code);
                        assert!(name.is_some_and(|n| !n.is_empty()), "{code}");
                        checked += 1;
                    }
                }
            }
        }
        assert!(checked > 150, "{checked}");
        // The bundled snapshot's codes too.
        let snapshot = crate::rates::bundled();
        for code in snapshot.codes() {
            if Currency::from_code(&code).is_some() {
                assert!(cldr_zh_name(&code).is_some(), "{code}");
            }
        }
    }

    #[test]
    fn conversion_is_exact() {
        // Rates against USD.
        let eur = d("0.8956");
        let jpy = d("158.32");
        assert_eq!(convert(d("100"), Decimal::ONE, eur), Some(d("89.56")));
        assert_eq!(
            convert(d("100"), Decimal::ONE, jpy).map(|v| round_to(v, 0)),
            Some(d("15832"))
        );
        // Cross rate EUR -> JPY.
        let v = convert(d("89.56"), eur, jpy).unwrap();
        assert_eq!(round_to(v, 0), d("15832"));
        assert_eq!(convert(d("1"), Decimal::ZERO, jpy), None);
    }

    #[test]
    fn rounding_half_away_from_zero() {
        assert_eq!(round_to(d("2.345"), 2), d("2.35"));
        assert_eq!(round_to(d("-2.345"), 2), d("-2.35"));
        assert_eq!(round_to(d("15831.5"), 0), d("15832"));
    }

    #[test]
    fn formatting() {
        assert_eq!(format_amount(d("100"), 2), "100.00");
        assert_eq!(format_amount(d("89.555"), 2), "89.56");
        assert_eq!(format_amount(d("15832.4"), 0), "15,832");
        assert_eq!(format_amount(d("1234567.891"), 3), "1,234,567.891");
        assert_eq!(format_amount(d("-1234.5"), 2), "-1,234.50");
        assert_eq!(format_amount(d("0"), 0), "0");
        assert_eq!(format_amount(d("-0.001"), 2), "0.00");
        assert_eq!(format_amount(d("999"), 2), "999.00");
    }

    #[test]
    fn raw_values() {
        assert_eq!(raw_amount(d("15832.4"), 0), "15832");
        assert_eq!(raw_amount(d("89.555"), 2), "89.56");
        assert_eq!(raw_amount(d("89.50"), 2), "89.5");
    }

    #[test]
    fn locale_home_currency() {
        assert_eq!(currency_for_locale("en-US").as_deref(), Some("USD"));
        assert_eq!(currency_for_locale("zh_CN.UTF-8").as_deref(), Some("CNY"));
        assert_eq!(currency_for_locale("de-AT").as_deref(), Some("EUR"));
        assert_eq!(currency_for_locale("zh-Hans-HK").as_deref(), Some("HKD"));
        assert_eq!(currency_for_locale("en").as_deref(), None);
        assert_eq!(currency_for_locale("C").as_deref(), None);
    }

    #[test]
    fn default_list_dedupes() {
        assert_eq!(
            default_currency_list(Some("USD")),
            ["USD", "EUR", "GBP", "JPY", "CNY"]
        );
        assert_eq!(
            default_currency_list(Some("HKD")),
            ["HKD", "USD", "EUR", "GBP", "JPY", "CNY"]
        );
        assert_eq!(
            default_currency_list(None),
            ["USD", "EUR", "GBP", "JPY", "CNY"]
        );
    }

    #[test]
    fn number_styles() {
        let v = d("1234567.891");
        assert_eq!(
            format_amount_in(v, 2, NumberStyle::COMMA_DOT),
            "1,234,567.89"
        );
        assert_eq!(
            format_amount_in(v, 2, NumberStyle::DOT_COMMA),
            "1.234.567,89"
        );
        assert_eq!(
            format_amount_in(v, 2, NumberStyle::SPACE_COMMA),
            "1\u{202f}234\u{202f}567,89"
        );
        assert_eq!(
            format_amount_in(d("-1234.5"), 0, NumberStyle::DOT_COMMA),
            "-1.235"
        );
        assert_eq!(
            raw_amount_in(d("89.555"), 2, NumberStyle::DOT_COMMA),
            "89,56"
        );
        assert_eq!(
            raw_amount_in(d("1250"), 2, NumberStyle::SPACE_COMMA),
            "1250"
        );
        assert_eq!(raw_amount_in(d("12.5"), 2, NumberStyle::COMMA_DOT), "12.5");
    }

    #[test]
    fn number_style_for_locale() {
        let style = |l: &str| NumberStyle::for_locale(Some(l));
        assert_eq!(style("en-US"), NumberStyle::COMMA_DOT);
        assert_eq!(style("zh_CN.UTF-8"), NumberStyle::COMMA_DOT);
        assert_eq!(style("ja-JP"), NumberStyle::COMMA_DOT);
        assert_eq!(style("de-DE"), NumberStyle::DOT_COMMA);
        assert_eq!(style("de_AT.UTF-8"), NumberStyle::DOT_COMMA);
        assert_eq!(style("de-CH"), NumberStyle::COMMA_DOT);
        assert_eq!(style("es-ES"), NumberStyle::DOT_COMMA);
        assert_eq!(style("es-MX"), NumberStyle::COMMA_DOT);
        assert_eq!(style("pt-BR"), NumberStyle::DOT_COMMA);
        assert_eq!(style("pt-PT"), NumberStyle::SPACE_COMMA);
        assert_eq!(style("fr-FR"), NumberStyle::SPACE_COMMA);
        assert_eq!(style("fr-CA"), NumberStyle::SPACE_COMMA);
        assert_eq!(style("ru"), NumberStyle::SPACE_COMMA);
        assert_eq!(style("sv-SE"), NumberStyle::SPACE_COMMA);
        assert_eq!(style("C"), NumberStyle::COMMA_DOT);
        assert_eq!(NumberStyle::for_locale(None), NumberStyle::COMMA_DOT);
    }

    #[test]
    fn display_formats() {
        let extra = Format {
            style: NumberStyle::DOT_COMMA,
            extra_dp: 2,
        };
        assert_eq!(extra.dp("JPY"), 2);
        assert_eq!(extra.dp("USD"), 4);
        assert_eq!(extra.amount("USD", d("1119.49512")), "1.119,4951");
        assert_eq!(extra.raw("JPY", d("197875.456")), "197875,46");
        let plain = Format::default();
        assert_eq!(plain.amount("JPY", d("197875.456")), "197,875");
        assert_eq!(plain.raw("USD", d("89.5")), "89.5");
    }
}
