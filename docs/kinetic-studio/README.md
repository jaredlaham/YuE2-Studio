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

The build is ad-hoc signed and not notarized, so the first open needs
right-click → **Open** → **Open**. If macOS still refuses ("damaged"), run once:

```bash
xattr -dr com.apple.quarantine "/Applications/Kinetic Studio.app"
```

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
  suggested starting point; its speed on M1/M2 has not been measured yet.

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
