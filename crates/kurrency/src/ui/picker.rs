//! The palette (Ctrl+K, DESIGN §2): a search field over the currencies
//! that also reads natural commands. `50 gbp` sets GBP to 50 (showing a live
//! conversion preview right inside the command row), `+chf` adds, `-jpy`
//! removes, `eur>usd` opens that history. Confirming a currency already in
//! the list switches it into the anchor instead of deleting it; an empty
//! search surfaces listed and popular currencies first.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui_kit::component::command::{Command, CommandGroup, CommandItem, CommandState};
use gpui_kit::component::{ActiveTheme as _, Icon, WindowExt as _, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AppContext as _, Context, Entity, Focusable as _, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, StatefulInteractiveElement as _, Styled as _, Window, div, px,
};
use kurrency_core::command;
use kurrency_core::i18n::tr;
use kurrency_core::money;
use kurrency_core::present;

use crate::app::{KurrencyApp, OpenPicker};
use crate::assets;
use crate::layout::metrics;
use crate::ui::anchor::tabular_numerals;
use crate::ui::flag::flag;
use crate::ui::theme::Tokens;

// Lucide "arrow-right-left" (ISC).
const COMMAND_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m16 3 4 4-4 4"/><path d="M20 7H4"/><path d="m8 21-4-4 4-4"/><path d="M4 17h16"/></svg>"#;

/// What confirming a currency item does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PickerMode {
    /// Ctrl+K / + Add / type-anywhere: commands work; listed currencies switch
    /// to anchor, unlisted currencies are added.
    Default,
    /// Clicked the anchor currency badge: picking any currency makes it the
    /// anchor (adding it if needed) while keeping the current value.
    SelectAnchor,
}

pub(crate) struct Picker {
    state: Entity<CommandState>,
    /// Every currency the palette can offer: ISO currencies with a rate.
    codes: Vec<String>,
}

/// What the palette shows for the current query, shared by its rendering
/// and its confirm handler.
#[derive(Default)]
struct Shown {
    command: Option<command::Command>,
    codes: Vec<String>,
}

impl Picker {
    #[cfg(test)]
    pub(crate) fn codes(&self) -> &[String] {
        &self.codes
    }

    #[cfg(test)]
    pub(crate) fn query(&self, cx: &gpui_kit::App) -> String {
        self.state.read(cx).query(cx).to_string()
    }

    pub(crate) fn new(window: &mut Window, cx: &mut Context<KurrencyApp>) -> Self {
        Self {
            state: cx.new(|cx| CommandState::new(window, cx)),
            codes: Vec::new(),
        }
    }
}

/// Opens the palette over the list, with `query` typed in (type-anywhere).
pub(crate) fn open(
    app: &mut KurrencyApp,
    query: &str,
    window: &mut Window,
    cx: &mut Context<KurrencyApp>,
) {
    open_mode(app, query, PickerMode::Default, window, cx);
}

