//! Rebuilds when a locale file changes: `rust_i18n::i18n!` reads
//! `locales/*.yml` at compile time, but cargo does not watch them otherwise.

fn main() {
    println!("cargo:rerun-if-changed=../../locales");
    println!("cargo:rerun-if-changed=build.rs");
}
