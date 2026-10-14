#!/usr/bin/env python3
"""Measure songs side by side: fidelity, loudness and dynamics, stereo image, timbre,
structure and how each one ends, plus null tests against a reference.

    python analyze.py runs/A-t1/audio.flac runs/B-t1/audio-0.flac runs/Breplay-t1/audio.wav \
        --labels A B B-replay --reference A --out runs/report-t1

Writes report.json and report.md. Needs numpy and ffmpeg/ffprobe on PATH; nothing else.
The numbers locate a difference; listening decides whether it matters (see README.md).
"""

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

import numpy as np

RATE = 48_000


def decode(path: Path) -> np.ndarray:
    """Float32 [frames, 2] at 48 kHz: mono is doubled, other rates are resampled by ffmpeg."""
    raw = subprocess.run(["ffmpeg", "-v", "error", "-i", str(path), "-f", "f32le", "-acodec", "pcm_f32le",
                          "-ac", "2", "-ar", str(RATE), "-"], check=True, capture_output=True).stdout
    return np.frombuffer(raw, dtype="<f4").reshape(-1, 2).astype(np.float64)


def probe(path: Path) -> dict:
    out = subprocess.run(["ffprobe", "-v", "error", "-select_streams", "a:0", "-show_entries",
                          "stream=codec_name,sample_rate,channels,bits_per_raw_sample,bits_per_sample,bit_rate",
                          "-of", "json", str(path)], check=True, capture_output=True, text=True).stdout
    stream = json.loads(out)["streams"][0]
    return {"codec": stream.get("codec_name"), "rate": int(stream.get("sample_rate", 0)),
            "channels": stream.get("channels"),
            "bits": stream.get("bits_per_raw_sample") or stream.get("bits_per_sample"),
            "bit_rate": stream.get("bit_rate")}


def ebur128(path: Path) -> dict:
    log = subprocess.run(["ffmpeg", "-nostats", "-hide_banner", "-i", str(path), "-filter_complex",
                          "ebur128=peak=true", "-f", "null", "-"], capture_output=True, text=True).stderr
    summary = log[log.rfind("Summary:"):]

    def grab(name):
        found = re.search(rf"{name}:\s+(-?[\d.]+|-inf)", summary)
        return None if not found or found.group(1) == "-inf" else float(found.group(1))
    return {"integrated_lufs": grab("I"), "loudness_range_lu": grab("LRA"), "true_peak_dbtp": grab("Peak")}


def db(value, floor=1e-12):
    return float(20 * np.log10(max(float(value), floor)))


