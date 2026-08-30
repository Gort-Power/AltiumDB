use serde::Deserialize;
use std::sync::{Arc, Mutex};

/// Shared slot that the background check thread fills with the latest release
/// info (only when it is newer than the running version).
pub type SharedRelease = Arc<Mutex<Option<ReleaseInfo>>>;

#[derive(Clone, Debug)]
pub struct ReleaseInfo {
    /// Release tag without a leading `v` (e.g. `1.2.3`).
    pub version: String,
    /// Raw release tag as published on GitHub (e.g. `v1.2.3`).
    pub tag: String,
    /// Release title (falls back to the tag when empty).
    pub name: String,
    /// Browser URL to the release page.
    pub url: String,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    name: Option<String>,
    html_url: String,
}

const REPO: &str = "Gort-Power/AltiumDB";

/// Running crate version, taken from `Cargo.toml`.
pub fn current_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Spawns a background thread that queries the GitHub "latest release" API and
/// stores the result (if newer than the current version) in the returned slot.
/// Failures are silently ignored so a missing network never blocks the app.
pub fn spawn_check() -> SharedRelease {
    let shared: SharedRelease = Arc::new(Mutex::new(None));
    let cloned = shared.clone();
    std::thread::spawn(move || {
        if let Ok(info) = fetch_latest() {
            if is_newer(&info.version, &current_version()) {
                if let Ok(mut guard) = cloned.lock() {
                    *guard = Some(info);
                }
            }
        }
    });
    shared
}

fn fetch_latest() -> Result<ReleaseInfo, String> {
    let url = format!("https://api.github.com/repos/{}/releases/latest", REPO);
    let agent = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
    let resp = ureq::get(&url)
        .set("User-Agent", agent)
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("network: {}", e))?;
    let release: GithubRelease = resp.into_json().map_err(|e| format!("json: {}", e))?;
    let tag = release.tag_name.trim();
    let version = tag.strip_prefix('v').unwrap_or(tag).to_string();
    let name = release.name.unwrap_or_else(|| tag.to_string());
    Ok(ReleaseInfo {
        version,
        tag: tag.to_string(),
        name,
        url: release.html_url,
    })
}

/// Returns `true` when `remote` is a higher semantic version than `local`.
/// Non-parseable versions are treated as equal (no update shown).
fn is_newer(remote: &str, local: &str) -> bool {
    match (
        semver::Version::parse(remote),
        semver::Version::parse(local),
    ) {
        (Ok(r), Ok(l)) => r > l,
        _ => false,
    }
}
