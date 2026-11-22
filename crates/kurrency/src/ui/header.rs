//! The unified single-layer TitleBar (DESIGN §1, §3, §6, §7):
//! - on the list: "Kurrency", a morphed freshness/refresh pill, and Settings;
//! - on the history page: back button, pair flags & codes, loading spinner and ⇄ flip;
//! - on Settings: back button and "Settings".

use std::f32::consts::PI;
use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, IconName, Sizable as _, TitleBar, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Animation, AnimationExt as _, Context, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _, div, ease_out_quint, px,
};
use kurrency_core::i18n::tr;
use kurrency_core::rates::{Dot, Freshness};

use crate::app::{HeaderPage, KurrencyApp, OpenSettings, RefreshRates};
use crate::layout::metrics;
use crate::ui::detail::flip_icon;
use crate::ui::flag::flag;
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
    let content = match app.header_page() {
        None => render_main(app, &t, cx).into_any_element(),
        Some(HeaderPage::Detail(view)) => {
            let view = view.clone();
            let (base, quote, busy) = view.read_with(cx, |v, _| {
                (
                    v.base().to_string(),
                    v.quote().to_string(),
                    v.is_busy_with_chart(),
                )
            });
            h_flex()
                .id("header-detail")
                .w_full()
                .pl(px(metrics::PAD_X - 8.))
                .pr(px(metrics::PAD_X - 6.))
                .gap_1()
                .child(
                    Button::new("detail-back")
                        .ghost()
                        .small()
                        .icon(IconName::ChevronLeft)
                        .text_color(t.text)
                        .tooltip(tr!("detail.back"))
                        .occlude()
                        .on_click(cx.listener(|this, _, window, cx| this.close_page(window, cx))),
                )
                .child(
                    h_flex()
                        .flex_1()
                        .min_w(px(0.))
                        .gap(px(6.))
                        .items_center()
                        .child(flag(&base, cx))
                        .child(
                            div()
                                .text_size(px(15.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(base),
                        )
                        .child(div().text_size(px(14.)).text_color(t.muted).child("/"))
                        .child(flag(&quote, cx))
                        .child(
                            div()
                                .text_size(px(15.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(quote),
                        ),
                )
                .when(busy, |el| el.child(Spinner::new().xsmall().color(t.muted)))
                .child(
                    Button::new("detail-flip")
                        .ghost()
                        .small()
                        .icon(flip_icon())
                        .text_color(t.text)
                        .tooltip(tr!("detail.flip"))
                        .occlude()
                        .on_click(move |_, _, cx| {
                            view.update(cx, crate::ui::detail::DetailView::flip);
                        }),
                )
                .into_any_element()
        }
        Some(HeaderPage::Settings) => h_flex()
            .id("header-settings")
            .w_full()
            .pl(px(metrics::PAD_X - 8.))
            .pr(px(metrics::PAD_X - 6.))
            .gap_1()
            .child(
                Button::new("settings-back")
                    .ghost()
                    .small()
                    .icon(IconName::ChevronLeft)
                    .text_color(t.text)
                    .tooltip(tr!("settings.back"))
                    .occlude()
                    .on_click(cx.listener(|this, _, window, cx| this.close_page(window, cx))),
            )
            .child(
                div()
                    .text_size(px(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(tr!("settings.title")),
            )
            .into_any_element(),
    };

    TitleBar::new()
        .h(px(metrics::HEADER))
        .bg(t.bg)
        .border_color(t.bg)
        .child(content)
}

fn render_main(app: &KurrencyApp, t: &Tokens, cx: &Context<KurrencyApp>) -> impl IntoElement {
    let rates = app.rates.read(cx);
    let freshness = rates.freshness(app.rows.iter().map(|row| row.code.as_str()));
    let breaths = rates.refreshes();

    h_flex()
        .id("header")
        .w_full()
        .pl(px(metrics::PAD_X))
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
                .child(render_freshness(freshness, breaths, t))
                .child(
                    Button::new("settings")
                        .ghost()
                        .small()
                        .icon(IconName::Settings)
                        .text_color(t.text)
                        .tooltip(tr!("header.settings"))
                        // `gpui_kit`'s `TitleBar` marks the whole header as the
                        // window's drag area; occlude interactive controls so
                        // Windows' native caption hitbox does not eat clicks.
                        .occlude()
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(OpenSettings), cx);
                        }),
                ),
        )
}

/// The single freshness + refresh pill: shows the status dot and relative age,
/// morphing the dot into a refresh icon on hover when healthy.
fn render_freshness(f: Freshness, breaths: usize, t: &Tokens) -> impl IntoElement {
    let color = match f.dot {
        Dot::Fresh | Dot::Offline => t.accent,
        Dot::Stale => t.amber,
    };
    let can_morph = !f.updating && f.error.is_none();
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

    let indicator = div()
        .relative()
        .size(px(12.))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .when(can_morph, |el| {
                    el.group_hover("freshness", |s| s.opacity(0.))
                })
                .child(dot),
        )
        .when(can_morph, |el| {
            el.child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .opacity(0.)
                    .group_hover("freshness", |s| s.opacity(1.))
                    .child(
                        Icon::new(IconName::RefreshCw)
                            .size(px(11.))
                            .text_color(t.text),
                    ),
            )
        });

    let tooltip = f.clone();
    let error = f.error.clone();
    let updating = f.updating;
    h_flex()
        .id("freshness")
        .group("freshness")
        .occlude()
        .h(px(28.))
        .px_1p5()
        .gap(px(5.))
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|s| s.bg(t.hover))
        .child(indicator)
        .child(
            div()
                .text_size(px(13.))
                .text_color(t.muted)
                .group_hover("freshness", |s| s.text_color(t.text))
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
        // A failure shows its details; otherwise clicking refreshes.
        .on_click(move |_, window, cx| match &error {
            Some(message) => window.push_notification(
                Notification::warning(message.clone()).title(tr!("header.update_failed")),
                cx,
            ),
            None if !updating => window.dispatch_action(Box::new(RefreshRates), cx),
            None => {}
        })
}
