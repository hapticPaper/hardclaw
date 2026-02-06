#!/usr/bin/env bash
set -euo pipefail

TARGET="$1"
ARTIFACT_NAME="$2"
VERSION="$3"

ROOT_DIR="$(pwd)"
DIST_DIR="$ROOT_DIR/dist"
BUILD_DIR="$ROOT_DIR/target/$TARGET/release"

mkdir -p "$DIST_DIR"

# Plain binary tar.gz — just the binary, nothing else
tar -czf "$DIST_DIR/$ARTIFACT_NAME.tar.gz" -C "$BUILD_DIR" hardclaw
