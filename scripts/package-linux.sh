#!/usr/bin/env bash
set -euo pipefail

TARGET="$1"
ARTIFACT_NAME="$2"
VERSION="$3"

ROOT_DIR="$(pwd)"
DIST_DIR="$ROOT_DIR/dist"
BUILD_DIR="$ROOT_DIR/target/$TARGET/release"
STAGING_DIR="$DIST_DIR/$ARTIFACT_NAME"

ICON_SRC="$ROOT_DIR/docs/assets/apple-touch-icon.png"
ICON_16="$ROOT_DIR/docs/assets/favicon-16.png"
ICON_32="$ROOT_DIR/docs/assets/favicon-32.png"
DESKTOP_SRC="$ROOT_DIR/packaging/linux/hardclaw.desktop"

rm -rf "$DIST_DIR"
mkdir -p "$STAGING_DIR/bin" "$STAGING_DIR/share/applications" "$STAGING_DIR/share/icons/hicolor/256x256/apps"

cp "$BUILD_DIR/hardclaw" "$STAGING_DIR/bin/"
cp "$BUILD_DIR/hardclaw-cli" "$STAGING_DIR/bin/"
cp "$BUILD_DIR/hardclaw-node" "$STAGING_DIR/bin/"

chmod +x "$STAGING_DIR/bin/hardclaw" "$STAGING_DIR/bin/hardclaw-cli" "$STAGING_DIR/bin/hardclaw-node"

if [[ -f "$ICON_SRC" ]]; then
  cp "$ICON_SRC" "$STAGING_DIR/share/icons/hicolor/256x256/apps/hardclaw.png"
else
  echo "Warning: icon source not found at $ICON_SRC"
fi

if [[ -f "$ICON_32" ]]; then
  mkdir -p "$STAGING_DIR/share/icons/hicolor/32x32/apps"
  cp "$ICON_32" "$STAGING_DIR/share/icons/hicolor/32x32/apps/hardclaw.png"
fi

if [[ -f "$ICON_16" ]]; then
  mkdir -p "$STAGING_DIR/share/icons/hicolor/16x16/apps"
  cp "$ICON_16" "$STAGING_DIR/share/icons/hicolor/16x16/apps/hardclaw.png"
fi

if [[ -f "$DESKTOP_SRC" ]]; then
  cp "$DESKTOP_SRC" "$STAGING_DIR/share/applications/hardclaw.desktop"
else
  cat > "$STAGING_DIR/share/applications/hardclaw.desktop" <<EOF
[Desktop Entry]
Name=HardClaw
Comment=HardClaw Onboarding and Node Tools
Exec=hardclaw
Icon=hardclaw
Terminal=true
Type=Application
Categories=Network;Utility;
EOF
fi

cat > "$STAGING_DIR/README-LINUX.txt" <<EOF
HardClaw $VERSION

This archive uses a standard Linux layout:
- bin/ contains executables
- share/applications contains the desktop entry
- share/icons contains the application icon

To install system-wide:
1) sudo cp -r bin/* /usr/local/bin/
2) sudo cp -r share/* /usr/local/share/
EOF

tar -czf "$DIST_DIR/$ARTIFACT_NAME.tar.gz" -C "$DIST_DIR" "$ARTIFACT_NAME"
