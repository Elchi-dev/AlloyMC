//! Modrinth API v2 (no key needed for reads). <https://docs.modrinth.com>

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{record, ContentHit, ContentKind, ContentMeta, SearchPage, SearchQuery, Source};
use crate::download::{download_with_retry, DownloadTask};
use crate::http::get_json;
use crate::instance::Instance;
use crate::meta::loaders::Loader;
use crate::{Error, Launcher, Result};

const API: &str = "https://api.modrinth.com/v2";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub downloads: u64,
    #[serde(default)]
    pub follows: u64,
    #[serde(default)]
    pub date_modified: Option<String>,
    #[serde(default)]
    pub display_categories: Vec<String>,
    #[serde(default)]
    pub project_type: String,
    #[serde(default)]
    pub gallery: Vec<String>,
    #[serde(default)]
    pub color: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    hits: Vec<SearchHit>,
    offset: u64,
    total_hits: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub body: String,
    pub project_type: String,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub downloads: u64,
    #[serde(default)]
    pub followers: u64,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub color: Option<u32>,
    #[serde(default)]
    pub gallery: Vec<GalleryImage>,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub loaders: Vec<String>,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub issues_url: Option<String>,
    #[serde(default)]
    pub wiki_url: Option<String>,
    #[serde(default)]
    pub discord_url: Option<String>,
    #[serde(default)]
    pub updated: Option<String>,
    #[serde(default)]
    pub client_side: Option<String>,
    #[serde(default)]
    pub server_side: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GalleryImage {
    pub url: String,
    #[serde(default)]
    pub featured: bool,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Version {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub version_number: String,
    #[serde(default)]
    pub changelog: Option<String>,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub loaders: Vec<String>,
    #[serde(default)]
    pub version_type: String,
    #[serde(default)]
    pub date_published: String,
    #[serde(default)]
    pub downloads: u64,
    pub files: Vec<VersionFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    #[serde(default)]
    pub version_id: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub file_name: Option<String>,
    /// required | optional | incompatible | embedded
    pub dependency_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionFile {
    pub hashes: HashMap<String, String>,
    pub url: String,
    pub filename: String,
    #[serde(default)]
    pub primary: bool,
    #[serde(default)]
    pub size: u64,
}

impl Version {
    /// Project ids of the required dependencies.
    pub fn required_projects(&self) -> Vec<String> {
        self.dependencies.iter().filter(|d| d.dependency_type == "required").filter_map(|d| d.project_id.clone()).collect()
    }

    pub fn primary_file(&self) -> Option<&VersionFile> {
        self.files.iter().find(|f| f.primary).or(self.files.first())
    }
}

impl From<SearchHit> for ContentHit {
    fn from(h: SearchHit) -> Self {
        ContentHit {
            source: Source::Modrinth,
            kind: ContentKind::from_modrinth(&h.project_type),
            id: h.project_id,
            slug: h.slug,
            title: h.title,
            description: h.description,
            author: h.author,
            icon_url: h.icon_url,
            downloads: h.downloads,
            follows: h.follows,
            updated: h.date_modified,
            categories: h.display_categories,
            gallery: h.gallery,
            color: h.color,
        }
    }
}

fn kind_facet(kind: ContentKind) -> &'static str {
    match kind {
        ContentKind::Mod => "mod",
        ContentKind::ResourcePack => "resourcepack",
        ContentKind::Shader => "shader",
        ContentKind::DataPack => "datapack",
        ContentKind::Modpack => "modpack",
    }
}

pub async fn search(launcher: &Launcher, q: &SearchQuery) -> Result<SearchPage> {
    let kind = q.kind.unwrap_or(ContentKind::Mod);
    let mut facets: Vec<Vec<String>> = vec![vec![format!("project_type:{}", kind_facet(kind))]];
    if let Some(v) = &q.game_version {
        facets.push(vec![format!("versions:{v}")]);
    }
    if let (Some(loader), ContentKind::Mod | ContentKind::Modpack) = (&q.loader, kind) {
        facets.push(vec![format!("categories:{loader}")]);
    }
    for c in &q.categories {
        facets.push(vec![format!("categories:{c}")]);
    }
    let limit = if q.limit == 0 { 20 } else { q.limit.min(100) };
    let index = q.sort.clone().unwrap_or_else(|| "relevance".into());

    let resp: SearchResponse = launcher
        .http
        .get(format!("{API}/search"))
        .query(&[
            ("query", q.query.as_str()),
            ("facets", &serde_json::to_string(&facets)?),
            ("index", &index),
            ("offset", &q.offset.to_string()),
            ("limit", &limit.to_string()),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(SearchPage { hits: resp.hits.into_iter().map(Into::into).collect(), total: resp.total_hits, offset: resp.offset })
}

pub async fn project(launcher: &Launcher, id_or_slug: &str) -> Result<Project> {
    get_json(&launcher.http, &format!("{API}/project/{id_or_slug}")).await
}

/// Several projects in one request (ids or slugs).
pub async fn projects(launcher: &Launcher, ids: &[String]) -> Result<Vec<Project>> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let mut out = vec![];
    // Long id lists make long URLs; 100 per request stays well under any limit.
    for chunk in ids.chunks(100) {
        let page: Vec<Project> = launcher
            .http
            .get(format!("{API}/projects"))
            .query(&[("ids", serde_json::to_string(chunk)?)])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        out.extend(page);
    }
    Ok(out)
}

/// Several versions in one request.
pub async fn versions(launcher: &Launcher, ids: &[String]) -> Result<Vec<Version>> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let mut out = vec![];
    for chunk in ids.chunks(100) {
        let page: Vec<Version> = launcher
            .http
            .get(format!("{API}/versions"))
            .query(&[("ids", serde_json::to_string(chunk)?)])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        out.extend(page);
    }
    Ok(out)
}

pub async fn version(launcher: &Launcher, id: &str) -> Result<Version> {
    get_json(&launcher.http, &format!("{API}/version/{id}")).await
}

/// Loaders an instance can use content for (Quilt runs Fabric mods).
pub fn compatible_loaders(loader: Loader) -> Vec<&'static str> {
    match loader {
        Loader::Vanilla => vec![],
        Loader::Quilt => vec!["quilt", "fabric"],
        other => other.modrinth_name().into_iter().collect(),
    }
}

