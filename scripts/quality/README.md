# Quality comparison harness (A / B / B-replay / C)

Finds where Kinetic Studio's sound diverges from m-a-p's official YuE2 pipeline. One request, one
seed, four renders, then measurements and a listening sheet. The audit that motivated it is
`docs/kinetic-studio/QUALITY-AUDIT.md`.

| Output | What it is | Made by |
|---|---|---|
| **A** | Official YuE2 (PyTorch, BF16, YuE2-Vae, release defaults, no length cap) | `run_official.py` |
| **B** | Kinetic Studio's own path: the app's API, the Create page's audited defaults (the old 130 s ceiling; Auto is now the default) | `run_kinetic.py studio` |
| **B-full** | The same, with no length cap (`--duration 0`) | `run_kinetic.py studio --duration 0` |
| **B-replay** | A's own score and semantic tokens re-rendered by the app's engine (yue2.cpp, Q8_0, Metal) | `run_kinetic.py replay` |
| **C** | B's library song exported as an Ableton project and rendered by Live | `check_ableton.py` |

Why B-replay exists: yue2.cpp draws tokens with Philox, PyTorch with its own sampler, so the
same seed does not give the same song in A and B. Seeds make each side reproducible, not equal.
B-replay holds the composition fixed (A's exact tokens, A's seed for the acoustic noise, the
reference 32 midpoint steps, no decoder companion), so **A vs B-replay isolates the acoustic
stage, the VAE and the Q8_0 quantisation**, and **B-replay vs B isolates everything the studio
and the semantic sampler add**. Compare distributions, not single takes: run three seeds when
time allows.

## Requests

- `requests/t1-city-lights.json`: m-a-p's own example, unchanged.
- `requests/t2-analog-electronica.json`: the Analog Electronica preset exactly as Kinetic's
  prompt builder compiles it for "Use in form".

## Steps on the Mac

Everything below writes under `runs/`. Use a fresh folder per run.

1. **A, official** (Python 3.12; downloads about 7.7 GB from Hugging Face the first time):

   ```bash
   python3.12 -m venv ~/yue2-official && source ~/yue2-official/bin/activate
   pip install "git+https://github.com/jaredlaham/YuE" numpy
   python run_official.py --request requests/t1-city-lights.json --device mps --output runs/A-t1
   ```

   If MPS runs out of memory or fails on BF16, stop there: A then needs a CUDA GPU with 24 GB
   (`--device cuda`), which costs money and is a separate decision.

2. **B and B-full** (Kinetic Studio open, engine ready, Q8_0 set, nothing else generating):

   ```bash
   python3 run_kinetic.py studio --request requests/t1-city-lights.json --output runs/B-t1
   python3 run_kinetic.py studio --request requests/t1-city-lights.json --duration 0 --output runs/B-t1-full
   ```

3. **B-replay** (needs A's folder and numpy):

   ```bash
   python3 run_kinetic.py replay --request requests/t1-city-lights.json --from runs/A-t1 --output runs/Breplay-t1
   ```

4. **Measure**:

   ```bash
   python3 analyze.py runs/A-t1/audio.flac runs/Breplay-t1/audio.wav runs/B-t1/audio-0.flac runs/B-t1-full/audio-0.flac \
       --labels A B-replay B B-full --reference A --out runs/report-t1
   ```

5. **C, Ableton**: in Kinetic Studio, Export → Ableton project for the B-t1 song. The master
   plays on an active track and any stems sit on tracks that are switched off, so Live's render
   is the master alone. Open the `.als` in Live 12, leave
   everything as it is, File → Export Audio/Video: Rendered Track Main, normalise off, dither
   off, 48 kHz, 32-bit float WAV. Then:

   ```bash
   python3 check_ableton.py "runs/B-t1 Project.zip" --master runs/B-t1/audio-0.flac --live-render "runs/Live-t1.wav" --out runs/report-t1/ableton.json
   ```

Repeat 1–5 with `t2-analog-electronica.json`.

## Speed benchmark

`bench_speed.py` times a 60-second and a full-length instrumental in Studio Quality, re-renders
each from its own tokens with the faster acoustic settings, compares their sound with the
Studio Quality master, and times an Ableton export cold and warm. See
`docs/kinetic-studio/SPEED.md`.

```bash
python3 bench_speed.py --output runs/speed-1 --extended
```

## Reading the numbers

- **Null test** (`analyze.py --reference`): only meaningful where the material is the same,
  i.e. A vs B-replay and B vs C. Under -90 dB is the same audio; -40 to -60 dB is the same
  music with numerical drift (quantisation); above -20 dB is different audio.
- **Energy above 16 kHz, 95% roll-off**: bandwidth. A codec or a low-pass shows here first.
- **L/R correlation, side/mid per band**: stereo width. Higher correlation is narrower.
- **Short-term spread, energy moves per minute, loudness range**: macro dynamics and how often
  the arrangement changes level.
- **Sections found, boundaries**: points where the timbre changes (Foote novelty on a mel
  spectrogram, 8 s kernel). A rough proxy for arrangement development, not a form analysis.
- **Ends abruptly**: the last second is as loud as the body and does not fade, the signature
  of a song cut off by a token budget rather than ended by the model.

## Listening sheet

Blind where possible (have someone rename the files X, Y, Z), headphones, loudness matched to
the quietest file. Score each 1–5 and write one line why:

| | Arrangement (development, transitions) | Timbre (instruments integrated or layered) | Stereo image | Dynamics | Structure (form, ending) | Fidelity (artefacts, air, low end) |
|---|---|---|---|---|---|---|
| X | | | | | | |
| Y | | | | | | |
| Z | | | | | | |

Where scores split, note the timestamp. A vs B-replay disagreeing points at the engine or
quantisation; B-replay vs B disagreeing points at the studio's request path or the sampler;
B vs C disagreeing points at the export.
