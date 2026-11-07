# Releasing Kurrency

Releases are built by `.github/workflows/release.yml` when a `v*` tag is pushed.
It builds every package on GitHub's runners, writes `SHA256SUMS`, and attaches
them to a **draft** GitHub Release, which you check and publish by hand.

## Cut a release

1. Update `version` under `[workspace.package]` in the root `Cargo.toml` (both
   crates inherit it), run `cargo check` so `Cargo.lock` follows,
   and add a `<release>` entry to `packaging/linux/dev.kurrency.Kurrency.metainfo.xml`.
2. Commit, and wait for CI on `main` to pass.
3. Tag and push:

   ```sh
   git tag -a v0.2.0 -m "Kurrency 0.2.0"
   git push origin v0.2.0
   ```

4. When the workflow finishes (about 15 to 25 minutes; the macOS job builds two
   architectures), open the draft on the Releases page, edit the generated
   notes, check the artifacts, and publish.

To try the packaging without a release, run the workflow from the Actions tab
(**Release**, then **Run workflow**). It builds every package as a workflow
artifact and skips the release job.

## What gets built

| Job | Runner | Packages |
|---|---|---|
| Linux x86_64 | `ubuntu-22.04` | `kurrency_<v>_amd64.deb`, `kurrency_<v>_x86_64.AppImage`, `kurrency-<v>-x86_64-linux.tar.gz` |
| Linux aarch64 | `ubuntu-22.04-arm` | the same for `arm64` / `aarch64` |
| macOS | `macos-latest` | `Kurrency_<v>_universal.dmg` (aarch64 and x86_64 joined with `lipo`) |
| Windows | `windows-latest` | `kurrency_<v>_x64-setup.exe` (NSIS, per-user), `kurrency-<v>-x86_64-windows.zip` (portable) |

Building on Ubuntu 22.04 keeps the Linux packages working on glibc 2.35 and
newer. GitHub's arm64 Linux runners are free for public repositories only; for a
private repository, drop the `aarch64` matrix entry or use a paid runner.

Packages are made with [cargo-packager](https://github.com/crabnebula-dev/cargo-packager)
(configuration: `[package.metadata.packager]` in `crates/kurrency/Cargo.toml`,
with paths relative to that file), plus
`scripts/package-linux-tarball.sh` for the tarball and a `7z` step for the
Windows zip. cargo-packager has no release binaries of its own, so the
workflow installs it with `taiki-e/install-action`, which falls back to
cargo-binstall.

## Secrets

All signing is optional. Without these secrets the workflow still succeeds and
the packages are unsigned. Add them under **Settings → Secrets and variables →
Actions**.

### macOS (signing and notarisation)

| Secret | Value |
|---|---|
| `APPLE_SIGNING_IDENTITY` | The certificate's name, e.g. `Developer ID Application: Jane Doe (TEAMID1234)`. Signing runs only when this is set. |
| `APPLE_CERTIFICATE` | The Developer ID Application certificate and key, exported from Keychain Access as `.p12`, then base64-encoded: `base64 -i cert.p12 \| pbcopy` |
| `APPLE_CERTIFICATE_PASSWORD` | The `.p12` export password |
| `APPLE_ID` | The Apple ID email used for notarisation |
| `APPLE_PASSWORD` | An [app-specific password](https://support.apple.com/102654) for that Apple ID |
| `APPLE_TEAM_ID` | The 10-character team ID |

With the first three, cargo-packager imports the certificate into a temporary
keychain and signs the app (hardened runtime) and the DMG. With all six, it also
notarises and staples the app. Without them, users must clear the quarantine
flag as described in the README.

### Windows (Authenticode)

| Secret | Value |
|---|---|
| `WINDOWS_CERTIFICATE` | A code-signing certificate as `.pfx`, base64-encoded: `base64 -w0 cert.pfx` |
| `WINDOWS_CERTIFICATE_PASSWORD` | The `.pfx` password |

When set, `.github/scripts/sign-windows.sh` signs `kurrency.exe` before it is
packaged, then the installer, with SHA-256 and an RFC 3161 timestamp
(DigiCert's server; override with a `WINDOWS_TIMESTAMP_URL` variable). Keys held
in a cloud HSM (Azure Trusted Signing, SignPath, and so on) need that service's
action in place of this script.

## Build the packages locally

Install cargo-packager 0.11.8 (`cargo binstall cargo-packager@0.11.8`, or
`cargo install cargo-packager --version 0.11.8 --locked`), then on Linux:

```sh
cargo build --profile dist --locked
mkdir -p dist
cargo packager --profile dist --formats deb,appimage --out-dir dist
scripts/package-linux-tarball.sh target/dist/kurrency x86_64 dist
```

`--profile dist` is what the release workflow ships (fat LTO, see below);
cargo-packager then reads the binary from `target/dist/` (with `--target`,
`target/<triple>/dist/`). `--release` works too, for a quicker package from
`target/release/`.

Create `dist` first: cargo-packager runs from `crates/kurrency/`, and only an
existing `--out-dir` is resolved from where you started it. The AppImage step
downloads linuxdeploy on first use; on a rolling distribution such as Arch, set
`NO_STRIP=1`, since linuxdeploy's bundled `strip` cannot read newer
libraries. macOS (`--formats app,dmg`)
and Windows (`--formats nsis`) packages have to be built on those systems.

## Bump the Rust toolchain

`rust-toolchain.toml` pins one toolchain for local builds, CI and releases,
and `rust-version` in the root `Cargo.toml` (the MSRV) is the same version, so there is
no separate MSRV job. Dependabot does not update either. Rust releases every
six weeks; CI's non-blocking **Clippy (latest stable)** job shows a warning on
`rust-toolchain.toml` once a newer stable is out. Then:

1. Set `channel` in `rust-toolchain.toml` to the new version (e.g. `"1.100.0"`)
   and `rust-version` in the root `Cargo.toml` to its minor (`"1.100"`).
2. Run `cargo update`, then `cargo fmt --check`,
   `cargo clippy --workspace --all-targets -- -D warnings` and
   `cargo nextest run --workspace`, and fix
   any new lints.
3. Commit the three files together. The first CI run after the bump is a cold
   build, since every cache key includes the compiler version.

## Build profiles

Three profiles in the root `Cargo.toml`, whose comments have the full
measurements:

- `[profile.release]` (`cargo build --release`, for local use): optimised
  for size (`opt-level = "s"`), thin LTO, one codegen unit, `strip` and
  `panic = "abort"`. On x86_64 Linux: 28.6 MB, 11.3 MB gzipped.
- `[profile.dist]` (`cargo build --profile dist`, for the published
  packages): the release profile with fat LTO, 25.2 MB (11.1 MB gzipped),
  3.5 MB smaller, but its final link takes about 3 minutes instead of 45 s
  and peaks at about 3 GB of memory. Binaries land in
  `target/[<triple>/]dist/`. The x86_64 Linux packages are about 11 MB each
  (`.deb` 11.2, AppImage 11.5, `.tar.gz` 11.2), down from 15.5 MB before the
  size work.
- `[profile.ci]`: CI's tests, which drops debug info from dependencies so the
  test build and its cache stay small.

## Before the first release

- If branch protection requires the CI checks, note that CI skips commits that
  only touch Markdown, `LICENSE`, `assets/screenshots/`;
  mark the checks as not required for those, or drop `paths-ignore`.
