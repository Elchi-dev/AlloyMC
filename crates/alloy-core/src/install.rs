//! Installs everything an instance needs to start: version JSONs, client jar, libraries,
//! natives, assets, logging config and Java. Shared files are reused across instances.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;

use crate::download::{download_all, DownloadTask};
use crate::instance::{InstallState, Instance};
use crate::meta::forge;
use crate::meta::loaders::{self, Loader};
use crate::meta::rules::Features;
use crate::meta::{mojang, VersionJson};
use crate::progress::{emit, ProgressFn, Stage, Unit};
use crate::{java, Error, Launcher, Result};

/// The merged version (loader on top of vanilla) plus where things live.
#[derive(Debug, Clone)]
pub struct ResolvedVersion {
    pub json: VersionJson,
    /// The vanilla version id (its client jar is what everything starts from).
    pub vanilla_id: String,
    /// The game jar for the classpath. Forge and NeoForge want it named after their profile.
    pub client_jar: PathBuf,
    /// Set for Forge and NeoForge, whose installers still have work to do after downloads.
    pub forge: Option<forge::Target>,
}

pub async fn resolve(launcher: &Launcher, instance: &Instance) -> Result<ResolvedVersion> {
    let vanilla = mojang::version_json(launcher, &instance.game_version).await?;
    let json = match instance.loader {
        Loader::Vanilla => vanilla,
        loader => {
            let lv = instance.loader_version.as_deref().ok_or_else(|| Error::other("Instance has a loader but no loader version"))?;
            loaders::profile(launcher, loader, &instance.game_version, lv).await?.merge_onto(vanilla)
        }
    };
    let forge = match (instance.loader, &instance.loader_version) {
        (Loader::Forge | Loader::NeoForge, Some(v)) => Some(forge::Target::new(instance.loader, &instance.game_version, v)?),
        _ => None,
    };
    let client_jar = match forge {
        Some(_) => launcher.dirs.client_jar(&json.id),
        None => launcher.dirs.client_jar(&instance.game_version),
    };
    Ok(ResolvedVersion { json, vanilla_id: instance.game_version.clone(), client_jar, forge })
}

#[derive(Deserialize)]
struct AssetIndex {
    objects: HashMap<String, AssetObject>,
    #[serde(default, rename = "virtual")]
    is_virtual: bool,
    #[serde(default)]
    map_to_resources: bool,
}
#[derive(Deserialize)]
struct AssetObject {
    hash: String,
    size: u64,
}

const RESOURCES_URL: &str = "https://resources.download.minecraft.net";

/// Full install. Safe to call repeatedly: valid files are skipped, so it doubles as "repair".
/// `repair` re-hashes every existing file; otherwise files whose size matches are trusted,
/// which makes a second instance of the same version install in about a second.
pub async fn install(launcher: &Launcher, instance: &mut Instance, progress: &ProgressFn, repair: bool) -> Result<ResolvedVersion> {
    instance.install_state = InstallState::Installing;
    instance.save(launcher).await?;

    let result = install_inner(launcher, instance, progress, !repair).await;
    instance.install_state = if result.is_ok() { InstallState::Installed } else { InstallState::Failed };
    instance.save(launcher).await?;
    result
}

async fn install_inner(launcher: &Launcher, instance: &Instance, progress: &ProgressFn, quick: bool) -> Result<ResolvedVersion> {
    emit(progress, Stage::Metadata, 0, 1, Unit::Steps, Some("Reading version data".into()));
    let resolved = resolve(launcher, instance).await?;
    let json = &resolved.json;
    let concurrency = launcher.settings.read().await.max_concurrent_downloads;

    let java = match &instance.java_path {
        Some(p) => p.clone(),
        None => java::ensure(launcher, &json.java_component(), json.java_major(), progress).await?,
    };

    // Client jar, libraries, logging config
    let mut tasks = vec![];
    if let Some(client) = json.downloads.as_ref().and_then(|d| d.client.as_ref()) {
        tasks.push(
            DownloadTask::new(&client.url, launcher.dirs.client_jar(&resolved.vanilla_id))
                .sha1(client.sha1.clone())
                .size(client.size)
                .quick(quick),
        );
    }
    tasks.extend(library_tasks(launcher, json).into_iter().map(|t| t.quick(quick)));
    if let Some(log) = json.logging.as_ref().and_then(|l| l.client.as_ref()) {
        tasks.push(DownloadTask::new(&log.file.url, log_config_path(launcher, &log.file.id)).sha1(Some(log.file.sha1.clone())));
    }
    if let Some(target) = &resolved.forge {
        emit(progress, Stage::Loader, 0, 1, Unit::Steps, Some("Reading the loader installer".into()));
        tasks.extend(forge::prepare_files(launcher, target).await?.into_iter().map(|t| t.quick(quick)));
    }
    download_all(&launcher.http, tasks, concurrency, Stage::Libraries, progress).await?;
    if let Some(target) = &resolved.forge {
        forge::run_processors(launcher, target, &java, progress, quick).await?;
        forge::ensure_version_jar(launcher, &json.id, &resolved.vanilla_id).await?;
    }

    // Assets
    if let Some(index_ref) = &json.asset_index {
        let index_path = launcher.dirs.assets().join("indexes").join(format!("{}.json", index_ref.id));
        download_all(
            &launcher.http,
            vec![DownloadTask::new(&index_ref.url, &index_path).sha1(Some(index_ref.sha1.clone()))],
            1,
            Stage::Assets,
            &crate::progress::noop(),
        )
        .await?;
        let index: AssetIndex = serde_json::from_slice(&tokio::fs::read(&index_path).await?)?;
        let objects = launcher.dirs.assets().join("objects");
        let tasks = index
            .objects
            .values()
            .map(|o| {
                let prefix = &o.hash[..2];
                DownloadTask::new(format!("{RESOURCES_URL}/{prefix}/{}", o.hash), objects.join(prefix).join(&o.hash))
                    .sha1(Some(o.hash.clone()))
                    .size(Some(o.size))
                    .quick(quick)
            })
            .collect();
        download_all(&launcher.http, tasks, concurrency, Stage::Assets, progress).await?;

        // Very old versions (pre-1.7.2) read assets by name from a "virtual" folder.
        if index.is_virtual || index.map_to_resources {
            let virt = launcher.dirs.assets().join("virtual").join(&index_ref.id);
            for (name, o) in &index.objects {
                let dest = crate::util::safe_join(&virt, name)?;
                if !dest.exists() {
                    if let Some(parent) = dest.parent() {
                        tokio::fs::create_dir_all(parent).await?;
                    }
                    tokio::fs::copy(objects.join(&o.hash[..2]).join(&o.hash), &dest).await?;
                }
            }
        }
    }

    emit(progress, Stage::Done, 1, 1, Unit::Steps, None);
    Ok(resolved)
}

pub fn log_config_path(launcher: &Launcher, id: &str) -> PathBuf {
    launcher.dirs.assets().join("log_configs").join(id)
}

fn library_tasks(launcher: &Launcher, json: &VersionJson) -> Vec<DownloadTask> {
    let features = Features::default();
    json.libraries
        .iter()
        .flat_map(|l| l.files(&features))
        .filter(|f| !f.url.is_empty())
        .map(|f| DownloadTask::new(&f.url, launcher.dirs.libraries().join(&f.path)).sha1(f.sha1).size(f.size))
        .collect()
}
