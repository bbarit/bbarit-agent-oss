#!/usr/bin/env bash
# Deploy prebuilt binaries + manifest + installer to bbarit.com so that
#   curl -fsSL https://bbarit.com/agent/install.sh | sh
#   bbarit --upgrade
# all resolve. Run after a GitHub release, or to publish a local build.
#
# Required env:
#   BBARIT_SSH          e.g. ubuntu@bbarit.com  (ssh target)
#   BBARIT_AGENT_ROOT   remote dir Nginx serves at https://bbarit.com/agent
# Optional:
#   VERSION             defaults to the crate version in Cargo.toml
#   FROM_GH=1           download all-platform binaries from the GitHub release
#                       (needs `gh`); otherwise only the local build is shipped.
set -euo pipefail
cd "$(dirname "$0")/.."

: "${BBARIT_SSH:?set BBARIT_SSH=user@host}"
: "${BBARIT_AGENT_ROOT:?set BBARIT_AGENT_ROOT=/var/www/.../agent}"
VERSION="${VERSION:-$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -1)}"
BASE="https://bbarit.com/agent/dist/$VERSION"

echo "Deploying bbarit-oss v$VERSION → $BBARIT_SSH:$BBARIT_AGENT_ROOT"
rm -rf .deploy && mkdir -p ".deploy/dist/$VERSION"

if [ "${FROM_GH:-0}" = "1" ]; then
  echo "Fetching release artifacts from GitHub…"
  gh release download "v$VERSION" --dir ".deploy/dist/$VERSION" --pattern 'bbarit-oss-*'
else
  echo "Building local target…"
  cargo build --release
  case "$(uname -sm)" in
    "Darwin arm64") key=macos-arm64 ;;
    "Darwin x86_64") key=macos-x64 ;;
    "Linux x86_64") key=linux-x64 ;;
    "Linux aarch64") key=linux-arm64 ;;
    *) echo "unknown local platform; use FROM_GH=1" >&2; exit 1 ;;
  esac
  cp target/release/bbarit-oss ".deploy/dist/$VERSION/bbarit-oss-$key"
  echo "NOTE: only $key was built locally. Use FROM_GH=1 to ship every platform."
fi

cat > .deploy/latest.json <<JSON
{
  "version": "$VERSION",
  "targets": {
    "macos-arm64": "$BASE/bbarit-oss-macos-arm64",
    "macos-x64":   "$BASE/bbarit-oss-macos-x64",
    "linux-x64":   "$BASE/bbarit-oss-linux-x64",
    "linux-arm64": "$BASE/bbarit-oss-linux-arm64",
    "windows-x64": "$BASE/bbarit-oss-windows-x64.exe"
  }
}
JSON
cp install.sh .deploy/install.sh
cp install.ps1 .deploy/install.ps1

echo "Uploading…"
ssh "$BBARIT_SSH" "mkdir -p '$BBARIT_AGENT_ROOT/dist/$VERSION'"
rsync -av ".deploy/dist/$VERSION/" "$BBARIT_SSH:$BBARIT_AGENT_ROOT/dist/$VERSION/"
rsync -av .deploy/latest.json .deploy/install.sh .deploy/install.ps1 "$BBARIT_SSH:$BBARIT_AGENT_ROOT/"

rm -rf .deploy
echo "Done. Verify:  curl -fsSL https://bbarit.com/agent/latest.json"
