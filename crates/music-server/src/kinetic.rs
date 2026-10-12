//! Kinetic Studio's own presets: the structured prompts a person saves from
//! the Create panel, kept in one file in the studio's data folder. The window
//! owns what a prompt holds; the service checks the list's shape, keeps the
//! file whole (a temporary file renamed over the old one) and never touches
//! the built in presets, which live in the window's code.

use std::path::{Path, PathBuf};

use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{api_error, ApiError};

const FILE: &str = "kinetic-presets.json";
/// Far more than anyone keeps by hand; a bound so a runaway client cannot fill the disk.
const MOST: usize = 500;
const NAME_CHARS: usize = 120;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub prompt: Map<String, Value>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Presets {
    #[serde(default)]
    pub presets: Vec<Preset>,
}

fn path() -> Result<PathBuf, (StatusCode, Json<ApiError>)> {
    let root = crate::studio_data_root().ok_or_else(|| api_error(StatusCode::INTERNAL_SERVER_ERROR, "the studio has no data folder".into()))?;
    Ok(root.join(FILE))
}

/// The saved presets; none when nothing was saved yet. A file that does not
/// read is an error rather than an empty list, so a save never replaces it unseen.
pub fn read(path: &Path) -> anyhow::Result<Presets> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Presets::default()),
        Err(error) => Err(error.into()),
    }
}

/// What is wrong with a list, if anything.
pub fn check(presets: &Presets) -> Result<(), String> {
    if presets.presets.len() > MOST {
        return Err(format!("at most {MOST} presets are kept"));
    }
    let mut ids = std::collections::HashSet::new();
    for preset in &presets.presets {
        if preset.id.trim().is_empty() {
            return Err("a preset needs an id".into());
        }
        if preset.name.trim().is_empty() || preset.name.chars().count() > NAME_CHARS {
            return Err(format!("preset names are 1 to {NAME_CHARS} characters"));
        }
        if !ids.insert(preset.id.as_str()) {
            return Err(format!("two presets share the id {}", preset.id));
        }
    }
    Ok(())
}

pub fn write(path: &Path, presets: &Presets) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("json.part");
    std::fs::write(&temporary, serde_json::to_vec_pretty(presets)?)?;
    std::fs::rename(&temporary, path)?;
    Ok(())
}

pub async fn list() -> Result<Json<Presets>, (StatusCode, Json<ApiError>)> {
    let path = path()?;
    read(&path)
        .map(Json)
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("read {}: {error}", path.display())))
}

pub async fn save(Json(presets): Json<Presets>) -> Result<Json<Presets>, (StatusCode, Json<ApiError>)> {
    check(&presets).map_err(|error| api_error(StatusCode::BAD_REQUEST, error))?;
    let path = path()?;
    write(&path, &presets).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("write {}: {error}", path.display())))?;
    Ok(Json(presets))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preset(id: &str, name: &str) -> Preset {
        let mut prompt = Map::new();
        prompt.insert("genre".into(), Value::String("ambient".into()));
        Preset { id: id.into(), name: name.into(), prompt }
    }

    #[test]
    fn nothing_saved_reads_as_no_presets() {
        let folder = tempfile::tempdir().unwrap();
        assert!(read(&folder.path().join(FILE)).unwrap().presets.is_empty());
    }

    #[test]
    fn a_saved_list_reads_back_whole_and_leaves_no_temporary_file() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("nested").join(FILE);
        let presets = Presets { presets: vec![preset("a", "Night drive"), preset("b", "Fog")] };
        write(&path, &presets).unwrap();
        assert_eq!(read(&path).unwrap().presets, presets.presets);
        assert!(!path.with_extension("json.part").exists());
    }

    #[test]
    fn a_damaged_file_is_an_error_not_an_empty_list() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join(FILE);
        std::fs::write(&path, b"{ not json").unwrap();
        assert!(read(&path).is_err());
    }

    #[test]
    fn a_list_with_blank_names_shared_ids_or_too_many_entries_is_refused() {
        assert!(check(&Presets { presets: vec![preset("a", "A"), preset("b", "B")] }).is_ok());
        assert!(check(&Presets { presets: vec![preset("", "A")] }).is_err());
        assert!(check(&Presets { presets: vec![preset("a", "  ")] }).is_err());
        assert!(check(&Presets { presets: vec![preset("a", &"x".repeat(NAME_CHARS + 1))] }).is_err());
        assert!(check(&Presets { presets: vec![preset("a", "A"), preset("a", "B")] }).is_err());
        let many = (0..=MOST).map(|index| preset(&index.to_string(), "P")).collect();
        assert!(check(&Presets { presets: many }).is_err());
    }
}
