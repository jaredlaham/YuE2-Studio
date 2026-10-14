//! A song as an Ableton Live project folder, zipped: a Live set with the
//! generated master on an active track, each separated stem on a track that is
//! switched off, and one track per part of the song's score, at the score's
//! tempo and meter, with the audio and MIDI files beside it in the folders
//! Live's own "Collect All and Save" uses.
//!
//! Nothing here runs when a song is generated. An export is a job of its own,
//! started from the song: it reuses whatever the song already has (stems, a
//! transcription), makes only what is missing and asked for, waits while a
//! song is being generated so it never slows one down, and reports each stage.
//!
//! Every file is on its track as a clip at bar 1, written from the clips of a
//! set Live 12 saved (see `ableton`). Audio clips are warped with two markers,
//! the sample's first and last frame, at the set's tempo, so they play at their
//! own rate. The read-me says which file is on which track as well, so the
//! project can be put back together by hand if Live does not take a clip.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use anyhow::{Context, Result};
use axum::extract::{Path as UrlPath, Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::ableton::{self, AudioClip, Clip, ClipNote, MidiClip, SetPlan, TrackPlan};
use crate::score::smf;
use crate::{api_error, kinetic_midi, ApiError, AppState};

/// Live's clip colours for each stem, so a set reads the same way every time.
fn stem_color(stem: &str) -> u32 {
    match stem {
        "drums" => 1,
        "bass" => 13,
        "vocals" => 9,
        "guitar" => 5,
        "piano" => 16,
        _ => 21,
    }
}

fn label(stem: &str) -> String {
    let mut chars = stem.chars();
    chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

fn safe(name: &str) -> String {
    let cleaned: String = name.chars().map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { '_' } else { c }).collect();
    let trimmed = cleaned.trim().trim_end_matches('.').to_string();
    if trimmed.is_empty() { "Untitled".into() } else { trimmed }
}

/// What goes into a project: the files, and the set that names them.
pub struct Contents {
    pub title: String,
    pub bpm: f64,
    pub meter: (u32, u32),
    pub mix: PathBuf,
    /// Stem name and its file.
    pub stems: Vec<(String, PathBuf)>,
    /// Track name, file name and bytes of each MIDI part.
    pub midi: Vec<(String, String, Vec<u8>)>,
    /// Whether the tempo came from the song's score; without one it is Live's default and says so.
    pub tempo_from_score: bool,
}

/// Live 12 Lite, the edition Kinetic Studio is made to work with, refuses a
/// ninth audio or MIDI track (returns are not counted).
pub const LIVE_LITE_TRACKS: usize = 8;

/// What goes on a track and what only goes in the folder. The master always
/// has the first track and plays: it is the song. Stems come next, on tracks
/// that are switched off (played with the master they would sound every part
/// twice, and they are estimates of it). The score's parts fill the tracks
/// that are left.
fn track_choice(stems: usize, midi: usize) -> (usize, usize) {
    let stems_on = stems.min(LIVE_LITE_TRACKS - 1);
    let midi_on = midi.min(LIVE_LITE_TRACKS - 1 - stems_on);
    (stems_on, midi_on)
}

fn read_me(contents: &Contents, files: &[(String, String)], left_off: &[String]) -> String {
    let mut text = format!(
        "{title} - Kinetic Studio project for Ableton Live 12\n\n\
         Open \"{title}.als\". The set is at {bpm} BPM in {num}/{den}{tempo_note}.\n\n\
         Each file is on its track as a clip at bar 1 in Arrangement view, named for the file.\n\
         The Master track is the song exactly as it was generated, and it is the one that plays.\n\
         The separated stems are on tracks that are switched off: turn a track's activator on (and the Master off)\n\
         to work from the stems instead.\n\
         Every audio file starts at the song's first sample and has the song's exact length, so at bar 1 they line up.\n\
         Audio clips are warped in Re-Pitch with a marker at the first and last sample: at this tempo they play at\n\
         their own speed. Changing the set's tempo changes their speed and pitch together.\n\
         If Live asks for a file, it is in Samples/Imported or MIDI below; if a clip is missing, drag the file\n\
         onto its track at bar 1.\n\n",
        title = contents.title,
        bpm = contents.bpm,
        num = contents.meter.0,
        den = contents.meter.1,
        tempo_note = if contents.tempo_from_score { ", the tempo of the song's score" } else { " (the song has no score, so this is Live's default tempo, not the song's)" },
    );
    text.push_str("Track -> file\n");
    for (track, file) in files {
        text.push_str(&format!("  {track} -> {file}\n"));
    }
    if !left_off.is_empty() {
        text.push_str(&format!("\nIn the folder but not on a track (Live 12 Lite holds {LIVE_LITE_TRACKS} tracks):\n"));
        for file in left_off {
            text.push_str(&format!("  {file}\n"));
        }
    }
    text.push_str(
        "\nWhat the files are\n\
         - Master: the song as generated, byte for byte.\n\
         - (separated): stems made from the mix by the HT-Demucs model. Each is the model's estimate of that part,\n\
           not an original recording; parts bleed into each other.\n\
         - (from score): the notes of the score the song was generated with - what the model was asked to play.\n\
           How closely the recording follows them is not measured. The MIDI tracks have no instrument: add one in Live.\n\
         - (transcribed): notes a transcription model heard in the master - what was played, as far as it can tell.\n",
    );
    text
}

/// An audio file as Live's clip: its length in frames and rate from decoding it.
fn audio_clip(name: &str, relative_path: String, path: &Path) -> Result<AudioClip> {
    let (channels, sample_rate) = crate::audio_pcm::decode_channels(path)?;
    let frames = channels.first().map_or(0, Vec::len) as u64;
    let meta = std::fs::metadata(path).with_context(|| format!("read {}", path.display()))?;
    let modified = meta.modified().ok().and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |since| since.as_secs());
    Ok(AudioClip { name: name.into(), relative_path, frames, sample_rate, file_size: meta.len(), modified })
}

