#!/usr/bin/env python3
"""Outputs B and B-replay: the same request through Kinetic Studio.

`studio`  submits the request to the running app's own API (127.0.0.1:8791), the path the
          Create page uses, so every studio-side default and rewrite applies. By default it
          sends the Create page's 130 s ceiling, as a new prompt does; `--duration 0` leaves
          the length to the model (the official behaviour). The job record it saves holds
          `generation_settings`, the exact body the studio sent to the engine.

`replay`  sends output A's own score and semantic tokens, with A's seed, straight to the
          app's engine (yue-server, 127.0.0.1:18087) at the reference settings (midpoint,
          32 steps, no companion). The composition is then identical to A by construction,
          so any difference from A is the acoustic stage, the VAE or the quantisation alone.

    python run_kinetic.py studio --request requests/t1-city-lights.json --output runs/B-t1
    python run_kinetic.py studio --request requests/t1-city-lights.json --duration 0 --output runs/B-t1-full
    python run_kinetic.py replay --request requests/t1-city-lights.json --from runs/A-t1 --output runs/Breplay-t1

Standard library only, except `replay`, which reads A's semantic.npy with numpy.
"""

import argparse
import email.parser
import email.policy
import json
import sys
import time
import urllib.request
from pathlib import Path

STUDIO = "http://127.0.0.1:8791"
ENGINE = "http://127.0.0.1:18087"


def call(method: str, url: str, body=None, timeout=60):
    data = json.dumps(body).encode() if body is not None else None
    request = urllib.request.Request(url, data=data, method=method, headers={"Content-Type": "application/json"} if data else {})
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return response.headers.get("Content-Type", ""), response.read()


def fresh(path: Path) -> Path:
    if path.exists() and any(path.iterdir()):
        sys.exit(f"{path} is not empty: choose a fresh output directory, so every run is kept")
    path.mkdir(parents=True, exist_ok=True)
    return path


def studio(args, spec):
    out = fresh(args.output)
    body = {
        "style": spec["style"],
        "lyrics": spec["lyrics"],
        "cot": spec.get("cot", "full"),
        "lm_seed": spec["seed"],
        "seed": spec["seed"],
        "output_format": args.format,
        "title": f"quality {spec.get('id', 'song')}",
    }
    if args.duration > 0:
        body["duration_seconds"] = args.duration
    started = time.time()
    _, raw = call("POST", f"{args.studio}/v1/music/jobs", body)
    job = json.loads(raw)
    print(f"job {job.get('id')} {job.get('status')}: {job.get('message', '')}", flush=True)
    while job.get("status") in ("queued", "running"):
        time.sleep(10)
        _, raw = call("GET", f"{args.studio}/v1/music/jobs/{job['id']}")
        job = json.loads(raw)
        print(f"  {int(time.time() - started)} s  {job.get('phase')}  {job.get('message', '')}", flush=True)
    (out / "job.json").write_text(json.dumps(job, indent=2), encoding="utf-8")
    if job.get("status") != "completed":
        sys.exit(f"the job ended {job.get('status')}: {job.get('message')}")
    songs = job.get("songs") or ([job["song"]] if job.get("song") else [])
    files = []
    for index, made in enumerate(songs):
        url = made["audio_url"] if made["audio_url"].startswith("http") else f"{args.studio}{made['audio_url']}"
        _, audio = call("GET", url, timeout=600)
        suffix = Path(url.split("?")[0]).suffix or f".{args.format}"
        target = out / f"audio-{index}{suffix}"
        target.write_bytes(audio)
        files.append({"file": target.name, "song_id": made["id"]})
    summary = {"output": "B", "pipeline": "Kinetic Studio API", "sent_to_studio": body,
               "sent_to_engine": job.get("generation_settings"), "files": files,
               "wall_seconds": round(time.time() - started, 1)}
    (out / "harness.json").write_text(json.dumps(summary, indent=2), encoding="utf-8")
    print(json.dumps(summary, indent=2))


def replay(args, spec):
    import numpy as np

    out = fresh(args.output)
    source = args.source
    tokens = np.load(source / "semantic.npy").astype(int).tolist()
    score = (source / "score.abc").read_text(encoding="utf-8") if (source / "score.abc").exists() else ""
    body = {
        "style": spec["style"],
        "lyrics": spec["lyrics"],
        "cot": spec.get("cot", "full"),
        "seed": spec["seed"],
        "lm_seed": spec["seed"],
        "semantic_tokens": ",".join(str(value) for value in tokens),
        "steps": 32,
        "solver": "midpoint",
        "companion_scale": 0,
        "output_format": "wav32",
    }
    if score:
        body["abc"] = score
    started = time.time()
    _, raw = call("POST", f"{args.engine}/synth", body)
    job_id = json.loads(raw)["id"]
    while True:
        time.sleep(5)
        _, raw = call("GET", f"{args.engine}/job?id={job_id}")
        status = json.loads(raw)["status"]
        print(f"  {int(time.time() - started)} s  {status}", flush=True)
        if status != "running":
            break
    if status != "done":
        sys.exit(f"the engine job ended {status}")
    content_type, payload = call("GET", f"{args.engine}/job?id={job_id}&result=1", timeout=600)
    message = email.parser.BytesParser(policy=email.policy.default).parsebytes(
        f"Content-Type: {content_type}\r\n\r\n".encode() + payload)
    parts = list(message.iter_parts())
    (out / "replay-request.json").write_bytes(parts[0].get_payload(decode=True))
    (out / "audio.wav").write_bytes(parts[1].get_payload(decode=True))
    summary = {"output": "B-replay", "pipeline": "yue-server, A's score and semantic tokens",
               "frames": len(tokens), "sent_to_engine": {k: v for k, v in body.items() if k != "semantic_tokens"},
               "wall_seconds": round(time.time() - started, 1)}
    (out / "harness.json").write_text(json.dumps(summary, indent=2), encoding="utf-8")
    print(json.dumps(summary, indent=2))


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="mode", required=True)
    one = sub.add_parser("studio")
    one.add_argument("--duration", type=float, default=130.0, help="seconds; 0 sends none (the model decides)")
    one.add_argument("--format", default="flac", choices=("flac", "wav32", "mp3"))
    one.add_argument("--studio", default=STUDIO)
    two = sub.add_parser("replay")
    two.add_argument("--from", dest="source", type=Path, required=True, help="output A's run directory")
    two.add_argument("--engine", default=ENGINE)
    for each in (one, two):
        each.add_argument("--request", type=Path, required=True)
        each.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    spec = json.loads(args.request.read_text(encoding="utf-8"))
    (studio if args.mode == "studio" else replay)(args, spec)


if __name__ == "__main__":
    main()
