//! Every committed change to the list and amount: one undo step each, and
//! saved to `settings.json`.

use gpui_kit::{Context, Window};
use kurrency_core::settings;
use kurrency_core::undo;

use super::{KurrencyApp, Row};

impl KurrencyApp {
    pub(crate) fn undo_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page.is_some() {
            return;
        }
        self.dismiss_toast(cx);
        if let Some(previous) = self.history.undo(self.snapshot()) {
            self.restore(previous, window, cx);
        }
    }

    pub(super) fn redo_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page.is_some() {
            return;
        }
        if let Some(next) = self.history.redo(self.snapshot()) {
            self.restore(next, window, cx);
        }
    }

    /// Puts back an undo snapshot, keeping the rows' focus handles.
    fn restore(&mut self, state: undo::Snapshot, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.is_some() {
            self.editing = None;
            self.echo = None;
            self.focus_handle.focus(window, cx);
        }
        let mut old = std::mem::take(&mut self.rows);
        self.rows = state
            .currencies
            .iter()
            .map(|code| match old.iter().position(|row| &row.code == code) {
                Some(ix) => old.swap_remove(ix),
                None => Row {
                    code: code.clone(),
                    focus_handle: cx.focus_handle(),
                },
            })
            .collect();
        self.amount = state.amount;
        self.last_state = state;
        self.hovered = None;
        // A focused row that became the anchor (or went) is not on screen
        // any more; keep the window's shortcuts reachable.
        if self.focused_row(window).is_none() {
            self.focus_handle.focus(window, cx);
        }
        self.save_settings();
        cx.notify();
    }

    /// The list and amount, for undo.
    fn snapshot(&self) -> undo::Snapshot {
        undo::Snapshot {
            currencies: self.rows.iter().map(|row| row.code.clone()).collect(),
            amount: self.amount,
        }
    }

    /// Runs several changes as one undo step, saved once at the end.
    pub(super) fn batch(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>),
    ) {
        self.batch += 1;
        f(self, window, cx);
        self.batch -= 1;
        self.save_settings();
    }

    /// Every committed change ends here: it becomes an undo step and is
    /// saved.
    pub(super) fn save_settings(&mut self) {
        if self.batch > 0 {
            return;
        }
        let now = self.snapshot();
        if now != self.last_state {
            let before = std::mem::replace(&mut self.last_state, now);
            self.history.record(before);
        }
        let Some(path) = &self.settings_path else {
            return;
        };
        self.prefs.currencies = self.rows.iter().map(|row| row.code.clone()).collect();
        self.prefs.source = self.anchor().map(str::to_string);
        self.prefs.amount = self.amount.map(|a| a.normalize().to_string());
        if let Err(err) = settings::save(path, &self.prefs) {
            log::warn!("could not save settings: {err:#}");
        }
    }
}
