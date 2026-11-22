#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
MACOSX_DEPLOYMENT_TARGET=14.0 cargo build --release --offline --bin compact-player
bundle='dist/main/轻巧播放器.app'
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Frameworks" "$bundle/Contents/Resources"
cp target/release/compact-player "$bundle/Contents/MacOS/compact-player"
cp resources/Info.plist "$bundle/Contents/Info.plist"
cp resources/AppIcon.icns "$bundle/Contents/Resources/AppIcon.icns"
cp runtime/libmpv-libs_v0.7.3_macos-arm64-video-default/*.dylib "$bundle/Contents/Frameworks/"
for library in "$bundle/Contents/Frameworks/"*.dylib; do
    codesign --force --sign - "$library"
done
codesign --force --sign - "$bundle"
codesign --verify --deep --strict "$bundle"
staging=$(mktemp -d -t compact-player-dmg)
trap 'rm -rf "$staging"' EXIT
ditto "$bundle" "$staging/轻巧播放器.app"
ln -s /Applications "$staging/Applications"
hdiutil create -volname '轻巧播放器' -srcfolder "$staging" -ov -format UDZO dist/CompactPlayer-0.1.6-main-arm64.dmg
