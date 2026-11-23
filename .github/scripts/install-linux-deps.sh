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
# from directly; refreshing only Ubuntu's own sources (skipping third-party
# repos on CI images) runs only when they are not.
set -euo pipefail

packages=(libxkbcommon-x11-dev libfontconfig-dev "$@")
apt_opts=(-y --no-install-recommends -o Acquire::Languages=none)
if [ -n "${CI:-}" ]; then
  # Ephemeral CI runner: skip per-file dpkg fsyncs (saves ~10s unpacking
  # headers on virtual disks).
  apt_opts+=(-o 'DPkg::Options::=--force-unsafe-io')
fi

install() {
  sudo DEBIAN_FRONTEND=noninteractive NEEDRESTART_SUSPEND=1 \
    apt-get install "${apt_opts[@]}" "${packages[@]}"
}

update_ubuntu_lists() {
  if [ -f /etc/apt/sources.list.d/ubuntu.sources ]; then
    # Ubuntu 24.04+ (DEB822 format): skip third-party repos in sources.list.d/.
    sudo apt-get update \
      -o Dir::Etc::sourcelist=/etc/apt/sources.list.d/ubuntu.sources \
      -o Dir::Etc::sourceparts=- \
      -o Acquire::Languages=none
  else
    # Ubuntu 22.04: skip third-party repos in sources.list.d/.
    sudo apt-get update \
      -o Dir::Etc::sourcelist=/etc/apt/sources.list \
      -o Dir::Etc::sourceparts=- \
      -o Acquire::Languages=none
  fi
}

install || { update_ubuntu_lists && install; } || { sudo apt-get update && install; }
