//! The root view: the anchor card, the read-only list, their editing model,
//! and the window-level actions (docs/DESIGN.md §1–§3).
//!
//! `rows[0]` is the anchor, the one currency with an amount input; the other
//! rows are read-only conversions of it. Promoting a row swaps it with the
//! anchor, so the old anchor drops into the row's place.
//!
//! The view's state is defined here; its behaviour is split by
//! responsibility over the submodules (`impl KurrencyApp` blocks): actions
//! and key bindings, editing the anchor, the list, palette commands and smart
//! paste, undo and saving, pages, settings, the window, and rendering.

mod actions;
mod changes;
mod commands;
mod debug_env;
#[cfg(debug_assertions)]
mod demo;
mod editing;
mod pages;
mod prefs;
mod render;
mod rows;
#[cfg(test)]
mod tests;
mod window;

use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use gpui_kit::component::input::InputState;
use gpui_kit::{
    AnyView, App, AppContext as _, Context, Entity, FocusHandle, Focusable, SharedString,
    Subscription, Task, Window,
};
use kurrency_core::money;
use kurrency_core::rates::history;
use kurrency_core::settings::{self, Settings};
use kurrency_core::undo;
use rust_decimal::Decimal;

use crate::rates::store::RatesStore;
use crate::ui;

pub(crate) use actions::{
    ANCHOR_CONTEXT, OpenPicker, OpenRowDetail, OpenSettings, ROW_CONTEXT, RefreshRates, SwapAnchor,
    init,
};
pub(crate) use window::window_options;

/// One favourite currency. `rows[0]` is the anchor.
pub(crate) struct Row {
    pub code: String,
    /// Keyboard focus for the read-only row (↑/↓, Enter, Ctrl+C).
    pub focus_handle: FocusHandle,
}

/// The state captured when the anchor input gains focus, restored by Esc.
#[derive(Clone)]
struct EditSnapshot {
    amount: Option<Decimal>,
}

pub(crate) struct KurrencyApp {
    focus_handle: FocusHandle,
    pub(crate) rates: Entity<RatesStore>,
    pub(crate) rows: Vec<Row>,
    /// The amount in the anchor currency; `None` when the field is empty.
    pub(crate) amount: Option<Decimal>,
    /// The anchor's only amount input, shown while editing.
    pub(crate) anchor_input: Entity<InputState>,
    /// The live result under an expression ("= 4,500"), or "—" when invalid.
    pub(crate) echo: Option<SharedString>,
    /// Set while the anchor input is being edited.
    editing: Option<EditSnapshot>,
    /// The row under the mouse, which shows its inverse rate.
    pub(crate) hovered: Option<String>,
    /// The row being dragged to a new place.
    pub(crate) drag: Option<ui::row::RowDrag>,
    pub(crate) picker: ui::picker::Picker,
    settings_path: Option<PathBuf>,
    /// Where history is cached; `None` keeps it in memory (tests).
    history_dir: Option<PathBuf>,
    /// Everything in settings.json; the list, anchor and amount in it are
    /// refreshed from `rows` and `amount` when saving.
    pub(crate) prefs: Settings,
    /// How amounts are written (Settings → Number format, Decimal places).
    pub(crate) fmt: money::Format,
    pub(crate) locale: Option<String>,
    /// The open page (history or settings).
    page: Option<Page>,
    /// Bumped on every open so the slide-in animation replays.
    page_opens: usize,
    /// The page sliding out after closing.
    closing: Option<(AnyView, usize)>,
    /// The number of currencies the window height was last fitted to.
    fitted_for: Option<usize>,
    /// Undo and redo of the list and amount.
    history: undo::History,
    /// The state last saved, which the next change is recorded against.
    last_state: undo::Snapshot,
    /// Nested [`Self::batch`] calls: one undo step for several changes.
    batch: usize,
    /// Saves the window's frame a moment after it stops moving.
    _save_bounds: Option<Task<()>>,
    pub(crate) motion: ui::motion::Motion,
    /// The undo toast on screen.
    pub(crate) toast: Option<ui::toast::Toast>,
    /// A debug theme override (`KURRENCY_THEME`), which stops system sync.
    forced_theme: bool,
    /// `KURRENCY_DRAG` (debug builds): the lifted row, drawn without a real
    /// pointer drag.
    debug_lifted: Option<AnyView>,
    /// `KURRENCY_DEMO` (debug builds): the scripted tour in progress.
    #[cfg(debug_assertions)]
    demo: Option<demo::Demo>,
    /// Re-renders the relative age and re-checks staleness every minute.
    _ticker: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

/// A full-window page over the list.
enum PageView {
    Detail(Entity<ui::detail::DetailView>),
    Settings(Entity<ui::settings::SettingsView>),
}

impl PageView {
    fn any(&self) -> AnyView {
        match self {
            Self::Detail(view) => view.clone().into(),
            Self::Settings(view) => view.clone().into(),
        }
    }
}

struct Page {
    view: PageView,
    /// The row a history page was opened from, focused again on close.
    from_row: Option<String>,
    _subscription: Subscription,
}

impl KurrencyApp {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let locale = sys_locale::get_locale();
        let settings_path = settings::default_path();
        let settings = settings::load(settings_path.as_deref(), locale.as_deref());
        log::info!("locale {locale:?}, currencies {:?}", settings.currencies);

