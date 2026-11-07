//! Historical rates for one currency pair: the Frankfurter v2 time series,
//! calendar dates, chart ranges, the on-disk cache format and its gap
//! arithmetic, statistics and downsampling.
//!
//! Plain Rust without GPUI; the fetching lives in [`super::history_provider`].
//!
//! `GET /v2/rates?base=USD&quotes=EUR&from=2026-10-01&to=2026-11-07` returns
//! `[{"date":"2026-10-01","base":"USD","quote":"EUR","rate":0.88296}, ...]`,
//! one row per calendar day (the blended feed carries weekends too), and `[]`
//! for a span before the pair existed. Omitting `to` runs to the latest date.

use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, bail};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::provider::decimal_from_f64;

pub(crate) const BASE_URL: &str = "https://api.frankfurter.dev/v2/rates";

/// Before any currency Frankfurter knows (USD starts 1948-06-21). A MAX
/// request starts here and the response begins at the pair's earliest date.
pub(crate) const EARLIEST: Date = Date::from_ymd_const(1948, 1, 1);

/// About how many points the chart draws; longer spans are downsampled.
pub const CHART_POINTS: usize = 300;

/// A calendar date, stored as days since 1970-01-01.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date(i32);

impl Date {
    /// Days since the epoch for a proleptic Gregorian date (Howard Hinnant's
    /// `days_from_civil`).
    const fn from_ymd_const(y: i32, m: u32, d: u32) -> Self {
        let y = if m <= 2 { y - 1 } else { y };
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400;
        let mp = (m + 9) % 12;
        let doy = (153 * mp + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy as i32;
        Self(era * 146_097 + doe - 719_468)
    }

    pub(crate) fn from_ymd(y: i32, m: u32, d: u32) -> Option<Self> {
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let days_in_month = match m {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => return None,
        };
        (1..=days_in_month)
            .contains(&d)
            .then(|| Self::from_ymd_const(y, m, d))
    }

    /// `(year, month, day)` (Hinnant's `civil_from_days`).
    pub(crate) fn ymd(self) -> (i32, u32, u32) {
        let z = self.0 + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let y = yoe + era * 400 + i32::from(m <= 2);
        (y, m, d)
    }

    /// Parses `YYYY-MM-DD`.
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.splitn(3, '-');
        let y = parts.next()?.parse().ok()?;
        let m = parts.next()?.parse().ok()?;
        let d = parts.next()?.parse().ok()?;
        Self::from_ymd(y, m, d)
    }

    /// The UTC date at `secs` since the epoch.
    pub fn from_unix_secs(secs: u64) -> Self {
        Self((secs / 86_400) as i32)
    }

    /// Midnight UTC of this date, in seconds since the epoch.
    pub(crate) fn unix_secs(self) -> u64 {
        u64::try_from(i64::from(self.0) * 86_400).unwrap_or(0)
    }

    pub fn add_days(self, days: i32) -> Self {
        Self(self.0 + days)
    }

    /// `14 Oct` (`10月14日`), or `14 Oct 2019` (`2019年10月14日`) with the
    /// year.
    pub fn short(self, with_year: bool) -> String {
        let (y, m, d) = self.ymd();
        crate::i18n::short_date(y, m, d, with_year)
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (y, m, d) = self.ymd();
        write!(f, "{y:04}-{m:02}-{d:02}")
    }
}

impl Serialize for Date {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Date {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        Self::parse(&text).ok_or_else(|| serde::de::Error::custom(format!("bad date {text:?}")))
    }
}

/// The chart's time ranges (the segmented control).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Range {
    D7,
    #[default]
    D30,
    Y1,
    Y5,
    Max,
}

impl Range {
    pub const ALL: [Range; 5] = [Self::D7, Self::D30, Self::Y1, Self::Y5, Self::Max];

    /// `7D` … `MAX` (`7天` … `全部`).
    pub fn label(self) -> String {
        match self {
            Self::D7 => crate::i18n::tr!("range.d7"),
            Self::D30 => crate::i18n::tr!("range.d30"),
            Self::Y1 => crate::i18n::tr!("range.y1"),
            Self::Y5 => crate::i18n::tr!("range.y5"),
            Self::Max => crate::i18n::tr!("range.max"),
        }
    }

    /// The first day of the range ending `today`.
    pub fn start(self, today: Date) -> Date {
        match self {
            Self::D7 => today.add_days(-7),
            Self::D30 => today.add_days(-30),
            Self::Y1 => today.add_days(-365),
            Self::Y5 => today.add_days(-(5 * 365 + 1)),
            Self::Max => EARLIEST,
        }
    }