/// Opens the palette in the given [`PickerMode`].
pub(crate) fn open_mode(
    app: &mut KurrencyApp,
    query: &str,
    mode: PickerMode,
    window: &mut Window,
    cx: &mut Context<KurrencyApp>,
) {
    if window.has_active_dialog(cx) {
        return;
    }
    app.picker.codes = app
        .rates
        .read(cx)
        .table()
        .codes()
        .into_iter()
        .filter(|code| money::is_regular_currency(code))
        .collect();
    let state = app.picker.state.clone();
    let query = query.to_string();
    state.update(cx, |state, cx| state.set_query(query, window, cx));

    let owner = cx.weak_entity();
    let focus_on_mount = Rc::new(Cell::new(true));
    let shown = Rc::new(RefCell::new(Shown::default()));
    window.open_dialog(cx, move |dialog, _, _| {
        let state = state.clone();
        let owner = owner.clone();
        let focus_on_mount = focus_on_mount.clone();
        let shown = shown.clone();
        dialog
            .close_button(false)
            .p_0()
            .w(px(320.))
            .content(move |content, window, cx| {
                if focus_on_mount.replace(false) {
                    let state = state.clone();
                    window.defer(cx, move |window, cx| {
                        state.read(cx).focus_handle(cx).focus(window, cx);
                    });
                }
                let query = state.read(cx).query(cx).to_string();
                let t = Tokens::get(cx);
                let (command_item, group) = owner
                    .read_with(cx, |app, cx| {
                        let list: Vec<String> =
                            app.rows.iter().map(|row| row.code.clone()).collect();
                        let parsed = command::parse(&query, &list);
                        let command = (mode == PickerMode::Default)
                            .then(|| app.palette_command(&query, cx))
                            .flatten();
                        let codes = if parsed.search.is_empty() {
                            command::order_for_palette(&app.picker.codes, &list)
                        } else {
                            command::search(&parsed.search, &app.picker.codes)
                        };
                        let item = command.as_ref().map(|command| {
                            let label = command::label_in(command, app.fmt.style);
                            let preview = command_preview(app, command, cx);
                            CommandItem::new().label(label.clone()).child(move |_, _| {
                                h_flex()
                                    .flex_1()
                                    .gap_2()
                                    .child(
                                        Icon::default()
                                            .data(COMMAND_SVG)
                                            .size_4()
                                            .text_color(t.accent),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .truncate()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(label.clone()),
                                    )
                                    .when_some(preview.clone(), |el, preview| {
                                        el.child(
                                            div()
                                                .flex_shrink_0()
                                                .text_size(px(12.))
                                                .text_color(t.muted)
                                                .font_features(tabular_numerals())
                                                .child(preview),
                                        )
                                    })
                                    .child(div().flex_shrink_0().text_color(t.muted).child("↵"))
                            })
                        });
                        let anchor = app.anchor().map(str::to_string);
                        let group = CommandGroup::new().label(tr!("palette.currencies")).items(
                            codes
                                .iter()
                                .map(|code| currency_item(code, &list, anchor.as_deref())),
                        );
                        *shown.borrow_mut() = Shown { command, codes };
                        (item, group)
                    })
                    .unwrap_or_else(|_| (None, CommandGroup::new()));

                let confirm_owner = owner.clone();
                let confirm_shown = shown.clone();
                let placeholder = match mode {
                    PickerMode::Default => tr!("palette.placeholder"),
                    PickerMode::SelectAnchor => tr!("palette.select_anchor_placeholder"),
                };
                let mut palette = Command::new(&state)
                    .bordered(false)
                    .filterable(false)
                    .rounded(cx.theme().radius_lg)
                    .placeholder(placeholder)
                    .max_h(px(320.));
                let has_command = command_item.is_some();
                if let Some(item) = command_item {
                    palette = palette.item(item);
                }
                assets::with_cjk_fallback(content).child(
                    palette
                        .group(group)
                        .on_query(|_, window, _| window.refresh())
                        .on_confirm(move |index, window, cx| {
                            let shown = confirm_shown.borrow();
                            let command = if has_command && index.section == 0 {
                                shown.command.clone()
                            } else {
                                None
                            };
                            let code = shown.codes.get(index.row).cloned();
                            drop(shown);
                            window.close_dialog(cx);
                            let owner = confirm_owner.clone();
                            window.defer(cx, move |window, cx| {
                                let _ = owner.update(cx, |app, cx| match (command, code) {
                                    (Some(command), _) => app.run_command(command, window, cx),
                                    (None, Some(code)) => match mode {
                                        PickerMode::SelectAnchor => {
                                            app.select_anchor(&code, window, cx);
                                        }
                                        PickerMode::Default => {
                                            if let Some(ix) =
                                                app.rows.iter().position(|row| row.code == code)
                                            {
                                                if ix > 0 {
                                                    app.promote(ix, window, cx);
                                                }
                                            } else {
                                                app.add_currency(&code, window, cx);
                                            }
                                        }
                                    },
                                    (None, None) => {}
                                });
                            });
                        })
                        .footer(|_, _, cx| {
                            h_flex()
                                .px_3()
                                .py_2()
                                .border_t_1()
                                .border_color(cx.theme().border)
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(tr!("palette.footer"))
                        }),
                )
            })
    });
}

/// Live conversion preview (`≈ $67.20`) for a `SetAmount` command so users can
/// check a quick price without committing over their current anchor.
fn command_preview(
    app: &KurrencyApp,
    command: &command::Command,
    cx: &gpui_kit::App,
) -> Option<String> {
    let command::Command::SetAmount { code, amount } = command else {
        return None;
    };
    let target = app
        .rows
        .iter()
        .map(|row| row.code.as_str())
        .find(|c| *c != code.as_str())?;
    let converted = app.rates.read(cx).table().convert(*amount, code, target)?;
    let parts = present::amount_parts(target, converted, &app.fmt, assets::has_glyph);
    let prefix = match parts.symbol {
        Some(sym) => format!("{sym} "),
        None => String::new(),
    };
    Some(format!("≈ {prefix}{}{}", parts.integer, parts.fraction))
}

/// A currency row: flag, code and name; ✓ on the anchor, subtle "Switch" hint
/// on listed rows so Enter's action is clear.
fn currency_item(code: &str, list: &[String], anchor: Option<&str>) -> CommandItem {
    let name = money::display_name(code);
    let row_code = code.to_string();
    let row_name = name.clone();
    let is_anchor = anchor == Some(code);
    let is_listed = list.iter().any(|c| c == code);
    CommandItem::new()
        .label(format!("{code}  {name}"))
        .checked(is_anchor)
        .child(move |_, cx| {
            let t = Tokens::get(cx);
            h_flex()
                .flex_1()
                .gap_3()
                .child(flag(&row_code, cx))
                .child(
                    div()
                        .w(px(36.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(row_code.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .text_color(t.muted)
                        .child(row_name.clone()),
                )
                .when(is_listed && !is_anchor, |el| {
                    el.child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(11.))
                            .text_color(t.muted)
                            .child(tr!("palette.switch_hint")),
                    )
                })
        })
}

/// "+ Add" on the left and a muted "type to search" on the right: the
/// palette also opens by typing a letter anywhere.
pub(crate) fn render_add_row(cx: &mut Context<KurrencyApp>) -> impl IntoElement {
    let t = Tokens::get(cx);
    h_flex()
        .id("add-currency")
        .mx(px(metrics::PAD_X))
        .px(px(8.))
        .h(px(metrics::ADD))
        .justify_between()
        .rounded(px(8.))
        .cursor_pointer()
        .hover(|s| s.bg(t.hover))
        .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenPicker), cx))
        .child(
            div()
                .text_size(px(15.))
                .font_weight(FontWeight::MEDIUM)
                .child(tr!("add_row.add")),
        )
        .child(
            div()
                .text_size(px(13.))
                .text_color(t.muted)
                .child(tr!("add_row.hint")),
        )
}
