//! Mods, resource packs and shaders: searching both platforms and managing what's installed.

pub mod blocked;
pub mod check;
pub mod curseforge;
pub mod identify;
pub mod jarmeta;
pub mod links;
pub mod listed;
pub mod manage;
pub mod modrinth;
pub mod scan;
pub mod version_range;

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::instance::Instance;
use crate::settings::write_json_atomic;
use crate::{Error, Launcher, Result};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Modrinth,
    CurseForge,
    Local,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ContentKind {
    Mod,
    ResourcePack,
    Shader,
    DataPack,
    Modpack,
}

impl ContentKind {
    pub fn folder(self) -> &'static str {
        match self {
            ContentKind::Mod => "mods",
            ContentKind::ResourcePack => "resourcepacks",
            ContentKind::Shader => "shaderpacks",
            ContentKind::DataPack => "datapacks",
            ContentKind::Modpack => "",
        }
    }

    pub fn from_modrinth(t: &str) -> Self {
        match t {
            "resourcepack" => Self::ResourcePack,
            "shader" => Self::Shader,
            "datapack" => Self::DataPack,
            "modpack" => Self::Modpack,
            _ => Self::Mod,
        }
    }
}

/// A search result from either platform, in one shape for the Discover page.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentHit {
    pub source: Source,
    pub id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub author: String,
    pub icon_url: Option<String>,
    pub downloads: u64,
    pub follows: u64,
    pub updated: Option<String>,
    pub categories: Vec<String>,
    pub kind: ContentKind,
    pub gallery: Vec<String>,
    /// Modrinth's dominant icon colour, used for card tints.
    pub color: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPage {
    pub hits: Vec<ContentHit>,
    pub total: u64,
    pub offset: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchQuery {
    pub query: String,
    pub kind: Option<ContentKind>,
    pub game_version: Option<String>,
    pub loader: Option<String>,
    pub categories: Vec<String>,
    /// relevance | downloads | follows | newest | updated
    pub sort: Option<String>,
    pub offset: u64,
    pub limit: u64,
}

/// What we know about each installed file, keyed by its path relative to the instance.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContentIndex {
    pub files: BTreeMap<String, ContentMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentMeta {
    pub source: Source,
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    pub title: Option<String>,
    pub version_name: Option<String>,
    pub icon_url: Option<String>,
    pub sha1: Option<String>,
    /// Installed only because another mod needed it; offered for cleanup once nothing does.
    #[serde(default)]
    pub auto: bool,
    /// Project ids the platform says this file needs (on the same platform). Jars often
    /// don't declare these themselves.
    /// None until Alloy has asked the platform.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires: Option<Vec<String>>,
}

impl ContentMeta {
    pub fn local() -> Self {
        ContentMeta {
            source: Source::Local,
            project_id: None,
            version_id: None,
            title: None,
            version_name: None,
            icon_url: None,
            sha1: None,
            auto: false,
            requires: None,
        }
    }
}

/// An installed file as the Content tab shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledContent {
    /// Path relative to the instance folder, without `.disabled`.
    pub path: String,
    pub file_name: String,
    pub kind: ContentKind,
    pub enabled: bool,
    pub size: u64,
    pub meta: Option<ContentMeta>,
}

fn index_path(launcher: &Launcher, instance: &Instance) -> std::path::PathBuf {
    instance.dir(launcher).join(".launcher").join("content.json")
}

pub async fn load_index(launcher: &Launcher, instance: &Instance) -> ContentIndex {
    tokio::fs::read(index_path(launcher, instance)).await.ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub async fn save_index(launcher: &Launcher, instance: &Instance, index: &ContentIndex) -> Result<()> {
    let path = index_path(launcher, instance);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    write_json_atomic(&path, index).await
}

pub async fn record(launcher: &Launcher, instance: &Instance, rel_path: String, meta: ContentMeta) -> Result<()> {
    let mut index = load_index(launcher, instance).await;
    index.files.insert(rel_path, meta);
    save_index(launcher, instance, &index).await
}

/// Everything in mods/, resourcepacks/ and shaderpacks/, enabled or not.
pub async fn list_installed(launcher: &Launcher, instance: &Instance) -> Result<Vec<InstalledContent>> {
    let index = load_index(launcher, instance).await;
    let dir = instance.dir(launcher);
    let mut out = vec![];
    for kind in [ContentKind::Mod, ContentKind::ResourcePack, ContentKind::Shader] {
        let folder = dir.join(kind.folder());
        let Ok(mut rd) = tokio::fs::read_dir(&folder).await else {
            continue;
        };
        while let Some(e) = rd.next_entry().await? {
            let meta = e.metadata().await?;
            if !meta.is_file() {
                continue;
            }
            let raw = e.file_name().to_string_lossy().to_string();
            let enabled = !raw.ends_with(".disabled");
            let file_name = raw.trim_end_matches(".disabled").to_string();
            if !(file_name.ends_with(".jar") || file_name.ends_with(".zip")) {
                continue;
            }
            let path = format!("{}/{file_name}", kind.folder());
            out.push(InstalledContent { meta: index.files.get(&path).cloned(), path, file_name, kind, enabled, size: meta.len() });
        }
    }
    out.sort_by(|a, b| {
        let name =
            |c: &InstalledContent| c.meta.as_ref().and_then(|m| m.title.clone()).unwrap_or_else(|| c.file_name.clone()).to_lowercase();
        name(a).cmp(&name(b))
    });
    Ok(out)
}

/// Enables or disables a file by toggling its `.disabled` suffix.
pub async fn set_enabled(launcher: &Launcher, instance: &Instance, rel_path: &str, enabled: bool) -> Result<()> {
    let base = crate::util::safe_join(&instance.dir(launcher), rel_path)?;
    let disabled = append_ext(&base, "disabled");
    let (from, to) = if enabled { (disabled, base) } else { (base.clone(), disabled) };
    if from.exists() {
        tokio::fs::rename(from, to).await?;
    } else if !to.exists() {
        return Err(Error::NotFound(rel_path.into()));
    }
    Ok(())
}

pub async fn remove(launcher: &Launcher, instance: &Instance, rel_path: &str) -> Result<()> {
    let base = crate::util::safe_join(&instance.dir(launcher), rel_path)?;
    for p in [base.clone(), append_ext(&base, "disabled")] {
        if p.exists() {
            tokio::fs::remove_file(p).await?;
        }
    }
    let mut index = load_index(launcher, instance).await;
    index.files.remove(rel_path);
    save_index(launcher, instance, &index).await
}

fn append_ext(path: &Path, ext: &str) -> std::path::PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".");
    s.push(ext);
    s.into()
}
