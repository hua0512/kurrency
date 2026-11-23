//! The characters of an amount at rest, drawn by one element.
//!
//! A digit roll (motion.rs) needs one element per character, so each can move
//! and fade on its own. Laying out two nodes per character (a div and its
//! text) is a large part of a row's cost, though, and a window resize redraws
//! every row in every frame. While nothing rolls, [`digits`] therefore lays a run
//! out as a single node and paints the characters itself, at the positions
//! that one text element per character gets. A roll that starts or ends
//! moves nothing.
//!
//! Those positions come from GPUI's measured-text sizes: a text element is
//! rounded up to a whole logical pixel (`TextLayout`), then to a whole device
//! pixel (taffy's measured-size snapping), and the characters of a flex row
//! sit end to end on those widths.

use gpui_kit::{
    App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, LayoutId, Pixels,
    ShapedLine, SharedString, Style, Window, point, px, size,
};

use crate::ui::motion::char_text;

/// The characters of `text`, laid out and drawn the way one text element
/// per character would be, in the inherited text style.
pub(crate) fn digits(text: &str) -> Digits {
    Digits {
        text: text.to_string().into(),
    }
}

pub(crate) struct Digits {
    text: SharedString,
}

/// Each character shaped, with its offset from the run's left edge in device
/// pixels.
pub(crate) struct Laid {
    chars: Vec<(ShapedLine, f32)>,
    line_height: Pixels,
}

impl Element for Digits {
    type RequestLayoutState = Laid;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Laid) {
        let text_style = window.text_style();
        let rem_size = window.rem_size();
        let font_size = text_style.font_size.to_pixels(rem_size);
        // As `TextLayout::layout` computes it.
        let line_height =
            window.pixel_snap(text_style.line_height.to_pixels(font_size.into(), rem_size));
        let scale = window.scale_factor();
        let mut chars = Vec::with_capacity(self.text.len());
        let mut x = 0.;
        for ch in self.text.chars() {
            let text = char_text(ch);
            let run = text_style.to_run(text.len());
            let line = window
                .text_system()
                .shape_line(text, font_size, &[run], None);
            // Its text element's width: whole logical, then device, pixels.
            let width = (f32::from(line.width).max(0.).ceil() * scale).ceil();
            chars.push((line, x));
            x += width;
        }
        let height = (f32::from(line_height) * scale).ceil();
        let style = Style {
            size: size(px(x / scale).into(), px(height / scale).into()),
            // A text element never shrinks below its text.
            flex_shrink: 0.,
            ..Style::default()
        };
        let layout_id = window.request_layout(style, [], cx);
        (layout_id, Laid { chars, line_height })
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Laid,
        _: &mut Window,
        _: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        laid: &mut Laid,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let align = window.text_style().text_align;
        let scale = window.scale_factor();
        // The snapped left edge, in device pixels; each character's text
        // element would start a whole number of device pixels after it.
        let left = (f32::from(bounds.origin.x) * scale).round();
        for (line, x) in &laid.chars {
            let origin = point(px((left + x) / scale), bounds.origin.y);
            if let Err(err) = line.paint(origin, laid.line_height, align, None, window, cx) {
                log::error!("painting an amount: {err}");
            }
        }
    }
}
