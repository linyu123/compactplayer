#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [ "$(uname -s)" != Darwin ] || [ "$(uname -m)" != arm64 ]; then
    echo "This runtime installer currently supports Apple Silicon macOS only." >&2
    exit 1
fi
archive=$(mktemp -t compact-libmpv)
trap 'rm -f "$archive"' EXIT
curl -fLsS --retry 2 --connect-timeout 10 --max-time 180 \
    https://github.com/media-kit/libmpv-darwin-build/releases/download/v0.7.3/libmpv-libs_v0.7.3_macos-arm64-video-default.tar.gz \
    -o "$archive"
printf '%s  %s\n' 9137a736c97dd8429a51b2dbbf061b69e879b0b69dc88b9d3a200d2354166af5 "$archive" | shasum -a 256 -c -
mkdir -p runtime
tar -xzf "$archive" -C runtime
for library in runtime/libmpv-libs_v0.7.3_macos-arm64-video-default/*.dylib; do
    install_name_tool -add_rpath @loader_path "$library"
    codesign --force --sign - "$library"
done
cargo run --bin check_libmpv -- runtime/libmpv-libs_v0.7.3_macos-arm64-video-default/libmpv.dylib
