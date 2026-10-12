//! What an agent connected over MCP may touch on this computer.
//!
//! Files: a tool that reads or writes a file the agent names by its path may
//! only reach the studio's own data folder, the user's Music and Downloads
//! folders, and folders the user added in Settings. Paths are resolved first
//! (symbolic links, `..`), so a path that only looks inside one of them is
//! refused.
//!
//! Consent: a tool that deletes or overwrites something, or reaches the
//! internet (downloads, OpenRouter, Hugging Face), runs only after the user
//! allows it in the studio's window. With no window open it does not run.

use std::path::{Component, Path, PathBuf};

use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde_json::{json, Value};

/// Every argument that names a file or folder on this computer, by tool.
/// `a.b` is a field inside an object; `[]` marks a list of paths.
pub const LOCAL_PATHS: &[(&str, &[&str])] = &[
    ("models_adopt", &["path"]),
    ("equalizer_import", &["path"]),
    ("equalizer_export", &["path"]),
    ("winamp_skin_add", &["path"]),
    ("video_set", &["album_art_path", "background.image_path", "background.video_path"]),
    ("score_transcribe", &["path"]),
    ("score_match_sections", &["path"]),
    ("library_import_audio", &["path"]),
    ("midi_transcribe", &["path"]),
    ("assistant_set", &["managed_path"]),
    ("assistant_start", &["model_path"]),
    ("cover_set_from_file", &["path"]),
    ("openrouter_transcribe", &["path"]),
    ("processing_set_reference", &["path"]),
    ("vst_open_editor", &["path"]),
    ("lora_import_files", &["paths[]"]),
    ("dataset_import_folder", &["path"]),
    ("dataset_add_folder", &["path"]),
    ("lora_export_comfyui", &["path"]),
];

/// Arguments named like paths that are not on this computer.
#[cfg_attr(not(test), allow(dead_code))] // read by the test that holds every tool to these lists
pub const NOT_LOCAL: &[(&str, &str)] = &[
    // file names inside a Hugging Face repository
    ("lora_install_hf", "paths"),
];

const FOLDERS_FILE: &str = "kinetic-mcp-folders.json";

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from)
}

/// The folders the user added for agents, as saved.
pub fn added_folders(data_root: &Path) -> Vec<PathBuf> {
    std::fs::read(data_root.join(FOLDERS_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|value| value.get("folders").and_then(Value::as_array).cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_str)
        .map(PathBuf::from)
        .collect()
}

/// The folders an agent may reach, each as written (not yet resolved).
pub fn allowed_folders(data_root: Option<&Path>, home: Option<&Path>) -> Vec<PathBuf> {
    let mut folders = Vec::new();
    if let Some(root) = data_root {
        folders.push(root.to_path_buf());
    }
    if let Some(home) = home {
        folders.push(home.join("Music"));
        folders.push(home.join("Downloads"));
    }
    if let Some(root) = data_root {
        folders.extend(added_folders(root));
    }
    folders
}

/// A path with links and `..` resolved. One that does not exist yet (a file
/// to be written) is resolved through the nearest folder that does, and may
/// not climb out of it.
pub fn resolve(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err(format!("{} is not a full path; give the whole path, starting at the root of the disk", path.display()));
    }
    let mut existing = path.to_path_buf();
    let mut rest = Vec::new();
    while !existing.exists() {
        let name = existing.file_name().map(|name| name.to_os_string()).ok_or_else(|| format!("{} has no folder on this computer", path.display()))?;
        rest.push(name);
        existing = existing.parent().map(Path::to_path_buf).ok_or_else(|| format!("{} has no folder on this computer", path.display()))?;
    }
    let mut resolved = std::fs::canonicalize(&existing).map_err(|error| format!("{}: {error}", existing.display()))?;
    for name in rest.into_iter().rev() {
        if Path::new(&name).components().any(|part| !matches!(part, Component::Normal(_))) {
            return Err(format!("{} climbs out of its folder", path.display()));
        }
        resolved.push(name);
    }
    Ok(resolved)
}

