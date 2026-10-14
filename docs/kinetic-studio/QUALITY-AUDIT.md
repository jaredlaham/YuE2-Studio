# Kinetic Studio: YuE2 architecture and audio-quality audit

Audited 2026-10-14 against:

- Kinetic Studio, `jaredlaham/YuE2-Studio` `main` at `c8f853b` (all Kinetic phases merged).
- Its engine, `timoncool/yue2.cpp` at `c8516db` (pinned in `engines/yue2-cpp-source.json`).
- The official YuE2 release, `multimodal-art-projection/YuE` (mirror `jaredlaham/YuE` at `1647252`), package `yue2` v0.1.6 protocol `yue2-native-v1`.

Every claim below is marked **Verified** (read in the code, with the file and line) or
**Hypothesis** (plausible, needs the listening comparison in `scripts/quality/`). No model was run
for this audit: the cloud container cannot reach Hugging Face, and no weights were downloaded.

---

## 1. Short answer

**Kinetic Studio runs the complete YuE2 neural pipeline. Nothing in the generation path is MIDI,
SoundFont or software-instrument synthesis, and the Ableton export does not touch generation.**
The song you hear is the model's own 48 kHz stereo audio: score → semantic tokens → flow-matching
acoustic latents → VAE, in a C++/Metal port with a quantised (Q8_0) backbone.

The export requirement did not compromise the architecture. What does depart from the official
recipe is a handful of **studio defaults that sit in front of the model**, and two of them affect
arrangement directly:

1. **Every new prompt is capped at 2:10.** The Create page sends `duration_seconds: 130` unless
   you change it, and the engine stops the semantic stage there. The model plans the whole song,
   but the audio is cut off at 130 s, without a fade and without any warning. The official
   pipeline lets the model end the song itself, up to 6:00. Kinetic's presets have five to seven
   sections and set no length, so they inherit the cap.
2. **The default instrumental path skips m-a-p's instrumental workflow.** "Use in form" sends
   section tags with no score, so the model plans a melody for a *voice* in a track meant to have
   none. The official workflow, which Kinetic also implements, moves every planned vocal note to
   the instrument voice first. That is the secondary button.

Beyond those, the studio adds a lyric-timing attention bias that the official pipeline does not
have, drops the engine's truncation flag, and leaves the original master off the Ableton set
whenever stems exist. The full list with file references is in section 5.

