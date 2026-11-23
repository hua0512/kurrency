# Vendored crates

Third-party crates that Kurrency builds with a local patch, through
`[patch.crates-io]` in the root `Cargo.toml`. They are excluded from the
workspace, so the workspace lints and `cargo clippy --workspace` skip them.
Drop each one once upstream ships the fix.

## gpui-pre-windows 0.3.14

GPUI's Windows platform: Zed's `gpui_windows` at
[zed@f126950](https://github.com/zed-industries/zed/tree/f126950f9d3b62666f97437ae5d4ccac2272c4db/crates/gpui_windows),
as published on crates.io. It is licensed under Apache-2.0 (`LICENSE-APACHE`).
The vendored version must be the one that gpui-pre pins. When gpui-kit is
bumped, either drop this crate or re-vendor the new version and re-apply the
patch.

**Patch: draw the new size during a live resize.** While the user drags a
window border, Windows runs `DefWindowProc`'s modal size loop. The
`WM_SIZE` handler resized the swap chain and marked the window dirty, but the
frame was only drawn on the loop's timer (`USER_TIMER_MINIMUM`, about 10–16 ms
later) or on a deferred `WM_PAINT`. Meanwhile DWM showed the resized window
with stale content and a black strip along the dragged edge, so the content
lagged behind the border. Now `WM_ENTERSIZEMOVE` and `WM_EXITSIZEMOVE` keep an
`in_size_move` flag, and inside the loop `WM_SIZE` draws and presents the new
size before returning. Programmatic resizes (`SetWindowPos`, maximise, snap)
happen outside the loop and keep the old path. The change is in
`src/events.rs` (`handle_size_msg`, `handle_size_move_loop`,
`handle_size_move_loop_exit`) and `src/window.rs` (the `in_size_move` field).
No release includes this fix yet; 0.3.14, the latest when it was vendored
(2026-11-23), does not.

Besides the patch, only the published package's `Cargo.lock`,
`Cargo.toml.orig` and `.cargo-ok` were left out. To see the whole difference:

```sh
cargo fetch --target x86_64-pc-windows-msvc
diff -ru ~/.cargo/registry/src/index.crates.io-*/gpui-pre-windows-0.3.14/src vendor/gpui-pre-windows/src
```

To type-check it from Linux or macOS, check only this crate, since other
dependencies need the MSVC toolchain to build their C code:

```sh
rustup target add x86_64-pc-windows-msvc
cargo check --target x86_64-pc-windows-msvc -p gpui-pre-windows
```
