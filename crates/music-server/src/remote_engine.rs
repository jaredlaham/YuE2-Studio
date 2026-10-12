//! A music engine on another computer: Kinetic Studio on a Mac can send its
//! work to `yue-server` running on a machine with an NVIDIA card instead of
//! starting its own engine.
//!
//! The address is kept in one file in the data folder and read once at launch,
//! like the engine's other launch settings, so a change applies after a
//! restart. While it is set, the studio starts no engine of its own, does not
//! ask for local model files and leaves the remote engine's launch flags alone:
//! they belong to whoever started it. `yue-server` has no password and sends
//! audio as plain HTTP, so the address meant here is the near end of an SSH
//! tunnel; anything else is accepted with a warning that says so.

use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{api_error, ApiError, AppState};

const FILE: &str = "kinetic-engine.json";

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct Saved {
    /// The engine's address, `http://host:port`; none means this computer's own engine.
    #[serde(default)]
    pub url: Option<String>,
}

fn path() -> Option<PathBuf> {
    crate::studio_data_root().map(|root| root.join(FILE))
}

pub fn read(path: &Path) -> Saved {
    std::fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

fn write(path: &Path, saved: &Saved) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("json.part");
    std::fs::write(&temporary, serde_json::to_vec_pretty(saved)?)?;
    std::fs::rename(&temporary, path)?;
    Ok(())
}

/// The address saved for this launch, if the studio is to use a remote engine.
pub fn saved_url() -> Option<String> {
    path().map(|path| read(&path)).and_then(|saved| saved.url).and_then(|url| normalise(&url).ok())
}

/// `scheme://host:port` with nothing after it, or why the text is not an engine address.
pub fn normalise(text: &str) -> Result<String, String> {
    let text = text.trim();
    let url = reqwest::Url::parse(text).map_err(|_| format!("\"{text}\" is not an address like http://127.0.0.1:18187"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("the address must start with http:// or https://".into());
    }
    let Some(host) = url.host_str() else { return Err("the address has no host".into()) };
    if !url.username().is_empty() || url.password().is_some() {
        return Err("put no user name or password in the address".into());
    }
    if url.query().is_some() || url.fragment().is_some() || !matches!(url.path(), "" | "/") {
        return Err("give only the engine's address and port, nothing after them".into());
    }
    let port = url.port_or_known_default().ok_or("the address has no port")?;
    Ok(format!("{}://{}:{port}", url.scheme(), host))
}

/// Whether traffic to the address stays on this computer (an SSH tunnel's near end).
pub fn is_loopback(url: &str) -> bool {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .is_some_and(|host| host == "localhost" || host.trim_matches(['[', ']']).parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback()))
}

fn warning(url: &str) -> Option<String> {
    (!is_loopback(url)).then(|| {
        "This address leaves your computer. yue-server has no password and sends your prompts and audio unencrypted, \
         so anyone who can reach it can use it. Reach it through an SSH tunnel instead."
            .to_owned()
    })
}

pub async fn status(State(state): State<AppState>) -> Json<Value> {
    let saved = path().map(|path| read(&path)).unwrap_or_default().url;
    let active = state.music_server.remote.then(|| state.music_server.base_url.clone());
    Json(json!({
        "saved": saved,
        "active": active,
        "restart_needed": saved != active,
        "warning": saved.as_deref().and_then(warning),
    }))
}

#[derive(Debug, Deserialize)]
pub struct Change {
    pub url: Option<String>,
}

pub async fn change(State(state): State<AppState>, Json(change): Json<Change>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let url = match change.url.as_deref().map(str::trim).filter(|text| !text.is_empty()) {
        Some(text) => Some(normalise(text).map_err(|error| api_error(StatusCode::BAD_REQUEST, error))?),
        None => None,
    };
    let path = path().ok_or_else(|| api_error(StatusCode::INTERNAL_SERVER_ERROR, "the studio has no data folder".into()))?;
    write(&path, &Saved { url: url.clone() }).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("write {}: {error}", path.display())))?;
    let active = state.music_server.remote.then(|| state.music_server.base_url.clone());
    Ok(Json(json!({
        "saved": url,
        "active": active,
        "restart_needed": url != active,
        "warning": url.as_deref().and_then(warning),
    })))
}

#[derive(Debug, Deserialize)]
pub struct Probe {
    pub url: String,
}

/// Asks an address whether a yue-server answers there and what it has loaded, without saving anything.
pub async fn test(Json(probe): Json<Probe>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let url = normalise(&probe.url).map_err(|error| api_error(StatusCode::BAD_REQUEST, error))?;
    let http = crate::net::builder().build().map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let health = http.get(format!("{url}/health")).timeout(Duration::from_secs(5)).send().await;
    let reached = match health {
        Ok(response) if response.status().is_success() => true,
        Ok(response) => {
            return Ok(Json(json!({ "ok": false, "url": url, "error": format!("something answered with {} but it is not a ready yue-server", response.status()) })));
        }
        Err(error) => {
            let reason = if error.is_timeout() { "no answer within 5 seconds".to_owned() } else if error.is_connect() { "nothing is listening there".to_owned() } else { error.to_string() };
            return Ok(Json(json!({ "ok": false, "url": url, "error": reason, "warning": warning(&url) })));
        }
    };
    // what the engine has loaded, as it reports it; an engine without /props is still an engine
    let props = match http.get(format!("{url}/props")).timeout(Duration::from_secs(10)).send().await {
        Ok(response) if response.status().is_success() => response.json::<Value>().await.ok(),
        _ => None,
    };
    Ok(Json(json!({ "ok": reached, "url": url, "props": props, "warning": warning(&url) })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_is_reduced_to_scheme_host_and_port() {
        assert_eq!(normalise(" http://127.0.0.1:18187/ ").unwrap(), "http://127.0.0.1:18187");
        assert_eq!(normalise("http://gpu-box:18087").unwrap(), "http://gpu-box:18087");
        assert_eq!(normalise("https://engine.example").unwrap(), "https://engine.example:443");
        assert_eq!(normalise("http://[::1]:9000").unwrap(), "http://[::1]:9000");
    }

    #[test]
    fn anything_but_a_bare_http_address_is_refused() {
        for text in ["127.0.0.1:18087", "ftp://host:21", "http://user:pass@host:1", "http://host:1/v1", "http://host:1/?x=1", "not an address", ""] {
            assert!(normalise(text).is_err(), "{text}");
        }
    }

    #[test]
    fn only_this_computer_counts_as_loopback() {
        assert!(is_loopback("http://127.0.0.1:18187"));
        assert!(is_loopback("http://localhost:18187"));
        assert!(is_loopback("http://[::1]:18187"));
        assert!(!is_loopback("http://192.168.1.20:18087"));
        assert!(!is_loopback("http://gpu-box:18087"));
        assert!(warning("http://192.168.1.20:18087").is_some());
        assert!(warning("http://127.0.0.1:18187").is_none());
    }

    #[test]
    fn the_address_survives_a_save_and_a_missing_or_damaged_file_means_this_computer() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join(FILE);
        assert_eq!(read(&path), Saved::default());
        write(&path, &Saved { url: Some("http://127.0.0.1:18187".into()) }).unwrap();
        assert_eq!(read(&path).url.as_deref(), Some("http://127.0.0.1:18187"));
        std::fs::write(&path, b"{").unwrap();
        assert_eq!(read(&path), Saved::default());
    }
}
