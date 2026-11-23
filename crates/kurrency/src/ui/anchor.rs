//! The anchor card (DESIGN §1): the source currency's clickable flag/code/name,
//! the ⇅ swap button, and the only amount input on screen. Idle, the amount is
//! drawn formatted (muted symbol, dimmed decimals) with the unit rate to the
//! first row underneath; while editing, the currency symbol stays pinned beside
//! the raw input and expressions echo "= 4,500" below. Under the card sit the
//! currency-scaled quick-amount pills.

use std::sync::{Arc, LazyLock};

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, BoxShadow, Context, FontFeatures, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, TextRun,
    Window, div, hsla, px,
};
use kurrency_core::i18n::tr;
use kurrency_core::money;
use kurrency_core::present;
use rust_decimal::Decimal;

use crate::app::{ANCHOR_CONTEXT, KurrencyApp, SwapAnchor};
use crate::assets;
use crate::layout::metrics;
use crate::ui::digits::digits;
use crate::ui::flag::flag;
use crate::ui::motion::{self, Roll};
use crate::ui::picker::{self, PickerMode};
use crate::ui::theme::{SYMBOL_OPACITY, Tokens};

// Lucide "arrow-down-up" (ISC).
const SWAP_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m3 16 4 4 4-4"/><path d="M7 20V4"/><path d="m21 8-4-4-4 4"/><path d="M17 4v16"/></svg>"#;

// Lucide "chevron-down" (ISC).
const CHEVRON_DOWN_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m6 9 6 6 6-6"/></svg>"#;

const AMOUNT_SIZE: f32 = 36.;
/// The anchor display's key in the digit rolls.
pub(crate) const ANCHOR_ROLL: &str = "anchor";

/// Tabular (fixed-width) numerals, so amounts line up.
pub(crate) fn tabular_numerals() -> FontFeatures {
    static TABULAR: LazyLock<FontFeatures> =
        LazyLock::new(|| FontFeatures(Arc::new(vec![("tnum".to_string(), 1)])));
    TABULAR.clone()
}

