//! Layout metrics (logical px) and the window geometry that follows from
//! them: the height that fits the list, where a remembered frame reopens,
//! and Wayland's quirks (docs/DESIGN.md §1).
//!
//! Plain Rust without GPUI, so all of it is unit-tested here.

use kurrency_core::settings::WindowGeometry;

/// Layout metrics (logical px) shared by the views and the window fit.
pub(crate) mod metrics {
    pub(crate) const WIDTH: f32 = 360.;
    pub(crate) const MIN_WIDTH: f32 = 320.;
    pub(crate) const MIN_HEIGHT: f32 = 440.;
    /// Kept free above and below the window when capping its height.
    pub(crate) const SCREEN_MARGIN: f32 = 48.;

    pub(crate) const PAD_X: f32 = 16.;
    pub(crate) const HEADER: f32 = 48.;
    /// The focus-ring gutter around the card (2px ring + 2px offset).
    pub(crate) const RING: f32 = 4.;
    pub(crate) const CARD_PAD_TOP: f32 = 14.;
    pub(crate) const CARD_PAD_BOTTOM: f32 = 12.;
    pub(crate) const CARD_TITLE: f32 = 24.;
    pub(crate) const CARD_AMOUNT: f32 = 44.;
    pub(crate) const CARD_ECHO: f32 = 16.;
    pub(crate) const CHIPS_GAP: f32 = 8.;
    pub(crate) const CHIPS: f32 = 28.;
    pub(crate) const LIST_GAP: f32 = 8.;
    /// A row's pitch: 55px of content and its 1px hairline.
    pub(crate) const ROW: f32 = 56.;
    pub(crate) const ADD: f32 = 44.;
    pub(crate) const BOTTOM: f32 = 12.;
    /// The empty state's block (illustration, text, button).
    pub(crate) const EMPTY: f32 = 220.;

    pub(crate) const CARD: f32 =
        CARD_PAD_TOP + CARD_TITLE + CARD_AMOUNT + CARD_ECHO + CARD_PAD_BOTTOM + 2.;
}

/// The content height for `list_rows` read-only rows under the anchor card,
/// or the empty state when there are no currencies at all.
pub(crate) fn content_height(currencies: usize) -> f32 {
    use metrics::*;
    if currencies == 0 {
        return HEADER + EMPTY;
    }
    let rows = (currencies - 1) as f32;
    HEADER + 2. * RING + CARD + CHIPS_GAP + CHIPS + LIST_GAP + rows * ROW + ADD + BOTTOM
}

/// The window height for the content: at least [`metrics::MIN_HEIGHT`], at
/// most the screen's visible height minus a margin.
pub(crate) fn fit_height(currencies: usize, visible_height: Option<f32>) -> f32 {
    let wanted = content_height(currencies).max(metrics::MIN_HEIGHT);
    match visible_height {
        Some(screen) => wanted
            .min(screen - 2. * metrics::SCREEN_MARGIN)
            .max(metrics::MIN_HEIGHT.min(screen)),
        None => wanted,
    }
    .round()
}

/// Where to reopen the window from its remembered frame, given the
/// connected displays' frames and the height that fits the content (which
/// wins over the remembered height, so auto-fit still applies). `None` when
/// the remembered title bar is on no connected display: the caller centres
/// the window. Otherwise the frame is moved fully onto its display.
pub(crate) fn restore_frame(
    saved: WindowGeometry,
    displays: &[WindowGeometry],
    height: f32,
) -> Option<(usize, WindowGeometry)> {
    if !(saved.x.is_finite() && saved.y.is_finite() && saved.width.is_finite()) {
        return None;
    }
    let width = saved.width.max(metrics::MIN_WIDTH);
    // The middle of the title bar must be on a display.
    let (px, py) = (saved.x + width / 2., saved.y + metrics::HEADER / 2.);
    let ix = displays
        .iter()
        .position(|d| px >= d.x && px < d.x + d.width && py >= d.y && py < d.y + d.height)?;
    let d = displays[ix];
    let width = width.min(d.width);
    let height = height.min(d.height);
    let x = saved.x.clamp(d.x, d.x + d.width - width);
    let y = saved.y.clamp(d.y, d.y + d.height - height);
    Some((
        ix,
        WindowGeometry {
            x,
            y,
            width,
            height,
        },
    ))
}

