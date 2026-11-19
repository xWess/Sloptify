#!/usr/bin/env bash
# Build a relocatable AppImage from a release binary (Linux x86_64).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
VERSION="${VERSION:-$(grep -m1 '^version' "$ROOT/Cargo.toml" | cut -d'"' -f2)}"
OUT_DIR="${OUT_DIR:-$ROOT/dist}"
BIN="${BIN:-$ROOT/target/release/sloptify}"

mkdir -p "$OUT_DIR"
APPDIR="$OUT_DIR/Sloptify.AppDir"
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/applications" "$APPDIR/usr/share/icons/hicolor/scalable/apps"

install -Dm755 "$BIN" "$APPDIR/usr/bin/sloptify"
install -Dm644 "$ROOT/pkgbuild/sloptify.desktop" "$APPDIR/usr/share/applications/sloptify.desktop"
# AppImage desktop entry must use AppRun-relative Exec.
sed -i 's|^Exec=.*|Exec=sloptify|' "$APPDIR/usr/share/applications/sloptify.desktop"
install -Dm644 "$ROOT/crates/sloptify/assets/sloptify-cassette.svg" \
  "$APPDIR/usr/share/icons/hicolor/scalable/apps/sloptify.svg"
ln -sf usr/share/applications/sloptify.desktop "$APPDIR/sloptify.desktop"
ln -sf usr/share/icons/hicolor/scalable/apps/sloptify.svg "$APPDIR/sloptify.svg"
ln -sf usr/bin/sloptify "$APPDIR/AppRun"

# Bundle common GTK/WebKit deps when linuxdeploy is available; otherwise ship thin AppDir.
LINUXDEPLOY="${LINUXDEPLOY:-}"
if [[ -z "$LINUXDEPLOY" && -x /tmp/linuxdeploy-x86_64.AppImage ]]; then
  LINUXDEPLOY=/tmp/linuxdeploy-x86_64.AppImage
fi

APPIMAGE_OUT="$OUT_DIR/Sloptify-${VERSION}-x86_64.AppImage"
if [[ -n "$LINUXDEPLOY" ]]; then
  export LDAI_OUTPUT="$APPIMAGE_OUT"
  export LDAI_UPDATE_INFORMATION="gh-releases-zsync|xWess|Sloptify|latest|Sloptify-*x86_64.AppImage.zsync"
  "$LINUXDEPLOY" --appdir "$APPDIR" --output appimage \
    --desktop-file "$APPDIR/usr/share/applications/sloptify.desktop" \
    --icon-file "$APPDIR/usr/share/icons/hicolor/scalable/apps/sloptify.svg"
else
  # Fallback: appimagetool on a thin AppDir (host libs required at runtime).
  if ! command -v appimagetool >/dev/null 2>&1; then
    echo "Install linuxdeploy or appimagetool, or set LINUXDEPLOY=" >&2
    exit 1
  fi
  ARCH=x86_64 appimagetool "$APPDIR" "$APPIMAGE_OUT"
fi

echo "Built $APPIMAGE_OUT"