/// A code in the 13px / 600 style of the typography table.
pub(crate) fn code_label(code: &str) -> impl IntoElement {
    div()
        .text_size(px(13.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(code.to_string())
}

/// `$ 1,250.00` fitted to `width`, with the symbol at 45% and the decimals at
/// the theme's opacity (55%, Ink 75%). With a [`Roll`], the digits in mid-roll
/// are drawn where they are, over what they replace rolling out.
#[allow(clippy::too_many_arguments)]
pub(crate) fn fitted_amount_text(
    code: &str,
    value: Decimal,
    format: &money::Format,
    max_size: f32,
    weight: FontWeight,
    width: f32,
    decimals: f32,
    roll: Option<&Roll>,
    window: &Window,
) -> AnyElement {
    let parts = present::amount_parts(code, value, format, assets::has_glyph);
    let size = fit_parts(&parts, max_size, weight, width, window);
    amount_parts_text(code, parts, size, weight, decimals, roll)
}

fn amount_parts_text(
    code: &str,
    parts: present::AmountParts,
    size: f32,
    weight: FontWeight,
    decimals: f32,
    roll: Option<&Roll>,
) -> AnyElement {
    let int_len = parts.integer.chars().count();
    // At rest, each part is one element (`digits`); a roll needs one per
    // character. Both lay the characters out alike, so nothing moves when a
    // roll starts or ends.
    let roll = roll.filter(|roll| roll.rolling());
    let chars = |text: &str, offset: usize| -> AnyElement {
        match roll {
            None => digits(text).into_any_element(),
            Some(_) => h_flex()
                .children(
                    text.chars()
                        .enumerate()
                        .map(|(ix, ch)| motion::roll_char(ch, offset + ix, roll, size)),
                )
                .into_any_element(),
        }
    };
    let selector = format!("amount-{code}");
    h_flex()
        .debug_selector(move || selector.clone())
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
                .child(chars(&parts.integer, 0))
                .when(!parts.fraction.is_empty(), |el| {
                    el.child(
                        h_flex()
                            .opacity(decimals)
                            .child(chars(&parts.fraction, int_len)),
                    )
                })
                .children(motion::roll_exits(roll, size, decimals)),
        )
        .into_any_element()
}

/// The smallest size [`fit_parts`] shrinks an amount to.
const MIN_FIT: f32 = 0.5;

/// The largest text size, up to `size`, at which `parts` fits in `width`: a
/// long amount (`IRR 1,884,417,000,000`) shrinks rather than being cut off,
/// which would show a different number. Not below half of `size`; past that
/// the column clips.
fn fit_parts(
    parts: &present::AmountParts,
    size: f32,
    weight: FontWeight,
    width: f32,
    window: &Window,
) -> f32 {
    let mut font = gpui_kit::font(assets::FONT_FAMILY);
    font.weight = weight;
    font.features = tabular_numerals();
    let text_system = window.text_system();
    let measure = |text: SharedString, size: f32| {
        let run = TextRun {
            len: text.len(),
            font: font.clone(),
            color: hsla(0., 0., 0., 1.),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        f32::from(text_system.shape_line(text, px(size), &[run], None).width)
    };
    let symbol: Option<SharedString> = parts.symbol.as_deref().map(|s| s.to_string().into());
    // With `tnum`, all ASCII digits share one advance width; count them once
    // and deduplicate group separators so resizing never shapes 15+ characters
    // one by one through DirectWrite.
    let mut digit_count = 0usize;
    let mut extras: Vec<(char, usize)> = Vec::with_capacity(3);
    for ch in parts.integer.chars().chain(parts.fraction.chars()) {
        if ch.is_ascii_digit() {
            digit_count += 1;
        } else if let Some((_, count)) = extras.iter_mut().find(|(c, _)| *c == ch) {
            *count += 1;
        } else {
            extras.push((ch, 1));
        }
    }
    let laid_out = |size: f32| {
        let digits = if digit_count > 0 {
            digit_count as f32 * measure(SharedString::new_static("0"), size).ceil()
        } else {
            0.
        };
        let punct: f32 = extras
            .iter()
            .map(|&(ch, n)| n as f32 * measure(motion::char_text(ch), size).ceil())
            .sum();
        let sym = symbol
            .as_ref()
            .map_or(0., |sym| measure(sym.clone(), size).ceil() + size * 0.25);
        digits + punct + sym
    };
    let natural = laid_out(size);
    if natural <= width {
        return size;
    }
    let min = size * MIN_FIT;
    let mut fit = ((size * width / natural * 2.).floor() / 2.).max(min);
    while fit > min && laid_out(fit) > width {
        fit -= 0.5;
    }
    fit
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
    let symbol = present::display_symbol(&code, assets::has_glyph);

    // Clickable currency trigger: opens the palette in source-currency mode.
    let selector = h_flex()
        .id("anchor-currency")
        .debug_selector(|| "anchor-currency".into())
        .flex_1()
        .min_w(px(0.))
        .h(px(metrics::CARD_TITLE))
        .px(px(4.))
        .ml(px(-4.))
        .gap_2()
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|s| s.bg(t.hover))
        .tooltip(|window, cx| Tooltip::new(tr!("anchor.change_currency")).build(window, cx))
        .on_click(cx.listener(|this, _, window, cx| {
            picker::open_mode(this, "", PickerMode::SelectAnchor, window, cx);
        }))
        .child(flag(&code, cx))
        .child(div().ml(px(2.)).child(code_label(&code)))
        .child(
            div()
                .min_w(px(0.))
                .truncate()
                .text_size(px(12.))
                .text_color(t.muted)
                .child(money::display_name(&code)),
        )
        .child(
            Icon::default()
                .data(CHEVRON_DOWN_SVG)
                .size(px(12.))
                .text_color(t.muted),
        );

    let title = h_flex()
        .h(px(metrics::CARD_TITLE))
        .gap_1()
        .child(selector)
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
        // Keep the muted currency symbol pinned beside the input so entering
        // and leaving edit mode causes zero horizontal jump.
        let input = Input::new(&app.anchor_input)
            .appearance(false)
            .line_height(px(metrics::CARD_AMOUNT))
            .px_0()
            .py_0()
            .text_size(px(AMOUNT_SIZE))
            .font_weight(FontWeight::MEDIUM);
        h_flex()
            .key_context(ANCHOR_CONTEXT)
            .w_full()
            .h(px(metrics::CARD_AMOUNT))
            .items_center()
            .when_some(symbol, |el, sym| {
                el.child(
                    div()
                        .flex_shrink_0()
                        .text_size(px(AMOUNT_SIZE))
                        .line_height(px(metrics::CARD_AMOUNT))
                        .font_weight(FontWeight::MEDIUM)
                        .opacity(SYMBOL_OPACITY)
                        .mr(px(AMOUNT_SIZE * 0.25))
                        .child(sym),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .child(gpui_kit::Styled::h(input, px(metrics::CARD_AMOUNT))),
            )
            .into_any_element()
    } else {
        let display = match app.amount {
            Some(value) => fitted_amount_text(
                &code,
                value,
                &app.fmt,
                AMOUNT_SIZE,
                FontWeight::MEDIUM,
                // The card's border and 16px padding on each side.
                f32::from(window.viewport_size().width) - 2. * (metrics::PAD_X + 17.),
                t.decimals,
                app.motion.roll(ANCHOR_ROLL),
                window,
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

    // While editing an expression, show the live "= 4,500" echo. Idle, show
    // the unit rate against the first row ("1 USD = 0.8914 EUR") so the slot
    // carries useful reference info instead of dead space.
    let base_rate = app.rows.get(1).and_then(|first| {
        let rate = app
            .rates
            .read(cx)
            .table()
            .convert(Decimal::ONE, &code, &first.code)?;
        Some(present::inverse_rate_text(
            &code,
            &first.code,
            rate,
            app.fmt.style.decimal,
        ))
    });
    let echo_text = app
        .echo
        .clone()
        .filter(|_| editing)
        .or_else(|| base_rate.map(Into::into));
    let echo = div()
        .h(px(metrics::CARD_ECHO))
        .text_size(px(12.))
        .text_color(t.muted)
        .font_features(tabular_numerals())
        .truncate()
        .children(echo_text);

    let border_color = if focused {
        t.accent
    } else if t.accent_fill > 0. {
        t.accent.opacity(t.accent_fill * 2.)
    } else {
        t.hairline
    };
    let halo_opacity = if t.accent_fill > 0. {
        t.accent_fill * 1.6
    } else if t.dark {
        0.22
    } else {
        0.14
    };

    let card = v_flex()
        .relative()
        .rounded(px(12.))
        .border_1()
        .border_color(border_color)
        .bg(t.card)
        .px(px(16.))
        .pt(px(metrics::CARD_PAD_TOP))
        .pb(px(metrics::CARD_PAD_BOTTOM))
        .when(focused, |el| {
            el.shadow(vec![
                BoxShadow::new(px(0.), px(0.), t.accent.opacity(halo_opacity))
                    .spread_radius(px(2.5)),
            ])
        })
        .when(t.dark && !focused, |el| {
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

    let card = match app.motion.shake {
        Some(version) => motion::shake(card, ("anchor-shake", version)),
        None => card.into_any_element(),
    };

    // Preserve the RING gutter around the card for the soft focus halo.
    div()
        .mx(px(metrics::PAD_X - metrics::RING))
        .p(px(metrics::RING))
        .child(card)
        .into_any_element()
}

/// Currency-aware quick-amount pills in a soft segmented strip.
pub(crate) fn render_quick_amounts(
    app: &KurrencyApp,
    cx: &mut Context<KurrencyApp>,
) -> impl IntoElement {
    let t = Tokens::get(cx);
    let rate_per_usd = app.anchor().and_then(|code| {
        app.rates
            .read(cx)
            .table()
            .convert(Decimal::ONE, "USD", code)
    });
    let presets = present::quick_amounts(rate_per_usd);
    let strip_bg = if t.dark {
        hsla(0., 0., 1., 0.04)
    } else {
        hsla(0., 0., 0., 0.04)
    };

    h_flex()
        .mt(px(metrics::CHIPS_GAP))
        .mx(px(metrics::PAD_X))
        .h(px(metrics::CHIPS))
        .p(px(2.))
        .gap(px(3.))
        .rounded(px(9.))
        .bg(strip_bg)
        .children(presets.iter().map(|&(label, value)| {
            let value = Decimal::from(value);
            let selected = app.amount == Some(value);
            div()
                .id(label)
                .debug_selector(move || format!("quick-{label}"))
                .flex_1()
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(7.))
                .text_size(px(12.5))
                .font_features(tabular_numerals())
                .cursor_pointer()
                .map(|el| {
                    if selected {
                        let bg = if t.accent_fill > 0. {
                            t.accent.opacity(t.accent_fill)
                        } else {
                            t.card
                        };
                        let fg = if t.accent_fill > 0. { t.accent } else { t.text };
                        el.bg(bg)
                            .text_color(fg)
                            .font_weight(FontWeight::MEDIUM)
                            .shadow(vec![
                                BoxShadow::new(
                                    px(0.),
                                    px(1.),
                                    hsla(0., 0., 0., if t.dark { 0.35 } else { 0.08 }),
                                )
                                .blur_radius(px(2.)),
                            ])
                    } else {
                        el.text_color(t.muted)
                            .hover(|s| s.text_color(t.text).bg(t.hover))
                    }
                })
                .child(quick_label(label))
                .on_click(
                    cx.listener(move |this, _, window, cx| this.set_amount(value, window, cx)),
                )
        }))
}

/// A chip's text in the UI language (`1k` → `1千`, `10k` → `1万`, …).
fn quick_label(label: &str) -> String {
    match label {
        "10" => tr!("quick.ten"),
        "100" => tr!("quick.hundred"),
        "1k" => tr!("quick.thousand"),
        "10k" => tr!("quick.ten_thousand"),
        "100k" => tr!("quick.hundred_thousand"),
        "1m" => tr!("quick.million"),
        "10m" => tr!("quick.ten_million"),
        other => other.to_string(),
    }
}
