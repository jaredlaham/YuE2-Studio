# Kinetic Studio — Phase 0 technical audit

Audit date: 2026-10-12. Sources inspected:

| Repository | Commit | Notes |
| --- | --- | --- |
| `jaredlaham/YuE` (fork of `multimodal-art-projection/YuE`) | `1647252` (2026-10-06) | YuE2 reference engine, Python/PyTorch, `yue2-infer` 0.1.6 |
| `jaredlaham/YuE2-Studio` (fork of `timoncool/YuE2-Studio`) | `35614ea` (2026-10-11) | Studio v3.5.4: Tauri 2 + Rust/Axum + React, engine is `yue2.cpp` |
| `timoncool/yue2.cpp` (pinned by the studio at `c8516db`) | README read at `f65537f` | C++/GGML port of YuE2; CUDA, Vulkan, CPU, Metal on macOS |

How things were verified, so the labels below mean something:

- **Source** — read in the code at the file and line given.
- **Tested here** — exercised by a test run in this audit (Linux x86-64 container, no GPU, no Mac).
- **Unverified** — claimed by documentation or implied by code, but not exercised. Nothing in this
  audit was run on Apple Silicon, nothing generated audio, and no model weights were downloaded.

Baseline test run on Linux (this container), unmodified `main`:

- `cargo test --workspace` (stable toolchain): **359 passed, 0 failed, 4 ignored** (music-server 324,
  audio-post 17, music-engine 16, music-core 2).
- `app`: `npm ci`, `npm run build`, `tsc --noEmit`, `npm test`: **build OK, types OK, 79 tests passed** (23 files).
- `desktop/src-tauri` (`cargo build`, debug) compiles on Linux with GTK/WebKit dev packages.

---

## A. Verified YuE2 capabilities (`jaredlaham/YuE`)

| Capability | Finding | Evidence |
| --- | --- | --- |
| Model | **YuE2-3B**: one AR–NAR Mixture-of-Transformers backbone. AR predicts an ABC score then semantic codec tokens; NAR produces 64-dim acoustic latents by flow matching (midpoint ODE, 32 steps); a VAE decodes to **48 kHz stereo**. | `src/yue2/pipeline.py:255-304` (`plan` → `generate_semantic` → `synthesize` → `decode`), `src/yue2/protocol.py:44-50`, `pipeline.py:398` (48000) |
| Vocab / context | 184,704 tokens, **24,576-token context**; codec offset 151,853, 32,768 codec ids. Prefix + budget over context is refused, never silently truncated. | `protocol.py:7-12`, `sampling.py:62-63` |
| Reference inference requirements | Documented: Linux, Python 3.12, **NVIDIA BF16 GPU, 24 GB VRAM**. Code picks `cuda` → `mps` → `cpu`; BF16 weights; CUDA path refuses without BF16. | `README.md` Quick start; `docs/generation.md:3`; `pipeline.py:134-165,216-217` |
| Apple Silicon (reference engine) | Code has explicit MPS branches (GQA attention fallback, CPU-side multinomial, `torch.mps.synchronize`), but no doc claims Mac support, the default sampler RNG falls back to CPU on MPS, CUDA graphs and FP8 are CUDA-only. **Unverified on any Mac.** | `modeling_yue2.py:21-24`, `sampling.py:12-13,69,113-116`, `nar.py:75`, `quantization.py:36,121-124` |
| Quantization (reference engine) | Only **experimental FP8** for the AR linears, CUDA compute capability ≥ 8.9, explicitly "quality_validation: unvalidated". No int8/int4. | `quantization.py:1-6,67-73,111-114` |
| Prompt conditioning | Plain text: `instruction + [Tags] style + [Lyrics] lyrics`. Style is free text; there is no structured field for tempo, key, meter or exclusions. | `protocol.py:13-17,113-114` |
| Composition planning (CoT) | `cot="full"` melody + chord ABC (default), `"melody"` melody only, `"off"` no score. An external `abc` can be supplied in `full`/`melody`. | `protocol.py:13-17,95-110`; `pipeline.py:255-269` |
| Symbolic score format | ABC notation with two voices, `Vocal` and `Ins`, and native chord symbols. Saved as `score.abc` plus exact token arrays and a hash manifest. | `pipeline.py:28-63`; `skills/yue2-music/references/abc-editing.md` |
| Sampling parameters | Per stage: temperature, top-p, top-k, repetition penalty, penalty window, min/max tokens; CFG scale 0–20 (defaults 1.0, 1.01 for `off`). ABC stage default `0.7/0.9/30/1.005/100/32/4096`; semantic `1.0/0.95/100/1.2/50/200/9000`. | `protocol.py:21-50,76-101` |
| Seed reproducibility | Integer seed per request; deterministic cuDNN, TF32 off. The docs warn that different GPUs, runtime versions or sampling settings change a seeded result. **Same machine + same weights + same settings only.** | `protocol.py:79,88`; `pipeline.py:139-144`; `docs/generation.md` "Outputs and reproducibility" |
| Negative / exclusion control | **None.** The CFG negative branch is the bare instruction (and exact ABC), not user text. "No vocals" etc. can only be written into the style text and is best-effort. | `protocol.py:131-142` |
| Instrumental generation | Not a model mode. The official **instrumental workflow** plans a score, then moves every `Vocal` note into `Ins` (pitch, onset, duration preserved) and renders with empty/section-only lyrics. The skill itself says listening is still needed to check for vocal leakage. | `skills/yue2-music/instrumental/SKILL.md`; `skills/yue2-music/instrumental/scripts/instrumental.py`, `abc_tools.py`; root `README.md` "Agent skill" |
| Vocal generation | Primary mode: lyrics with section tags are sung. | `docs/generation.md` |
| Song extension / continuation | **Not supported.** No continuation or inpainting API; an edit regenerates a whole new recording; `--resume` only reuses a *completed* result. | `skills/yue2-music/references/editing-workflows.md:3`; `generation-and-covers.md:139-141,205` |
| Regeneration from edited score | Supported: submit edited ABC as a new `abc` input; a saved plan cannot be edited in place (hash check). | `pipeline.py:43-63`; root `README.md` "Edit a composition" |
| Covers | SheetSage2 (separate environment) transcribes a recording to melody ABC; YuE2 renders it with `cot="melody"`. | root `README.md` "Cover a song"; `skills/yue2-music/scripts/transcribe.py` |
| Audio output | `save()` writes **FLAC (PCM_24) or WAV (float32)** only; MP3 is refused here ("optional delivery conversion"). Decoded audio is clamped to [-1, 1]. | `pipeline.py:93-101,352` |
| Duration control | No duration parameter; length follows lyrics/score and the token budget (`max_tokens` 9000 semantic). `truncated` flags report hitting a limit. | `protocol.py:28`, `pipeline.py:89-91` |
| Watermarking / provenance | **No audio watermark** in the code (searched). Provenance is metadata: `result.json` with request identity hash, weights identity, runtime hash, timings, artifact hashes. | `pipeline.py:103-118`, `storage.py` |
| Licensing | Code, skill, docs: **Apache-2.0**. Weights (YuE2-3B, YuE2-Vae, -legacy): **CC BY-NC 4.0 + an "individual creator permission"** that lets individuals monetize outputs; companies need a commercial licence. | `LICENSE`, `MODEL_LICENSE:1-60`, root `README.md` "License" |

