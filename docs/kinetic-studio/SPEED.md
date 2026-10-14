# Generation speed: where the wait goes, and what changed

Goal: the shortest wait from pressing Create to hearing a finished song, without giving up
sound. Ableton export is secondary and must never hold up the first playback.

Every number here is measured or comes from the engine's own logs. Where a number needs
Jared's Mac, this says so; `scripts/quality/bench_speed.py` measures it.

## 1. What runs when you press Create

Traced through the code (`crates/music-server/src/lib.rs`, `follow_job`,
`import_completed_result`, `after_import`):

| Stage | Runs at Create? | Before the song is playable? |
|---|---|---|
| Prompt and score planning (Kinetic prompt panel, `/v1/scores`) | Only if you plan a score first | Yes, it is a separate click before Create |
| Instrumental transfer (`/v1/score/instrumental`) | Only for instrumentals planned with a score | Yes, also before Create |
| Load models | Yes, when not already resident | Yes |
| Score (ABC, AR half) | Yes, unless the request carries a score | Yes |
| Semantic tokens (AR half) | Yes | Yes |
| Acoustic synthesis (NAR) | Yes | Yes |
| VAE decode | Yes | Yes |
| Import: fetch result, encode FLAC, add to library, tag | Yes | Yes |
| Cover art | Yes, but only with an OpenRouter key | No, after import, in the background |
| Lyric timing for karaoke | Only if karaoke is enabled and the song has sung lines | No, after import, in the background |
| Stems | No, on request | No |
| MIDI transcription | No, on request | No |
| Beat and tempo analysis | No, on request (Ableton export, when the song has no score) | No |
| Mastering | No, on request | No |
| Ableton packaging | No, on request | No |

**Ableton export never slowed generation.** Before this change it was already a button that
packaged what the song had; it ran nothing at Create. Stems and MIDI were also already
user-initiated. Generate was already master-only; this change keeps it so and makes the
export a proper on-demand job.

## 2. The measured baseline

One run on Jared's MacBook Pro, M1 Pro, 16 GB, Q8_0 set, every stage on Metal (`MTL0`),
32 midpoint steps. A 5:12 song took 33.6 minutes:

| Stage | Time | Share |
|---|---|---|
| Load the language model | 3.3 s | |
| Score, 2,351 tokens at 18 ms | 42 s | 2 % |
| Semantic, 7,805 tokens at 20 ms | 157 s | 8 % |
| Acoustic (NAR), 32 midpoint steps = 64 passes | 1,734 s | 86 % |
| VAE decode, 16 tiles | 77 s | 4 % |

No 60-second baseline exists yet. That is the first thing the benchmark measures.

## 3. Apple Silicon findings

- **The acoustic stage is the only stage worth attacking.** It is 86 % of the measured wait.
  Each of the 64 passes takes about 27 s on the M1 Pro for a 5:12 song, and each attends over
  the whole song plus the prompt, so its cost grows faster than the song's length.
- **Metal is already used for every stage**, with flash attention in the NAR. There is no CPU
  fallback to remove.
- **The solver is the lever.** AB2 makes S+1 network passes against 2S for midpoint: 33
  instead of 64 at 32 steps, about half the acoustic time. The engine's authors measured
  AB2 against midpoint at 16 steps (mel correlation 0.9996). Nobody has compared AB2-32
  with midpoint-32 by ear on this build yet. That is why it is the Fast Draft setting, not the
  default.
- **Fewer steps (16)** halve the acoustic time again but have never been checked against 32.
  They are left out of both modes until the benchmark and a listen say otherwise.
- **Model loading and eviction cost seconds, not minutes.** The language model loaded in
  3.3 s in the measured run. Keeping every stage resident would save a few seconds of a
  33-minute song and risks swap on 16 GB, so stage eviction stays. The benchmark logs each
  load, so the Mac's own numbers will confirm this.
