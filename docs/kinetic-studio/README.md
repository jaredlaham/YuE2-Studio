# Kinetic Studio

Kinetic Studio is YuE2 Studio built under its own name for Apple Silicon Macs,
the base for an instrumental electronic music workflow (generate, edit, stems,
export, Ableton). See [AUDIT.md](AUDIT.md) for what exists, what is verified
and the roadmap.

## Get the app

Every push to a `kinetic-studio/**` branch builds it on GitHub's Apple Silicon
runner (`.github/workflows/kinetic-macos.yml`). Download the
`kinetic-studio-macos-arm64` artifact from the run, unzip
`Kinetic-Studio-macos-arm64.app.zip` (or open the `.dmg`) and drag
**Kinetic Studio.app** to Applications.

The build is ad-hoc signed and not notarized, so macOS blocks the first open
with "“Kinetic Studio” Not Opened … Apple could not verify…". On current macOS
(15 Sequoia and later) right-click → **Open** no longer gets past it. Either:

- Press **Done**, open **System Settings → Privacy & Security**, scroll to
  "Kinetic Studio was blocked…", press **Open Anyway** and confirm with your
  password; or
- run once in Terminal:

  ```bash
  xattr -dr com.apple.quarantine "/Applications/Kinetic Studio.app"
  ```

Opening it without this step needs a Developer ID certificate and Apple
notarization, which takes a paid Apple Developer Program membership
(US$99 a year); the project does not have one.

## Build it yourself

Needs Xcode command line tools, CMake, Ninja, Node 22 and Rust (stable):

```bash
scripts/build-kinetic-macos.sh
# desktop/src-tauri/target/release/bundle/macos/Kinetic Studio.app
```

## Where things live

- App data, library, models, settings: `~/Library/Application Support/Kinetic Studio`
- Model weights are never bundled. The first screen offers a model set; nothing
  downloads until you press download. On 16 GB, the Q8_0 set (5.1 GB) is the
  suggested starting point. See "Speed on Apple Silicon" below for a measured run.

## Speed on Apple Silicon

One measured run, Jared's MacBook with an M1 Pro, 16 GB, Q8_0 set, every stage
on Metal (`MTL0` in the engine log), default settings (midpoint solver, 32
steps), Dark Cinematic Ambient preset with Max duration raised above the song's length. The song came out
at 5:12 (312 s) and took 33.6 minutes:

| Stage | What it does | Time | Share |
|---|---|---|---|
| Load the language model | | 3.3 s | |
| Score | writes the ABC score, 2,351 tokens at 18 ms each | 42 s | 2 % |
| Semantic | writes the audio codes, 7,805 tokens (25 a second of music) at 20 ms each | 157 s | 8 % |
| Acoustic (NAR) | 32 midpoint steps, two network passes each, over all 7,805 frames at once | 1,734 s | 86 % |
| Decode (VAE) | latents to 48 kHz stereo, 16 tiles | 77 s | 4 % |

The acoustic stage is where the time goes. Each pass attends over the whole
song (7,805 frames plus the 2,834-token prompt and score), so its cost grows
faster than the song's length: a song half as long takes much less than half
the time there. What shortens it, in Create → Quality:

- **Solver: AB2.** 33 network passes instead of 64 for 32 steps, so about half
  the acoustic time. The engine's authors measured its sound against midpoint
  (mel spectra correlated at 0.9996 on a 122 s song at 16 steps).
- **Fewer flow-matching steps**, for example 16 instead of 32: half again. The
  engine's own comparison above was run at 16 steps; how it sounds against 32
  has not been checked on this build.
- **A shorter Max duration.** The default ceiling is 130 s; the model ends a
  song when it judges it finished, up to 6:00.

Estimated, not measured: the same 5:12 song with AB2 and 16 steps would spend
about 7.5 minutes in the acoustic stage and about 12.5 minutes in all.

## Differences from YuE2 Studio

- Own name, bundle id (`com.kineticfields.kinetic-studio`) and data folder, so it
  can sit beside an installed YuE2 Studio (but not run at the same time: both
  use port 8791).
- The upstream auto-updater is off: it would install YuE2 Studio over it.
- Built with `--features kinetic` and `src-tauri/tauri.kinetic-macos.conf.json`;
  the upstream Windows build is unchanged.

## Using a GPU on another computer

A 16 GB Mac can run the quantised YuE2 sets with Metal, slowly. To use an NVIDIA machine instead, run `yue-server`
there and point Kinetic Studio at it; this Mac then starts no engine of its own and needs no model files.

1. On the GPU computer, build or install `yue2.cpp`'s `yue-server` with its model files, and start it on loopback only:
   `yue-server --model … --vae … --companion … --host 127.0.0.1 --port 18087`.
2. On the Mac, open a tunnel and leave it running: `ssh -N -L 18187:127.0.0.1:18087 you@gpu-computer`.
3. In Kinetic Studio: first-run page → "Use a music engine on another computer instead", or Settings → Engine.
   Enter `http://127.0.0.1:18187`, press **Test connection**, then **Use this engine**, and quit and reopen the app.

`yue-server` has no password and no encryption, which is why the tunnel matters; an address that leaves your
computer is accepted but warned about. LoRA adapters and the model set are whatever that engine was started with.
Running a cloud GPU costs money by the hour: that is your call, the app never starts one.

## Smoke test

`scripts/kinetic-smoke.sh <studio executable> [expected path fragment]` launches
the real app, imports a 48 kHz/24-bit WAV, checks the library lists it and
plays back the identical bytes, restarts and checks it persisted.
