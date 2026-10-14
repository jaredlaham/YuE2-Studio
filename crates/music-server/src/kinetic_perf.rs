//! How long a song took, stage by stage, read from what the engine says.
//!
//! yue2.cpp times every stage of a job in its log (`src/generate.h`,
//! `src/nar.h`, `src/vae.h`, `src/model-store.cpp`, `src/pipeline.h`) and
//! opens each job with `[Server] Job <id>: …`. This module follows those lines
//! as the log streams in and keeps, per job, the engine's own numbers: model
//! loads, the score, the semantic stream (and whether its budget cut it), the
//! acoustic solve, the decode and the total. Nothing is estimated; a stage the
//! log did not report stays absent.
//!
//! The studio's side (waiting in the queue, importing and encoding the song,
//! memory while it ran) is measured where it happens and added by the caller.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use serde::Serialize;
use sysinfo::{ProcessesToUpdate, System};

/// The jobs whose numbers are kept after they finish, newest last.
const FINISHED_KEPT: usize = 16;

/// What the engine reported about one job.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct EngineRun {
    pub job_id: String,
    /// The GGML backend the engine named at load, e.g. `MTL0` or `CUDA0`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    /// Loading modules from disk or the page cache, all stages together.
    pub load_ms: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_frames: Option<u64>,
    /// The semantic stage stopped at its budget (the length asked for or the
    /// checkpoint's 9000 frames) instead of the model ending the song.
    pub truncated: bool,
    /// The acoustic stage's wall time, chunk by chunk: the solve plus swapping
    /// the backbone's halves and prefilling a replay's or a later chunk's codes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acoustic_wall_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acoustic_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acoustic_steps: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub solver: Option<String>,
    /// Network passes of the acoustic stage: 2 per step for midpoint, steps + 1 for AB2.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evaluations: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decode_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine_total_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_seconds: Option<f64>,
    /// True once `[Pipeline] Done` was read.
    pub finished: bool,
}

/// The number after `key` in `line`, up to the next space, comma or bracket.
fn number_after(line: &str, key: &str) -> Option<f64> {
    let rest = &line[line.find(key)? + key.len()..];
    let token: String = rest.trim_start().chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    token.parse().ok()
}

/// The number just before `unit` in `line` (`… 42.0 s (` → 42.0).
fn number_before(line: &str, unit: &str) -> Option<f64> {
    let end = line.find(unit)?;
    let head = line[..end].trim_end();
    let start = head.rfind(|c: char| !(c.is_ascii_digit() || c == '.')).map_or(0, |at| at + 1);
    head[start..].parse().ok()
}

fn add(slot: &mut Option<f64>, value: f64) {
    *slot = Some(slot.unwrap_or(0.0) + value);
}

impl EngineRun {
    /// Reads one line of this job's log into its numbers.
    fn read(&mut self, line: &str) {
        if let Some(rest) = line.strip_prefix("[Load] ") {
            // "[Load] LM backend: MTL0 (shared)" / "[Load] NAR backend: CPU (CPU threads: 8)"
            if self.backend.is_none() {
                if let Some(at) = rest.find("backend: ") {
                    let name: String = rest[at + 9..].chars().take_while(|c| !c.is_whitespace()).collect();
                    if !name.is_empty() {
                        self.backend = Some(name);
                    }
                }
            }
        } else if line.starts_with("[Store] Load ") {
            if let Some(ms) = number_before(line, " ms") {
                self.load_ms += ms;
            }
        } else if line.starts_with("[AR] Score prefill:") {
            if let Some(ms) = number_after(line, "prefill:") {
                add(&mut self.score_ms, ms);
            }
        } else if line.starts_with("[AR] Score:") {
            // "[AR] Score: 2351 tokens over 1 songs, 2351 steps, 42.0 s (18.0 ms/step)"
            if let Some(seconds) = number_before(line, " s (") {
                add(&mut self.score_ms, seconds * 1000.0);
            }
            self.score_tokens = number_after(line, "Score:").map(|tokens| tokens as u64);
        } else if line.starts_with("[AR] Semantic prefill:") {
            if let Some(ms) = number_after(line, "prefill:") {
                add(&mut self.semantic_ms, ms);
            }
        } else if line.starts_with("[AR] Semantic song ") {
            if line.contains("(truncated)") {
                self.truncated = true;
            }
        } else if line.starts_with("[AR] Semantic:") {
            if let Some(seconds) = number_before(line, " s (") {
                add(&mut self.semantic_ms, seconds * 1000.0);
            }
            self.semantic_frames = number_after(line, "Semantic:").map(|frames| frames as u64);
        } else if line.starts_with("[NAR] Song ") && line.contains(" chunk ") {
            // "[NAR] Song 0 chunk 1/1: 7805 frames, cache 10639 rows, 0 forwarded, 1734.1 s"
            if let Some(seconds) = line.strip_suffix(" s").and_then(|head| head.rsplit(", ").next()).and_then(|value| value.parse::<f64>().ok()) {
                add(&mut self.acoustic_wall_ms, seconds * 1000.0);
            }
        } else if line.starts_with("[NAR] Solved:") {
            // "[NAR] Solved: T_lat=7805, 1 variations, 32 steps (midpoint, 64 evaluations), 1734000 ms (54187.5 ms/step)"
            if let Some(ms) = number_before(line, " ms (") {
                add(&mut self.acoustic_ms, ms);
            }
            self.acoustic_steps = number_before(line, " steps (").map(|steps| steps as u32);
            if let Some(open) = line.find(" steps (") {
                let solver: String = line[open + 8..].chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
                if !solver.is_empty() {
                    self.solver = Some(solver);
                }
            }
            if let Some(passes) = number_before(line, " evaluations") {
                self.evaluations = Some(self.evaluations.unwrap_or(0) + passes as u32);
            }
        } else if line.starts_with("[VAE] Decoded:") || line.starts_with("[VAE] Tiled decode done:") {
            if let Some(ms) = number_before(line, " ms") {
                add(&mut self.decode_ms, ms);
            }
        } else if line.starts_with("[Pipeline] Done:") {
            // "[Pipeline] Done: 1 tracks, 312.0 s of audio in 2016.0 s (0.2x realtime)"
            self.audio_seconds = number_before(line, " s of audio");
            self.engine_total_ms = number_before(line, " s (").map(|seconds| seconds * 1000.0);
            self.finished = true;
        }
    }
}

