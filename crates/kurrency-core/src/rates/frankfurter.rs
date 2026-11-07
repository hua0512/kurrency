//! Frankfurter v2: `GET /v2/rates?base=USD` returns one object per quote:
//! `[{"date":"2026-11-06","base":"USD","quote":"EUR","rate":0.8956}, ...]`.

use anyhow::{Context as _, bail};
use serde::Deserialize;

use super::provider::{RateTable, Source, decimal_from_f64};

pub const URL: &str = "https://api.frankfurter.dev/v2/rates?base=USD";
/// The ECB's reference rates only (`providers=ECB`, checked against the live
/// API: 30 quotes instead of the blend's ~200).
pub(crate) const ECB_URL: &str = "https://api.frankfurter.dev/v2/rates?base=USD&providers=ECB";

#[derive(Deserialize)]
struct Quote {
    date: String,
    base: String,
    quote: String,
    rate: f64,
}

/// Parses a `providers=ECB` response.
pub(crate) fn parse_ecb(body: &[u8], fetched_at: u64) -> anyhow::Result<RateTable> {
    let mut table = parse(body, fetched_at)?;
    table.source = Source::FrankfurterEcb;
    Ok(table)
}

/// Parses a v2 rates response fetched at `fetched_at` (Unix seconds).
pub fn parse(body: &[u8], fetched_at: u64) -> anyhow::Result<RateTable> {
    let quotes: Vec<Quote> = serde_json::from_slice(body).context("Frankfurter: bad JSON")?;
    let Some(base) = quotes.first().map(|q| q.base.to_ascii_uppercase()) else {
        bail!("Frankfurter: empty response");
    };
    let mut table = RateTable {
        base,
        rates: Default::default(),
        dates: Default::default(),
        source: Source::Frankfurter,
        fetched_at,
    };
    for quote in quotes {
        if !quote.base.eq_ignore_ascii_case(&table.base) {
            continue;
        }
        let Some(rate) =
            decimal_from_f64(quote.rate).filter(rust_decimal::Decimal::is_sign_positive)
        else {
            continue;
        };
        let code = quote.quote.to_ascii_uppercase();
        table.dates.insert(code.clone(), quote.date);
        table.rates.insert(code, rate);
    }
    if table.rates.is_empty() {
        bail!("Frankfurter: no usable quotes");
    }
    Ok(table)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use rust_decimal::Decimal;

    use super::*;

    #[test]
    fn parses_quotes_with_their_own_dates() {
        let body = br#"[
            {"date":"2026-11-06","base":"USD","quote":"EUR","rate":0.8956},
            {"date":"2026-11-08","base":"USD","quote":"AED","rate":3.6725},
            {"date":"2026-11-06","base":"USD","quote":"JPY","rate":158.32}
        ]"#;
        let t = parse(body, 42).unwrap();
        assert_eq!(t.base, "USD");
        assert_eq!(t.source, Source::Frankfurter);
        assert_eq!(t.fetched_at, 42);
        assert_eq!(t.rate("EUR"), Some(Decimal::from_str("0.8956").unwrap()));
        assert_eq!(t.dates["AED"], "2026-11-08");
        assert_eq!(t.rates.len(), 3);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse(b"[]", 0).is_err());
        assert!(parse(b"{}", 0).is_err());
        assert!(parse(b"not json", 0).is_err());
    }
}
