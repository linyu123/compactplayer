#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --offline --bin compact-player
mkdir -p 'target/Compact Player.app/Contents/MacOS'
mkdir -p 'target/Compact Player.app/Contents/Resources'
cp resources/AppIcon.icns 'target/Compact Player.app/Contents/Resources/AppIcon.icns'
cp target/debug/compact-player 'target/Compact Player.app/Contents/MacOS/compact-player'
cp resources/Info.plist 'target/Compact Player.app/Contents/Info.plist'
codesign --force --sign - 'target/Compact Player.app'
open -n 'target/Compact Player.app' --args "$@"
