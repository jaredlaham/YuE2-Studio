#!/usr/bin/env python3
"""Generation speed benchmark: how long until a song is playable, stage by stage, and what
the faster acoustic settings cost in sound. Runs against Kinetic Studio's own API on the Mac.

What it does, for a 60-second and a full-length instrumental (Analog Electronica preset):

1. Studio Quality, the official instrumental workflow: plan a score, move its sung notes to
   the instrument voice, render it (32 midpoint steps, no length ceiling for the full-length
   song). The planning call and the render are timed separately and together.
2. Re-renders of that song from its own semantic tokens and noise seed, with the acoustic
   settings to compare: midpoint 32 (the control: the same render again), AB2 32 (Fast
   Draft), and with --extended AB2 16 and midpoint 16. Same composition, so a null test and
   log-spectral distance against the Studio Quality master measure only what the solver
   changed.
3. Ableton export of the 60-second song twice: cold (stems separated if the separator is
   installed), then warm (everything reused).

Every number comes from the app: the per-stage timings the engine logged and the studio's
own clock (the song's `performance` record). Nothing is estimated.

    python3 bench_speed.py --output runs/speed-1
    python3 bench_speed.py --output runs/speed-1 --only 60 --extended

Needs Kinetic Studio open with the engine ready, nothing else generating, numpy and ffmpeg
(for the comparisons; `pip install numpy`, `brew install ffmpeg`). Plug the Mac in and leave
it alone while it runs: other work changes the timings.
"""

import argparse
import json
import platform
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

STUDIO = "http://127.0.0.1:8791"
HERE = Path(__file__).parent
PRESET = json.loads((HERE / "requests" / "t2-analog-electronica.json").read_text())

CASES = {
    "60": {
        "label": "60-second instrumental",
        "style": PRESET["style"],
        "lyrics": "[Intro]\n\n[Theme]\n\n[Outro]",
        "duration": 60,
        "seed": 831001,
    },
    "full": {
        "label": "full-length instrumental",
        "style": PRESET["style"],
        "lyrics": PRESET["lyrics"],
        "duration": None,  # Auto: the model ends the song, up to 6:00
        "seed": 831001,
    },
}

# (name, steps, solver)
RENDERS = [("midpoint-32 (control)", 32, "midpoint"), ("ab2-32 (Fast Draft)", 32, "ab2")]
EXTENDED = [("ab2-16", 16, "ab2"), ("midpoint-16", 16, "midpoint")]


def call(method, path, body=None, timeout=120, raw=False):
    data = json.dumps(body).encode() if body is not None else None
    request = urllib.request.Request(STUDIO + path, data=data, method=method, headers={"Content-Type": "application/json"} if data else {})
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            payload = response.read()
    except urllib.error.HTTPError as error:
        sys.exit(f"{method} {path}: HTTP {error.code}: {error.read().decode(errors='replace')[:400]}")
    return payload if raw else json.loads(payload or b"{}")


def say(text):
    print(time.strftime("%H:%M:%S"), text, flush=True)


def wait_score(job_id):
    while True:
        time.sleep(1)
        state = call("GET", f"/v1/scores/{job_id}")
        if state.get("status") == "done" and state.get("abc"):
            return state["abc"]
        if state.get("status") in ("failed", "cancelled"):
            sys.exit(f"the score job {state.get('status')}: {state.get('error')}")


def wait_song(job_id):
    while True:
        time.sleep(2)
        job = call("GET", f"/v1/music/jobs/{job_id}")
        if job["status"] == "completed":
            return job["song"]["song"]
        if job["status"] in ("failed", "cancelled"):
            sys.exit(f"the job {job['status']}: {job.get('message')}")


def answer(path, body):
    reply = call("POST", path, body)
    if not reply.get("ok", True):
        sys.exit(f"{path}: {reply.get('error')}")
    return reply


def studio_quality(case):
    """The official instrumental workflow, timed: plan, transfer, render."""
    clock = time.monotonic()
    job = call("POST", "/v1/scores", {"style": case["style"], "lyrics": case["lyrics"], "cot": "full", "lm_seed": case["seed"]})
    planned = wait_score(job["id"])
    moved = call("POST", "/v1/score/instrumental", {"abc": planned})
    if not moved.get("ok", True):
        if "instrumental already" not in (moved.get("error") or ""):
            sys.exit(f"/v1/score/instrumental: {moved.get('error')}")
        moved = {"abc": planned}  # nothing was sung: the plan is instrumental as it is
    marked = answer("/v1/score/mark", {"abc": moved["abc"], "style": case["style"], "lyrics": case["lyrics"], "cot": "full", "keep": False})
    planning_s = time.monotonic() - clock
    body = {
        "style": case["style"], "lyrics": case["lyrics"], "cot": "full", "abc": marked["abc"],
        "lyric_timing": False, "lm_seed": case["seed"], "seed": case["seed"], "output_format": "flac",
        "title": f"speed bench {case['label']}",
    }
    if case["duration"]:
        body["duration_seconds"] = case["duration"]
    job = call("POST", "/v1/music/jobs", body)
    song = wait_song(job["id"])
    return song, planning_s, time.monotonic() - clock


