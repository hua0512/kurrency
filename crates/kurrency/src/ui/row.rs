//! The read-only rows under the anchor card (DESIGN §1, §3), 56px each:
//! flag, code, the converted amount, and the daily change chip. Hovering a
//! row shows its name and the inverse rate. Clicking the chip opens the
//! history page; clicking anywhere else promotes the row to the anchor.
//!
//! Rows reorder by dragging their ⋮⋮ grip (docs/DESIGN.md §1), which lifts
//! the row while the others spring apart around the drop slot, or with
//! Alt+↑/↓ on a focused row.

use gpui_kit::component::button::Button;
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, BoxShadow, Context, Div, DragMoveEvent, ElementId,
    FontWeight, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render,
    SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, Window, div, hsla, px,
    transparent_black,
};
use kurrency_core::i18n::tr;
use kurrency_core::money;
use kurrency_core::present::{self, Change};

use crate::app::{KurrencyApp, OpenPicker, OpenRowDetail, ROW_CONTEXT};
use crate::layout::metrics;
use crate::ui::anchor::{amount_text, code_label, tabular_numerals};
use crate::ui::flag::flag;
use crate::ui::motion::{self, Leaving};
use crate::ui::theme::{CHIP_TINT, Tokens};

/// The row's horizontal inset inside the window: its hover and focus
/// highlight lines up with the anchor card and the hairlines' right end.
const INSET: f32 = metrics::PAD_X;
/// The padding inside the highlight, before the flag and after the chip.
const ROW_PAD: f32 = 8.;
/// Where the code column starts (and the hairlines with it).
const CODE_X: f32 = INSET + ROW_PAD + 20. + 16.;
const CODE_W: f32 = 82.;
const CHIP_SLOT: f32 = 62.;
/// Row amounts: 18px, weight 450.
const AMOUNT_SIZE: f32 = 18.;
const AMOUNT_WEIGHT: FontWeight = FontWeight(450.);

pub(crate) fn render_list(
    app: &KurrencyApp,
    window: &mut Window,
    cx: &mut Context<KurrencyApp>,
) -> impl IntoElement {
    let _ = window;
    // While a row is dragged, the list follows the pointer: the drop slot
    // is worked out from where the lifted row's middle is.
    let mut list = v_flex()
        .w_full()
        .mt(px(metrics::LIST_GAP))
        .on_drag_move(
            cx.listener(|this, event: &DragMoveEvent<DraggedRow>, _, cx| {
                this.drag_row_to(
                    f32::from(event.event.position.y),
                    f32::from(event.bounds.origin.y),
                    cx,
                );
            }),
        );
    if let Some(drag) = &app.drag {
        return render_dragging(app, drag, list, cx);
    }
    let ghosts = |list: Div, ix: usize, cx: &mut Context<KurrencyApp>| {
        app.motion
            .leaving
            .iter()
            .filter(|leaving| leaving.ix == ix)
            .fold(list, |list, leaving| {
                list.child(render_ghost(app, leaving, cx))
            })
    };
    for ix in 1..app.rows.len() {
        list = ghosts(list, ix, cx);
        let code = app.rows[ix].code.clone();
        let row = with_context_menu(render_row(app, ix, cx), &code, cx).into_any_element();
        list = list.child(match app.motion.entering.get(&code) {
            Some(&version) => motion::resize(
                row,
                ElementId::NamedInteger(format!("enter-{code}").into(), version as u64),
                metrics::ROW,
                true,
            ),
            None => row,
        });
    }
    // Ghosts past the end (the last row was removed).
    let end = app.rows.len().max(1);
    for leaving in app.motion.leaving.iter().filter(|l| l.ix >= end) {
        list = list.child(render_ghost(app, leaving, cx));
    }
    list
}

/// The list while a row is lifted: the other rows in order, with an open
/// gap at the drop slot (and the one it just left closing).
fn render_dragging(
    app: &KurrencyApp,
    drag: &RowDrag,
    mut list: Div,
    cx: &mut Context<KurrencyApp>,
) -> Div {
    let t = Tokens::get(cx);
    let others: Vec<usize> = (1..app.rows.len())
        .filter(|&ix| app.rows[ix].code != drag.code)
        .collect();
    let gap = |slot: usize, opening: bool| {
        let id = ElementId::NamedInteger(
            format!("drag-gap-{}-{slot}", if opening { "open" } else { "close" }).into(),
            drag.version as u64,
        );
        motion::gap(drop_indicator(&t, opening), id, metrics::ROW, opening)
    };
    for slot in 0..=others.len() {
        if drag.previous == Some(slot) {
            list = list.child(gap(slot, false));
        }
        if drag.slot == slot {
            list = list.child(gap(slot, true));
        }
        if let Some(&ix) = others.get(slot) {
            let code = app.rows[ix].code.clone();
            list = list.child(with_context_menu(render_row(app, ix, cx), &code, cx));
        }
    }
    list
}

