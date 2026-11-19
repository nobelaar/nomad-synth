#!/usr/bin/env bash
# Build the Linux release artifacts into rust/target/dist:
#   NOMAD-<version>-x86_64.AppImage, nomad-<version>-x86_64-linux.tar.gz, SHA256SUMS.
# Usage: packaging/linux/build.sh [--skip-build]
# The AppImage bundles only the nomad binary: ALSA, GPU drivers and libGL/Vulkan come
# from the system (AppImage exclude list). Nothing outside the repository is touched.
set -euo pipefail

APPIMAGETOOL_URL="https://github.com/AppImage/appimagetool/releases/download/1.9.0/appimagetool-x86_64.AppImage"
APPIMAGETOOL_SHA256="46fdd785094c7f6e545b61afcfb0f3d98d8eab243f644b4b17698c01d06083d1"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
RUST="$ROOT/rust"
PKG="$ROOT/packaging/linux"
WORK="$RUST/target/appimage"
DIST="$RUST/target/dist"
APPDIR="$WORK/NOMAD.AppDir"

skip_build=0
for arg in "$@"; do
    case "$arg" in
        --skip-build) skip_build=1 ;;
        *) echo "usage: $0 [--skip-build]" >&2; exit 2 ;;
    esac
done

VERSION="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$RUST/nomad-app/Cargo.toml" | head -n 1)"
[ -n "$VERSION" ] || { echo "no version in nomad-app/Cargo.toml" >&2; exit 1; }

if [ "$skip_build" -eq 0 ]; then
    (cd "$RUST" && cargo build --release -p nomad-app)
fi
BIN="$RUST/target/release/nomad"
[ -x "$BIN" ] || { echo "missing $BIN (build first or drop --skip-build)" >&2; exit 1; }

# AppDir
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/doc/nomad"
install -m 755 "$BIN" "$APPDIR/usr/bin/nomad"
install -m 755 "$PKG/AppRun" "$APPDIR/AppRun"
install -m 644 "$PKG/nomad.desktop" "$APPDIR/nomad.desktop"
install -m 644 "$PKG/nomad.svg" "$APPDIR/nomad.svg"
cp "$PKG/nomad.svg" "$APPDIR/.DirIcon"
install -m 644 "$ROOT/LICENSE" "$ROOT/THIRD_PARTY_NOTICES.md" "$APPDIR/usr/share/doc/nomad/"

# appimagetool, pinned by version and checksum
TOOL="$WORK/appimagetool-x86_64.AppImage"
if [ ! -f "$TOOL" ]; then
    curl -fL --retry 3 -o "$TOOL.part" "$APPIMAGETOOL_URL"
    mv "$TOOL.part" "$TOOL"
fi
if ! echo "$APPIMAGETOOL_SHA256  $TOOL" | sha256sum -c --quiet -; then
    echo "appimagetool checksum mismatch: refusing to run it" >&2
    rm -f "$TOOL"
    exit 1
fi
chmod +x "$TOOL"

mkdir -p "$DIST"
APPIMAGE="NOMAD-$VERSION-x86_64.AppImage"
TARBALL="nomad-$VERSION-x86_64-linux.tar.gz"
rm -f "$DIST/$APPIMAGE" "$DIST/$TARBALL" "$DIST/SHA256SUMS"

# Extract-and-run: works without FUSE (CI runners, containers).
APPIMAGE_EXTRACT_AND_RUN=1 ARCH=x86_64 "$TOOL" --no-appstream "$APPDIR" "$DIST/$APPIMAGE"

# Tarball
STAGE="$WORK/nomad-$VERSION-x86_64-linux"
rm -rf "$STAGE"
mkdir -p "$STAGE"
install -m 755 "$BIN" "$STAGE/nomad"
install -m 644 "$ROOT/LICENSE" "$ROOT/THIRD_PARTY_NOTICES.md" "$ROOT/README.md" \
    "$PKG/nomad.desktop" "$PKG/nomad.svg" "$STAGE/"
tar -C "$WORK" --sort=name --owner=0 --group=0 --numeric-owner \
    -czf "$DIST/$TARBALL" "nomad-$VERSION-x86_64-linux"

(cd "$DIST" && sha256sum "$APPIMAGE" "$TARBALL" > SHA256SUMS)
echo "built:"
ls -l "$DIST"
