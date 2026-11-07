//! The rates store entity: owns the current [`RateTable`], loads the cache
//! synchronously at startup, and refreshes it in the background through the
//! provider chain.

use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::http_client::HttpClient;
use gpui_kit::{Context, Task};
use kurrency_core::rates::previous::{self, Previous};
use kurrency_core::rates::{
    self, Freshness, REFRESH_AFTER_SECS, RETRY_AFTER_SECS, RateTable, RefreshState, Source, cache,
    frankfurter, freshness, now_secs, provider,
};
use kurrency_core::settings::RateSource;

use super::{FetchError, fetch};

pub(crate) struct RatesStore {
    table: RateTable,
    /// The previous day's rates for the change chips, when known.
    previous: Option<Previous>,
    state: RefreshState,
    /// When the last refresh started (Unix seconds), for the retry delay.
    last_attempt: u64,
    /// Bumped whenever a refresh starts, so the freshness dot pulses once.
    refreshes: usize,
    /// Settings → Source.
    source: RateSource,
    /// Settings → Refresh: seconds between automatic refreshes, `None` for
    /// manual.
    refresh_after: Option<u64>,
    cache_path: Option<PathBuf>,
    previous_path: Option<PathBuf>,
    /// The in-flight refresh. Dropping it cancels the request.
    _refresh: Option<Task<()>>,
    /// The in-flight previous-day request.
    _previous: Option<Task<()>>,
}

impl RatesStore {
    /// Loads the cache (or the bundled snapshot) synchronously.
    pub(crate) fn new(_cx: &mut Context<Self>) -> Self {
        let cache_path = cache::default_path();
        let table = cache_path
            .as_deref()
            .and_then(cache::load)
            .unwrap_or_else(rates::bundled);
        log::info!(
            "loaded {} rates from {:?} (fetched_at {})",
            table.rates.len(),
            table.source,
            table.fetched_at
        );
        let previous_path = previous::default_path();
        let previous = previous_path.as_deref().and_then(previous::load);
        Self {
            table,
            previous,
            state: RefreshState::Idle,
            last_attempt: 0,
            refreshes: 0,
            source: RateSource::default(),
            refresh_after: Some(REFRESH_AFTER_SECS),
            cache_path,
            previous_path,
            _refresh: None,
            _previous: None,
        }
    }

    /// A store over fixed tables that never touches the disk.
    #[cfg(test)]
    pub(crate) fn from_table(table: RateTable) -> Self {
        Self {
            table,
            previous: None,
            state: RefreshState::Idle,
            last_attempt: 0,
            refreshes: 0,
            source: RateSource::default(),
            refresh_after: Some(REFRESH_AFTER_SECS),
            cache_path: None,
            previous_path: None,
            _refresh: None,
            _previous: None,
        }
    }

    pub(crate) fn table(&self) -> &RateTable {
        &self.table
    }

    /// Applies Settings → Source and Refresh. A new source cancels a refresh
    /// in flight and fetches from its own chain when the table came from
    /// another feed; a shorter interval may make the table due.
    pub(crate) fn configure(
        &mut self,
        source: RateSource,
        refresh_after: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        if self.source != source {
            self.source = source;
            self._refresh = None;
            self.state = RefreshState::Idle;
        }
        self.refresh_after = refresh_after;
        self.refresh_if_stale(cx);
        cx.notify();
    }

    /// The previous day's table, when it belongs to the current one.
    pub(crate) fn previous(&self) -> Option<&Previous> {
        self.previous.as_ref().filter(|p| p.matches(&self.table))
    }

    pub(crate) fn is_refreshing(&self) -> bool {
        self.state == RefreshState::Refreshing
    }

    /// How many refreshes have started.
    pub(crate) fn refreshes(&self) -> usize {
        self.refreshes
    }

