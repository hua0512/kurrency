//! The window-level actions, their key bindings (Cmd on macOS, Ctrl
//! elsewhere), the key contexts, and the handlers that route an action or a
//! keystroke to the view (type-anywhere, DESIGN §2).

use gpui_kit::{Action, App, Context, KeyBinding, KeyDownEvent, SharedString, Window, actions};
use kurrency_core::present::{self, TypeAnywhere};

use super::KurrencyApp;
use crate::rates::store::RatesStore;
use crate::ui;

actions!(
    kurrency,
    [
        RefreshRates,
        OpenPicker,
        OpenDetail,
        SelectPrev,
        SelectNext,
        Promote,
        CopyValue,
        SwapAnchor,
        StepUp,
        StepDown,
        StepUpTen,
        StepDownTen,
        OpenSettings,
        SmartPaste,
        UndoChange,
        RedoChange,
        MoveRowUp,
        MoveRowDown,
    ]
);

/// Opens the detail page of a row (clicking its change chip).
#[derive(Clone, PartialEq, Action)]
#[action(namespace = kurrency, no_json)]
pub(crate) struct OpenRowDetail {
    pub code: SharedString,
}

/// The key context of the root view.
pub(super) const CONTEXT: &str = "Kurrency";
/// The key context of a focused read-only row.
pub(crate) const ROW_CONTEXT: &str = "KurrencyRow";
/// The key context around the anchor's amount input.
pub(crate) const ANCHOR_CONTEXT: &str = "KurrencyAnchor";