/// Versions of a project, newest first, optionally filtered to an instance's game version + loader.
pub async fn project_versions(launcher: &Launcher, project_id: &str, loaders: &[&str], game_versions: &[&str]) -> Result<Vec<Version>> {
    let mut req = launcher.http.get(format!("{API}/project/{project_id}/version"));
    if !loaders.is_empty() {
        req = req.query(&[("loaders", serde_json::to_string(loaders)?)]);
    }
    if !game_versions.is_empty() {
        req = req.query(&[("game_versions", serde_json::to_string(game_versions)?)]);
    }
    Ok(req.send().await?.error_for_status()?.json().await?)
}

/// Looks up installed files by SHA-1. Returns hash -> version for files Modrinth knows.
pub async fn versions_from_hashes(launcher: &Launcher, sha1s: &[String]) -> Result<HashMap<String, Version>> {
    if sha1s.is_empty() {
        return Ok(HashMap::new());
    }
    Ok(launcher
        .http
        .post(format!("{API}/version_files"))
        .json(&json!({ "hashes": sha1s, "algorithm": "sha1" }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

/// Newest compatible version for each hash; used for "Update all".
pub async fn updates_from_hashes(
    launcher: &Launcher,
    sha1s: &[String],
    loaders: &[&str],
    game_versions: &[&str],
) -> Result<HashMap<String, Version>> {
    if sha1s.is_empty() {
        return Ok(HashMap::new());
    }
    Ok(launcher
        .http
        .post(format!("{API}/version_files/update"))
        .json(&json!({
            "hashes": sha1s, "algorithm": "sha1",
            "loaders": loaders, "game_versions": game_versions,
        }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

/// Installs a version into an instance, pulling in required dependencies that aren't there yet.
/// Returns the titles of everything installed.
pub async fn install_version(launcher: &Launcher, instance: &Instance, version_id: &str) -> Result<Vec<String>> {
    let loaders = compatible_loaders(instance.loader);
    let games = [instance.game_version.as_str()];
    let index = super::load_index(launcher, instance).await;
    let mut have: HashSet<String> = index.files.values().filter_map(|m| m.project_id.clone()).collect();

    let mut queue = vec![version(launcher, version_id).await?];
    let mut installed = vec![];
    while let Some(v) = queue.pop() {
        if !have.insert(v.project_id.clone()) && !installed.is_empty() {
            continue;
        }
        for dep in v.dependencies.iter().filter(|d| d.dependency_type == "required") {
            let dep_version = match (&dep.version_id, &dep.project_id) {
                (Some(vid), _) => Some(version(launcher, vid).await?),
                (None, Some(pid)) if !have.contains(pid) => project_versions(launcher, pid, &loaders, &games).await?.into_iter().next(),
                _ => None,
            };
            if let Some(dv) = dep_version {
                if !have.contains(&dv.project_id) {
                    queue.push(dv);
                }
            }
        }
        let project = project(launcher, &v.project_id).await?;
        install_file(launcher, instance, &project, &v).await?;
        installed.push(project.title);
    }
    Ok(installed)
}

async fn install_file(launcher: &Launcher, instance: &Instance, project: &Project, v: &Version) -> Result<()> {
    let file = v.primary_file().ok_or_else(|| Error::NotFound(format!("file for {}", v.name)))?;
    let kind = ContentKind::from_modrinth(&project.project_type);
    if kind == ContentKind::Modpack {
        return Err(Error::other("Modpacks install as a new instance, not into one"));
    }
    let rel = format!("{}/{}", kind.folder(), file.filename);
    let dest = crate::util::safe_join(&instance.dir(launcher), &rel)?;

    // Replace an older version of the same project.
    let index = super::load_index(launcher, instance).await;
    for (old_path, meta) in &index.files {
        if meta.project_id.as_deref() == Some(&project.id) && *old_path != rel {
            let _ = super::remove(launcher, instance, old_path).await;
        }
    }

    let sha1 = file.hashes.get("sha1").cloned();
    download_with_retry(&launcher.http, &DownloadTask::new(&file.url, &dest).sha1(sha1.clone()).size(Some(file.size))).await?;
    record(
        launcher,
        instance,
        rel,
        ContentMeta {
            source: Source::Modrinth,
            project_id: Some(project.id.clone()),
            version_id: Some(v.id.clone()),
            title: Some(project.title.clone()),
            version_name: Some(v.version_number.clone()),
            icon_url: project.icon_url.clone(),
            sha1,
            auto: false,
            requires: Some(v.required_projects()),
        },
    )
    .await
}
