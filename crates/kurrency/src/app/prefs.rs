//! Applying the settings page's choices: theme, motion, number format and
//! language, live.

use gpui_kit::{Context, Window};
use kurrency_core::i18n;
use kurrency_core::money;
use kurrency_core::settings::Settings;

use super::KurrencyApp;
use crate::ui;

impl KurrencyApp {
    /// Changes settings from the settings page: applied at once and saved.
    pub(crate) fn update_prefs(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Settings),
    ) {
        let before = self.prefs.clone();
        f(&mut self.prefs);
        if self.prefs == before {
            return;
        }
        self.apply_prefs(Some(&before), window, cx);
        if (before.rate_source, before.refresh_interval)
            != (self.prefs.rate_source, self.prefs.refresh_interval)
        {
            let (source, every) = (self.prefs.rate_source, self.prefs.refresh_interval.secs());
            self.rates
                .update(cx, |rates, cx| rates.configure(source, every, cx));
        }
        self.save_settings();
        cx.notify();
    }

    /// Applies the theme, motion, number and language settings (all of them when
    /// `before` is `None`, at startup).
    pub(super) fn apply_prefs(
        &mut self,
        before: Option<&Settings>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let p = &self.prefs;
        let theme_changed = before.is_none_or(|b| {
            (b.theme_mode, b.theme_light, b.theme_dark)
                != (p.theme_mode, p.theme_light, p.theme_dark)
        });
        if theme_changed {
            self.forced_theme = false;
            ui::theme::apply(p, Some(window), cx);
        }
        let env_reduce =
            std::env::var("KURRENCY_REDUCE_MOTION").is_ok_and(|v| !v.is_empty() && v != "0");
        cx.set_reduce_motion(p.reduce_motion || env_reduce);
        self.fmt = money::Format {
            style: p.number_format.style(self.locale.as_deref()),
            extra_dp: p.decimal_places.extra(),
        };
        // Settings → Language, live: every string is looked up per frame.
        let lang = i18n::Lang::resolve(p.language, self.locale.as_deref());
        if before.is_none() || i18n::lang() != lang {
            i18n::set(lang);
            window.refresh();
        }
    }

    /// Whether a rise is green, for the up/down setting in the UI language.
    pub(crate) fn green_up(&self) -> bool {
        self.prefs
            .up_down_colors
            .green_up(Some(i18n::lang().code()))
    }
}