/// A MIDI file as Live's clip: every note of every track, in quarter notes,
/// as long as the file (and never shorter than its last note).
fn midi_clip(name: &str, bytes: &[u8]) -> Result<MidiClip> {
    let song = smf::read(bytes).map_err(|reason| anyhow::anyhow!("{name}: {reason}"))?;
    let division = f64::from(song.division);
    let notes: Vec<ClipNote> = song
        .tracks
        .iter()
        .flat_map(|track| track.notes.iter())
        .map(|note| ClipNote { key: note.pitch, time: note.start as f64 / division, duration: note.end.saturating_sub(note.start) as f64 / division, velocity: note.velocity })
        .collect();
    let last = song.tracks.iter().flat_map(|track| track.notes.iter()).map(|note| note.end).max().unwrap_or(0);
    let beats = song.end().max(last) as f64 / division;
    Ok(MidiClip { name: name.into(), beats, notes })
}

fn stem_of(file_name: &str) -> &str {
    file_name.rsplit_once('.').map_or(file_name, |(stem, _)| stem)
}

/// What a written project holds: its files, the tracks in its set, and the
/// files that are in the folder but not on a track.
#[derive(Debug)]
pub struct Project {
    pub files: Vec<String>,
    pub tracks: Vec<String>,
    pub left_off: Vec<String>,
}

/// The project as a zip at `target`: the set, the audio, the MIDI and a read-me.
pub fn write_project(contents: &Contents, target: &Path) -> Result<Project> {
    let title = safe(&contents.title);
    let root = format!("{title} Project");
    let mut files: Vec<(String, String)> = Vec::new();
    let mut audio_tracks = Vec::new();
    let mix_extension = contents.mix.extension().and_then(|value| value.to_str()).unwrap_or("wav").to_ascii_lowercase();
    let mix_name = format!("{title} - Master.{mix_extension}");
    let mut audio_files = vec![("Master".to_string(), 23, mix_name, contents.mix.clone())];
    for (stem, path) in &contents.stems {
        let track = format!("{} (separated)", label(stem));
        let name = format!("{title} - {track}.wav");
        audio_files.push((track, stem_color(stem), name, path.clone()));
    }
    let (stems_on, midi_on) = track_choice(contents.stems.len(), contents.midi.len());
    let mut left_off = Vec::new();
    for (index, (track, color, name, path)) in audio_files.iter().enumerate() {
        let relative = format!("Samples/Imported/{name}");
        if index > stems_on {
            left_off.push(relative);
            continue;
        }
        let clip = audio_clip(stem_of(name), relative.clone(), path)?;
        // the master plays; a stem is there to switch on
        audio_tracks.push(TrackPlan { name: track.clone(), color: *color, clip: Some(Clip::Audio(clip)), active: index == 0 });
        files.push((track.clone(), relative));
    }
    let mut midi_tracks = Vec::new();
    for (index, (track, name, bytes)) in contents.midi.iter().enumerate() {
        if index >= midi_on {
            left_off.push(format!("MIDI/{name}"));
            continue;
        }
        midi_tracks.push(TrackPlan { name: track.clone(), color: 14, clip: Some(Clip::Midi(midi_clip(stem_of(name), bytes)?)), active: true });
        files.push((track.clone(), format!("MIDI/{name}")));
    }
    let set = ableton::build(ableton::LIVE_12_TEMPLATE, &SetPlan { bpm: contents.bpm, meter: contents.meter, midi: midi_tracks, audio: audio_tracks })?;
    let audio_files: Vec<(String, PathBuf)> = audio_files.into_iter().map(|(_, _, name, path)| (name, path)).collect();

    let partial = target.with_extension("zip.part");
    let file = std::fs::File::create(&partial).with_context(|| format!("create {}", partial.display()))?;
    let mut zip = zip::ZipWriter::new(std::io::BufWriter::new(file));
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored).large_file(true);
    let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut listed = Vec::new();
    let mut add = |zip: &mut zip::ZipWriter<_>, name: String, options, bytes: &mut dyn std::io::Read| -> Result<()> {
        zip.start_file(format!("{root}/{name}"), options)?;
        std::io::copy(bytes, zip)?;
        listed.push(name);
        Ok(())
    };
    // the set is already gzipped; storing it keeps it byte for byte what was built
    add(&mut zip, format!("{title}.als"), stored, &mut set.as_slice())?;
    for (name, path) in &audio_files {
        let mut source = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
        add(&mut zip, format!("Samples/Imported/{name}"), stored, &mut source)?;
    }
    for (_, name, bytes) in &contents.midi {
        add(&mut zip, format!("MIDI/{name}"), deflated, &mut bytes.as_slice())?;
    }
    add(&mut zip, "READ ME.txt".into(), deflated, &mut read_me(contents, &files, &left_off).as_bytes())?;
    zip.finish()?.flush()?;
    std::fs::rename(&partial, target).with_context(|| format!("publish {}", target.display()))?;
    Ok(Project { files: listed, tracks: files.into_iter().map(|(track, _)| track).collect(), left_off })
}

