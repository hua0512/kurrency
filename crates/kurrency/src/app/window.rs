//! The window: its options at startup (the remembered frame, on a display
//! that is still connected), fitting its height to the list (DESIGN §1),
//! and remembering its frame.

use std::time::Duration;

use gpui_kit::component::TitleBar;
use gpui_kit::{App, Bounds, Context, Window, WindowBounds, WindowOptions, point, px, size};
use kurrency_core::settings;

use super::KurrencyApp;
use crate::layout;

/// The main window's options: titled, at the height that fits the list, at
/// the remembered position and width when they are still on a connected
/// display (otherwise centred), with the Wayland app id.
pub(crate) fn window_options(cx: &mut App) -> WindowOptions {
    let mut options = TitleBar::window_options();
    if let Some(titlebar) = &mut options.titlebar {
        titlebar.title = Some("Kurrency".into());
    }
    // Open at the height that fits the list, so floating window
    // managers get a compact size hint; the view keeps fitting it.
    // The remembered position and width come back when they are
    // still on a connected display; otherwise the window is centred.
    let settings = settings::load(
        settings::default_path().as_deref(),
        sys_locale::get_locale().as_deref(),
    );
    let visible = cx
        .primary_display()
        .map(|display| f32::from(display.visible_bounds().size.height));
    let height = layout::fit_height(settings.currencies.len(), visible);
    let displays = cx.displays();
    let frames: Vec<settings::WindowGeometry> = displays
        .iter()
        .map(|display| {
            let b = display.bounds();
            settings::WindowGeometry {
                x: f32::from(b.origin.x),
                y: f32::from(b.origin.y),
                width: f32::from(b.size.width),
                height: f32::from(b.size.height),
            }
        })
        .collect();
    // On Wayland the compositor places the window and only the size
    // is remembered (see `SavedWindow`), so it is never positioned.
    let restored = settings
        .window
        .filter(|_| !layout::on_wayland())
        .and_then(settings::SavedWindow::frame)
        .and_then(|saved| layout::restore_frame(saved, &frames, height));
    let width = settings
        .window
        .map(|saved| saved.width)
        .filter(|width| width.is_finite())
        .map_or(layout::metrics::WIDTH, |width| {
            width.max(layout::metrics::MIN_WIDTH)
        });
    options.window_bounds = Some(WindowBounds::Windowed(match restored {
        Some((ix, frame)) => {
            options.display_id = Some(displays[ix].id());
            Bounds::new(
                point(px(frame.x), px(frame.y)),
                size(px(frame.width), px(frame.height)),
            )
        }
        None => Bounds::centered(None, size(px(width), px(height)), cx),
    }));
    options.window_min_size = Some(size(
        px(layout::metrics::MIN_WIDTH),
        px(layout::metrics::MIN_HEIGHT),
    ));
    options.app_id = Some("dev.kurrency.Kurrency".to_string());
    options
}

impl KurrencyApp {
    /// The window moved or was resized: remember its frame a moment after
    /// it settles.
    pub(super) fn window_moved(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let _ = window;
        self._save_bounds = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(500))
                .await;
            let _ = this.update_in(cx, |this, window, _| {
                this.remember_bounds(window);
                this._save_bounds = None;
            });
        }));
    }

    /// Saves the window's frame (windowed only; maximised and fullscreen
    /// windows keep the last windowed one).
    ///
    /// Wayland: the size only. The compositor owns the position, which a
    /// client cannot read: GPUI reports the origin the window was opened
    /// with, so saving it would only remember a made-up place.
    pub(super) fn remember_bounds(&mut self, window: &Window) {
        let WindowBounds::Windowed(bounds) = window.window_bounds() else {
            return;
        };
        let position = (!layout::on_wayland())
            .then(|| (f32::from(bounds.origin.x), f32::from(bounds.origin.y)));
        let frame = settings::SavedWindow {
            x: position.map(|(x, _)| x),
            y: position.map(|(_, y)| y),
            width: f32::from(bounds.size.width),
            height: f32::from(bounds.size.height),
        };
        if self.prefs.window != Some(frame) {
            self.prefs.window = Some(frame);
            self.save_settings();
        }
    }

    /// Resizes the window height to the content (DESIGN §1), keeping the
    /// width (or resetting it to the default). Tiling compositors may ignore
    /// the request.
    pub(super) fn fit_window(
        &mut self,
        reset_width: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(window.window_bounds(), WindowBounds::Windowed(_)) {
            return;
        }
        let wayland = layout::on_wayland();
        let visible = window.display(cx).map(|display| {
            let reported = f32::from(display.visible_bounds().size.height);
            if wayland {
                layout::wayland_logical_height(reported, window.scale_factor())
            } else {
                reported
            }
        });
        let height = layout::fit_height(self.rows.len(), visible);
        let viewport = window.viewport_size();
        let width = if reset_width {
            px(layout::metrics::WIDTH)
        } else {
            viewport.width
        };
        if (f32::from(viewport.height) - height).abs() >= 1. || width != viewport.width {
            log::debug!("fitting the window to {width:?} × {height}");
            window.resize(size(width, px(height)));
        }
    }
}