## B. Verified YuE2-Studio features

Classification legend: **Reusable** = implemented and reusable; **Platform** = implemented but platform-dependent;
**Partial**; **Docs only** = documented but not verified in code; **Missing**.

The studio does **not** run the Python engine. It supervises `yue-server` from yue2.cpp (GGUF weights), over HTTP
on 127.0.0.1:18087 (`crates/music-engine/src/yue_server.rs:24`, `README.md` "Architecture").

| Feature | Class | Evidence and notes |
| --- | --- | --- |
| Text-to-music generation | Reusable | `crates/music-engine/src/yue_server.rs` (engine supervision, jobs), `crates/music-server/src/lib.rs` (routes, job import, progress from the engine log), `app/components/CreatePanel.tsx` |
| Instrumental generation | Reusable | `crates/music-server/src/score/instrumental.rs` ports m-a-p's Vocal→Ins transfer; route `/v1/score/instrumental` (`lib.rs:1057`); UI in `CreatePanel.tsx`, `ScoreView.tsx` |
| Song extension | Missing | Not in YuE2 either (see A). |
| Exact replay / re-render | Reusable | Track keeps request + audio codes; `ReplayModal.tsx`, `songTokens.ts` |
| Cover generation | Reusable (Mac: unverified) | `cover_prompt.rs`, SheetSage2 GGUF via the engine; transcription on Metal is untested |
| Score-first composition | Reusable | `score/api.rs`, "compose the score first" (`README.md`), `score/*` |
| ABC notation editing | Reusable | `score/abc.rs`, `score/notation.rs`, `score/edits.rs`, `score/transpose.rs`, `score/rebuild.rs` (+ tests) |
| Sheet music rendering | Reusable | `abcjs` (`app/package.json`), `app/services/scoreEngraving.ts` (+ test), `app/public/editor` |
| Piano roll / MIDI editing | Reusable | Bundled build of `signal` (MIT) at `app/public/midi-editor` (`engines/midi-editor-source.json`); `app/components/midi/MidiEditor.tsx` |
| MIDI recording | Reusable (Mac: unverified) | Web MIDI in the bundled editor (`app/public/midi-editor/assets/studio-*.js` uses `requestMIDIAccess`). WKWebView on macOS does **not** implement Web MIDI, so this likely fails on Mac — must be tested |
| Score → MIDI export | Reusable | `score/export.rs` + `score/smf.rs`: conductor track (tempo, meter, key, section markers), `Vocal`, `Ins`, `Chords` tracks; SMF round-trip tests (`smf.rs:372-410`) |
| Audio-to-MIDI transcription | Platform | MuScriptor via HOT-Step `ace-midi`: Windows download; macOS built from source with Metal (`scripts/build-midi-runtime.sh`, `midi.rs:46-48`). Weights CC BY-NC 4.0 |
| Stem separation | Platform | HT-Demucs 6-stem ONNX in-process (`separation.rs`). ONNX Runtime is fetched as **Windows `.dll` only** (`lyrics_sync.rs:707-808,1096-1113`): **not available on macOS today**. Output is **44.1 kHz, 16-bit** (`separation.rs:104,296-325`), not the 48 kHz/24-bit Kinetic needs |
| Stem player | Partial | `StemPlayer.tsx`, `openStems.ts`, `songStems.ts`; mute/solo/volume exist, loop regions and per-stem pan not confirmed |
| Noise reduction / Spectral Lifter / vocal naturaliser | Reusable | `crates/audio-post/src/denoise.rs`, `lifter.rs`, `naturalize.rs` (pure Rust) |
| Reference mastering | Reusable | `audio-post/src/mastering.rs` (matchering port) |
| Normalisation / limiter | Reusable, opt-in | `encode.rs:18-33` (`normalize_peak`), `limiter.rs`; applied only as a chosen processing stage |
| VST3 | Platform | Separate HOT-Step `vst-host` process, looked up as `vst-host.exe` (`vst.rs:60`); built by `build-vst-host.ps1` only. **Windows only** |
| Audio encoding | Reusable | libFLAC 24-bit with verify (`encode.rs:48-86`), LAME CBR MP3 32–320 kbps at 32/44.1/48 kHz, no resampling (`encode.rs:97-136`). Note: both apply `fitting_gain` (scale down only when float peaks exceed full scale) |
| WAV export options | Partial | 24-bit and float WAV writers exist (`audio_pcm.rs:69-100`); no 16-bit-with-dither export, no user-facing 44.1/48 kHz choice |
| Quality analysis | Partial | `audio-post/src/quality.rs` gives a heuristic score. **No integrated LUFS, LRA, true peak, clipping report** (searched: no `lufs`/`true_peak` in the code) |
| Metadata tagging | Reusable | `lofty` (`tagging.rs`), ID3 for MP3 |
| Library / history / versions | Reusable | SQLite (`library.rs`, `rusqlite`), each result a track with its parent; `LibraryView.tsx`, `SongList.tsx`, playlists |
| Model downloads / sets | Reusable | `model_manager.rs`, `downloads.rs`: pinned revisions, size + SHA-256 checks, resume. Sets: BF16 6.67 GB, Q8_0 3.55 GB, Q6_K 2.74 GB, Q5_K_M 2.44 GB backbone + 506 MB VAE (`README.md` "Models") |
| Quantization | Reusable | GGUF quantised backbones from `Serveurperso/YuE2-GGUF` (pinned `e630f2b`) |
| LoRA management | Reusable | `adapters.rs`, `AdapterPicker.tsx`, `AdaptersPage.tsx`, catalogue + Hugging Face search |
| LoRA training | Platform | HOT-Step `music-train`, NVIDIA CUDA ≥ 11 GB only (`README.md` "What runs where"; `training.rs`). **Not on Mac** |
| AI-assisted prompting | Reusable | `assistant.rs`, `assistant_runtime.rs` (llama.cpp sidecar), `providers/openrouter.rs`, MCP agent mode |
| MCP integration | Reusable, needs hardening | `mcp.rs` (2,961 lines, ~162 tools), loopback + Origin check only (`mcp.rs:607-626`), no key. Several tools read **any local path** the agent names (e.g. `cover_set_from_file`, `mcp.rs:1919-1926`; import tools `:1161-1173`). Annotations mark destructive tools but nothing enforces consent |
| Generation progress and recovery | Reusable | Progress parsed from the engine log (`engine_log.rs`, `progress.rs`); the service imports finished songs even if the window closed (`README.md` "Architecture") |
| Remote access | Partial | `remote.rs`: whole studio served to another computer with an access key. Engine URL override exists only as env `YUE_ENGINE_BASE_URL` (`lib.rs:8044`); no remote-engine setting or auth |
| macOS shell | Partial | `.dmg` build with Metal engine (`scripts/build-release-macos.sh`, `.github/workflows/release-unix.yml`, manual trigger), Application Support data dir (`desktop/src-tauri/src/lib.rs:40-60`), Cmd+Q confirm (`lib.rs:660-690`), app-bundle engine lookup (`music-server/src/lib.rs:4206-4225`). The upstream README still says "the author works on Windows"; contributed via upstream PR #51 |
| Telemetry | Present, opt-out | Anonymous daily stats to the upstream author's server, **checked by default** on the first screen (`README.md` "Anonymous statistics"; `studio-hub-client` in `music-server/Cargo.toml`) |
| Auto-update | Present | Points at `timoncool/YuE2-Studio` releases (`tauri.conf.json` plugins.updater) |
| Ableton Live export | Missing | No `.als` code anywhere (searched) |
| Dither, 16-bit WAV, sample-rate choice | Missing | — |

