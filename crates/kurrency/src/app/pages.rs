//! The full-window pages over the list (a pair's history, the settings):
//! opening, sliding in, and back to the list with focus restored.

use std::time::Duration;

use gpui_kit::{AppContext as _, Context, Focusable, Window};

use super::{KurrencyApp, Page, PageView};
use crate::rates::history_provider::HistoryProvider;
use crate::ui;

impl KurrencyApp {
    /// Opens the settings page (the gear, Ctrl+,), replacing a history page.
    pub(crate) fn open_settings_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(
            self.page.as_ref().map(|p| &p.view),
            Some(PageView::Settings(_))
        ) {
            return;
        }
        if self.editing.is_some() {
            self.commit(window, cx);
            self.editing = None;
            self.echo = None;
        }
        let app = cx.weak_entity();
        let rates = self.rates.clone();
        let locale = self.locale.clone();
        let view = cx.new(|cx| ui::settings::SettingsView::new(app, rates, locale, cx));
        let subscription = cx.subscribe_in(
            &view,
            window,
            |this, _, _: &ui::settings::CloseSettings, window, cx| this.close_page(window, cx),
        );
        view.read(cx).focus_handle(cx).focus(window, cx);
        self.show_page(
            Page {
                view: PageView::Settings(view),
                from_row: None,
                _subscription: subscription,
            },
            cx,
        );
    }

    /// Slides a page in over the list.
    fn show_page(&mut self, page: Page, cx: &mut Context<Self>) {
        self.page = Some(page);
        self.page_opens += 1;
        self.hovered = None;
        self.dismiss_toast(cx);
        cx.notify();
    }

    /// Opens the history page for `code` against the anchor currency. The
    /// anchor itself is compared with the first row (or USD/EUR).
    pub(crate) fn open_detail(&mut self, code: &str, window: &mut Window, cx: &mut Context<Self>) {
        if !self.rows.iter().any(|row| row.code == code) {
            log::warn!("no row {code} to open");
            return;
        }
        let Some(anchor) = self.anchor().map(str::to_string) else {
            return;
        };
        let base = if code != anchor {
            anchor
        } else {
            self.rows
                .iter()
                .map(|row| row.code.as_str())
                .chain(["USD", "EUR"])
                .find(|c| *c != code)
                .unwrap_or("USD")
                .to_string()
        };
        self.open_history(base, code.to_string(), window, cx);
    }

    /// Opens the history page of `base` → `quote` (also the palette's
    /// `eur>usd`).
    pub(crate) fn open_history(
        &mut self,
        base: String,
        quote: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let code = quote.clone();
        let provider = HistoryProvider::new(cx.http_client(), self.history_dir.clone());
        let green_up = self.green_up();
        let decimal = self.fmt.style.decimal;
        let rates = self.rates.clone();
        let view = cx.new(|cx| {
            ui::detail::DetailView::new(base, quote, rates, Some(provider), green_up, decimal, cx)
        });
        let subscription = cx.subscribe_in(
            &view,
            window,
            |this, _, _: &ui::detail::CloseDetail, window, cx| this.close_page(window, cx),
        );
        view.read(cx).focus_handle(cx).focus(window, cx);
        self.show_page(
            Page {
                view: PageView::Detail(view),
                from_row: Some(code),
                _subscription: subscription,
            },
            cx,
        );
    }

    /// Back to the list, with focus on the row a history page was opened
    /// from; the page slides back out to the right.
    pub(crate) fn close_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(page) = self.page.take() else {
            return;
        };
        if !cx.reduce_motion() {
            let version = self.motion.bump();
            self.closing = Some((page.view.any(), version));
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(ui::detail::SLIDE_DURATION + Duration::from_millis(40))
                    .await;
                let _ = this.update(cx, |this, cx| {
                    if this.closing.as_ref().is_some_and(|(_, v)| *v == version) {
                        this.closing = None;
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        let from_row = page.from_row.as_deref();
        match self
            .rows
            .iter()
            .position(|row| Some(row.code.as_str()) == from_row)
        {
            Some(ix) if ix > 0 => self.focus_row(ix, window, cx),
            _ => self.focus_handle.focus(window, cx),
        }
        cx.notify();
    }
}
