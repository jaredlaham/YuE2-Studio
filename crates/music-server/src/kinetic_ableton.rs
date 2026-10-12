//! A song as an Ableton Live project folder, zipped: a Live set with a track
//! for the mix, one per separated stem and one per part of the song's score,
//! at the score's tempo and meter, with the audio and MIDI files beside it in
//! the folders Live's own "Collect All and Save" uses.
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
use serde::Deserialize;
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

/// What goes on a track and what only goes in the folder. Stems come first:
/// they are what a set is built from. The score's parts fill the tracks that
/// are left. The mix gets a track only when there are no stems, since played
/// with them it would sound every part twice; it is always in the folder.
fn track_choice(stems: usize, midi: usize) -> (bool, usize, usize) {
    let stems_on = stems.min(LIVE_LITE_TRACKS);
    let mix_on = stems == 0;
    let midi_on = midi.min(LIVE_LITE_TRACKS - stems_on - usize::from(mix_on));
    (mix_on, stems_on, midi_on)
}

fn read_me(contents: &Contents, files: &[(String, String)], left_off: &[String]) -> String {
    let mut text = format!(
        "{title} - Kinetic Studio project for Ableton Live 12\n\n\
         Open \"{title}.als\". The set is at {bpm} BPM in {num}/{den}{tempo_note}.\n\n\
         Each file is on its track as a clip at bar 1 in Arrangement view, named for the file.\n\
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
        text.push_str(&format!(
            "\nIn the folder but not on a track (Live 12 Lite holds {LIVE_LITE_TRACKS} tracks; the stems are on tracks, so the mix is\n\
             kept as a reference rather than played with them):\n"
        ));
        for file in left_off {
            text.push_str(&format!("  {file}\n"));
        }
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
    let mix_name = format!("{title} - Mix.{mix_extension}");
    let mut audio_files = vec![("Mix".to_string(), 23, mix_name, contents.mix.clone())];
    for (stem, path) in &contents.stems {
        let track = format!("{} (separated)", label(stem));
        let name = format!("{title} - {track}.wav");
        audio_files.push((track, stem_color(stem), name, path.clone()));
    }
    let (mix_on, stems_on, midi_on) = track_choice(contents.stems.len(), contents.midi.len());
    let mut left_off = Vec::new();
    for (index, (track, color, name, path)) in audio_files.iter().enumerate() {
        let relative = format!("Samples/Imported/{name}");
        let on = if index == 0 { mix_on } else { index <= stems_on };
        if !on {
            left_off.push(relative);
            continue;
        }
        let clip = audio_clip(stem_of(name), relative.clone(), path)?;
        audio_tracks.push(TrackPlan { name: track.clone(), color: *color, clip: Some(Clip::Audio(clip)) });
        files.push((track.clone(), relative));
    }
    let mut midi_tracks = Vec::new();
    for (index, (track, name, bytes)) in contents.midi.iter().enumerate() {
        if index >= midi_on {
            left_off.push(format!("MIDI/{name}"));
            continue;
        }
        midi_tracks.push(TrackPlan { name: track.clone(), color: 14, clip: Some(Clip::Midi(midi_clip(stem_of(name), bytes)?)) });
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
    let project = tokio::task::spawn_blocking(move || write_project(&contents, &written))
        .await
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{error:#}")))?;
    let token = uuid::Uuid::now_v7().to_string();
    made().lock().expect("made").insert(token.clone(), target.clone());
    Ok(Json(json!({
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
        assert_eq!(project.tracks, ["Drums (separated)", "Lead (from score)"], "with stems, the mix stays off the tracks");
        assert_eq!(project.left_off, ["Samples/Imported/Night_ drive - Mix.wav"]);
        assert_eq!(project.files, [
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
        for name in ["Drums (separated)", "Lead (from score)"] {
            assert!(xml.contains(&format!(r#"<UserName Value="{name}" />"#)), "{name} track");
        }
        assert!(!xml.contains(r#"<UserName Value="Mix" />"#));
        assert!(xml.contains(r#"<Manual Value="92" />"#));
        // each file is on its track: 2 s at 92 BPM is 3.0666… beats
        let document = roxmltree::Document::parse(&xml).unwrap();
        let paths: Vec<&str> = document.descendants().filter(|node| node.has_tag_name("RelativePath")).filter_map(|node| node.attribute("Value")).filter(|value| value.starts_with("Samples/")).collect();
        assert_eq!(paths, ["Samples/Imported/Night_ drive - Drums (separated).wav"]);
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
        let mut drums_back = Vec::new();
        archive.by_name("Night_ drive Project/Samples/Imported/Night_ drive - Drums (separated).wav").unwrap().read_to_end(&mut drums_back).unwrap();
        assert_eq!(drums_back, std::fs::read(&dir.join("drums.wav")).unwrap(), "the audio goes in byte for byte");
        let mut readme = String::new();
        archive.by_name("Night_ drive Project/READ ME.txt").unwrap().read_to_string(&mut readme).unwrap();
        assert!(readme.contains("Drums (separated) -> Samples/Imported/Night_ drive - Drums (separated).wav"));
        assert!(readme.contains("as a clip at bar 1"));
        assert!(readme.contains("not on a track"));
        assert!(readme.contains("  Samples/Imported/Night_ drive - Mix.wav\n"));
        assert!(readme.contains("not an original recording"));
        assert!(!target.with_extension("zip.part").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_set_never_has_more_tracks_than_live_lite_takes() {
        // (mix on, stems on, MIDI parts on)
        assert_eq!(track_choice(0, 3), (true, 0, 3));
        assert_eq!(track_choice(6, 3), (false, 6, 2));
        assert_eq!(track_choice(4, 2), (false, 4, 2));
        assert_eq!(track_choice(0, 9), (true, 0, 7));
        for stems in 0..=10 {
            for midi in 0..=10 {
                let (mix, on, parts) = track_choice(stems, midi);
                assert!(usize::from(mix) + on + parts <= LIVE_LITE_TRACKS);
            }
        }
    }

    #[test]
    fn without_a_score_the_read_me_says_the_tempo_is_not_the_songs() {
        let contents = Contents { title: "x".into(), bpm: 120.0, meter: (4, 4), mix: PathBuf::from("m.wav"), stems: vec![], midi: vec![], tempo_from_score: false };
        assert!(read_me(&contents, &[], &[]).contains("not the song's"));
    }
}
