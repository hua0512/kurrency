# Developing Kurrency

## Build and run

Install [rustup](https://rustup.rs). `rust-toolchain.toml` pins the toolchain
(Rust 1.99.1, with rustfmt and clippy), and rustup installs it on the first
build.

On Linux, also install the fontconfig and xkbcommon development files:

```sh
sudo apt install libfontconfig-dev libxkbcommon-x11-dev    # Debian, Ubuntu
sudo dnf install fontconfig-devel libxkbcommon-x11-devel   # Fedora
sudo pacman -S fontconfig libxkbcommon-x11                 # Arch
```

That is the whole list for this dependency tree: TLS is rustls (no OpenSSL),
and Wayland and Vulkan are loaded at runtime, so running needs a Vulkan driver
(Mesa's on most systems) but building does not. macOS needs the Xcode command
line tools (`xcode-select --install`); GPUI's shaders are compiled at runtime,
so full Xcode is not required. Windows needs the MSVC build tools.

```sh
cargo run                  # debug build
cargo run --release        # size-optimised, thin LTO: about a minute longer
cargo build --profile dist # fat LTO, what the packages ship: about 3 minutes
```

(The workspace has one binary, so `cargo run` needs no `-p kurrency`.)

The profiles are in the root `Cargo.toml`, with their measurements:
`release` is `opt-level = "s"` with thin LTO, one codegen unit, `strip` and
`panic = "abort"`; `dist` is `release` with fat LTO (binaries in
`target/dist/`), used only by the release workflow; `ci` is CI's test build.

Fonts, flags and icons are compiled into the binary, so the executable runs on
its own. In debug builds, `KURRENCY_CONFIG_DIR` and `KURRENCY_CACHE_DIR` point
the settings and rate cache at throwaway directories.

HTTP goes through `crates/kurrency/src/net.rs`, GPUI's `HttpClient` over
[ureq](https://github.com/algesten/ureq) with rustls: each request on a thread
of its own, 10 s to connect and 30 s in all, gzip, redirects, the system's
certificates (the bundled webpki roots when there are none), and proxies from
`HTTPS_PROXY`, `ALL_PROXY` or `HTTP_PROXY`, minus `NO_PROXY`. It replaced
gpui-pre's reqwest client, which brought tokio, hyper and HTTP/2 along
(0.9 MB of the release binary).

## Layout

The product and visual design (layout, input, motion, themes, keyboard) is
described in [DESIGN.md](DESIGN.md); code comments cite its sections.

A Cargo workspace (the root `Cargo.toml` holds the shared package fields,
every dependency's version, the lints and the profiles) with two crates:

```
crates/
  kurrency-core/      library, no GPUI: expressions, money, presentation,
    src/              palette commands, smart paste, rates and their caches,
                      settings, undo, i18n (compiles locales/)
      generated/      tables written by scripts/ (do not edit)
      rates/          providers' formats, caches, history, previous day
  kurrency/           the GPUI app (package and binary `kurrency`)
    build.rs          compresses assets/flags/ and the fonts; the Windows
                      icon and version
    src/
      main.rs         bootstrap only
      app.rs, app/    the root view: state in app.rs, behaviour by concern
                      (actions, editing, rows, commands, changes, pages,
                      prefs, window, render), debug-build hooks
                      (debug_env, demo), UI flow tests in app/tests/
      ui.rs, ui/      the view components (header, card, rows, pages, ...)
      rates.rs, rates/  the rates store entity and HTTP fetching
      net.rs          the HTTP client (ureq, rustls, one thread a request)
      assets.rs       fonts, flags and icons compiled into the binary
      layout.rs       layout metrics and window geometry
assets/               fonts, flags, themes, icon, rate snapshot, screenshots
locales/              UI strings (en, zh-CN), for both crates
packaging/            icons, desktop entry, metainfo, Windows resources
scripts/              generators (icons, CLDR names, font subsets), tarball,
                      README screenshots and demo (record-demo.sh, demo-fixture/)
```

Only `crates/kurrency` may depend on gpui-kit; CI checks that
`kurrency-core` does not. Modules use the `foo.rs` + `foo/` layout, never
`mod.rs`. Items are `pub(crate)` unless another crate needs them, and
`kurrency-core` makes `pub` only what the app uses. Imports come in three
groups: `std`, other crates, then `crate`/`super`.

## Test

```sh
cargo nextest run --workspace      # or: cargo test --workspace
cargo nextest run -p kurrency-core # the core alone, without building GPUI
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

The UI tests (`#[gpui_kit::test]`) run on GPUI's test platform, which needs no
display, window server or GPU, so they also run headless and in CI on every
OS. They are unit tests in `crates/kurrency/src/app/tests/` rather than
integration tests, because they read the view's private state.

## Continuous integration

`.github/workflows/ci.yml` runs on pushes to `main` and on pull requests,
except for changes that only touch Markdown, `LICENSE` or
`assets/screenshots/`.

1. **Lint** (Ubuntu): `cargo fmt --check`, a check that `kurrency-core`'s
   dependency graph has no GPUI crate (`cargo tree`, nothing compiled), then
   `cargo clippy --workspace --all-targets -- -D warnings`.
2. **Test** (Ubuntu, macOS arm64, Windows), after Lint passes:
   `cargo nextest run --workspace --cargo-profile ci --profile ci`, then the
   core's doctests. `[profile.ci]` in the root
   `Cargo.toml` drops debug info from dependencies. On Linux the
   [mold](https://github.com/rui314/mold) linker is set up by the workflow, not
   by the repository, so local builds keep their own linker.
3. **Clippy (latest stable)**: non-blocking. It shows new lints early, and warns
   when `rust-toolchain.toml` is behind the newest stable.

Caching uses [rust-cache](https://github.com/Swatinem/rust-cache). Only `main`
saves caches; pull requests restore `main`'s, so they never push `main`'s
entries out of GitHub's 10 GB quota. A new push to a pull request cancels its
running checks. sccache is not used: rust-cache already restores every
dependency in one archive, while sccache would fetch about 570 objects
one by one from the cache service and still relink.

Third-party actions are pinned to commit SHAs, and Dependabot proposes updates
for them and for crates weekly. gpui-kit and the `gpui-pre` crates are pinned
exactly (`=`), so Dependabot leaves them alone: bump them together by hand
when gpui-kit releases.

## Packaging and releases

See [RELEASING.md](RELEASING.md): how to cut a release, the signing secrets,
building the packages locally (with `--profile dist`), bumping the Rust
toolchain, and the build profiles.

## Screenshots and the demo

The README's screenshots and animated demo (`assets/screenshots/`) are
recorded from the real app by `scripts/record-demo.sh`:

```sh
scripts/record-demo.sh              # every screenshot, then demo.webp
scripts/record-demo.sh stills       # the screenshots only
scripts/record-demo.sh demo         # demo.webp only
scripts/record-demo.sh history zh-cn  # some screenshots by name
```

It needs Linux with the [niri](https://github.com/YaLTeR/niri) compositor,
`grim`, `wf-recorder`, `ffmpeg` (with libwebp), `bwrap` and Python's Pillow
(and uses `oxipng` when it is installed). It starts a nested niri whose
output has scale 2, so captures are 2x whatever your display's scale; its
window is pushed almost off-screen, and neither your pointer nor your
keyboard reaches the app. Leave the session alone for the minute or so
it takes.

Every capture is the same whenever it is taken: the app reads a copy of
`scripts/demo-fixture/` (rates saved on 6 November 2026, the day before's
for the change chips, and a year of USD/EUR and USD/GBP history) and
throwaway settings, its clock is set to 6 November 2026 18:30 UTC, and it
has no network. These use debug-build hooks (`KURRENCY_CACHE_DIR`,
`KURRENCY_CONFIG_DIR`, `KURRENCY_NOW`, and the ones in
`crates/kurrency/src/app/debug_env.rs`), so the script builds a debug
build with optimised dependencies (`--profile demo`, in `target/demo/`).

The demo is a scripted tour, `KURRENCY_DEMO=1`
(`crates/kurrency/src/app/demo.rs`). It drives the app's real input paths
(keystrokes, a synthetic pointer, smart paste's parser), so every
animation plays, and it ends exactly as it began; the script trims the
recording so the first and last frames match and the loop has no seam.
Like the other hooks it is compiled into debug builds only. To change the
tour, edit `TOUR`; its test (`app/tests/demo.rs`) checks that each part
still does what it shows and that it ends where it began.

## Fonts and flags

The bundled fonts in `assets/fonts/` are subsets, made by two scripts and
committed, so builds need no font tools. Each script downloads its pinned
source (checked against a SHA-256 sum) into a temporary directory and runs
fontTools' `pyftsubset` (from an installed fontTools, else through
[uv](https://docs.astral.sh/uv/)'s `uvx`); the same inputs give the same bytes.

```sh
python3 -I scripts/subset-inter-font.py   # Inter 4.1: Latin, symbols, locales
python3 -I scripts/subset-cjk-font.py     # Noto Sans SC: the Chinese UI's characters
```

Inter keeps Latin-1 and Latin Extended-A, punctuation, currency symbols,
arrows, math operators, the macOS key symbols, geometric shapes, the
currency-symbol characters it has (Cyrillic лв, ден, дин, Thai ฿) and every
character of `locales/*.yml`, with its hinting (Linux and Windows hint) and
all layout features. Greek and Cyrillic words would fall back to a system
font. Re-run the scripts after editing a locale; the font tests in
`crates/kurrency/src/assets.rs` and `crates/kurrency/src/i18n.rs` fail when
the UI needs a character a subset lacks, or when a currency symbol would
fall back to its ISO code needlessly.

`build.rs` stores the flags gzipped (SVGZ, which GPUI's `img()` renders as
is through resvg) and the fonts deflated (inflated once at startup with
flate2). `assets/flags/` stays plain SVG, so diffs remain readable.

## Icons

`packaging/icons/` (hicolor PNGs, `.ico`, `.icns`) is generated from
`assets/icon.svg` by `scripts/gen-icons.sh` and committed, so builds
need no image tools. Run the script again after changing the SVG; it needs
`rsvg-convert` and Python's Pillow.
