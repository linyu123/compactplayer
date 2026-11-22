#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
architecture=${1:-$(uname -m)}
case "$architecture" in
    arm64) target=aarch64-apple-darwin; runtime_arch=arm64; minimum=14.0 ;;
    x86_64|amd64) architecture=x86_64; target=x86_64-apple-darwin; runtime_arch=amd64; minimum=12.0 ;;
    *) echo "不支持的处理器：$architecture" >&2; exit 1 ;;
esac
MACOSX_DEPLOYMENT_TARGET="$minimum" cargo build --release --offline --target "$target" --bin compact-player
bundle="dist/$architecture/轻巧播放器.app"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Frameworks" "$bundle/Contents/Resources"
cp "target/$target/release/compact-player" "$bundle/Contents/MacOS/compact-player"
cp resources/Info.plist "$bundle/Contents/Info.plist"
cp resources/AppIcon.icns "$bundle/Contents/Resources/AppIcon.icns"
/usr/libexec/PlistBuddy -c "Set :LSMinimumSystemVersion $minimum" "$bundle/Contents/Info.plist"
cp runtime/libmpv-libs_v0.7.3_macos-"$runtime_arch"-video-default/*.dylib "$bundle/Contents/Frameworks/"
for library in "$bundle/Contents/Frameworks/"*.dylib; do
    codesign --force --sign - "$library"
done
codesign --force --sign - "$bundle"
codesign --verify --deep --strict "$bundle"
staging=$(mktemp -d -t compact-player-dmg)
trap 'rm -rf "$staging"' EXIT
ditto "$bundle" "$staging/轻巧播放器.app"
ln -s /Applications "$staging/Applications"
hdiutil create -volname '轻巧播放器' -srcfolder "$staging" -ov -format UDZO "dist/CompactPlayer-0.1.4-${architecture}.dmg"