/// The target slot: a soft accent placeholder with a dashed outline.
fn drop_indicator(t: &Tokens, target: bool) -> impl IntoElement {
    div().h(px(metrics::ROW)).px(px(INSET)).py(px(3.)).child(
        div().size_full().rounded(px(8.)).when(target, |el| {
            el.bg(t.accent.opacity(0.07))
                .border_1()
                .border_dashed()
                .border_color(t.accent.opacity(0.55))
        }),
    )
}

/// The list slot (0 = just under the anchor, up to `count - 1`) a row held
/// `grab_y` px below its top drops into, with the pointer at `pointer_y`
/// and the list starting at `list_top`: the one its middle is over.
pub(crate) fn drop_slot(pointer_y: f32, grab_y: f32, list_top: f32, count: usize) -> usize {
    let middle = pointer_y - grab_y + metrics::ROW / 2.;
    let slot = ((middle - list_top) / metrics::ROW).floor();
    if slot.is_nan() || slot < 0. {
        0
    } else {
        (slot as usize).min(count.saturating_sub(1))
    }
}

/// What gpui carries while a row is dragged.
#[derive(Debug, Clone)]
pub(crate) struct DraggedRow {
    pub code: String,
}

/// A row lifted for reordering: where it came from and the slot it would
/// drop into.
#[derive(Debug, Clone)]
pub(crate) struct RowDrag {
    pub code: String,
    /// The list slot the row would drop into (its index in `rows` minus 1).
    pub slot: usize,
    /// The slot it was over a moment ago, whose gap closes.
    pub previous: Option<usize>,
    /// Bumped when `slot` changes, restarting the gaps' animation.
    pub version: usize,
    /// Where the pointer holds the row, from its top.
    pub grab_y: f32,
    /// The pointer and the list's top, in window px.
    pub pointer_y: f32,
    pub list_top: f32,
}

/// The lifted row under the pointer: raised a little above the list (a
/// slightly larger surface with a soft shadow), moving only vertically.
pub(crate) struct RowDragView {
    code: String,
    value: Option<rust_decimal::Decimal>,
    fmt: money::Format,
    change: Change,
    green_up: bool,
    /// The pointer's x when the drag began, which the row stays under.
    start_x: Pixels,
    width: Pixels,
}

impl RowDragView {
    pub(crate) fn new(app: &KurrencyApp, code: &str, window: &Window, cx: &App) -> Self {
        let anchor = app.anchor().unwrap_or_default();
        let rates = app.rates.read(cx);
        let today = rates
            .table()
            .convert(rust_decimal::Decimal::ONE, anchor, code);
        let previous = rates.previous().and_then(|p| p.cross_rate(anchor, code));
        Self {
            code: code.to_string(),
            value: app.value_of(code, cx),
            fmt: app.fmt,
            change: Change::between(today, previous),
            green_up: app.green_up(),
            start_x: window.mouse_position().x,
            width: window.viewport_size().width - px(2. * INSET),
        }
    }
}

/// How far the lifted row grows past its slot on each side (≈ 1.02 scale).
const LIFT: f32 = 3.;