## C. Existing reusable components

- **Whole service** (`crates/music-server`, 43.8 k lines with the other crates): routes, jobs, library, score tools,
  MCP, model manager. Platform code is already `cfg`-gated for Windows/macOS/Linux.
- **Engine adapter** (`crates/music-engine/src/yue_server.rs`): starts, health-checks, cancels and restarts `yue-server`.
- **Audio DSP crate** (`crates/audio-post`): pure Rust, FLAC/MP3 encoders, resampler (`rubato`), STFT,
  denoise, lifter, mastering, limiter. The natural home for the export manager and loudness analysis.
- **Score stack** (`crates/music-server/src/score/*`): ABC parse/write, notation, transpose, instrumental transfer,
  SMF read/write. This is the source of truth for "MIDI derived from the original symbolic composition".
- **React UI** (`app/`): Create, Library, Score, MIDI editor, Stems, Settings, Player; Vitest suite.
- **Build scripts**: Metal engine and Metal `ace-midi` builds for macOS; Tauri macOS bundle config.

## D. macOS compatibility and GPU feasibility (M1/M2, 16 GB)

| Path | Assessment |
| --- | --- |
| **yue2.cpp on Metal (recommended local path)** | Feasible on memory: yue2.cpp's README gives the Q8_0 pair at ~4.4 GB + KV cache, the BF16 pair at ~7.7 GB. On 16 GB unified memory, Q8_0 (or Q6_K) leaves room for the app and macOS. The studio's macOS build already compiles the engine with `-DGGML_METAL=ON` and upstream CI runs `yue-server --help` on `macos-14`. **Speed and output quality on M1/M2 are unmeasured**; the only published timing is an RTX 4090 (3:38 song in ~46 s). `--max-seq` can shrink the KV cache if memory is tight. |
| PyTorch reference on MPS | Possible in principle (MPS branches exist), but BF16 3.6B + VAE in PyTorch on 16 GB is tight, FP8 and CUDA graphs are unavailable, and nothing documents or tests it. Not recommended. |
| Quantized local | Covered by GGUF Q8_0/Q6_K/Q5_K_M via yue2.cpp. Quality vs BF16 is the GGUF publisher's claim ("near lossless" for Q8_0), not measured here. |
| Remote NVIDIA GPU | Viable with little new code: `yue-server` is an HTTP job server and the studio's client already honours `YUE_ENGINE_BASE_URL`. Needs a settings field, health/timeout handling, and **a secure transport** (yue-server has no auth and binds 0.0.0.0 by default — use an SSH tunnel or a reverse proxy with a token). Alternative already in the code: run the whole studio on the GPU box with network access on (`remote.rs`). |
| Cloud-hosted generation | Same as remote, on a rented GPU. Costs money; must be an explicit user choice. Not done in this audit. |
| Other Mac gaps | Stems (no macOS ONNX Runtime download), VST3 (Windows host only), LoRA training (CUDA only), Web MIDI recording in WKWebView (likely unsupported), karaoke Parakeet/tempo (ONNX, same gap as stems). |

