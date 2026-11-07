//! The list: converted values, promoting a row into the anchor, adding and
//! removing currencies (with the undo toast), copying, reordering by keyboard
//! or drag, row focus, and the one-shot motion states these play.

use std::time::Duration;

use gpui_kit::{App, AppContext as _, ClipboardItem, Context, Window};
use kurrency_core::i18n::tr;
use kurrency_core::money;
use rust_decimal::Decimal;

use super::{KurrencyApp, Row};
use crate::layout;
use crate::ui;

impl KurrencyApp {
    /// The value of `code` for the current amount, unrounded.
    pub(crate) fn value_of(&self, code: &str, cx: &App) -> Option<Decimal> {
        let amount = self.amount?;
        let anchor = self.anchor()?;
        if code == anchor {
            return Some(amount);
        }
        self.rates.read(cx).table().convert(amount, anchor, code)
    }

    /// The row's value as a raw number for the clipboard (`15832`, `89.56`).
    pub(crate) fn raw_value(&self, code: &str, cx: &App) -> Option<String> {
        let value = self.value_of(code, cx)?;
        Some(self.fmt.raw(code, value))
    }

    /// Makes the list row `ix` the anchor, swapping it with the old anchor.
    /// The new amount is the row's value rounded to its minor units, so
    /// every other row keeps its value.
    pub(crate) fn promote(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix == 0 || ix >= self.rows.len() {
            return;
        }
        if self.editing.is_some() {
            self.commit(window, cx);
            self.editing = None;
            self.echo = None;
        }
        let code = self.rows[ix].code.clone();
        if self.amount.is_some() {
            self.amount = self
                .value_of(&code, cx)
                .map(|v| money::round_to(v, self.fmt.dp(&code)).normalize());
        }
        let version = self.motion.bump();
        self.motion.swap = Some(ui::motion::Swap {
            promoted: code.clone(),
            demoted: self.rows[0].code.clone(),
            version,
        });
        self.expire_motion(version, ui::motion::SWAP, cx);
        self.rows.swap(0, ix);
        // Focus stays on the same slot, which now holds the old anchor.
        let slot = self.rows[ix].focus_handle.clone();
        slot.focus(window, cx);
        self.hovered = None;
        self.save_settings();
        cx.notify();
    }

    pub(crate) fn add_currency(&mut self, code: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.rows.iter().any(|row| row.code == code) {
            return;
        }
        let _ = window;
        self.rows.push(Row {
            code: code.to_string(),
            focus_handle: cx.focus_handle(),
        });
        if self.rows.len() > 1 {
            let version = self.motion.bump();
            self.motion.entering.insert(code.to_string(), version);
            self.expire_motion(version, ui::motion::RESIZE, cx);
        }
        self.save_settings();
        cx.notify();
    }

