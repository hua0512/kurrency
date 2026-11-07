//! Exchange rates: the tables, their providers' formats, the on-disk caches,
//! the previous day and the history maths. Fetching them over HTTP and the
//! store entity that refreshes them are the app's (`kurrency::rates`).

pub mod cache;
pub(crate) mod ecb;
pub mod fawaz;
pub mod frankfurter;
pub mod history;
pub mod previous;
pub mod provider;

pub use provider::{RateTable, Source};

use crate::i18n::tr;

/// How old fetched rates may get before a background refresh, by default
/// (Settings → Refresh: 1h / 6h / 12h / Manual).
pub const REFRESH_AFTER_SECS: u64 = 6 * 3600;

/// Rates older than this show the amber (stale) freshness dot.
pub(crate) const STALE_AFTER_SECS: u64 = 12 * 3600;

/// How long to wait before retrying after a failed refresh.
pub const RETRY_AFTER_SECS: u64 = 15 * 60;

/// The bundled snapshot (Frankfurter v2 USD rates taken at build time).
pub(crate) const BUNDLED_SNAPSHOT: &str = include_str!("../../../assets/rates-snapshot.json");

/// The bundled table: the last resort when there is no cache and no network.
pub fn bundled() -> RateTable {
    let mut table = frankfurter::parse(BUNDLED_SNAPSHOT.as_bytes(), 0)
        .expect("the bundled rates snapshot must parse");
    table.source = Source::Bundled;
    table
}

/// The outcome of the last refresh attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshState {
    Idle,
    Refreshing,
    /// Every provider failed. `offline` is true when none of them answered at
    /// all (as opposed to answering with an error or unreadable data).
    Failed {
        offline: bool,
        message: String,
    },
}

/// The colour of the header's freshness dot (DESIGN §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dot {
    /// Slate: fetched within [`STALE_AFTER_SECS`].
    Fresh,
    /// Amber: older, the bundled snapshot, or the last refresh failed.
    Stale,
    /// Hollow: the last refresh reached no provider at all.
    Offline,
}

/// What the header shows about the rates: the dot, the relative age, and the
/// lines of its tooltip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Freshness {
    pub dot: Dot,
    /// A refresh is in flight: the dot is a hollow ring (after one short pulse).
    pub updating: bool,
    /// `now`, `5m`, `2h`, `3d`.
    pub age: String,
    /// "Frankfurter · rates of 6 November 2026"
    pub source_line: String,
    /// "Updated 2h ago · next refresh in 4h", or the failure.
    pub status_line: String,
    /// The full error of a failed refresh, shown on click.
    pub error: Option<String>,
}

/// `now` under a minute, then `5m`, `2h`, `3d` (`刚刚`, `5分钟`, `2小时`,
/// `3天`).
pub(crate) fn relative_age(secs: u64) -> String {
    match secs {
        0..60 => tr!("age.now"),
        60..3600 => tr!("age.minutes", n = secs / 60),
        3600..86_400 => tr!("age.hours", n = secs / 3600),
        _ => tr!("age.days", n = secs / 86_400),
    }
}

/// `in 4h`, `in 12m`, or `soon` when the time has come.
fn relative_in(secs: u64) -> String {
    if secs < 60 {
        tr!("age.soon")
    } else {
        tr!("age.in", age = relative_age(secs))
    }
}

/// `2026-11-06` → `6 November 2026` (`2026年11月6日`). Unparseable input is
/// returned unchanged.
pub(crate) fn long_date(iso: &str) -> String {
    match history::Date::parse(iso) {
        Some(date) => {
            let (y, m, d) = date.ymd();
            crate::i18n::long_date(y, m, d)
        }
        None => iso.to_string(),
    }
}

