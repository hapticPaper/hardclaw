#!/usr/bin/env bash
set -euo pipefail

TARGET="$1"
ARTIFACT_NAME="$2"

ROOT_DIR="$(pwd)"
DIST_DIR="$ROOT_DIR/dist"
BUILD_DIR="$ROOT_DIR/target/$TARGET/release"
APPDIR="$DIST_DIR/AppDir"

DESKTOP_SRC="$ROOT_DIR/packaging/linux/hardclaw.desktop"
ICON_SRC="$ROOT_DIR/docs/assets/claw-256.png"
ICON_16="$ROOT_DIR/docs/assets/favicon-16.png"
ICON_32="$ROOT_DIR/docs/assets/favicon-32.png"
ICON_128="$ROOT_DIR/docs/assets/claw-128.png"

rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" \
  "$APPDIR/usr/share/applications" \
  "$APPDIR/usr/share/icons/hicolor/256x256/apps" \
  "$APPDIR/usr/share/icons/hicolor/128x128/apps" \
  "$APPDIR/usr/share/icons/hicolor/32x32/apps" \
  "$APPDIR/usr/share/icons/hicolor/16x16/apps"

cp "$BUILD_DIR/hardclaw" "$APPDIR/usr/bin/"
chmod +x "$APPDIR/usr/bin/hardclaw"

# Backward-compat symlinks for old binary names
ln -s hardclaw "$APPDIR/usr/bin/hardclaw-node"
ln -s hardclaw "$APPDIR/usr/bin/hardclaw-cli"

if [[ -f "$DESKTOP_SRC" ]]; then
  cp "$DESKTOP_SRC" "$APPDIR/usr/share/applications/hardclaw.desktop"
else
  echo "Error: desktop entry not found at $DESKTOP_SRC" >&2
  exit 1
fi

if [[ -f "$ICON_SRC" ]]; then
  cp "$ICON_SRC" "$APPDIR/usr/share/icons/hicolor/256x256/apps/hardclaw.png"
else
  echo "Error: icon source not found at $ICON_SRC" >&2
  exit 1
fi

if [[ -f "$ICON_128" ]]; then
  cp "$ICON_128" "$APPDIR/usr/share/icons/hicolor/128x128/apps/hardclaw.png"
fi

if [[ -f "$ICON_32" ]]; then
  cp "$ICON_32" "$APPDIR/usr/share/icons/hicolor/32x32/apps/hardclaw.png"
fi

if [[ -f "$ICON_16" ]]; then
  cp "$ICON_16" "$APPDIR/usr/share/icons/hicolor/16x16/apps/hardclaw.png"
fi

export APPIMAGE_EXTRACT_AND_RUN=1

pushd "$DIST_DIR" >/dev/null
"$DIST_DIR/linuxdeploy.AppImage" \
  --appdir "$APPDIR" \
  --desktop-file "$APPDIR/usr/share/applications/hardclaw.desktop" \
  --icon-file "$APPDIR/usr/share/icons/hicolor/256x256/apps/hardclaw.png" \
  --output appimage
popd >/dev/null

APPIMAGE_PATH="$(ls "$DIST_DIR"/*.AppImage 2>/dev/null | grep -v "linuxdeploy" | head -n 1 || true)"
if [[ -z "$APPIMAGE_PATH" ]]; then
  echo "Error: AppImage was not created" >&2
  exit 1
fi

mv "$APPIMAGE_PATH" "$DIST_DIR/$ARTIFACT_NAME.AppImage"