    /// Whether chart labels need the year.
    pub fn spans_years(self) -> bool {
        matches!(self, Self::Y1 | Self::Y5 | Self::Max)
    }
}

/// One day's rate: units of the quote per one unit of the base.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub date: Date,
    pub rate: Decimal,
}

#[derive(Deserialize)]
struct Row {
    date: String,
    base: String,
    quote: String,
    rate: f64,
}

/// The time-series URL. `to: None` runs to the latest available date.
pub fn url(base: &str, quote: &str, from: Date, to: Option<Date>) -> String {
    let mut url = format!("{BASE_URL}?base={base}&quotes={quote}&from={from}");
    if let Some(to) = to {
        url.push_str(&format!("&to={to}"));
    }
    url
}

/// Parses a time-series response for `base`/`quote`, sorted by date. An empty
/// array is valid (a span before the pair existed).
pub fn parse(body: &[u8], base: &str, quote: &str) -> anyhow::Result<Vec<Point>> {
    let rows: Vec<Row> = serde_json::from_slice(body).context("Frankfurter history: bad JSON")?;
    let mut points: Vec<Point> = rows
        .into_iter()
        .filter(|r| r.base.eq_ignore_ascii_case(base) && r.quote.eq_ignore_ascii_case(quote))
        .filter_map(|r| {
            Some(Point {
                date: Date::parse(&r.date)?,
                rate: decimal_from_f64(r.rate).filter(|r| r.is_sign_positive() && !r.is_zero())?,
            })
        })
        .collect();
    points.sort_by_key(|p| p.date);
    points.dedup_by_key(|p| p.date);
    Ok(points)
}

/// The cached daily series of one pair, `{cache_dir}/kurrency/history/USD-EUR.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct History {
    pub base: String,
    pub quote: String,
    /// Every day from here up to the last point has been fetched; earlier days
    /// were never asked for.
    pub(crate) covered_from: Date,
    /// When the tail was last fetched (Unix seconds).
    pub(crate) fetched_at: u64,
    pub(crate) points: Vec<Point>,
}

/// How long a fetched tail is considered current.
pub(crate) const TAIL_FRESH_SECS: u64 = 3600;

/// A span to fetch; `to: None` is "up to the latest".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gap {
    pub from: Date,
    pub to: Option<Date>,
}

impl History {
    pub fn empty(base: &str, quote: &str) -> Self {
        Self {
            base: base.to_string(),
            quote: quote.to_string(),
            covered_from: Date(i32::MAX),
            fetched_at: 0,
            points: Vec::new(),
        }
    }

    /// The spans to fetch so the series covers `from` to the latest day.
    ///
    /// Past days never change, so only the missing head (older than anything
    /// asked for so far) and the tail are fetched. The tail restarts at the
    /// last cached day, which may have been provisional, unless it was fetched
    /// within [`TAIL_FRESH_SECS`].
    pub fn gaps(&self, from: Date, now: u64) -> Vec<Gap> {
        let Some(last) = self.points.last().map(|p| p.date) else {
            return vec![Gap { from, to: None }];
        };
        let mut gaps = Vec::new();
        if from < self.covered_from {
            gaps.push(Gap {
                from,
                to: Some(self.covered_from.add_days(-1)),
            });
        }
        if now.saturating_sub(self.fetched_at) > TAIL_FRESH_SECS {
            gaps.push(Gap {
                from: last,
                to: None,
            });
        }
        gaps
    }

    /// Merges fetched points for `gap`, replacing what the cache had there.
    pub fn merge(&mut self, gap: Gap, fetched: Vec<Point>, now: u64) {
        self.points
            .retain(|p| p.date < gap.from || gap.to.is_some_and(|to| p.date > to));
        self.points.extend(fetched);
        self.points.sort_by_key(|p| p.date);
        self.points.dedup_by_key(|p| p.date);
        self.covered_from = self.covered_from.min(gap.from);
        if gap.to.is_none() {
            self.fetched_at = now;
        }
    }

    /// The points on or after `from`.
    pub fn since(&self, from: Date) -> &[Point] {
        let start = self.points.partition_point(|p| p.date < from);
        &self.points[start..]
    }
}

pub fn cache_dir() -> Option<PathBuf> {
    crate::paths::cache_dir().map(|dir| dir.join("history"))
}

