#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
swift scripts/generate-icon.swift resources/AppIcon.png
temporary=$(mktemp -d -t compact-icon)
iconset="$temporary/AppIcon.iconset"
mkdir -p "$iconset"
trap 'rm -rf "$temporary"' EXIT
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" resources/AppIcon.png --out "$iconset/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z "$double" "$double" resources/AppIcon.png --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o resources/AppIcon.icns