    pub(crate) fn freshness<'a>(&self, codes: impl IntoIterator<Item = &'a str>) -> Freshness {
        freshness(
            &self.table,
            self.table.as_of(codes),
            &self.state,
            self.last_attempt,
            self.refresh_after,
            now_secs(),
        )
    }

    /// Refreshes when the table is the bundled snapshot or older than the
    /// refresh interval, waiting [`RETRY_AFTER_SECS`] after a failure; with
    /// manual refresh, only when it is the bundled snapshot. A table from
    /// another feed than Settings → Source asks for is due too. Otherwise
    /// fetches the previous day when it is missing.
    pub(crate) fn refresh_if_stale(&mut self, cx: &mut Context<Self>) {
        let now = now_secs();
        let due = self.table.source == Source::Bundled
            || !self.table.source.serves(self.source)
            || self
                .refresh_after
                .is_some_and(|after| self.table.is_older_than(after, now));
        let waiting = matches!(self.state, RefreshState::Failed { .. })
            && now.saturating_sub(self.last_attempt) < RETRY_AFTER_SECS;
        if due && !waiting {
            self.refresh(cx);
        } else if !due {
            self.fetch_previous_if_missing(cx);
        }
    }

    /// Fetches the previous day's rates for the current table, unless they
    /// are already here or on their way. Failures only leave the chips at "·".
    pub(crate) fn fetch_previous_if_missing(&mut self, cx: &mut Context<Self>) {
        if self.previous().is_some() || self._previous.is_some() {
            return;
        }
        let Some(date) = previous::wanted_date(&self.table) else {
            return;
        };
        let source = self.table.source;
        let url = previous::url(&self.table.base, date, source);
        let client = cx.http_client();
        self._previous = Some(cx.spawn(async move |this, cx| {
            let result = match fetch(client.as_ref(), &url).await {
                Ok(body) => frankfurter::parse(&body, now_secs()).map(|mut table| {
                    table.source = source;
                    table
                }),
                Err(FetchError::Transport(err) | FetchError::Http(err)) => Err(err),
            };
            let _ = this.update(cx, |this, cx| {
                this._previous = None;
                match result {
                    Ok(table) => {
                        let previous = Previous {
                            date: date.to_string(),
                            table,
                        };
                        if let Some(path) = &this.previous_path
                            && let Err(err) = previous::save(path, &previous)
                        {
                            log::warn!("could not write the previous-day cache: {err:#}");
                        }
                        this.previous = Some(previous);
                        cx.notify();
                    }
                    Err(err) => log::warn!("no previous-day rates: {err:#}"),
                }
            });
        }));
    }

    /// Fetches fresh rates down the provider chain in the background.
    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.is_refreshing() {
            return;
        }
        self.state = RefreshState::Refreshing;
        self.last_attempt = now_secs();
        self.refreshes += 1;
        cx.notify();

        let client = cx.http_client();
        let chain = provider::chain(self.source);
        self._refresh = Some(cx.spawn(async move |this, cx| {
            let result = fetch_chain(client, chain).await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(table) => {
                        log::info!(
                            "fetched {} rates from {:?}",
                            table.rates.len(),
                            table.source
                        );
                        if let Some(path) = &this.cache_path
                            && let Err(err) = cache::save(path, &table)
                        {
                            log::warn!("could not write rate cache: {err:#}");
                        }
                        this.table = table;
                        this.state = RefreshState::Idle;
                        this.fetch_previous_if_missing(cx);
                    }
                    Err(failure) => {
                        log::warn!("rate refresh failed: {}", failure.message);
                        this.state = RefreshState::Failed {
                            offline: failure.offline,
                            message: failure.message,
                        };
                    }
                }
                this._refresh = None;
                cx.notify();
            });
        }));
    }
}

struct ChainFailure {
    offline: bool,
    message: String,
}

