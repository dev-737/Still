#!/usr/bin/env bash
set -euo pipefail

binary=${1:?Pass the Still binary}
version=${2:?Pass the release version}
output=${3:?Pass the output directory}

if [[ ! $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  printf 'Expected a version such as 0.1.0.\n' >&2
  exit 1
fi

case "$(uname -m)" in
  x86_64) arch=x86_64; deb_arch=amd64 ;;
  aarch64) arch=aarch64; deb_arch=arm64 ;;
  *) printf 'Unsupported architecture.\n' >&2; exit 1 ;;
esac

mkdir -p "$output"
output=$(realpath "$output")
root=$(mktemp -d)
trap 'rm -rf -- "$root"' EXIT
name="still-$version-linux-$arch"
stage="$root/$name"
install -Dm755 "$binary" "$stage/bin/still"
install -Dm644 packaging/still.desktop "$stage/share/applications/still.desktop"
install -Dm644 packaging/icons/hicolor/scalable/apps/still.svg "$stage/share/icons/hicolor/scalable/apps/still.svg"
install -Dm644 packaging/icons/hicolor/symbolic/apps/still-symbolic.svg "$stage/share/icons/hicolor/symbolic/apps/still-symbolic.svg"
install -Dm644 packaging/Linux.md "$stage/share/doc/still/Linux.md"
tar -C "$root" -czf "$output/$name.tar.gz" "$name"

deb="$root/deb"
mkdir -p "$deb/usr" "$deb/DEBIAN"
cp -a "$stage/." "$deb/usr/"
cat > "$deb/DEBIAN/control" <<CONTROL
Package: still
Version: $version
Section: graphics
Priority: optional
Architecture: $deb_arch
Maintainer: Still contributors <73829355+dev-737@users.noreply.github.com>
Depends: libc6 (>= 2.39), libgtk-4-1 (>= 4.12), libadwaita-1-0 (>= 1.4), libcairo2, libpango-1.0-0, libpangocairo-1.0-0, libgdk-pixbuf-2.0-0, libglib2.0-0t64, grim, slurp, wl-clipboard, libnotify-bin
Description: Screenshot editor for Wayland
 Capture regions, annotate images, and import ShareX framing presets.
CONTROL
dpkg-deb --root-owner-group --build "$deb" "$output/still_${version}_${deb_arch}.deb"

mkdir -p "$root/rpm"
cat > "$root/still.spec" <<SPEC
Name: still
Version: $version
Release: 1
Summary: Screenshot editor for Wayland
License: LicenseRef-Still
URL: https://github.com/dev-737/Still
Requires: gtk4 >= 4.12, libadwaita >= 1.4, grim, slurp, wl-clipboard, libnotify

%description
Capture regions, annotate images, and import ShareX framing presets.

%install
mkdir -p "%{buildroot}%{_prefix}"
cp -a "$stage/." "%{buildroot}%{_prefix}/"

%files
%{_bindir}/still
%{_datadir}/applications/still.desktop
%{_datadir}/icons/hicolor/scalable/apps/still.svg
%{_datadir}/icons/hicolor/symbolic/apps/still-symbolic.svg
%{_datadir}/doc/still/Linux.md
SPEC
rpmbuild -bb --define "_topdir $root/rpm" --define "debug_package %{nil}" "$root/still.spec"
install -m644 "$root/rpm/RPMS/$arch/still-$version-1.$arch.rpm" "$output/"