/// The jobs the engine log has described: the one running and the last few finished.
#[derive(Debug, Default)]
pub struct RunTracker {
    current: Option<EngineRun>,
    finished: VecDeque<EngineRun>,
}

impl RunTracker {
    /// Follows one more line of the engine log.
    pub fn push(&mut self, line: &str) {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("[Server] Job ") {
            let id: String = rest.chars().take_while(|c| *c != ':' && !c.is_whitespace()).collect();
            if !id.is_empty() {
                self.close();
                self.current = Some(EngineRun { job_id: id, ..EngineRun::default() });
            }
            return;
        }
        let Some(run) = self.current.as_mut() else { return };
        run.read(line);
        if run.finished || line.contains("FATAL") || line.contains("Cancelled at") {
            self.close();
        }
    }

    fn close(&mut self) {
        if let Some(run) = self.current.take() {
            if let Some(existing) = self.finished.iter_mut().find(|kept| kept.job_id == run.job_id) {
                *existing = run;
            } else {
                self.finished.push_back(run);
            }
            while self.finished.len() > FINISHED_KEPT {
                self.finished.pop_front();
            }
        }
    }

    /// What the log said about `job_id`, finished or still running.
    pub fn run(&self, job_id: &str) -> Option<EngineRun> {
        self.current.iter().chain(self.finished.iter().rev()).find(|run| run.job_id == job_id).cloned()
    }

    /// Forgets everything: the subscription to the log was lost and starts again.
    pub fn reset(&mut self) {
        self.current = None;
    }
}

/// Memory and processor load while a job runs, sampled by the studio.
///
/// On Apple Silicon the GPU shares the machine's memory, so "unified memory in
/// use" is the system's used memory; the engine process's own resident size
/// does not include every Metal buffer, so both are kept. macOS does not tell an
/// unprivileged process how busy the GPU is, so that is never reported here.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ResourcePeaks {
    pub samples: u32,
    pub system_used_peak_mb: u64,
    pub system_total_mb: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine_rss_peak_mb: Option<u64>,
    /// Swap in use when the job started and at its highest while it ran.
    pub swap_start_mb: u64,
    pub swap_peak_mb: u64,
    /// The engine's processor use, averaged over the samples (100 = one core).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine_cpu_percent_avg: Option<f32>,
    /// The engine runs on another computer: nothing here measures it.
    pub remote_engine: bool,
}

pub struct ResourceSampler {
    system: System,
    peaks: ResourcePeaks,
    cpu_total: f32,
    cpu_samples: u32,
    last: Option<Instant>,
}

/// How often memory is sampled while a job runs.
pub const SAMPLE_EVERY: Duration = Duration::from_secs(2);

impl ResourceSampler {
    pub fn new(remote_engine: bool) -> Self {
        let mut system = System::new();
        system.refresh_memory();
        let swap = system.used_swap() / 1_048_576;
        Self {
            peaks: ResourcePeaks { swap_start_mb: swap, swap_peak_mb: swap, system_total_mb: system.total_memory() / 1_048_576, remote_engine, ..ResourcePeaks::default() },
            system,
            cpu_total: 0.0,
            cpu_samples: 0,
            last: None,
        }
    }

