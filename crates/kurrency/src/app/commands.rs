//! The palette's commands (`50 gbp`, `+chf`, `-jpy`, `eur>usd`) and smart
//! paste of a price from the clipboard.

use gpui_kit::component::WindowExt as _;
use gpui_kit::{App, Context, Window};
use kurrency_core::command::{self, Command};
use kurrency_core::i18n::tr;
use kurrency_core::paste;
use kurrency_core::present;
use rust_decimal::Decimal;

use super::{KurrencyApp, Row};
use crate::assets;
use crate::ui;

impl KurrencyApp {
    /// Runs a palette command (`50 gbp`, `+chf`, `-jpy`, `eur>usd`).
    pub(crate) fn run_command(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match command {
            Command::SetAmount { code, amount } => self.apply_price(&code, amount, window, cx),
            Command::Add(code) => self.add_currency(&code, window, cx),
            Command::Remove(code) => self.remove_currency(&code, window, cx),
            Command::History { base, quote } => self.open_history(base, quote, window, cx),
        }
    }

    /// The palette command for `query`, when it is one that can run now:
    /// its currencies have rates, an add is not already listed, a removal is.
    pub(crate) fn palette_command(&self, query: &str, cx: &App) -> Option<Command> {
        let list: Vec<String> = self.rows.iter().map(|row| row.code.clone()).collect();
        let listed = |code: &String| list.contains(code);
        let command = command::parse(query, &list).command?;
        let ok = match &command {
            Command::SetAmount { code, .. } => self.known(code, cx),
            Command::Add(code) => !listed(code) && self.known(code, cx),
            Command::Remove(code) => listed(code),
            Command::History { base, quote } => self.known(base, cx) && self.known(quote, cx),
        };
        ok.then_some(command)
    }

    /// Whether `code` can be shown: it has a rate, or is already listed.
    pub(crate) fn known(&self, code: &str, cx: &App) -> bool {
        self.rows.iter().any(|row| row.code == code)
            || self
                .rates
                .read(cx)
                .table()
                .codes()
                .iter()
                .any(|c| c == code)
    }

    /// Makes `code` the anchor with `amount`, adding it when missing: smart
    /// paste and the palette's "50 gbp". One undo step.
    pub(crate) fn apply_price(
        &mut self,
        code: &str,
        amount: Decimal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.editing.is_some() {
            self.editing = None;
            self.echo = None;
            self.focus_handle.focus(window, cx);
        }
        self.batch(window, cx, |this, window, cx| {
            if !this.rows.iter().any(|row| row.code == code) {
                this.rows.push(Row {
                    code: code.to_string(),
                    focus_handle: cx.focus_handle(),
                });
            }
            if let Some(ix) = this.rows.iter().position(|row| row.code == code) {
                this.promote(ix, window, cx);
            }
            this.amount = Some(amount);
        });
        cx.notify();
    }

    /// Reads a price from the clipboard and applies it. A bare number sets
    /// the anchor amount unless `only_priced`. Returns whether it did.
    pub(super) fn smart_paste(
        &mut self,
        only_priced: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return false;
        };
        self.paste_text(&text, only_priced, window, cx)
    }

    /// Applies pasted `text` as smart paste does: a price makes its currency
    /// the anchor, a bare number sets the amount unless `only_priced`.
    /// Returns whether it did.
    pub(super) fn paste_text(
        &mut self,
        text: &str,
        only_priced: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let list: Vec<String> = self.rows.iter().map(|row| row.code.clone()).collect();
        let Some(price) = paste::parse_price(text, &list) else {
            return false;
        };
        match price.code {
            Some(code) if self.known(&code, cx) => {
                self.apply_price(&code, price.amount, window, cx);
                true
            }
            Some(_) => false,
            None if only_priced || self.anchor().is_none() => false,
            None => {
                if self.editing.is_some() {
                    self.editing = None;
                    self.echo = None;
                    self.focus_handle.focus(window, cx);
                }
                self.set_amount(price.amount, window, cx);
                true
            }
        }
    }

    /// Paste inside the anchor input: a price takes over when the input is
    /// empty or all selected; anything else is pasted as text.
    pub(super) fn on_input_paste(
        &mut self,
        _: &gpui_kit::component::input::Paste,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.anchor_focused(window, cx) {
            return;
        }
        let input = self.anchor_input.read(cx);
        let all_selected = input.selected_range() == (0..input.value().len());
        if all_selected && self.smart_paste(true, window, cx) {
            cx.stop_propagation();
        }
    }

    /// Checks the clipboard when returning to the window and offers a one-click
    /// convert toast if it holds a new currency-tagged price (DESIGN §2).
    pub(crate) fn check_clipboard_hint(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page.is_some()
            || self.is_editing()
            || self.toast.is_some()
            || window.has_active_dialog(cx)
        {
            return;
        }
        let Some(raw) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.len() > 64 {
            return;
        }
        if self.last_clipboard_hint.as_deref() == Some(trimmed) {
            return;
        }
        let list: Vec<String> = self.rows.iter().map(|row| row.code.clone()).collect();
        let Some(price) = paste::parse_price(trimmed, &list) else {
            return;
        };
        let Some(code) = price.code else {
            return;
        };
        if !self.known(&code, cx) {
            return;
        }
        if self.anchor() == Some(code.as_str()) && self.amount == Some(price.amount) {
            self.last_clipboard_hint = Some(trimmed.to_string());
            return;
        }
        self.last_clipboard_hint = Some(trimmed.to_string());
        let parts = present::amount_parts(&code, price.amount, &self.fmt, assets::has_glyph);
        let pretty = match parts.symbol {
            Some(sym) => format!("{sym}{}{}", parts.integer, parts.fraction),
            None => format!("{code} {}{}", parts.integer, parts.fraction),
        };
        self.show_toast(
            tr!("toast.clipboard_convert", price = pretty),
            ui::toast::ToastAction::ConvertClipboard {
                code,
                amount: price.amount,
            },
            ui::toast::CLIPBOARD_SHOW_FOR,
            cx,
        );
        cx.notify();
    }
}
