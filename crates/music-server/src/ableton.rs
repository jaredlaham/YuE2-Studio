//! Ableton Live sets, written from a set Live itself saved.
//!
//! The `.als` format is gzipped XML that Ableton does not document, so nothing
//! here is written from a guess at it: a set saved by Live 12 is the template,
//! and the set made for a song is that file with its MIDI and audio tracks
//! copied, renamed and given fresh ids, and its tempo and time signature set.
//! Everything else (devices, returns, the main track, Live's own settings) is
//! left exactly as Live wrote it.
//!
//! Live numbers two kinds of things. Automation and modulation targets and
//! pointees share one counter for the whole set (`NextPointeeId` is the next
//! free one), so every copy gets new ones; tracks have ids of their own. Clip
//! slots, sends and lanes are numbered within their track and stay as they are.

use std::io::{Read, Write};

use anyhow::{anyhow, bail, Context, Result};
use regex::Regex;

/// The set Live Lite 12.4.6 saved, empty: one MIDI track, one audio track,
/// the Reverb and Delay returns and the main track.
pub const LIVE_12_TEMPLATE: &[u8] = include_bytes!("../assets/kinetic/live12-default.als");

/// One track to make, named as it shows in Live.
#[derive(Debug, Clone)]
pub struct TrackPlan {
    pub name: String,
    /// One of Live's clip colour indices (0..=69).
    pub color: u32,
}

#[derive(Debug, Clone)]
pub struct SetPlan {
    pub bpm: f64,
    pub meter: (u32, u32),
    /// MIDI tracks first, as Live's default set has them, then audio.
    pub midi: Vec<TrackPlan>,
    pub audio: Vec<TrackPlan>,
}

pub fn gunzip(data: &[u8]) -> Result<String> {
    let mut text = String::new();
    flate2::read::GzDecoder::new(data).read_to_string(&mut text).context("an .als is gzipped XML")?;
    Ok(text)
}

pub fn gzip(text: &str) -> Result<Vec<u8>> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(text.as_bytes())?;
    Ok(encoder.finish()?)
}

/// Where the first element opening with `open` starts and where its closing tag ends.
fn element(xml: &str, open: &str, close: &str) -> Option<(usize, usize)> {
    let start = xml.find(open)?;
    let end = xml[start..].find(close)? + start + close.len();
    Some((start, end))
}

/// Live's encoding of a time signature: 99 for each doubling of the denominator, plus the numerator less one.
pub fn time_signature_value(numerator: u32, denominator: u32) -> Result<u32> {
    if !(1..=99).contains(&numerator) || !denominator.is_power_of_two() || !(1..=16).contains(&denominator) {
        bail!("Live has no time signature {numerator}/{denominator}");
    }
    Ok(99 * denominator.trailing_zeros() + numerator - 1)
}

fn attribute(value: &str) -> String {
    value.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

fn global_ids() -> &'static Regex {
    static PATTERN: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r#"<(\w*Target|Pointee) Id="(\d+)""#).expect("a valid pattern"))
}

