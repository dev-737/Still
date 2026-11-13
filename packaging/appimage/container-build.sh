#!/usr/bin/env bash
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive

if [ -n "${APT_MIRROR:-}" ]; then
  sed -i "s#http://archive.ubuntu.com#http://$APT_MIRROR#; s#http://security.ubuntu.com#http://$APT_MIRROR#" /etc/apt/sources.list.d/ubuntu.sources
fi
rm -f /etc/apt/apt.conf.d/docker-clean
apt-get update -qq
apt-get install -y -qq --no-install-recommends \
  build-essential curl ca-certificates pkg-config file patchelf desktop-file-utils \
  libgtk-4-dev libadwaita-1-dev libcairo2-dev libpango1.0-dev libgraphene-1.0-dev \
  libglib2.0-bin librsvg2-common shared-mime-info adwaita-icon-theme hicolor-icon-theme \
  grim slurp wl-clipboard libnotify-bin

if ! command -v cargo >/dev/null; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
fi
. "$HOME/.cargo/env"

tools=/opt/appimage-tools
mkdir -p "$tools"
curl -fsSL -o "$tools/linuxdeploy-x86_64.AppImage" \
  https://github.com/linuxdeploy/linuxdeploy/releases/download/1-alpha-20251107-1/linuxdeploy-x86_64.AppImage
curl -fsSL -o "$tools/linuxdeploy-plugin-gtk.sh" \
  https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gtk/master/linuxdeploy-plugin-gtk.sh
chmod +x "$tools"/*
export PATH="$tools:$PATH"
export APPIMAGE_EXTRACT_AND_RUN=1 DEPLOY_GTK_VERSION=4 NO_STRIP=1

export CARGO_TARGET_DIR=/tmp/still-target
cargo build --release --locked

rm -rf AppDir
linuxdeploy-x86_64.AppImage --appdir AppDir \
  --executable /tmp/still-target/release/still \
  --executable /usr/bin/grim \
  --executable /usr/bin/slurp \
  --executable /usr/bin/wl-copy \
  --executable /usr/bin/notify-send \
  --desktop-file packaging/still.desktop \
  --icon-file packaging/icons/hicolor/scalable/apps/still.svg \
  --custom-apprun packaging/appimage/AppRun \
  --plugin gtk

sed -i '/GDK_BACKEND/d' AppDir/apprun-hooks/*.sh
chown -R "$HOST_UID:$HOST_GID" AppDir 2>/dev/null || true
