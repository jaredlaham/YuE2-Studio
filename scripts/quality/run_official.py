#!/usr/bin/env python3
"""Output A: one song from m-a-p's official YuE2 pipeline, nothing else in the way.

Runs `YuE2Pipeline` from the official repository (github.com/multimodal-art-projection/YuE,
mirrored read-only at jaredlaham/YuE) with its release defaults: BF16 backbone, the
listening decoder m-a-p/YuE2-Vae, the checkpoint's sampling presets, 32 midpoint ODE
steps, no duration cap. The run directory keeps what `save_artifacts()` writes
(audio.flac, score.abc, semantic.npy, latent.npy, config.json, result.json with the
`truncated` flags), so output B can be re-rendered from the same semantic tokens.

    pip install "git+https://github.com/jaredlaham/YuE"   # Python 3.12, torch with MPS or CUDA
    python run_official.py --request requests/t1-city-lights.json --output runs/A-t1

On an Apple Silicon Mac the device is `mps`; on Linux, `cuda` (24 GB, BF16).
"""

import argparse
import json
import sys
import time
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--request", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True, help="a fresh directory")
    parser.add_argument("--device", default="auto", help="auto, mps, cuda or cpu")
    parser.add_argument("--model", default="m-a-p/YuE2-3B")
    parser.add_argument("--vae", default="m-a-p/YuE2-Vae")
    parser.add_argument("--revision")
    parser.add_argument("--vae-revision")
    parser.add_argument("--abc-file", type=Path, help="render this score instead of planning one")
    parser.add_argument("--local-files-only", action="store_true")
    args = parser.parse_args()
    if args.output.exists() and any(args.output.iterdir()):
        parser.error("choose a fresh output directory, so every run is kept")

    spec = json.loads(args.request.read_text(encoding="utf-8"))
    request = {key: spec[key] for key in ("style", "lyrics", "cot", "seed") if key in spec}
    request["id"] = spec.get("id", "song")
    if args.abc_file:
        request["abc"] = args.abc_file.read_text(encoding="utf-8")

    from yue2 import YuE2Pipeline  # imported late so --help works without torch

    started = time.time()
    with YuE2Pipeline.from_pretrained(args.model, vae=args.vae, revision=args.revision,
                                      vae_revision=args.vae_revision, device=args.device,
                                      local_files_only=args.local_files_only) as pipe:
        song = pipe(**request)
        result = song.save_artifacts(args.output)
    summary = {
        "output": "A",
        "pipeline": "official yue2 (PyTorch)",
        "request": request,
        "audio": str(args.output / "audio.flac"),
        "audio_seconds": result["audio_seconds"],
        "truncated": result["truncated"],
        "wall_seconds": round(time.time() - started, 1),
        "weights": result["weights"],
    }
    (args.output / "harness.json").write_text(json.dumps(summary, indent=2), encoding="utf-8")
    print(json.dumps(summary, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
