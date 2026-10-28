#!/bin/sh
# Scan + map benchmark using the headless CLI (release build). Usage: bench/scan.sh [root ...]
# Only pass folders your terminal can read without a privacy prompt (not ~/Library/Containers, /, etc.).
set -e
cd "$(dirname "$0")/../src-tauri"
cargo build --release --quiet --bin atlas
[ $# -eq 0 ] && set -- "$HOME/Documents" /Applications /opt/homebrew
for root in "$@"; do
  [ -d "$root" ] || continue
  ./target/release/atlas scan "$root" 2>/dev/null | grep -E '^(scanned|patch)' | sed "s|^|$root: |"
done