/// Whether `path` is inside one of `folders`, both resolved.
pub fn inside(path: &Path, folders: &[PathBuf]) -> Result<(), String> {
    let resolved = resolve(path)?;
    let allowed = folders.iter().filter_map(|folder| std::fs::canonicalize(folder).ok()).any(|folder| resolved.starts_with(&folder));
    if allowed {
        Ok(())
    } else {
        let listed: Vec<String> = folders.iter().map(|folder| folder.display().to_string()).collect();
        Err(format!(
            "{} is outside the folders agents may use ({}). Move the file into one of them, or add its folder in Settings → Agents (MCP) → Folders agents may use.",
            path.display(),
            listed.join(", ")
        ))
    }
}

/// The values of one argument pointer (`path`, `background.image_path`, `paths[]`).
fn values<'a>(args: &'a Value, pointer: &str) -> Vec<&'a str> {
    let (pointer, list) = match pointer.strip_suffix("[]") {
        Some(name) => (name, true),
        None => (pointer, false),
    };
    let mut value = Some(args);
    for part in pointer.split('.') {
        value = value.and_then(|inner| inner.get(part));
    }
    match value {
        Some(Value::String(text)) if !list => vec![text.as_str()],
        Some(Value::Array(items)) if list => items.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    }
    .into_iter()
    .map(str::trim)
    .filter(|text| !text.is_empty())
    .collect()
}

/// Refuses a call whose local paths reach outside `folders`.
pub fn check_paths(tool: &str, args: &Value, folders: &[PathBuf]) -> Result<(), String> {
    let Some((_, pointers)) = LOCAL_PATHS.iter().find(|(name, _)| *name == tool) else { return Ok(()) };
    for pointer in *pointers {
        for path in values(args, pointer) {
            inside(Path::new(path), folders)?;
        }
    }
    Ok(())
}

/// The folders agents may use, for this computer.
pub fn folders_here() -> Vec<PathBuf> {
    allowed_folders(crate::studio_data_root().as_deref(), home().as_deref())
}

/// Whether a tool waits for the user to allow it: it deletes or overwrites,
/// or it reaches the internet.
pub fn needs_consent(annotations: &Value) -> bool {
    annotations["destructiveHint"] == true || annotations["openWorldHint"] == true
}

/// What the consent dialog shows of the arguments: short, and never a key.
pub fn shown_arguments(args: &Value) -> String {
    let mut shown = args.clone();
    if let Some(fields) = shown.as_object_mut() {
        for (name, value) in fields.iter_mut() {
            if name.contains("key") || name.contains("token") || name.contains("secret") {
                *value = json!("(hidden)");
            }
        }
    }
    let text = serde_json::to_string(&shown).unwrap_or_default();
    if text.chars().count() > 400 { format!("{}…", text.chars().take(400).collect::<String>()) } else { text }
}

// ---------------------------------------------------------------- the Settings routes

pub async fn get_folders() -> Json<Value> {
    let root = crate::studio_data_root();
    let added: Vec<String> = root.as_deref().map(added_folders).unwrap_or_default().iter().map(|path| path.display().to_string()).collect();
    let all: Vec<String> = folders_here().iter().map(|path| path.display().to_string()).collect();
    Json(json!({ "folders": all, "added": added }))
}