/// Tries every provider URL in order and returns the first table that parses.
async fn fetch_chain(
    client: Arc<dyn HttpClient>,
    chain: &'static [provider::Provider],
) -> Result<RateTable, ChainFailure> {
    let mut errors = Vec::new();
    let mut answered = false;
    for provider in chain {
        for url in provider.urls {
            log::debug!("fetching {:?} rates from {url}", provider.source);
            match fetch(client.as_ref(), url).await {
                Ok(body) => {
                    answered = true;
                    match (provider.parse)(&body, now_secs()) {
                        Ok(table) => return Ok(table),
                        Err(err) => errors.push(format!("{err:#}")),
                    }
                }
                Err(FetchError::Http(err)) => {
                    answered = true;
                    errors.push(format!("{err:#}"));
                }
                Err(FetchError::Transport(err)) => errors.push(format!("{url}: {err:#}")),
            }
        }
    }
    Err(ChainFailure {
        offline: !answered,
        message: errors.join("\n"),
    })
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;
    use std::sync::Mutex;

    use gpui_kit::http_client::{AsyncBody, Request, Response, Url, http};
    use gpui_kit::{AppContext as _, TestAppContext};
    use rust_decimal::Decimal;

    use super::*;

    /// What a feed answers for a URL: a status and body, or `None` for no
    /// response at all (offline).
    type Answer = fn(&str) -> Option<(u16, String)>;

    /// Records the URLs asked for and answers each with `answer`.
    struct Feed {
        urls: Arc<Mutex<Vec<String>>>,
        answer: Answer,
    }

    impl Feed {
        /// Frankfurter's rates dated 2026-11-07, and the previous day's.
        fn frankfurter(urls: Arc<Mutex<Vec<String>>>) -> Self {
            Self {
                urls,
                answer: |url| {
                    let (date, eur) = if url.contains("date=2026-11-06") {
                        ("2026-11-06", 0.91)
                    } else {
                        ("2026-11-07", 0.9)
                    };
                    let body = format!(
                        r#"[{{"date":"{date}","base":"USD","quote":"EUR","rate":{eur}}},
                            {{"date":"{date}","base":"USD","quote":"JPY","rate":150}}]"#
                    );
                    Some((200, body))
                },
            }
        }
    }

    impl HttpClient for Feed {
        fn user_agent(&self) -> Option<&http::HeaderValue> {
            None
        }

        fn proxy(&self) -> Option<&Url> {
            None
        }

        fn send(
            &self,
            req: Request<AsyncBody>,
        ) -> futures::future::BoxFuture<'static, anyhow::Result<Response<AsyncBody>>> {
            use futures::FutureExt as _;
            let url = req.uri().to_string();
            self.urls.lock().unwrap().push(url.clone());
            let answer = (self.answer)(&url);
            async move {
                let (status, body) = answer.ok_or_else(|| anyhow::anyhow!("dns error"))?;
                Ok(Response::builder()
                    .status(status)
                    .body(body.into())
                    .unwrap())
            }
            .boxed()
        }
    }

    #[gpui_kit::test]
    fn refresh_fetches_the_previous_day_once(cx: &mut TestAppContext) {
        let urls = Arc::new(Mutex::new(Vec::new()));
        cx.update(|cx| cx.set_http_client(Arc::new(Feed::frankfurter(urls.clone()))));
        let store = cx.new(|_| RatesStore::from_table(rates::bundled()));

        store.update(cx, RatesStore::refresh_if_stale);
        cx.run_until_parked();
        store.read_with(cx, |store, _| {
            assert_eq!(store.table().main_date(), Some("2026-11-07"));
            assert_eq!(store.refreshes(), 1);
            let previous = store.previous().expect("the previous day was fetched");
            assert_eq!(previous.date, "2026-11-06");
            assert_eq!(
                previous.cross_rate("USD", "EUR"),
                Decimal::from_str("0.91").ok()
            );
        });
        assert_eq!(
            *urls.lock().unwrap(),
            [
                "https://api.frankfurter.dev/v2/rates?base=USD",
                "https://api.frankfurter.dev/v2/rates?base=USD&date=2026-11-06",
            ]
        );

        // Fresh rates with their previous day: nothing more to fetch.
        store.update(cx, RatesStore::refresh_if_stale);
        cx.run_until_parked();
        assert_eq!(urls.lock().unwrap().len(), 2);
    }

    #[gpui_kit::test]
    fn the_source_setting_picks_the_chain(cx: &mut TestAppContext) {
        let urls = Arc::new(Mutex::new(Vec::new()));
        cx.update(|cx| cx.set_http_client(Arc::new(Feed::frankfurter(urls.clone()))));
        let store = cx.new(|_| RatesStore::from_table(rates::bundled()));

        store.update(cx, |store, cx| {
            store.configure(RateSource::Ecb, Some(3600), cx);
        });
        cx.run_until_parked();
        store.read_with(cx, |store, _| {
            assert_eq!(store.table().source, Source::FrankfurterEcb);
            assert!(store.previous().is_some());
        });
        assert_eq!(
            *urls.lock().unwrap(),
            [
                "https://api.frankfurter.dev/v2/rates?base=USD&providers=ECB",
                "https://api.frankfurter.dev/v2/rates?base=USD&date=2026-11-06&providers=ECB",
            ]
        );

        // Fresh ECB rates serve "ECB official": nothing to fetch.
        store.update(cx, RatesStore::refresh_if_stale);
        cx.run_until_parked();
        assert_eq!(urls.lock().unwrap().len(), 2);

        // Switching to the blend fetches it at once.
        store.update(cx, |store, cx| {
            store.configure(RateSource::Frankfurter, None, cx);
        });
        cx.run_until_parked();
        store.read_with(cx, |store, _| {
            assert_eq!(store.table().source, Source::Frankfurter);
        });
        assert_eq!(
            urls.lock().unwrap()[2],
            "https://api.frankfurter.dev/v2/rates?base=USD"
        );
        // Manual refresh: an old table stays until asked.
        let n = urls.lock().unwrap().len();
        store.update(cx, |store, cx| {
            store.table.fetched_at = 0;
            store.refresh_if_stale(cx);
        });
        cx.run_until_parked();
        assert_eq!(urls.lock().unwrap().len(), n);
    }

    /// Auto falls back down the chain past a server error and an unreachable
    /// host. With nothing usable it keeps the table, tells an offline machine
    /// from a failing feed, and waits before trying again.
    #[gpui_kit::test]
    fn the_chain_falls_back_and_tells_offline_from_failed(cx: &mut TestAppContext) {
        let urls = Arc::new(Mutex::new(Vec::new()));
        let feed = Feed {
            urls: urls.clone(),
            answer: |url| {
                if url.contains("frankfurter") {
                    Some((500, String::new()))
                } else if url.contains("jsdelivr") {
                    Some((200, r#"{"date":"2026-11-07","usd":{"eur":0.86}}"#.into()))
                } else {
                    None
                }
            },
        };
        cx.update(|cx| cx.set_http_client(Arc::new(feed)));
        let store = cx.new(|_| RatesStore::from_table(rates::bundled()));
        store.update(cx, RatesStore::refresh_if_stale);
        cx.run_until_parked();
        store.read_with(cx, |store, _| {
            assert_eq!(store.table().source, Source::Fawaz);
            assert_eq!(store.table().rate("EUR"), Decimal::from_str("0.86").ok());
            assert_eq!(store.state, RefreshState::Idle);
        });
        let fawaz = rates::fawaz::URLS;
        assert_eq!(
            *urls.lock().unwrap(),
            [frankfurter::URL, fawaz[0], fawaz[1]]
        );

        let cases: [(Answer, bool); 2] = [
            (|_| None, true),
            (|_| Some((200, "not json".into())), false),
        ];
        for (answer, offline) in cases {
            let urls = Arc::new(Mutex::new(Vec::new()));
            let feed = Feed {
                urls: urls.clone(),
                answer,
            };
            cx.update(|cx| cx.set_http_client(Arc::new(feed)));
            let store = cx.new(|_| RatesStore::from_table(rates::bundled()));
            store.update(cx, RatesStore::refresh_if_stale);
            cx.run_until_parked();
            store.read_with(cx, |store, _| {
                assert_eq!(store.table().source, Source::Bundled);
                assert!(
                    matches!(store.state, RefreshState::Failed { offline: o, .. } if o == offline),
                    "{:?}",
                    store.state
                );
            });
            // Frankfurter, both fawazahmed0 hosts and the ECB, once each.
            assert_eq!(urls.lock().unwrap().len(), 4);
            store.update(cx, RatesStore::refresh_if_stale);
            cx.run_until_parked();
            assert_eq!(urls.lock().unwrap().len(), 4, "a retry waits");
        }
    }
}
