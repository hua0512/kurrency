//! The palette (Ctrl+K, DESIGN §2): a search field over the currencies
//! that also reads natural commands. `50 gbp` sets GBP to 50, `+chf` adds,
//! `-jpy` removes, `eur>usd` opens that history; the command is the top
//! entry, run with ↵, above the "Currencies" matches (code, English or
//! Chinese name). ✓ marks listed currencies and confirming one toggles it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui_kit::component::command::{Command, CommandGroup, CommandItem, CommandState};
use gpui_kit::component::{ActiveTheme as _, Icon, WindowExt as _, h_flex};
use gpui_kit::{
    AppContext as _, Context, Entity, Focusable as _, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, StatefulInteractiveElement as _, Styled as _, Window, div, px,
};
use kurrency_core::command;
use kurrency_core::i18n::tr;
use kurrency_core::money;

use crate::app::{KurrencyApp, OpenPicker};
use crate::layout::metrics;
use crate::ui::flag::flag;
use crate::ui::theme::Tokens;

// Lucide "arrow-right-left" (ISC).
const COMMAND_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m16 3 4 4-4 4"/><path d="M20 7H4"/><path d="m8 21-4-4 4-4"/><path d="M4 17h16"/></svg>"#;

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
    if window.has_active_dialog(cx) {
        return;
    }
    // Only real, current ISO currencies that have a rate.
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
                        let command = app.palette_command(&query, cx);
                        let codes = command::search(&parsed.search, &app.picker.codes);
                        let item = command.as_ref().map(|command| {
                            let label = command::label_in(command, app.fmt.style);
                            CommandItem::new().label(label.clone()).child(move |_, _| {
                                h_flex()
                                    .flex_1()
                                    .gap_3()
                                    .child(
                                        Icon::default()
                                            .data(COMMAND_SVG)
                                            .size_4()
                                            .text_color(t.accent),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(label.clone()),
                                    )
                                    .child(div().text_color(t.muted).child("↵"))
                            })
                        });
                        let group = CommandGroup::new()
                            .label(tr!("palette.currencies"))
                            .items(codes.iter().map(|code| currency_item(code, &list)));
                        *shown.borrow_mut() = Shown { command, codes };
                        (item, group)
                    })
                    .unwrap_or_else(|_| (None, CommandGroup::new()));

                let confirm_owner = owner.clone();
                let confirm_shown = shown.clone();
                let mut palette = Command::new(&state)
                    .bordered(false)
                    .filterable(false)
                    .rounded(cx.theme().radius_lg)
                    .placeholder(tr!("palette.placeholder"))
                    .max_h(px(320.));
                let has_command = command_item.is_some();
                if let Some(item) = command_item {
                    palette = palette.item(item);
                }
                // The dialog is drawn outside the app's root view.
                crate::assets::with_cjk_fallback(content).child(
                    palette
                        .group(group)
                        // The items depend on the query: draw them again.
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
                            // After the dialog has given focus back, so a
                            // history page keeps it.
                            window.defer(cx, move |window, cx| {
                                let _ = owner.update(cx, |app, cx| match (command, code) {
                                    (Some(command), _) => app.run_command(command, window, cx),
                                    (None, Some(code)) => {
                                        if app.rows.iter().any(|row| row.code == code) {
                                            app.remove_currency(&code, window, cx);
                                        } else {
                                            app.add_currency(&code, window, cx);
                                        }
                                    }
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

/// A currency row: flag, code and name; ✓ when it is in the list.
fn currency_item(code: &str, list: &[String]) -> CommandItem {
    let name = money::display_name(code);
    let row_code = code.to_string();
    let row_name = name.clone();
    CommandItem::new()
        // Read out; the child draws the row.
        .label(format!("{code}  {name}"))
        .checked(list.iter().any(|c| c == code))
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
                        .min_w(px(0.))
                        .truncate()
                        .text_color(t.muted)
                        .child(row_name.clone()),
                )
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
