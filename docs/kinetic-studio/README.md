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

## Smoke test

`scripts/kinetic-smoke.sh <studio executable> [expected path fragment]` launches
the real app, imports a 48 kHz/24-bit WAV, checks the library lists it and
plays back the identical bytes, restarts and checks it persisted.
