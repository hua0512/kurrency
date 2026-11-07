//! The ECB's daily reference rates as XML, the last fallback of the Auto
//! chain and of "ECB official":
//!
//! ```xml
//! <Cube><Cube time='2026-11-06'><Cube currency='USD' rate='1.1165'/>…
//! ```
//!
//! The feed quotes against the euro. The table is rebased to USD (the
//! cross rates are computed here), like every other provider's, with EUR
//! added. A tiny tag scanner instead of an XML parser: the format has been
//! stable for two decades. Plain Rust without GPUI.

use std::collections::BTreeMap;
use std::str::FromStr;

use anyhow::{Context as _, bail};
use rust_decimal::Decimal;

use super::provider::{RateTable, Source};

pub(crate) const URL: &str = "https://www.ecb.europa.eu/stats/eurofxref/eurofxref-daily.xml";

/// Decimal places kept for the rebased cross rates.
const CROSS_DP: u32 = 10;

/// Parses the daily feed fetched at `fetched_at` (Unix seconds).
pub(crate) fn parse(body: &[u8], fetched_at: u64) -> anyhow::Result<RateTable> {
    let text = std::str::from_utf8(body).context("ECB: not UTF-8")?;
    let mut date = None;
    let mut per_eur: BTreeMap<String, Decimal> = BTreeMap::new();
    for tag in text.split('<').skip(1) {
        let Some(tag) = tag.strip_prefix("Cube") else {
            continue;
        };
        let tag = tag.split('>').next().unwrap_or_default();
        if let Some(time) = attribute(tag, "time") {
            date = Some(time.to_string());
        }
        if let (Some(code), Some(rate)) = (attribute(tag, "currency"), attribute(tag, "rate")) {
            let code = code.trim().to_ascii_uppercase();
            let Ok(rate) = Decimal::from_str(rate.trim()) else {
                continue;
            };
            if code.len() == 3 && rate > Decimal::ZERO {
                per_eur.insert(code, rate);
            }
        }
    }
    let date = date.context("ECB: no reference date")?;
    if per_eur.is_empty() {
        bail!("ECB: no rates");
    }
    per_eur.insert("EUR".to_string(), Decimal::ONE);

    // Rebase to USD when the feed has it: units of each currency per dollar.
    let (base, rates) = match per_eur.get("USD").copied() {
        Some(usd) => {
            let rates = per_eur
                .iter()
                .filter(|(code, _)| *code != "USD")
                .filter_map(|(code, rate)| {
                    let cross = rate.checked_div(usd)?.round_dp(CROSS_DP).normalize();
                    Some((code.clone(), cross))
                })
                .collect();
            ("USD".to_string(), rates)
        }
        None => {
            per_eur.remove("EUR");
            ("EUR".to_string(), per_eur)
        }
    };
    let dates = rates
        .keys()
        .map(|code: &String| (code.clone(), date.clone()))
        .collect();
    Ok(RateTable {
        base,
        rates,
        dates,
        source: Source::Ecb,
        fetched_at,
    })
}

/// The value of `name='…'` or `name="…"` in a tag's attributes.
fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let mut rest = tag;
    while let Some(at) = rest.find(name) {
        let before_ok = at == 0 || rest[..at].ends_with(char::is_whitespace);
        let after = rest[at + name.len()..].trim_start();
        if before_ok && let Some(after) = after.strip_prefix('=') {
            let after = after.trim_start();
            let quote = after.chars().next().filter(|c| *c == '\'' || *c == '"')?;
            let value = &after[1..];
            return value.find(quote).map(|end| &value[..end]);
        }
        rest = &rest[at + name.len()..];
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    #[test]
    fn parses_the_saved_daily_feed() {
        let t = parse(include_bytes!("testdata/eurofxref-daily.xml"), 7).unwrap();
        assert_eq!(t.base, "USD");
        assert_eq!(t.source, Source::Ecb);
        assert_eq!(t.fetched_at, 7);
        // 29 quotes against the euro, rebased: EUR joins, USD is the base.
        assert_eq!(t.rates.len(), 29);
        assert!(t.dates.values().all(|date| date == "2026-11-06"));
        // 1 USD = 1/1.1165 EUR.
        assert_eq!(t.rate("EUR"), Some(d("0.8956560681")));
        // Cross rates match the feed: 100 EUR = 176.77 * 100 JPY.
        let jpy = t.convert(d("100"), "EUR", "JPY").unwrap();
        assert_eq!(jpy.round_dp(4), d("17677"));
        let gbp_per_chf = t.convert(Decimal::ONE, "CHF", "GBP").unwrap();
        assert_eq!(
            gbp_per_chf.round_dp(6),
            (d("0.83415") / d("0.9143")).round_dp(6)
        );
    }

    #[test]
    fn double_quotes_and_no_dollar() {
        let body = br#"<Cube><Cube time="2026-11-05">
            <Cube currency="GBP" rate="0.84"/><Cube rate="160.5" currency="jpy"/>
            <Cube currency="BAD" rate="x"/></Cube></Cube>"#;
        let t = parse(body, 0).unwrap();
        // Without USD the table stays on the euro.
        assert_eq!(t.base, "EUR");
        assert_eq!(t.rate("JPY"), Some(d("160.5")));
        assert_eq!(t.rate("GBP"), Some(d("0.84")));
        assert_eq!(t.rate("BAD"), None);
        assert_eq!(t.dates["GBP"], "2026-11-05");
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse(b"", 0).is_err());
        assert!(parse(b"<Cube time='2026-11-05'></Cube>", 0).is_err());
        assert!(parse(b"<Cube currency='USD' rate='1.1'/>", 0).is_err());
        assert!(parse(&[0xff, 0xfe], 0).is_err());
        assert_eq!(attribute(" currency='USD' rate='1'", "rate"), Some("1"));
        assert_eq!(attribute(" xrate='2' rate='1'", "rate"), Some("1"));
    }
}