    pub(crate) fn remove_currency(
        &mut self,
        code: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ix) = self.rows.iter().position(|row| row.code == code) else {
            return;
        };
        // Keep the amount meaningful: the next row becomes the anchor.
        if ix == 0 {
            if self.editing.is_some() {
                self.focus_handle.focus(window, cx);
                self.editing = None;
                self.echo = None;
            }
            if let Some(next) = self.rows.get(1).map(|row| row.code.clone()) {
                self.amount = self
                    .value_of(&next, cx)
                    .map(|v| money::round_to(v, self.fmt.dp(&next)).normalize());
            }
        }
        self.rows.remove(ix);
        if self.hovered.as_deref() == Some(code) {
            self.hovered = None;
        }
        if ix > 0 && !cx.reduce_motion() {
            let version = self.motion.bump();
            self.motion.leaving.push(ui::motion::Leaving {
                code: code.to_string(),
                ix,
                version,
            });
            self.expire_motion(version, ui::motion::RESIZE, cx);
        }
        self.focus_handle.focus(window, cx);
        self.save_settings();
        self.toast_removed(code, window, cx);
        cx.notify();
    }

    /// "EUR removed · Undo", at the bottom of the window.
    fn toast_removed(&mut self, code: &str, _window: &mut Window, cx: &mut Context<Self>) {
        let version = self.motion.bump();
        self.toast = Some(ui::toast::Toast {
            message: tr!("toast.removed", code = code).into(),
            version,
        });
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(ui::toast::SHOW_FOR).await;
            let _ = this.update(cx, |this, cx| {
                if this.toast.as_ref().is_some_and(|t| t.version == version) {
                    this.toast = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(crate) fn dismiss_toast(&mut self, cx: &mut Context<Self>) {
        if self.toast.take().is_some() {
            cx.notify();
        }
    }

    /// Clears a one-shot animation state once it has played.
    fn expire_motion(&self, version: usize, duration: Duration, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(duration + Duration::from_millis(40))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.motion.expire(version);
                cx.notify();
            });
        })
        .detach();
    }

    /// Copies a row's raw value; a check mark flashes in place of its
    /// change chip.
    pub(crate) fn copy_row(&mut self, code: &str, cx: &mut Context<Self>) {
        let Some(raw) = self.raw_value(code, cx) else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(raw));
        let version = self.motion.bump();
        self.motion.copied = Some((code.to_string(), version));
        self.expire_motion(version, ui::motion::COPIED, cx);
        cx.notify();
    }

    /// Moves a list row to the top of the list (just under the anchor).
    pub(crate) fn move_to_top(&mut self, code: &str, cx: &mut Context<Self>) {
        if let Some(ix) = self.rows.iter().position(|row| row.code == code)
            && ix > 1
        {
            let row = self.rows.remove(ix);
            self.rows.insert(1, row);
            self.save_settings();
            cx.notify();
        }
    }

    /// Moves list row `from` to `to` (both 1-based: the anchor stays put).
    /// One undo step; each row in `settle` springs in from its offset (px).
    fn move_row(
        &mut self,
        from: usize,
        to: usize,
        settle: Vec<(String, f32)>,
        cx: &mut Context<Self>,
    ) {
        let len = self.rows.len();
        if from == 0 || to == 0 || from >= len || to >= len {
            return;
        }
        if from != to {
            let row = self.rows.remove(from);
            self.rows.insert(to, row);
            self.save_settings();
        }
        let settle: Vec<_> = settle.into_iter().filter(|(_, y)| y.abs() >= 0.5).collect();
        if !settle.is_empty() && !cx.reduce_motion() {
            let version = self.motion.bump();
            self.motion.settle = Some((settle, version));
            self.expire_motion(version, ui::motion::SPRING, cx);
        }
        cx.notify();
    }

    /// Alt+↑ / Alt+↓: moves the focused row one place, keeping its focus.
    pub(super) fn move_focused(&mut self, down: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.page.is_some() {
            return;
        }
        let Some(from) = self.focused_row(window) else {
            return;
        };
        let to = if down { from + 1 } else { from - 1 };
        if to == 0 || to >= self.rows.len() {
            return;
        }
        let row = layout::metrics::ROW;
        let (moved, other) = (self.rows[from].code.clone(), self.rows[to].code.clone());
        let sign = if down { 1. } else { -1. };
        self.move_row(
            from,
            to,
            vec![(moved, -sign * row), (other, sign * row)],
            cx,
        );
    }

    /// A row's grip started a drag, held `grab_y` px below the row's top.
    pub(crate) fn start_row_drag(
        &mut self,
        code: &str,
        grab_y: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(from) = self.rows.iter().position(|row| row.code == code) else {
            return;
        };
        if from == 0 {
            return;
        }
        let pointer_y = f32::from(window.mouse_position().y);
        let slot = from - 1;
        self.drag = Some(ui::row::RowDrag {
            code: code.to_string(),
            slot,
            previous: None,
            version: self.motion.bump(),
            grab_y,
            pointer_y,
            list_top: pointer_y - grab_y - slot as f32 * layout::metrics::ROW,
        });
        self.hovered = None;
        self.motion.settle = None;
        cx.notify();
    }

    /// `KURRENCY_DRAG`: lifts `code` as if dragged a third of a row past
    /// list slot `slot`, and keeps it there.
    pub(super) fn debug_drag(
        &mut self,
        code: &str,
        slot: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use layout::metrics::*;
        let Some(from) = self
            .rows
            .iter()
            .position(|row| row.code == code)
            .filter(|&ix| ix > 0)
        else {
            return;
        };
        let list_top = HEADER + 2. * RING + CARD + CHIPS_GAP + CHIPS + LIST_GAP;
        let grab_y = ROW / 2.;
        let slot = slot.min(self.rows.len() - 2);
        self.drag = Some(ui::row::RowDrag {
            code: code.to_string(),
            slot,
            previous: (slot != from - 1).then_some(from - 1),
            version: self.motion.bump(),
            grab_y,
            pointer_y: list_top + slot as f32 * ROW + grab_y + ROW / 4.,
            list_top,
        });
        self.hovered = None;
        let view = ui::row::RowDragView::new(self, code, window, cx);
        self.debug_lifted = Some(cx.new(|_| view).into());
        cx.notify();
    }

    /// The pointer moved during a row drag: the drop slot follows it.
    pub(crate) fn drag_row_to(&mut self, pointer_y: f32, list_top: f32, cx: &mut Context<Self>) {
        let count = self.rows.len().saturating_sub(1);
        let Some(drag) = &mut self.drag else {
            return;
        };
        drag.pointer_y = pointer_y;
        drag.list_top = list_top;
        let slot = ui::row::drop_slot(pointer_y, drag.grab_y, list_top, count);
        if slot != drag.slot {
            drag.previous = Some(drag.slot);
            drag.slot = slot;
            drag.version = self.motion.bump();
            cx.notify();
        }
    }

    /// The dragged row was let go: it moves to its drop slot and springs
    /// from where it was let go into place.
    pub(super) fn drop_row(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        let Some(from) = self.rows.iter().position(|row| row.code == drag.code) else {
            return;
        };
        let slot_top = drag.list_top + drag.slot as f32 * layout::metrics::ROW;
        let offset = drag.pointer_y - drag.grab_y - slot_top;
        self.move_row(from, drag.slot + 1, vec![(drag.code, offset)], cx);
    }

    /// Focuses list row `ix` (1-based: 0 is the anchor).
    pub(super) fn focus_row(&self, ix: usize, window: &mut Window, cx: &mut App) {
        if let Some(row) = self.rows.get(ix).filter(|_| ix > 0) {
            row.focus_handle.focus(window, cx);
        }
    }

    /// The list row with keyboard focus.
    pub(super) fn focused_row(&self, window: &Window) -> Option<usize> {
        self.rows
            .iter()
            .enumerate()
            .skip(1)
            .find(|(_, row)| row.focus_handle.is_focused(window))
            .map(|(ix, _)| ix)
    }

    /// Records what every amount display shows, for the digit rolls.
    pub(super) fn update_rolls(&mut self, cx: &App) {
        let mut shown = Vec::with_capacity(self.rows.len());
        if let (Some(code), Some(amount), false) = (self.anchor(), self.amount, self.is_editing()) {
            shown.push((
                ui::anchor::ANCHOR_ROLL.to_string(),
                ui::anchor::amount_digits(code, amount, &self.fmt),
            ));
        }
        for row in self.rows.iter().skip(1) {
            if let Some(value) = self.value_of(&row.code, cx) {
                shown.push((
                    row.code.clone(),
                    ui::anchor::amount_digits(&row.code, value, &self.fmt),
                ));
            }
        }
        self.motion.update_rolls(shown);
    }
}
