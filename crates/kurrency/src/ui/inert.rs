//! An element that is drawn but takes no mouse input: no clicks, hover
//! styles or tooltips, and nothing behind it is blocked. The closing pages
//! slide out through it, so the list underneath takes input at once.
//!
//! GPUI hit-tests a hitbox inside the content mask it was inserted with,
//! during prepaint, while painting clips to the mask at paint time. Prepainting
//! the child under an empty mask therefore leaves every hitbox it inserts
//! empty, and paints it as usual.

use gpui_kit::{
    AnyElement, App, Bounds, ContentMask, Element, ElementId, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Pixels, Window,
};

pub(crate) struct Inert(AnyElement);

/// Draws `child` without letting it take mouse input.
pub(crate) fn inert(child: impl IntoElement) -> Inert {
    Inert(child.into_any_element())
}

impl IntoElement for Inert {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Inert {
    type RequestLayoutState = ();
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
    ) -> (LayoutId, ()) {
        (self.0.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let nowhere = ContentMask {
            bounds: Bounds::default(),
        };
        window.with_content_mask(Some(nowhere), |window| {
            self.0.prepaint(window, cx);
        });
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.0.paint(window, cx);
    }
}
