//! Mojang's version manifest and version JSONs, cached on disk.
//!
//! Versions are opaque strings (1.21.11, 26.1, 26.1-snapshot-3, b1.7.3 ...).
//! Never parse them as semver; order comes from `releaseTime`.

use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use super::version::VersionJson;
use crate::download::{download_all, DownloadTask};
use crate::progress::{self, Stage};
use crate::{Launcher, Result};

pub const MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const MANIFEST_TTL: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionManifest {
    pub latest: Latest,
    pub versions: Vec<GameVersion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Latest {
    pub release: String,
    pub snapshot: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameVersion {
    pub id: String,
    /// release, snapshot, old_beta, old_alpha
    #[serde(rename = "type")]
    pub kind: String,
    pub url: String,
    pub time: String,
    pub release_time: String,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub compliance_level: Option<u32>,
}

/// Fresh manifest if online; the cached copy if the network is down.
pub async fn manifest(launcher: &Launcher) -> Result<VersionManifest> {
    let cache = launcher.dirs.meta().join("version_manifest_v2.json");
    let fresh = tokio::fs::metadata(&cache)
        .await
        .and_then(|m| m.modified())
        .map(|t| SystemTime::now().duration_since(t).unwrap_or_default() < MANIFEST_TTL)
        .unwrap_or(false);

    if !fresh {
        match crate::download::fetch_bytes(&launcher.http, MANIFEST_URL).await {
            Ok(bytes) => {
                let parsed: VersionManifest = serde_json::from_slice(&bytes)?;
                tokio::fs::write(&cache, &bytes).await?;
                return Ok(parsed);
            }
            Err(e) => tracing::warn!("manifest fetch failed, using cache: {e}"),
        }
    }
    let bytes = tokio::fs::read(&cache).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

/// The vanilla version JSON for `id`, downloaded and hash-checked once, then read from disk.
pub async fn version_json(launcher: &Launcher, id: &str) -> Result<VersionJson> {
    let path = launcher.dirs.version_json(id);
    if !path.exists() {
        let manifest = manifest(launcher).await?;
        let entry =
            manifest.versions.iter().find(|v| v.id == id).ok_or_else(|| crate::Error::NotFound(format!("Minecraft version {id}")))?;
        download_all(
            &launcher.http,
            vec![DownloadTask::new(&entry.url, &path).sha1(entry.sha1.clone())],
            1,
            Stage::Metadata,
            &progress::noop(),
        )
        .await?;
    }
    let bytes = tokio::fs::read(&path).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_new_style_ids() {
        let json = r#"{"latest":{"release":"26.2","snapshot":"26.3-snapshot-5"},
          "versions":[{"id":"26.3-snapshot-5","type":"snapshot","url":"u","time":"t","releaseTime":"2026-07-28T10:00:00+00:00","sha1":"a","complianceLevel":1}]}"#;
        let m: VersionManifest = serde_json::from_str(json).unwrap();
        assert_eq!(m.latest.release, "26.2");
        assert_eq!(m.versions[0].kind, "snapshot");
    }
}
