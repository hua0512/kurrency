#!/usr/bin/env bash
# Packs a built Linux binary into a relocatable tarball with a freedesktop layout:
#
#   kurrency-<version>-<arch>-linux/
#     bin/kurrency
#     share/applications/dev.kurrency.Kurrency.desktop
#     share/metainfo/dev.kurrency.Kurrency.metainfo.xml
#     share/icons/hicolor/<size>/apps/kurrency.{png,svg}
#     LICENSE  NOTICE  README.md  licenses/
#
# so `tar -xzf … --strip-components=1 -C ~/.local` installs it for one user.
# Fonts, flags and icons are compiled into the binary; nothing else is needed.
#
# usage: scripts/package-linux-tarball.sh <binary> <arch> <out-dir>
set -euo pipefail

binary=$1 arch=$2 out=$3
root=$(cd "$(dirname "$0")/.." && pwd)
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -n 1)
name="kurrency-$version-$arch-linux"
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
dir="$stage/$name"

install -Dm755 "$binary" "$dir/bin/kurrency"
install -Dm644 "$root/packaging/linux/dev.kurrency.Kurrency.desktop" \
  "$dir/share/applications/dev.kurrency.Kurrency.desktop"
install -Dm644 "$root/packaging/linux/dev.kurrency.Kurrency.metainfo.xml" \
  "$dir/share/metainfo/dev.kurrency.Kurrency.metainfo.xml"
mkdir -p "$dir/share/icons"
cp -R "$root/packaging/icons/hicolor" "$dir/share/icons/"
install -m644 "$root/LICENSE" "$root/NOTICE" "$root/README.md" "$dir/"
install -Dm644 "$root/assets/fonts/OFL.txt" "$dir/licenses/Inter-OFL.txt"
install -Dm644 "$root/assets/fonts/NotoSansSC-OFL.txt" "$dir/licenses/NotoSansSC-OFL.txt"
install -Dm644 "$root/assets/flags/LICENSE" "$dir/licenses/flag-icons-MIT.txt"

mkdir -p "$out"
# Reproducible-ish: fixed owner and sorted entries.
tar --sort=name --owner=0 --group=0 --numeric-owner -C "$stage" -czf "$out/$name.tar.gz" "$name"
echo "$out/$name.tar.gz"
