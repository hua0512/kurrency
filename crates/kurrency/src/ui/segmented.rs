//! A segmented control: a hairline pill of segments, the selected one marked
//! like the selected quick-amount chip (accent border and content over an
//! accent tint). Used instead of gpui-kit's segmented `TabBar`, whose
//! selected tab takes the window background, which is darker than the bar
//! in the dark themes and so barely shows.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{Div, ElementId, InteractiveElement as _, Stateful, Styled as _, div, px};

use crate::ui::theme::Tokens;

/// The pill the segments sit in.
pub(crate) fn bar(t: &Tokens) -> Div {
    div()
        .flex()
        .flex_row()
        .p(px(2.))
        .gap(px(2.))
        .rounded(px(9.))
        .border_1()
        .border_color(t.hairline)
}

/// One segment, 26 px tall, its content centred; the caller sizes it and
/// adds the content and the click handler.
pub(crate) fn segment(id: impl Into<ElementId>, selected: bool, t: &Tokens) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(26.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(7.))
        .border_1()
        .cursor_pointer()
        .map(|el| {
            if selected {
                el.border_color(t.accent)
                    .bg(t.accent.opacity(0.14))
                    .text_color(t.accent)
            } else {
                let (hover, text) = (t.hover, t.text);
                el.border_color(gpui_kit::transparent_black())
                    .text_color(t.muted)
                    .hover(move |s| s.bg(hover).text_color(text))
            }
        })
}
