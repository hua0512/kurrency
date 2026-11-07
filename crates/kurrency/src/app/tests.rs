//! The UI flow tests: the whole view on GPUI's test platform (no display,
//! window server or GPU), driven by actions, keystrokes and simulated clicks.
//! The helpers here open the view over a fixture rate table and read its
//! state; the tests are grouped by what they exercise.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use gpui_kit::component::{Root, WindowExt as _};
use gpui_kit::http_client::{AsyncBody, HttpClient, Request, Response, Url, http};
use gpui_kit::{ClipboardItem, Modifiers, MouseButton, TestAppContext, VisualTestContext, px};
use kurrency_core::command::Command;
use kurrency_core::rates::{RateTable, Source};

use super::actions::*;
use super::*;
use crate::layout;
use crate::rates::history_provider::HistoryError;

fn d(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

fn table() -> RateTable {
    let rates = [("EUR", "0.9"), ("JPY", "150")];
    RateTable {
        base: "USD".into(),
        rates: rates.iter().map(|(c, r)| (c.to_string(), d(r))).collect(),
        dates: rates
            .iter()
            .map(|(c, _)| (c.to_string(), "2026-11-06".to_string()))
            .collect(),
        source: Source::Frankfurter,
        fetched_at: kurrency_core::rates::now_secs(),
    }
}

fn settings(currencies: &[&str], amount: &str) -> Settings {
    Settings {
        currencies: currencies.iter().map(ToString::to_string).collect(),
        source: currencies.first().map(ToString::to_string),
        amount: Some(amount.into()),
        ..Settings::default()
    }
}

/// Opens the app over the fixture table with USD 100 and EUR, JPY rows.
fn open_app(
    cx: &mut TestAppContext,
    http: Option<Arc<AtomicUsize>>,
) -> (Entity<KurrencyApp>, &mut VisualTestContext) {
    open_with(cx, http, settings(&["USD", "EUR", "JPY"], "100"))
}

fn open_with(
    cx: &mut TestAppContext,
    http: Option<Arc<AtomicUsize>>,
    settings: Settings,
) -> (Entity<KurrencyApp>, &mut VisualTestContext) {
    open_saving(cx, http, settings, None)
}

/// [`open_with`], saving the settings to `path`.
fn open_saving(
    cx: &mut TestAppContext,
    http: Option<Arc<AtomicUsize>>,
    settings: Settings,
    path: Option<PathBuf>,
) -> (Entity<KurrencyApp>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        init(cx);
        cx.set_http_client(Arc::new(FixtureHttp {
            offline: http.is_none(),
            requests: http.unwrap_or_default(),
        }));
    });
    let mut app = None;
    let (_root, cx) = cx.add_window_view(|window, cx| {
        let rates = cx.new(|_| RatesStore::from_table(table()));
        let view = cx.new(|cx| KurrencyApp::with_state(settings, path, rates, window, cx));
        app = Some(view.clone());
        Root::new(view, window, cx)
    });
    draw(cx);
    (app.unwrap(), cx)
}

/// Focus and blur events are delivered when the window draws.
fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Lets every animation and its clean-up timer finish.
fn settle(cx: &mut VisualTestContext) {
    cx.executor().advance_clock(Duration::from_secs(2));
    draw(cx);
}

fn codes(app: &Entity<KurrencyApp>, cx: &mut VisualTestContext) -> Vec<String> {
    app.read_with(cx, |app, _| {
        app.rows.iter().map(|r| r.code.clone()).collect()
    })
}

/// What the read-only rows show, formatted.
fn values(app: &Entity<KurrencyApp>, cx: &mut VisualTestContext) -> Vec<String> {
    app.read_with(cx, |app, cx| {
        app.rows[1..]
            .iter()
            .map(|row| match app.value_of(&row.code, cx) {
                Some(v) => money::format_amount(v, money::minor_units(&row.code)),
                None => "—".into(),
            })
            .collect()
    })
}

fn amount(app: &Entity<KurrencyApp>, cx: &mut VisualTestContext) -> Option<Decimal> {
    app.read_with(cx, |app, _| app.amount)
}

fn input_text(app: &Entity<KurrencyApp>, cx: &mut VisualTestContext) -> String {
    app.read_with(cx, |app, cx| app.anchor_input.read(cx).value().to_string())
}

fn echo(app: &Entity<KurrencyApp>, cx: &mut VisualTestContext) -> Option<String> {
    app.read_with(cx, |app, _| app.echo.as_ref().map(ToString::to_string))
}

fn anchor_focused(app: &Entity<KurrencyApp>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| app.read(cx).anchor_focused(window, cx))
}

fn focused_row(app: &Entity<KurrencyApp>, cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|window, cx| {
        let app = app.read(cx);
        app.focused_row(window).map(|ix| app.rows[ix].code.clone())
    })
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is not on screen"));
    cx.simulate_click(bounds.center(), Modifiers::none());
    draw(cx);
}

/// Serves a made-up daily series for any time-series request and counts
/// the requests.
struct FixtureHttp {
    requests: Arc<AtomicUsize>,
    /// Fail every request like a machine without network.
    offline: bool,
}

impl HttpClient for FixtureHttp {
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
        use history::Date;
        self.requests.fetch_add(1, Ordering::SeqCst);
        if self.offline {
            return async { anyhow::bail!("dns error: no network") }.boxed();
        }
        let query = req.uri().query().unwrap_or_default().to_string();
        let param = |key: &str| {
            query
                .split('&')
                .find_map(|kv| kv.strip_prefix(&format!("{key}=")))
                .unwrap_or_default()
                .to_string()
        };
        let (base, quote) = (param("base"), param("quotes"));
        let today = Date::from_unix_secs(kurrency_core::rates::now_secs());
        let from = Date::parse(&param("from"))
            .unwrap_or(today)
            .max(today.add_days(-400));
        let to = Date::parse(&param("to")).unwrap_or(today);
        let mut rows = Vec::new();
        let mut day = from;
        let mut i = 0;
        while day <= to {
            let rate = if base == "USD" { 0.9 } else { 1.1 } + f64::from(i % 10) / 1000.;
            rows.push(format!(
                r#"{{"date":"{day}","base":"{base}","quote":"{quote}","rate":{rate}}}"#
            ));
            day = day.add_days(1);
            i += 1;
        }
        let body = format!("[{}]", rows.join(","));
        async move { Ok(Response::builder().status(200).body(body.into()).unwrap()) }.boxed()
    }
}

fn detail(
    app: &Entity<KurrencyApp>,
    cx: &mut VisualTestContext,
) -> Option<Entity<ui::detail::DetailView>> {
    app.read_with(cx, |app, _| match app.page.as_ref().map(|p| &p.view) {
        Some(PageView::Detail(view)) => Some(view.clone()),
        _ => None,
    })
}

#[cfg(debug_assertions)]
mod demo;
mod editing;
mod pages;
mod palette;
mod rows;

/// Keystrokes written with `ctrl-`, sent with `cmd-` on macOS, where the app
/// (and gpui-kit's inputs) bind Cmd instead.
fn keys(strokes: &str) -> String {
    if !cfg!(target_os = "macos") {
        return strokes.to_owned();
    }
    strokes
        .split(' ')
        .map(|k| match k.strip_prefix("ctrl-") {
            Some(rest) => format!("cmd-{rest}"),
            None => k.to_owned(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}