## E. Recommended architecture: **Option A — keep YuE2-Studio's Tauri/Rust/React stack**

| Criterion | A: Port YuE2-Studio (Tauri) | B: Native SwiftUI + reuse services |
| --- | --- | --- |
| Effort | Low–medium: most of the app, a macOS bundle config, the Metal engine build and Mac data paths already exist | High: rebuild Create, Library, Score, MIDI editor, Stem player, Settings, Export UIs in SwiftUI; keep Rust service anyway |
| Reuse | ~all of `crates/*` and `app/*` | Only `crates/*`; ~all UI rewritten |
| Audio performance | Heavy audio work is in Rust/C++ (engine, DSP, encoders); WebView only plays and draws | Same backend; AVFoundation playback slightly better integrated |
| Memory | WKWebView adds ~100–200 MB; small next to 4–8 GB of model | Slightly lower |
| macOS fit | Good with Tauri 2 (native menus, dialogs, drag-drop, Keychain via plugin); some web APIs differ in WKWebView (Web MIDI) | Best |
| Maintainability | Can keep merging upstream's fast-moving fixes (v3.5.4, frequent releases) if Kinetic changes stay additive | Diverges permanently from upstream |
| Testing | Existing Rust (359) and Vitest (79) suites carry over | New UI test suite needed |
| Packaging | Tauri produces `.app`/`.dmg`, ad-hoc signed | Xcode project + embedding the Rust service |

**Decision: Option A.** It delivers a working app soonest with the least rewriting, keeps ~440 existing tests,
and keeps the door open to upstream fixes. Kinetic-specific work is kept additive (separate Tauri config overlay,
a `kinetic` cargo feature, new modules) so upstream merges stay cheap. A native SwiftUI shell can be revisited later
only if WKWebView limits (e.g. Web MIDI) block a must-have feature; the Rust service would be reused either way.

## F. Missing features and implementation risks

| Gap / risk | Impact | Plan |
| --- | --- | --- |
| Metal generation speed and quality on M1/M2 16 GB unknown | Could make local generation impractical | Phase 2 benchmark on your Mac (model load, tokens/s, wall time, peak memory) before promising local generation; remote backend as fallback |
| No macOS ONNX Runtime → no stems, karaoke, tempo on Mac | Blocks Phase 4 on Mac | Add `onnxruntime-osx-arm64` (official release, MIT) download + CoreML/CPU provider; validate |
| Stems at 44.1 kHz/16-bit | Breaks 48 kHz/24-bit stem spec | Resample stems back to source rate, write 24-bit; measure sum-vs-mix reconstruction error |
| No LUFS / true peak / LRA | Phase 3 analysis requirement | ITU-R BS.1770-4 / EBU R128 in `audio-post`, tested against EBU Tech 3341/3342 signals |
| No dither / 16-bit / rate options | Phase 3 export spec | TPDF dither on reduction to 16-bit, explicit resample choice, original never modified |
| `fitting_gain` scales hot float audio down on export | Silent gain change when peaks > 0 dBFS | Report it in the export summary and make it explicit (clip vs scale vs float WAV) |
| No `.als` export; format undocumented | Phase 6 cannot claim compatibility without Live Lite | Generate from a template saved by Live Lite 12; validate on your Mac; otherwise ship the labelled stems+MIDI fallback |
| No continuation / extend in YuE2 | Requested workflow item | Not offered; regenerate-from-score is the supported path |
| No real exclusion control | Prompt "exclusions" are best-effort | Prompt compiler labels them as such |
| MCP reads arbitrary paths, no consent | Security requirement in §17 | Phase 7: path allow-list (library/import folders), confirmation for destructive/open-world tools |
| Telemetry on by default to upstream server | Privacy/product decision | Your call (see PR) |
| Upstream updater would replace Kinetic with YuE2 Studio | Wrong app installed | Disabled in the Kinetic build (this PR) |
| Port 8791 shared with YuE2 Studio | Both apps open at once would share one service | Give Kinetic its own port later if both are installed |
| Web MIDI unavailable in WKWebView | MIDI keyboard recording likely broken on Mac | Test; native CoreMIDI bridge via Tauri plugin if needed |
| No signing / notarization | Gatekeeper warning on first open | Ad-hoc signed build; right-click → Open, or sign with your Developer ID later |

## G. Licensing and dependency considerations

