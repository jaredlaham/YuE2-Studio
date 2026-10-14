#!/usr/bin/env python3
"""Output C, checked: does the Ableton project carry the generated master unchanged?

Three questions, answered from the files:
  1. Is the project's Mix file the library's master, byte for byte (or sample for sample)?
  2. Would Live play it unchanged: on a track, warped in Re-Pitch at rate 1.0, track volume
     0 dB, pan centre, no devices on the track or the main track?
  3. With --live-render: does what Live actually exported null against the master?

    python check_ableton.py "Night drive Project.zip" --master "Night drive.flac"
    python check_ableton.py "Night drive Project.zip" --master master.flac --live-render "Live export.wav"

For 3, export from Live with File > Export Audio/Video: Rendered Track = Main, the clip's
length, normalise off, dither off, 48 kHz, 32-bit float or 24-bit WAV.
"""

import argparse
import gzip
import hashlib
import json
import sys
import tempfile
import xml.etree.ElementTree as ET
import zipfile
from pathlib import Path

WARP_MODES = {0: "Beats", 1: "Tones", 2: "Texture", 3: "Re-Pitch", 4: "Complex", 5: "REX", 6: "Complex Pro"}


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def value(node, path, default=None):
    found = node.find(path)
    return found.get("Value") if found is not None else default


