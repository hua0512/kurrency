//! Kurrency: a small, beautiful desktop currency converter on gpui-kit.
//!
//! This is the GPUI application; the model it shows (expressions, money,
//! rates, settings) is the `kurrency-core` crate. `main` only bootstraps:
//! logging, the UI language, the HTTP client, key bindings and themes, and
//! the one window, whose view is [`app::KurrencyApp`].

// Release builds on Windows are GUI programs, with no console window. Debug
// builds keep the console for the log.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod assets;
mod i18n;
mod layout;
mod net;
mod rates;
mod ui;

use std::sync::Arc;

use gpui_kit::AppContext as _;

use crate::app::KurrencyApp;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("kurrency=info"))
        .init();
    i18n::init();

    gpui_kit::application()
        .with_assets(assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            cx.set_http_client(Arc::new(net::UreqClient::new()));
            app::init(cx);
            ui::theme::init(cx);

            let options = app::window_options(cx);
            gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| KurrencyApp::new(window, cx))
            })
            .expect("failed to open the Kurrency window");

            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            cx.activate(true);
        });
}
