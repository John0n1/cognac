#!/usr/bin/env bash
set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"

VERSION=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)
ARCH=$(uname -m)
if [[ "$ARCH" != "x86_64" ]]; then
  echo "release packaging currently supports x86_64 hosts only" >&2
  exit 1
fi

for command in cargo dpkg-deb makepkg sha256sum tar; do
  command -v "$command" >/dev/null || {
    echo "missing packaging command: $command" >&2
    exit 1
  }
done

DIST="$ROOT/dist"
mkdir -p "$DIST"
WORK=$(mktemp -d "${TMPDIR:-/tmp}/cognac-package.XXXXXXXX")
trap 'rm -rf -- "$WORK"' EXIT

cargo build --release --locked
BINARY="$ROOT/target/release/cognac"

# Generic portable binary archive.
PORTABLE="cognac-${VERSION}-x86_64"
mkdir -p "$WORK/$PORTABLE/bin"
install -m755 "$BINARY" "$WORK/$PORTABLE/bin/cognac"
install -m644 LICENSE README.md CHANGELOG.md "$WORK/$PORTABLE/"
tar --sort=name --owner=0 --group=0 --numeric-owner \
  --mtime="UTC 2026-08-26" -C "$WORK" -czf "$DIST/$PORTABLE.tar.gz" "$PORTABLE"

# Debian-family package.
DEBROOT="$WORK/deb"
mkdir -p "$DEBROOT/DEBIAN" "$DEBROOT/usr/bin" \
  "$DEBROOT/usr/share/doc/cognac" "$DEBROOT/usr/share/licenses/cognac"
install -m644 packaging/debian/control "$DEBROOT/DEBIAN/control"
install -m755 "$BINARY" "$DEBROOT/usr/bin/cognac"
install -m644 README.md CHANGELOG.md "$DEBROOT/usr/share/doc/cognac/"
install -m644 LICENSE "$DEBROOT/usr/share/licenses/cognac/LICENSE"
dpkg-deb --root-owner-group --build "$DEBROOT" "$DIST/cognac_${VERSION}_amd64.deb"

# RPM package (cargo-generate-rpm creates RPM files without rpmbuild).
if ! cargo generate-rpm --version >/dev/null 2>&1; then
  echo "cargo-generate-rpm is required: cargo install cargo-generate-rpm" >&2
  exit 1
fi
cargo generate-rpm
install -m644 "$ROOT/target/generate-rpm/cognac-${VERSION}-1.x86_64.rpm" \
  "$DIST/cognac-${VERSION}-1.x86_64.rpm"

# Portable AppImage.
APPDIR="$WORK/Cognac.AppDir"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/applications" \
  "$APPDIR/usr/share/icons/hicolor/scalable/apps" "$APPDIR/usr/share/metainfo"
install -m755 "$BINARY" "$APPDIR/usr/bin/cognac"
install -m755 packaging/appimage/AppRun "$APPDIR/AppRun"
install -m644 packaging/cognac.desktop "$APPDIR/io.github.john0n1.cognac.desktop"
install -m644 packaging/cognac.desktop \
  "$APPDIR/usr/share/applications/io.github.john0n1.cognac.desktop"
install -m644 packaging/cognac.svg "$APPDIR/cognac.svg"
install -m644 packaging/cognac.svg \
  "$APPDIR/usr/share/icons/hicolor/scalable/apps/cognac.svg"
install -m644 packaging/cognac.metainfo.xml \
  "$APPDIR/usr/share/metainfo/io.github.john0n1.cognac.appdata.xml"

APPIMAGETOOL="$ROOT/target/package-tools/appimagetool-x86_64.AppImage"
if [[ ! -x "$APPIMAGETOOL" ]]; then
  mkdir -p "$(dirname -- "$APPIMAGETOOL")"
  curl --fail --location --retry 3 \
    --output "$APPIMAGETOOL" \
    https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-x86_64.AppImage
  chmod +x "$APPIMAGETOOL"
fi
ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$APPIMAGETOOL" \
  "$APPDIR" "$DIST/Cognac-${VERSION}-x86_64.AppImage"

# Arch binary package, using the same recipe published for AUR users but a
# local copy of the release tarball so it can be verified before publishing.
ARCHWORK="$WORK/arch"
mkdir -p "$ARCHWORK"
install -m644 packaging/arch/PKGBUILD "$ARCHWORK/PKGBUILD"
install -m644 "$DIST/$PORTABLE.tar.gz" "$ARCHWORK/$PORTABLE.tar.gz"
sed -i "s|^source=.*|source=(\"$PORTABLE.tar.gz\")|" "$ARCHWORK/PKGBUILD"
(
  cd "$ARCHWORK"
  makepkg --clean --cleanbuild --force --nodeps --noconfirm
)
install -m644 "$ARCHWORK/cognac-bin-${VERSION}-1-x86_64.pkg.tar.zst" "$DIST/"
install -m644 packaging/arch/PKGBUILD packaging/arch/.SRCINFO "$DIST/"

(
  cd "$DIST"
  sha256sum \
    "cognac_${VERSION}_amd64.deb" \
    "cognac-${VERSION}-1.x86_64.rpm" \
    "cognac-bin-${VERSION}-1-x86_64.pkg.tar.zst" \
    "Cognac-${VERSION}-x86_64.AppImage" \
    "$PORTABLE.tar.gz" \
    PKGBUILD .SRCINFO > SHA256SUMS
)

echo "Release artifacts are in $DIST"
