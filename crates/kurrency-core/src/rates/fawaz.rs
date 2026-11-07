//! fawazahmed0 currency-api: `{"date":"2026-11-07","usd":{"eur":0.86,...}}`
//! with lowercase codes and one date for the whole table.

use anyhow::{Context as _, bail};
use serde_json::Value;

use super::provider::{RateTable, Source, decimal_from_f64};

/// The primary host, then the jsDelivr mirror the project documents.
pub const URLS: &[&str] = &[
    "https://latest.currency-api.pages.dev/v1/currencies/usd.min.json",
    "https://cdn.jsdelivr.net/npm/@fawazahmed0/currency-api@latest/v1/currencies/usd.min.json",
];

/// Parses a response fetched at `fetched_at` (Unix seconds).
pub(crate) fn parse(body: &[u8], fetched_at: u64) -> anyhow::Result<RateTable> {
    let root: Value = serde_json::from_slice(body).context("fawazahmed0: bad JSON")?;
    let Some(object) = root.as_object() else {
        bail!("fawazahmed0: expected an object");
    };
    let date = object
        .get("date")
        .and_then(Value::as_str)
        .context("fawazahmed0: missing date")?
        .to_string();
    let Some((base, rates)) = object
        .iter()
        .find_map(|(key, value)| (key != "date").then(|| value.as_object().map(|r| (key, r)))?)
    else {
        bail!("fawazahmed0: missing rates");
    };
    let base = base.to_ascii_uppercase();
    let mut table = RateTable {
        base: base.clone(),
        rates: Default::default(),
        dates: Default::default(),
        source: Source::Fawaz,
        fetched_at,
    };
    for (code, rate) in rates {
        let code = code.to_ascii_uppercase();
        if code == base {
            continue;
        }
        let Some(rate) = rate
            .as_f64()
            .and_then(decimal_from_f64)
            .filter(|r| r.is_sign_positive() && !r.is_zero())
        else {
            continue;
        };
        table.dates.insert(code.clone(), date.clone());
        table.rates.insert(code, rate);
    }
    if table.rates.is_empty() {
        bail!("fawazahmed0: no usable quotes");
    }
    Ok(table)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use rust_decimal::Decimal;

    use super::*;

    #[test]
    fn parses_lowercase_codes() {
        let body = br#"{"date":"2026-11-07","usd":{"eur":0.86,"jpy":154.5,"usd":1,"1inch":12.7}}"#;
        let t = parse(body, 7).unwrap();
        assert_eq!(t.base, "USD");
        assert_eq!(t.source, Source::Fawaz);
        assert_eq!(t.rate("EUR"), Some(Decimal::from_str("0.86").unwrap()));
        assert_eq!(t.rate("JPY"), Some(Decimal::from_str("154.5").unwrap()));
        assert_eq!(t.dates["JPY"], "2026-11-07");
        assert!(!t.rates.contains_key("USD"));
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse(b"{}", 0).is_err());
        assert!(parse(br#"{"date":"2026-11-07"}"#, 0).is_err());
        assert!(parse(br#"[1,2]"#, 0).is_err());
    }
}
