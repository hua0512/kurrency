//! The provider-independent rate table and the provider chain.
//!
//! No GPUI dependency: parsing and lookups are unit-tested here and in the
//! provider modules.

use std::collections::BTreeMap;
use std::str::FromStr;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::{ecb, fawaz, frankfurter};
use crate::settings::RateSource;

/// Where a rate table came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// Frankfurter's blend of central banks.
    Frankfurter,
    /// Frankfurter's ECB feed (`providers=ECB`).
    FrankfurterEcb,
    Fawaz,
    /// The ECB's own XML feed.
    Ecb,
    /// The snapshot compiled into the binary.
    Bundled,
}

impl Source {
    /// The short label shown in the freshness tooltip and Settings, in the
    /// UI language (`ECB`, `欧洲央行`).
    pub fn display_name(self) -> String {
        match self {
            Self::Frankfurter => crate::i18n::tr!("source.frankfurter"),
            Self::FrankfurterEcb => crate::i18n::tr!("source.frankfurter_ecb"),
            Self::Fawaz => crate::i18n::tr!("source.fawaz"),
            Self::Ecb => crate::i18n::tr!("source.ecb"),
            Self::Bundled => crate::i18n::tr!("source.bundled"),
        }
    }

    /// Whether a table from this source can stand for the `chosen` setting;
    /// otherwise switching the setting refreshes at once. The bundled
    /// snapshot stands for anything until the first fetch.
    pub fn serves(self, chosen: RateSource) -> bool {
        match chosen {
            RateSource::Auto => true,
            RateSource::Ecb => matches!(self, Self::FrankfurterEcb | Self::Ecb | Self::Bundled),
            RateSource::Frankfurter => matches!(self, Self::Frankfurter | Self::Bundled),
        }
    }
}

/// Exchange rates against one base currency: `rates[code]` units of `code`
/// per one unit of `base`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RateTable {
    pub base: String,
    pub rates: BTreeMap<String, Decimal>,
    /// The publication date (`YYYY-MM-DD`) of each quote; quotes from blended
    /// sources can carry different dates.
    pub dates: BTreeMap<String, String>,
    pub source: Source,
    /// When this table was fetched, in seconds since the Unix epoch. Zero for
    /// the bundled snapshot.
    pub fetched_at: u64,
}

impl RateTable {
    /// Units of `code` per one base unit; the base itself is 1.
    pub fn rate(&self, code: &str) -> Option<Decimal> {
        if code == self.base {
            Some(Decimal::ONE)
        } else {
            self.rates.get(code).copied().filter(|r| !r.is_zero())
        }
    }

    /// Converts `amount` from one currency to another through the base.
    pub fn convert(&self, amount: Decimal, from: &str, to: &str) -> Option<Decimal> {
        crate::money::convert(amount, self.rate(from)?, self.rate(to)?)
    }

    /// The oldest quote date among `codes` (the base is ignored), falling back
    /// to the oldest date in the table. Showing the oldest keeps the footer
    /// honest when one displayed quote lags behind the others.
    pub fn as_of<'a>(&self, codes: impl IntoIterator<Item = &'a str>) -> Option<&str> {
        codes
            .into_iter()
            .filter_map(|code| self.dates.get(code))
            .min()
            .or_else(|| self.dates.values().min())
            .map(String::as_str)
    }

    /// The date most quotes carry (the newest among ties): the table's day.
    pub fn main_date(&self) -> Option<&str> {
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for date in self.dates.values() {
            *counts.entry(date.as_str()).or_default() += 1;
        }
        // Iterating in date order and keeping `>=` lets the newest tie win.
        let mut best: Option<(&str, usize)> = None;
        for (date, count) in counts {
            if best.is_none_or(|(_, n)| count >= n) {
                best = Some((date, count));
            }
        }
        best.map(|(date, _)| date)
    }

    /// Every code with a rate, including the base, sorted.
    pub fn codes(&self) -> Vec<String> {
        let mut codes: Vec<String> = self.rates.keys().cloned().collect();
        if !codes.contains(&self.base) {
            codes.push(self.base.clone());
            codes.sort();
        }
        codes
    }

    /// Whether the table is older than `max_age_secs` at `now`.
    pub fn is_older_than(&self, max_age_secs: u64, now: u64) -> bool {
        now.saturating_sub(self.fetched_at) > max_age_secs
    }
}

