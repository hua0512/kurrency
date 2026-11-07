//! Smart paste (DESIGN §2): reads a price such as `€45.99`, `¥1,200`,
//! `45,99 EUR`, `HK$300`, `US$ 12`, `1.234,56 €`, `£1.2k` or `300 hkd` into
//! a currency and an amount.
//!
//! - The currency is an ISO code (any case), a symbol (`€`, `HK$`, `kr`), or a
//!   common Chinese name (`港币`), before or after the number. Ambiguous
//!   symbols are resolved against the user's list, see [`resolve_currency`].
//! - Both decimal conventions are read: when a number has two kinds of
//!   separator the last one is the decimal point (`1.234,56`, `1,234.56`);
//!   a single `.` or `,` followed by exactly three digits groups thousands
//!   (`1,200`, `1.200`), otherwise it is the decimal point (`45,99`).
//! - `k` and `m` multiply by a thousand and a million (`£1.2k`).
//!
//! Plain Rust without GPUI.

use std::str::FromStr;

use rust_decimal::Decimal;

use crate::money;

/// A parsed price. `code` is `None` when the text is just a number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Price {
    pub code: Option<String>,
    pub amount: Decimal,
}

/// Longer clipboard contents are prose, not a price.
const MAX_LEN: usize = 48;

/// Currencies written with a bare `$`.
const DOLLARS: [&str; 30] = [
    "USD", "CAD", "AUD", "NZD", "HKD", "SGD", "TWD", "MXN", "ARS", "CLP", "COP", "UYU", "BSD",
    "BBD", "BZD", "BMD", "BND", "FJD", "GYD", "JMD", "LRD", "NAD", "SBD", "SRD", "TTD", "XCD",
    "KYD", "DOP", "CUP", "TOP",
];
/// Currencies written with a bare `¥`.
const YENS: [&str; 2] = ["JPY", "CNY"];
/// Currencies written with a bare `kr`.
const KRONER: [&str; 4] = ["SEK", "NOK", "DKK", "ISK"];

/// Reads `text` as a price. `list` is the user's currencies, anchor first;
/// it decides ambiguous symbols.
pub fn parse_price(text: &str, list: &[String]) -> Option<Price> {
    let text = normalize(text)?;
    let start = text.find(|c: char| c.is_ascii_digit())?;
    let (prefix, rest) = text.split_at(start);
    let (number, multiplier, suffix) = split_number(rest);
    let mut amount = parse_number(number)?;
    if let Some(multiplier) = multiplier {
        amount = amount.checked_mul(multiplier)?;
    }

    let (prefix, suffix) = (prefix.trim(), suffix.trim());
    let code = match (prefix.is_empty(), suffix.is_empty()) {
        (true, true) => None,
        (false, true) => Some(resolve_currency(prefix, list)?),
        (true, false) => Some(resolve_currency(suffix, list)?),
        // `$12 USD`: both must agree.
        (false, false) => {
            let before = resolve_currency(prefix, list)?;
            if resolve_currency(suffix, list)? != before {
                return None;
            }
            Some(before)
        }
    };
    Some(Price {
        code,
        amount: amount.normalize(),
    })
}

/// One line, with the various spaces and full-width forms made plain.
fn normalize(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() || text.contains(['\n', '\r']) || text.chars().count() > MAX_LEN {
        return None;
    }
    Some(
        text.chars()
            .map(|c| match c {
                '\u{a0}' | '\u{202f}' | '\u{2009}' | '\u{2007}' | '\u{3000}' => ' ',
                '０'..='９' => char::from(b'0' + (c as u32 - '０' as u32) as u8),
                '，' => ',',
                '．' => '.',
                '＄' => '$',
                '￥' => '¥',
                '￡' => '£',
                '’' => '\'',
                c => c,
            })
            .collect(),
    )
}