/// Derives the header's freshness. `as_of` is the table's date for the
/// displayed currencies (`YYYY-MM-DD`); `last_attempt` is when the last
/// refresh started (for the retry time after a failure); `refresh_after` is
/// the refresh interval, `None` for manual.
pub fn freshness(
    table: &RateTable,
    as_of: Option<&str>,
    state: &RefreshState,
    last_attempt: u64,
    refresh_after: Option<u64>,
    now: u64,
) -> Freshness {
    let date = as_of
        .map(long_date)
        .unwrap_or_else(|| tr!("freshness.unknown_date"));
    // The bundled snapshot has no fetch time; its age is the rates' own.
    let fetched_at = match table.source {
        Source::Bundled => as_of
            .and_then(history::Date::parse)
            .map(history::Date::unix_secs)
            .unwrap_or(0),
        _ => table.fetched_at,
    };
    let age_secs = now.saturating_sub(fetched_at);
    let source_line = match table.source {
        Source::Bundled => tr!("freshness.bundled_source", date = date),
        source => tr!(
            "freshness.source",
            source = source.display_name(),
            date = date
        ),
    };
    let next_refresh =
        refresh_after.map(|after| relative_in((fetched_at + after).saturating_sub(now)));
    let retry = (last_attempt + RETRY_AFTER_SECS).saturating_sub(now);
    let (dot, status_line, error) = match state {
        RefreshState::Failed { offline: true, .. } => (
            Dot::Offline,
            tr!("freshness.offline", when = relative_in(retry)),
            None,
        ),
        RefreshState::Failed { message, .. } => (
            Dot::Stale,
            tr!("freshness.failed", when = relative_in(retry)),
            Some(message.clone()),
        ),
        _ if table.source == Source::Bundled => (Dot::Stale, tr!("freshness.not_updated"), None),
        RefreshState::Refreshing => (
            if age_secs > STALE_AFTER_SECS {
                Dot::Stale
            } else {
                Dot::Fresh
            },
            tr!("freshness.updating"),
            None,
        ),
        RefreshState::Idle => (
            if age_secs > STALE_AFTER_SECS {
                Dot::Stale
            } else {
                Dot::Fresh
            },
            format!(
                "{} · {}",
                if age_secs < 60 {
                    tr!("freshness.updated_now")
                } else {
                    tr!("freshness.updated_ago", age = relative_age(age_secs))
                },
                match next_refresh {
                    Some(when) => tr!("freshness.next_refresh", when = when),
                    None => tr!("freshness.manual"),
                }
            ),
            None,
        ),
    };
    Freshness {
        dot,
        updating: *state == RefreshState::Refreshing,
        age: relative_age(age_secs),
        source_line,
        status_line,
        error,
    }
}

/// Settings → Last updated: `6 Nov, 16:05` (`11月6日 16:05`) in local time,
/// given the local UTC offset in seconds. The bundled snapshot has no fetch
/// time and shows `Bundled · rates of 6 Nov`; the year shows when it is not
/// this year's.
pub fn last_updated(table: &RateTable, offset_secs: i64, now: u64) -> String {
    let local = |secs: u64| secs as i64 + offset_secs;
    let day_of = |secs: i64| history::Date::from_unix_secs(secs.max(0) as u64).ymd();
    let (year_now, _, _) = day_of(local(now));
    let day = |(y, m, d): (i32, u32, u32)| crate::i18n::short_date(y, m, d, y != year_now);
    if table.source == Source::Bundled || table.fetched_at == 0 {
        let date = table
            .main_date()
            .and_then(history::Date::parse)
            .map(|d| day(d.ymd()))
            .unwrap_or_else(|| tr!("date.build_time"));
        return tr!("date.bundled", date = date);
    }
    let at = local(table.fetched_at);
    let minutes = at.rem_euclid(86_400) / 60;
    crate::i18n::date_time(&day(day_of(at)), minutes / 60, minutes % 60)
}

/// Seconds since the Unix epoch.
///
/// Debug builds read `KURRENCY_NOW` (Unix seconds): the clock then starts at
/// that moment and runs on from it, so screenshots and the demo show the same
/// rate ages and history dates whenever they are taken. Release builds
/// always use the system clock.
pub fn now_secs() -> u64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    #[cfg(debug_assertions)]
    if let Some(shift) = debug_clock_shift(now) {
        return now.saturating_add_signed(shift);
    }
    now
}

