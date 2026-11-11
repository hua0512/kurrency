//! The anchor card (DESIGN §1): the source currency's flag, code and
//! name, the ⇅ swap button, and the only amount input on screen. Idle, the
//! amount is drawn formatted (muted symbol, dimmed decimals); while editing
//! it is the raw expression with the "= 4,500" echo below. Under the card sit
//! the quick-amount chips.

use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::{Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, BoxShadow, Context, FontFeatures, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _, Window, div, hsla, px,
    transparent_black,
};
use kurrency_core::i18n::tr;
use kurrency_core::money;
use kurrency_core::present::{self, QUICK_AMOUNTS};
use rust_decimal::Decimal;

use crate::app::{ANCHOR_CONTEXT, KurrencyApp, SwapAnchor};
use crate::assets;
use crate::layout::metrics;
use crate::ui::flag::flag;
use crate::ui::motion::{self, Roll};
use crate::ui::theme::{SYMBOL_OPACITY, Tokens};

// Lucide "arrow-down-up" (ISC), outside gpui-kit's default embedded icons.
const SWAP_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m3 16 4 4 4-4"/><path d="M7 20V4"/><path d="m21 8-4-4-4 4"/><path d="M17 4v16"/></svg>"#;

const AMOUNT_SIZE: f32 = 36.;
/// The anchor display's key in the digit rolls.
pub(crate) const ANCHOR_ROLL: &str = "anchor";

/// Tabular (fixed-width) numerals, so amounts line up.
pub(crate) fn tabular_numerals() -> FontFeatures {
    FontFeatures(Arc::new(vec![("tnum".to_string(), 1)]))
}

/// A code in the 13px / 600 style of the typography table.
pub(crate) fn code_label(code: &str) -> impl IntoElement {
    div()
        .text_size(px(13.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(code.to_string())
}

/// `$ 1,250.00` with the symbol at 45% and the decimals at the theme's
/// opacity (55%, Ink 75%). With a [`Roll`], the digits in mid-roll are
/// drawn where they are, over what they replace rolling out.
pub(crate) fn amount_text(
    code: &str,
    value: Decimal,
    format: &money::Format,
    size: f32,
    weight: FontWeight,
    decimals: f32,
    roll: Option<&Roll>,
) -> AnyElement {
    let parts = present::amount_parts(code, value, format, assets::has_glyph);
    let int_len = parts.integer.chars().count();
    let chars = |text: &str, offset: usize| {
        text.chars()
            .enumerate()
            .map(|(ix, ch)| motion::roll_char(ch, offset + ix, roll, size))
            .collect::<Vec<_>>()
    };
    h_flex()
        .items_baseline()
        .text_size(px(size))
        .font_weight(weight)
        .font_features(tabular_numerals())
        .whitespace_nowrap()
        .when_some(parts.symbol, |el, symbol| {
            el.child(
                div()
                    .opacity(SYMBOL_OPACITY)
                    .mr(px(size * 0.25))
                    .child(symbol),
            )
        })
        .child(
            h_flex()
                .relative()
                .child(h_flex().children(chars(&parts.integer, 0)))
                .when(!parts.fraction.is_empty(), |el| {
                    el.child(
                        h_flex()
                            .opacity(decimals)
                            .children(chars(&parts.fraction, int_len)),
                    )
                })
                .children(motion::roll_exits(roll, size, decimals)),
        )
        .into_any_element()
}

/// What [`amount_text`] shows after the symbol (`1,250.00`), which digit
/// rolls compare, and how many characters of it are the integer part.
pub(crate) fn amount_digits(code: &str, value: Decimal, format: &money::Format) -> (String, usize) {
    let parts = present::amount_parts(code, value, format, assets::has_glyph);
    let int_len = parts.integer.chars().count();
    (parts.integer + &parts.fraction, int_len)
}

pub(crate) fn render(
    app: &KurrencyApp,
    window: &mut Window,
    cx: &mut Context<KurrencyApp>,
) -> AnyElement {
    let t = Tokens::get(cx);
    let Some(code) = app.anchor().map(str::to_string) else {
        return div().into_any_element();
    };
    let editing = app.is_editing();
    let focused = app.anchor_focused(window, cx);

    let title = h_flex()
        .h(px(metrics::CARD_TITLE))
        .gap_2()
        .child(flag(&code, cx))
        .child(div().ml_1().child(code_label(&code)))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_size(px(12.))
                .text_color(t.muted)
                .child(money::display_name(&code)),
        )
        .when(app.rows.len() > 1, |el| {
            el.child(
                Button::new("swap-anchor")
                    .ghost()
                    .small()
                    .icon(Icon::default().data(SWAP_SVG))
                    .text_color(t.text)
                    .tooltip(tr!("anchor.swap"))
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(SwapAnchor), cx)),
            )
        });

    let amount = if editing {
        // The input pads itself, sets a 1.25rem line height for any text
        // size and is 32px high; all three are overridden for the 36px
        // figure. Its height goes through `Styled`: `Input::h` sizes only
        // multi-line inputs, and the card would shrink 12px while editing.
        let input = Input::new(&app.anchor_input)
            .appearance(false)
            .line_height(px(metrics::CARD_AMOUNT))
            .px_0()
            .py_0()
            .text_size(px(AMOUNT_SIZE))
            .font_weight(FontWeight::MEDIUM);
        div()
            .key_context(ANCHOR_CONTEXT)
            .w_full()
            .child(gpui_kit::Styled::h(input, px(metrics::CARD_AMOUNT)))
            .into_any_element()
    } else {
        let display = match app.amount {
            Some(value) => amount_text(
                &code,
                value,
                &app.fmt,
                AMOUNT_SIZE,
                FontWeight::MEDIUM,
                t.decimals,
                app.motion.roll(ANCHOR_ROLL),
            ),
            None => div()
                .text_size(px(AMOUNT_SIZE))
                .text_color(t.muted)
                .child("0")
                .into_any_element(),
        };
        div()
            .id("anchor-amount")
            .debug_selector(|| "anchor-amount".into())
            .w_full()
            .h(px(metrics::CARD_AMOUNT))
            .flex()
            .items_center()
            .overflow_hidden()
            .cursor_text()
            .on_click(cx.listener(|this, _, window, cx| this.focus_anchor(window, cx)))
            .child(display)
            .into_any_element()
    };

    // The promoted row slides up into the card.
    let head = v_flex().child(title).child(amount);
    let head = match &app.motion.swap {
        Some(swap) if swap.promoted == code => {
            motion::slide_in(head, ("anchor-swap", swap.version), 14.)
        }
        _ => head.into_any_element(),
    };

    let echo = div()
        .h(px(metrics::CARD_ECHO))
        .text_size(px(12.))
        .text_color(t.muted)
        .font_features(tabular_numerals())
        .when_some(app.echo.clone().filter(|_| editing), |el, echo| {
            el.child(echo)
        });

    let card = v_flex()
        .relative()
        .rounded(px(12.))
        .border_1()
        // Slate edges the card in a wash of its accent.
        .border_color(if t.accent_fill > 0. {
            t.accent.opacity(t.accent_fill * 2.)
        } else {
            t.hairline
        })
        .bg(t.card)
        .px(px(16.))
        .pt(px(metrics::CARD_PAD_TOP))
        .pb(px(metrics::CARD_PAD_BOTTOM))
        // Dark: a faint inner glow along the edge instead of a shadow.
        .when(t.dark, |el| {
            el.child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(11.))
                    .border_1()
                    .border_color(hsla(0., 0., 1., 0.035)),
            )
        })
        .child(head)
        .child(echo);

    // The 2px focus ring, offset 2px from the card.
    div()
        .mx(px(metrics::PAD_X - metrics::RING))
        .p(px(2.))
        .border_2()
        .rounded(px(16.))
        .border_color(if focused {
            t.accent
        } else {
            transparent_black()
        })
        // Slate adds a soft accent halo around the ring.
        .when(focused && t.accent_fill > 0., |el| {
            el.shadow(vec![
                BoxShadow::new(px(0.), px(0.), t.accent.opacity(t.accent_fill * 1.5))
                    .spread_radius(px(3.)),
            ])
        })
        .child(card)
        .into_any_element()
}