pub fn cache_path(dir: &Path, base: &str, quote: &str) -> PathBuf {
    dir.join(format!("{base}-{quote}.json"))
}

/// Reads a cached series; a missing, unreadable or mismatched one is `None`.
pub fn load(path: &Path, base: &str, quote: &str) -> Option<History> {
    let bytes = std::fs::read(path).ok()?;
    match serde_json::from_slice::<History>(&bytes) {
        Ok(h) if h.base == base && h.quote == quote => Some(h),
        Ok(_) => None,
        Err(err) => {
            log::warn!("ignoring unreadable history {}: {err}", path.display());
            None
        }
    }
}

pub fn save(path: &Path, history: &History) -> anyhow::Result<()> {
    if history.base.is_empty() || history.quote.is_empty() {
        bail!("history without a pair");
    }
    let json = serde_json::to_vec(history)?;
    super::cache::write_atomic(path, &json)
}

/// The figures under the hero and in the stats table.
#[derive(Debug, Clone, PartialEq)]
pub struct Stats {
    pub(crate) first: Decimal,
    pub last: Decimal,
    pub(crate) last_date: Date,
    /// `last - first`.
    pub change: Decimal,
    /// The change in percent of `first`, rounded to 2 places.
    pub change_pct: Decimal,
    pub high: Point,
    pub low: Point,
    pub average: Decimal,
}

/// Statistics over `points` (in date order); `None` when there are none.
pub fn stats(points: &[Point]) -> Option<Stats> {
    let first = points.first()?;
    let last = points.last()?;
    let mut high = first;
    let mut low = first;
    let mut sum = Decimal::ZERO;
    for p in points {
        // Ties go to the latest day.
        if p.rate >= high.rate {
            high = p;
        }
        if p.rate <= low.rate {
            low = p;
        }
        sum += p.rate;
    }
    let change = last.rate - first.rate;
    let change_pct = (change * Decimal::ONE_HUNDRED / first.rate).round_dp(2);
    Some(Stats {
        first: first.rate,
        last: last.rate,
        last_date: last.date,
        change,
        change_pct,
        high: high.clone(),
        low: low.clone(),
        average: sum / Decimal::from(points.len()),
    })
}

/// Reduces `points` to at most `max` with largest-triangle-three-buckets,
/// which keeps the visual peaks and troughs; the first and last stay.
pub fn downsample(points: &[Point], max: usize) -> Vec<Point> {
    let n = points.len();
    if n <= max || max < 3 {
        return points.to_vec();
    }
    let y = |p: &Point| -> f64 { p.rate.try_into().unwrap_or(0.) };
    let bucket = (n - 2) as f64 / (max - 2) as f64;
    let mut out = Vec::with_capacity(max);
    out.push(points[0].clone());
    let mut a = 0usize;
    for i in 0..max - 2 {
        let start = (i as f64 * bucket) as usize + 1;
        let end = (((i + 1) as f64 * bucket) as usize + 1).min(n - 1);
        // The average of the next bucket (or the last point).
        let next_start = end;
        let next_end = (((i + 2) as f64 * bucket) as usize + 1).min(n);
        let next = &points[next_start..next_end.max(next_start + 1).min(n)];
        let avg_x = next_start as f64 + (next.len() as f64 - 1.) / 2.;
        let avg_y = next.iter().map(y).sum::<f64>() / next.len() as f64;
        let (ax, ay) = (a as f64, y(&points[a]));
        let mut best = start;
        let mut best_area = -1.;
        for (j, p) in points.iter().enumerate().take(end).skip(start) {
            let area = ((ax - avg_x) * (y(p) - ay) - (ax - j as f64) * (avg_y - ay)).abs();
            if area > best_area {
                best_area = area;
                best = j;
            }
        }
        out.push(points[best].clone());
        a = best;
    }
    out.push(points[n - 1].clone());
    out
}

/// Decimal places for showing a rate: about 5 significant digits, never fewer
/// than 2 (`158.32`, `7.1234`, `0.8956`, `0.0076554`).
pub fn rate_decimals(rate: Decimal) -> u32 {
    let abs = rate.abs();
    if abs >= Decimal::from(100) {
        2
    } else if abs >= Decimal::TEN {
        3
    } else if abs >= Decimal::new(1, 2) {
        4
    } else {
        // Leading zeros after the point, plus 4 significant digits.
        let mut dp = 4;
        let mut v = abs;
        while v < Decimal::new(1, 1) && !v.is_zero() && dp < 12 {
            v *= Decimal::TEN;
            dp += 1;
        }
        dp
    }
}

