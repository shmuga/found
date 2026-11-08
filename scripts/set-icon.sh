#!/usr/bin/env bash
# Set the app icon (window, Dock and Found.app), then run scripts/bundle-macos.sh.
#
#   scripts/set-icon.sh                    # current artwork: assets/icon-source.jpg
#   scripts/set-icon.sh path/to/art.png    # your own square picture
#   scripts/set-icon.sh a|b|c              # one of the generated designs (examples/icon.rs)
#
# Pictures are fitted into the macOS icon shape (rounded square, transparent margin, shadow).
set -euo pipefail
cd "$(dirname "$0")/.."
arg="${1:-assets/icon-source.jpg}"
case "$arg" in
  a|b|c)
    mkdir -p assets/icons
    cargo run --release -q --example icon -- "$arg" "assets/icons/icon-$arg.png" >/dev/null
    cp "assets/icons/icon-$arg.png" assets/icon.png ;;
  *)
    [ -f "$arg" ] || { echo "no such file: $arg" >&2; exit 2; }
    if [ "$arg" != assets/icon-source.jpg ]; then
      sips -s format jpeg -s formatOptions 95 "$arg" --out assets/icon-source.jpg >/dev/null
    fi
    cargo run --release -q --example icon -- from assets/icon-source.jpg assets/icon.png >/dev/null ;;
esac
echo "icon → assets/icon.png (rebuild the app: scripts/bundle-macos.sh)"
