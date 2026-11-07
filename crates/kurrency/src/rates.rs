//! The GPUI side of the exchange rates: the store entity that loads, caches
//! and refreshes the current table, and the history fetcher of the detail
//! page, both over GPUI's HTTP client. The tables, their formats and maths
//! are `kurrency_core::rates`.

pub(crate) mod history_provider;
pub(crate) mod store;

use anyhow::{Context as _, bail};
use futures::AsyncReadExt as _;
use gpui_kit::http_client::{AsyncBody, HttpClient};

/// Why a request failed, which decides between "offline" and "failed".
enum FetchError {
    /// No HTTP response: DNS, connect, TLS or timeout.
    Transport(anyhow::Error),
    /// A response that was not a usable 2xx.
    Http(anyhow::Error),
}

/// GETs `url` and returns the body of a 2xx response.
async fn fetch(client: &dyn HttpClient, url: &str) -> Result<Vec<u8>, FetchError> {
    let mut response = client
        .get(url, AsyncBody::empty(), true)
        .await
        .map_err(FetchError::Transport)?;
    let status = response.status();
    let mut body = Vec::new();
    let read = response
        .body_mut()
        .read_to_end(&mut body)
        .await
        .with_context(|| format!("{url}: reading body"));
    let check = || -> anyhow::Result<Vec<u8>> {
        read?;
        if !status.is_success() {
            bail!("{url}: HTTP {status}");
        }
        Ok(body)
    };
    check().map_err(FetchError::Http)
}
