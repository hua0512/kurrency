#!/usr/bin/env bash
# Installs the system libraries Kurrency links against on Ubuntu (22.04 and 24.04).
#
# This is the minimal list for our dependency tree, not gpui-kit's full CI list:
#   libxkbcommon-x11-dev  `xkbcommon` links libxkbcommon and libxkbcommon-x11, and
#                         `x11rb` links libxcb (pulled in as libxcb1-dev).
#   libfontconfig-dev     `yeslogic-fontconfig-sys` probes fontconfig with
#                         pkg-config; it also pulls in libfreetype-dev, which
#                         `freetype-sys` then uses instead of building its copy.
# Not needed: Wayland and Vulkan are loaded with dlopen at runtime, TLS is rustls
# (no OpenSSL), there is no zstd or WebKit in the tree, and gcc, clang,
# pkg-config and cmake are already on GitHub's images.
#
# The package lists on the runner image are usually fresh enough to install
# from directly; `apt-get update` (10-30 s) runs only when they are not.
set -euo pipefail

packages=(libxkbcommon-x11-dev libfontconfig-dev "$@")
install() {
  sudo DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends "${packages[@]}"
}
install || { sudo apt-get update && install; }
