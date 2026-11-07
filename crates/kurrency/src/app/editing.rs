//! Editing the anchor amount: the input's events, the live result of an
//! expression, commit (Enter, blur), revert (Esc) and ↑/↓ stepping.

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::{Context, Entity, Window};
use kurrency_core::expr;
use kurrency_core::present;
use rust_decimal::Decimal;

use super::{EditSnapshot, KurrencyApp};

impl KurrencyApp {
    pub(super) fn on_input_event(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Focus => {
                if self.editing.is_none() {
                    self.begin_editing(window, cx);
                }
                cx.notify();
            }
            InputEvent::Change => self.on_anchor_typed(window, cx),
            InputEvent::PressEnter { .. } => self.commit(window, cx),
            InputEvent::Blur => self.end_editing(window, cx),
        }
    }

    /// Leaving the input commits the expression and shows the formatted
    /// amount again.
    ///
    /// Called on blur, and from `render` when focus has left the input
    /// without one: the input's blur event depends on the previous frame's
    /// focus path, which does not always end at the input's own handle.
    pub(super) fn end_editing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.is_none() {
            return;
        }
        self.commit(window, cx);
        self.editing = None;
        self.echo = None;
        // Keep the window-level actions (Ctrl+K, Ctrl+R) and type-anywhere
        // reachable when focus leaves for nowhere.
        let focus_handle = self.focus_handle.clone();
        window.defer(cx, move |window, cx| {
            if window.focused(cx).is_none() {
                focus_handle.focus(window, cx);
            }
        });
        cx.notify();
    }

    /// Takes the Esc snapshot and puts the raw amount into the input, all
    /// selected, so typing replaces it.
    fn begin_editing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = Some(EditSnapshot {
            amount: self.amount,
        });
        self.echo = None;
        let text = self.raw_anchor_text();
        self.anchor_input.update(cx, |input, cx| {
            input.set_value(text, window, cx);
            input.select_all(window, cx);
        });
    }

    /// The anchor amount as plain digits (`1250`, `1250.5`), for editing.
    fn raw_anchor_text(&self) -> String {
        match (self.amount, self.anchor()) {
            (Some(amount), Some(code)) => self.fmt.raw(code, amount),
            _ => String::new(),
        }
    }

    /// Focuses the anchor input to edit the amount.
    pub(crate) fn focus_anchor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.anchor().is_none() {
            return;
        }
        if self.editing.is_none() {
            self.begin_editing(window, cx);
        }
        let input = self.anchor_input.clone();
        input.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    /// Typing recomputes the rows live.
    pub(super) fn on_anchor_typed(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.is_none() {
            self.editing = Some(EditSnapshot {
                amount: self.amount,
            });
        }
        let text = self.anchor_input.read(cx).value();
        let code = self.anchor().unwrap_or("USD").to_string();
        let style = self.fmt.style;
        match expr::eval_in(&text, style) {
            Ok(value) => {
                self.amount = Some(value);
                self.echo = (!expr::is_plain_number_in(&text, style))
                    .then(|| present::echo_text(value, self.fmt.dp(&code), style).into());
            }
            Err(expr::ExprError::Empty) => {
                self.amount = None;
                self.echo = None;
            }
            // The rows keep their last valid values.
            // TODO(polish): the subtle shake from docs/DESIGN.md "Not yet built".
            Err(_) => self.echo = Some("—".into()),
        }
        cx.notify();
    }

    /// Enter (and blur): replace a valid expression with its result, or put
    /// back the value from before the edit when it is invalid.
    pub(super) fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.is_none() {
            return;
        }
        let text = self.anchor_input.read(cx).value();
        match expr::eval_in(&text, self.fmt.style) {
            Ok(_) | Err(expr::ExprError::Empty) => {
                self.echo = None;
                let raw = self.raw_anchor_text();
                if text != raw {
                    self.anchor_input
                        .update(cx, |input, cx| input.set_value(raw, window, cx));
                }
                if let Some(snapshot) = &mut self.editing {
                    snapshot.amount = self.amount;
                }
                self.save_settings();
            }
            Err(_) => {
                self.revert(window, cx);
            }
        }
        cx.notify();
    }

    /// Esc: restore the amount captured when editing began. Returns false
    /// when there was nothing to revert.
    fn revert(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(snapshot) = self.editing.clone() else {
            return false;
        };
        let raw_before = self.raw_anchor_text();
        let changed =
            snapshot.amount != self.amount || self.anchor_input.read(cx).value() != raw_before;
        self.amount = snapshot.amount;
        self.echo = None;
        let raw = self.raw_anchor_text();
        self.anchor_input
            .update(cx, |input, cx| input.set_value(raw, window, cx));
        cx.notify();
        changed
    }

    /// Esc in the anchor input: revert, and leave the input when there was
    /// nothing to revert.
    pub(super) fn on_anchor_escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.revert(window, cx) {
            self.focus_handle.focus(window, cx);
        }
    }

    /// Sets the anchor amount (quick chips, stepping).
    pub(crate) fn set_amount(
        &mut self,
        amount: Decimal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.amount = Some(amount);
        self.echo = None;
        if self.editing.is_some() {
            let raw = self.raw_anchor_text();
            self.anchor_input.update(cx, |input, cx| {
                input.set_value(raw, window, cx);
            });
        }
        self.save_settings();
        cx.notify();
    }

    pub(super) fn step(
        &mut self,
        up: bool,
        shift: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = self.anchor_input.read(cx).value();
        match present::step_amount(&text, up, shift, self.fmt.style) {
            Some(next) => {
                self.set_amount(next, window, cx);
                if let Some(snapshot) = &mut self.editing {
                    snapshot.amount = self.amount;
                }
            }
            // An expression: ↓ leaves for the list.
            None if !up => {
                self.commit(window, cx);
                self.focus_row(1, window, cx);
            }
            None => {}
        }
    }
}