fn made() -> &'static Mutex<HashMap<String, PathBuf>> {
    static MADE: OnceLock<Mutex<HashMap<String, PathBuf>>> = OnceLock::new();
    MADE.get_or_init(Default::default)
}

#[derive(Debug, Deserialize)]
pub struct Start {
    pub song_id: String,
}

/// The parts of a song a project is made from, as they are now.
struct Parts {
    title: String,
    mix: PathBuf,
    stems: Vec<(String, PathBuf)>,
    midi: Vec<(String, String, Vec<u8>)>,
    bpm: f64,
    meter: (u32, u32),
    tempo_from_score: bool,
}

/// The score's tempo and meter, when the song has a score.
fn score_tempo(song: &crate::library::Song) -> Result<Option<(f64, (u32, u32))>, String> {
    let Some(score) = kinetic_midi::song_score(&song.generation_settings, song.replay_request.as_ref()) else { return Ok(None) };
    let facts = kinetic_midi::facts(&score).map_err(|reason| format!("the song's score could not be read: {reason}"))?;
    let meter = facts.meter.split_once('/').and_then(|(num, den)| Some((num.parse().ok()?, den.parse().ok()?))).unwrap_or((4, 4));
    Ok(Some((f64::from(facts.bpm), meter)))
}

/// One MIDI file per line of the song's score; the full score would double them.
fn score_midi(song: &crate::library::Song) -> Result<Vec<(String, String, Vec<u8>)>, String> {
    let Some(score) = kinetic_midi::song_score(&song.generation_settings, song.replay_request.as_ref()) else { return Ok(vec![]) };
    let facts = kinetic_midi::facts(&score).map_err(|reason| format!("the song's score could not be read: {reason}"))?;
    let mut midi = Vec::new();
    for part in facts.parts.iter().filter(|part| part.id != "score") {
        let bytes = kinetic_midi::part_midi(&score, part.id)?;
        let track = format!("{} (from score)", part.label);
        midi.push((track.clone(), kinetic_midi::file_name(&song.title, &track), bytes));
    }
    Ok(midi)
}

fn stems_of(state: &AppState, song_id: &str) -> Vec<(String, PathBuf)> {
    crate::stems_on_disk(state, song_id).into_iter().map(|stem| { let path = crate::stem_path(state, song_id, &stem); (stem, path) }).collect()
}

/// A transcription of the song kept beside it, from the MIDI tools.
fn transcribed_midi(state: &AppState, song: &crate::library::Song) -> Option<(String, String, Vec<u8>)> {
    let bytes = std::fs::read(crate::midi_path(state, &song.id)).ok()?;
    let track = "Master (transcribed)".to_string();
    Some((track.clone(), kinetic_midi::file_name(&song.title, &track), bytes))
}

/// Writes the project and keeps it for download, under a name no earlier export has.
async fn package(parts: Parts) -> Result<Value, String> {
    let contents = Contents { title: parts.title, bpm: parts.bpm, meter: parts.meter, mix: parts.mix, stems: parts.stems, midi: parts.midi, tempo_from_score: parts.tempo_from_score };
    let folder = crate::studio_data_root().map(|root| root.join("exports")).ok_or("the studio has no data folder")?;
    std::fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
    // a new name every time: an earlier project is never overwritten
    let base = format!("{} Project", safe(&contents.title));
    let target = (1..)
        .map(|n| folder.join(if n == 1 { format!("{base}.zip") } else { format!("{base} ({n}).zip") }))
        .find(|path| !path.exists())
        .expect("a free name");
    let written = target.clone();
    let summary = (contents.stems.len(), contents.midi.len(), contents.tempo_from_score, contents.bpm, contents.meter);
    let project = tokio::task::spawn_blocking(move || write_project(&contents, &written))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| format!("{error:#}"))?;
    let token = uuid::Uuid::now_v7().to_string();
    made().lock().expect("made").insert(token.clone(), target.clone());
    Ok(json!({
        "id": token,
        "file_name": target.file_name().and_then(|name| name.to_str()),
        "files": project.files,
        "tracks": project.tracks,
        "left_off": project.left_off,
        "stems": summary.0,
        "midi_parts": summary.1,
        "tempo_from_score": summary.2,
        "bpm": summary.3,
        "meter": format!("{}/{}", summary.4 .0, summary.4 .1),
        "clips_placed": true,
        "master_active": true,
    }))
}

