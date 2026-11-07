#!/usr/bin/env bash
# Regenerates the platform icons in packaging/icons/ from assets/icon.svg.
#
# The outputs are committed, so builds and CI never need these tools; run this
# only after changing the SVG. Needs `rsvg-convert` (librsvg2-bin on Debian and
# Ubuntu, librsvg on Homebrew and Arch) and Python 3 with Pillow
# (`python3 -m pip install pillow`), all available on GitHub's runners.
#
#   packaging/icons/hicolor/<N>x<N>/apps/kurrency.png   Linux (freedesktop hicolor theme)
#   packaging/icons/hicolor/scalable/apps/kurrency.svg  Linux, scalable
#   packaging/icons/kurrency.ico                        Windows: executable and installer
#   packaging/icons/kurrency.icns                       macOS: the .app bundle
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
svg="$root/assets/icon.svg"
out="$root/packaging/icons"

for tool in rsvg-convert python3; do
  command -v "$tool" > /dev/null || { echo "gen-icons: $tool is required" >&2; exit 1; }
done
python3 -c 'import PIL' 2> /dev/null || {
  echo "gen-icons: Python's Pillow is required (python3 -m pip install pillow)" >&2
  exit 1
}

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# Render each size straight from the vector, rather than downscaling one large
# raster, so the small sizes stay crisp.
for size in 16 24 32 48 64 128 256 512 1024; do
  rsvg-convert --width "$size" --height "$size" --format png "$svg" --output "$tmp/$size.png"
done

rm -rf "$out/hicolor"
for size in 16 24 32 48 64 128 256 512; do
  dir="$out/hicolor/${size}x${size}/apps"
  mkdir -p "$dir"
  cp "$tmp/$size.png" "$dir/kurrency.png"
done
mkdir -p "$out/hicolor/scalable/apps"
cp "$svg" "$out/hicolor/scalable/apps/kurrency.svg"

# Pillow writes both containers. Each entry is given its own pre-rendered
# bitmap; Pillow only resamples when a size is missing.
python3 -I - "$tmp" "$out" << 'PY'
import sys
from pathlib import Path
from PIL import Image

src, out = Path(sys.argv[1]), Path(sys.argv[2])
img = lambda n: Image.open(src / f"{n}.png").convert("RGBA")

# Windows: 16-48 for lists and the taskbar, 256 for large views (PNG-compressed).
ico_sizes = [16, 24, 32, 48, 64, 128, 256]
img(256).save(
    out / "kurrency.ico",
    format="ICO",
    sizes=[(n, n) for n in ico_sizes],
    append_images=[img(n) for n in ico_sizes],
)

# macOS: 16-512 at 1x and 2x (1024 is 512@2x).
icns_sizes = [16, 32, 64, 128, 256, 512, 1024]
img(1024).save(
    out / "kurrency.icns",
    format="ICNS",
    append_images=[img(n) for n in icns_sizes],
)
PY

echo "gen-icons: wrote $(find "$out" -type f | wc -l) files under ${out#"$root"/}"
