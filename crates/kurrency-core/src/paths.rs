//! Where Kurrency keeps its files: `{config_dir}/kurrency` for the settings
//! and `{cache_dir}/kurrency` for rates and history.
//!
//! Debug builds read `KURRENCY_CONFIG_DIR` and `KURRENCY_CACHE_DIR` first, so
//! screenshots and manual tests run against a throwaway directory and never
//! touch the real files. Release builds ignore them.

use std::path::PathBuf;

/// The directory holding `settings.json`.
pub(crate) fn config_dir() -> Option<PathBuf> {
    override_dir("KURRENCY_CONFIG_DIR")
        .or_else(|| dirs::config_dir().map(|dir| dir.join("kurrency")))
}

/// The directory holding the rate cache and the history cache.
pub(crate) fn cache_dir() -> Option<PathBuf> {
    override_dir("KURRENCY_CACHE_DIR").or_else(|| dirs::cache_dir().map(|dir| dir.join("kurrency")))
}

fn override_dir(var: &str) -> Option<PathBuf> {
    if !cfg!(debug_assertions) {
        return None;
    }
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}
