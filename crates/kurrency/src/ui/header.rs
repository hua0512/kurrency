//! The header (DESIGN §1, §3): the "Kurrency" title, the freshness dot
//! with the relative age, a refresh button, and the gear. Replaces the v1
//! footer: hover the dot for the source, date and next refresh; click it to
//! see a failure, or to refresh.

use std::f32::consts::PI;
use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{
    Disableable as _, IconName, Sizable as _, TitleBar, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Animation, AnimationExt as _, Context, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _, div, ease_out_quint, px,
};
use kurrency_core::i18n::tr;
use kurrency_core::rates::{Dot, Freshness};

use crate::app::{KurrencyApp, OpenSettings, RefreshRates};
use crate::layout::metrics;
use crate::ui::theme::Tokens;

/// A short pulse each time an update starts (the motion rule's 250ms cap);
/// the hollow "updating" ring stays for as long as the fetch lasts.
const PULSE: Duration = Duration::from_millis(240);
const DOT: f32 = 7.;

/// Ctrl+R, or ⌘R on macOS.
const REFRESH_SHORTCUT: &str = if cfg!(target_os = "macos") {
    "⌘R"
} else {
    "Ctrl+R"
};

pub(crate) fn render(app: &KurrencyApp, cx: &mut Context<KurrencyApp>) -> impl IntoElement {
    let t = Tokens::get(cx);
    let rates = app.rates.read(cx);
    let freshness = rates.freshness(app.rows.iter().map(|row| row.code.as_str()));
    let refreshing = rates.is_refreshing();
    let breaths = rates.refreshes();
    let refresh_hint = tr!("header.refresh", shortcut = REFRESH_SHORTCUT);

    TitleBar::new()
        .h(px(metrics::HEADER))
        .pl(px(metrics::PAD_X))
        .bg(t.bg)
        .border_color(t.bg)
        .child(
            h_flex()
                .id("header")
                .group("header")
                .w_full()
                .pr(px(metrics::PAD_X - 6.))
                .justify_between()
                .child(
                    div()
                        .text_size(px(16.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Kurrency"),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .child(
                            // Revealed on hover; Ctrl+R works any time.
                            div()
                                .opacity(0.)
                                .group_hover("header", |s| s.opacity(1.))
                                .child(
                                    Button::new("refresh-rates")
                                        .ghost()
                                        .xsmall()
                                        .icon(IconName::RefreshCw)
                                        .text_color(t.muted)
                                        .disabled(refreshing)
                                        .tooltip(refresh_hint)
                                        // `gpui_kit`'s `TitleBar` marks the whole
                                        // header as the window's drag area, which
                                        // Windows treats as the native caption: a
                                        // left click there is consumed by the system
                                        // and the button never gets its mouse-up.
                                        // Occluding the control keeps the caption
                                        // hitbox out from under it.
                                        .occlude()
                                        .on_click(|_, window, cx| {
                                            window.dispatch_action(Box::new(RefreshRates), cx);
                                        }),
                                ),
                        )
                        .child(render_freshness(freshness, breaths, &t))
                        .child(
                            Button::new("settings")
                                .ghost()
                                .small()
                                .icon(IconName::Settings)
                                .text_color(t.text)
                                .tooltip(tr!("header.settings"))
                                // See the refresh button above: without this the
                                // header's native caption area eats the click.
                                .occlude()
                                .on_click(|_, window, cx| {
                                    window.dispatch_action(Box::new(OpenSettings), cx);
                                }),
                        ),
                ),
        )
}

fn render_freshness(f: Freshness, breaths: usize, t: &Tokens) -> impl IntoElement {
    let color = match f.dot {
        Dot::Fresh | Dot::Offline => t.accent,
        Dot::Stale => t.amber,
    };
    // Updating: a hollow ring in the dot's colour, at lower opacity, for as
    // long as the fetch lasts. Offline: a hollow muted ring.
    let dot = div()
        .size(px(DOT))
        .rounded_full()
        .map(|el| match (f.updating, f.dot) {
            (true, _) => el.border(px(1.5)).border_color(color.opacity(0.7)),
            (false, Dot::Offline) => el.border_1().border_color(t.muted),
            (false, _) => el.bg(color),
        });
    let dot = if f.updating {
        dot.with_animation(
            ("freshness-pulse", breaths),
            Animation::new(PULSE).with_easing(ease_out_quint()),
            |el, p| el.opacity(1. - 0.6 * (p * PI).sin()),
        )
        .into_any_element()
    } else {
        dot.into_any_element()
    };

    let tooltip = f.clone();
    let error = f.error.clone();
    h_flex()
        .id("freshness")
        // The header is the window's native drag area; occlude the clickable
        // parts (see the refresh button) so the caption does not eat clicks.
        .occlude()
        .h(px(28.))
        .px_1p5()
        .gap(px(6.))
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|s| s.bg(t.hover))
        .child(dot)
        .child(
            div()
                .text_size(px(13.))
                .text_color(t.muted)
                .child(f.age.clone()),
        )
        .tooltip(move |window, cx| {
            let f = tooltip.clone();
            Tooltip::element(move |_, _| {
                v_flex()
                    .gap_0p5()
                    .text_size(px(12.))
                    .child(f.source_line.clone())
                    .child(div().opacity(0.7).child(f.status_line.clone()))
                    .child(div().opacity(0.7).child(if f.error.is_some() {
                        tr!("header.click_for_details")
                    } else {
                        tr!("header.click_to_refresh", shortcut = REFRESH_SHORTCUT)
                    }))
            })
            .build(window, cx)
        })
        // A failure shows its details; otherwise the dot refreshes.
        .on_click(move |_, window, cx| match &error {
            Some(message) => window.push_notification(
                Notification::warning(message.clone()).title(tr!("header.update_failed")),
                cx,
            ),
            None => window.dispatch_action(Box::new(RefreshRates), cx),
        })
}
