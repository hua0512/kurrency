//! Presentation logic for the v2 main window (docs/DESIGN.md §1–§5): how an
//! amount splits into symbol / integer / decimals, the daily change chip, the
//! inverse rate line, type-anywhere key classification and amount stepping.
//! The layout metrics and the window fit are the app's (`kurrency::layout`).
//!
//! Plain Rust without GPUI, so all of it is unit-tested here.

use iso_currency::Currency;
use rust_decimal::Decimal;

use crate::money;
use crate::rates::history;

/// An amount split for display: `€ 1,119.50` → `€`, `1,119`, `.50`. The
/// symbol is drawn at 45% opacity and the decimals at 55%.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmountParts {
    /// [`display_symbol`]: the symbol, or the ISO code standing in for it.
    pub symbol: Option<String>,
    /// The sign and grouped integer digits (`-1,234`).
    pub integer: String,
    /// The decimal point and the minor-unit digits (`.50`), or empty.
    pub fraction: String,
}

/// The display symbol of a currency (`$`, `€`, `¥`, `HK$`), or `None` when
/// ISO 4217 only has the generic `¤`.
pub(crate) fn currency_symbol(code: &str) -> Option<String> {
    // ISO's ₣ for the Swiss franc is historic; people write "Fr.".
    if code == "CHF" {
        return Some("Fr.".to_string());
    }
    let symbol = Currency::from_code(code)?.symbol().symbol;
    (!symbol.is_empty() && symbol != "¤").then_some(symbol)
}

/// The symbol drawn before an amount: [`currency_symbol`] when `drawable`
/// says the bundled fonts have a glyph for each of its characters, else the
/// ISO code (`AED 1,250.00`), never a box or a system font's glyph. The code
/// is how CLDR's English and Chinese locales write these currencies anyway
/// (Arabic-script symbols such as `د.إ` and `ر.س`, `৳`, `ლ`, `៛`, `֏`), and
/// GPUI has no bidirectional layout for the right-to-left ones. `None` when
/// the currency has no symbol at all.
pub fn display_symbol(code: &str, drawable: impl Fn(char) -> bool) -> Option<String> {
    let symbol = currency_symbol(code)?;
    Some(if symbol.chars().all(drawable) {
        symbol
    } else {
        code.to_string()
    })
}

/// Splits a formatted amount (`1,119.50`) into integer and fraction parts
/// at the `decimal` separator.
pub(crate) fn split_formatted(formatted: &str, decimal: char) -> (String, String) {
    match formatted.find(decimal) {
        Some(at) => (formatted[..at].to_string(), formatted[at..].to_string()),
        None => (formatted.to_string(), String::new()),
    }
}

/// `value` in `code`, rounded to the places `format` shows and split, with
/// the symbol the fonts can draw (see [`display_symbol`]).
pub fn amount_parts(
    code: &str,
    value: Decimal,
    format: &money::Format,
    drawable: impl Fn(char) -> bool,
) -> AmountParts {
    let formatted = format.amount(code, value);
    let (integer, fraction) = split_formatted(&formatted, format.style.decimal);
    AmountParts {
        symbol: display_symbol(code, drawable),
        integer,
        fraction,
    }
}

/// The live result under an expression: `= 4,500`, or `= 4,500.25` when it
/// has a fraction (rounded to the currency's minor units).
pub fn echo_text(value: Decimal, dp: u32, style: money::NumberStyle) -> String {
    let rounded = money::round_to(value, dp);
    let dp = if rounded.fract().is_zero() { 0 } else { dp };
    format!("= {}", money::format_amount_in(rounded, dp, style))
}

/// The daily change of a row: units of the row's currency per anchor unit,
/// today against the previous day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// The rate rose by this many percent (rounded to one decimal, > 0).
    Up(Decimal),
    /// The rate fell by this many percent (rounded to one decimal, > 0).
    Down(Decimal),
    /// Unchanged at one decimal, or no data for the previous day.
    Flat,
}