impl Render for RowDragView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = Tokens::get(cx);
        let code = self.code.clone();
        let amount = match self.value {
            Some(value) => amount_text(
                &code,
                value,
                &self.fmt,
                AMOUNT_SIZE,
                AMOUNT_WEIGHT,
                t.decimals,
                None,
            ),
            None => div().child("—").into_any_element(),
        };
        let shadow = if t.dark {
            vec![
                BoxShadow::new(px(0.), px(10.), hsla(0., 0., 0., 0.55)).blur_radius(px(24.)),
                BoxShadow::new(px(0.), px(1.), hsla(0., 0., 0., 0.4)).blur_radius(px(3.)),
            ]
        } else {
            vec![
                BoxShadow::new(px(0.), px(8.), hsla(0., 0., 0., 0.12)).blur_radius(px(20.)),
                BoxShadow::new(px(0.), px(1.), hsla(0., 0., 0., 0.08)).blur_radius(px(3.)),
            ]
        };
        // Pinned to the column the drag started in.
        let dx = self.start_x - window.mouse_position().x;
        div()
            .relative()
            .left(dx)
            .w(self.width)
            .font_family(cx.theme().font_family.clone())
            .text_color(t.text)
            .child(
                h_flex()
                    .relative()
                    .left(px(-LIFT))
                    .top(px(-1.))
                    .w(self.width + px(2. * LIFT))
                    .h(px(metrics::ROW + 1.))
                    // The row's 8px inset (padding and border) plus the lift.
                    .pl(px(ROW_PAD + LIFT - 1.))
                    .pr(px(ROW_PAD + LIFT - 1.))
                    .rounded(px(10.))
                    .border_1()
                    .border_color(if t.dark {
                        hsla(0., 0., 1., 0.08)
                    } else {
                        t.hairline
                    })
                    // Dark themes raise it by lightening the card a touch.
                    .bg(if t.dark {
                        t.card.blend(hsla(0., 0., 1., 0.06))
                    } else {
                        t.card
                    })
                    .shadow(shadow)
                    .cursor_grabbing()
                    .child(grip(&t, LIFT + 1.))
                    .child(flag(&code, cx))
                    .child(div().w(px(CODE_X - INSET - ROW_PAD - 20.)).flex_shrink_0())
                    .child(div().w(px(CODE_W)).flex_shrink_0().child(code_label(&code)))
                    .child(div().flex_1().min_w(px(0.)).overflow_hidden().child(amount))
                    .child(
                        h_flex()
                            .min_w(px(CHIP_SLOT))
                            .justify_center()
                            .flex_shrink_0()
                            .child(render_chip(self.change, self.green_up, &t)),
                    ),
            )
    }
}

/// The ⋮⋮ grip: two columns of three dots, `left` px into the row.
fn grip(t: &Tokens, left: f32) -> impl IntoElement {
    let dot = || div().size(px(2.)).rounded_full().bg(t.muted);
    let column = || v_flex().gap(px(1.5)).child(dot()).child(dot()).child(dot());
    h_flex()
        .absolute()
        .left(px(left))
        .top_0()
        .bottom_0()
        .items_center()
        .child(h_flex().gap(px(1.5)).child(column()).child(column()))
}

/// A removed row collapsing: a static copy of its flag, code and amount.
fn render_ghost(app: &KurrencyApp, leaving: &Leaving, cx: &mut Context<KurrencyApp>) -> AnyElement {
    let t = Tokens::get(cx);
    let code = leaving.code.clone();
    let amount = app.value_of(&code, cx).map(|value| {
        amount_text(
            &code,
            value,
            &app.fmt,
            AMOUNT_SIZE,
            AMOUNT_WEIGHT,
            t.decimals,
            None,
        )
    });
    let row = v_flex()
        .px(px(INSET))
        .child(
            h_flex()
                .h(px(metrics::ROW - 1.))
                .px(px(ROW_PAD))
                .child(flag(&code, cx))
                .child(div().w(px(CODE_X - INSET - ROW_PAD - 20.)).flex_shrink_0())
                .child(div().w(px(CODE_W)).flex_shrink_0().child(code_label(&code)))
                .children(amount),
        )
        .child(
            div()
                .ml(px(CODE_X - INSET))
                .mr(px(ROW_PAD))
                .h(px(1.))
                .bg(t.hairline),
        );
    motion::resize(
        row,
        ElementId::NamedInteger(format!("leave-{code}").into(), leaving.version as u64),
        metrics::ROW,
        false,
    )
}