fn song_or_404(state: &AppState, song_id: &str) -> Result<crate::library::Song, (StatusCode, Json<ApiError>)> {
    state
        .library
        .get_song(song_id)
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Song not found".into()))
}

/// Builds a song's Live project from what it has now, making nothing new, and answers with what is in it.
pub async fn start(State(state): State<AppState>, Json(start): Json<Start>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let song = song_or_404(&state, &start.song_id)?;
    let mix = state.library.media_path_for_song(&song).ok_or_else(|| api_error(StatusCode::NOT_FOUND, "the song's audio is not in the library".into()))?;
    let tempo = score_tempo(&song).map_err(|reason| api_error(StatusCode::UNPROCESSABLE_ENTITY, reason))?;
    let mut midi = score_midi(&song).map_err(|reason| api_error(StatusCode::UNPROCESSABLE_ENTITY, reason))?;
    midi.extend(transcribed_midi(&state, &song));
    let (bpm, meter) = tempo.unwrap_or((120.0, (4, 4)));
    let parts = Parts { title: song.title.clone(), mix, stems: stems_of(&state, &song.id), midi, bpm, meter, tempo_from_score: tempo.is_some() };
    package(parts).await.map(Json).map_err(|reason| api_error(StatusCode::INTERNAL_SERVER_ERROR, reason))
}

// ---- the export as a job of its own ----------------------------------------

/// One step of an export, as the window shows it.
#[derive(Debug, Clone, Serialize)]
pub struct Stage {
    pub id: &'static str,
    pub label: &'static str,
    /// pending, waiting, running, reused, done, skipped or failed.
    pub state: &'static str,
    /// 0 to 1 while it runs.
    pub progress: f64,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExportJob {
    pub id: String,
    pub song_id: String,
    pub title: String,
    pub stages: Vec<Stage>,
    pub done: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// What the project holds, once it is written; its `id` downloads it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<Value>,
    pub started_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_ms: Option<f64>,
}

fn export_jobs() -> &'static Mutex<HashMap<String, ExportJob>> {
    static JOBS: OnceLock<Mutex<HashMap<String, ExportJob>>> = OnceLock::new();
    JOBS.get_or_init(Default::default)
}

fn with_job(id: &str, change: impl FnOnce(&mut ExportJob)) {
    if let Some(job) = export_jobs().lock().expect("export jobs").get_mut(id) {
        change(job);
    }
}

fn set_stage(job: &str, stage: &'static str, state: &'static str, progress: f64, detail: impl Into<String>, ms: Option<f64>) {
    let detail = detail.into();
    with_job(job, |job| {
        if let Some(entry) = job.stages.iter_mut().find(|entry| entry.id == stage) {
            entry.state = state;
            entry.progress = progress;
            entry.detail = detail;
            if ms.is_some() {
                entry.ms = ms;
            }
        }
    });
}

#[derive(Debug, Deserialize)]
pub struct JobRequest {
    pub song_id: String,
    /// Separate the stems if the song has none yet (reused when it has them).
    #[serde(default = "yes")]
    pub stems: bool,
    /// Transcribe the master to MIDI if it has no transcription yet.
    #[serde(default)]
    pub transcribe: bool,
}

fn yes() -> bool {
    true
}

/// A song is being made, or waits to be: heavy export work holds back so the song is not slowed.
async fn generating(state: &AppState) -> bool {
    state.jobs.read().await.values().any(|job| matches!(job.status, crate::MusicJobStatus::Queued | crate::MusicJobStatus::Running))
}

/// Waits until no song is being generated, saying so on the stage.
async fn wait_for_generation(state: &AppState, job: &str, stage: &'static str) {
    while generating(state).await {
        set_stage(job, stage, "waiting", 0.0, "Waiting for the song being generated to finish, so it is not slowed down.", None);
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

fn unix_millis() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_millis() as u64)
}