/// How far `KURRENCY_NOW` moves the clock, fixed at the first reading.
#[cfg(debug_assertions)]
fn debug_clock_shift(now: u64) -> Option<i64> {
    static SHIFT: std::sync::OnceLock<Option<i64>> = std::sync::OnceLock::new();
    *SHIFT.get_or_init(|| {
        let start: i64 = std::env::var("KURRENCY_NOW").ok()?.trim().parse().ok()?;
        Some(start - now as i64)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(source: Source, fetched_at: u64) -> RateTable {
        RateTable {
            base: "USD".into(),
            rates: Default::default(),
            dates: Default::default(),
            source,
            fetched_at,
        }
    }

    #[test]
    fn dates_and_ages() {
        assert_eq!(long_date("2026-11-06"), "6 November 2026");
        assert_eq!(long_date("2026-01-31"), "31 January 2026");
        assert_eq!(long_date("nonsense"), "nonsense");
        assert_eq!(relative_age(0), "now");
        assert_eq!(relative_age(59), "now");
        assert_eq!(relative_age(60), "1m");
        assert_eq!(relative_age(2 * 3600 + 59), "2h");
        assert_eq!(relative_age(3 * 86_400 + 5), "3d");
    }

    #[test]
    fn bundled_snapshot_is_usable() {
        let t = bundled();
        assert_eq!(t.source, Source::Bundled);
        assert_eq!(t.base, "USD");
        // Offline on first launch, every default currency still converts.
        for code in crate::money::DEFAULT_CURRENCIES {
            assert!(
                t.rate(code)
                    .is_some_and(|r| r.is_sign_positive() && !r.is_zero()),
                "{code}"
            );
        }
        assert!(t.main_date().is_some());
    }

    #[test]
    fn freshness_variants() {
        let now = 1_794_000_000;
        let d = Some("2026-11-06");
        let fresh = table(Source::Frankfurter, now - 2 * 3600);
        let f = freshness(
            &fresh,
            d,
            &RefreshState::Idle,
            0,
            Some(REFRESH_AFTER_SECS),
            now,
        );
        assert_eq!(f.dot, Dot::Fresh);
        assert_eq!(f.age, "2h");
        assert!(!f.updating);
        assert_eq!(f.source_line, "Frankfurter · rates of 6 November 2026");
        assert_eq!(f.status_line, "Updated 2h ago · next refresh in 4h");
        let f = freshness(&fresh, d, &RefreshState::Idle, 0, Some(3600), now);
        assert_eq!(f.status_line, "Updated 2h ago · next refresh soon");
        let f = freshness(&fresh, d, &RefreshState::Idle, 0, None, now);
        assert_eq!(f.status_line, "Updated 2h ago · refresh manually");

        let just = table(Source::Frankfurter, now - 10);
        let f = freshness(
            &just,
            d,
            &RefreshState::Refreshing,
            now,
            Some(REFRESH_AFTER_SECS),
            now,
        );
        assert!(f.updating);
        assert_eq!((f.dot, f.age.as_str()), (Dot::Fresh, "now"));

        let old = table(Source::Fawaz, now - STALE_AFTER_SECS - 1);
        let f = freshness(
            &old,
            d,
            &RefreshState::Idle,
            0,
            Some(REFRESH_AFTER_SECS),
            now,
        );
        assert_eq!((f.dot, f.age.as_str()), (Dot::Stale, "12h"));
        assert_eq!(f.source_line, "fawazahmed0 · rates of 6 November 2026");

        let offline = RefreshState::Failed {
            offline: true,
            message: "dns".into(),
        };
        let f = freshness(&fresh, d, &offline, now - 60, Some(REFRESH_AFTER_SECS), now);
        assert_eq!(f.dot, Dot::Offline);
        assert_eq!(f.status_line, "Offline · next try in 14m");
        assert_eq!(f.error, None);

        let failed = RefreshState::Failed {
            offline: false,
            message: "HTTP 500".into(),
        };
        let f = freshness(&fresh, d, &failed, now, Some(REFRESH_AFTER_SECS), now);
        assert_eq!(f.dot, Dot::Stale);
        assert_eq!(f.error.as_deref(), Some("HTTP 500"));

        // The bundled snapshot ages from its own date.
        let bundled = table(Source::Bundled, 0);
        let midnight = history::Date::parse("2026-11-06").unwrap().unix_secs();
        let f = freshness(
            &bundled,
            d,
            &RefreshState::Idle,
            0,
            Some(REFRESH_AFTER_SECS),
            midnight + 3 * 86_400,
        );
        assert_eq!((f.dot, f.age.as_str()), (Dot::Stale, "3d"));
        assert!(f.source_line.starts_with("Bundled snapshot"));
    }

    #[test]
    fn last_updated_in_local_time() {
        // 2026-11-06 15:05 UTC.
        let at = history::Date::parse("2026-11-06").unwrap().unix_secs() + 15 * 3600 + 5 * 60;
        let mut t = table(Source::Frankfurter, at);
        assert_eq!(last_updated(&t, 3600, at + 60), "6 Nov, 16:05");
        assert_eq!(last_updated(&t, 0, at), "6 Nov, 15:05");
        // Past midnight in UTC+10, and before it in UTC−16.
        assert_eq!(last_updated(&t, 10 * 3600, at), "7 Nov, 01:05");
        assert_eq!(last_updated(&t, -16 * 3600, at), "5 Nov, 23:05");
        // Another year shows it.
        assert_eq!(last_updated(&t, 0, at + 400 * 86_400), "6 Nov 2026, 15:05");
        t.source = Source::Bundled;
        t.dates.insert("EUR".into(), "2026-11-02".into());
        assert_eq!(last_updated(&t, 0, at), "Bundled · rates of 2 Nov");
    }

    #[test]
    fn chinese_dates_ages_and_freshness() {
        use crate::i18n::{Lang, with_lang};
        with_lang(Lang::ZhCn, || {
            assert_eq!(long_date("2026-11-06"), "2026年11月6日");
            assert_eq!(relative_age(0), "刚刚");
            assert_eq!(relative_age(2 * 60 + 5), "2分钟");
            assert_eq!(relative_age(3 * 3600), "3小时");
            assert_eq!(relative_age(4 * 86_400), "4天");

            let at = history::Date::parse("2026-11-07").unwrap().unix_secs() + 19 * 3600 + 59 * 60;
            let mut t = table(Source::Frankfurter, at);
            assert_eq!(last_updated(&t, 0, at), "11月7日 19:59");
            assert_eq!(
                last_updated(&t, 0, at + 400 * 86_400),
                "2026年11月7日 19:59"
            );
            t.source = Source::Bundled;
            t.dates.insert("EUR".into(), "2026-11-02".into());
            assert_eq!(last_updated(&t, 0, at), "内置 · 11月2日汇率");

            let now = 1_794_000_000;
            let d = Some("2026-11-06");
            let fresh = table(Source::Frankfurter, now - 2 * 3600);
            let f = freshness(
                &fresh,
                d,
                &RefreshState::Idle,
                0,
                Some(REFRESH_AFTER_SECS),
                now,
            );
            assert_eq!(f.age, "2小时");
            assert_eq!(f.source_line, "Frankfurter · 2026年11月6日汇率");
            assert_eq!(f.status_line, "2小时前更新 · 4小时后刷新");
            let f = freshness(&fresh, d, &RefreshState::Idle, 0, Some(3600), now);
            assert_eq!(f.status_line, "2小时前更新 · 即将刷新");
            let f = freshness(&fresh, d, &RefreshState::Idle, 0, None, now);
            assert_eq!(f.status_line, "2小时前更新 · 需手动刷新");
            let just = table(Source::Ecb, now - 10);
            let f = freshness(&just, d, &RefreshState::Idle, 0, None, now);
            assert_eq!(f.status_line, "刚刚更新 · 需手动刷新");
            assert_eq!(f.source_line, "欧洲央行 · 2026年11月6日汇率");
            let offline = RefreshState::Failed {
                offline: true,
                message: "dns".into(),
            };
            let f = freshness(&fresh, d, &offline, now - 60, Some(REFRESH_AFTER_SECS), now);
            assert_eq!(f.status_line, "离线 · 14分钟后重试");
        });
    }
}