    /// The engine runs on another computer: this machine's numbers say nothing about it.
    pub fn is_remote(&self) -> bool {
        self.peaks.remote_engine
    }

    /// Takes a sample if the last one is at least `SAMPLE_EVERY` old.
    pub fn sample(&mut self) {
        if self.last.is_some_and(|last| last.elapsed() < SAMPLE_EVERY) {
            return;
        }
        let first = self.last.is_none();
        self.last = Some(Instant::now());
        self.system.refresh_memory();
        self.system.refresh_processes(ProcessesToUpdate::All, true);
        let peaks = &mut self.peaks;
        peaks.samples += 1;
        peaks.system_used_peak_mb = peaks.system_used_peak_mb.max(self.system.used_memory() / 1_048_576);
        peaks.swap_peak_mb = peaks.swap_peak_mb.max(self.system.used_swap() / 1_048_576);
        let engine = self.system.processes().values().find(|process| {
            let name = process.name().to_string_lossy().to_ascii_lowercase();
            name == "yue-server" || name == "yue-server.exe"
        });
        if let Some(engine) = engine {
            peaks.engine_rss_peak_mb = Some(peaks.engine_rss_peak_mb.unwrap_or(0).max(engine.memory() / 1_048_576));
            // the first reading has nothing to measure processor time against
            if !first {
                self.cpu_total += engine.cpu_usage();
                self.cpu_samples += 1;
            }
        }
    }

    pub fn finish(mut self) -> ResourcePeaks {
        if self.cpu_samples > 0 {
            self.peaks.engine_cpu_percent_avg = Some(self.cpu_total / self.cpu_samples as f32);
        }
        self.peaks
    }
}