/// A rate rounded for display, without grouping (`0.8956`).
pub(crate) fn format_rate(rate: Decimal, dp: u32) -> String {
    let mut r = rate.round_dp(dp);
    r.rescale(dp);
    r.to_string()
}

/// [`format_rate`] with another decimal separator (`0,8956`).
pub fn format_rate_in(rate: Decimal, dp: u32, decimal: char) -> String {
    let text = format_rate(rate, dp);
    if decimal == '.' {
        text
    } else {
        text.replace('.', &decimal.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn date(s: &str) -> Date {
        Date::parse(s).unwrap()
    }

    fn pt(day: &str, rate: &str) -> Point {
        Point {
            date: date(day),
            rate: d(rate),
        }
    }

    #[test]
    fn dates_round_trip() {
        assert_eq!(date("1970-01-01"), Date(0));
        assert_eq!(date("2026-11-07").to_string(), "2026-11-07");
        assert_eq!(date("2024-02-29").add_days(1).to_string(), "2024-03-01");
        assert_eq!(date("2026-01-01").add_days(-1).to_string(), "2025-12-31");
        assert_eq!(Date::parse("2025-02-29"), None);
        assert_eq!(Date::parse("2025-13-01"), None);
        assert_eq!(Date::parse("garbage"), None);
        assert_eq!(
            Date::from_unix_secs(1_794_009_600).to_string(),
            "2026-11-07"
        );
        assert_eq!(date("2026-10-14").short(false), "14 Oct");
        assert_eq!(date("2019-03-02").short(true), "2 Mar 2019");
        crate::i18n::with_lang(crate::i18n::Lang::ZhCn, || {
            assert_eq!(date("2026-11-07").short(false), "11月7日");
            assert_eq!(date("2019-03-02").short(true), "2019年3月2日");
            let labels: Vec<String> = Range::ALL.iter().map(|r| r.label()).collect();
            assert_eq!(labels, ["7天", "30天", "1年", "5年", "全部"]);
        });
        let labels: Vec<String> = Range::ALL.iter().map(|r| r.label()).collect();
        assert_eq!(labels, ["7D", "30D", "1Y", "5Y", "MAX"]);
        for days in -30_000..30_000 {
            let d = Date(days);
            assert_eq!(Date::parse(&d.to_string()), Some(d));
        }
    }

    #[test]
    fn ranges() {
        let today = date("2026-11-07");
        assert_eq!(Range::D7.start(today).to_string(), "2026-10-31");
        assert_eq!(Range::D30.start(today).to_string(), "2026-10-08");
        assert_eq!(Range::Y1.start(today).to_string(), "2025-11-07");
        assert_eq!(Range::Y5.start(today).to_string(), "2021-11-07");
        assert_eq!(Range::Max.start(today), EARLIEST);
    }

    #[test]
    fn parses_a_time_series() {
        let body = br#"[
            {"date":"2026-10-02","base":"USD","quote":"EUR","rate":0.88597},
            {"date":"2026-10-01","base":"USD","quote":"EUR","rate":0.88296},
            {"date":"2026-10-03","base":"USD","quote":"JPY","rate":150.1},
            {"date":"bad","base":"USD","quote":"EUR","rate":0.9},
            {"date":"2026-10-04","base":"USD","quote":"EUR","rate":0}
        ]"#;
        let points = parse(body, "USD", "EUR").unwrap();
        assert_eq!(
            points,
            [pt("2026-10-01", "0.88296"), pt("2026-10-02", "0.88597")]
        );
        assert!(parse(b"[]", "USD", "EUR").unwrap().is_empty());
        assert!(
            parse(
                br#"{"status":422,"message":"invalid currency"}"#,
                "USD",
                "X"
            )
            .is_err()
        );
        assert_eq!(
            url("USD", "EUR", date("2026-10-01"), Some(date("2026-11-07"))),
            "https://api.frankfurter.dev/v2/rates?base=USD&quotes=EUR&from=2026-10-01&to=2026-11-07"
        );
        assert_eq!(
            url("USD", "EUR", date("2026-10-01"), None),
            "https://api.frankfurter.dev/v2/rates?base=USD&quotes=EUR&from=2026-10-01"
        );
    }

    #[test]
    fn gaps_fetch_only_the_head_and_tail() {
        let now = 1_000_000;
        let mut h = History::empty("USD", "EUR");
        let from = date("2026-10-08");
        assert_eq!(h.gaps(from, now), [Gap { from, to: None }]);
        h.merge(
            Gap { from, to: None },
            vec![pt("2026-10-08", "0.89"), pt("2026-11-06", "0.90")],
            now,
        );
        // Fresh: nothing to fetch for the same range.
        assert!(h.gaps(from, now + 10).is_empty());
        // Stale tail: restart at the last (maybe provisional) day.
        let later = now + TAIL_FRESH_SECS + 1;
        assert_eq!(
            h.gaps(from, later),
            [Gap {
                from: date("2026-11-06"),
                to: None
            }]
        );
        // A longer range adds the head.
        let y1 = date("2025-11-07");
        assert_eq!(
            h.gaps(y1, now),
            [Gap {
                from: y1,
                to: Some(date("2026-10-07"))
            }]
        );
        // The tail fetch replaces the provisional day and appends.
        h.merge(
            Gap {
                from: date("2026-11-06"),
                to: None,
            },
            vec![pt("2026-11-06", "0.91"), pt("2026-11-07", "0.92")],
            later,
        );
        h.merge(
            Gap {
                from: y1,
                to: Some(date("2026-10-07")),
            },
            vec![pt("2025-11-07", "0.85")],
            later,
        );
        assert_eq!(h.covered_from, y1);
        assert_eq!(h.fetched_at, later);
        let rates: Vec<_> = h.points.iter().map(|p| p.rate.to_string()).collect();
        assert_eq!(rates, ["0.85", "0.89", "0.91", "0.92"]);
        assert_eq!(h.since(date("2026-11-01")).len(), 2);
        assert_eq!(h.since(date("2027-01-01")).len(), 0);
    }

    #[test]
    fn cache_round_trips() {
        let dir = std::env::temp_dir().join(format!("kurrency-history-{}", std::process::id()));
        let path = cache_path(&dir, "USD", "EUR");
        assert_eq!(load(&path, "USD", "EUR"), None);
        let mut h = History::empty("USD", "EUR");
        h.merge(
            Gap {
                from: date("2026-10-01"),
                to: None,
            },
            vec![pt("2026-10-01", "0.88296")],
            5,
        );
        save(&path, &h).unwrap();
        assert_eq!(load(&path, "USD", "EUR"), Some(h));
        assert_eq!(load(&path, "EUR", "USD"), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn statistics() {
        let points = [
            pt("2026-10-08", "0.9065"),
            pt("2026-10-12", "0.9065"),
            pt("2026-10-20", "0.8990"),
            pt("2026-11-02", "0.8931"),
            pt("2026-11-07", "0.8956"),
        ];
        let s = stats(&points).unwrap();
        assert_eq!(s.change, d("-0.0109"));
        assert_eq!(s.change_pct, d("-1.20"));
        assert_eq!(s.high.date, date("2026-10-12"));
        assert_eq!(s.low, pt("2026-11-02", "0.8931"));
        assert_eq!(s.average.round_dp(4), d("0.9001"));
        assert_eq!(s.last_date, date("2026-11-07"));
        assert_eq!(stats(&[]), None);
    }

    #[test]
    fn downsampling_keeps_the_ends_and_extremes() {
        let start = date("2000-01-01");
        let points: Vec<Point> = (0..5000)
            .map(|i| Point {
                date: start.add_days(i),
                rate: if i == 1234 {
                    d("9")
                } else {
                    Decimal::from(1 + i % 7) / Decimal::TEN
                },
            })
            .collect();
        let out = downsample(&points, 300);
        assert_eq!(out.len(), 300);
        assert_eq!(out.first(), points.first());
        assert_eq!(out.last(), points.last());
        assert!(out.iter().any(|p| p.rate == d("9")));
        assert!(out.windows(2).all(|w| w[0].date < w[1].date));
        assert_eq!(downsample(&points[..10], 300).len(), 10);
    }

    #[test]
    fn rate_display() {
        assert_eq!(rate_decimals(d("158.32")), 2);
        assert_eq!(rate_decimals(d("12.5")), 3);
        assert_eq!(rate_decimals(d("0.8956")), 4);
        assert_eq!(rate_decimals(d("0.0076554")), 6);
        assert_eq!(rate_decimals(d("0.00006")), 8);
        assert_eq!(format_rate(d("0.89564"), 4), "0.8956");
        assert_eq!(format_rate(d("0.9"), 4), "0.9000");
        assert_eq!(format_rate_in(d("0.89564"), 4, ','), "0,8956");
        assert_eq!(format_rate_in(d("158"), 2, '.'), "158.00");
    }
}
