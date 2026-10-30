//! Mod loader version lists and launch profiles.
//!
//! Fabric and Quilt publish ready-to-merge profiles. Forge and NeoForge versions are listed
//! from Modrinth's loader metadata and installed from their official installers (`forge.rs`).

use serde::{Deserialize, Serialize};

use super::version::VersionJson;
use crate::http::get_json;
use crate::{Error, Launcher, Result};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Loader {
    Vanilla,
    Fabric,
    Quilt,
    Forge,
    NeoForge,
}

impl Loader {
    /// The loader name Modrinth and CurseForge use in filters.
    pub fn modrinth_name(self) -> Option<&'static str> {
        match self {
            Loader::Vanilla => None,
            Loader::Fabric => Some("fabric"),
            Loader::Quilt => Some("quilt"),
            Loader::Forge => Some("forge"),
            Loader::NeoForge => Some("neoforge"),
        }
    }

    pub fn from_modrinth(name: &str) -> Option<Self> {
        Some(match name {
            "fabric" | "fabric-loader" => Loader::Fabric,
            "quilt" | "quilt-loader" => Loader::Quilt,
            "forge" => Loader::Forge,
            "neoforge" | "neo-forge" => Loader::NeoForge,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LoaderVersion {
    pub id: String,
    pub stable: bool,
    /// The build a new instance of this game version should get (see [`recommended`]).
    pub recommended: bool,
}

#[derive(Deserialize)]
struct FabricEntry {
    loader: FabricLoader,
}
#[derive(Deserialize)]
struct FabricLoader {
    version: String,
    #[serde(default)]
    stable: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModrinthLoaderManifest {
    game_versions: Vec<ModrinthGameVersion>,
}
#[derive(Deserialize)]
struct ModrinthGameVersion {
    id: String,
    loaders: Vec<ModrinthLoaderEntry>,
}
#[derive(Deserialize)]
struct ModrinthLoaderEntry {
    id: String,
    #[serde(default)]
    stable: bool,
}

/// Loader versions for a game version, newest first.
pub async fn versions(launcher: &Launcher, loader: Loader, game: &str) -> Result<Vec<LoaderVersion>> {
    match loader {
        Loader::Vanilla => Ok(vec![]),
        Loader::Fabric | Loader::Quilt => {
            let url = match loader {
                Loader::Fabric => format!("https://meta.fabricmc.net/v2/versions/loader/{game}"),
                _ => format!("https://meta.quiltmc.org/v3/versions/loader/{game}"),
            };
            // Both answer 400 for a game version they have no loader for (1.12.2, say).
            let resp = launcher.http.get(&url).send().await?;
            if matches!(resp.status().as_u16(), 400 | 404) {
                return Ok(vec![]);
            }
            let entries: Vec<FabricEntry> = resp.error_for_status()?.json().await?;
            Ok(entries
                .into_iter()
                .map(|e| LoaderVersion {
                    stable: e.loader.stable.unwrap_or(!e.loader.version.contains("beta")),
                    id: e.loader.version,
                    recommended: false,
                })
                .collect())
        }
        Loader::Forge | Loader::NeoForge => {
            let kind = if loader == Loader::Forge { "forge" } else { "neo" };
            let url = format!("https://launcher-meta.modrinth.com/{kind}/v0/manifest.json");
            let manifest: ModrinthLoaderManifest = get_json(&launcher.http, &url).await?;
            Ok(manifest
                .game_versions
                .into_iter()
                .find(|g| g.id == game)
                .map(|g| g.loaders.into_iter().map(|l| LoaderVersion { id: l.id, stable: l.stable, recommended: false }).collect())
                .unwrap_or_default())
        }
    }
}

/// Picks the version a new instance should get: Forge's own "recommended" (else "latest")
/// promotion, NeoForge's newest non-beta, Fabric's and Quilt's newest stable.
pub async fn recommended(launcher: &Launcher, loader: Loader, game: &str) -> Result<Option<String>> {
    let list = versions(launcher, loader, game).await?;
    Ok(pick_recommended(launcher, loader, game, &list).await)
}

/// [`versions`] with the recommended build marked, for the version picker. Fabric and Quilt
/// loaders are one line for every game version, so their pick is the same everywhere; Forge
/// and NeoForge builds belong to one game version each.
pub async fn versions_marked(launcher: &Launcher, loader: Loader, game: &str) -> Result<Vec<LoaderVersion>> {
    let mut list = versions(launcher, loader, game).await?;
    if let Some(pick) = pick_recommended(launcher, loader, game, &list).await {
        list.iter_mut().filter(|v| v.id == pick).for_each(|v| v.recommended = true);
    }
    Ok(list)
}

async fn pick_recommended(launcher: &Launcher, loader: Loader, game: &str, list: &[LoaderVersion]) -> Option<String> {
    if loader == Loader::Forge {
        if let Some(promoted) = super::forge::forge_promoted(launcher, game).await {
            if list.iter().any(|v| v.id == promoted) {
                return Some(promoted);
            }
        }
    }
    if loader == Loader::NeoForge {
        if let Some(v) = list.iter().find(|v| !v.id.contains("beta") && !v.id.contains("alpha")) {
            return Some(v.id.clone());
        }
    }
    list.iter().find(|v| v.stable).or(list.first()).map(|v| v.id.clone())
}

/// The loader's version profile, ready to merge onto vanilla. Cached under `versions/`.
pub async fn profile(launcher: &Launcher, loader: Loader, game: &str, loader_version: &str) -> Result<VersionJson> {
    let url = match loader {
        Loader::Fabric => format!("https://meta.fabricmc.net/v2/versions/loader/{game}/{loader_version}/profile/json"),
        Loader::Quilt => format!("https://meta.quiltmc.org/v3/versions/loader/{game}/{loader_version}/profile/json"),
        Loader::Forge | Loader::NeoForge => {
            return super::forge::profile(launcher, &super::forge::Target::new(loader, game, loader_version)?).await
        }
        Loader::Vanilla => return Err(Error::other("vanilla has no loader profile")),
    };
    let cache_id = format!("{}-{loader_version}-{game}", loader.modrinth_name().unwrap_or("x"));
    let path = launcher.dirs.version_json(&cache_id);
    if let Ok(bytes) = tokio::fs::read(&path).await {
        if let Ok(v) = serde_json::from_slice(&bytes) {
            return Ok(v);
        }
    }
    let bytes = crate::download::fetch_bytes(&launcher.http, &url).await?;
    let parsed: VersionJson = serde_json::from_slice(&bytes)?;
    tokio::fs::create_dir_all(launcher.dirs.version_dir(&cache_id)).await?;
    tokio::fs::write(&path, &bytes).await?;
    Ok(parsed)
}
