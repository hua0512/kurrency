//! The previous trading day's rates, for the daily change chips (DESIGN §1, §3).
//!
//! One extra Frankfurter request per refresh: `GET /v2/rates?base=USD&date=…`
//! for the FX trading day before the table's day, in the same format as
//! today's table. It is cached next to the rate cache and refetched whenever
//! today's table moves to a new trading session. Plain Rust without GPUI.

use std::path::{Path, PathBuf};

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::history::{self, Date};
use super::provider::{RateTable, Source};

/// The previous trading day's table and the day it was requested for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Previous {
    /// `YYYY-MM-DD`: the previous trading day for today's table's main date.
    pub date: String,
    pub table: RateTable,
}

impl Previous {
    /// Whether this is the previous trading day of `today` (same base, right day).
    pub fn matches(&self, today: &RateTable) -> bool {
        wanted_date(today).is_some_and(|d| d.to_string() == self.date)
            && self.table.base == today.base
            && self.table.source == today.source
    }

    /// Units of `code` per one `anchor`, on the previous trading day.
    pub fn cross_rate(&self, anchor: &str, code: &str) -> Option<Decimal> {
        self.table.convert(Decimal::ONE, anchor, code)
    }
}

/// The day to fetch for `today`: the FX trading day before its main date
/// (stepping back across closed weekends). Only Frankfurter tables (blended or
/// ECB) get one, so the two days come from the same feed.
pub fn wanted_date(today: &RateTable) -> Option<Date> {
    if !matches!(today.source, Source::Frankfurter | Source::FrankfurterEcb) {
        return None;
    }
    Date::parse(today.main_date()?).map(Date::previous_trading_day)
}

/// `https://api.frankfurter.dev/v2/rates?base=USD&date=2026-11-06`, with
/// `&providers=ECB` for the ECB feed.
pub fn url(base: &str, date: Date, source: Source) -> String {
    let providers = if source == Source::FrankfurterEcb {
        "&providers=ECB"
    } else {
        ""
    };
    format!("{}?base={base}&date={date}{providers}", history::BASE_URL)
}

pub fn default_path() -> Option<PathBuf> {
    crate::paths::cache_dir().map(|dir| dir.join("rates-previous.json"))
}

pub fn load(path: &Path) -> Option<Previous> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn save(path: &Path, previous: &Previous) -> anyhow::Result<()> {
    super::cache::write_atomic(path, &serde_json::to_vec(previous)?)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn table(date: &str, eur: &str, source: Source) -> RateTable {
        RateTable {
            base: "USD".into(),
            rates: [("EUR".to_string(), Decimal::from_str(eur).unwrap())].into(),
            dates: [("EUR".to_string(), date.to_string())].into(),
            source,
            fetched_at: 0,
        }
    }

    #[test]
    fn previous_day_of_the_table() {
        // Friday 2026-11-06 compares against Thursday 2026-11-05.
        let today = table("2026-11-06", "0.9", Source::Frankfurter);
        let date = wanted_date(&today).unwrap();
        assert_eq!(date.to_string(), "2026-11-05");
        assert_eq!(
            url("USD", date, Source::Frankfurter),
            "https://api.frankfurter.dev/v2/rates?base=USD&date=2026-11-05"
        );
        assert_eq!(
            url("USD", date, Source::FrankfurterEcb),
            "https://api.frankfurter.dev/v2/rates?base=USD&date=2026-11-05&providers=ECB"
        );
        let ecb_day = table("2026-11-06", "0.9", Source::FrankfurterEcb);
        assert_eq!(wanted_date(&ecb_day), Some(date));
        // A blended previous day does not stand for the ECB feed's.
        let blended = Previous {
            date: "2026-11-05".into(),
            table: table("2026-11-05", "0.8", Source::Frankfurter),
        };
        assert!(!blended.matches(&ecb_day));
        assert_eq!(
            wanted_date(&table("2026-11-06", "0.9", Source::Fawaz)),
            None
        );
        assert_eq!(
            wanted_date(&table("2026-11-06", "0.9", Source::Bundled)),
            None
        );

        let prev = Previous {
            date: "2026-11-05".into(),
            table: table("2026-11-05", "0.8", Source::Frankfurter),
        };
        assert!(prev.matches(&today));
        // Saturday and Sunday still carry Friday's session (against Thursday);
        // Monday moves to comparing against Friday.
        assert!(prev.matches(&table("2026-11-07", "0.9", Source::Frankfurter)));
        assert!(prev.matches(&table("2026-11-08", "0.9", Source::Frankfurter)));
        let monday = table("2026-11-09", "0.9", Source::Frankfurter);
        assert!(!prev.matches(&monday));
        assert_eq!(
            wanted_date(&monday).map(|d| d.to_string()),
            Some("2026-11-06".into())
        );
        assert_eq!(prev.cross_rate("USD", "EUR"), Decimal::from_str("0.8").ok());
        assert_eq!(
            prev.cross_rate("EUR", "USD"),
            Decimal::from_str("1.25").ok()
        );
        assert_eq!(prev.cross_rate("USD", "JPY"), None);
    }
}