def frames(signal: np.ndarray, size: int, hop: int) -> np.ndarray:
    count = max(1, 1 + (len(signal) - size) // hop)
    index = np.arange(size)[None, :] + hop * np.arange(count)[:, None]
    padded = np.pad(signal, (0, max(0, index.max() + 1 - len(signal))))
    return padded[index]


def spectrogram(mono: np.ndarray, size=4096, hop=2048) -> np.ndarray:
    window = np.hanning(size)
    return np.abs(np.fft.rfft(frames(mono, size, hop) * window, axis=1)) ** 2


def mel_bank(bins: int, size: int, bands=40, low=30.0, high=16000.0) -> np.ndarray:
    mel = lambda hz: 2595 * np.log10(1 + hz / 700)
    edges = 700 * (10 ** (np.linspace(mel(low), mel(high), bands + 2) / 2595) - 1)
    freqs = np.fft.rfftfreq(size, 1 / RATE)[:bins]
    bank = np.zeros((bands, bins))
    for b in range(bands):
        lo, mid, hi = edges[b:b + 3]
        bank[b] = np.clip(np.minimum((freqs - lo) / (mid - lo), (hi - freqs) / (hi - mid)), 0, None)
    return bank


def novelty_boundaries(features: np.ndarray, seconds_per_frame: float, kernel_seconds=8.0) -> list:
    """Foote novelty over a cosine self-similarity matrix: where the music changes character."""
    unit = features / (np.linalg.norm(features, axis=1, keepdims=True) + 1e-9)
    similarity = unit @ unit.T
    half = max(2, int(kernel_seconds / seconds_per_frame / 2))
    sign = np.r_[-np.ones(half), np.ones(half)]
    kernel = np.outer(sign, sign) * np.outer(np.hanning(2 * half), np.hanning(2 * half))
    n = len(similarity)
    curve = np.zeros(n)
    for i in range(half, n - half):
        curve[i] = np.sum(kernel * similarity[i - half:i + half, i - half:i + half])
    if not curve.any():
        return []
    curve = np.clip(curve, 0, None)
    threshold = curve.mean() + curve.std()
    peaks = [i for i in range(1, n - 1) if curve[i] > threshold and curve[i] >= curve[i - 1] and curve[i] >= curve[i + 1]]
    kept = []
    for i in peaks:  # at least one kernel apart
        if not kept or (i - kept[-1]) * seconds_per_frame >= kernel_seconds:
            kept.append(i)
    return [round(i * seconds_per_frame, 1) for i in kept]


def band_stereo(left: np.ndarray, right: np.ndarray, size=4096, hop=4096) -> dict:
    """Side/mid energy ratio and L/R correlation in low, mid and high bands."""
    window = np.hanning(size)
    fl = np.fft.rfft(frames(left, size, hop) * window, axis=1)
    fr = np.fft.rfft(frames(right, size, hop) * window, axis=1)
    freqs = np.fft.rfftfreq(size, 1 / RATE)
    result = {}
    for name, lo, hi in (("low_0_150", 0, 150), ("mid_150_4k", 150, 4000), ("high_4k_up", 4000, RATE / 2)):
        sel = (freqs >= lo) & (freqs < hi)
        l, r = fl[:, sel], fr[:, sel]
        mid, side = (l + r) / 2, (l - r) / 2
        cross = np.real(np.sum(l * np.conj(r)))
        corr = cross / (np.sqrt(np.sum(np.abs(l) ** 2) * np.sum(np.abs(r) ** 2)) + 1e-12)
        ratio = np.sum(np.abs(side) ** 2) / (np.sum(np.abs(mid) ** 2) + 1e-12)
        result[name] = {"lr_correlation": round(float(corr), 3), "side_to_mid_db": round(db(np.sqrt(ratio)), 1)}
    return result


def measure(path: Path) -> dict:
    audio = decode(path)
    left, right = audio[:, 0], audio[:, 1]
    mono = audio.mean(axis=1)
    seconds = len(audio) / RATE
    peak = float(np.max(np.abs(audio))) if len(audio) else 0.0
    rms = float(np.sqrt(np.mean(audio ** 2))) if len(audio) else 0.0

    spec = spectrogram(mono)
    freqs = np.fft.rfftfreq(4096, 1 / RATE)
    power = spec.sum(axis=1) + 1e-20
    centroid = (spec * freqs).sum(axis=1) / power
    cumulative = np.cumsum(spec, axis=1) / power[:, None]
    rolloff = freqs[np.argmax(cumulative >= 0.95, axis=1)]
    loud = power > np.percentile(power, 20)  # skip near-silence when averaging timbre
    above16 = spec[:, freqs >= 16000].sum(axis=1) / power
    flatness = np.exp(np.mean(np.log(spec[:, 1:] + 1e-20), axis=1)) / (np.mean(spec[:, 1:], axis=1) + 1e-20)
    norm_spec = spec / power[:, None]
    flux = np.sqrt(np.sum(np.diff(np.sqrt(norm_spec), axis=0) ** 2, axis=1))

    # macro dynamics: 3 s windows; transitions: 4 s windows that move 3 dB or more
    short = frames(mono, 3 * RATE, RATE)
    short_db = np.array([db(np.sqrt(np.mean(w ** 2))) for w in short])
    short_db = short_db[short_db > -70]
    blocks = frames(mono, 4 * RATE, 4 * RATE)
    block_db = np.array([db(np.sqrt(np.mean(w ** 2))) for w in blocks])
    moves = int(np.sum(np.abs(np.diff(block_db)) >= 3.0)) if len(block_db) > 1 else 0

    mel = np.log(spec @ mel_bank(spec.shape[1], 4096).T + 1e-10)
    per_second = RATE / 2048
    pooled = np.array([mel[i:i + int(per_second)].mean(axis=0) for i in range(0, len(mel), int(per_second))])
    boundaries = novelty_boundaries(pooled - pooled.mean(axis=0), 1.0) if len(pooled) > 16 else []

    # how it ends: the last second against the body of the song, and whether it fades
    tail = mono[-RATE:] if seconds > 2 else mono
    body_db = float(np.median(short_db)) if len(short_db) else -120.0
    last_db = db(np.sqrt(np.mean(tail ** 2)))
    fade = [db(np.sqrt(np.mean(mono[-(k + 1) * RATE // 2: len(mono) - k * RATE // 2] ** 2))) for k in range(6, 0, -1)] if seconds > 4 else []
    abrupt = bool(last_db > body_db - 6 and (not fade or fade[-1] > fade[0] - 3))

    return {
        "file": str(path),
        "format": probe(path),
        "seconds": round(seconds, 2),
        "loudness": ebur128(path),
        "peak_dbfs": round(db(peak), 2),
        "rms_dbfs": round(db(rms), 2),
        "crest_db": round(db(peak) - db(rms), 2),
        "clipped_samples": int(np.sum(np.abs(audio) >= 0.9999)),
        "dc_offset": [round(float(left.mean()), 6), round(float(right.mean()), 6)],
        "stereo": {"lr_correlation": round(float(np.corrcoef(left, right)[0, 1]), 3) if np.std(left) > 0 and np.std(right) > 0 else 1.0,
                   "bands": band_stereo(left, right)},
        "timbre": {"centroid_hz": round(float(np.mean(centroid[loud])), 0),
                   "rolloff95_hz": round(float(np.median(rolloff[loud])), 0),
                   "energy_above_16k_pct": round(float(np.mean(above16[loud]) * 100), 4),
                   "flatness": round(float(np.mean(flatness[loud])), 4),
                   "spectral_flux": round(float(np.mean(flux)), 4)},
        "dynamics": {"short_term_spread_db": round(float(np.percentile(short_db, 90) - np.percentile(short_db, 10)), 2) if len(short_db) else 0.0,
                     "short_term_std_db": round(float(np.std(short_db)), 2) if len(short_db) else 0.0,
                     "energy_moves_3db_per_min": round(moves / max(seconds / 60, 1e-9), 2)},
        "structure": {"boundaries_s": boundaries, "sections": len(boundaries) + 1,
                      "mean_section_s": round(seconds / (len(boundaries) + 1), 1)},
        "ending": {"last_second_db": round(last_db, 1), "body_median_db": round(body_db, 1),
                   "fade_db_per_half_second": [round(v, 1) for v in fade], "ends_abruptly": abrupt},
    }


def align(reference: np.ndarray, test: np.ndarray, search_seconds=2.0) -> int:
    """Lag (in samples) of test against reference, from the first 30 s of their mono mixes."""
    a = reference[:30 * RATE].mean(axis=1)
    b = test[:30 * RATE].mean(axis=1)
    n = 1 << int(np.ceil(np.log2(len(a) + len(b))))
    corr = np.fft.irfft(np.fft.rfft(a, n) * np.conj(np.fft.rfft(b, n)), n)
    limit = int(search_seconds * RATE)
    lags = np.r_[np.arange(0, limit), np.arange(-limit, 0)]
    values = np.r_[corr[:limit], corr[-limit:]]
    return int(lags[np.argmax(values)])


def null_test(reference_path: Path, test_path: Path) -> dict:
    ref, test = decode(reference_path), decode(test_path)
    lag = align(ref, test)
    if lag > 0:
        test = np.pad(test, ((lag, 0), (0, 0)))
    elif lag < 0:
        test = test[-lag:]
    n = min(len(ref), len(test))
    r, t = ref[:n], test[:n]
    gain = float(np.sum(r * t) / (np.sum(t * t) + 1e-20))  # best level match, reported
    residual = r - t
    matched = r - gain * t
    ref_rms = np.sqrt(np.mean(r ** 2)) + 1e-20
    rs, ts = spectrogram(r.mean(axis=1)), spectrogram(t.mean(axis=1))
    floor = rs.max() * 1e-10  # -100 dB under the loudest bin, so empty bands do not dominate
    lsd = float(np.mean(np.sqrt(np.mean((10 * np.log10(np.maximum(rs, floor)) - 10 * np.log10(np.maximum(ts, floor))) ** 2, axis=1))))
    return {
        "reference": str(reference_path), "test": str(test_path),
        "lag_samples": lag, "compared_seconds": round(n / RATE, 2),
        "length_difference_s": round((len(decode(test_path)) - len(ref)) / RATE, 2),
        "bit_identical": bool(lag == 0 and len(ref) == len(decode(test_path)) and np.array_equal(r, t)),
        "residual_db_rel_ref": round(db(np.sqrt(np.mean(residual ** 2)) / ref_rms), 2),
        "residual_after_gain_match_db": round(db(np.sqrt(np.mean(matched ** 2)) / ref_rms), 2),
        "gain_match_db": round(db(abs(gain)), 3),
        "log_spectral_distance_db": round(lsd, 2),
    }


def markdown(results: list, labels: list, nulls: list) -> str:
    rows = [
        ("Length (s)", lambda m: m["seconds"]),
        ("Format", lambda m: f"{m['format']['codec']} {m['format']['rate']} Hz {m['format']['bits'] or ''}".strip()),
        ("Integrated loudness (LUFS)", lambda m: m["loudness"]["integrated_lufs"]),
        ("Loudness range (LU)", lambda m: m["loudness"]["loudness_range_lu"]),
        ("True peak (dBTP)", lambda m: m["loudness"]["true_peak_dbtp"]),
        ("Crest factor (dB)", lambda m: m["crest_db"]),
        ("Clipped samples", lambda m: m["clipped_samples"]),
        ("Short-term spread P10-P90 (dB)", lambda m: m["dynamics"]["short_term_spread_db"]),
        ("Energy moves >= 3 dB per min", lambda m: m["dynamics"]["energy_moves_3db_per_min"]),
        ("L/R correlation", lambda m: m["stereo"]["lr_correlation"]),
        ("Side/mid, highs (dB)", lambda m: m["stereo"]["bands"]["high_4k_up"]["side_to_mid_db"]),
        ("Side/mid, lows (dB)", lambda m: m["stereo"]["bands"]["low_0_150"]["side_to_mid_db"]),
        ("Spectral centroid (Hz)", lambda m: m["timbre"]["centroid_hz"]),
        ("95% roll-off (Hz)", lambda m: m["timbre"]["rolloff95_hz"]),
        ("Energy above 16 kHz (%)", lambda m: m["timbre"]["energy_above_16k_pct"]),
        ("Spectral flux", lambda m: m["timbre"]["spectral_flux"]),
        ("Sections found", lambda m: m["structure"]["sections"]),
        ("Mean section (s)", lambda m: m["structure"]["mean_section_s"]),
        ("Ends abruptly", lambda m: "yes" if m["ending"]["ends_abruptly"] else "no"),
    ]
    lines = ["| Measure | " + " | ".join(labels) + " |", "|---" * (len(labels) + 1) + "|"]
    for name, get in rows:
        lines.append(f"| {name} | " + " | ".join(str(get(m)) for m in results) + " |")
    lines.append("")
    for label, m in zip(labels, results):
        lines.append(f"- **{label}** section boundaries (s): {m['structure']['boundaries_s']}")
    if nulls:
        lines += ["", "| Null test | Lag (samples) | Bit identical | Residual (dB) | Residual after gain match (dB) | Gain (dB) | Log-spectral distance (dB) |",
                  "|---|---|---|---|---|---|---|"]
        for label, n in nulls:
            lines.append(f"| {label} | {n['lag_samples']} | {'yes' if n['bit_identical'] else 'no'} | {n['residual_db_rel_ref']} | "
                         f"{n['residual_after_gain_match_db']} | {n['gain_match_db']} | {n['log_spectral_distance_db']} |")
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("files", nargs="+", type=Path)
    parser.add_argument("--labels", nargs="+")
    parser.add_argument("--reference", help="label to null-test the others against")
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    labels = args.labels or [path.stem for path in args.files]
    if len(labels) != len(args.files):
        sys.exit("give one label per file")
    results = [measure(path) for path in args.files]
    nulls = []
    if args.reference:
        ref = args.files[labels.index(args.reference)]
        nulls = [(f"{label} vs {args.reference}", null_test(ref, path)) for label, path in zip(labels, args.files) if path != ref]
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / "report.json").write_text(json.dumps({"labels": labels, "measures": results,
                                                      "null_tests": dict(nulls)}, indent=2), encoding="utf-8")
    (args.out / "report.md").write_text(markdown(results, labels, nulls), encoding="utf-8")
    print(markdown(results, labels, nulls))


if __name__ == "__main__":
    main()