        let rates = cx.new(RatesStore::new);
        let mut this = Self::with_state(settings, settings_path, rates.clone(), window, cx);
        this.locale = locale;
        this.history_dir = history::cache_dir();
        this.apply_prefs(None, window, cx);
        let (source, every) = (this.prefs.rate_source, this.prefs.refresh_interval.secs());
        rates.update(cx, |rates, cx| rates.configure(source, every, cx));
        this._subscriptions
            .push(cx.observe_window_bounds(window, KurrencyApp::window_moved));
        let app = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            let _ = app.update(cx, |this, _| this.remember_bounds(window));
            true
        });
        this._ticker = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(60))
                    .await;
                let alive = this.update(cx, |this, cx| {
                    this.rates.update(cx, RatesStore::refresh_if_stale);
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
            }
        }));
        this.apply_debug_env(window, cx);
        this
    }

    /// Builds the view from loaded settings and a rates store. Settings are
    /// saved to `settings_path` when it is set.
    pub(crate) fn with_state(
        settings: Settings,
        settings_path: Option<PathBuf>,
        rates: Entity<RatesStore>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut subscriptions = vec![cx.observe(&rates, |_, _, cx| cx.notify())];
        subscriptions.push(cx.observe_window_appearance(window, |this, window, cx| {
            if !this.forced_theme && this.prefs.theme_mode == settings::ThemeMode::System {
                ui::theme::sync_with_system(window, cx);
            }
        }));

        let anchor_input = cx.new(|cx| InputState::new(window, cx).placeholder("0"));
        subscriptions.push(cx.subscribe_in(&anchor_input, window, Self::on_input_event));

        let amount = settings
            .amount
            .as_deref()
            .and_then(|a| Decimal::from_str(a).ok());
        let mut rows: Vec<Row> = settings
            .currencies
            .iter()
            .map(|code| Row {
                code: code.clone(),
                focus_handle: cx.focus_handle(),
            })
            .collect();
        // The anchor is always first (v1 settings kept it anywhere).
        if let Some(source) = &settings.source
            && let Some(ix) = rows.iter().position(|row| &row.code == source)
        {
            rows.swap(0, ix);
        }

        let last_state = undo::Snapshot {
            currencies: rows.iter().map(|row| row.code.clone()).collect(),
            amount,
        };
        let this = Self {
            focus_handle: cx.focus_handle(),
            rates,
            rows,
            amount,
            anchor_input,
            echo: None,
            editing: None,
            hovered: None,
            drag: None,
            picker: ui::picker::Picker::new(window, cx),
            settings_path,
            history_dir: None,
            fmt: money::Format {
                style: settings.number_format.style(None),
                extra_dp: settings.decimal_places.extra(),
            },
            prefs: settings,
            locale: None,
            page: None,
            page_opens: 0,
            closing: None,
            fitted_for: None,
            history: undo::History::default(),
            last_state,
            batch: 0,
            _save_bounds: None,
            motion: Default::default(),
            toast: None,
            forced_theme: false,
            debug_lifted: None,
            #[cfg(debug_assertions)]
            demo: None,
            _ticker: None,
            _subscriptions: subscriptions,
        };
        this.focus_handle.focus(window, cx);
        this
    }

    /// The anchor currency, when there is any currency at all.
    pub(crate) fn anchor(&self) -> Option<&str> {
        self.rows.first().map(|row| row.code.as_str())
    }

    pub(crate) fn is_editing(&self) -> bool {
        self.editing.is_some()
    }

    pub(crate) fn anchor_focused(&self, window: &Window, cx: &App) -> bool {
        self.anchor_input
            .read(cx)
            .focus_handle(cx)
            .is_focused(window)
    }
}

impl Focusable for KurrencyApp {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