Part of the gap to Suno will remain after these fixes, and the audit should say so plainly:
m-a-p's "competitive with Suno v5/v6" result is for the unquantised BF16 model **with best-of-8
selection** (`YuE/README.md`, WildSongBench), on songs with vocals. Kinetic renders one take, on a
Q8_0 backbone, mostly instrumentals, from prompts that ask for restraint ("restrained sparse synth
melody", "sparse arrangement", "slowly unfolding"). Those are the next things to measure, not
defects.

---

## 2. Phase 1: the eight questions

| # | Question | Answer | Evidence |
|---|---|---|---|
| 1 | Exact model and checkpoint | **YuE2-3B** (3.6B AR–NAR Mixture-of-Transformers) as `YuE2-3B-Q8_0.gguf` from `Serveurperso/YuE2-GGUF` @ `e630f2b`, SHA-256 `41121ce9…`; VAE `YuE2-Vae-F32.gguf` converted from **m-a-p/YuE2-Vae** (the official listening decoder, not `-legacy`). BF16, Q6_K, Q5_K_M sets also offered. Your Mac runs the Q8_0 set. | `crates/music-server/src/model_manager.rs:16-17, 731-734, 773-778`; yue2.cpp `checkpoints.sh:4-7` |
| 2 | Official pipeline executed? | **Yes, as a faithful port, not the PyTorch code.** yue2.cpp reimplements the four stages and the checkpoint protocol (same token IDs, prompts, sampling presets, CFG rule, 32-step midpoint ODE, chunking, VAE). It ships parity harnesses against the reference modeling code. Not bit-identical by design: token sampling uses Philox, so a seed does not reproduce the PyTorch song. | yue2.cpp `src/sampling.h:46-47` (presets equal `yue2/protocol.py:46,51-58`), `src/request.cpp:24-45`, `src/flow-solver.h`, `docs/ARCHITECTURE.md` "Cosine similarity harness" |
| 3 | Symbolic, semantic, acoustic, VAE all running? | **Yes.** Your 5:12 engine log shows all four (score 2,351 tokens; semantic 7,805 tokens; NAR 32 midpoint steps; VAE 16 tiles, on Metal). | `docs/kinetic-studio/README.md:50-63` |
| 4 | Native audio or MIDI reconstruction? | **Native.** The studio forces the engine to return 32-bit float WAV with no normalisation and stores it as 24-bit FLAC. No synthesis anywhere in the generation path. (A SoundFont render exists only in the MIDI editor's "save as library track", which is user-initiated; see D7.) | `crates/music-server/src/lib.rs:8324-8333`, `lib.rs:7729-7743`, `crates/audio-post/src/encode.rs:44-53` |
| 5 | Does Ableton export influence generation? | **No.** Export reads the finished library file and copies it byte for byte (stored, not re-encoded); stems and score MIDI are made after the fact. Nothing in the request path knows about Ableton. | `crates/music-server/src/kinetic_ableton.rs:159-221, 235-262` |
| 6 | Stages bypassed, mocked, simplified, replaced? | **None bypassed or mocked.** Optional replacements exist and are off by default: chord bed (studio-written chords replace the model's plan, `lockToScore`), harmony biasing, AB2 solver, fewer steps, decoder companion LoRA. **One default addition is on:** the lyric-timing attention bias whenever a score is supplied (D3). | `app/services/kineticPrompt.ts:151, 333-344`; `lib.rs:7122-7129, 8499-8515`; `CreatePanel.tsx:343-358` |
| 7 | Instrumental workflow official? | **Only on the secondary button.** "Compose an instrumental score" follows m-a-p's skill (plan, move Vocal notes to Ins, render the fixed score with tags as lyrics). The primary "Use in form" sends tags without a score and skips the transfer (D2). | `KineticPromptPanel.tsx:187-224, 386-402`; `crates/music-server/src/score/instrumental.rs:1-63`; YuE `skills/yue2-music/instrumental/SKILL.md` |
| 8 | Hardware, offloading, settings, limits | Q8_0 instead of BF16 (H1). Stage eviction (AR → NAR → VAE) changes nothing numerically. VAE tiles 512+16 vs official 1024+16 (negligible by design, H5). **The 130 s default cap is the one limit that cuts music (D1).** The guide's speed tips (AB2, 16 steps) halve the acoustic work and have not been checked at 16 steps against 32 (H2); your measured song used the reference 32 midpoint. | `model_manager.rs:731-734`; yue2.cpp `README.md` "Pipeline"; `docs/kinetic-studio/README.md:66-79` |

---

## 3. Existing pipeline

```
                      KINETIC STUDIO (React)                                    MUSIC-SERVER (Rust, 127.0.0.1:8791)
 ┌────────────────────────────────────────────────┐   POST /v1/music/jobs   ┌──────────────────────────────────────────────┐
 │ Kinetic prompt builder (kineticPrompt.ts)      │ ──────────────────────▶ │ yue_request_from()            lib.rs:8371    │
 │   style text + lyrics or [section] tags        │                         │  · duration_seconds 130 ◀── D1 (UI default)  │
 │   optional: chord bed  ─▶ replaces model plan  │                         │  · companion_scale 0 (= official)            │
 │   optional: instrumental transfer (official)   │                         │  · laid_out(): re-lays unsectioned scores    │
 │ Create page (CreatePanel.tsx)                  │                         │  · lyric_schedule bias -4 ◀── D3 (score set) │
 │   duration 130 s ◀── D1   lyric timing ON ◀ D3 │                         │  · output forced to wav32, no normalising    │
 └────────────────────────────────────────────────┘                         └──────────────────────┬───────────────────────┘
                                                                                                   │ POST /synth
                                     YUE-SERVER (yue2.cpp, Metal, 127.0.0.1:18087)                 ▼
 ┌──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐
 │  YuE2-3B-Q8_0 AR half ── ABC score (≤4096 tok) ── semantic codes 25/s (≤ min(9000, duration·25)) ── truncated? ◀ D4 │
 │        │ evict                                     (stops at 130 s by default, no fade)             (logged, dropped) │
 │  YuE2-3B-Q8_0 NAR half ── flow matching, 32 midpoint steps (AB2 / fewer steps optional) ── 64-ch latents            │
 │        │ evict                                                                                                       │
 │  YuE2-Vae F32 ── 1920× upsample, tiles 512+16 ── 48 kHz stereo float                                                 │
 └──────────────────────────────────────────────────────────────────────────────────────┬───────────────────────────────┘
                                                                                        ▼ multipart: replay JSON + wav32
 ┌──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐
 │ LIBRARY: float → FLAC 24 (scaled down only if peaks > 0 dBFS ◀ D6) + score + semantic codes + replay request         │
 └──────┬───────────────────┬─────────────────────┬──────────────────────┬────────────────────┬─────────────────────────┘
        ▼ opt-in            ▼ opt-in              ▼ opt-in               ▼ opt-in             ▼ opt-in
   Processing           Export (loudness,     Stems: HT-Demucs 6s     Score MIDI           Ableton project (.zip)
   (denoise, lifter,    dither, MP3/FLAC)     at 44.1k → 48k,         (Lead, Chords,       stems on tracks, master
   VST, master-to-ref)  copies only           24-bit, sum report      Vocal) from ABC      in folder only ◀── D5
   new version, never
   overwrites                                                         MIDI editor ─▶ SoundFont WAV ─▶ library as
                                                                      "imported audio", no provenance ◀── D7
```

## 4. Official pipeline, for comparison

| Setting | Official `yue2` | Kinetic default | Same? |
|---|---|---|---|
| Backbone precision | BF16 (FP8 optional) | Q8_0 GGUF | No (H1) |
| Decoder | m-a-p/YuE2-Vae, FP32 | YuE2-Vae F32 GGUF | Yes |
| Score sampling | T .7, top-p .9, top-k 30, rep 1.005/100, 32–4096 tok | same | Yes |
| Semantic sampling | T 1.0, top-p .95, top-k 100, rep 1.2/50, 200–9000 tok | same, but max = **130 s × 25 = 3250** | **No (D1)** |
| CFG | 1.0 (full/melody), 1.01 (off) | same | Yes |
| ODE | 32 steps midpoint | 32 midpoint (AB2 / fewer optional) | Yes |
| Seeds | one seed, torch sampler | lm_seed (Philox) + seed (torch-compatible noise) | Equivalent, not identical |
| Attention on lyrics | full prompt visible | sections hidden until reached (bias −4) when a score is supplied | **No (D3)** |
| Instrumental | plan → Vocal→Ins → render fixed score | only via "Compose an instrumental score" | **Partly (D2)** |
| Output | float, clamped to ±1, FLAC 24 | float, scaled if > ±1, FLAC 24 | Nearly (D6) |
| Truncation | `result.json` `truncated` flags | computed by engine, not returned | **No (D4)** |
| Candidates | one per call; benchmark uses best-of-8 | one take | Same per call |

---

## 5. Confirmed defects

Ordered by likely effect on what you hear.

**D1. A 2:10 ceiling on every new prompt, applied silently.** *Verified.*
`app/components/CreatePanel.tsx:101` (`DEFAULT_DURATION_SECONDS = 130`), sent at
`CreatePanel.tsx:652-653`, becomes the semantic budget in the engine (yue2.cpp
`src/pipeline.h:559-568`, "the requested length caps the budget"). The score stage is not capped
(`lib.rs:8474-8484` raises it only above ~273 s), so the model writes a plan for the whole song and
the audio stops at frame 3,250, mid-phrase. Kinetic presets carry `durationSeconds: null`
(`kineticPrompt.ts:152`), which `applyKinetic` treats as "keep the current value"
(`CreatePanel.tsx:538`), so a seven-section Analog Electronica preset inherits 2:10. Effect: missing
bridges and outros, no ending, and score MIDI in the Ableton set that runs past the audio. This is
the most direct, code-level explanation for "arrangements feel basic" and "lack transitions".

**D2. Instrumental "Use in form" skips the official instrumental transfer.** *Verified.*
`KineticPromptPanel.tsx:187-191, 391` sends style + tags with `cot: full` and no score. The model
then plans a Vocal melody for words that do not exist; m-a-p's skill says to move those notes to Ins
before rendering and not to skip that step (YuE `skills/yue2-music/instrumental/SKILL.md`, "选择来源").
The correct path exists at `KineticPromptPanel.tsx:207-224` but is the secondary button. Effect
(hypothesis until listened to): wordless vocal-like lines, a lead melody that is thinner than an
instrumental theme, more "layered" feel.

**D3. A lyric-timing attention bias the official pipeline does not have, on by default.** *Verified.*
Whenever a score is supplied (composed first, edited, chord bed, instrumental transfer, covers),
`lib.rs:7127-7129` adds `lyric_schedule` with bias −4 (`score/schedule.rs:17`): lyric sections are
masked in attention until the score reaches them (yue2.cpp `src/request.h:34-45`, HOT-Step's C6).
It is meant to keep words on the melody, and it changes what the semantic stage conditions on.
Effect on arrangement and cohesion: *hypothesis*; the toggle exists (`CreatePanel.tsx:358`) but
defaults on.

**D4. Truncation is computed and thrown away.** *Verified.* The engine marks a song truncated
(yue2.cpp `src/pipeline.h:86, 651`; logged at `src/generate.h:244`) but `tools/yue-server.cpp:666-688`
never puts it in the reply, and the studio never derives it. The official pipeline records it in
`result.json` and its docs say to inspect it. Combined with D1, cut-off songs look like finished ones.

**D5. The original master is left out of the Ableton set when stems exist.** *Verified.*
`kinetic_ableton.rs:73-78`: `mix_on = stems == 0`. The master is copied into the folder but not
placed on a track, so the set you open plays separated stems, which are estimates with bleed and
phase loss, instead of the generated master. Confirmed on your Night drive export with
`scripts/quality/check_ableton.py` ("FAIL master on a track"). Every audio clip that *is* placed
plays unchanged (Re-Pitch, rate 1.000000, clip gain 1, fader 0 dB, no devices): that part passes.

**D6. The library FLAC is not always the engine's exact output.** *Verified, minor.*
`encode.rs:39-53` scales the whole song down when any float sample exceeds ±1.0; the float original
is then discarded. Official clamps instead. Neither is wrong, but "the original stereo master is
preserved without alteration" is not true in that case and is not recorded.

**D7. SoundFont renders enter the library looking like recordings.** *Verified, minor.*
`app/services/midiEditor.ts:140-148` posts the MIDI editor's SoundFont WAV to `/v1/library/import`,
stored as `engine_id: "imported-audio"` (`lib.rs:3835`). Nothing marks it as synthesised, so it can
be mistaken for a generation and exported as one.

**D8. Speed advice that trades fidelity is presented next to the measured run.** *Verified as text,
effect unknown.* `docs/kinetic-studio/README.md:69-79` suggests AB2 and 16 steps. AB2 at the same
steps was measured close to midpoint by the engine's author (mel correlation 0.9996, at 16 steps);
16 vs 32 steps has not been compared on this build. If any of your later songs used 16 steps, that
alone could soften transients and detail.
*Addressed in `SPEED.md`:* the guide no longer suggests 16 steps; AB2 at 32 steps is offered as a
labelled Fast Draft, and `scripts/quality/bench_speed.py` measures both against the reference.

## 6. Hypotheses to test (Phase 2)

| | Hypothesis | Why it is plausible | Test |
|---|---|---|---|
| H1 | Q8_0 loses detail vs BF16 | Quantising the NAR half changes every velocity step; yue2.cpp calls Q8_0 "near lossless" with its own cosine harness | A vs B-replay null test and LSD; then the BF16 set on the Mac (7.2 GB weights, evicted per stage, likely fits 16 GB) |
| H2 | 16 steps or AB2 soften the sound | Fewer ODE evaluations | B-replay at 32/16 steps, midpoint/AB2, same tokens |
| H3 | Prompts ask for minimal music | Presets and the project's style rules say restrained, sparse, limited instrumentation | Same seed, a preset vs a "dense, evolving, layered" rewrite |
| H4 | The lyric bias (D3) flattens development | It hides context the model was trained to see | Same score, `lyric_timing` on vs off |
| H5 | VAE tiling 512 vs 1024 | Halo-crop is designed to be exact | Decode the same latents both ways; expect a null under −80 dB |
| H6 | One take is the biggest remaining gap | m-a-p's Suno-level number is best-of-8 | Four takes per prompt, blind-pick |
| H7 | The decoder companion widens stereo | HOT-Step measured narrower stereo without it, on re-tokenised real songs, not generations | B-replay with companion 0 vs 1 |
| H8 | Score MIDI does not line up with the audio | YuE2 follows the score's tempo loosely | Beat-track the master (`beat_dbn.rs` exists) against the score's Q: |

No fallback audio generator exists. When the engine fails, the job fails
(`lib.rs:7170-7174` for a refused job; the watcher marks engine failures failed); nothing substitutes another sound.

---

## 7. Phase 3: controlled comparison

**Built, not yet run on real models.** `scripts/quality/` has the harness:

- `run_official.py`: output **A**, the official PyTorch pipeline with release defaults.
- `run_kinetic.py studio`: output **B**, the same request through Kinetic's own API with the Create
  page's defaults (and `--duration 0` for B-full).
- `run_kinetic.py replay`: **B-replay**, A's exact score and semantic tokens and A's seed
  re-rendered by Kinetic's engine. Because the two samplers differ, a seed cannot make A and B the
  same song; B-replay is what makes the comparison controlled. A vs B-replay isolates engine and
  quantisation; B-replay vs B isolates the studio's request path and sampler.
- `check_ableton.py`: output **C**, structural checks of the export plus a null test of Live's own
  render against the master.
- `analyze.py`: loudness, true peak, crest, clipping, bandwidth (energy above 16 kHz, roll-off),
  stereo correlation and side/mid per band, spectral flux, macro dynamics, novelty-based section
  boundaries, abrupt-ending detection, null tests and log-spectral distance.

Self-test on synthetic signals (container, 2026-10-14): FLAC vs WAV nulled bit-identically; an
MP3 at 128 kbps showed −23 dB residual and energy above 16 kHz falling from 0.061 % to 0.004 %; a
stereo-narrowed copy showed L/R correlation 0.925 → 0.993; a copy cut at 50 s was flagged
"ends abruptly" and lost two of five sections. On the Night drive export (synthetic tones), every
placed clip passed and the master-on-a-track check failed, as D5 predicts.

What is left needs your Mac (A on MPS, B and C in the app and Live) or approval for a download or a
GPU. The steps are in `scripts/quality/README.md`.

Where I expect divergence, from the code alone: **between B-full and B on any request longer than
2:10 (D1)**, and between Kinetic instrumentals made with "Use in form" and with "Compose an
instrumental score" (D2). A vs B-replay should be close (yue2.cpp's parity work); if it is not,
quantisation is the next suspect.

---

## 8. Phase 4: recommended architecture

The current shape is already production-first: the neural master is generated first and every
export reads from it. The recommendation keeps that shape and closes the gaps.

```
 PROMPT ─▶ REQUEST BUILDER (no silent rewrites: every non-official field is opt-in and recorded)
              │  Reference profile (default): cot full, no length cap (model ends the song, ≤ 6:00),
              │  32 midpoint steps, no lyric bias, companion 0, harmony off
              │  Instrumental: plan → Vocal→Ins transfer → render fixed score (always)
              │  Takes: 1–4 candidates per prompt (official best-of-N), picked by ear
              ▼
 YUE2 ENGINE  yue2.cpp Metal (Q8_0 or BF16 set)   or   remote CUDA engine (BF16, official PyTorch or yue2.cpp)
              ▼
 GENERATION RECORD (immutable)
   master.wav  float32 48 kHz stereo, exactly as decoded, SHA-256 recorded
   master.flac 24-bit for the library (gain recorded if any)
   score.abc · semantic codes · replay request · seeds · model and engine identity · truncated flag
              │
              ├─▶ Playback, library, processing (new versions only, master never replaced)
              │
              └─▶ OPTIONAL DOWNSTREAM: ABLETON PACKAGE  (never feeds back into generation)
                    ├─ Master track: the original master, on a track, active, 0 dB, Re-Pitch rate 1
                    ├─ Stems group: HT-Demucs estimates, deactivated by default, sum-to-mix report
                    ├─ Score MIDI: Lead, Chords (what the model was asked to play)
                    ├─ Transcribed MIDI: from stems via the existing transcriber (what it played)
                    ├─ Tempo map: beat-tracked from the master, so MIDI lines up with audio
                    └─ READ ME: what each file is and how far to trust it
```

Live 12 Lite holds 8 tracks. Priority when space runs out: master, then 4 stems (drums, bass,
vocals, other), then Lead and Chords MIDI; anything left goes in the folder with the read-me saying
so. Live Standard and Suite take everything.

**Limitations to state in the app and the read-me:**

- The score is a lead sheet: a vocal line, an instrumental line and chord symbols. Drums, bass,
  pads and sound design are in the audio only, never in the score.
- Score MIDI is the plan, not the performance. YuE2 follows it loosely in pitch and loosely in time.
- Transcribed MIDI is an estimate from an estimate: polyphony, drums and effects-heavy synths
  transcribe poorly.
- Stems are model estimates of parts that were never separate. They bleed, lose phase coherence,
  and do not sum exactly to the master (the stem report gives the residual). The master is always
  the reference.
- Re-pitched audio follows a tempo change in pitch too. Warping to a new tempo is Live's
  processing, outside the generated audio.

## 9. Phase 5: roadmap

### Prioritised remediation

| Priority | Change | Fixes | Risk |
|---|---|---|---|
| P0 | Length: "Auto" by default (send no `duration_seconds`); the field becomes an optional ceiling | D1 | Longer runs on the Mac; show the estimate |
| P0 | Truncation: derive it in the studio (frames ≥ budget) now, and ask yue2.cpp to return the flag; badge it on the song | D4 | None |
| P0 | Instrumental: "Use in form" runs the official transfer when the engine is ready (or swap the buttons) | D2 | A planning pass before Create |
| P0 | Ableton: master always on a track; stems deactivated when the master is on | D5 | Track budget in Lite |
| P1 | Lyric timing off by default, opt-in with its effect explained | D3 | Lyrics may drift on supplied scores; test first (H4) |
| P1 | Keep the float master; record any gain applied to the FLAC | D6 | Disk: about 2× the FLAC |
| P1 | Provenance: MIDI-editor renders stored as `source: midi_editor_render`, labelled "SoundFont render" | D7 | None |
| P1 | "Reference" vs "Fast" quality modes, with Fast labelled; README's 16-step tip marked unverified | D8 | None |
| P1 | Takes: 2–4 candidates per prompt with a blind picker | H6 | Time per prompt ×N |
| P2 | BF16 set on the Mac, if H1 shows a difference | H1 | Memory headroom |
| P2 | Transcribed MIDI from stems and a beat-tracked tempo map in the Ableton package | H8 | New downstream work |
| P2 | Prompt guidance for density and development, if H3 shows a difference | H3 | Changes the house style: your call |

### Specific code changes

1. `app/components/CreatePanel.tsx:101, 331-335, 625`: default the duration to empty ("Auto");
   keep `studio.createDuration` only when set by hand; `applyKinetic` (`:538`) clears it when the
   preset has no length.
2. `crates/music-server/src/lib.rs` (song import, near `:7729-7792`): compute
   `truncated = frames >= min(duration·25, semantic max_tokens)` from the replay request and store it
   in the song's metadata; show it in `RightSidebar.tsx`. Upstream: add `truncated` to the replay JSON in
   yue2.cpp `tools/yue-server.cpp:684-686`.
3. `app/components/KineticPromptPanel.tsx:386-402`: in instrumental mode the primary action calls
   `composeInstrumental()`; "Use in form without a score" stays as a secondary choice.
4. `crates/music-server/src/kinetic_ableton.rs:73-78`: `mix_on = true`; stems placed but deactivated
   when the mix is on (needs the track `Speaker` state in `ableton.rs`'s template writer); budget
   order master → stems → MIDI.
5. `app/components/CreatePanel.tsx:358` → `useState(false)` and `lib.rs:7127` → `== Some(true)`.
6. `crates/audio-post/src/encode.rs:48-53` and the import path: write `master.wav` (float) beside
   the FLAC and record `fitting_gain` when it is not 1.0.
7. `app/services/midiEditor.ts:140-148`: send a `source` field; `lib.rs:3835` honours it.

### Quality-validation test plan

1. Run `scripts/quality/` for both requests: A, B, B-full, B-replay, C. Three seeds for B and
   B-full.
2. Pass criteria. A vs B-replay: residual under −40 dB and LSD under 2 dB (same music, numerical
   drift only); B vs C: bit-identical master in the zip and Live's render nulling under −90 dB;
   B-full: never flagged "ends abruptly" unless the model hit 6:00.
3. Listening: the blind sheet in `scripts/quality/README.md`, loudness-matched.
4. After each P0/P1 change, repeat on the same requests and seeds and keep the reports side by side.

### Regression safeguards

- **Golden request test** (Rust unit test, Linux CI): the body `submit_music_job` sends for a
  plain prompt equals a stored JSON with no `duration`, no `lyric_schedule`, no `steps`/`solver`,
  `companion_scale: 0`. Any new default that reaches the engine fails the test until it is
  reviewed.
- **Master integrity test**: import a known float WAV through the generation import path; assert the
  FLAC decodes to the same samples (or the gain is recorded), and the Ableton zip's mix matches it
  byte for byte and is on a track.
- **Truncation test**: a replay request whose frames equal the budget is flagged.
- **Engine bump check** (macOS CI, needs a one-time approval to download the Q8_0 set): replay
  yue2.cpp's frozen `tests/request0.json` stream and null-test against a stored render. Fails if a
  new engine pin moves the sound by more than the agreed threshold.
- **Per-song quality fingerprint**: run `analyze.py`'s measures at import and keep them in the song's
  metadata, so drift across versions shows up in numbers.
