#!/usr/bin/env bash
set -euo pipefail

version=${1:?Pass the release version}
output=${2:?Pass the output directory}
cd "$(dirname "$0")/../.."

docker run --rm \
  -e APT_MIRROR="${APT_MIRROR:-}" \
  -e HOST_UID="$(id -u)" -e HOST_GID="$(id -g)" \
  -v "$PWD:/src" -w /src \
  -v still-appimage-apt:/var/cache/apt -v still-appimage-cargo:/root/.cargo -v still-appimage-rustup:/root/.rustup \
  ubuntu:24.04 bash packaging/appimage/container-build.sh

mkdir -p "$output"
ARCH=x86_64 appimagetool AppDir "$output/Still-$version-x86_64.AppImage"
rm -rf AppDir
