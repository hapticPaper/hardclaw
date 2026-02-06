#!/bin/bash
set -e

if [ ! -f "Cargo.toml" ] || [ ! -f "hardclaw-mcp/package.json" ]; then
  echo "Error: Must run from repo root"
  exit 1
fi

CHANGES=$(git status --porcelain --untracked-files=no | grep -v '^ M release\.sh$' || true)
if [ -n "$CHANGES" ]; then
  echo "Error: Uncommitted tracked changes"
  echo "$CHANGES"
  exit 1
fi

bump_version() {
  local v=$1
  local type=$2
  IFS='.' read -r major minor patch <<< "$v"
  
  case $type in
    major) echo "$((major+1)).0.0" ;;
    minor) echo "$major.$((minor+1)).0" ;;
    patch) echo "$major.$minor.$((patch+1))" ;;
  esac
}

RUST=$(grep '^version' Cargo.toml | head -1 | cut -d'"' -f2)
MCP=$(grep '"version"' hardclaw-mcp/package.json | head -1 | cut -d'"' -f4)

type=${1:-patch}

case $type in
  major|minor|patch) ;;
  *) echo "Usage: $0 [patch|minor|major]"; exit 1 ;;
esac

NEW_RUST=$(bump_version "$RUST" "$type")
NEW_MCP=$(bump_version "$MCP" "$type")

sed -i.bak "s/^version = \"$RUST\"/version = \"$NEW_RUST\"/" Cargo.toml
rm Cargo.toml.bak

sed -i.bak "s/\"version\": \"$MCP\"/\"version\": \"$NEW_MCP\"/" hardclaw-mcp/package.json
rm hardclaw-mcp/package.json.bak

echo "Publishing @hardclaw/mcp-server v$NEW_MCP to npm..."
(
  cd hardclaw-mcp
  npm install
  npm publish --access public
)

git add Cargo.toml hardclaw-mcp/package.json hardclaw-mcp/package-lock.json
git commit -m "Release v$NEW_RUST / mcp-v$NEW_MCP"
git tag "v$NEW_RUST"
git tag "mcp-v$NEW_MCP"

BRANCH=$(git rev-parse --abbrev-ref HEAD)
git push origin "$BRANCH" --tags

echo "✓ Released v$NEW_RUST / mcp-v$NEW_MCP"