/// Whether GPUI runs on Wayland here (it does whenever `WAYLAND_DISPLAY` is
/// set). A Wayland client cannot read or set its window's position, so
/// only the window's size is remembered there.
pub(crate) fn on_wayland() -> bool {
    cfg!(target_os = "linux") && std::env::var_os("WAYLAND_DISPLAY").is_some()
}

/// Undoes a display-size bug: on Wayland, gpui-pre 0.3.11 divides the
/// output's device height by the integer `wl_output` scale, which rounds a
/// fractional scale up (1.25 → 2), so a 1080px screen reads as 540 logical
/// px instead of 864. Given the reported height and the window's actual
/// (fractional) scale factor, returns the real logical height.
pub(crate) fn wayland_logical_height(reported: f32, scale: f32) -> f32 {
    let integer = scale.ceil();
    if scale > 0. && integer != scale {
        reported * integer / scale
    } else {
        reported
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wayland_fractional_scale() {
        assert_eq!(wayland_logical_height(540., 1.25), 864.);
        assert_eq!(wayland_logical_height(540., 2.), 540.);
        assert_eq!(wayland_logical_height(1080., 1.), 1080.);
    }

    #[test]
    fn window_fit() {
        use metrics::*;
        // Four rows under the card; taller than the minimum.
        let four = content_height(5);
        assert_eq!(four, content_height(4) + ROW);
        assert!(four > MIN_HEIGHT);
        assert_eq!(fit_height(5, None), four.round());
        // Short lists keep the minimum; long ones stop at the screen.
        assert_eq!(fit_height(1, None), MIN_HEIGHT);
        assert_eq!(fit_height(0, Some(1080.)), MIN_HEIGHT);
        assert_eq!(fit_height(40, Some(1080.)), 1080. - 2. * SCREEN_MARGIN);
        // A screen shorter than the minimum wins over the minimum.
        assert_eq!(fit_height(40, Some(400.)), 400.);
    }

    #[test]
    fn window_restore() {
        let frame = |x, y, width, height| WindowGeometry {
            x,
            y,
            width,
            height,
        };
        let displays = [frame(0., 0., 1920., 1080.), frame(1920., 0., 1280., 800.)];
        // On the second display, with the fitted height instead of the saved one.
        assert_eq!(
            restore_frame(frame(2000., 100., 380., 900.), &displays, 490.),
            Some((1, frame(2000., 100., 380., 490.)))
        );
        // Hanging off the bottom right: moved back onto the display.
        assert_eq!(
            restore_frame(frame(1700., 900., 360., 500.), &displays, 490.),
            Some((0, frame(1560., 590., 360., 490.)))
        );
        // Too narrow is widened to the minimum.
        assert_eq!(
            restore_frame(frame(10., 10., 100., 500.), &displays, 490.).map(|(_, f)| f.width),
            Some(metrics::MIN_WIDTH)
        );
        // A display that is gone, or nonsense: centre instead.
        assert_eq!(
            restore_frame(frame(4000., 100., 360., 500.), &displays, 490.),
            None
        );
        assert_eq!(
            restore_frame(frame(100., -500., 360., 500.), &displays, 490.),
            None
        );
        assert_eq!(
            restore_frame(frame(f32::NAN, 0., 360., 500.), &displays, 490.),
            None
        );
        assert_eq!(restore_frame(frame(0., 0., 360., 500.), &[], 490.), None);
        // A screen shorter than the content.
        assert_eq!(
            restore_frame(
                frame(0., 0., 360., 500.),
                &[frame(0., 0., 1024., 400.)],
                490.
            ),
            Some((0, frame(0., 0., 360., 400.)))
        );
    }
}