def rerender(song_id, steps, solver):
    job = call("POST", "/v1/music/replay", {"song_id": song_id, "steps": steps, "solver": solver, "output_format": "flac"})
    return wait_song(job["id"])


def download(song, folder, name):
    path = folder / f"{name}.flac"
    path.write_bytes(call("GET", f"/v1/library/media/{song['id']}", raw=True, timeout=600))
    return path


def compare(reference, test):
    """Null test, log-spectral distance and mel correlation of test against reference."""
    try:
        sys.path.insert(0, str(HERE))
        import numpy as np
        from analyze import decode, mel_bank, null_test, spectrogram
    except Exception as error:  # numpy or ffmpeg missing: timings still stand
        return {"skipped": f"comparison needs numpy and ffmpeg: {error}"}
    result = null_test(reference, test)
    a, b = decode(reference).mean(axis=1), decode(test).mean(axis=1)
    n = min(len(a), len(b))
    sa, sb = spectrogram(a[:n]), spectrogram(b[:n])
    bank = mel_bank(sa.shape[1], 4096)
    ma, mb = np.log10(sa @ bank.T + 1e-10).ravel(), np.log10(sb @ bank.T + 1e-10).ravel()
    result["mel_correlation"] = round(float(np.corrcoef(ma, mb)[0, 1]), 5)
    return result


def stages(song):
    perf = song["metadata"].get("performance") or {}
    engine = perf.get("engine") or {}
    seconds = lambda ms: round(ms / 1000, 1) if isinstance(ms, (int, float)) else None
    return {
        "time_to_playable_s": seconds(perf.get("time_to_playable_ms")),
        "queued_s": seconds(perf.get("queued_ms")),
        "load_s": seconds(engine.get("load_ms")),
        "score_s": seconds(engine.get("score_ms")),
        "semantic_s": seconds(engine.get("semantic_ms")),
        "acoustic_s": seconds(engine.get("acoustic_ms")),
        "acoustic_wall_s": seconds(engine.get("acoustic_wall_ms")),
        "decode_s": seconds(engine.get("decode_ms")),
        "import_s": seconds(perf.get("import_ms")),
        "engine_total_s": seconds(engine.get("engine_total_ms")),
        "audio_s": perf.get("audio_seconds"),
        "realtime_factor": perf.get("realtime_factor"),
        "solver": perf.get("solver"),
        "steps": perf.get("steps"),
        "passes": engine.get("evaluations"),
        "backend": engine.get("backend"),
        "truncated": song["metadata"].get("truncated", False),
        "memory_peak_gb": round(perf["resources"]["system_used_peak_mb"] / 1024, 2) if perf.get("resources") else None,
        "engine_rss_peak_gb": round(perf["resources"]["engine_rss_peak_mb"] / 1024, 2) if (perf.get("resources") or {}).get("engine_rss_peak_mb") else None,
        "swap_growth_mb": (perf["resources"]["swap_peak_mb"] - perf["resources"]["swap_start_mb"]) if perf.get("resources") else None,
    }


def export(song_id, stems):
    clock = time.monotonic()
    job = call("POST", "/v1/kinetic/ableton/jobs", {"song_id": song_id, "stems": stems})
    while not job["done"]:
        time.sleep(1)
        job = call("GET", f"/v1/kinetic/ableton/jobs/{job['id']}")
    return {
        "wall_s": round(time.monotonic() - clock, 1),
        "error": job.get("error"),
        "stages": {stage["id"]: {"state": stage["state"], "s": round(stage.get("ms", 0) / 1000, 2), "detail": stage["detail"]} for stage in job["stages"]},
        "tracks": (job.get("project") or {}).get("tracks"),
    }