fn render_row(app: &KurrencyApp, ix: usize, cx: &mut Context<KurrencyApp>) -> Stateful<Div> {
    let t = Tokens::get(cx);
    let row = &app.rows[ix];
    let code = row.code.clone();
    let anchor = app.anchor().unwrap_or_default().to_string();
    // No hover while a row is being dragged.
    let hovered = app.hovered.as_deref() == Some(code.as_str()) && app.drag.is_none();
    let green_up = app.green_up();

    let rates = app.rates.read(cx);
    let table = rates.table();
    let today = table.convert(rust_decimal::Decimal::ONE, &anchor, &code);
    let previous = rates.previous().and_then(|p| p.cross_rate(&anchor, &code));
    let change = Change::between(today, previous);
    let inverse = table
        .convert(rust_decimal::Decimal::ONE, &code, &anchor)
        .map(|rate| present::inverse_rate_text(&code, &anchor, rate, app.fmt.style.decimal));

    let amount = match app.value_of(&code, cx) {
        Some(value) => amount_text(
            &code,
            value,
            &app.fmt,
            AMOUNT_SIZE,
            AMOUNT_WEIGHT,
            t.decimals,
            app.motion.roll(&code).map(|roll| (code.as_str(), roll)),
        ),
        None => div()
            .text_size(px(AMOUNT_SIZE))
            .text_color(t.muted)
            .child("—")
            .into_any_element(),
    };

    let code_col = v_flex()
        .w(px(CODE_W))
        .flex_shrink_0()
        .child(code_label(&code))
        .when(hovered, |el| {
            el.child(
                div()
                    .text_size(px(11.))
                    .text_color(t.muted)
                    .truncate()
                    .child(money::display_name(&code)),
            )
        });

    let amount_col = v_flex()
        .flex_1()
        .min_w(px(0.))
        .overflow_hidden()
        .child(amount)
        .when_some(inverse.filter(|_| hovered), |el, inverse| {
            el.child(
                div()
                    .text_size(px(11.))
                    .text_color(t.muted)
                    .font_features(tabular_numerals())
                    .whitespace_nowrap()
                    .child(inverse),
            )
        });

    let chip_code: SharedString = code.clone().into();
    let chip = h_flex()
        .id(("change", ix))
        .debug_selector({
            let code = code.clone();
            move || format!("chip-{code}")
        })
        .min_w(px(CHIP_SLOT))
        .h(px(24.))
        .justify_center()
        .flex_shrink_0()
        .cursor_pointer()
        .tooltip(|window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(tr!("row.change_tooltip")).build(window, cx)
        })
        .on_click(move |_, window, cx| {
            window.dispatch_action(
                Box::new(OpenRowDetail {
                    code: chip_code.clone(),
                }),
                cx,
            );
            cx.stop_propagation();
        })
        .child(match &app.motion.copied {
            // Copying flashes a check mark in place of the chip.
            Some((copied, version)) if *copied == code => motion::fade_in(
                Icon::new(IconName::Check)
                    .size(px(16.))
                    .text_color(t.accent),
                ElementId::NamedInteger(format!("copied-{code}").into(), *version as u64),
            ),
            _ => render_chip(change, green_up, &t).into_any_element(),
        });

    let focus_handle = row.focus_handle.clone();
    let hover_code = code.clone();
    let content = h_flex()
        .id(("row-body", ix))
        .debug_selector({
            let code = code.clone();
            move || format!("row-{code}")
        })
        .track_focus(&focus_handle)
        .key_context(ROW_CONTEXT)
        .h(px(metrics::ROW - 1.))
        .px(px(ROW_PAD - 2.))
        .rounded(px(8.))
        .border_2()
        .border_color(transparent_black())
        .focus_visible(|s| s.border_color(t.accent))
        .when(hovered, |el| el.bg(t.hover))
        .cursor_pointer()
        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
            if *hovered {
                this.hovered = Some(hover_code.clone());
            } else if this.hovered.as_deref() == Some(hover_code.as_str()) {
                this.hovered = None;
            }
            cx.notify();
        }))
        .on_click(cx.listener({
            let code = code.clone();
            move |this, _, window, cx| {
                // The release that ends a drag is not a click.
                if this.drag.is_some() {
                    return;
                }
                if let Some(ix) = this.rows.iter().position(|row| row.code == code) {
                    this.promote(ix, window, cx);
                }
            }
        }))
        .child(flag(&code, cx))
        .child(div().w(px(CODE_X - INSET - ROW_PAD - 20.)).flex_shrink_0())
        .child(code_col)
        .child(amount_col)
        .child(chip);
    let content = match (&app.motion.swap, app.motion.settling(&code)) {
        // A reordered row springs into its new place.
        (_, Some((from, version))) => motion::settle(
            content,
            ElementId::NamedInteger(format!("row-settle-{code}").into(), version as u64),
            from,
        ),
        // The old anchor drops into the promoted row's place.
        (Some(swap), None) if swap.demoted == code => {
            motion::slide_in(content, ("row-drop", swap.version), -14.)
        }
        _ => content.into_any_element(),
    };

    // The drag handle: the row's left inset and its flag, where the ⋮⋮
    // grip shows on hover. A press without a drag still promotes the row
    // (the row body under it takes the click); dragging it lifts the row.
    let app_entity = cx.entity();
    let handle = div()
        .id(("row-grip", ix))
        .debug_selector({
            let code = code.clone();
            move || format!("grip-{code}")
        })
        .absolute()
        .left(px(INSET))
        .top_0()
        .w(px(CODE_X - INSET - 8.))
        .h(px(metrics::ROW - 1.))
        .cursor_grab()
        .when(hovered, |el| el.child(grip(&t, 2.)))
        .on_drag(
            DraggedRow { code: code.clone() },
            move |drag, offset, window, cx| {
                app_entity.update(cx, |app, cx| {
                    app.start_row_drag(&drag.code, f32::from(offset.y), window, cx);
                    let view = RowDragView::new(app, &drag.code, window, cx);
                    cx.new(|_| view)
                })
            },
        );

    v_flex()
        .id(("row", ix))
        .relative()
        .px(px(INSET))
        .child(content)
        .child(
            div()
                .ml(px(CODE_X - INSET))
                .mr(px(ROW_PAD))
                .h(px(1.))
                .bg(t.hairline),
        )
        .child(handle)
}