impl Change {
    /// Compares today's rate with the previous day's.
    pub fn between(today: Option<Decimal>, previous: Option<Decimal>) -> Self {
        let (Some(today), Some(previous)) = (today, previous) else {
            return Self::Flat;
        };
        if previous.is_zero() {
            return Self::Flat;
        }
        let Some(pct) = today
            .checked_sub(previous)
            .and_then(|d| d.checked_div(previous))
            .and_then(|r| r.checked_mul(Decimal::ONE_HUNDRED))
        else {
            return Self::Flat;
        };
        let rounded = money::round_to(pct, 1);
        if rounded.is_zero() {
            Self::Flat
        } else if rounded.is_sign_positive() {
            Self::Up(rounded.normalize())
        } else {
            Self::Down((-rounded).normalize())
        }
    }

    /// `▴ 0.1%`, `▾ 0.3%`, or `·`.
    pub fn label(self) -> String {
        match self {
            Self::Up(pct) => format!("▴ {}%", one_decimal(pct)),
            Self::Down(pct) => format!("▾ {}%", one_decimal(pct)),
            Self::Flat => "·".to_string(),
        }
    }
}

fn one_decimal(pct: Decimal) -> String {
    let mut pct = pct;
    pct.rescale(1);
    pct.to_string()
}

/// `1 EUR = 1.1165 USD`: what one unit of the row's currency is worth in the
/// anchor currency. `rate` is anchor units per one `code` unit.
pub fn inverse_rate_text(code: &str, anchor: &str, rate: Decimal, decimal: char) -> String {
    let dp = history::rate_decimals(rate);
    crate::i18n::tr!(
        "row.inverse",
        code = code,
        rate = history::format_rate_in(rate, dp, decimal),
        anchor = anchor
    )
}

/// What a keystroke means when no text input has focus (DESIGN §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeAnywhere {
    /// Start editing the anchor amount with this character.
    Amount(char),
    /// Open the Ctrl+K palette with this letter as the query.
    Search(char),
    Ignore,
}

/// Classifies the text a key would type. `with_command_modifier` is true when
/// Ctrl, Alt, Cmd or Fn is held, which never types. `decimal` is the number
/// format's decimal separator: with `,` (`1.234,56`, `1 234,56`) a comma
/// starts an amount too, like `.` does.
pub fn classify_key(
    key_char: Option<&str>,
    with_command_modifier: bool,
    decimal: char,
) -> TypeAnywhere {
    if with_command_modifier {
        return TypeAnywhere::Ignore;
    }
    let mut chars = key_char.unwrap_or_default().chars();
    let (Some(ch), None) = (chars.next(), chars.next()) else {
        return TypeAnywhere::Ignore;
    };
    match ch {
        '0'..='9' | '.' | '(' | '-' => TypeAnywhere::Amount(ch),
        _ if ch == decimal => TypeAnywhere::Amount(ch),
        _ if ch.is_alphabetic() => TypeAnywhere::Search(ch),
        _ => TypeAnywhere::Ignore,
    }
}

/// ↑/↓ in the anchor input: the next amount when the text is a plain number
/// (or empty), stepping by 1, or 10 with Shift, and never below zero.
pub fn step_amount(
    text: &str,
    up: bool,
    shift: bool,
    style: money::NumberStyle,
) -> Option<Decimal> {
    let current = if text.trim().is_empty() {
        Decimal::ZERO
    } else if crate::expr::is_plain_number_in(text, style) {
        crate::expr::eval_in(text, style).ok()?
    } else {
        return None;
    };
    let step = if shift { Decimal::TEN } else { Decimal::ONE };
    let next = if up {
        current.checked_add(step)?
    } else {
        current.checked_sub(step)?
    };
    Some(next.max(Decimal::ZERO).normalize())
}