/// A copy of a track with its own track id, fresh set-wide ids, a name and a colour.
fn copy_track(block: &str, kind: &str, track_id: u64, next: &mut u64, plan: &TrackPlan) -> Result<String> {
    let renumbered = global_ids().replace_all(block, |captures: &regex::Captures| {
        let id = *next;
        *next += 1;
        format!(r#"<{} Id="{id}""#, &captures[1])
    });
    let mut text = renumbered.into_owned();
    // the track's own id, on its opening tag
    let head_end = text.find('>').ok_or_else(|| anyhow!("a {kind} without an opening tag"))?;
    let head = Regex::new(r#"Id="\d+""#)?.replace(&text[..head_end], format!(r#"Id="{track_id}""#)).into_owned();
    text.replace_range(..head_end, &head);
    // its name: the first Name block is the track's
    let (name_start, name_end) = element(&text, "<Name>", "</Name>").ok_or_else(|| anyhow!("a {kind} without a name"))?;
    let name = &text[name_start..name_end];
    let named = Regex::new(r#"<UserName Value="[^"]*" />"#)?
        .replace(name, format!(r#"<UserName Value="{}" />"#, attribute(&plan.name)))
        .into_owned();
    text.replace_range(name_start..name_end, &named);
    // and its colour, the first Color after the name
    let color_at = text[name_start..].find("<Color Value=\"").map(|at| at + name_start).ok_or_else(|| anyhow!("a {kind} without a colour"))?;
    let color_end = text[color_at..].find("/>").map(|at| at + color_at + 2).ok_or_else(|| anyhow!("a broken colour"))?;
    text.replace_range(color_at..color_end, &format!(r#"<Color Value="{}" />"#, plan.color.min(69)));
    Ok(text)
}

/// The template with the planned tracks in place of its own, at the planned tempo and meter.
pub fn build_xml(template: &str, plan: &SetPlan) -> Result<String> {
    if !(20.0..=999.0).contains(&plan.bpm) {
        bail!("Live's tempo runs from 20 to 999 BPM, not {}", plan.bpm);
    }
    let signature = time_signature_value(plan.meter.0, plan.meter.1)?;
    let (midi_start, midi_end) = element(template, "<MidiTrack ", "</MidiTrack>").ok_or_else(|| anyhow!("the template has no MIDI track"))?;
    let (audio_start, audio_end) = element(template, "<AudioTrack ", "</AudioTrack>").ok_or_else(|| anyhow!("the template has no audio track"))?;
    if audio_start != midi_end + template[midi_end..audio_start].len() || template[midi_end..audio_start].trim() != "" {
        bail!("the template's MIDI and audio tracks are not side by side");
    }
    let midi_block = &template[midi_start..midi_end];
    let audio_block = &template[audio_start..audio_end];
    let indent = &template[template[..midi_start].rfind('\n').map_or(0, |at| at + 1)..midi_start];

    let next_pattern = Regex::new(r#"<NextPointeeId Value="(\d+)" />"#)?;
    let mut next: u64 = next_pattern.captures(template).and_then(|found| found[1].parse().ok()).ok_or_else(|| anyhow!("the template has no NextPointeeId"))?;
    // track ids: above every id any track or return already has
    let track_ids = Regex::new(r#"<(?:MidiTrack|AudioTrack|ReturnTrack|GroupTrack) Id="(\d+)""#)?;
    let mut track_id = track_ids.captures_iter(template).filter_map(|found| found[1].parse::<u64>().ok()).max().unwrap_or(0) + 1;

    let mut tracks = Vec::new();
    for (plans, block, kind) in [(&plan.midi, midi_block, "MidiTrack"), (&plan.audio, audio_block, "AudioTrack")] {
        for track in plans {
            tracks.push(copy_track(block, kind, track_id, &mut next, track)?);
            track_id += 1;
        }
    }
    let joined = tracks.join(&format!("\n{indent}"));
    let mut xml = format!("{}{}{}", &template[..midi_start], joined, &template[audio_end..]);
    xml = next_pattern.replace(&xml, format!(r#"<NextPointeeId Value="{next}" />"#)).into_owned();

    // tempo and time signature live on the main track's mixer
    let (main_start, main_end) = element(&xml, "<MainTrack ", "</MainTrack>").ok_or_else(|| anyhow!("the template has no main track"))?;
    let mut main = xml[main_start..main_end].to_string();
    for (tag, value) in [("<Tempo>", format_number(plan.bpm)), ("<TimeSignature>", signature.to_string())] {
        let at = main.find(tag).ok_or_else(|| anyhow!("the main track has no {tag}"))?;
        let manual = main[at..].find("<Manual Value=\"").map(|found| found + at).ok_or_else(|| anyhow!("{tag} has no value"))?;
        let end = main[manual..].find("/>").map(|found| found + manual + 2).ok_or_else(|| anyhow!("a broken {tag}"))?;
        main.replace_range(manual..end, &format!(r#"<Manual Value="{value}" />"#));
    }
    xml.replace_range(main_start..main_end, &main);
    Ok(xml)
}

fn format_number(value: f64) -> String {
    if value.fract() == 0.0 { format!("{}", value as i64) } else { format!("{value}") }
}

/// A gzipped set from a gzipped template.
pub fn build(template_gz: &[u8], plan: &SetPlan) -> Result<Vec<u8>> {
    gzip(&build_xml(&gunzip(template_gz)?, plan)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn plan() -> SetPlan {
        SetPlan {
            bpm: 92.0,
            meter: (6, 8),
            midi: vec![TrackPlan { name: "Lead (from score)".into(), color: 14 }, TrackPlan { name: "Chords (from score)".into(), color: 15 }],
            audio: ["Mix", "Drums (separated)", "Bass (separated)"].iter().map(|name| TrackPlan { name: (*name).into(), color: 3 }).collect(),
        }
    }

    /// The element names of a subtree in document order, with values blanked: the shape Live reads.
    fn shape(node: roxmltree::Node) -> Vec<String> {
        node.descendants().filter(|node| node.is_element()).map(|node| node.tag_name().name().to_string()).collect()
    }

    #[test]
    fn time_signatures_use_lives_encoding() {
        // the template's 4/4 is stored as 201
        assert_eq!(time_signature_value(4, 4).unwrap(), 201);
        assert_eq!(time_signature_value(3, 4).unwrap(), 200);
        assert_eq!(time_signature_value(6, 8).unwrap(), 302);
        assert!(time_signature_value(4, 3).is_err());
    }

    #[test]
    fn the_template_is_live_12() {
        let xml = gunzip(LIVE_12_TEMPLATE).unwrap();
        let document = roxmltree::Document::parse(&xml).unwrap();
        let root = document.root_element();
        assert_eq!(root.attribute("Creator"), Some("Ableton Live 12.4.6"));
        assert!(xml.contains(r#"<Manual Value="201" />"#));
    }

    #[test]
    fn a_built_set_has_the_planned_tracks_and_live_shaped_xml() {
        let template = gunzip(LIVE_12_TEMPLATE).unwrap();
        let built = gunzip(&build(LIVE_12_TEMPLATE, &plan()).unwrap()).unwrap();
        let document = roxmltree::Document::parse(&built).expect("well-formed XML");
        let original = roxmltree::Document::parse(&template).unwrap();
        let tracks: Vec<roxmltree::Node> = document.descendants().filter(|node| node.has_tag_name("Tracks")).next().unwrap().children().filter(|node| node.is_element()).collect();
        let kinds: Vec<&str> = tracks.iter().map(|node| node.tag_name().name()).collect();
        assert_eq!(kinds, ["MidiTrack", "MidiTrack", "AudioTrack", "AudioTrack", "AudioTrack", "ReturnTrack", "ReturnTrack"]);
        let names: Vec<&str> = tracks
            .iter()
            .take(5)
            .map(|track| track.children().find(|node| node.has_tag_name("Name")).unwrap().children().find(|node| node.has_tag_name("UserName")).unwrap().attribute("Value").unwrap())
            .collect();
        assert_eq!(names, ["Lead (from score)", "Chords (from score)", "Mix", "Drums (separated)", "Bass (separated)"]);
        // every copy has exactly the shape of the track Live saved
        let template_midi = original.descendants().find(|node| node.has_tag_name("MidiTrack")).unwrap();
        let template_audio = original.descendants().find(|node| node.has_tag_name("AudioTrack")).unwrap();
        for track in &tracks[..2] {
            assert_eq!(shape(*track), shape(template_midi));
        }
        for track in &tracks[2..5] {
            assert_eq!(shape(*track), shape(template_audio));
        }
        // set-wide ids are unique and below NextPointeeId; track ids are unique
        let next: u64 = document.descendants().find(|node| node.has_tag_name("NextPointeeId")).unwrap().attribute("Value").unwrap().parse().unwrap();
        let mut seen = HashSet::new();
        for node in document.descendants().filter(|node| node.is_element() && (node.tag_name().name().ends_with("Target") || node.has_tag_name("Pointee"))) {
            if let Some(id) = node.attribute("Id") {
                let id: u64 = id.parse().unwrap();
                assert!(id < next, "{} id {id} is not below NextPointeeId {next}", node.tag_name().name());
                assert!(seen.insert(id), "id {id} is used twice");
            }
        }
        let track_ids: Vec<&str> = tracks.iter().map(|track| track.attribute("Id").unwrap()).collect();
        assert_eq!(track_ids.iter().collect::<HashSet<_>>().len(), track_ids.len());
        // tempo and meter on the main track
        let main = document.descendants().find(|node| node.has_tag_name("MainTrack")).unwrap();
        let manual = |tag: &str| main.descendants().find(|node| node.has_tag_name(tag)).unwrap().children().find(|node| node.has_tag_name("Manual")).unwrap().attribute("Value").unwrap().to_string();
        assert_eq!(manual("Tempo"), "92");
        assert_eq!(manual("TimeSignature"), "302");
        // the rest of the set is untouched: returns and main track keep their shape
        let main_before = original.descendants().find(|node| node.has_tag_name("MainTrack")).unwrap();
        assert_eq!(shape(main), shape(main_before));
    }

    #[test]
    fn names_are_escaped_and_tempo_is_checked() {
        let mut odd = plan();
        odd.midi[0].name = r#"Lead "A" <1> & 2"#.into();
        let built = build_xml(&gunzip(LIVE_12_TEMPLATE).unwrap(), &odd).unwrap();
        let document = roxmltree::Document::parse(&built).unwrap();
        assert!(document.descendants().any(|node| node.attribute("Value") == Some(r#"Lead "A" <1> & 2"#)));
        odd.bpm = 5.0;
        assert!(build_xml(&gunzip(LIVE_12_TEMPLATE).unwrap(), &odd).is_err());
    }
}
