//! Modpack import: Modrinth `.mrpack` and CurseForge `.zip`, from a file or a URL.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use crate::content::manage::BlockedFile;
use crate::content::{curseforge, ContentMeta, Source};
use crate::download::{download_all, download_with_progress, DownloadTask};
use crate::instance::{self, Instance, LinkedPack, NewInstance, PackSource};
use crate::meta::loaders::Loader;
use crate::progress::{emit, ProgressFn, Stage, Unit};
use crate::{Error, Launcher, Result};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub instance: Instance,
    /// CurseForge files the author won't let launchers download.
    pub blocked: Vec<BlockedFile>,
}

// ---------- .mrpack ----------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MrpackIndex {
    pub format_version: u32,
    pub game: String,
    pub version_id: String,
    pub name: String,
    #[serde(default)]
    pub summary: Option<String>,
    pub files: Vec<MrpackFile>,
    pub dependencies: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MrpackFile {
    pub path: String,
    pub hashes: HashMap<String, String>,
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,
    pub downloads: Vec<String>,
    #[serde(default)]
    pub file_size: u64,
}

impl MrpackIndex {
    pub fn game_and_loader(&self) -> Result<(String, Loader, Option<String>)> {
        let game = self
            .dependencies
            .get("minecraft")
            .cloned()
            .ok_or_else(|| Error::other("Modpack doesn't say which Minecraft version it needs"))?;
        for (key, loader) in
            [("fabric-loader", Loader::Fabric), ("quilt-loader", Loader::Quilt), ("neoforge", Loader::NeoForge), ("forge", Loader::Forge)]
        {
            if let Some(v) = self.dependencies.get(key) {
                return Ok((game, loader, Some(v.clone())));
            }
        }
        Ok((game, Loader::Vanilla, None))
    }
}

// ---------- CurseForge zip ----------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfManifest {
    minecraft: CfMinecraft,
    name: String,
    #[serde(default)]
    version: Option<String>,
    files: Vec<CfManifestFile>,
    #[serde(default = "default_overrides")]
    overrides: String,
}
fn default_overrides() -> String {
    "overrides".into()
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfMinecraft {
    version: String,
    #[serde(default)]
    mod_loaders: Vec<CfModLoader>,
}
#[derive(Debug, Deserialize)]
struct CfModLoader {
    id: String,
    #[serde(default)]
    primary: bool,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfManifestFile {
    #[serde(rename = "projectID")]
    #[allow(dead_code)]
    project_id: u64,
    #[serde(rename = "fileID")]
    file_id: u64,
    #[serde(default = "yes")]
    required: bool,
}
fn yes() -> bool {
    true
}

/// "forge-47.2.0" -> (Forge, "47.2.0")
fn parse_cf_loader(id: &str) -> (Loader, Option<String>) {
    let (name, version) = id.split_once('-').unwrap_or((id, ""));
    let loader = Loader::from_modrinth(name).unwrap_or(Loader::Vanilla);
    (loader, Some(version.to_string()).filter(|v| !v.is_empty()))
}

// ---------- entry points ----------

/// Downloads a pack from a URL into `meta/imports/` and imports it.
pub async fn import_url(launcher: &Launcher, url: &str, progress: &ProgressFn) -> Result<ImportResult> {
    let parsed = url::Url::parse(url).map_err(|_| Error::other("That doesn't look like a URL"))?;
    let name = parsed
        .path_segments()
        .and_then(|mut s| s.next_back().map(str::to_string))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "pack.zip".into());
    // `COBBLEVERSE%201.7.mrpack` is a file name with a space in it.
    let name = percent_encoding::percent_decode_str(&name).decode_utf8_lossy().replace(['/', '\\'], "_");
    let dest = launcher.dirs.meta().join("imports").join(&name);
    // Packs with big overrides run to hundreds of MB, so this download reports every byte.
    let message = Some("Downloading the pack".to_string());
    emit(progress, Stage::Metadata, 0, 0, Unit::Bytes, message.clone());
    let got = std::sync::atomic::AtomicI64::new(0);
    let on_bytes = |delta: i64, len: Option<u64>| {
        let now = got.fetch_add(delta, std::sync::atomic::Ordering::Relaxed) + delta;
        emit(progress, Stage::Metadata, now.max(0) as u64, len.unwrap_or(0), Unit::Bytes, message.clone());
    };
    download_with_progress(&launcher.http, &DownloadTask::new(url, &dest), &on_bytes).await?;
    let mut result = import_file(launcher, &dest, progress).await?;
    if let Some(link) = result.instance.linked_pack.as_mut() {
        link.source = PackSource::Url;
    }
    result.instance.save(launcher).await?;
    Ok(result)
}

/// Detects the format from the archive's contents, not its extension.
pub async fn import_file(launcher: &Launcher, path: &Path, progress: &ProgressFn) -> Result<ImportResult> {
    let p = path.to_path_buf();
    let (mrpack, cf) = tokio::task::spawn_blocking(move || -> Result<(Option<String>, Option<String>)> {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&p)?)?;
        Ok((read_entry(&mut zip, "modrinth.index.json"), read_entry(&mut zip, "manifest.json")))
    })
    .await
    .map_err(|e| Error::other(e.to_string()))??;

    match (mrpack, cf) {
        (Some(index), _) => import_mrpack(launcher, path, serde_json::from_str(&index)?, progress).await,
        (None, Some(manifest)) => import_cf(launcher, path, serde_json::from_str(&manifest)?, progress).await,
        _ => Err(Error::other("Not a modpack: no modrinth.index.json or manifest.json inside")),
    }
}