/// Splits `digits[seps digits]… [k|m] rest` off the start of `text`, which
/// starts with a digit. A separator only counts when a digit follows it, and
/// `k`/`m` only when no letter follows (`5mxn` is 5 MXN).
fn split_number(text: &str) -> (&str, Option<Decimal>, &str) {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut end = 0;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i].1;
        if c.is_ascii_digit() {
            i += 1;
            end = i;
        } else if matches!(c, '.' | ',' | '\'' | ' ')
            && chars.get(i + 1).is_some_and(|(_, n)| n.is_ascii_digit())
        {
            i += 1;
        } else {
            break;
        }
    }
    let byte = |ix: usize| chars.get(ix).map_or(text.len(), |(b, _)| *b);
    let number = &text[..byte(end)];
    let mut rest = &text[byte(end)..];
    let mut multiplier = None;
    let mut it = rest.chars();
    if let Some(c) = it.next()
        && !it.next().is_some_and(char::is_alphabetic)
    {
        multiplier = match c {
            'k' | 'K' => Some(Decimal::from(1_000)),
            'm' | 'M' => Some(Decimal::from(1_000_000)),
            _ => None,
        };
        if multiplier.is_some() {
            rest = &rest[1..];
        }
    }
    (number, multiplier, rest)
}

/// `1,234.56`, `1.234,56`, `1 234,56`, `1'234.5`, `1,200`, `45,99`.
fn parse_number(text: &str) -> Option<Decimal> {
    let seps: Vec<(usize, char)> = text
        .char_indices()
        .filter(|(_, c)| !c.is_ascii_digit())
        .collect();
    let Some(&(last_at, last)) = seps.last() else {
        return Decimal::from_str(text).ok();
    };
    let mut kinds: Vec<char> = seps.iter().map(|(_, c)| *c).collect();
    kinds.dedup();
    let mut sorted = kinds.clone();
    sorted.sort_unstable();
    sorted.dedup();

    let decimal_at = match sorted.len() {
        1 => {
            let grouping_only = matches!(last, ' ' | '\'') || seps.len() > 1;
            let before = &text[..last_at];
            let after = &text[last_at + 1..];
            let thousands =
                grouping_only || (after.len() == 3 && before.len() <= 3 && before != "0");
            (!thousands).then_some(last_at)
        }
        2 => {
            // The last separator is the decimal point; it appears once, after
            // every grouping separator.
            if matches!(last, ' ' | '\'') || seps.iter().filter(|(_, c)| *c == last).count() != 1 {
                return None;
            }
            Some(last_at)
        }
        _ => return None,
    };

    let (int_part, frac) = match decimal_at {
        Some(at) => (&text[..at], &text[at + 1..]),
        None => (text, ""),
    };
    let groups: Vec<&str> = int_part.split(|c: char| !c.is_ascii_digit()).collect();
    if groups.len() > 1 {
        // Thousands groups: the last has three digits, the others one to
        // three (Indian 1,00,000 included).
        let (tail, head) = groups.split_last()?;
        if tail.len() != 3 || head.iter().any(|g| g.is_empty() || g.len() > 3) {
            return None;
        }
    }
    let digits: String = groups.concat();
    let literal = if frac.is_empty() {
        digits
    } else {
        format!("{digits}.{frac}")
    };
    Decimal::from_str(&literal).ok()
}