/// The quick-amount chips under the anchor card.
pub const QUICK_AMOUNTS: [(&str, i64); 4] =
    [("10", 10), ("100", 100), ("1k", 1_000), ("10k", 10_000)];

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    #[test]
    fn symbols() {
        assert_eq!(currency_symbol("USD").as_deref(), Some("$"));
        assert_eq!(currency_symbol("EUR").as_deref(), Some("€"));
        assert_eq!(currency_symbol("JPY").as_deref(), Some("¥"));
        assert_eq!(currency_symbol("HKD").as_deref(), Some("HK$"));
        assert_eq!(currency_symbol("CHF").as_deref(), Some("Fr."));
        assert_eq!(currency_symbol("XXX"), None);
        assert_eq!(currency_symbol("BTC"), None);
        // What the fonts cannot draw shows as the code.
        let latin = |c: char| c.is_ascii() || c == '€';
        assert_eq!(display_symbol("EUR", latin).as_deref(), Some("€"));
        assert_eq!(display_symbol("HKD", latin).as_deref(), Some("HK$"));
        assert_eq!(display_symbol("JPY", latin).as_deref(), Some("JPY"));
        assert_eq!(display_symbol("AED", |_| true).as_deref(), Some("د.إ"));
        assert_eq!(display_symbol("XXX", latin), None);
    }

    #[test]
    fn amount_splits() {
        assert_eq!(
            amount_parts("EUR", d("1119.495"), &money::Format::default(), |_| true),
            AmountParts {
                symbol: Some("€".into()),
                integer: "1,119".into(),
                fraction: ".50".into(),
            }
        );
        let jpy = amount_parts("JPY", d("197875.4"), &money::Format::default(), |_| true);
        assert_eq!(
            (jpy.integer.as_str(), jpy.fraction.as_str()),
            ("197,875", "")
        );
        let kwd = amount_parts("KWD", d("-1.5"), &money::Format::default(), |_| true);
        assert_eq!(
            (kwd.integer.as_str(), kwd.fraction.as_str()),
            ("-1", ".500")
        );
        assert_eq!(split_formatted("—", '.'), ("—".to_string(), String::new()));
        // Other number formats and extra precision.
        let de = money::Format {
            style: money::NumberStyle::DOT_COMMA,
            extra_dp: 2,
        };
        let eur = amount_parts("EUR", d("1119.49512"), &de, |_| true);
        assert_eq!(
            (eur.integer.as_str(), eur.fraction.as_str()),
            ("1.119", ",4951")
        );
        let fr = money::Format {
            style: money::NumberStyle::SPACE_COMMA,
            extra_dp: 0,
        };
        let jpy = amount_parts("JPY", d("197875.4"), &fr, |_| true);
        assert_eq!(
            (jpy.integer.as_str(), jpy.fraction.as_str()),
            ("197\u{202f}875", "")
        );
    }

    #[test]
    fn echoes() {
        assert_eq!(
            echo_text(d("4500"), 2, money::NumberStyle::COMMA_DOT),
            "= 4,500"
        );
        assert_eq!(
            echo_text(d("4500.004"), 2, money::NumberStyle::COMMA_DOT),
            "= 4,500"
        );
        assert_eq!(
            echo_text(d("642.857142"), 2, money::NumberStyle::COMMA_DOT),
            "= 642.86"
        );
        assert_eq!(
            echo_text(d("1234.5"), 0, money::NumberStyle::COMMA_DOT),
            "= 1,235"
        );
        assert_eq!(
            echo_text(d("642.857142"), 2, money::NumberStyle::DOT_COMMA),
            "= 642,86"
        );
    }

    #[test]
    fn daily_change() {
        assert_eq!(
            Change::between(Some(d("0.8956")), Some(d("0.8983"))),
            Change::Down(d("0.3"))
        );
        assert_eq!(
            Change::between(Some(d("158.32")), Some(d("158.16"))),
            Change::Up(d("0.1"))
        );
        // Under 0.05% rounds to unchanged.
        assert_eq!(
            Change::between(Some(d("7.1240")), Some(d("7.1238"))),
            Change::Flat
        );
        assert_eq!(Change::between(Some(d("1")), None), Change::Flat);
        assert_eq!(Change::between(None, Some(d("1"))), Change::Flat);
        assert_eq!(Change::between(Some(d("1")), Some(d("0"))), Change::Flat);
        assert_eq!(Change::Down(d("0.3")).label(), "▾ 0.3%");
        assert_eq!(Change::Up(d("2")).label(), "▴ 2.0%");
        assert_eq!(Change::Up(d("12.5")).label(), "▴ 12.5%");
        assert_eq!(Change::Flat.label(), "·");
    }

    #[test]
    fn inverse_rates() {
        assert_eq!(
            inverse_rate_text("EUR", "USD", d("1.116537"), '.'),
            "1 EUR = 1.1165 USD"
        );
        assert_eq!(
            inverse_rate_text("JPY", "USD", d("0.0063171"), '.'),
            "1 JPY = 0.006317 USD"
        );
        assert_eq!(
            inverse_rate_text("USD", "JPY", d("158.3249"), '.'),
            "1 USD = 158.32 JPY"
        );
        assert_eq!(
            inverse_rate_text("EUR", "USD", d("1.116537"), ','),
            "1 EUR = 1,1165 USD"
        );
    }

    #[test]
    fn key_classification() {
        use TypeAnywhere::*;
        assert_eq!(classify_key(Some("7"), false, '.'), Amount('7'));
        assert_eq!(classify_key(Some("."), false, '.'), Amount('.'));
        assert_eq!(classify_key(Some("("), false, '.'), Amount('('));
        assert_eq!(classify_key(Some("-"), false, '.'), Amount('-'));
        assert_eq!(classify_key(Some("j"), false, '.'), Search('j'));
        assert_eq!(classify_key(Some("E"), false, '.'), Search('E'));
        assert_eq!(classify_key(Some("7"), true, '.'), Ignore);
        assert_eq!(classify_key(Some("k"), true, '.'), Ignore);
        assert_eq!(classify_key(Some(" "), false, '.'), Ignore);
        assert_eq!(classify_key(Some("+"), false, '.'), Ignore);
        assert_eq!(classify_key(Some("ab"), false, '.'), Ignore);
        assert_eq!(classify_key(None, false, '.'), Ignore);
        // A comma starts an amount only where it is the decimal separator.
        assert_eq!(classify_key(Some(","), false, '.'), Ignore);
        assert_eq!(classify_key(Some(","), false, ','), Amount(','));
        assert_eq!(classify_key(Some("."), false, ','), Amount('.'));
        assert_eq!(classify_key(Some(","), true, ','), Ignore);
    }

    #[test]
    fn stepping() {
        assert_eq!(
            step_amount("100", true, false, Default::default()),
            Some(d("101"))
        );
        assert_eq!(
            step_amount("100", false, true, Default::default()),
            Some(d("90"))
        );
        assert_eq!(
            step_amount("12.5", true, false, Default::default()),
            Some(d("13.5"))
        );
        assert_eq!(
            step_amount("1,250", true, true, Default::default()),
            Some(d("1260"))
        );
        assert_eq!(
            step_amount("", true, false, Default::default()),
            Some(d("1"))
        );
        assert_eq!(
            step_amount("3", false, true, Default::default()),
            Some(d("0"))
        );
        assert_eq!(step_amount("1.5k*3", true, false, Default::default()), None);
        assert_eq!(step_amount("2k", true, false, Default::default()), None);
        let de = money::NumberStyle::DOT_COMMA;
        assert_eq!(step_amount("12,5", true, false, de), Some(d("13.5")));
        assert_eq!(step_amount("1.250", true, true, de), Some(d("1260")));
    }
}