def markdown(report):
    lines = [f"# Kinetic Studio speed benchmark, {report['started']}", "", f"Machine: {report['machine']}", ""]
    for key, case in report["cases"].items():
        lines += [f"## {case['label']}", "", f"Planning (score + instrumental transfer): {case['planning_s']:.1f} s. "
                  f"Generate pressed to playable, planning included: {case['studio_total_s']:.1f} s.", ""]
        header = "| Render | Playable after | Score | Semantic | Acoustic | Decode | Import | Passes | Song | RTF | Peak memory | Swap growth | Null vs Studio | LSD | Mel corr |"
        lines += [header, "|" + "---|" * (header.count("|") - 1)]
        for name, row in case["renders"].items():
            t, q = row["stages"], row.get("vs_studio") or {}
            cell = lambda v, unit="": "" if v is None else f"{v}{unit}"
            lines.append(f"| {name} | {cell(t['time_to_playable_s'], ' s')} | {cell(t['score_s'], ' s')} | {cell(t['semantic_s'], ' s')} | "
                         f"{cell(t['acoustic_s'], ' s')} | {cell(t['decode_s'], ' s')} | {cell(t['import_s'], ' s')} | {cell(t['passes'])} | "
                         f"{cell(t['audio_s'], ' s')} | {cell(round(t['realtime_factor'], 3) if t['realtime_factor'] else None)} | "
                         f"{cell(t['memory_peak_gb'], ' GB')} | {cell(t['swap_growth_mb'], ' MB')} | "
                         f"{cell(q.get('residual_db_rel_ref'), ' dB')} | {cell(q.get('log_spectral_distance_db'), ' dB')} | {cell(q.get('mel_correlation'))} |")
        lines.append("")
    if report.get("export"):
        lines += ["## Ableton export (60-second song)", ""]
        for name, run in report["export"].items():
            lines.append(f"- {name}: {run['wall_s']} s; " + "; ".join(f"{stage} {info['state']} {info['s']} s" for stage, info in run["stages"].items()))
        lines.append("")
    lines += ["Null test: under -90 dB is the same audio; -40 to -60 dB is the same music with numerical drift; above -20 dB is different audio.",
              "LSD: log-spectral distance in dB, 0 is identical. Listen before deciding: the numbers locate a difference, they do not judge it."]
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--only", choices=sorted(CASES), help="run one case")
    parser.add_argument("--extended", action="store_true", help="also AB2 16 and midpoint 16 (60-second case)")
    parser.add_argument("--no-export", action="store_true")
    args = parser.parse_args()
    if args.output.exists() and any(args.output.iterdir()):
        sys.exit(f"{args.output} is not empty: use a fresh folder so every run is kept")
    args.output.mkdir(parents=True, exist_ok=True)
    try:
        call("GET", "/health")
    except Exception as error:
        sys.exit(f"Kinetic Studio is not answering on {STUDIO}: {error}")
    machine = platform.platform()
    if sys.platform == "darwin":
        chip = subprocess.run(["sysctl", "-n", "machdep.cpu.brand_string"], capture_output=True, text=True).stdout.strip()
        memory = int(subprocess.run(["sysctl", "-n", "hw.memsize"], capture_output=True, text=True).stdout.strip() or 0) / 2**30
        machine = f"{chip}, {memory:.0f} GB, {machine}"
    report = {"started": time.strftime("%Y-%m-%d %H:%M"), "machine": machine, "cases": {}}

    for key in ([args.only] if args.only else ["60", "full"]):
        case = CASES[key]
        folder = args.output / key
        folder.mkdir()
        say(f"{case['label']}: Studio Quality (plan, transfer, render)")
        song, planning_s, total_s = studio_quality(case)
        master = download(song, folder, "studio")
        renders = {"Studio Quality": {"song_id": song["id"], "stages": stages(song)}}
        plan = RENDERS + (EXTENDED if args.extended and key == "60" else [])
        if key == "full":
            plan = [render for render in plan if "control" not in render[0]]  # the control repeats a long render
        for name, steps, solver in plan:
            say(f"{case['label']}: re-render {name}")
            made = rerender(song["id"], steps, solver)
            path = download(made, folder, name.split(" ")[0])
            renders[name] = {"song_id": made["id"], "stages": stages(made), "vs_studio": compare(master, path)}
        report["cases"][key] = {"label": case["label"], "planning_s": round(planning_s, 1), "studio_total_s": round(total_s, 1), "renders": renders}
        (args.output / "report.json").write_text(json.dumps(report, indent=1))
        if key == "60" and not args.no_export:
            say("Ableton export, cold then warm")
            report["export"] = {"cold": export(song["id"], True), "warm (reused)": export(song["id"], True)}
            (args.output / "report.json").write_text(json.dumps(report, indent=1))

    (args.output / "report.md").write_text(markdown(report))
    say(f"done: {args.output / 'report.md'}")
    print((args.output / "report.md").read_text())


if __name__ == "__main__":
    main()