/// Starts an export: reuses what the song has, makes what is missing, then packages the set.
pub async fn start_job(State(state): State<AppState>, Json(request): Json<JobRequest>) -> Result<Json<ExportJob>, (StatusCode, Json<ApiError>)> {
    let song = song_or_404(&state, &request.song_id)?;
    if song.metadata["derived"]["tool"].as_str() == Some("stems") {
        return Err(api_error(StatusCode::CONFLICT, "this track is a stem; export the song it came from".into()));
    }
    let stage = |id, label| Stage { id, label, state: "pending", progress: 0.0, detail: String::new(), ms: None };
    let job = ExportJob {
        id: uuid::Uuid::now_v7().to_string(),
        song_id: song.id.clone(),
        title: song.title.clone(),
        stages: vec![
            stage("master", "Master"),
            stage("stems", "Stems"),
            stage("score_midi", "Score MIDI"),
            stage("transcription", "Transcribed MIDI"),
            stage("tempo", "Tempo"),
            stage("package", "Live set"),
        ],
        done: false,
        error: None,
        project: None,
        started_at: unix_millis(),
        total_ms: None,
    };
    export_jobs().lock().expect("export jobs").insert(job.id.clone(), job.clone());
    let id = job.id.clone();
    tokio::spawn(async move {
        let started = std::time::Instant::now();
        let outcome = run_job(&state, &id, song, &request).await;
        with_job(&id, |job| {
            job.done = true;
            job.total_ms = Some(started.elapsed().as_secs_f64() * 1000.0);
            match outcome {
                Ok(project) => job.project = Some(project),
                Err(reason) => job.error = Some(reason),
            }
        });
    });
    Ok(Json(job))
}

pub async fn job_status(UrlPath(id): UrlPath<String>) -> Result<Json<ExportJob>, (StatusCode, Json<ApiError>)> {
    export_jobs().lock().expect("export jobs").get(&id).cloned().map(Json).ok_or_else(|| api_error(StatusCode::NOT_FOUND, "no such export".into()))
}

async fn run_job(state: &AppState, id: &str, song: crate::library::Song, request: &JobRequest) -> Result<Value, String> {
    let fail = |stage: &'static str, reason: String| {
        set_stage(id, stage, "failed", 0.0, reason.clone(), None);
        reason
    };

    // the master is the library's file, as generated: nothing to make
    let mix = state.library.media_path_for_song(&song).ok_or_else(|| fail("master", "the song's audio is not in the library".into()))?;
    set_stage(id, "master", "reused", 1.0, "The generated master, byte for byte.", Some(0.0));

    let clock = std::time::Instant::now();
    let stems = make_stems(state, id, &song, &mix, request.stems).await.map_err(|reason| fail("stems", reason))?;
    if stems.1 {
        set_stage(id, "stems", "done", 1.0, format!("Separated {} stems.", stems.0.len()), Some(clock.elapsed().as_secs_f64() * 1000.0));
    }

    let clock = std::time::Instant::now();
    let mut midi = score_midi(&song).map_err(|reason| fail("score_midi", reason))?;
    if midi.is_empty() {
        set_stage(id, "score_midi", "skipped", 0.0, "The song has no score.", Some(0.0));
    } else {
        set_stage(id, "score_midi", "done", 1.0, format!("{} parts from the song's score.", midi.len()), Some(clock.elapsed().as_secs_f64() * 1000.0));
    }

    let clock = std::time::Instant::now();
    match transcription(state, id, &song, &mix, request.transcribe).await.map_err(|reason| fail("transcription", reason))? {
        Some((made_now, part)) => {
            let ms = clock.elapsed().as_secs_f64() * 1000.0;
            if made_now {
                set_stage(id, "transcription", "done", 1.0, "Transcribed the master.", Some(ms));
            }
            midi.push(part);
        }
        None => {}
    }

    let clock = std::time::Instant::now();
    let (bpm, meter, tempo_from_score) = match score_tempo(&song).map_err(|reason| fail("tempo", reason))? {
        Some((bpm, meter)) => {
            set_stage(id, "tempo", "reused", 1.0, format!("{bpm} BPM in {}/{}, from the score the song was generated from.", meter.0, meter.1), Some(clock.elapsed().as_secs_f64() * 1000.0));
            (bpm, meter, true)
        }
        None => match measured_tempo(state, &mix).await {
            Some(bpm) => {
                set_stage(id, "tempo", "done", 1.0, format!("{bpm} BPM, measured from the master with Beat This!"), Some(clock.elapsed().as_secs_f64() * 1000.0));
                (f64::from(bpm), (4, 4), false)
            }
            None => {
                set_stage(id, "tempo", "skipped", 0.0, "No score and no beat tracker installed: the set uses Live's default 120 BPM.", Some(0.0));
                (120.0, (4, 4), false)
            }
        },
    };

    let clock = std::time::Instant::now();
    set_stage(id, "package", "running", 0.5, "Writing the set and copying the audio.", None);
    let parts = Parts { title: song.title.clone(), mix, stems: stems.0, midi, bpm, meter, tempo_from_score };
    let project = package(parts).await.map_err(|reason| fail("package", reason))?;
    let tracks = project["tracks"].as_array().map_or(0, Vec::len);
    set_stage(id, "package", "done", 1.0, format!("{tracks} {}; the Master plays{}.", if tracks == 1 { "track" } else { "tracks" }, if stems_on_tracks(&project) { ", the stems are switched off" } else { "" }), Some(clock.elapsed().as_secs_f64() * 1000.0));
    Ok(project)
}

fn stems_on_tracks(project: &Value) -> bool {
    project["tracks"].as_array().is_some_and(|tracks| tracks.iter().any(|track| track.as_str().is_some_and(|name| name.ends_with("(separated)"))))
}

