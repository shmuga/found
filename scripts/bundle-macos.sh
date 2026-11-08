#!/usr/bin/env bash
# Build Found.app (and optionally a .dmg) in target/macos/.
#
#   scripts/bundle-macos.sh              # this Mac's architecture
#   scripts/bundle-macos.sh --universal  # Apple silicon + Intel (needs both rustup targets)
#   scripts/bundle-macos.sh --dmg        # also make target/macos/Found.dmg
#   scripts/bundle-macos.sh --universal --dmg --notarize   # release build
#
# Signing: uses the first "Developer ID Application" certificate in your keychain (or
# SIGN_IDENTITY), with the hardened runtime; without one the app is ad-hoc signed (runs on
# this Mac only). --notarize submits the DMG to Apple with the notarytool keychain profile
# NOTARY_PROFILE (default "found-notary") and staples the ticket.
set -euo pipefail
cd "$(dirname "$0")/.."

UNIVERSAL=0
DMG=0
NOTARIZE=0
for a in "$@"; do
  case "$a" in
    --universal) UNIVERSAL=1 ;;
    --dmg) DMG=1 ;;
    --notarize) NOTARIZE=1; DMG=1 ;;
    *) echo "unknown option: $a" >&2; exit 2 ;;
  esac
done

NAME="Found"
BIN="found"
ID="${BUNDLE_ID:-io.github.shmuga.found}"
SIGN="${SIGN_IDENTITY:-$(security find-identity -v -p codesigning 2>/dev/null | sed -n 's/.*"\(Developer ID Application:[^"]*\)".*/\1/p' | head -1)}"
if [ "$NOTARIZE" = 1 ] && [ -z "$SIGN" ]; then
  echo "--notarize needs a Developer ID Application certificate (see README › Releases)" >&2
  exit 1
fi
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
OUT="target/macos"
APP="$OUT/$NAME.app"

echo "==> building $NAME $VERSION"
if [ "$UNIVERSAL" = 1 ]; then
  for t in aarch64-apple-darwin x86_64-apple-darwin; do
    rustup target add "$t" >/dev/null
    cargo build --release --target "$t"
  done
  mkdir -p target/universal
  lipo -create -output "target/universal/$BIN" \
    "target/aarch64-apple-darwin/release/$BIN" "target/x86_64-apple-darwin/release/$BIN"
  EXE="target/universal/$BIN"
else
  cargo build --release
  EXE="target/release/$BIN"
fi

echo "==> icon (assets/icon.png — change with scripts/set-icon.sh a|b|c)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
cp assets/icon.png "$WORK/icon.png"
ICONSET="$WORK/AppIcon.iconset"
mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
  sips -z $s $s "$WORK/icon.png" --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  d=$((s * 2))
  sips -z $d $d "$WORK/icon.png" --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$WORK/AppIcon.icns"

echo "==> bundle $APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$EXE" "$APP/Contents/MacOS/$BIN"
cp "$WORK/AppIcon.icns" "$APP/Contents/Resources/AppIcon.icns"
cp LICENSE CREDITS.md "$APP/Contents/Resources/"
cp -R licenses "$APP/Contents/Resources/"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>$NAME</string>
  <key>CFBundleDisplayName</key><string>$NAME</string>
  <key>CFBundleIdentifier</key><string>$ID</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleExecutable</key><string>$BIN</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.photography</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
</dict>
</plist>
PLIST

if [ -n "$SIGN" ]; then
  echo "==> sign: $SIGN (hardened runtime)"
  codesign --force --options runtime --timestamp --sign "$SIGN" "$APP"
else
  echo "==> sign (ad-hoc — no Developer ID certificate found)"
  codesign --force --sign - "$APP"
fi
codesign --verify --strict --verbose=1 "$APP"

if [ "$DMG" = 1 ]; then
  echo "==> dmg"
  STAGE="$WORK/dmg"
  mkdir -p "$STAGE"
  cp -R "$APP" "$STAGE/"
  ln -s /Applications "$STAGE/Applications"
  rm -f "$OUT/$NAME.dmg"
  hdiutil create -volname "$NAME" -srcfolder "$STAGE" -ov -format UDZO "$OUT/$NAME.dmg" >/dev/null
  if [ -n "$SIGN" ]; then
    codesign --force --timestamp --sign "$SIGN" "$OUT/$NAME.dmg"
  fi
  echo "    $OUT/$NAME.dmg"
fi

if [ "$NOTARIZE" = 1 ]; then
  echo "==> notarize (this takes a few minutes)"
  xcrun notarytool submit "$OUT/$NAME.dmg" --keychain-profile "${NOTARY_PROFILE:-found-notary}" --wait
  xcrun stapler staple "$OUT/$NAME.dmg"
  spctl --assess --type open --context context:primary-signature --verbose=2 "$OUT/$NAME.dmg"
fi

echo "==> done: $APP"
echo "    open \"$APP\"   or   cp -R \"$APP\" /Applications/"
