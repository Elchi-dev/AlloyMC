//! CurseForge Core API. Every call needs your own key from console.curseforge.com
//! (Settings → Accounts & APIs).

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{record, ContentHit, ContentKind, ContentMeta, SearchPage, SearchQuery, Source};
use crate::download::{download_with_retry, DownloadTask};
use crate::instance::Instance;
use crate::meta::loaders::Loader;
use crate::{Error, Launcher, Result};

const API: &str = "https://api.curseforge.com/v1";
pub const MINECRAFT_GAME_ID: u32 = 432;

pub fn class_id(kind: ContentKind) -> u32 {
    match kind {
        ContentKind::Mod => 6,
        ContentKind::Modpack => 4471,
        ContentKind::ResourcePack => 12,
        ContentKind::Shader => 6552,
        ContentKind::DataPack => 6945,
    }
}

fn kind_from_class(class_id: Option<u32>) -> ContentKind {
    match class_id {
        Some(4471) => ContentKind::Modpack,
        Some(12) => ContentKind::ResourcePack,
        Some(6552) => ContentKind::Shader,
        Some(6945) => ContentKind::DataPack,
        _ => ContentKind::Mod,
    }
}

pub fn loader_type(loader: Loader) -> Option<u32> {
    match loader {
        Loader::Forge => Some(1),
        Loader::Fabric => Some(4),
        Loader::Quilt => Some(5),
        Loader::NeoForge => Some(6),
        Loader::Vanilla => None,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfMod {
    pub id: u64,
    pub name: String,
    pub slug: String,
    pub summary: String,
    #[serde(default)]
    pub download_count: f64,
    #[serde(default)]
    pub thumbs_up_count: u64,
    #[serde(default)]
    pub class_id: Option<u32>,
    #[serde(default)]
    pub logo: Option<CfAsset>,
    #[serde(default)]
    pub screenshots: Vec<CfAsset>,
    #[serde(default)]
    pub authors: Vec<CfAuthor>,
    #[serde(default)]
    pub categories: Vec<CfCategory>,
    #[serde(default)]
    pub date_modified: Option<String>,
    #[serde(default)]
    pub links: Option<CfLinks>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfAsset {
    pub url: String,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CfAuthor {
    pub name: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CfCategory {
    pub name: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfLinks {
    #[serde(default)]
    pub website_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfFile {
    pub id: u64,
    pub mod_id: u64,
    pub display_name: String,
    pub file_name: String,
    /// Null when the author opted out of third-party downloads.
    #[serde(default)]
    pub download_url: Option<String>,
    #[serde(default)]
    pub file_length: u64,
    #[serde(default)]
    pub hashes: Vec<CfHash>,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<CfDependency>,
    #[serde(default)]
    pub file_date: Option<String>,
    #[serde(default)]
    pub release_type: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CfHash {
    pub value: String,
    /// 1 = sha1, 2 = md5
    pub algo: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfDependency {
    pub mod_id: u64,
    /// 3 = required
    pub relation_type: u8,
}

#[derive(Deserialize)]
struct Data<T> {
    data: T,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Paged<T> {
    data: T,
    pagination: Pagination,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pagination {
    index: u64,
    total_count: u64,
}

/// The key compiled into private builds (ALLOY_CURSEFORGE_KEY). Public builds have none.
pub fn builtin_key() -> Option<&'static str> {
    option_env!("ALLOY_CURSEFORGE_KEY").filter(|k| !k.trim().is_empty())
}

/// The key from Settings, else the one built into this build.
async fn key(launcher: &Launcher) -> Result<String> {
    launcher
        .settings
        .read()
        .await
        .curseforge_api_key
        .clone()
        .filter(|k| !k.trim().is_empty())
        .or_else(|| builtin_key().map(str::to_string))
        .ok_or_else(|| Error::other("CurseForge support is a work in progress"))
}

impl From<CfMod> for ContentHit {
    fn from(m: CfMod) -> Self {
        ContentHit {
            source: Source::CurseForge,
            kind: kind_from_class(m.class_id),
            id: m.id.to_string(),
            slug: m.slug,
            title: m.name,
            description: m.summary,
            author: m.authors.first().map(|a| a.name.clone()).unwrap_or_default(),
            icon_url: m.logo.map(|l| l.thumbnail_url.unwrap_or(l.url)),
            downloads: m.download_count as u64,
            follows: m.thumbs_up_count,
            updated: m.date_modified,
            categories: m.categories.into_iter().map(|c| c.name).collect(),
            gallery: m.screenshots.into_iter().map(|s| s.url).collect(),
            color: None,
        }
    }
}

pub async fn search(launcher: &Launcher, q: &SearchQuery) -> Result<SearchPage> {
    let key = key(launcher).await?;
    let kind = q.kind.unwrap_or(ContentKind::Mod);
    let sort = match q.sort.as_deref() {
        Some("downloads") => "6",
        Some("updated") | Some("newest") => "3",
        Some("follows") => "2",
        _ => "2", // popularity
    };
    let limit = if q.limit == 0 { 20 } else { q.limit.min(50) };
    let mut params: Vec<(&str, String)> = vec![
        ("gameId", MINECRAFT_GAME_ID.to_string()),
        ("classId", class_id(kind).to_string()),
        ("searchFilter", q.query.clone()),
        ("sortField", sort.into()),
        ("sortOrder", "desc".into()),
        ("index", q.offset.to_string()),
        ("pageSize", limit.to_string()),
    ];
    if let Some(v) = &q.game_version {
        params.push(("gameVersion", v.clone()));
    }
    if let Some(lt) = q.loader.as_deref().and_then(Loader::from_modrinth).and_then(loader_type) {
        if matches!(kind, ContentKind::Mod | ContentKind::Modpack) {
            params.push(("modLoaderType", lt.to_string()));
        }
    }
    let resp: Paged<Vec<CfMod>> = launcher
        .http
        .get(format!("{API}/mods/search"))
        .header("x-api-key", key)
        .query(&params)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(SearchPage {
        hits: resp.data.into_iter().map(Into::into).collect(),
        total: resp.pagination.total_count,
        offset: resp.pagination.index,
    })
}

pub async fn get_mod(launcher: &Launcher, id: u64) -> Result<CfMod> {
    let key = key(launcher).await?;
    let r: Data<CfMod> = launcher
        .http
        .get(format!("{API}/mods/{id}"))
        .header("x-api-key", key)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(r.data)
}

pub async fn mod_files(launcher: &Launcher, id: u64, game_version: Option<&str>, loader: Option<Loader>) -> Result<Vec<CfFile>> {
    let key = key(launcher).await?;
    let mut req = launcher
        .http
        .get(format!("{API}/mods/{id}/files"))
        .header("x-api-key", key)
        .query(&[("pageSize", "50")]);
    if let Some(v) = game_version {
        req = req.query(&[("gameVersion", v)]);
    }
    if let Some(lt) = loader.and_then(loader_type) {
        req = req.query(&[("modLoaderType", lt.to_string())]);
    }
    let r: Data<Vec<CfFile>> = req.send().await?.error_for_status()?.json().await?;
    Ok(r.data)
}

/// Batch file lookup, used for CurseForge modpack manifests.
pub async fn files(launcher: &Launcher, file_ids: &[u64]) -> Result<Vec<CfFile>> {
    let key = key(launcher).await?;
    let r: Data<Vec<CfFile>> = launcher
        .http
        .post(format!("{API}/mods/files"))
        .header("x-api-key", key)
        .json(&json!({ "fileIds": file_ids }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(r.data)
}

/// Web page to send people to when an author blocks API downloads.
pub fn manual_download_page(m: &CfMod, file_id: u64) -> String {
    let base = m
        .links
        .as_ref()
        .and_then(|l| l.website_url.clone())
        .unwrap_or_else(|| format!("https://www.curseforge.com/minecraft/mc-mods/{}", m.slug));
    format!("{base}/download/{file_id}")
}

/// Installs one file into an instance (no dependency resolution yet; Phase 2).
pub async fn install_file(launcher: &Launcher, instance: &Instance, m: &CfMod, file: &CfFile) -> Result<()> {
    let url = file.download_url.clone().ok_or_else(|| {
        Error::other(format!(
            "{} blocks downloads from other launchers. Download it here and drop it on the instance: {}",
            m.name,
            manual_download_page(m, file.id)
        ))
    })?;
    let kind = kind_from_class(m.class_id);
    let rel = format!("{}/{}", kind.folder(), file.file_name);
    let dest = crate::util::safe_join(&instance.dir(launcher), &rel)?;
    let sha1 = file.hashes.iter().find(|h| h.algo == 1).map(|h| h.value.clone());
    download_with_retry(&launcher.http, &DownloadTask::new(url, &dest).sha1(sha1.clone())).await?;
    record(
        launcher,
        instance,
        rel,
        ContentMeta {
            source: Source::CurseForge,
            project_id: Some(m.id.to_string()),
            version_id: Some(file.id.to_string()),
            title: Some(m.name.clone()),
            version_name: Some(file.display_name.clone()),
            icon_url: m.logo.as_ref().map(|l| l.url.clone()),
            sha1,
        },
    )
    .await
}
