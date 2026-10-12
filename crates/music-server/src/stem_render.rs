//! Stems shaped to drop into a DAW beside the mix they came from.
//!
//! The separation model works at 44.1 kHz. A stem a producer lines up against
//! the original has to match it exactly: the mix's own sample rate, its exact
//! length, starting at its first sample. Each stem is brought back to the
//! mix's rate, cut or padded to its frame count, and written as 24-bit PCM
//! with TPDF dither. How closely the stems add back up to the mix is measured
//! and reported, because separation is an estimate: the stems are the model's
//! guess at the parts, never the parts the track was made from.

use std::path::Path;

use anyhow::{Context, Result};
use audio_post::Stereo;
use serde::Serialize;

use crate::separation::{Stem, SAMPLE_RATE};

/// The bit depth stems are written at.
pub const BITS: u32 = 24;

/// One stem at the mix's rate and length.
pub struct Fitted {
    pub name: &'static str,
    pub audio: Stereo,
}

/// What the written stems are and how well they add back up to the mix.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StemReport {
    pub sample_rate: u32,
    pub bits: u32,
    /// Every stem has exactly this many frames, the mix's own count.
    pub frames: usize,
    /// The level of (sum of every stem - mix) against the mix, in dB: how much
    /// of the mix the stems do not account for. None when the mix is silent.
    pub sum_error_db: Option<f64>,
    /// The largest single-sample difference between the stems' sum and the mix, in dBFS.
    pub sum_peak_error_dbfs: f64,
    /// Whether every stem the model produces was summed (a partial set cannot add up to the mix).
    pub all_stems: bool,
}

fn deinterleave(samples: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let frames = samples.len() / 2;
    let mut left = Vec::with_capacity(frames);
    let mut right = Vec::with_capacity(frames);
    for frame in samples.chunks_exact(2) {
        left.push(frame[0]);
        right.push(frame[1]);
    }
    (left, right)
}

/// Brings 44.1 kHz stems to the mix's rate and exact length.
pub fn fit_to_mix(stems: Vec<Stem>, mix_rate: u32, mix_frames: usize) -> Result<Vec<Fitted>> {
    stems
        .into_iter()
        .map(|stem| {
            let (left, right) = deinterleave(&stem.samples);
            let mut audio = Stereo::new(left, right, SAMPLE_RATE);
            if mix_rate != SAMPLE_RATE {
                audio = audio.resampled(mix_rate).with_context(|| format!("bring the {} stem to {mix_rate} Hz", stem.name))?;
            }
            audio.left.resize(mix_frames, 0.0);
            audio.right.resize(mix_frames, 0.0);
            Ok(Fitted { name: stem.name, audio })
        })
        .collect()
}

fn to_db(value: f64) -> f64 {
    if value > 0.0 { 20.0 * value.log10() } else { f64::NEG_INFINITY }
}

/// Measures how the stems add back up to the mix. Both must be at the same rate and length.
pub fn report(stems: &[Fitted], mix: &Stereo, all_stems: bool) -> StemReport {
    let frames = mix.frames();
    let (mut error_energy, mut mix_energy, mut peak) = (0.0f64, 0.0f64, 0.0f64);
    for frame in 0..frames {
        for (channel, original) in [(0, mix.left[frame]), (1, mix.right[frame])] {
            let sum: f64 = stems
                .iter()
                .map(|stem| if channel == 0 { stem.audio.left[frame] } else { stem.audio.right[frame] } as f64)
                .sum();
            let difference = sum - original as f64;
            error_energy += difference * difference;
            mix_energy += (original as f64) * (original as f64);
            peak = peak.max(difference.abs());
        }
    }
    StemReport {
        sample_rate: mix.rate,
        bits: BITS,
        frames,
        sum_error_db: (mix_energy > 0.0).then(|| 10.0 * (error_energy.max(1e-30) / mix_energy).log10()),
        sum_peak_error_dbfs: to_db(peak),
        all_stems,
    }
}

