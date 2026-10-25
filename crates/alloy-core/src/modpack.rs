//! Modpack import: Modrinth `.mrpack` and CurseForge `.zip`, from a file or a URL.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::content::{curseforge, record, ContentMeta, Source};
use crate::download::{download_all, download_with_retry, DownloadTask};
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

#[derive(Debug, Clone, Serialize)]
pub struct BlockedFile {
    pub name: String,
    pub url: String,
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
        for (key, loader) in [
            ("fabric-loader", Loader::Fabric),
            ("quilt-loader", Loader::Quilt),
            ("neoforge", Loader::NeoForge),
            ("forge", Loader::Forge),
        ] {
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
    #[allow(dead_code)] // used by Phase 2 blocked-file matching
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
    let dest = launcher.dirs.meta().join("imports").join(&name);
    emit(progress, Stage::Metadata, 0, 1, Unit::Steps, Some(format!("Downloading {name}")));
    download_with_retry(&launcher.http, &DownloadTask::new(url, &dest)).await?;
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
        NewInstance {
            name: index.name.clone(),
            game_version: game,
            loader,
            loader_version,
            icon: None,
            group: None,
        },
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
    let known = crate::content::modrinth::versions_from_hashes(launcher, &sha1s)
        .await
        .unwrap_or_default();
    for (rel, sha1) in metas {
        let v = sha1.as_ref().and_then(|h| known.get(h));
        record(
            launcher,
            &inst,
            rel,
            ContentMeta {
                source: if v.is_some() { Source::Modrinth } else { Source::Local },
                project_id: v.map(|v| v.project_id.clone()),
                version_id: v.map(|v| v.id.clone()),
                title: None,
                version_name: v.map(|v| v.version_number.clone()),
                icon_url: None,
                sha1,
            },
        )
        .await?;
    }
    Ok(ImportResult {
        instance: inst,
        blocked: vec![],
    })
}

async fn import_cf(launcher: &Launcher, path: &Path, manifest: CfManifest, progress: &ProgressFn) -> Result<ImportResult> {
    let primary = manifest
        .minecraft
        .mod_loaders
        .iter()
        .find(|l| l.primary)
        .or(manifest.minecraft.mod_loaders.first());
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
    let mut blocked = vec![];
    let mut tasks = vec![];
    let mut metas = vec![];
    for f in files {
        let rel = format!("mods/{}", f.file_name);
        let sha1 = f.hashes.iter().find(|h| h.algo == 1).map(|h| h.value.clone());
        match &f.download_url {
            Some(url) => {
                tasks.push(DownloadTask::new(url, crate::util::safe_join(&dir, &rel)?).sha1(sha1.clone()));
                metas.push((rel, f.mod_id, f.id, f.display_name.clone(), sha1));
            }
            None => blocked.push(BlockedFile {
                name: f.display_name.clone(),
                url: format!("https://www.curseforge.com/api/v1/mods/{}/files/{}/download", f.mod_id, f.id),
            }),
        }
    }
    let concurrency = launcher.settings.read().await.max_concurrent_downloads;
    download_all(&launcher.http, tasks, concurrency, Stage::Content, progress).await?;
    extract_prefix(path, &format!("{}/", manifest.overrides.trim_end_matches('/')), &dir).await?;

    for (rel, mod_id, file_id, name, sha1) in metas {
        record(
            launcher,
            &inst,
            rel,
            ContentMeta {
                source: Source::CurseForge,
                project_id: Some(mod_id.to_string()),
                version_id: Some(file_id.to_string()),
                title: None,
                version_name: Some(name),
                icon_url: None,
                sha1,
            },
        )
        .await?;
    }
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
            let Some(rel) = name.strip_prefix(&prefix) else { continue };
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
