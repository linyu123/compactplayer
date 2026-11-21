#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --offline --bin compact-player
mkdir -p 'target/Compact Player.app/Contents/MacOS'
cp target/debug/compact-player 'target/Compact Player.app/Contents/MacOS/compact-player'
cp resources/Info.plist 'target/Compact Player.app/Contents/Info.plist'
open -n 'target/Compact Player.app' --args "$@"
