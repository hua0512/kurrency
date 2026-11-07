//! Debug-build environment variables that put the window into a given state
//! for screenshots.

use std::time::Duration;

use gpui_kit::component::ThemeMode;
use gpui_kit::{Context, Window};
use kurrency_core::settings;

use super::{KurrencyApp, PageView};
use crate::ui;

impl KurrencyApp {
    /// Debug builds only, for screenshots:
    /// - `KURRENCY_THEME=dark|light` forces the theme;
    /// - `KURRENCY_EDIT=1.5k*3` starts editing the anchor with that text;
    /// - `KURRENCY_HOVER=EUR` shows that row's hover state;
    /// - `KURRENCY_OPEN_DETAIL=EUR` opens that row's detail page;
    /// - `KURRENCY_FIT_AFTER_MS=3000` fits the window height again after a
    ///   delay (after a tiling compositor has been told to float it);
    /// - `KURRENCY_PALETTE="50 gbp"` opens the palette with that query;
    /// - `KURRENCY_REMOVE=JPY` removes that row after 3s (the undo toast);
    /// - `KURRENCY_OPEN_SETTINGS=1` opens the settings page, scrolled down
    ///   by `KURRENCY_SETTINGS_SCROLL` px;
    /// - `KURRENCY_SETTINGS_THEME=light:slate` sets the theme mode and that
    ///   mode's theme (in memory only, unless a setting is changed);
    /// - `KURRENCY_DRAG=GBP:0` freezes that row lifted mid-drag over list
    ///   slot 0 after 3s (a still of the drag, which a screenshot cannot
    ///   otherwise catch without a pointer);
    /// - `KURRENCY_POINTER=chart:0.6` (or `row:2`) puts a synthetic pointer
    ///   there after 3.5s: the history chart's crosshair, a row's hover;
    /// - `KURRENCY_DEMO=1` plays the scripted tour (`demo.rs`) after
    ///   `KURRENCY_DEMO_DELAY_MS` (3000 by default).
    ///
    /// `KURRENCY_NOW` (the clock), `KURRENCY_CONFIG_DIR` and
    /// `KURRENCY_CACHE_DIR` are read by `kurrency-core`.
    pub(super) fn apply_debug_env(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !cfg!(debug_assertions) {
            return;
        }
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
        if let Some(theme) = var("KURRENCY_THEME") {
            let mode = if theme == "light" {
                ThemeMode::Light
            } else {
                ThemeMode::Dark
            };
            self.forced_theme = true;
            gpui_kit::component::Theme::change(mode, Some(window), cx);
        }
        if let Some(code) = var("KURRENCY_HOVER") {
            self.hovered = Some(code.to_ascii_uppercase());
        }
        if let Some(text) = var("KURRENCY_EDIT") {
            cx.defer_in(window, move |this, window, cx| {
                this.focus_anchor(window, cx);
                this.anchor_input
                    .update(cx, |input, cx| input.set_value(text, window, cx));
                this.on_anchor_typed(window, cx);
            });
        }
        if let Some(ms) = var("KURRENCY_FIT_AFTER_MS").and_then(|ms| ms.parse().ok()) {
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(ms))
                    .await;
                let _ = this.update_in(cx, |this, window, cx| this.fit_window(true, window, cx));
            })
            .detach();
        }
        if let Some(query) = var("KURRENCY_PALETTE") {
            cx.defer_in(window, move |this, window, cx| {
                ui::picker::open(this, &query, window, cx);
            });
        }
        if let Some(code) = var("KURRENCY_REMOVE") {
            // Late enough for the toast to be on screen at a screenshot.
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(3000))
                    .await;
                let _ = this.update_in(cx, |this, window, cx| {
                    this.remove_currency(&code.to_ascii_uppercase(), window, cx);
                });
            })
            .detach();
        }
        if var("KURRENCY_OPEN_SETTINGS").is_some() {
            let scroll: f32 = var("KURRENCY_SETTINGS_SCROLL")
                .and_then(|y| y.parse().ok())
                .unwrap_or(0.);
            cx.defer_in(window, |this, window, cx| {
                this.open_settings_page(window, cx);
            });
            // After the window has been fitted and laid out.
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(3000))
                    .await;
                let _ = this.update(cx, |this, cx| {
                    if let Some(PageView::Settings(view)) = this.page.as_ref().map(|p| &p.view) {
                        view.read(cx).scroll_by(scroll);
                    }
                    cx.notify();
                });
            })
            .detach();
        }
        if let Some(theme) = var("KURRENCY_SETTINGS_THEME") {
            // `light:slate`, `dark:ink`: the mode and that slot's theme.
            let (mode, id) = theme.split_once(':').unwrap_or((theme.as_str(), ""));
            let id = settings::ThemeId::ALL
                .into_iter()
                .find(|t| t.label().eq_ignore_ascii_case(id));
            let mode = if mode == "dark" {
                settings::ThemeMode::Dark
            } else {
                settings::ThemeMode::Light
            };
            self.forced_theme = false;
            self.prefs.theme_mode = mode;
            match id {
                Some(id) if id.is_dark() => self.prefs.theme_dark = id,
                Some(id) => self.prefs.theme_light = id,
                None => {}
            }
            ui::theme::apply(&self.prefs, Some(window), cx);
        }
        if let Some(spec) = var("KURRENCY_DRAG") {
            let (code, slot) = spec.split_once(':').unwrap_or((spec.as_str(), "0"));
            let (code, slot) = (code.to_ascii_uppercase(), slot.parse().unwrap_or(0));
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(3000))
                    .await;
                let _ = this.update_in(cx, |this, window, cx| {
                    this.debug_drag(&code, slot, window, cx);
                });
            })
            .detach();
        }
        #[cfg(debug_assertions)]
        if let Some(target) = var("KURRENCY_POINTER").and_then(|t| super::demo::parse_target(&t)) {
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(3500))
                    .await;
                let Ok(at) = this.update_in(cx, |this, window, _| this.demo_point(target, window))
                else {
                    return;
                };
                let _ =
                    cx.update(|window, cx| super::demo::Input::move_to(at).dispatch(window, cx));
            })
            .detach();
        }
        #[cfg(debug_assertions)]
        if var("KURRENCY_DEMO").is_some_and(|v| v != "0") {
            let delay = var("KURRENCY_DEMO_DELAY_MS")
                .and_then(|ms| ms.parse().ok())
                .unwrap_or(3000);
            self.start_demo(Duration::from_millis(delay), window, cx);
        }
        if let Some(code) = var("KURRENCY_OPEN_DETAIL") {
            cx.defer_in(window, move |this, window, cx| {
                this.open_detail(&code.to_ascii_uppercase(), window, cx);
            });
        }
    }
}