| Component | Licence | Consequence |
| --- | --- | --- |
| YuE2-Studio code | MIT (© Ilya Timonin) | Free to modify and ship; keep the notice |
| YuE2 code | Apache-2.0 | Free; keep NOTICE/licence |
| **YuE2-3B, YuE2-Vae weights** | **CC BY-NC 4.0 + individual creator permission** | You, as an individual creator, may use outputs commercially. A *company* distributing or monetising the app/weights needs a commercial licence from M-A-P. Weights are downloaded by the user, never bundled |
| GGUF conversions (Serveurperso) | Derived from the above | Same terms as the weights |
| yue2.cpp, ggml | MIT | Bundled in the app |
| HT-Demucs 6s ONNX | MIT (per the studio's catalogue note) | OK |
| MuScriptor weights | CC BY-NC 4.0 | Non-commercial; audio-to-MIDI output usage should be flagged |
| HOT-Step (ace-midi, vst-host, trainer) | MIT | OK |
| signal (MIDI editor) | MIT; A320U SoundFonts **GPL-2.0** | SoundFont files are GPL; distribute their source/licence as upstream does (`licenses/`) |
| LAME | LGPL | Statically linked via `mp3lame-encoder`; LGPL relinking obligations apply to a distributed binary |
| libFLAC | BSD-3 | OK |
| Gemma (optional assistant) | Gemma terms | User-downloaded |
| Ableton Live Set Export SDK | Not evaluated yet | Phase 6: confirm licence and macOS applicability before use |

No cloud cost has been incurred. GitHub Actions on this public fork are free.

## H. Updated roadmap

| Phase | Scope (Option A) |
| --- | --- |
| 0 | This audit |
| 1 | **Kinetic Studio macOS foundation**: own name, bundle id, data folder; upstream updater off; `.app` + `.dmg` build script; macOS CI that runs the Rust tests on Apple Silicon, builds the app with the Metal engine, launches it and checks the service answers from the bundle. Existing navigation, library, import, playback, settings and storage carried over unchanged |
| 2 | Generation on Mac: Metal benchmark on your Mac; remote engine setting (URL + SSH-tunnel guidance, health, timeouts); structured prompt editor + prompt compiler + Kinetic presets; instrumental mode on by default for presets |
| 3 | Export manager: WAV 16/24-bit at 44.1/48 kHz with TPDF dither, MP3 192/256/320 with tags, FLAC at source rate; loudness/peak analysis (LUFS, LRA, true peak, clipping, silence); export presets; originals never touched |
| 4 | Stems on Mac: macOS ONNX Runtime; 4- and 6-stem; 48 kHz/24-bit aligned output; reconstruction error report; stem player loop + pan |
| 5 | Composition & MIDI: score-derived melody/chord MIDI labelled as such; audio-to-MIDI labelled as estimated |
| 6 | Ableton: Live-Lite-12-template `.als` with ≤ 8 tracks, or the labelled fallback package; validation on your Mac |
| 7 | MCP hardening (path allow-list, consent), LoRA on Mac (inference only), reference mastering surfaced |
| 8 | Regression, performance profiling, packaging, user guide |

## I. Acceptance criteria

| Phase | Done when |
| --- | --- |
| 1 | (a) Rust suite passes on `macos-14` (Apple Silicon) in CI; (b) CI builds `Kinetic Studio.app` arm64 with `CFBundleName` "Kinetic Studio" and the Metal engine inside; (c) the built app launches in CI and `/health` reports it is served from `Kinetic Studio.app` with the engine bundle present; (d) data lands in `~/Library/Application Support/Kinetic Studio`; (e) **on your Mac**: the app opens from /Applications (after right-click → Open), you can import an audio file, play it, change a setting, quit and find it all again after relaunch |
| 2 | One instrumental generated end-to-end on your Mac (or the remote backend) with measured load time, wall time and peak memory recorded; prompt compiler unit tests; presets editable and saved |
| 3 | Tests prove: bit depth and rate in every written file, dither only when reducing bit depth, original file hash unchanged, MP3 bitrate in the frame headers, LUFS within ±0.1 LU on EBU 3341 signals, cancellation leaves no partial files |
| 4 | Stems start at sample 0, equal length to the mix, 48 kHz/24-bit, sum-to-mix error reported; runs on your Mac |
| 5 | MIDI notes and tempo round-trip against the score in tests; estimated MIDI is labelled |
| 6 | The checklist in §14 of the brief passes in Live Lite 12 on your Mac, or the fallback is shipped and labelled as such |
| 7–8 | Per-feature tests plus a full real-world pass of the 10-step workflow on your Mac |

---

## Phase 1 results (this branch)

| Check | Where | Result |
| --- | --- | --- |
| Rust workspace tests | Linux container | 359 passed, 4 ignored |
| UI build, types, tests | Linux container | build OK, `tsc` OK, 79 passed |
| Desktop shell, default and `--features kinetic` | Linux container (debug) | compiles; with the overlay config merged the binary carries `Kinetic Studio`, `com.kineticfields.kinetic-studio`, `Kinetic-Studio` |
| `scripts/kinetic-smoke.sh` against the real desktop binary | Linux container under Xvfb | passed: launch, import 48 kHz/24-bit WAV, library lists it, playback bytes identical (sha256), survives restart |
| Rust tests on Apple Silicon | `kinetic-macos.yml`, `macos-14` | passed ([run 38202240169](https://github.com/jaredlaham/YuE2-Studio/actions/runs/38202240169)) |
| `Kinetic Studio.app` build with Metal engine, bundle checks, launch, smoke test | `kinetic-macos.yml`, `macos-14` | passed ([run 38202240169](https://github.com/jaredlaham/YuE2-Studio/actions/runs/38202240169)): arm64 app and Metal engine, ad-hoc signature verifies, `/health` answers from `Kinetic Studio.app` with the engine present, data in `~/Library/Application Support/Kinetic Studio`, smoke test passed; `.dmg` (62 MB) and zipped `.app` uploaded as the run's artifact |
| Opens from /Applications, UI usable, import/play/settings by hand | Your Mac | **unverified** |

Observed while testing: at launch the studio contacts `studio-hub.timoncool.workers.dev` (the upstream news feed,
fetched whatever the statistics setting is). The container's proxy blocked it and the studio carried on normally.

---

## Phase 2 results so far (`kinetic-studio/phase-2`)

What was added:

- **Prompt compiler** (`app/services/kineticPrompt.ts`): structured fields (genre, described influences, mood, direction,
  six instrument groups, sections, arrangement, production, tempo, key, meter, harmony, energy, length, exclusions, and
  for vocal mode language, voice and lyrics) compiled into YuE2's two real inputs, the `[Tags]` style and the `[Lyrics]`
  text. Every setting is reported with how it reaches the model: *style* (guidance), *score* (fixed in a chord-bed
  score), *request* (engine field) or *not sent*, with the reason. Repeats are merged with a note, contradictions
  ("no drums" with "soft drums") are errors, exclusions are labelled best-effort, a long style is warned about and
  never cut. Instrumental mode follows m-a-p's own workflow: section tags only as lyrics, "no vocals, no singing, no
  choir, no spoken words" added, key named in the style as m-a-p's example does.
- **Lock to score**: tempo, key and meter can be fixed by writing the studio's chord bed (`/v1/score/chord-bed`) and
  rendering from it. Without the lock they are only words in the style.
- **Compose an instrumental score**: plans a score from the style and section tags, then moves sung notes to the
  instrument voice (`/v1/score/instrumental`), as m-a-p's instrumental skill does.
- **Presets**: the five from the brief, built in; user presets saved, updated and deleted through
  `GET/PUT /v1/kinetic/presets` (`crates/music-server/src/kinetic.rs`), one file in the data folder written atomically.
- **Create panel**: the builder sits above the Style card in Studio mode, shows exactly what YuE2 is sent, fills the
  form's style, lyrics, chain-of-thought and length (the fields stay editable) and can undo that fill.

| Check | Where | Result |
| --- | --- | --- |
| Compiler and presets unit tests | Linux container | 15 passed (all 5 presets compile without warnings, ≤50 words, tempo last) |
| Preset store tests | Linux container | 4 passed; whole Rust workspace 363 passed |
| UI types, tests, build | Linux container | `tsc` OK, 94 passed, build OK |
| Builder in a real browser against the real service | Linux container, Chromium | passed: preset compiles, "Use in form" fills style and lyrics exactly, manual edit, undo restores, lock writes a 28-bar chord bed at 118 BPM in Am, saved preset survives a reload. Only the "engine ready" flag was stubbed because no weights are installed here |
| Compose an instrumental score, then render | needs the engine and weights | **untested**: no weights in the container, none downloaded |
| A generated instrumental with load time, wall time and peak memory | Your Mac | **unverified** (Phase 2 acceptance) |

### Remote GPU engine (`kinetic-studio/phase-2`)

Settings → Engine and the first-run page can point the studio at `yue-server` on another computer
(`crates/music-server/src/remote_engine.rs`, `GET/PUT /v1/kinetic/engine`, `POST /v1/kinetic/engine/test`). The
address is read at launch. While it is set the studio starts no local engine, does not require local model files,
leaves launch flags alone and refuses to "restart" an engine it does not own. An address that is not loopback is
warned about because `yue-server` has no auth or TLS; the documented route is an SSH tunnel.

| Check | Where | Result |
| --- | --- | --- |
| Address parsing, loopback detection, saved-file round trip | Linux container | 4 tests passed; Rust workspace 367 passed |
| First-run page → test → save → restart → Create screen with no local models | Linux container, Chromium, a stand-in `yue-server` answering `/health` and `/props` | passed with nothing in the window stubbed: `ready` and `engine_ready` true, no local engine started, options change does not restart, the stand-in going away is reported as "No answer from the music engine at …" |
| Generating through a real remote `yue-server` | needs a GPU computer with weights | **untested** |

---

## Phase 3 results so far (`kinetic-studio/phase-3`)

Song menu → **Export…** writes a new file into `<data folder>/exports/` and offers it through the normal Save dialog.
The song's own file is only read; its SHA-256 is taken before and after, and the export fails rather than writes if it changed.

- Formats: WAV 16 / 24-bit or 32-bit float, FLAC 16 / 24-bit (libFLAC, verify on), MP3 CBR 128–320 kbps (LAME). AIFF is not offered yet.
- Sample rate: kept, or converted to 44.1 / 48 / 88.2 / 96 kHz (rubato, already in the workspace).
- Dither: TPDF, added only when samples do not already fit the target's bits; an untouched 16-bit source written at
  16 or 24 bits is copied sample for sample. MP3 is not dithered.
- Loudness: optional target (-14, -16, -23 LUFS or custom) reached with **one gain change**, measured to ITU-R BS.1770-4,
  and held back so the true peak (4× oversampled) stays under the ceiling. There is no limiter or compressor; when the
  ceiling stops the gain short, the report says by how much. Without a target, a float source too hot for an integer
  format is turned down to fit, and the report says so.
- Every export returns a report: written and source format, loudness and true peak before and after, gain, dither, notes.
- Written as `<name>.part` and renamed when complete; an existing export is never replaced (`name (2).flac`).

| Check | Where | Result |
| --- | --- | --- |
| BS.1770 meter on EBU Tech 3341 cases 1–5 (44.1 and 48 kHz) | Linux container, `audio-post` tests | within ±0.1 LU |
| True peak on an inter-sample peak (fs/4 sine, 45° phase) | same | within 0.2 dB of the analogue peak |
| Rate and depth of every written WAV/FLAC (3 depths × 3 rates, 2 FLAC depths) read back from the file | `music-server` export tests | passed |
| Dither only when reducing: 16-bit source → 16-bit WAV identical, → 24-bit FLAC identical | same | passed |
| Original SHA-256 unchanged; second export gets a new name | same | passed |
| MP3 frame headers carry the requested bitrate (320/192/128) and rate | same | passed, every frame |
| Loudness of the written file within ±0.1 LU of -23/-16/-14; ceiling respected | same | passed |
| Cancel at every stage leaves no file and no `.part` | same | passed |
| Export dialog in Chromium against the real service | Linux container | passed (only the engine-ready flag stubbed). The written FLAC measured by **ffmpeg's ebur128**: -14.0 LUFS, 44.1 kHz, s16; ffprobe on the MP3: 48 kHz, 192 kb/s |
| Ableton import of exported files | Your Mac | **unverified** (Phase 6) |

## Phase 4 results so far (`kinetic-studio/phase-4`)

Stems that line up with the mix in a DAW, and ONNX Runtime on the Mac.

| Item | Status | Evidence |
|---|---|---|
| ONNX Runtime on macOS | Built: the processor build of ORT 1.30.0 is now downloaded per platform (`onnxruntime-osx-arm64-1.30.0.tgz` on macOS, `linux-x64` on Linux, the Windows zip unchanged), its libraries unpacked from the `.tgz` (`downloads.rs::extract_tgz_libraries`) and found by their versioned name (`lyrics_sync.rs::ort_library_in`) | Verified here on Linux through the app's own install route: the 11.3 MB archive downloaded, `libonnxruntime.so.1.30.0` unpacked, and separation ran on it. The macOS archive is the same layout; it is exercised by the new `stems` CI job on macos-14 |
| Stem format | 24-bit PCM WAV, TPDF-dithered, at the mix's own sample rate (the model runs at 44.1 kHz; stems are resampled back), cut or padded to the mix's exact frame count, starting at its first sample (`stem_render.rs`) | Unit tests: rate and exact length at 44.1/48/96 kHz, a click stays on its sample after 44.1k→48k, 24-bit readback within 2 LSB, repeatable files |
| Sum-to-mix report | Every run measures (sum of the six stems − mix) against the mix, and the largest sample error; shown under the stems, kept in each stem's library metadata. A partial set says so instead of a number | Unit test: exact parts read under −100 dB; a missing part reads its share of the mix within 0.1 dB |
| Labelling | Stems are marked `separated: true` in their metadata, and the panel says they are the model's estimate of each part, not the original recording | Browser check of the stems panel |
| HT-Demucs on this machine | **Not run here**: the environment's network policy blocks huggingface.co, so the 136 MB model could not be downloaded into this container | The pipeline was run end to end through the service with a *stand-in* ONNX model (six fixed shares of the mix, which separates nothing) on a 20 s, 48 kHz/24-bit track: six stems, each 48 kHz/24-bit/960,000 frames; independent check with ffmpeg + numpy: lag 0 against the mix, sum error −89.8 dB, the only error over −100 dBFS in the first and last ~50 samples (resampler edges) |
| HT-Demucs on Apple Silicon | CI job `stems` (macos-14, M1) downloads the model and ORT (cached) and separates a synthetic 48 kHz mix (bass line + hi-hats) with the real model, asserting rate, exact length and sum error under −10 dB, and printing the report | Passed on macos-14 (M1), run 38209028790 job `stems`: stems 48 kHz / 24-bit / 576,123 frames each, exactly the mix. The bass line landed in `bass` (−13.5 dB), the hi-hats in `drums` (−33.9 dB), the other four near silence (−83 to −91 dB). Sum error −30.0 dB, peak −15.7 dBFS; on this test mix that includes the white-noise hats' energy above 22.05 kHz, which no stem can carry. The 12 s mix took about 71 s in an unoptimised debug test build on the CPU, so it says nothing yet about release-build speed. The real macOS archive unpacked with the studio's own code to `libonnxruntime.1.30.0.dylib` (43,879,424 bytes) |

Known limits, stated rather than hidden:

- The model sees the mix at 44.1 kHz. From a 48 kHz or 96 kHz mix, anything above 22.05 kHz is not in any stem; the sum-to-mix figure includes it. With the stand-in on white-noise hi-hats this is about −30 dB; on music it is usually far smaller.
- Only the processor build runs on the Mac. A Core ML execution provider is not wired in yet, so separation speed on M1/M2 is whatever the CI job measures, not a GPU figure.
- Only the 6-stem model exists; "4 stems" today means keeping drums, bass, other and vocals. A dedicated 4-stem model is not added.

### Stem mixer

The stems card now plays every stem together on one transport (the mix's waveform), with per-stem mute, solo, volume (0 to +6 dB) and pan, and a loop region set by seconds or from the playhead (`StemMixer.tsx`, logic in `services/stemMix.ts` with 4 unit tests). Each stem is an audio element through a gain and a stereo panner; the others follow the first and are re-seeked past 30 ms of drift. Looping is by seeking, so the seam is about a screen frame, not sample-accurate: it is for auditioning, and nothing set here changes the stem files.

Browser check (Chromium, real service, the stand-in stems): after 2.5 s all six elements read 2.525 s; with a 5.0–6.0 s loop the transport stayed between 5.00 and 5.95 s and wrapped three times in 3 s. Mute/solo/pan state shows correctly; their effect on the audio graph is covered by the unit tests of the gain rules, not measured from the output.

## Phase 5 results so far (`kinetic-studio/phase-5`)

MIDI for a DAW, every file named for where its notes come from.

| Item | Status | Evidence |
|---|---|---|
| Score MIDI per part | The song's own score (the one it was generated with, from its generation settings or replay request) written as the full score, or one line of it: Lead (`Ins`), Voice line (`Vocal`), Chords. Each file keeps the conductor track: tempo, meter, key and section markers. Parts with no notes are not offered; nothing is generated for a song made without a score (`kinetic_midi.rs`, `score/export.rs::midi_with`) | 5 unit tests: every lead note at its score tick and length (480 PPQ), chords held bar by bar, tempo exactly 60e6/BPM µs, meter, key and marker read back with the studio's SMF reader; a part keeps only its track; an unknown part is refused |
| Independent check | The files from the running service read with `mido` (a separate MIDI library): format 1, 480 PPQ, 92 BPM, 4/4, D minor, marker `intro`, lead D4 F4 A4 D5 C5 Bb4 at ticks 0/480/960/1440/1920/2880, chords Dm at 0 and Bb at 1920, length 5.217 s (8 beats at 92 BPM) | |
| Labels | File names end in `(from score)`, `audio to MIDI (estimated)` or `MIDI (edited)`. The Export dialog's MIDI section says the score MIDI is what the model was asked to play and that how closely the recording follows it is not measured; audio-to-MIDI is called an estimate with wrong and missing notes to be expected | Browser check of the Export dialog |
| Not done | The recording's timing is not compared with the score (no beat tracking runs here; the tempo model is on Hugging Face, which this container cannot reach). Audio-to-MIDI on the Mac still needs the `ace-midi` build checked there. A bass line is not derived from the chords: it would be new notes, not the score's | |

## Phase 6 results so far (`kinetic-studio/phase-6`)

A song as an Ableton Live 12 project. Nothing here is written from a guess at the `.als` format: Jared saved two sets in Live Lite 12.4.6 on his Mac, an empty one (`assets/kinetic/live12-default.als`) and one with an audio clip and a MIDI clip in Arrangement view. The second set is not committed: it holds his home folder path and sample names. Only its two clips are kept (`live12-audio-clip.xml`, `live12-midi-clip.xml`), with that set's sample name, paths, size, checksum, date, warp markers, notes and clip envelopes removed.

| Item | Status | Evidence |
|---|---|---|
| Set from Live's own set | The empty set's MIDI and audio tracks are copied once per planned track, renamed, coloured and given fresh ids. Automation and modulation targets share one counter for the whole set (`NextPointeeId`), and the copies get new ids from it. Tempo and time signature are set on the main track (Live stores 4/4 as 201: 99 × log2(denominator) + numerator − 1). The returns, the main track and Live's settings are left exactly as Live wrote them (`ableton.rs`) | 8 unit tests: track order and names; each copied track has exactly the template track's elements; set-wide ids are unique and below `NextPointeeId`; tempo and meter values; names are escaped; tempos outside Live's 20–999 range are refused |
| Clips at bar 1 | Each track holds its file as a clip in the empty event list where Jared's set kept its clips. Those lists are `MainSequencer/ClipTimeable` on a MIDI track and `MainSequencer/Sample` on an audio track. Apart from those lists and the remembered first clip name, the two sets differ only in view state. Audio clips use the sample's path from the project folder (`Samples/Imported/…`). Their length in beats is frames ÷ rate × BPM ÷ 60. They are warped with a marker at the first and the last frame and play in Re-Pitch, so at the set's tempo they play at their own speed. Looping is off. MIDI clips hold every note of their file in quarter notes, grouped by key the way Live groups them | A unit test places one of each and reads the timing back. An ignored test (`clips_match_a_set_live_saved`, run with `KINETIC_LIVE_CLIP_SET`) compares our clips with the clips in Jared's set: same elements, same position in the track. **It passes against his set here** |
| Project folder | `POST /v1/kinetic/ableton` writes `"<Title> Project.zip"` to the studio's exports folder, choosing a new name each time so an earlier export is never replaced. The zip holds `<Title>.als`, `Samples/Imported/<Title> - Mix.wav`, one `<Title> - <Stem> (separated).wav` per separated stem, one `MIDI/<Title> - <Part> (from score).mid` per score line, and `READ ME.txt`. Audio is copied byte for byte. Without a score the set is at 120 BPM in 4/4, and both the read-me and the dialog say that is not the song's tempo | The scratch song (20 s, 48 kHz, 6 stems, 92 BPM score) checked with Python's own `wave` and `mido`: all 7 audio clips end at 30.667 beats = 960,000 frames at 48 kHz at 92 BPM, and every sample path exists in the zip. Both MIDI clips hold exactly the notes `mido` reads from the `.mid` beside them |
| Export dialog | A "Save as Live project (.zip)" button under MIDI. It builds the project and saves it, then says what went in. It states that the set has not yet been checked by opening it in Live | Browser check: the dialog saved `Night drive Project (2).zip` (11 files) and showed "the mix, 6 stems and 2 MIDI parts at 92 BPM in 4/4" |
| **Not validated** | No set made here has been opened in Live. These choices follow the format but are not proven: Re-Pitch as warp mode `3`, an empty absolute `Path` (Live is expected to find samples by `RelativePath`), and `OriginalCrc` left at 0 because Live's checksum is not known. Until Jared opens a generated project in Live 12 on his Mac, Ableton compatibility is not claimed. The MIDI tracks have no instrument, as in Live's empty set | Needs Jared's Mac |