/// The ISO code for a currency token: a code in any case (`hkd`), a symbol
/// (`€`, `HK$`, `kr.`) or a Chinese name (`港币`). Ambiguous symbols use the
/// user's `list` (anchor first):
/// - `$` is the anchor when it is a dollar, else the list's one dollar when
///   it has exactly one and no USD, else USD;
/// - `¥` is JPY or CNY, whichever is the anchor or comes first in the list,
///   else JPY;
/// - `kr` is SEK, NOK, DKK or ISK the same way, else SEK.
pub(crate) fn resolve_currency(token: &str, list: &[String]) -> Option<String> {
    let token = token.trim();
    if token.len() == 3 && token.chars().all(|c| c.is_ascii_alphabetic()) {
        let code = token.to_ascii_uppercase();
        if money::is_regular_currency(&code) || list.contains(&code) {
            return Some(code);
        }
    }
    if let Some(code) = money::code_for_zh_name(token) {
        return Some(code.to_string());
    }
    let lower = token.to_lowercase();
    let key = lower.trim_end_matches('.');
    let code = match key {
        "$" => return Some(pick_dollar(list)),
        "¥" => return Some(pick(&YENS, list)),
        "kr" => return Some(pick(&KRONER, list)),
        "us$" | "u$s" => "USD",
        "hk$" => "HKD",
        "c$" | "ca$" | "can$" => "CAD",
        "a$" | "au$" => "AUD",
        "nz$" => "NZD",
        "s$" | "sg$" => "SGD",
        "nt$" => "TWD",
        "mx$" => "MXN",
        "r$" => "BRL",
        "€" => "EUR",
        "£" => "GBP",
        "jp¥" | "円" => "JPY",
        "cn¥" | "rmb" | "元" => "CNY",
        "₩" | "원" => "KRW",
        "₹" | "rs" => "INR",
        "₽" | "руб" => "RUB",
        "₺" | "tl" => "TRY",
        "₪" | "nis" => "ILS",
        "₫" => "VND",
        "₱" => "PHP",
        "฿" => "THB",
        "₴" => "UAH",
        "₦" => "NGN",
        "₸" => "KZT",
        "₾" => "GEL",
        "₡" => "CRC",
        "zł" | "zl" => "PLN",
        "kč" => "CZK",
        "ft" => "HUF",
        "fr" | "sfr" => "CHF",
        "r" => "ZAR",
        "rm" => "MYR",
        "rp" => "IDR",
        "lei" => "RON",
        _ => return None,
    };
    Some(code.to_string())
}

/// The anchor when it is one of `candidates`, else the first of them in the
/// list, else the first candidate.
fn pick(candidates: &[&str], list: &[String]) -> String {
    list.iter()
        .find(|code| candidates.contains(&code.as_str()))
        .cloned()
        .unwrap_or_else(|| candidates[0].to_string())
}

