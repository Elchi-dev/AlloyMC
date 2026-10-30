//! Instances: one folder per instance, `instance.json` inside, the game runs in that folder.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::meta::loaders::Loader;
use crate::settings::write_json_atomic;
use crate::util::slugify;
use crate::{Error, Launcher, Result};

pub const INSTANCE_FILE: &str = "instance.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    /// Folder name; stable for the life of the instance.
    pub id: String,
    pub name: String,
    /// Path to an icon file inside the instance folder, or a remote URL.
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub group: Option<String>,

    pub game_version: String,
    pub loader: Loader,
    #[serde(default)]
    pub loader_version: Option<String>,

    #[serde(default)]
    pub java_path: Option<PathBuf>,
    /// None = use the launcher default.
    #[serde(default)]
    pub memory_mb: Option<u32>,
    #[serde(default)]
    pub jvm_args: Vec<String>,
    #[serde(default)]
    pub game_args: Vec<String>,
    #[serde(default)]
    pub resolution: Option<Resolution>,
    #[serde(default)]
    pub env: Vec<(String, String)>,
    /// Account that plays this instance by default (else the active account).
    #[serde(default)]
    pub account: Option<String>,

    #[serde(default)]
    pub linked_pack: Option<LinkedPack>,
    #[serde(default)]
    pub install_state: InstallState,

    pub created_at: i64,
    #[serde(default)]
    pub last_played: Option<i64>,
    #[serde(default)]
    pub play_time_secs: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InstallState {
    #[default]
    NotInstalled,
    Installing,
    Installed,
    Failed,
}

