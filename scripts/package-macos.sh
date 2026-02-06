#!/usr/bin/env bash
set -euo pipefail

TARGET="$1"
ARTIFACT_NAME="$2"
VERSION="$3"

ROOT_DIR="$(pwd)"
DIST_DIR="$ROOT_DIR/dist"
BUILD_DIR="$ROOT_DIR/target/$TARGET/release"

APP_NAME="HardClaw"
APP_DIR="$DIST_DIR/$APP_NAME.app"
CONTENTS_DIR="$APP_DIR/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"
RESOURCES_DIR="$CONTENTS_DIR/Resources"

ICON_LARGE="$ROOT_DIR/docs/assets/claw-transparent.png"
ICON_16="$ROOT_DIR/docs/assets/favicon-16.png"
ICON_32="$ROOT_DIR/docs/assets/favicon-32.png"

rm -rf "$DIST_DIR"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"

# Copy single binary
cp "$BUILD_DIR/hardclaw" "$MACOS_DIR/hardclaw-bin"
chmod +x "$MACOS_DIR/hardclaw-bin"

# Create launcher script that opens Terminal with the TUI
cat > "$MACOS_DIR/hardclaw" <<'LAUNCHER'
#!/bin/bash
DIR="$(cd "$(dirname "$0")" && pwd)"
if [ -t 0 ] && [ -t 1 ]; then
  # Already in a terminal — run directly
  exec "$DIR/hardclaw-bin" "$@"
else
  # Launched from Finder — open Terminal and run the TUI
  osascript <<EOF
tell application "Terminal"
  activate
  do script "'$DIR/hardclaw-bin'"
end tell
EOF
fi
LAUNCHER
chmod +x "$MACOS_DIR/hardclaw"

# Build .icns icon
if [[ -f "$ICON_LARGE" ]]; then
  ICONSET_DIR="$DIST_DIR/AppIcon.iconset"
  ICNS_PATH="$RESOURCES_DIR/AppIcon.icns"

  mkdir -p "$ICONSET_DIR"
  if [[ -f "$ICON_16" ]]; then
    cp "$ICON_16" "$ICONSET_DIR/icon_16x16.png"
  else
    sips -z 16 16 "$ICON_LARGE" --out "$ICONSET_DIR/icon_16x16.png" >/dev/null
  fi

  if [[ -f "$ICON_32" ]]; then
    cp "$ICON_32" "$ICONSET_DIR/icon_16x16@2x.png"
    cp "$ICON_32" "$ICONSET_DIR/icon_32x32.png"
  else
    sips -z 32 32 "$ICON_LARGE" --out "$ICONSET_DIR/icon_16x16@2x.png" >/dev/null
    sips -z 32 32 "$ICON_LARGE" --out "$ICONSET_DIR/icon_32x32.png" >/dev/null
  fi

  sips -z 64 64 "$ICON_LARGE" --out "$ICONSET_DIR/icon_32x32@2x.png" >/dev/null
  sips -z 128 128 "$ICON_LARGE" --out "$ICONSET_DIR/icon_128x128.png" >/dev/null
  sips -z 256 256 "$ICON_LARGE" --out "$ICONSET_DIR/icon_128x128@2x.png" >/dev/null
  sips -z 256 256 "$ICON_LARGE" --out "$ICONSET_DIR/icon_256x256.png" >/dev/null
  sips -z 512 512 "$ICON_LARGE" --out "$ICONSET_DIR/icon_256x256@2x.png" >/dev/null
  sips -z 512 512 "$ICON_LARGE" --out "$ICONSET_DIR/icon_512x512.png" >/dev/null
  sips -z 1024 1024 "$ICON_LARGE" --out "$ICONSET_DIR/icon_512x512@2x.png" >/dev/null

  iconutil -c icns "$ICONSET_DIR" -o "$ICNS_PATH"
else
  echo "Warning: icon source not found at $ICON_LARGE"
fi

cat > "$CONTENTS_DIR/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key>
  <string>hardclaw</string>
  <key>CFBundleIdentifier</key>
  <string>com.hardclaw.hardclaw</string>
  <key>CFBundleName</key>
  <string>HardClaw</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>$VERSION</string>
  <key>CFBundleVersion</key>
  <string>$VERSION</string>
  <key>CFBundleIconFile</key>
  <string>AppIcon</string>
</dict>
</plist>
EOF

# Ad-hoc codesign to prevent "damaged" error on macOS
codesign --force --deep -s - "$APP_DIR"

# Build DMG with .app + raw binary
DMG_ROOT="$DIST_DIR/dmg-root"
mkdir -p "$DMG_ROOT"
cp -R "$APP_DIR" "$DMG_ROOT/"
cp "$BUILD_DIR/hardclaw" "$DMG_ROOT/hardclaw"
chmod +x "$DMG_ROOT/hardclaw"

hdiutil create -volname "HardClaw" -srcfolder "$DMG_ROOT" -ov -format UDZO "$DIST_DIR/$ARTIFACT_NAME.dmg"
