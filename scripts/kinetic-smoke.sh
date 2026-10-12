#!/bin/bash
# Phase 1 smoke test for Kinetic Studio, run against the real desktop binary:
# launch it, import a 48 kHz / 24-bit WAV through the service the window uses,
# check the library lists it and serves the same bytes for playback, quit,
# launch again and check the song is still there.
#
# Usage: scripts/kinetic-smoke.sh <path to the studio executable> [expected bundle name]
# The data folder is wherever the build keeps it (Application Support on
# macOS); the test removes only the song it imported.
set -euo pipefail

binary="${1:?usage: kinetic-smoke.sh STUDIO_EXECUTABLE [EXPECTED_IN_PATH]}"
expected="${2:-}"
base="http://127.0.0.1:${YUE_STUDIO_PORT:-8791}"
work="$(mktemp -d)"
pid=""

stop() {
    if [ -n "$pid" ]; then
        kill "$pid" 2>/dev/null || true
        for _ in $(seq 1 20); do kill -0 "$pid" 2>/dev/null || break; sleep 0.5; done
        kill -9 "$pid" 2>/dev/null || true
        pid=""
    fi
    # the service lets go of its port a moment after the process ends
    for _ in $(seq 1 20); do curl -fsS "$base/health" >/dev/null 2>&1 || break; sleep 0.5; done
}
trap 'stop; rm -rf "$work"' EXIT

launch() {
    if curl -fsS "$base/health" >/dev/null 2>&1; then
        echo "something already answers on $base; close it first" >&2
        exit 1
    fi
    "$binary" >> "$work/launch.log" 2>&1 &
    pid=$!
    for _ in $(seq 1 120); do
        if curl -fsS "$base/health" > "$work/health.json" 2>/dev/null; then return 0; fi
        if ! kill -0 "$pid" 2>/dev/null; then break; fi
        sleep 1
    done
    echo "the studio's service never answered" >&2
    cat "$work/launch.log" >&2
    exit 1
}

# One second of a 440 Hz tone, 48 kHz, 24-bit stereo, the format YuE2 renders.
python3 - "$work/tone.wav" <<'PY'
import math, struct, sys, wave
with wave.open(sys.argv[1], "wb") as out:
    out.setnchannels(2); out.setsampwidth(3); out.setframerate(48000)
    frames = bytearray()
    for i in range(48000):
        value = int(0.25 * math.sin(2 * math.pi * 440 * i / 48000) * 8388607)
        sample = struct.pack("<i", value)[:3]
        frames += sample + sample
    out.writeframes(bytes(frames))
PY
original_sha="$(shasum -a 256 "$work/tone.wav" 2>/dev/null | cut -d' ' -f1 || sha256sum "$work/tone.wav" | cut -d' ' -f1)"

launch
python3 - "$work/health.json" "$expected" <<'PY'
import json, sys
health = json.load(open(sys.argv[1]))
assert health["status"] == "ok", health
if sys.argv[2]:
    assert sys.argv[2] in (health.get("service_executable") or ""), health
print("launch: service answered from", health.get("service_executable"))
PY

curl -fsS -F "audio=@$work/tone.wav;filename=kinetic-smoke-tone.wav" -F "title=Kinetic smoke tone" \
    "$base/v1/library/import" > "$work/imported.json"
id="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["id"])' "$work/imported.json")"
echo "import: song $id"

curl -fsS "$base/v1/library/songs" > "$work/songs.json"
python3 - "$work/songs.json" "$id" <<'PY'
import json, sys
data = json.load(open(sys.argv[1]))
songs = data if isinstance(data, list) else data.get("songs", data.get("items", []))
assert any(song.get("id") == sys.argv[2] for song in songs), "imported song missing from the library"
print("library: lists the imported song")
PY

curl -fsS "$base/v1/library/media/$id" -o "$work/played.bin"
played_sha="$(shasum -a 256 "$work/played.bin" 2>/dev/null | cut -d' ' -f1 || sha256sum "$work/played.bin" | cut -d' ' -f1)"
if [ "$played_sha" != "$original_sha" ]; then
    echo "playback: the served audio differs from the imported file ($played_sha vs $original_sha)" >&2
    exit 1
fi
echo "playback: serves the imported bytes unchanged (sha256 $original_sha)"

stop
launch
curl -fsS "$base/v1/library/songs/$id" > "$work/again.json"
echo "persistence: song $id is still in the library after a restart"

curl -fsS -X DELETE "$base/v1/library/songs/$id" > /dev/null
echo "cleanup: removed the test song"
echo "kinetic smoke test passed"
