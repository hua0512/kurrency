//! Drawing the root view: the header, the anchor card, the list, and the
//! page, toast and dragged row over them, with every action handler bound.

use gpui_kit::component::{ActiveTheme as _, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Animation, AnimationExt as _, Context, InteractiveElement, IntoElement, ParentElement as _,
    Render, StatefulInteractiveElement as _, Styled as _, Window, div, ease_out_quint, px,
    relative,
};

use super::KurrencyApp;
use super::actions::{
    CONTEXT, MoveRowDown, MoveRowUp, RedoChange, StepDown, StepDownTen, StepUp, StepUpTen,
    UndoChange,
};
use crate::layout;
use crate::ui;

impl Render for KurrencyApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.editing.is_some() && !self.anchor_focused(window, cx) {
            cx.defer_in(window, |this, window, cx| {
                if !this.anchor_focused(window, cx) {
                    this.end_editing(window, cx);
                }
            });
        }
        if self.fitted_for != Some(self.rows.len()) {
            self.fitted_for = Some(self.rows.len());
            cx.defer_in(window, |this, window, cx| {
                this.fit_window(false, window, cx);
            });
        }
        // A drag let go outside the window ends without a drop.
        if self.drag.is_some() && !cx.has_active_drag() && self.debug_lifted.is_none() {
            self.drag = None;
            cx.defer_in(window, |_, _, cx| cx.notify());
        }
        self.update_rolls(window, cx);
        let theme = cx.theme();
        let (bg, fg) = (theme.background, theme.foreground);

        let body = if self.rows.is_empty() {
            ui::row::render_empty(cx).into_any_element()
        } else {
            v_flex()
                .w_full()
                .pb(px(layout::metrics::BOTTOM))
                .child(ui::anchor::render(self, window, cx))
                .child(ui::anchor::render_quick_amounts(self, cx))
                .child(ui::row::render_list(self, window, cx))
                .child(ui::picker::render_add_row(cx))
                .into_any_element()
        };

        crate::assets::with_cjk_fallback(v_flex())
            .id("kurrency")
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::refresh_rates))
            .on_action(cx.listener(Self::open_picker))
            .on_action(cx.listener(Self::open_settings))
            .on_action(cx.listener(Self::open_row_detail))
            .on_action(cx.listener(Self::open_focused_detail))
            .on_action(cx.listener(Self::select_prev))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::promote_focused))
            .on_action(cx.listener(Self::copy_focused))
            .on_action(cx.listener(Self::swap_anchor))
            .on_action(cx.listener(Self::on_smart_paste))
            .on_action(cx.listener(|this, _: &UndoChange, window, cx| this.undo_change(window, cx)))
            .on_action(cx.listener(|this, _: &RedoChange, window, cx| this.redo_change(window, cx)))
            .on_action(
                cx.listener(|this, _: &MoveRowUp, window, cx| this.move_focused(false, window, cx)),
            )
            .on_action(cx.listener(|this, _: &MoveRowDown, window, cx| {
                this.move_focused(true, window, cx);
            }))
            // A dragged row drops wherever it is let go.
            .on_drop(cx.listener(|this, _: &ui::row::DraggedRow, _, cx| this.drop_row(cx)))
            .capture_action(cx.listener(Self::on_input_paste))
            .on_action(
                cx.listener(|this, _: &StepUp, window, cx| this.step(true, false, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &StepDown, window, cx| this.step(false, false, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &StepUpTen, window, cx| this.step(true, true, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &StepDownTen, window, cx| this.step(false, true, window, cx)),
            )
            .capture_action(cx.listener(
                |this, _: &gpui_kit::component::input::Escape, window, cx| {
                    if this.anchor_focused(window, cx) {
                        this.on_anchor_escape(window, cx);
                        cx.stop_propagation();
                    }
                },
            ))
            .on_key_down(cx.listener(Self::on_key_down))
            .size_full()
            .bg(bg)
            .text_color(fg)
            .child(ui::header::render(self, cx))
            .child(
                v_flex()
                    .relative()
                    .flex_1()
                    .min_h(px(0.))
                    .w_full()
                    .child(
                        div()
                            .id("rows-scroll")
                            .size_full()
                            .overflow_y_scroll()
                            .child(body),
                    )
                    .when_some(self.toast.as_ref(), |el, toast| {
                        let area =
                            f32::from(window.viewport_size().height) - layout::metrics::HEADER;
                        let top = ui::toast::top(self.rows.len(), area);
                        el.child(ui::toast::render(toast, top, cx))
                    })
                    // Closing, it slides back out, drawn but inert: the
                    // list underneath takes clicks and hover at once.
                    .when_some(self.closing.as_ref(), |el, (view, version)| {
                        el.child(
                            div()
                                .absolute()
                                .top_0()
                                .bottom_0()
                                .w_full()
                                .child(ui::inert::inert(view.clone()))
                                .with_animation(
                                    ("detail-slide-out", *version),
                                    Animation::new(ui::detail::SLIDE_DURATION)
                                        .with_easing(ease_out_quint()),
                                    |el, t| el.left(relative(t)),
                                ),
                        )
                    })
                    // A page slides in from the right over the list.
                    .when_some(self.page.as_ref(), |el, page| {
                        el.child(
                            div()
                                .absolute()
                                .top_0()
                                .bottom_0()
                                .w_full()
                                .child(page.view.any())
                                .with_animation(
                                    ("page-slide", self.page_opens),
                                    Animation::new(ui::detail::SLIDE_DURATION)
                                        .with_easing(ease_out_quint()),
                                    |el, t| el.left(relative(1. - t)),
                                ),
                        )
                    }),
            )
            .min_h(px(0.))
            .when_some(
                self.debug_lifted.clone().zip(self.drag.as_ref()),
                |el, (view, drag)| {
                    el.child(
                        div()
                            .absolute()
                            .top(px(drag.pointer_y - drag.grab_y))
                            .left(px(layout::metrics::PAD_X))
                            .child(view),
                    )
                },
            )
    }
}