/// The song's stems: the ones on disk when they are all there, else separated now
/// if asked and the separator is installed. The flag says they were made now.
async fn make_stems(state: &AppState, id: &str, song: &crate::library::Song, mix: &Path, wanted: bool) -> Result<(Vec<(String, PathBuf)>, bool), String> {
    let asked: Vec<String> = state.separation_config.read().await.stems.clone();
    let complete = |state: &AppState| {
        let on_disk = crate::stems_on_disk(state, &song.id);
        !on_disk.is_empty() && asked.iter().all(|stem| on_disk.contains(stem))
    };
    if complete(state) {
        set_stage(id, "stems", "reused", 1.0, "Reused the stems separated earlier.", Some(0.0));
        return Ok((stems_of(state, &song.id), false));
    }
    let partial = stems_of(state, &song.id);
    if !wanted {
        let detail = if partial.is_empty() { "Not asked for.".to_string() } else { format!("Not asked for; reused the {} stems the song has.", partial.len()) };
        set_stage(id, "stems", if partial.is_empty() { "skipped" } else { "reused" }, 1.0, detail, Some(0.0));
        return Ok((partial, false));
    }
    if !state.separator.is_installed() {
        set_stage(id, "stems", "skipped", 0.0, "Install the stem separator in Tools to add stems.", Some(0.0));
        return Ok((partial, false));
    }
    wait_for_generation(state, id, "stems").await;
    // another separation (the stem mixer's) goes first; it may be this song's
    loop {
        let running = state.separation_run.read().await.as_ref().filter(|run| !run.done).map(|run| run.song_id.clone());
        match running {
            Some(other) => {
                set_stage(id, "stems", "waiting", 0.0, if other == song.id { "The stems are being separated already." } else { "Another song is being separated." }, None);
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
            None => break,
        }
    }
    if complete(state) {
        set_stage(id, "stems", "reused", 1.0, "Reused the stems separated just now.", Some(0.0));
        return Ok((stems_of(state, &song.id), false));
    }
    crate::card_free_of_training(state, "separate tracks").await.map_err(|(_, Json(error))| error.error)?;
    let config = state.separation_config.read().await.clone();
    let card = state.lyrics_sync.onnx_card(config.runtime).map_err(|error| format!("{error:#}"))?.filter(|card| crate::separates_on(*card));
    *state.separation_run.write().await = Some(crate::SeparationRun {
        song_id: song.id.clone(),
        progress: 0.0,
        done: false,
        error: None,
        stems: vec![],
        used_gpu: None,
        library_songs: vec![],
        report: None,
    });
    set_stage(id, "stems", "running", 0.0, "Separating the master into stems.", None);
    let (background, song_id, audio, model, overlap, names) = (state.clone(), song.id.clone(), mix.to_path_buf(), state.separator.model_path(), config.sane_overlap(), config.stems.clone());
    let work = tokio::task::spawn_blocking(move || crate::separate_into_stems(&background, &song_id, &audio, &model, card, overlap, &names));
    tokio::pin!(work);
    let joined = loop {
        tokio::select! {
            joined = &mut work => break joined,
            _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => {
                let progress = state.separation_run.read().await.as_ref().map_or(0.0, |run| run.progress);
                set_stage(id, "stems", "running", progress, format!("Separating the master into stems: {:.0}%", progress * 100.0), None);
            }
        }
    };
    let (outcome, library) = joined.map_err(|error| error.to_string())?;
    if let Some(run) = state.separation_run.write().await.as_mut() {
        run.done = true;
        run.progress = 1.0;
        match &outcome {
            Ok((stems, used_gpu, report)) => {
                run.stems = stems.clone();
                run.used_gpu = Some(*used_gpu);
                run.report = Some(report.clone());
                if let Some(Ok((songs, problem))) = &library {
                    run.library_songs = songs.clone();
                    run.error = problem.clone();
                }
            }
            Err(error) => run.error = Some(format!("{error:#}")),
        }
    }
    outcome.map_err(|error| format!("{error:#}"))?;
    Ok((stems_of(state, &song.id), true))
}

/// The master's transcription: kept from the MIDI tools, or made now if asked and the
/// transcriber is installed. The flag says it was made now.
async fn transcription(state: &AppState, id: &str, song: &crate::library::Song, mix: &Path, wanted: bool) -> Result<Option<(bool, (String, String, Vec<u8>))>, String> {
    if let Some(part) = transcribed_midi(state, song) {
        set_stage(id, "transcription", "reused", 1.0, "Reused the transcription made earlier.", Some(0.0));
        return Ok(Some((false, part)));
    }
    if !wanted {
        set_stage(id, "transcription", "skipped", 0.0, "Not asked for.", Some(0.0));
        return Ok(None);
    }
    let size = crate::midi::size(crate::preferred_midi_size(state)).ok_or("no transcriber size")?;
    if !state.midi.missing(size).is_empty() {
        set_stage(id, "transcription", "skipped", 0.0, "Install the MIDI transcriber in Tools to add a transcription.", Some(0.0));
        return Ok(None);
    }
    wait_for_generation(state, id, "transcription").await;
    while state.midi_run.read().await.as_ref().is_some_and(|run| !run.done) {
        set_stage(id, "transcription", "waiting", 0.0, "Another transcription is running.", None);
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    if let Some(part) = transcribed_midi(state, song) {
        set_stage(id, "transcription", "reused", 1.0, "Reused the transcription made just now.", Some(0.0));
        return Ok(Some((false, part)));
    }
    crate::card_free_of_training(state, "turn tracks into MIDI").await.map_err(|(_, Json(error))| error.error)?;
    state.midi_stop.0.store(false, std::sync::atomic::Ordering::Relaxed);
    *state.midi_run.write().await = Some(crate::midi::Run { song_id: Some(song.id.clone()), title: song.title.clone(), size: size.id, stage: "preparing", chunks_done: 0, chunks_total: 0, notes: 0, done: false, error: None, file: None });
    set_stage(id, "transcription", "running", 0.0, "Transcribing the master.", None);
    let output = crate::midi_path(state, &song.id);
    let work = crate::transcribe_to_midi(state, size, mix, &output);
    tokio::pin!(work);
    let outcome = loop {
        tokio::select! {
            outcome = &mut work => break outcome,
            _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => {
                let progress = state.midi_run.read().await.as_ref().map_or(0.0, |run| if run.chunks_total > 0 { f64::from(run.chunks_done) / f64::from(run.chunks_total) } else { 0.0 });
                set_stage(id, "transcription", "running", progress, format!("Transcribing the master: {:.0}%", progress * 100.0), None);
            }
        }
    };
    if let Some(run) = state.midi_run.write().await.as_mut() {
        run.done = true;
        match &outcome {
            Ok(notes) => {
                run.stage = "done";
                run.notes = *notes;
                run.file = Some(output.display().to_string());
            }
            Err(error) => run.error = Some(format!("{error:#}")),
        }
    }
    outcome.map_err(|error| format!("{error:#}"))?;
    Ok(transcribed_midi(state, song).map(|part| (true, part)))
}

/// The master's tempo from Beat This!, when its model is installed (the LoRA tools fetch it).
async fn measured_tempo(state: &AppState, mix: &Path) -> Option<u32> {
    let models = state.training.audio_facts_dir();
    if !models.join(crate::audio_facts::BEAT_MODEL).is_file() {
        return None;
    }
    let audio = mix.to_path_buf();
    tokio::task::spawn_blocking(move || crate::audio_facts::Measurer::load(&models, None).and_then(|mut measurer| measurer.measure_mono(&crate::audio_facts::decode(&audio)?)))
        .await
        .ok()?
        .ok()
        .map(|facts| facts.bpm)
}

pub async fn file(UrlPath(id): UrlPath<String>, request: Request) -> Result<axum::response::Response, (StatusCode, Json<ApiError>)> {
    let path = made().lock().expect("made").get(&id).cloned().ok_or_else(|| api_error(StatusCode::NOT_FOUND, "no such project".into()))?;
    let mut response = crate::serve_audio_file(&path, request).await;
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/zip"));
    if let Some(name) = path.file_name().and_then(|value| value.to_str()) {
        let ascii: String = name.chars().map(|c| if c.is_ascii() && c != '"' { c } else { '_' }).collect();
        if let Ok(value) = HeaderValue::from_str(&format!("attachment; filename=\"{ascii}\"")) {
            response.headers_mut().insert(header::CONTENT_DISPOSITION, value);
        }
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn a_project_holds_the_set_the_files_and_a_read_me() {
        let dir = std::env::temp_dir().join(format!("kinetic-ableton-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wav = |path: &Path, frames: u32| {
            let spec = hound::WavSpec { channels: 2, sample_rate: 48_000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
            let mut writer = hound::WavWriter::create(path, spec).unwrap();
            for _ in 0..frames * 2 {
                writer.write_sample(0i16).unwrap();
            }
            writer.finalize().unwrap();
        };
        let mix = dir.join("mix.wav");
        wav(&mix, 96_000);
        let drums = dir.join("drums.wav");
        wav(&drums, 96_000);
        // two beats of D, then two of F
        let lead = smf::write(
            &[vec![], vec![(0, smf::note_on(0, 62, 90)), (960, smf::note_off(0, 62)), (960, smf::note_on(0, 65, 80)), (1920, smf::note_off(0, 65))]],
            smf::DIVISION,
            1920,
        )
        .unwrap();
        let contents = Contents {
            title: "Night: drive".into(),
            bpm: 92.0,
            meter: (4, 4),
            mix,
            stems: vec![("drums".into(), drums)],
            midi: vec![("Lead (from score)".into(), "Night_ drive - Lead (from score).mid".into(), lead)],
            tempo_from_score: true,
        };
        let target = dir.join("out.zip");
        let project = write_project(&contents, &target).unwrap();
        assert_eq!(project.tracks, ["Master", "Drums (separated)", "Lead (from score)"], "the master always has a track");
        assert!(project.left_off.is_empty());
        assert_eq!(project.files, [
            "Night_ drive.als",
            "Samples/Imported/Night_ drive - Master.wav",
            "Samples/Imported/Night_ drive - Drums (separated).wav",
            "MIDI/Night_ drive - Lead (from score).mid",
            "READ ME.txt",
        ]);
        let mut archive = zip::ZipArchive::new(std::fs::File::open(&target).unwrap()).unwrap();
        let mut set = Vec::new();
        archive.by_name("Night_ drive Project/Night_ drive.als").unwrap().read_to_end(&mut set).unwrap();
        let xml = ableton::gunzip(&set).unwrap();
        for name in ["Master", "Drums (separated)", "Lead (from score)"] {
            assert!(xml.contains(&format!(r#"<UserName Value="{name}" />"#)), "{name} track");
        }
        assert!(xml.contains(r#"<Manual Value="92" />"#));
        // each file is on its track: 2 s at 92 BPM is 3.0666… beats
        let document = roxmltree::Document::parse(&xml).unwrap();
        let paths: Vec<&str> = document.descendants().filter(|node| node.has_tag_name("RelativePath")).filter_map(|node| node.attribute("Value")).filter(|value| value.starts_with("Samples/")).collect();
        assert_eq!(paths, ["Samples/Imported/Night_ drive - Master.wav", "Samples/Imported/Night_ drive - Drums (separated).wav"]);
        // the master plays and the stem waits to be switched on
        let activator = |name: &str| {
            let track = document.descendants().filter(|node| node.has_tag_name("AudioTrack")).find(|track| track.descendants().any(|node| node.has_tag_name("UserName") && node.attribute("Value") == Some(name))).unwrap();
            let speaker = track.descendants().find(|node| node.has_tag_name("Speaker")).unwrap();
            speaker.children().find(|node| node.has_tag_name("Manual")).unwrap().attribute("Value").unwrap().to_string()
        };
        assert_eq!(activator("Master"), "true");
        assert_eq!(activator("Drums (separated)"), "false");
        let audio = document.descendants().find(|node| node.has_tag_name("AudioClip")).unwrap();
        let end: f64 = audio.descendants().find(|node| node.has_tag_name("CurrentEnd")).unwrap().attribute("Value").unwrap().parse().unwrap();
        assert!((end - 2.0 * 92.0 / 60.0).abs() < 1e-9);
        let lead = document.descendants().find(|node| node.has_tag_name("MidiClip")).unwrap();
        let notes: Vec<(&str, &str, &str)> = lead
            .descendants()
            .filter(|node| node.has_tag_name("MidiNoteEvent"))
            .map(|node| (node.attribute("Time").unwrap(), node.attribute("Duration").unwrap(), node.attribute("Velocity").unwrap()))
            .collect();
        assert_eq!(notes, [("0", "2", "90"), ("2", "2", "80")]);
        for (inside, original) in [("Samples/Imported/Night_ drive - Drums (separated).wav", "drums.wav"), ("Samples/Imported/Night_ drive - Master.wav", "mix.wav")] {
            let mut back = Vec::new();
            archive.by_name(&format!("Night_ drive Project/{inside}")).unwrap().read_to_end(&mut back).unwrap();
            assert_eq!(back, std::fs::read(dir.join(original)).unwrap(), "the audio goes in byte for byte");
        }
        let mut readme = String::new();
        archive.by_name("Night_ drive Project/READ ME.txt").unwrap().read_to_string(&mut readme).unwrap();
        assert!(readme.contains("Drums (separated) -> Samples/Imported/Night_ drive - Drums (separated).wav"));
        assert!(readme.contains("Master -> Samples/Imported/Night_ drive - Master.wav"));
        assert!(readme.contains("as a clip at bar 1"));
        assert!(readme.contains("switched off"));
        assert!(readme.contains("not an original recording"));
        assert!(!target.with_extension("zip.part").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_set_never_has_more_tracks_than_live_lite_takes() {
        // (stems on, MIDI parts on); the master always has the first track
        assert_eq!(track_choice(0, 3), (0, 3));
        assert_eq!(track_choice(6, 3), (6, 1));
        assert_eq!(track_choice(4, 2), (4, 2));
        assert_eq!(track_choice(0, 9), (0, 7));
        assert_eq!(track_choice(9, 2), (7, 0));
        for stems in 0..=10 {
            for midi in 0..=10 {
                let (on, parts) = track_choice(stems, midi);
                assert!(1 + on + parts <= LIVE_LITE_TRACKS);
            }
        }
    }

    #[test]
    fn without_a_score_the_read_me_says_the_tempo_is_not_the_songs() {
        let contents = Contents { title: "x".into(), bpm: 120.0, meter: (4, 4), mix: PathBuf::from("m.wav"), stems: vec![], midi: vec![], tempo_from_score: false };
        assert!(read_me(&contents, &[], &[]).contains("not the song's"));
    }
}