fn pick_dollar(list: &[String]) -> String {
    let is_dollar = |code: &String| DOLLARS.contains(&code.as_str());
    if let Some(anchor) = list.first().filter(|code| is_dollar(code)) {
        return anchor.clone();
    }
    let others: Vec<&String> = list
        .iter()
        .filter(|code| is_dollar(code) && *code != "USD")
        .collect();
    let has_usd = list.iter().any(|code| code == "USD");
    match others.as_slice() {
        [only] if !has_usd => (*only).clone(),
        _ => "USD".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn list(codes: &[&str]) -> Vec<String> {
        codes.iter().map(ToString::to_string).collect()
    }

    /// Parses against a USD-anchored default list.
    fn p(text: &str) -> Option<(Option<String>, Decimal)> {
        parse_with(text, &["USD", "EUR", "GBP", "JPY", "CNY"])
    }

    fn parse_with(text: &str, codes: &[&str]) -> Option<(Option<String>, Decimal)> {
        parse_price(text, &list(codes)).map(|price| (price.code, price.amount))
    }

    fn priced(code: &str, amount: &str) -> Option<(Option<String>, Decimal)> {
        Some((Some(code.to_string()), d(amount)))
    }

    #[test]
    fn the_spec_examples() {
        assert_eq!(p("€45.99"), priced("EUR", "45.99"));
        assert_eq!(p("¥1,200"), priced("JPY", "1200"));
        assert_eq!(p("45,99 EUR"), priced("EUR", "45.99"));
        assert_eq!(p("HK$300"), priced("HKD", "300"));
        assert_eq!(p("US$ 12"), priced("USD", "12"));
        assert_eq!(p("1.234,56 €"), priced("EUR", "1234.56"));
        assert_eq!(p("£1.2k"), priced("GBP", "1200"));
        assert_eq!(p("300 hkd"), priced("HKD", "300"));
    }

    #[test]
    fn plain_numbers_have_no_currency() {
        assert_eq!(p("42"), Some((None, d("42"))));
        assert_eq!(p("  42.5 "), Some((None, d("42.5"))));
        assert_eq!(p("1,200"), Some((None, d("1200"))));
        assert_eq!(p("1.5k"), Some((None, d("1500"))));
        assert_eq!(p("2M"), Some((None, d("2000000"))));
        assert_eq!(p("0"), Some((None, d("0"))));
    }

    #[test]
    fn codes_in_any_case_and_position() {
        assert_eq!(p("EUR 45"), priced("EUR", "45"));
        assert_eq!(p("eur45"), priced("EUR", "45"));
        assert_eq!(p("45eur"), priced("EUR", "45"));
        assert_eq!(p("45 Chf"), priced("CHF", "45"));
        assert_eq!(p("5mxn"), priced("MXN", "5"));
        assert_eq!(p("5 m"), None);
        assert_eq!(p("$12 USD"), priced("USD", "12"));
        assert_eq!(p("€12 USD"), None);
    }

    #[test]
    fn decimal_conventions() {
        // Two kinds of separator: the last is the decimal point.
        assert_eq!(p("1,234.56"), Some((None, d("1234.56"))));
        assert_eq!(p("1.234,56"), Some((None, d("1234.56"))));
        assert_eq!(p("1.234.567,8"), Some((None, d("1234567.8"))));
        assert_eq!(p("1,234,567.89"), Some((None, d("1234567.89"))));
        assert_eq!(p("1 234,56 €"), priced("EUR", "1234.56"));
        assert_eq!(p("1\u{a0}234,56 €"), priced("EUR", "1234.56"));
        assert_eq!(p("CHF 1'234.50"), priced("CHF", "1234.5"));
        // One separator: three digits after it group thousands.
        assert_eq!(p("1.200"), Some((None, d("1200"))));
        assert_eq!(p("12,500 JPY"), priced("JPY", "12500"));
        assert_eq!(p("0.125"), Some((None, d("0.125"))));
        assert_eq!(p("45,9"), Some((None, d("45.9"))));
        assert_eq!(p("1234.567"), Some((None, d("1234.567"))));
        // Repeated: grouping only.
        assert_eq!(p("1,000,000"), Some((None, d("1000000"))));
        assert_eq!(p("1.000.000"), Some((None, d("1000000"))));
        // Indian grouping.
        assert_eq!(p("₹1,00,000"), priced("INR", "100000"));
    }

    #[test]
    fn malformed_numbers() {
        assert_eq!(p("1,2,3"), None);
        assert_eq!(p("1.234.56"), None);
        assert_eq!(p("1,23.456,7"), None);
        assert_eq!(p("12 34"), None);
        assert_eq!(p("1.234,567.8"), None);
    }

    #[test]
    fn trailing_separators_belong_to_the_text() {
        // "45." — the dot has no digit after it, so it is the suffix.
        assert_eq!(p("45."), None);
        assert_eq!(p("kr. 45"), priced("SEK", "45"));
        assert_eq!(p("Fr. 12.50"), priced("CHF", "12.5"));
        assert_eq!(p("45 kr."), priced("SEK", "45"));
    }

    #[test]
    fn not_prices() {
        assert_eq!(p(""), None);
        assert_eq!(p("hello"), None);
        assert_eq!(p("call 555 1234"), None);
        assert_eq!(p("45 apples"), None);
        assert_eq!(p("EUR"), None);
        assert_eq!(p("-5 EUR"), None);
        assert_eq!(p("5\n6"), None);
        assert_eq!(p("12 to 15 EUR"), None);
        assert_eq!(p(&"9".repeat(60)), None);
    }

    #[test]
    fn many_symbols() {
        assert_eq!(p("₩50,000"), priced("KRW", "50000"));
        assert_eq!(p("₹ 1,499"), priced("INR", "1499"));
        assert_eq!(p("R$ 29,90"), priced("BRL", "29.9"));
        assert_eq!(p("A$20"), priced("AUD", "20"));
        assert_eq!(p("C$ 5.25"), priced("CAD", "5.25"));
        assert_eq!(p("NT$300"), priced("TWD", "300"));
        assert_eq!(p("S$8"), priced("SGD", "8"));
        assert_eq!(p("12 zł"), priced("PLN", "12"));
        assert_eq!(p("12 Kč"), priced("CZK", "12"));
        assert_eq!(p("1990 Ft"), priced("HUF", "1990"));
        assert_eq!(p("JP¥500"), priced("JPY", "500"));
        assert_eq!(p("CN¥500"), priced("CNY", "500"));
        assert_eq!(p("RMB 88"), priced("CNY", "88"));
        assert_eq!(p("88元"), priced("CNY", "88"));
        assert_eq!(p("500円"), priced("JPY", "500"));
        assert_eq!(p("300港币"), priced("HKD", "300"));
        assert_eq!(p("100 美元"), priced("USD", "100"));
        assert_eq!(p("฿100"), priced("THB", "100"));
    }

    #[test]
    fn full_width_forms() {
        assert_eq!(p("￥１，２００"), priced("JPY", "1200"));
        assert_eq!(p("＄５"), priced("USD", "5"));
    }

    #[test]
    fn dollar_resolution() {
        // USD unless the anchor is a dollar, or the list holds exactly one
        // other dollar and no USD.
        assert_eq!(parse_with("$5", &["EUR", "GBP"]), priced("USD", "5"));
        assert_eq!(parse_with("$5", &["EUR", "CAD"]), priced("CAD", "5"));
        assert_eq!(parse_with("$5", &["EUR", "CAD", "AUD"]), priced("USD", "5"));
        assert_eq!(parse_with("$5", &["EUR", "USD", "CAD"]), priced("USD", "5"));
        assert_eq!(parse_with("$5", &["HKD", "USD"]), priced("HKD", "5"));
        assert_eq!(parse_with("$5", &["USD", "HKD"]), priced("USD", "5"));
        // Prefixed dollars are never ambiguous.
        assert_eq!(parse_with("US$5", &["HKD"]), priced("USD", "5"));
    }

    #[test]
    fn yen_resolution() {
        assert_eq!(parse_with("¥5", &["USD", "EUR"]), priced("JPY", "5"));
        assert_eq!(parse_with("¥5", &["USD", "CNY"]), priced("CNY", "5"));
        assert_eq!(parse_with("¥5", &["USD", "JPY", "CNY"]), priced("JPY", "5"));
        assert_eq!(parse_with("¥5", &["USD", "CNY", "JPY"]), priced("CNY", "5"));
        // The anchor wins.
        assert_eq!(parse_with("¥5", &["JPY", "CNY"]), priced("JPY", "5"));
        assert_eq!(parse_with("¥5", &["CNY", "JPY"]), priced("CNY", "5"));
    }

    #[test]
    fn krona_resolution() {
        assert_eq!(parse_with("45 kr", &["USD"]), priced("SEK", "45"));
        assert_eq!(parse_with("45 kr", &["USD", "NOK"]), priced("NOK", "45"));
        assert_eq!(parse_with("45 kr", &["DKK", "NOK"]), priced("DKK", "45"));
        assert_eq!(
            parse_with("kr 45", &["EUR", "ISK", "SEK"]),
            priced("ISK", "45")
        );
        assert_eq!(parse_with("45 KR", &["USD", "NOK"]), priced("NOK", "45"));
    }

    #[test]
    fn codes_outside_iso_only_when_listed() {
        assert_eq!(p("5 XYZ"), None);
        assert_eq!(parse_with("5 xyz", &["USD", "XYZ"]), priced("XYZ", "5"));
    }
}
