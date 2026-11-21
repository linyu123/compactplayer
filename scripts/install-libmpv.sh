#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
architecture=${1:-$(uname -m)}
case "$architecture" in
    arm64) checksum=9137a736c97dd8429a51b2dbbf061b69e879b0b69dc88b9d3a200d2354166af5 ;;
    x86_64|amd64) architecture=amd64; checksum=0dc5fc4f33eb40ec8f114b47100e0c0ddf690445f47be4afc4b83dbedd3316a9 ;;
    *) echo "不支持的处理器：$architecture" >&2; exit 1 ;;
esac
runtime="libmpv-libs_v0.7.3_macos-${architecture}-video-default"
archive=$(mktemp -t compact-libmpv)
trap 'rm -f "$archive"' EXIT
curl -fLsS --retry 2 --connect-timeout 10 --max-time 180 \
    "https://github.com/media-kit/libmpv-darwin-build/releases/download/v0.7.3/${runtime}.tar.gz" \
    -o "$archive"
printf '%s  %s\n' "$checksum" "$archive" | shasum -a 256 -c -
mkdir -p runtime
tar -xzf "$archive" -C runtime
for library in "runtime/$runtime/"*.dylib; do
    install_name_tool -add_rpath @loader_path "$library"
    codesign --force --sign - "$library"
done
if [ "$architecture" = "$(uname -m)" ] || { [ "$architecture" = amd64 ] && [ "$(uname -m)" = x86_64 ]; }; then
    cargo run --bin check_libmpv -- "runtime/$runtime/libmpv.dylib"
fi