fn read_entry(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<String> {
    let mut f = zip.by_name(name).ok()?;
    let mut s = String::new();
    f.read_to_string(&mut s).ok()?;
    Some(s)
}

async fn import_mrpack(launcher: &Launcher, path: &Path, index: MrpackIndex, progress: &ProgressFn) -> Result<ImportResult> {
    if index.game != "minecraft" {
        return Err(Error::Unsupported(format!("modpacks for {}", index.game)));
    }
    let (game, loader, loader_version) = index.game_and_loader()?;
    let mut inst = instance::create(
        launcher,
        NewInstance { name: index.name.clone(), game_version: game, loader, loader_version, icon: None, group: None, fabric_api: None },
    )
    .await?;
    inst.linked_pack = Some(LinkedPack {
        source: PackSource::File,
        project_id: None,
        version_id: None,
        name: Some(index.name.clone()),
        version_name: Some(index.version_id.clone()),
    });
    inst.save(launcher).await?;

    let dir = inst.dir(launcher);
    let mut tasks = vec![];
    let mut metas = vec![];
    for f in &index.files {
        let client = f.env.as_ref().and_then(|e| e.get("client")).map(String::as_str);
        if client == Some("unsupported") {
            continue;
        }
        let url = f.downloads.first().ok_or_else(|| Error::other(format!("No download for {}", f.path)))?;
        let dest = crate::util::safe_join(&dir, &f.path)?;
        let sha1 = f.hashes.get("sha1").cloned();
        tasks.push(DownloadTask::new(url, dest).sha1(sha1.clone()).size(Some(f.file_size)));
        metas.push((f.path.clone(), sha1));
    }
    let concurrency = launcher.settings.read().await.max_concurrent_downloads;
    download_all(&launcher.http, tasks, concurrency, Stage::Content, progress).await?;

    emit(progress, Stage::Extracting, 0, 1, Unit::Steps, Some("Copying overrides".into()));
    extract_prefix(path, "overrides/", &dir).await?;
    extract_prefix(path, "client-overrides/", &dir).await?;

    // Name the files we just installed (best effort; offline import still works).
    let sha1s: Vec<String> = metas.iter().filter_map(|(_, h)| h.clone()).collect();
    let known = crate::content::modrinth::versions_from_hashes(launcher, &sha1s).await.unwrap_or_default();
    let project_ids: Vec<String> =
        known.values().map(|v| v.project_id.clone()).collect::<std::collections::HashSet<_>>().into_iter().collect();
    let projects: HashMap<String, crate::content::modrinth::Project> = crate::content::modrinth::projects(launcher, &project_ids)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|p| (p.id.clone(), p))
        .collect();
    let mut index = crate::content::load_index(launcher, &inst).await;
    for (rel, sha1) in metas {
        let v = sha1.as_ref().and_then(|h| known.get(h));
        let project = v.and_then(|v| projects.get(&v.project_id));
        index.files.insert(
            rel,
            ContentMeta {
                source: if v.is_some() { Source::Modrinth } else { Source::Local },
                project_id: v.map(|v| v.project_id.clone()),
                version_id: v.map(|v| v.id.clone()),
                title: project.map(|p| p.title.clone()),
                version_name: v.map(|v| v.version_number.clone()),
                icon_url: project.and_then(|p| p.icon_url.clone()),
                sha1,
                auto: false,
                requires: v.map(|v| v.required_projects()),
            },
        );
    }
    crate::content::save_index(launcher, &inst, &index).await?;
    // Files the pack lists but couldn't include (CurseForge-only); fetch what we can.
    let listed = crate::content::listed::fetch(launcher, &inst).await.unwrap_or_default();
    Ok(ImportResult { instance: inst, blocked: listed.blocked })
}