/// The record a song keeps of how it was made, in its metadata as `performance`.
pub fn record(
    run: Option<&EngineRun>,
    peaks: Option<&ResourcePeaks>,
    submitted_at_ms: u64,
    started_at_ms: Option<u64>,
    engine_done_at_ms: u64,
    imported_at_ms: u64,
    import_ms: f64,
    settings: &serde_json::Value,
    model: Option<&str>,
    audio_seconds: Option<f64>,
) -> serde_json::Value {
    let ready_ms = imported_at_ms.saturating_sub(submitted_at_ms) as f64;
    let audio = audio_seconds.or_else(|| run.and_then(|run| run.audio_seconds));
    let steps = run.and_then(|run| run.acoustic_steps).or_else(|| settings.get("steps").and_then(serde_json::Value::as_u64).map(|steps| steps as u32));
    let solver = run.and_then(|run| run.solver.clone()).or_else(|| settings.get("solver").and_then(serde_json::Value::as_str).map(str::to_owned)).unwrap_or_else(|| "midpoint".into());
    serde_json::json!({
        "version": 1,
        "submitted_at": submitted_at_ms,
        "started_at": started_at_ms,
        "engine_done_at": engine_done_at_ms,
        "imported_at": imported_at_ms,
        // Generate pressed to a playable song in the library
        "time_to_playable_ms": ready_ms,
        "queued_ms": started_at_ms.map(|started| started.saturating_sub(submitted_at_ms) as f64),
        "import_ms": import_ms,
        "engine": run,
        "resources": peaks,
        "model": model,
        "solver": solver,
        "steps": steps.unwrap_or(32),
        "audio_seconds": audio,
        // seconds of music per second of waiting: above 1 is faster than real time
        "realtime_factor": audio.filter(|_| ready_ms > 0.0).map(|seconds| seconds / (ready_ms / 1000.0)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lines of a real job, in the formats yue2.cpp c8516db prints.
    const SONG: &str = "\
[Server] Job 4f2a: {\"style\":\"ambient\"}
[Load] LM backend: MTL0 (shared)
[Store] Load LM: 3300 ms
[AR] Score song 0: 470 tokens prefilled, 120 ms
[AR] Score prefill: 121 ms, CFG=1.00, top_k=30, budget=4096, songs=1, batch=1
[AR] Score 100/4096
[AR] Score song 0: 2351 tokens
[AR] Score: 2351 tokens over 1 songs, 2351 steps, 42.0 s (17.9 ms/step)
[AR] Semantic song 0: 2834 tokens prefilled, 900 ms
[AR] Semantic prefill: 901 ms, CFG=1.00, top_k=100, budget=9000, songs=1, batch=1
[AR] Semantic 7800/9000
[AR] Semantic song 0: 7805 tokens
[AR] Semantic: 7805 tokens over 1 songs, 7806 steps, 157.0 s (20.1 ms/step)
[Store] Load NAR: 1500 ms
[NAR] Song 0: 7805 frames (312.2 s), prefix 2834, 1 chunk of 7805, 1 variation
[NAR] Step 1/32, 54000 ms
[NAR] Solved: T_lat=7805, 1 variations, 32 steps (midpoint, 64 evaluations), 1734000 ms (54187.5 ms/step)
[NAR] Song 0 chunk 1/1: 7805 frames, cache 10639 rows, 0 forwarded, 1734.1 s
[Store] Load VAE: 200 ms
[VAE] Track 1/1: song 0 variation 0
[VAE] Tiled decode done: 16 tiles -> T_audio=14985600 (312.20s @ 48kHz), 77000 ms
[Pipeline] Done: 1 tracks, 312.2 s of audio in 2016.0 s (0.2x realtime)";

    fn tracker(text: &str) -> RunTracker {
        let mut tracker = RunTracker::default();
        for line in text.lines() {
            tracker.push(line);
        }
        tracker
    }

    #[test]
    fn reads_every_stage_of_a_song() {
        let run = tracker(SONG).run("4f2a").unwrap();
        assert_eq!(run.backend.as_deref(), Some("MTL0"));
        assert_eq!(run.load_ms, 5000.0);
        assert_eq!(run.score_tokens, Some(2351));
        assert_eq!(run.score_ms, Some(121.0 + 42_000.0));
        assert_eq!(run.semantic_frames, Some(7805));
        assert_eq!(run.semantic_ms, Some(901.0 + 157_000.0));
        assert!(!run.truncated);
        assert_eq!(run.acoustic_ms, Some(1_734_000.0));
        assert_eq!(run.acoustic_steps, Some(32));
        assert_eq!(run.solver.as_deref(), Some("midpoint"));
        assert_eq!(run.evaluations, Some(64));
        assert_eq!(run.decode_ms, Some(77_000.0));
        assert_eq!(run.engine_total_ms, Some(2_016_000.0));
        assert_eq!(run.audio_seconds, Some(312.2));
        assert!(run.finished);
        assert_eq!(run.acoustic_wall_ms, Some(1_734_100.0));
    }

    #[test]
    fn a_song_cut_by_its_budget_says_so() {
        let text = SONG.replace("[AR] Semantic song 0: 7805 tokens\n", "[AR] Semantic song 0: 3250 tokens (truncated)\n");
        assert!(tracker(&text).run("4f2a").unwrap().truncated);
    }

    #[test]
    fn a_replay_has_no_composition_and_its_own_prefill() {
        let text = "\
[Server] Job r1: {}
[Pipeline] Replay: 1500 frames supplied
[NAR] Solved: T_lat=1500, 1 variations, 32 steps (ab2, 33 evaluations), 90000 ms (2812.5 ms/step)
[NAR] Song 0 chunk 1/1: 1500 frames, cache 3500 rows, 3500 forwarded, 92.5 s
[VAE] Decoded: T_latent=1500 -> T_audio=2880000 (60.00s @ 48kHz), 4000 ms
[Pipeline] Done: 1 tracks, 60.0 s of audio in 96.0 s (0.6x realtime)";
        let run = tracker(text).run("r1").unwrap();
        assert_eq!(run.score_ms, None);
        assert_eq!(run.semantic_ms, None);
        assert_eq!(run.acoustic_wall_ms, Some(92_500.0));
        assert_eq!(run.solver.as_deref(), Some("ab2"));
        assert_eq!(run.evaluations, Some(33));
        assert_eq!(run.decode_ms, Some(4000.0));
    }

    #[test]
    fn jobs_are_kept_apart_and_a_failed_one_closes() {
        let text = "[Server] Job a: {}\n[AR] Score: 10 tokens over 1 songs, 10 steps, 1.0 s (100.0 ms/step)\n[Pipeline] FATAL: out of memory\n[Server] Job b: {}\n[AR] Score: 20 tokens over 1 songs, 20 steps, 2.0 s (100.0 ms/step)";
        let tracker = tracker(text);
        let a = tracker.run("a").unwrap();
        assert_eq!(a.score_tokens, Some(10));
        assert!(!a.finished);
        assert_eq!(tracker.run("b").unwrap().score_ms, Some(2000.0));
        assert!(tracker.run("c").is_none());
    }

    #[test]
    fn the_record_derives_time_to_playable_and_realtime_factor() {
        let run = tracker(SONG).run("4f2a").unwrap();
        let settings = serde_json::json!({});
        let record = record(Some(&run), None, 1_000, Some(2_000), 2_018_000, 2_020_000, 1500.0, &settings, Some("YuE2-3B-Q8_0"), Some(312.2));
        assert_eq!(record["time_to_playable_ms"], 2_019_000.0);
        assert_eq!(record["queued_ms"], 1000.0);
        assert_eq!(record["solver"], "midpoint");
        assert_eq!(record["steps"], 32);
        let factor = record["realtime_factor"].as_f64().unwrap();
        assert!((factor - 312.2 / 2019.0).abs() < 1e-9);
    }
}