/// Reads a JSON number into an exact decimal.
///
/// The JSON parser hands numbers over as `f64`; its `Display` is the shortest
/// text that round-trips, which for published rates (at most ~10 significant
/// digits) is exactly the literal in the response.
pub(crate) fn decimal_from_f64(value: f64) -> Option<Decimal> {
    if !value.is_finite() {
        return None;
    }
    let text = value.to_string();
    Decimal::from_str(&text)
        .or_else(|_| Decimal::from_scientific(&text))
        .ok()
}

/// One step of the fallback chain: the URLs to try in order and their parser.
pub struct Provider {
    pub source: Source,
    pub urls: &'static [&'static str],
    pub parse: fn(&[u8], u64) -> anyhow::Result<RateTable>,
}

const FRANKFURTER: Provider = Provider {
    source: Source::Frankfurter,
    urls: &[frankfurter::URL],
    parse: frankfurter::parse,
};

const FRANKFURTER_ECB: Provider = Provider {
    source: Source::FrankfurterEcb,
    urls: &[frankfurter::ECB_URL],
    parse: frankfurter::parse_ecb,
};

const FAWAZ: Provider = Provider {
    source: Source::Fawaz,
    urls: fawaz::URLS,
    parse: fawaz::parse,
};

const ECB: Provider = Provider {
    source: Source::Ecb,
    urls: &[ecb::URL],
    parse: ecb::parse,
};

/// The providers for a Settings → Source choice, in fallback order.
pub fn chain(source: RateSource) -> &'static [Provider] {
    match source {
        RateSource::Auto => &[FRANKFURTER, FAWAZ, ECB],
        RateSource::Ecb => &[FRANKFURTER_ECB, ECB],
        RateSource::Frankfurter => &[FRANKFURTER],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn table() -> RateTable {
        RateTable {
            base: "USD".into(),
            rates: [("EUR", "0.8956"), ("JPY", "158.32"), ("BAD", "0")]
                .into_iter()
                .map(|(c, r)| (c.to_string(), d(r)))
                .collect(),
            dates: [("EUR", "2026-11-06"), ("JPY", "2026-11-07")]
                .into_iter()
                .map(|(c, r)| (c.to_string(), r.to_string()))
                .collect(),
            source: Source::Frankfurter,
            fetched_at: 1_000,
        }
    }

    #[test]
    fn lookups() {
        let t = table();
        assert_eq!(t.rate("USD"), Some(Decimal::ONE));
        assert_eq!(t.rate("EUR"), Some(d("0.8956")));
        assert_eq!(t.rate("BAD"), None);
        assert_eq!(t.rate("XYZ"), None);
        assert_eq!(t.convert(d("100"), "USD", "EUR"), Some(d("89.56")));
        assert_eq!(t.convert(d("1"), "USD", "XYZ"), None);
    }

    #[test]
    fn as_of_uses_oldest_displayed_date() {
        let t = table();
        assert_eq!(t.as_of(["USD", "JPY"]), Some("2026-11-07"));
        assert_eq!(t.as_of(["EUR", "JPY"]), Some("2026-11-06"));
        assert_eq!(t.as_of(["USD"]), Some("2026-11-06"));
    }

    #[test]
    fn main_date_is_the_most_common() {
        let mut t = table();
        assert_eq!(t.main_date(), Some("2026-11-07"));
        t.dates.insert("GBP".into(), "2026-11-06".into());
        assert_eq!(t.main_date(), Some("2026-11-06"));
        t.dates.clear();
        assert_eq!(t.main_date(), None);
    }

    #[test]
    fn staleness() {
        let t = table();
        assert!(!t.is_older_than(6 * 3600, 1_000 + 6 * 3600));
        assert!(t.is_older_than(6 * 3600, 1_001 + 6 * 3600));
    }

    #[test]
    fn f64_to_decimal_is_the_literal() {
        assert_eq!(decimal_from_f64(0.0076554), Some(d("0.0076554")));
        assert_eq!(decimal_from_f64(158.32), Some(d("158.32")));
        assert_eq!(decimal_from_f64(1544.86606302), Some(d("1544.86606302")));
        assert_eq!(decimal_from_f64(f64::NAN), None);
    }

    /// Which tables count as fresh for a Settings → Source choice (the chain
    /// order itself is tested through a refresh in `store`).
    #[test]
    fn sources_serve_their_settings() {
        assert!(Source::Fawaz.serves(RateSource::Auto));
        assert!(!Source::Fawaz.serves(RateSource::Ecb));
        assert!(Source::Ecb.serves(RateSource::Ecb));
        assert!(!Source::FrankfurterEcb.serves(RateSource::Frankfurter));
        assert!(Source::FrankfurterEcb.serves(RateSource::Auto));
        assert!(Source::Bundled.serves(RateSource::Frankfurter));
    }
}