def track_report(track, bpm: float) -> dict:
    name = value(track, "Name/UserName") or value(track, "Name/EffectiveName") or "?"
    mixer = track.find("DeviceChain/Mixer")
    volume = float(value(mixer, "Volume/Manual", "1")) if mixer is not None else None
    pan = float(value(mixer, "Pan/Manual", "0")) if mixer is not None else None
    devices = track.find("DeviceChain/DeviceChain/Devices")
    device_names = [child.tag for child in devices] if devices is not None else []
    clips = []
    for clip in track.iter("AudioClip"):
        markers = [(float(m.get("SecTime")), float(m.get("BeatTime"))) for m in clip.iter("WarpMarker")]
        rate = None
        if len(markers) >= 2 and markers[-1][0] > markers[0][0]:
            beats = markers[-1][1] - markers[0][1]
            rate = (beats * 60.0 / bpm) / (markers[-1][0] - markers[0][0])  # 1.0 = played at its own speed
        clips.append({
            "file": value(clip, ".//FileRef/RelativePath") or value(clip, ".//FileRef/Path"),
            "warped": value(clip, "IsWarped") == "true",
            "warp_mode": WARP_MODES.get(int(value(clip, "WarpMode", "-1")), value(clip, "WarpMode")),
            "playback_rate": round(rate, 6) if rate is not None else None,
            "clip_gain": float(value(clip, "SampleVolume", "1")),
            "pitch_coarse": float(value(clip, "PitchCoarse", "0")),
            "pitch_fine": float(value(clip, "PitchFine", "0")),
        })
    active = value(mixer, "Speaker/Manual", "true") == "true" if mixer is not None else True
    return {"track": name, "active": active, "volume_linear": volume, "pan": pan, "devices": device_names, "audio_clips": clips}


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("project", type=Path, help="the exported .zip or its unpacked folder")
    parser.add_argument("--master", type=Path, help="the song's audio file from the library")
    parser.add_argument("--live-render", type=Path, help="Live's own export of the set, for a null test")
    parser.add_argument("--out", type=Path)
    args = parser.parse_args()

    with tempfile.TemporaryDirectory() as scratch:
        root = args.project
        if root.suffix.lower() == ".zip":
            with zipfile.ZipFile(root) as archive:
                archive.extractall(scratch)
            root = Path(scratch)
        sets = list(root.rglob("*.als"))
        if len(sets) != 1:
            sys.exit(f"expected one .als in the project, found {len(sets)}")
        tree = ET.fromstring(gzip.decompress(sets[0].read_bytes()))
        live = tree.find("LiveSet")
        bpm = float(value(live, ".//MainTrack//Tempo/Manual") or value(live, ".//MasterTrack//Tempo/Manual") or 120)
        tracks = [track_report(t, bpm) for t in live.find("Tracks") if t.tag in ("AudioTrack", "MidiTrack")]
        main = live.find("MainTrack") if live.find("MainTrack") is not None else live.find("MasterTrack")
        main_report = track_report(main, bpm) if main is not None else None
        # "Master" since the generation-first export; "Mix" in projects made before it
        mixes = [p for p in root.rglob("*") if p.is_file() and (" - Master." in p.name or " - Mix." in p.name)]
        report = {"set": sets[0].name, "bpm": bpm, "tracks": tracks, "main": main_report, "checks": []}

        def check(name, ok, detail):
            report["checks"].append({"check": name, "ok": bool(ok), "detail": detail})

        mix = mixes[0] if mixes else None
        check("master file in project", mix is not None, mix.name if mix else "no '<title> - Master.*' file")
        if mix and args.master:
            same = sha(mix) == sha(args.master)
            detail = "byte-identical to the library master" if same else "bytes differ"
            if not same:
                sys.path.insert(0, str(Path(__file__).parent))
                from analyze import null_test
                result = null_test(args.master, mix)
                detail += f"; samples {'identical' if result['bit_identical'] else 'differ'} (residual {result['residual_db_rel_ref']} dB)"
                same = result["bit_identical"]
            check("mix equals master", same, detail)
        on_track = [t for t in tracks for c in t["audio_clips"] if c["file"] and mix and c["file"].endswith(mix.name)]
        check("master on a track", bool(on_track), "on track " + on_track[0]["track"] if on_track else
              "the master is only in the folder, not in the set (Kinetic left it off when stems existed, before the fix)")
        if on_track:
            check("master track is active", on_track[0]["active"], "plays" if on_track[0]["active"] else "switched off")
        stems = [t for t in tracks if "(separated)" in t["track"]]
        if stems:
            playing = [t["track"] for t in stems if t["active"]]
            check("stems are switched off", not playing, "all off" if not playing else "playing with the master: " + ", ".join(playing))
        for t in tracks:
            for c in t["audio_clips"]:
                unchanged = (c["warp_mode"] == "Re-Pitch" or not c["warped"]) and (c["playback_rate"] in (None, 1.0) or abs(c["playback_rate"] - 1) < 1e-4) \
                    and c["clip_gain"] == 1.0 and c["pitch_coarse"] == 0 and c["pitch_fine"] == 0
                check(f"{t['track']}: clip plays unchanged", unchanged,
                      f"warp {c['warp_mode']}, rate {c['playback_rate']}, clip gain {c['clip_gain']}, pitch {c['pitch_coarse']}/{c['pitch_fine']}")
            if t["audio_clips"]:
                check(f"{t['track']}: fader at 0 dB, centre, no devices", t["volume_linear"] == 1.0 and t["pan"] == 0 and not t["devices"],
                      f"volume {t['volume_linear']}, pan {t['pan']}, devices {t['devices']}")
        if main_report:
            check("main track at 0 dB, no devices", main_report["volume_linear"] == 1.0 and not main_report["devices"],
                  f"volume {main_report['volume_linear']}, devices {main_report['devices']}")
        if args.live_render and args.master:
            sys.path.insert(0, str(Path(__file__).parent))
            from analyze import null_test
            result = null_test(args.master, args.live_render)
            report["live_render_null_test"] = result
            check("Live's render nulls against the master", result["residual_db_rel_ref"] < -90,
                  f"residual {result['residual_db_rel_ref']} dB, lag {result['lag_samples']} samples, gain {result['gain_match_db']} dB")

    for item in report["checks"]:
        print(("PASS " if item["ok"] else "FAIL ") + item["check"] + " - " + item["detail"])
    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(json.dumps(report, indent=2), encoding="utf-8")


if __name__ == "__main__":
    main()