- **KV cache and AR throughput are already in good shape.** The AR halves run at 18 to 20 ms
  a token with a KV cache, which is 10 % of the wait in total.
- **VAE decode is 4 %.** The tile size is a numerics question (see the quality audit, H5), not
  a speed one.
- **Upstream yue2.cpp has no speed work to take.** Beyond the pinned commit, upstream
  has a conv_1d graph fix for the VAE, a `--device` flag, song continuation and web UI
  latents. None of them changes speed in this pipeline, and the ggml fork is already at 0.26.0.
- **Token reuse already exists.** Replay re-renders a song from its own score and semantic
  tokens with the same noise seed, skipping the 10 % AR part. This change adds the solver to
  that path, so a Fast Draft can be rendered again in Studio Quality.
- **GPU utilisation cannot be read by an app.** macOS reports it only to processes with
  admin rights (`powermetrics`). The diagnostics say so, and they show memory, swap and
  engine CPU instead.

## 4. What changed

- **Generate stays master-only.** Nothing new runs at Create, and the export waits while a
  song is being generated.
- **Ableton export is an on-demand job** (`POST /v1/kinetic/ableton/jobs`, then poll
  `GET /v1/kinetic/ableton/jobs/{id}`). Its stages are: master, stems, score MIDI,
  transcription, tempo, package. Each stage reuses what the song has, makes only what is
  missing and asked for, and reports its state, progress and time. The set always has the
  master on an active track. Stems are on tracks that are switched off. Playback is never
  touched. This fixes D5.
- **Two render modes on the Create page.**
  - **Studio Quality** (default): 32 midpoint steps, no length ceiling ("Auto"), the official
    instrumental workflow and the native stereo master.
  - **Fast Draft**: AB2 at 32 steps, labelled as not yet listening-tested.
  - Any other combination shows as Custom. A draft can be re-rendered in Studio Quality from
    its tokens. The app says this gives the same composition and seed, not a new song, and
    not a bit-identical copy of a fresh generation.
- **Length is Auto by default** (D1). The model ends the song, up to 6:00. A longer song
  takes longer to make; that is the cost of not cutting the music off.
- **Instrumentals use the official workflow by default** (D2): "Compose an instrumental
  score" is the primary button.
- **Truncation is recorded** (D4). A song cut at the length limit is marked as cut, and the
  song panel says so.
- **Lyric timing is off by default**, as in the reference pipeline.
- **Performance diagnostics on every new song**, folded away under the song's details. It
  shows:
  - time to playable and each stage's time and share
  - real-time factor, model set and compute backend
  - solver, steps and network passes
  - peak unified memory, engine memory and swap growth
  - engine CPU, and a note that GPU utilisation is not measurable

## 5. What needs Jared's Mac

The container that made these changes runs Linux with no access to the model weights, so
every speed number from it is synthetic. The benchmark runs against the app's own API and
records only what the app measured:

```bash
cd scripts/quality
pip install numpy && brew install ffmpeg      # once
python3 bench_speed.py --output runs/speed-1 --extended
```

Open Kinetic Studio, let the engine get ready, plug the Mac in and leave it alone while the
benchmark runs. Judging from the 5:12 run, it should take about an hour. That is an
estimate. The benchmark runs:

- a 60-second and a full-length instrumental (Analog Electronica preset, official workflow)
- re-renders of each from their own tokens: midpoint-32 as the control, AB2-32 (Fast
  Draft), and with `--extended` AB2-16 and midpoint-16 on the 60-second song
- Ableton export of the 60-second song, cold and warm

It writes `report.md` and `report.json` with every stage's time, passes, real-time factor,
memory, and a null test, log-spectral distance and mel correlation of each re-render against
the Studio Quality master. The WAVs are in the same folder for listening.

Decisions the results settle:

- whether Fast Draft (AB2-32) is close enough to keep as the draft setting
- whether 16 steps can ever be offered
- the real 60-second time to playable