/// Writes a stem as 24-bit PCM WAV with TPDF dither, through a `.part` file
/// so a stopped run never leaves half a stem under the real name.
pub fn write(path: &Path, audio: &Stereo, seed: u64) -> Result<()> {
    let spec = hound::WavSpec { channels: 2, sample_rate: audio.rate, bits_per_sample: BITS as u16, sample_format: hound::SampleFormat::Int };
    let partial = path.with_extension("wav.part");
    let mut out = hound::WavWriter::create(&partial, spec).with_context(|| format!("create {}", partial.display()))?;
    for sample in audio_post::dither::quantize(audio, BITS, true, seed) {
        out.write_sample(sample)?;
    }
    out.finalize().context("finish writing the stem")?;
    std::fs::rename(&partial, path).with_context(|| format!("publish {}", path.display()))?;
    Ok(())
}

/// A repeatable dither seed per stem, so writing the same stems twice gives the same files.
pub fn seed_for(song_id: &str, stem: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in song_id.bytes().chain([0]).chain(stem.bytes()) {
        hash = (hash ^ byte as u64).wrapping_mul(0x100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, frames: usize, frequency: f64, amplitude: f64) -> Vec<f32> {
        (0..frames).map(|n| (amplitude * (2.0 * std::f64::consts::PI * frequency * n as f64 / rate as f64).sin()) as f32).collect()
    }

    fn interleave(left: &[f32], right: &[f32]) -> Vec<f32> {
        left.iter().zip(right).flat_map(|(l, r)| [*l, *r]).collect()
    }

    #[test]
    fn stems_take_the_mix_rate_and_exact_length() {
        for (rate, frames) in [(48_000u32, 48_000 * 3 + 17), (44_100, 44_100 * 2 + 5), (96_000, 96_000 + 1)] {
            // what the model would hand back: 44.1 kHz, a little longer or shorter than the mix
            let made = (frames as f64 * SAMPLE_RATE as f64 / rate as f64).round() as usize + 3;
            let samples = interleave(&tone(SAMPLE_RATE, made, 440.0, 0.25), &tone(SAMPLE_RATE, made, 440.0, 0.25));
            let fitted = fit_to_mix(vec![Stem { name: "drums", samples }], rate, frames).unwrap();
            assert_eq!(fitted[0].audio.rate, rate);
            assert_eq!(fitted[0].audio.frames(), frames);
        }
    }

    #[test]
    fn resampling_keeps_the_stem_in_time_with_the_mix() {
        // a click at a known moment must land at the same moment at 48 kHz
        let at_seconds = 1.0;
        let mut samples = vec![0f32; SAMPLE_RATE as usize * 2 * 2];
        let click = (at_seconds * SAMPLE_RATE as f64) as usize;
        samples[click * 2] = 0.9;
        samples[click * 2 + 1] = 0.9;
        let fitted = fit_to_mix(vec![Stem { name: "drums", samples }], 48_000, 96_000).unwrap();
        let left = &fitted[0].audio.left;
        let loudest = (0..left.len()).max_by(|a, b| left[*a].abs().total_cmp(&left[*b].abs())).unwrap();
        let expected = (at_seconds * 48_000.0) as isize;
        assert!((loudest as isize - expected).abs() <= 1, "the click moved from {expected} to {loudest}");
    }

    #[test]
    fn the_report_measures_how_the_stems_add_up() {
        let rate = 48_000;
        let frames = rate as usize;
        let low = tone(rate, frames, 110.0, 0.3);
        let high = tone(rate, frames, 3_000.0, 0.2);
        let mix_left: Vec<f32> = low.iter().zip(&high).map(|(a, b)| a + b).collect();
        let mix = Stereo::new(mix_left.clone(), mix_left, rate);
        let parts = vec![
            Fitted { name: "bass", audio: Stereo::new(low.clone(), low.clone(), rate) },
            Fitted { name: "other", audio: Stereo::new(high.clone(), high.clone(), rate) },
        ];
        let exact = report(&parts, &mix, true);
        assert!(exact.sum_error_db.unwrap() < -100.0, "{exact:?}");
        assert_eq!((exact.sample_rate, exact.bits, exact.frames), (rate, 24, frames));
        // leave one part out: the error is that part's share of the mix
        let missing = report(&parts[..1], &mix, false);
        let share = 10.0 * (0.2f64.powi(2) / (0.3f64.powi(2) + 0.2f64.powi(2))).log10();
        assert!((missing.sum_error_db.unwrap() - share).abs() < 0.1, "{missing:?} vs {share}");
        assert!(!missing.all_stems);
    }

    #[test]
    fn written_stems_are_24_bit_at_the_mix_rate_and_length() {
        let dir = std::env::temp_dir().join(format!("stem-render-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("song-drums.wav");
        let left = tone(48_000, 48_001, 220.0, 0.5);
        let audio = Stereo::new(left.clone(), left, 48_000);
        write(&path, &audio, seed_for("song", "drums")).unwrap();
        let reader = hound::WavReader::open(&path).unwrap();
        let spec = reader.spec();
        assert_eq!((spec.sample_rate, spec.bits_per_sample, spec.channels), (48_000, 24, 2));
        assert_eq!(reader.duration(), 48_001);
        assert!(!dir.join("song-drums.wav.part").exists());
        // dither is under one step of 24-bit: the file decodes back within 2 LSB
        let decoded = crate::audio_pcm::decode_stereo(&path).unwrap();
        let worst = decoded.left.iter().zip(&audio.left).map(|(a, b)| (a - b).abs()).fold(0f32, f32::max);
        assert!(worst <= 2.0 / 8_388_608.0 + 1e-9, "worst {worst}");
        // and the same stem written twice is the same file
        let again = dir.join("again.wav");
        write(&again, &audio, seed_for("song", "drums")).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), std::fs::read(&again).unwrap());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The real model, when one is pointed at: `KINETIC_STEMS_MODEL` (the
    /// HT-Demucs ONNX file) and `ORT_DYLIB_PATH` (the ONNX Runtime library).
    /// A 48 kHz mix of a bass line and a hi-hat pattern goes through the whole
    /// path; the stems must come back at 48 kHz, the mix's exact length, and
    /// add back up to it. Run with `cargo test -p music-server real_model -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs the separation model and ONNX Runtime on disk"]
    fn real_model_stems_line_up_with_a_48k_mix() {
        let model = std::path::PathBuf::from(std::env::var("KINETIC_STEMS_MODEL").expect("KINETIC_STEMS_MODEL"));
        assert!(std::env::var("ORT_DYLIB_PATH").is_ok(), "ORT_DYLIB_PATH must name the ONNX Runtime library");
        let rate = 48_000u32;
        let frames = rate as usize * 12 + 123;
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut noise = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
        };
        let beat = rate as usize / 4; // a hat every quarter second
        let mix: Vec<f32> = (0..frames)
            .map(|n| {
                let t = n as f64 / rate as f64;
                let bass = 0.3 * (2.0 * std::f64::consts::PI * 55.0 * t).sin();
                let since = (n % beat) as f64 / rate as f64;
                let hat = 0.2 * noise() * (-since * 60.0).exp();
                (bass + hat) as f32
            })
            .collect();
        let mix = Stereo::new(mix.clone(), mix, rate);
        let at_model = mix.resampled(SAMPLE_RATE).unwrap();
        let interleaved: Vec<f32> = at_model.left.iter().zip(&at_model.right).flat_map(|(l, r)| [*l, *r]).collect();
        let separated = crate::separation::separate(&model, &interleaved, crate::separation::STEMS.len(), 0.25, None, |_| {}).unwrap();
        let fitted = fit_to_mix(separated.stems, mix.rate, mix.frames()).unwrap();
        let report = report(&fitted, &mix, true);
        println!("{report:?}");
        for stem in &fitted {
            let energy: f64 = stem.audio.left.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / stem.audio.frames() as f64;
            println!("{:>7}: {:.1} dB", stem.name, 10.0 * energy.max(1e-20).log10());
            assert_eq!((stem.audio.rate, stem.audio.frames()), (rate, frames));
        }
        assert!(report.sum_error_db.unwrap() < -10.0, "the stems do not add up to the mix: {report:?}");
    }
}