/// `▾ 0.3%` on a 10% tint of its colour, or a muted `·`. The arrow is drawn
/// a little smaller than the figure, as in the mockups.
fn render_chip(change: Change, green_up: bool, t: &Tokens) -> impl IntoElement {
    let (up, pct) = match change {
        Change::Flat => {
            return div()
                .text_size(px(13.))
                .text_color(t.muted)
                .child(change.label());
        }
        Change::Up(pct) => (true, pct),
        Change::Down(pct) => (false, pct),
    };
    let color = t.change(up, green_up);
    let figure = change.label();
    let figure = figure
        .split_once(' ')
        .map_or(figure.as_str(), |(_, f)| f)
        .to_string();
    let _ = pct;
    div()
        .h(px(22.))
        .px(px(7.))
        .flex()
        .items_center()
        .gap(px(5.))
        .rounded(px(6.))
        .bg(color.opacity(CHIP_TINT))
        .text_color(color)
        .font_weight(FontWeight::MEDIUM)
        .font_features(tabular_numerals())
        .whitespace_nowrap()
        .child(div().text_size(px(8.)).child(if up { "▲" } else { "▼" }))
        .child(div().text_size(px(11.)).child(figure))
}

/// Right-click menu: Copy value · Move to top · Remove (docs/DESIGN.md §1).
pub(crate) fn with_context_menu(
    row: Stateful<Div>,
    code: &str,
    cx: &mut Context<KurrencyApp>,
) -> impl IntoElement {
    let app = cx.weak_entity();
    let code = code.to_string();
    row.context_menu(move |menu, _, _| {
        let (copy_app, copy_code) = (app.clone(), code.clone());
        let (top_app, top_code) = (app.clone(), code.clone());
        let (remove_app, remove_code) = (app.clone(), code.clone());
        menu.item(
            PopupMenuItem::new(tr!("row.copy_value"))
                .icon(IconName::Copy)
                .on_click(move |_, _, cx| {
                    let _ = copy_app.update(cx, |app, cx| app.copy_row(&copy_code, cx));
                }),
        )
        .item(
            PopupMenuItem::new(tr!("row.move_to_top")).on_click(move |_, _, cx| {
                let _ = top_app.update(cx, |app, cx| app.move_to_top(&top_code, cx));
            }),
        )
        .separator()
        .item(
            PopupMenuItem::new(tr!("row.remove"))
                .icon(IconName::Delete)
                .on_click(move |_, window, cx| {
                    let _ = remove_app
                        .update(cx, |app, cx| app.remove_currency(&remove_code, window, cx));
                }),
        )
    })
}

/// Every currency removed (docs/DESIGN.md §1).
pub(crate) fn render_empty(cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    v_flex()
        .w_full()
        .py_16()
        .gap_3()
        .items_center()
        .child(
            div()
                .text_size(px(32.))
                .text_color(theme.muted_foreground)
                .child("¤"),
        )
        .child(
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(tr!("empty.message")),
        )
        .child(
            Button::new("add-currency-empty")
                .outline()
                .icon(IconName::Plus)
                .label(tr!("empty.add"))
                .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenPicker), cx)),
        )
}