/// The 10 / 100 / 1k / 10k ghost chips; the one matching the amount is
/// drawn selected in the accent.
pub(crate) fn render_quick_amounts(
    app: &KurrencyApp,
    cx: &mut Context<KurrencyApp>,
) -> impl IntoElement {
    let t = Tokens::get(cx);
    h_flex()
        .mt(px(metrics::CHIPS_GAP))
        .px(px(metrics::PAD_X))
        .gap_2()
        .children(QUICK_AMOUNTS.iter().map(|&(label, value)| {
            let value = Decimal::from(value);
            let selected = app.amount == Some(value);
            div()
                .id(label)
                .debug_selector(move || format!("quick-{label}"))
                .flex_1()
                .h(px(metrics::CHIPS))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(8.))
                .border_1()
                .border_color(if selected { t.accent } else { t.hairline })
                .text_size(px(13.))
                .font_features(tabular_numerals())
                .text_color(if selected { t.accent } else { t.text })
                .cursor_pointer()
                // Slate tints the selected chip.
                .map(|el| {
                    if selected && t.accent_fill > 0. {
                        el.bg(t.accent.opacity(t.accent_fill))
                            .font_weight(FontWeight::MEDIUM)
                            .hover(|s| s.bg(t.accent.opacity(t.accent_fill * 1.4)))
                    } else {
                        el.hover(|s| s.bg(t.hover))
                    }
                })
                .child(quick_label(label))
                .on_click(
                    cx.listener(move |this, _, window, cx| this.set_amount(value, window, cx)),
                )
        }))
}

/// A chip's text in the UI language (`1k` → `1千`, `10k` → `1万`).
fn quick_label(label: &str) -> String {
    use kurrency_core::i18n::tr;
    match label {
        "10" => tr!("quick.ten"),
        "100" => tr!("quick.hundred"),
        "1k" => tr!("quick.thousand"),
        "10k" => tr!("quick.ten_thousand"),
        other => other.to_string(),
    }
}