/// Saves the folders the user added. Only the studio's own window may: a web
/// page in the user's browser must not widen what agents can read.
pub async fn put_folders(headers: HeaderMap, Json(body): Json<Value>) -> Result<Json<Value>, (StatusCode, String)> {
    if headers.get(axum::http::header::ORIGIN).is_none() || !crate::mcp::local_origin(&headers) {
        return Err((StatusCode::FORBIDDEN, "Only the studio's window can change which folders agents may use.".into()));
    }
    let root = crate::studio_data_root().ok_or((StatusCode::INTERNAL_SERVER_ERROR, "the studio has no data folder".into()))?;
    let mut folders = Vec::new();
    for folder in body.get("added").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
        let path = PathBuf::from(folder.trim());
        if !path.is_absolute() || !path.is_dir() {
            return Err((StatusCode::UNPROCESSABLE_ENTITY, format!("{folder} is not a folder on this computer")));
        }
        if path.parent().is_none() {
            return Err((StatusCode::UNPROCESSABLE_ENTITY, "the whole disk cannot be given to agents; choose a folder".into()));
        }
        folders.push(path.display().to_string());
    }
    folders.dedup();
    std::fs::create_dir_all(&root).map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let partial = root.join(format!("{FOLDERS_FILE}.part"));
    std::fs::write(&partial, serde_json::to_vec_pretty(&json!({ "folders": folders })).unwrap_or_default()).map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    std::fs::rename(&partial, root.join(FOLDERS_FILE)).map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(get_folders().await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_counts_only_inside_an_allowed_folder() {
        let root = tempfile::tempdir().unwrap();
        let music = root.path().join("Music");
        let elsewhere = root.path().join("Documents");
        std::fs::create_dir_all(music.join("set")).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::write(music.join("set/a.wav"), b"x").unwrap();
        std::fs::write(elsewhere.join("secret.txt"), b"x").unwrap();
        let folders = vec![music.clone()];
        assert!(inside(&music.join("set/a.wav"), &folders).is_ok());
        // a file to be written there, not yet made
        assert!(inside(&music.join("set/new.eqf"), &folders).is_ok());
        assert!(inside(&elsewhere.join("secret.txt"), &folders).is_err());
        // looks inside, climbs out
        assert!(inside(&music.join("../Documents/secret.txt"), &folders).is_err());
        assert!(inside(&music.join("set/../../Documents/secret.txt"), &folders).is_err());
        assert!(inside(&music.join("nothing/../../Documents/x.txt"), &folders).is_err());
        assert!(inside(Path::new("Music/set/a.wav"), &folders).is_err(), "a relative path is refused");
        let message = inside(&elsewhere.join("secret.txt"), &folders).unwrap_err();
        assert!(message.contains("Settings"));
    }

    #[cfg(unix)]
    #[test]
    fn a_link_out_of_an_allowed_folder_is_followed_and_refused() {
        let root = tempfile::tempdir().unwrap();
        let music = root.path().join("Music");
        let elsewhere = root.path().join("Documents");
        std::fs::create_dir_all(&music).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::write(elsewhere.join("secret.txt"), b"x").unwrap();
        std::os::unix::fs::symlink(&elsewhere, music.join("door")).unwrap();
        assert!(inside(&music.join("door/secret.txt"), &[music.clone()]).is_err());
    }

    #[test]
    fn every_tool_path_argument_is_checked() {
        let root = tempfile::tempdir().unwrap();
        let music = root.path().join("Music");
        std::fs::create_dir_all(&music).unwrap();
        std::fs::write(music.join("a.wav"), b"x").unwrap();
        let folders = vec![music.clone()];
        let outside = root.path().join("a.wav");
        std::fs::write(&outside, b"x").unwrap();
        let ok = music.join("a.wav").display().to_string();
        let bad = outside.display().to_string();
        assert!(check_paths("library_import_audio", &json!({ "path": ok }), &folders).is_ok());
        assert!(check_paths("library_import_audio", &json!({ "path": bad }), &folders).is_err());
        assert!(check_paths("video_set", &json!({ "background": { "image_path": bad } }), &folders).is_err());
        assert!(check_paths("lora_import_files", &json!({ "paths": [ok, bad] }), &folders).is_err());
        assert!(check_paths("lora_install_hf", &json!({ "paths": ["/etc/passwd"] }), &folders).is_ok(), "repository file names are not local");
        assert!(check_paths("score_transcribe", &json!({ "song_id": "s" }), &folders).is_ok(), "an absent path is not checked");
    }

    #[test]
    fn the_default_folders_are_the_studios_music_and_downloads() {
        let data = tempfile::tempdir().unwrap();
        let folders = allowed_folders(Some(data.path()), Some(Path::new("/Users/someone")));
        assert_eq!(folders, [data.path().to_path_buf(), PathBuf::from("/Users/someone/Music"), PathBuf::from("/Users/someone/Downloads")]);
        std::fs::write(data.path().join(FOLDERS_FILE), br#"{"folders":["/Volumes/Samples"]}"#).unwrap();
        assert_eq!(allowed_folders(Some(data.path()), None).last(), Some(&PathBuf::from("/Volumes/Samples")));
    }

    #[test]
    fn deleting_overwriting_and_the_internet_wait_for_the_user() {
        assert!(needs_consent(&json!({ "destructiveHint": true, "openWorldHint": false })));
        assert!(needs_consent(&json!({ "destructiveHint": false, "openWorldHint": true })));
        assert!(!needs_consent(&json!({ "destructiveHint": false, "openWorldHint": false })));
        assert_eq!(shown_arguments(&json!({ "api_key": "sk-123", "model": "m" })), r#"{"api_key":"(hidden)","model":"m"}"#);
    }
}
