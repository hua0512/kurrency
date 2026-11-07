//! Fetches and caches the history of one pair from Frankfurter v2.
//!
//! No GPUI entity: the detail page runs [`HistoryProvider::load`] on the
//! background executor and keeps the task, so leaving the page cancels it.

use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::http_client::HttpClient;
use kurrency_core::rates::history::{self, Date, History};

use super::{FetchError, fetch};

/// The provider label in the detail page footer.
pub(crate) const SOURCE_LABEL: &str = "Frankfurter";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HistoryError {
    /// No response at all (DNS, connect, TLS, timeout).
    Offline,
    /// A response that was an error or unreadable.
    Failed(String),
}

#[derive(Clone)]
pub(crate) struct HistoryProvider {
    client: Arc<dyn HttpClient>,
    /// `None` keeps everything in memory (tests, platforms without a cache dir).
    dir: Option<PathBuf>,
}

impl HistoryProvider {
    pub(crate) fn new(client: Arc<dyn HttpClient>, dir: Option<PathBuf>) -> Self {
        Self { client, dir }
    }

    /// The cached series, or an empty one. Blocking file IO: call it off the
    /// main thread.
    pub(crate) fn cached(&self, base: &str, quote: &str) -> History {
        self.dir
            .as_deref()
            .map(|dir| history::cache_path(dir, base, quote))
            .and_then(|path| history::load(&path, base, quote))
            .unwrap_or_else(|| History::empty(base, quote))
    }

    /// Fetches what `history` lacks to cover `from` up to the latest day and
    /// writes the result back to the cache. Whatever was fetched is kept even
    /// when a later request fails; the error is returned alongside.
    pub(crate) async fn update(
        &self,
        mut history: History,
        from: Date,
        now: u64,
    ) -> (History, Option<HistoryError>) {
        let gaps = history.gaps(from, now);
        if gaps.is_empty() {
            return (history, None);
        }
        let mut error = None;
        let mut changed = false;
        for gap in gaps {
            let url = history::url(&history.base, &history.quote, gap.from, gap.to);
            log::debug!("fetching history {url}");
            let result = match fetch(self.client.as_ref(), &url).await {
                Ok(body) => history::parse(&body, &history.base, &history.quote)
                    .map_err(|err| HistoryError::Failed(format!("{err:#}"))),
                Err(FetchError::Transport(err)) => {
                    log::warn!("history offline: {err:#}");
                    Err(HistoryError::Offline)
                }
                Err(FetchError::Http(err)) => Err(HistoryError::Failed(format!("{err:#}"))),
            };
            match result {
                Ok(points) => {
                    log::info!(
                        "fetched {} {}/{} history points from {}",
                        points.len(),
                        history.base,
                        history.quote,
                        gap.from
                    );
                    history.merge(gap, points, now);
                    changed = true;
                }
                Err(err) => {
                    error = Some(err);
                    break;
                }
            }
        }
        if changed && let Some(dir) = &self.dir {
            let path = history::cache_path(dir, &history.base, &history.quote);
            if let Err(err) = history::save(&path, &history) {
                log::warn!("could not write history cache: {err:#}");
            }
        }
        (history, error)
    }
}
