//! The bottom toast: undo ("JPY removed · Undo", DESIGN §2) or a clipboard
//! price hint ("Convert ¥1,200 from clipboard? · Convert"). It sits at the
//! bottom of the window, in the "+ Add" row's place, at the content width:
//! clear of the anchor card at any window height, and inside the window
//! (gpui-kit's notification layer has no height, so its bottom placements
//! fall off-screen).

use std::time::Duration;

use gpui_kit::component::{Icon, h_flex};
use gpui_kit::{
    Animation, AnimationExt as _, Context, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, div,
    ease_out_quint, px,
};
use kurrency_core::i18n::tr;
use rust_decimal::Decimal;

use crate::app::KurrencyApp;
use crate::layout::metrics;
use crate::ui::theme::Tokens;

// Lucide "x" (ISC).
const CLOSE_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>"#;

/// How long an undo toast stays.
pub(crate) const SHOW_FOR: Duration = Duration::from_secs(5);
/// How long a clipboard suggestion toast stays.
pub(crate) const CLIPBOARD_SHOW_FOR: Duration = Duration::from_secs(8);
/// Its fade and rise in.
const APPEAR: Duration = Duration::from_millis(180);

/// What the toast's primary button does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ToastAction {
    Undo,
    ConvertClipboard { code: String, amount: Decimal },
}

/// The toast on screen; `version` restarts its timer and animation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Toast {
    pub message: SharedString,
    pub action: ToastAction,
    pub version: usize,
}

/// Where the toast goes in a content area (the window below the header) of
/// `area_height` holding `currencies`: over the "+ Add" row when the list
/// fits, so it replaces that row; else pinned to the bottom edge, over the
/// end of the scrolled list. Either way it is clear of the anchor card.
/// Returns the toast's top edge.
pub(crate) fn top(currencies: usize, area_height: f32) -> f32 {
    let add_row = crate::layout::content_height(currencies)
        - metrics::HEADER
        - metrics::BOTTOM
        - metrics::ADD;
    let pinned = area_height - metrics::BOTTOM - metrics::ADD;
    if currencies == 0 {
        pinned
    } else {
        add_row.min(pinned)
    }
}

/// The toast at `top` (see [`top`]). The caller puts it in the content
/// area, which is `relative`.
pub(crate) fn render(toast: &Toast, top: f32, cx: &mut Context<KurrencyApp>) -> impl IntoElement {
    let t = Tokens::get(cx);
    let action_btn = match &toast.action {
        ToastAction::Undo => div()
            .id("toast-undo")
            .debug_selector(|| "toast-undo".into())
            .h(px(32.))
            .px(px(12.))
            .flex()
            .items_center()
            .rounded(px(7.))
            .text_size(px(13.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(t.accent)
            .cursor_pointer()
            .hover(|s| s.bg(t.hover))
            .child(tr!("toast.undo"))
            .on_click(cx.listener(|this, _, window, cx| {
                this.dismiss_toast(cx);
                this.undo_change(window, cx);
            })),
        ToastAction::ConvertClipboard { code, amount } => {
            let code = code.clone();
            let amount = *amount;
            div()
                .id("toast-apply")
                .debug_selector(|| "toast-apply".into())
                .h(px(30.))
                .px(px(10.))
                .flex()
                .items_center()
                .rounded(px(7.))
                .text_size(px(13.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.accent)
                .cursor_pointer()
                .hover(|s| s.bg(t.hover))
                .child(tr!("toast.apply"))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.dismiss_toast(cx);
                    this.apply_price(&code, amount, window, cx);
                }))
        }
    };
    let dismiss_btn = matches!(toast.action, ToastAction::ConvertClipboard { .. }).then(|| {
        div()
            .id("toast-dismiss")
            .size(px(26.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.))
            .text_color(t.muted)
            .cursor_pointer()
            .hover(|s| s.bg(t.hover).text_color(t.text))
            .child(Icon::default().data(CLOSE_SVG).size(px(13.)))
            .on_click(cx.listener(|this, _, _, cx| this.dismiss_toast(cx)))
    });

    div()
        .id("toast")
        .debug_selector(|| "toast".into())
        .absolute()
        .left(px(metrics::PAD_X))
        .right(px(metrics::PAD_X))
        .top(px(top))
        .h(px(metrics::ADD))
        // Clicks on the toast never reach the row behind it.
        .occlude()
        .child(
            h_flex()
                .size_full()
                .pl(px(14.))
                .pr(px(4.))
                .gap_1()
                .justify_between()
                .rounded(px(10.))
                .border_1()
                .border_color(t.hairline)
                .bg(t.card)
                .shadow_md()
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_size(px(13.))
                        .text_color(t.text)
                        .truncate()
                        .child(toast.message.clone()),
                )
                .child(
                    h_flex()
                        .flex_shrink_0()
                        .gap_0p5()
                        .child(action_btn)
                        .children(dismiss_btn),
                ),
        )
        .with_animation(
            ("toast-in", toast.version),
            Animation::new(APPEAR).with_easing(ease_out_quint()),
            move |el, t| el.opacity(t).top(px(top + 6. * (1. - t))),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::content_height;

    #[test]
    fn the_toast_replaces_the_add_row_or_pins_to_the_bottom() {
        use metrics::*;
        // At the smallest window, a short list: over the "+ Add" row.
        let area = MIN_HEIGHT - HEADER;
        let at = top(2, area);
        assert_eq!(at + ADD + BOTTOM + HEADER, content_height(2));
        // Below the anchor card and its chips, always.
        let card_bottom = 2. * RING + CARD + CHIPS_GAP + CHIPS;
        for n in 1..12 {
            let at = top(n, area);
            assert!(at >= card_bottom, "{n}: {at}");
            assert!(at + ADD + BOTTOM <= area, "{n}: inside the window");
        }
        // A list longer than the window: pinned to the bottom edge.
        assert_eq!(top(12, area), area - BOTTOM - ADD);
    }
}