/// `cmd-{key}` on macOS, `ctrl-{key}` elsewhere. (`KeyBinding::new` rejects
/// GPUI's `secondary-` modifier: it needs the OS, which only `KeyBinding::load`
/// is given.)
pub(crate) fn shortcut(key: &'static str) -> &'static str {
    let mac = cfg!(target_os = "macos");
    match key {
        "k" if mac => "cmd-k",
        "k" => "ctrl-k",
        "r" if mac => "cmd-r",
        "r" => "ctrl-r",
        "c" if mac => "cmd-c",
        "c" => "ctrl-c",
        "," if mac => "cmd-,",
        "," => "ctrl-,",
        "enter" if mac => "cmd-enter",
        "enter" => "ctrl-enter",
        "right" if mac => "cmd-right",
        "right" => "ctrl-right",
        "v" if mac => "cmd-v",
        "v" => "ctrl-v",
        "z" if mac => "cmd-z",
        "z" => "ctrl-z",
        "shift-z" if mac => "cmd-shift-z",
        "shift-z" => "ctrl-shift-z",
        other => other,
    }
}

/// Registers the window-level key bindings (Cmd on macOS, Ctrl elsewhere).
pub(crate) fn init(cx: &mut App) {
    let anchor_input = "KurrencyAnchor > Input";
    cx.bind_keys([
        // No context: these work from inside the amount input too.
        KeyBinding::new(shortcut("r"), RefreshRates, None),
        KeyBinding::new(shortcut("k"), OpenPicker, None),
        KeyBinding::new(shortcut(","), OpenSettings, None),
        // ↑/↓ move between the rows (the input overrides them below).
        KeyBinding::new("up", SelectPrev, Some(CONTEXT)),
        KeyBinding::new("down", SelectNext, Some(CONTEXT)),
        // Smart paste and undo outside the anchor input; inside it the
        // input's own paste and text undo win, being deeper.
        KeyBinding::new(shortcut("v"), SmartPaste, Some(CONTEXT)),
        KeyBinding::new(shortcut("z"), UndoChange, Some(CONTEXT)),
        KeyBinding::new(shortcut("shift-z"), RedoChange, Some(CONTEXT)),
        KeyBinding::new("enter", Promote, Some(ROW_CONTEXT)),
        KeyBinding::new(shortcut("c"), CopyValue, Some(ROW_CONTEXT)),
        KeyBinding::new(shortcut("enter"), OpenDetail, Some(ROW_CONTEXT)),
        KeyBinding::new(shortcut("right"), OpenDetail, Some(ROW_CONTEXT)),
        // The keyboard way to reorder (dragging the grip is the other).
        KeyBinding::new("alt-up", MoveRowUp, Some(ROW_CONTEXT)),
        KeyBinding::new("alt-down", MoveRowDown, Some(ROW_CONTEXT)),
        // In the anchor input. Registered after gpui_kit::init so these win
        // over the input's own bindings (move up/down, next word, ...).
        KeyBinding::new("up", StepUp, Some(anchor_input)),
        KeyBinding::new("down", StepDown, Some(anchor_input)),
        KeyBinding::new("shift-up", StepUpTen, Some(anchor_input)),
        KeyBinding::new("shift-down", StepDownTen, Some(anchor_input)),
        KeyBinding::new(shortcut("enter"), OpenDetail, Some(anchor_input)),
        KeyBinding::new(shortcut("right"), OpenDetail, Some(anchor_input)),
        KeyBinding::new(
            shortcut("shift-z"),
            gpui_kit::component::input::Redo,
            Some(anchor_input),
        ),
    ]);
    ui::detail::init(cx);
    ui::settings::init(cx);
}

impl KurrencyApp {
    pub(super) fn refresh_rates(
        &mut self,
        _: &RefreshRates,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.rates.update(cx, RatesStore::refresh);
    }

    pub(super) fn open_settings(
        &mut self,
        _: &OpenSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_settings_page(window, cx);
    }

    pub(super) fn open_picker(
        &mut self,
        _: &OpenPicker,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        ui::picker::open(self, "", window, cx);
    }

    pub(super) fn swap_anchor(
        &mut self,
        _: &SwapAnchor,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.promote(1, window, cx);
    }

    pub(super) fn select_prev(
        &mut self,
        _: &SelectPrev,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page.is_some() {
            return;
        }
        match self.focused_row(window) {
            Some(1) => self.focus_anchor(window, cx),
            Some(ix) => self.focus_row(ix - 1, window, cx),
            None => self.focus_row(self.rows.len().saturating_sub(1), window, cx),
        }
    }

    pub(super) fn select_next(
        &mut self,
        _: &SelectNext,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page.is_some() {
            return;
        }
        match self.focused_row(window) {
            Some(ix) => self.focus_row(ix + 1, window, cx),
            None => self.focus_row(1, window, cx),
        }
    }

    pub(super) fn promote_focused(
        &mut self,
        _: &Promote,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(ix) = self.focused_row(window) {
            self.promote(ix, window, cx);
        }
    }

    pub(super) fn copy_focused(
        &mut self,
        _: &CopyValue,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(ix) = self.focused_row(window) {
            let code = self.rows[ix].code.clone();
            self.copy_row(&code, cx);
        }
    }

    pub(super) fn open_focused_detail(
        &mut self,
        _: &OpenDetail,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let code = match self.focused_row(window) {
            Some(ix) => self.rows[ix].code.clone(),
            None if self.anchor_focused(window, cx) => match self.anchor() {
                Some(code) => code.to_string(),
                None => return,
            },
            None => return,
        };
        self.open_detail(&code, window, cx);
    }

    /// Type-anywhere: with no text input focused, a digit starts editing the
    /// anchor and a letter opens the palette with it (DESIGN §2).
    pub(super) fn on_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page.is_some() || self.anchor_focused(window, cx) {
            return;
        }
        let m = &event.keystroke.modifiers;
        let blocked = m.control || m.alt || m.platform || m.function;
        let decimal = self.fmt.style.decimal;
        match present::classify_key(event.keystroke.key_char.as_deref(), blocked, decimal) {
            TypeAnywhere::Amount(ch) => {
                if self.anchor().is_none() {
                    return;
                }
                self.focus_anchor(window, cx);
                self.anchor_input.update(cx, |input, cx| {
                    input.set_value(ch.to_string(), window, cx);
                });
                self.on_anchor_typed(window, cx);
                cx.stop_propagation();
            }
            TypeAnywhere::Search(ch) => {
                ui::picker::open(self, &ch.to_string(), window, cx);
                cx.stop_propagation();
            }
            TypeAnywhere::Ignore => {}
        }
    }

    pub(super) fn open_row_detail(
        &mut self,
        action: &OpenRowDetail,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Releasing a dragged row over a change chip does not open it.
        if self.drag.is_some() {
            return;
        }
        self.open_detail(&action.code, window, cx);
    }

    pub(super) fn on_smart_paste(
        &mut self,
        _: &SmartPaste,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page.is_none() {
            self.smart_paste(false, window, cx);
        }
    }
}