/// The modpack this instance came from, so updates know where to look.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedPack {
    pub source: PackSource,
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    pub name: Option<String>,
    pub version_name: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PackSource {
    Modrinth,
    CurseForge,
    Url,
    File,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewInstance {
    pub name: String,
    pub game_version: String,
    pub loader: Loader,
    /// None = pick the recommended loader version.
    #[serde(default)]
    pub loader_version: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub group: Option<String>,
    /// Fabric and Quilt only: start with Fabric API. Defaults to yes.
    #[serde(default)]
    pub fabric_api: Option<bool>,
}

impl Instance {
    pub fn dir(&self, launcher: &Launcher) -> PathBuf {
        launcher.dirs.instance(&self.id)
    }

    pub async fn save(&self, launcher: &Launcher) -> Result<()> {
        let dir = self.dir(launcher);
        tokio::fs::create_dir_all(&dir).await?;
        write_json_atomic(&dir.join(INSTANCE_FILE), self).await
    }
}

/// All instances, most recently played first.
pub async fn list(launcher: &Launcher) -> Result<Vec<Instance>> {
    let mut out = vec![];
    let mut rd = tokio::fs::read_dir(launcher.dirs.instances()).await?;
    while let Some(entry) = rd.next_entry().await? {
        let file = entry.path().join(INSTANCE_FILE);
        match tokio::fs::read(&file).await {
            Ok(bytes) => match serde_json::from_slice::<Instance>(&bytes) {
                Ok(i) => out.push(i),
                Err(e) => tracing::warn!("skipping {}: {e}", file.display()),
            },
            Err(_) => continue,
        }
    }
    out.sort_by(|a, b| b.last_played.unwrap_or(b.created_at).cmp(&a.last_played.unwrap_or(a.created_at)));
    Ok(out)
}

pub async fn get(launcher: &Launcher, id: &str) -> Result<Instance> {
    let file = launcher.dirs.instance(id).join(INSTANCE_FILE);
    let bytes = tokio::fs::read(&file).await.map_err(|_| Error::NotFound(format!("instance {id}")))?;
    Ok(serde_json::from_slice(&bytes)?)
}

/// Creates the instance record and folder. Installing files is a separate step
/// so the card appears instantly while the download runs.
pub async fn create(launcher: &Launcher, new: NewInstance) -> Result<Instance> {
    let name = new.name.trim();
    if name.is_empty() {
        return Err(Error::other("Instance name can't be empty"));
    }
    let base = slugify(name);
    let mut id = base.clone();
    let mut n = 2;
    while launcher.dirs.instance(&id).exists() {
        id = format!("{base}-{n}");
        n += 1;
    }

    let loader_version = match (new.loader, new.loader_version) {
        (Loader::Vanilla, _) => None,
        (_, Some(v)) => Some(v),
        (loader, None) => crate::meta::loaders::recommended(launcher, loader, &new.game_version).await?,
    };

    let instance = Instance {
        id,
        name: name.into(),
        icon: new.icon,
        group: new.group,
        game_version: new.game_version,
        loader: new.loader,
        loader_version,
        java_path: None,
        memory_mb: None,
        jvm_args: vec![],
        game_args: vec![],
        resolution: None,
        env: vec![],
        account: None,
        linked_pack: None,
        install_state: InstallState::NotInstalled,
        created_at: chrono::Utc::now().timestamp(),
        last_played: None,
        play_time_secs: 0,
    };
    let dir = instance.dir(launcher);
    for sub in ["mods", "resourcepacks", "shaderpacks", "saves", "config"] {
        tokio::fs::create_dir_all(dir.join(sub)).await?;
    }
    instance.save(launcher).await?;
    Ok(instance)
}

/// Moves the instance to `trash/` instead of deleting it.
pub async fn trash(launcher: &Launcher, id: &str) -> Result<()> {
    let from = launcher.dirs.instance(id);
    if !from.exists() {
        return Err(Error::NotFound(format!("instance {id}")));
    }
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let to = launcher.dirs.trash().join(format!("{id}-{stamp}"));
    tokio::fs::rename(&from, &to).await?;
    Ok(())
}

/// Copies an instance (files and all) under a new name.
pub async fn duplicate(launcher: &Launcher, id: &str, new_name: &str) -> Result<Instance> {
    let src = get(launcher, id).await?;
    let mut copy = create(
        launcher,
        NewInstance {
            name: new_name.into(),
            game_version: src.game_version.clone(),
            loader: src.loader,
            loader_version: src.loader_version.clone(),
            icon: src.icon.clone(),
            group: src.group.clone(),
            fabric_api: None,
        },
    )
    .await?;
    copy_dir(&src.dir(launcher), &copy.dir(launcher)).await?;
    copy = Instance { id: copy.id, name: copy.name, created_at: copy.created_at, last_played: None, play_time_secs: 0, ..src };
    copy.save(launcher).await?;
    Ok(copy)
}

fn copy_dir<'a>(from: &'a std::path::Path, to: &'a std::path::Path) -> futures::future::BoxFuture<'a, Result<()>> {
    Box::pin(async move {
        tokio::fs::create_dir_all(to).await?;
        let mut rd = tokio::fs::read_dir(from).await?;
        while let Some(e) = rd.next_entry().await? {
            let target = to.join(e.file_name());
            if e.file_type().await?.is_dir() {
                copy_dir(&e.path(), &target).await?;
            } else if e.file_name() != INSTANCE_FILE {
                tokio::fs::copy(e.path(), &target).await?;
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn create_list_trash() {
        let dir = tempfile::tempdir().unwrap();
        let launcher = Launcher::open(crate::paths::Dirs::new(dir.path())).await.unwrap();
        let new = || NewInstance {
            name: "My World".into(),
            game_version: "26.2".into(),
            loader: Loader::Vanilla,
            loader_version: None,
            icon: None,
            group: None,
            fabric_api: None,
        };
        let a = create(&launcher, new()).await.unwrap();
        let b = create(&launcher, new()).await.unwrap();
        assert_eq!(a.id, "my-world");
        assert_eq!(b.id, "my-world-2");
        assert_eq!(list(&launcher).await.unwrap().len(), 2);
        trash(&launcher, &a.id).await.unwrap();
        assert_eq!(list(&launcher).await.unwrap().len(), 1);
    }
}
