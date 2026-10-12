#!/bin/bash
# Builds Kinetic Studio for Apple Silicon: the studio under its own name, with
# the pinned yue2.cpp engine (Metal) and the audio-to-MIDI runtime bundled
# inside "Kinetic Studio.app", plus a .dmg. Ad-hoc signed, not notarized.
# Model weights are not part of it; the app downloads them on first start,
# only when the user presses download.
#
# Usage: scripts/build-kinetic-macos.sh
# Output: desktop/src-tauri/target/release/bundle/{macos,dmg}/
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"

if [ "$(uname -s)" != "Darwin" ] || [ "$(uname -m)" != "arm64" ]; then
    echo "Kinetic Studio's macOS build needs an Apple Silicon Mac (this is $(uname -s) $(uname -m))." >&2
    exit 1
fi

engine="$root/desktop/src-tauri/resources/yue2-cpp"
commit="$(sed -n 's/.*"commit": "\(.*\)".*/\1/p' "$root/engines/yue2-cpp-source.json")"
if [ ! -x "$engine/yue-server" ] || [ "$(cat "$engine/runtime.commit" 2>/dev/null)" != "$commit" ]; then
    rm -rf "$engine"
    "$root/scripts/build-yue-runtime.sh" "$engine"
    echo "$commit" > "$engine/runtime.commit"
fi

midi="$root/desktop/src-tauri/resources/music-midi"
midi_commit="$(sed -n 's/.*"commit": "\(.*\)".*/\1/p' "$root/engines/music-midi-source.json")"
if [ ! -x "$midi/music-midi" ] || [ "$(cat "$midi/runtime.commit" 2>/dev/null)" != "$midi_commit" ]; then
    rm -rf "$midi"
    "$root/scripts/build-midi-runtime.sh" "$midi"
    echo "$midi_commit" > "$midi/runtime.commit"
fi

# the engines must find their libraries beside them, not in this Mac's build folder
for runtime in "$engine" "$midi"; do
    "$root/scripts/macos-relocate.sh" "$runtime"
    "$root/scripts/macos-check-portable.sh" "$runtime"
done

cd "$root"
npm --prefix app install
npm --prefix desktop install
cd desktop
npm exec tauri build -- --config src-tauri/tauri.kinetic-macos.conf.json --features kinetic --bundles app,dmg

ls -d src-tauri/target/release/bundle/macos/*.app
find src-tauri/target/release/bundle/dmg -name '*.dmg'
