//! A currency's flag: a 20×15 bundled full-colour image with a hairline
//! border, so white flags (JP) stay visible (docs/DESIGN.md §5).
//!
//! `img()` rasterises SVG assets in colour through resvg, unlike `svg()`,
//! which is a mask tinted with the text colour.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{
    AnyElement, App, FontWeight, IntoElement as _, ParentElement as _, Styled as _, div, img, px,
};
use kurrency_core::money;

use crate::assets;

const WIDTH: f32 = 20.;
const HEIGHT: f32 = 15.;
const RADIUS: f32 = 3.;

/// The flag of `code`, or a neutral badge of the same size with the code's
/// first two letters (or `¤`) when no flag is bundled.
pub(crate) fn flag(code: &str, cx: &App) -> AnyElement {
    let theme = cx.theme();
    match assets::flag_path(code) {
        // GPUI paints an element's border over its content, so the hairline
        // sits on top of the image's rounded edge.
        Some(path) => img(path)
            .flex_shrink_0()
            .w(px(WIDTH))
            .h(px(HEIGHT))
            .rounded(px(RADIUS))
            .border_1()
            .border_color(theme.border)
            .into_any_element(),
        None => div()
            .flex_shrink_0()
            .w(px(WIDTH))
            .h(px(HEIGHT))
            .rounded(px(RADIUS))
            .border_1()
            .border_color(theme.border)
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.muted)
            .text_color(theme.muted_foreground)
            .text_size(px(9.))
            .font_weight(FontWeight::SEMIBOLD)
            .child(money::flag_badge(code))
            .into_any_element(),
    }
}

/// A miniature flag for the theme gallery's previews, `width` wide (4:3),
/// with the preview theme's `hairline`. Codes without a flag get a plain
/// muted tile.
pub(crate) fn mini_flag(code: &str, width: f32, hairline: gpui_kit::Hsla) -> AnyElement {
    let height = (width * 0.75).round();
    match assets::flag_path(code) {
        Some(path) => img(path)
            .flex_shrink_0()
            .w(px(width))
            .h(px(height))
            .rounded(px(1.5))
            .border_1()
            .border_color(hairline)
            .into_any_element(),
        None => div()
            .flex_shrink_0()
            .w(px(width))
            .h(px(height))
            .rounded(px(1.5))
            .bg(hairline)
            .into_any_element(),
    }
}
