//! Kurrency's model, without the UI: everything the converter knows and
//! decides that does not need a window, so it builds and tests without GPUI
//! (the crate graph enforces it: this crate does not depend on gpui-kit).
//!
//! - [`expr`]: the arithmetic typed into an amount (`1.5k*3`);
//! - [`money`]: currency metadata, conversion, rounding and number formats;
//! - [`present`]: how amounts, daily changes and rates read on screen, and
//!   what a keystroke means outside a text input;
//! - [`command`] and [`paste`]: the palette's commands and search, and prices
//!   read from the clipboard;
//! - [`rates`]: the rate tables, their providers' formats, the caches, the
//!   previous day and the history maths;
//! - [`settings`]: `settings.json`, its defaults and migrations;
//! - [`undo`]: undo and redo of the list and amount;
//! - [`i18n`]: the UI languages and [`tr!`], over the strings in `locales/`;
//! - [`paths`]: where the settings and caches live.
//!
//! The GPUI application is the `kurrency` crate.

pub mod command;
pub mod expr;
mod generated;
pub mod i18n;
pub mod money;
pub mod paste;
pub(crate) mod paths;
pub mod present;
pub mod rates;
pub mod settings;
pub mod undo;

// The UI strings (`locales/*.yml` at the repository root), compiled in; see
// `i18n`. The app's strings live here too, so that one table, one language
// switch and one pair of locale files serve both crates.
rust_i18n::i18n!("../../locales", fallback = "en");