async fn import_cf(launcher: &Launcher, path: &Path, manifest: CfManifest, progress: &ProgressFn) -> Result<ImportResult> {
    let primary = manifest.minecraft.mod_loaders.iter().find(|l| l.primary).or(manifest.minecraft.mod_loaders.first());
    let (loader, loader_version) = primary.map(|l| parse_cf_loader(&l.id)).unwrap_or((Loader::Vanilla, None));

    let mut inst = instance::create(
        launcher,
        NewInstance {
            name: manifest.name.clone(),
            game_version: manifest.minecraft.version.clone(),
            loader,
            loader_version,
            icon: None,
            group: None,
            fabric_api: None,
        },
    )
    .await?;
    inst.linked_pack = Some(LinkedPack {
        source: PackSource::CurseForge,
        project_id: None,
        version_id: None,
        name: Some(manifest.name.clone()),
        version_name: manifest.version.clone(),
    });
    inst.save(launcher).await?;
    let dir = inst.dir(launcher);

    let ids: Vec<u64> = manifest.files.iter().filter(|f| f.required).map(|f| f.file_id).collect();
    let files = curseforge::files(launcher, &ids).await?;
    // One batch for every project: names, icons, and which folder each file belongs in
    // (CurseForge packs list resource packs and shaders alongside mods).
    let mod_ids: Vec<u64> = files.iter().map(|f| f.mod_id).collect::<std::collections::HashSet<_>>().into_iter().collect();
    let mods: HashMap<u64, curseforge::CfMod> =
        curseforge::mods(launcher, &mod_ids).await.unwrap_or_default().into_iter().map(|m| (m.id, m)).collect();
    let mut blocked = vec![];
    let mut tasks = vec![];
    let mut metas = vec![];
    for f in files {
        let m = mods.get(&f.mod_id);
        let folder = match m.and_then(|m| m.class_id) {
            Some(12) => "resourcepacks",
            Some(6552) => "shaderpacks",
            Some(6945) => "datapacks",
            _ => "mods",
        };
        let rel = format!("{folder}/{}", f.file_name);
        let sha1 = f.hashes.iter().find(|h| h.algo == 1).map(|h| h.value.clone());
        let icon = m.and_then(|m| m.logo.as_ref().map(|l| l.thumbnail_url.clone().unwrap_or(l.url.clone())));
        match &f.download_url {
            Some(url) => {
                tasks.push(
                    DownloadTask::new(url, crate::util::safe_join(&dir, &rel)?)
                        .sha1(sha1.clone())
                        .size(Some(f.file_length).filter(|s| *s > 0)),
                );
                metas.push((
                    rel,
                    ContentMeta {
                        source: Source::CurseForge,
                        project_id: Some(f.mod_id.to_string()),
                        version_id: Some(f.id.to_string()),
                        title: m.map(|m| m.name.clone()),
                        version_name: Some(f.display_name.clone()),
                        icon_url: icon,
                        sha1,
                        auto: false,
                        requires: Some(f.required_mods()),
                    },
                ));
            }
            None => blocked.push(BlockedFile {
                title: m.map(|m| m.name.clone()).unwrap_or_else(|| f.display_name.clone()),
                file_name: f.file_name.clone(),
                page_url: m
                    .map(|m| curseforge::manual_download_page(m, f.id))
                    .unwrap_or_else(|| format!("https://www.curseforge.com/projects/{}", f.mod_id)),
                sha1,
                size: f.file_length,
                mod_id: f.mod_id,
                file_id: f.id,
                folder: folder.into(),
                icon_url: icon,
            }),
        }
    }
    let concurrency = launcher.settings.read().await.max_concurrent_downloads;
    download_all(&launcher.http, tasks, concurrency, Stage::Content, progress).await?;
    extract_prefix(path, &format!("{}/", manifest.overrides.trim_end_matches('/')), &dir).await?;

    let mut index = crate::content::load_index(launcher, &inst).await;
    index.files.extend(metas);
    crate::content::save_index(launcher, &inst, &index).await?;
    crate::content::blocked::add(launcher, &inst, &blocked).await?;
    Ok(ImportResult { instance: inst, blocked })
}

/// Extracts every zip entry under `prefix` into `dest`, keeping the rest of the path.
async fn extract_prefix(zip_path: &Path, prefix: &str, dest: &Path) -> Result<()> {
    let zip_path = zip_path.to_path_buf();
    let prefix = prefix.to_string();
    let dest: PathBuf = dest.to_path_buf();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&zip_path)?)?;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i)?;
            let name = entry.name().replace('\\', "/");
            let Some(rel) = name.strip_prefix(&prefix) else {
                continue;
            };
            if rel.is_empty() || entry.is_dir() {
                continue;
            }
            let out = crate::util::safe_join(&dest, rel)?;
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut file = std::fs::File::create(&out)?;
            std::io::copy(&mut entry, &mut file)?;
        }
        Ok(())
    })
    .await
    .map_err(|e| Error::other(e.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_mrpack_dependencies() {
        let index: MrpackIndex = serde_json::from_str(
            r#"{"formatVersion":1,"game":"minecraft","versionId":"1.0","name":"P","files":[],
               "dependencies":{"minecraft":"26.2","fabric-loader":"0.20.1"}}"#,
        )
        .unwrap();
        let (g, l, v) = index.game_and_loader().unwrap();
        assert_eq!((g.as_str(), l, v.as_deref()), ("26.2", Loader::Fabric, Some("0.20.1")));
    }

    #[test]
    fn cf_loader_ids() {
        assert_eq!(parse_cf_loader("forge-47.2.0"), (Loader::Forge, Some("47.2.0".into())));
        assert_eq!(parse_cf_loader("neoforge-21.1.77"), (Loader::NeoForge, Some("21.1.77".into())));
        assert_eq!(parse_cf_loader("fabric-0.16.5"), (Loader::Fabric, Some("0.16.5".into())));
    }
}
