//! A song as an Ableton Live project folder, zipped: a Live set with a track
//! for the mix, one per separated stem and one per part of the song's score,
//! at the score's tempo and meter, with the audio and MIDI files beside it in
//! the folders Live's own "Collect All and Save" uses.
//!
//! The clips are not placed on the tracks yet: how Live 12 writes a clip has
//! not been seen in a set it saved, and a guessed clip could make Live refuse
//! the whole file. Until it is, the read-me in the folder says which file goes
//! on which track.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use anyhow::{Context, Result};
use axum::extract::{Path as UrlPath, Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::ableton::{self, SetPlan, TrackPlan};
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

fn read_me(contents: &Contents, files: &[(String, String)]) -> String {
    let mut text = format!(
        "{title} - Kinetic Studio project for Ableton Live 12\n\n\
         Open \"{title}.als\". The set is at {bpm} BPM in {num}/{den}{tempo_note}.\n\n\
         Each track is named for the file that belongs on it. Kinetic Studio does not place the clips yet;\n\
         drag each file from Samples/Imported or MIDI onto its track at bar 1 in Arrangement view.\n\
         Every audio file starts at the song's first sample and has the song's exact length, so at bar 1 they line up.\n\n",
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
    text.push_str(
        "\nWhat the files are\n\
         - Mix: the song as generated.\n\
         - (separated): stems made from the mix by the HT-Demucs model. Each is the model's estimate of that part,\n\
           not an original recording; parts bleed into each other.\n\
         - (from score): the notes of the score the song was generated with - what the model was asked to play.\n\
           How closely the recording follows them is not measured. The MIDI tracks have no instrument: add one in Live.\n",
    );
    text
}

/// The project as a zip at `target`: the set, the audio, the MIDI and a read-me.
pub fn write_project(contents: &Contents, target: &Path) -> Result<Vec<String>> {
    let title = safe(&contents.title);
    let root = format!("{title} Project");
    let mut files: Vec<(String, String)> = Vec::new();
    let mut audio_tracks = Vec::new();
    let mix_extension = contents.mix.extension().and_then(|value| value.to_str()).unwrap_or("wav").to_ascii_lowercase();
    let mix_name = format!("{title} - Mix.{mix_extension}");
    audio_tracks.push(TrackPlan { name: "Mix".into(), color: 23 });
    files.push(("Mix".into(), format!("Samples/Imported/{mix_name}")));
    let mut audio_files = vec![(mix_name, contents.mix.clone())];
    for (stem, path) in &contents.stems {
        let track = format!("{} (separated)", label(stem));
        let name = format!("{title} - {track}.wav");
        audio_tracks.push(TrackPlan { name: track.clone(), color: stem_color(stem) });
        files.push((track, format!("Samples/Imported/{name}")));
        audio_files.push((name, path.clone()));
    }
    let midi_tracks: Vec<TrackPlan> = contents.midi.iter().map(|(track, _, _)| TrackPlan { name: track.clone(), color: 14 }).collect();
    for (track, name, _) in &contents.midi {
        files.push((track.clone(), format!("MIDI/{name}")));
    }
    let set = ableton::build(ableton::LIVE_12_TEMPLATE, &SetPlan { bpm: contents.bpm, meter: contents.meter, midi: midi_tracks, audio: audio_tracks })?;

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
    add(&mut zip, "READ ME.txt".into(), deflated, &mut read_me(contents, &files).as_bytes())?;
    zip.finish()?.flush()?;
    std::fs::rename(&partial, target).with_context(|| format!("publish {}", target.display()))?;
    Ok(listed)
}

fn made() -> &'static Mutex<HashMap<String, PathBuf>> {
    static MADE: OnceLock<Mutex<HashMap<String, PathBuf>>> = OnceLock::new();
    MADE.get_or_init(Default::default)
}

#[derive(Debug, Deserialize)]
pub struct Start {
    pub song_id: String,
}

/// Builds a song's Live project and answers with what is in it.
pub async fn start(State(state): State<AppState>, Json(start): Json<Start>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let song = state
        .library
        .get_song(&start.song_id)
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Song not found".into()))?;
    let mix = state.library.media_path_for_song(&song).ok_or_else(|| api_error(StatusCode::NOT_FOUND, "the song's audio is not in the library".into()))?;
    let stems: Vec<(String, PathBuf)> = crate::stems_on_disk(&state, &song.id).into_iter().map(|stem| { let path = crate::stem_path(&state, &song.id, &stem); (stem, path) }).collect();
    let score = kinetic_midi::song_score(&song.generation_settings, song.replay_request.as_ref());
    let facts = score.as_deref().map(kinetic_midi::facts).transpose().map_err(|reason| api_error(StatusCode::UNPROCESSABLE_ENTITY, format!("the song's score could not be read: {reason}")))?;
    let mut midi = Vec::new();
    if let (Some(score), Some(facts)) = (&score, &facts) {
        // one track per line of the score; the full score would double them
        for part in facts.parts.iter().filter(|part| part.id != "score") {
            let bytes = kinetic_midi::part_midi(score, part.id).map_err(|reason| api_error(StatusCode::UNPROCESSABLE_ENTITY, reason))?;
            let track = format!("{} (from score)", part.label);
            midi.push((track.clone(), kinetic_midi::file_name(&song.title, &track), bytes));
        }
    }
    let (bpm, meter, tempo_from_score) = match &facts {
        Some(facts) => {
            let (num, den) = facts.meter.split_once('/').and_then(|(num, den)| Some((num.parse().ok()?, den.parse().ok()?))).unwrap_or((4, 4));
            (f64::from(facts.bpm), (num, den), true)
        }
        None => (120.0, (4, 4), false),
    };
    let contents = Contents { title: song.title.clone(), bpm, meter, mix, stems, midi, tempo_from_score };
    let folder = crate::studio_data_root().map(|root| root.join("exports")).ok_or_else(|| api_error(StatusCode::INTERNAL_SERVER_ERROR, "the studio has no data folder".into()))?;
    std::fs::create_dir_all(&folder).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    // a new name every time: an earlier project is never overwritten
    let base = format!("{} Project", safe(&song.title));
    let target = (1..)
        .map(|n| folder.join(if n == 1 { format!("{base}.zip") } else { format!("{base} ({n}).zip") }))
        .find(|path| !path.exists())
        .expect("a free name");
    let written = target.clone();
    let summary = (contents.stems.len(), contents.midi.len(), contents.tempo_from_score, contents.bpm, contents.meter);
    let listed = tokio::task::spawn_blocking(move || write_project(&contents, &written))
        .await
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{error:#}")))?;
    let token = uuid::Uuid::now_v7().to_string();
    made().lock().expect("made").insert(token.clone(), target.clone());
    Ok(Json(json!({
        "id": token,
        "file_name": target.file_name().and_then(|name| name.to_str()),
        "files": listed,
        "stems": summary.0,
        "midi_parts": summary.1,
        "tempo_from_score": summary.2,
        "bpm": summary.3,
        "meter": format!("{}/{}", summary.4 .0, summary.4 .1),
        "clips_placed": false,
    })))
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
        let mix = dir.join("mix.wav");
        std::fs::write(&mix, b"RIFF mix").unwrap();
        let drums = dir.join("drums.wav");
        std::fs::write(&drums, b"RIFF drums").unwrap();
        let contents = Contents {
            title: "Night: drive".into(),
            bpm: 92.0,
            meter: (4, 4),
            mix,
            stems: vec![("drums".into(), drums)],
            midi: vec![("Lead (from score)".into(), "Night_ drive - Lead (from score).mid".into(), b"MThd".to_vec())],
            tempo_from_score: true,
        };
        let target = dir.join("out.zip");
        let listed = write_project(&contents, &target).unwrap();
        assert_eq!(listed, [
            "Night_ drive.als",
            "Samples/Imported/Night_ drive - Mix.wav",
            "Samples/Imported/Night_ drive - Drums (separated).wav",
            "MIDI/Night_ drive - Lead (from score).mid",
            "READ ME.txt",
        ]);
        let mut archive = zip::ZipArchive::new(std::fs::File::open(&target).unwrap()).unwrap();
        let mut set = Vec::new();
        archive.by_name("Night_ drive Project/Night_ drive.als").unwrap().read_to_end(&mut set).unwrap();
        let xml = ableton::gunzip(&set).unwrap();
        for name in ["Mix", "Drums (separated)", "Lead (from score)"] {
            assert!(xml.contains(&format!(r#"<UserName Value="{name}" />"#)), "{name} track");
        }
        assert!(xml.contains(r#"<Manual Value="92" />"#));
        let mut drums_back = Vec::new();
        archive.by_name("Night_ drive Project/Samples/Imported/Night_ drive - Drums (separated).wav").unwrap().read_to_end(&mut drums_back).unwrap();
        assert_eq!(drums_back, b"RIFF drums", "the audio goes in byte for byte");
        let mut readme = String::new();
        archive.by_name("Night_ drive Project/READ ME.txt").unwrap().read_to_string(&mut readme).unwrap();
        assert!(readme.contains("Drums (separated) -> Samples/Imported/Night_ drive - Drums (separated).wav"));
        assert!(readme.contains("not an original recording"));
        assert!(!target.with_extension("zip.part").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn without_a_score_the_read_me_says_the_tempo_is_not_the_songs() {
        let contents = Contents { title: "x".into(), bpm: 120.0, meter: (4, 4), mix: PathBuf::from("m.wav"), stems: vec![], midi: vec![], tempo_from_score: false };
        assert!(read_me(&contents, &[]).contains("not the song's"));
    }
}
