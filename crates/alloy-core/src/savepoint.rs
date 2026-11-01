//! Save points: a copy of an instance's setup (mods, configs, resource and shader packs,
//! options, versions) to go back to after an update or an experiment goes wrong. Worlds are
//! not included: they change every second you play and have their own backups.
//!
//! Files are stored once by SHA-1 in `<data>/savepoints/objects`, shared by every save point
//! of every instance, so a second save point of a 300-mod pack costs only what changed.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha1::Digest;

use crate::instance::{self, InstallState, Instance};
use crate::meta::loaders::Loader;
use crate::settings::write_json_atomic;
use crate::{Error, Launcher, Result};

/// Folders a save point covers, relative to the instance.
pub const FOLDERS: &[&str] = &["mods", "config", "defaultconfigs", "kubejs", "scripts", "resourcepacks", "shaderpacks"];
/// Single files a save point covers.
pub const FILES: &[&str] = &["options.txt", "servers.dat", ".launcher/content.json"];
/// Automatic save points kept per instance; the oldest go first. Ones you made are kept.
const KEEP_AUTO: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavePoint {
    pub id: String,
    pub label: String,
    /// Made by Alloy before a change (an update, a restore), not by you.
    pub auto: bool,
    pub created_at: i64,
    pub game_version: String,
    pub loader: Loader,
    pub loader_version: Option<String>,
    pub mods: usize,
    /// Bytes the files add up to (not what the save point costs on disk).
    pub size: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct Manifest {
    point: SavePoint,
    /// Path relative to the instance (as on disk, `.disabled` included) to SHA-1.
    files: BTreeMap<String, String>,
}

fn store(launcher: &Launcher) -> PathBuf {
    launcher.dirs.root().join("savepoints")
}

fn object(launcher: &Launcher, sha1: &str) -> PathBuf {
    store(launcher).join("objects").join(&sha1[..2]).join(sha1)
}

fn manifests_dir(launcher: &Launcher, instance_id: &str) -> PathBuf {
    store(launcher).join("instances").join(instance_id)
}

/// Every file a save point of this instance covers, relative paths.
async fn covered(dir: &Path) -> Result<Vec<String>> {
    let dir = dir.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let mut out = vec![];
        for f in FOLDERS {
            walk(&dir, &dir.join(f), &mut out);
        }
        for f in FILES {
            if dir.join(f).is_file() {
                out.push(f.to_string());
            }
        }
        out.sort();
        out
    })
    .await
    .map_err(|e| Error::other(e.to_string()))
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(t) = e.file_type() else { continue };
        if t.is_dir() {
            walk(root, &p, out);
        } else if t.is_file() {
            if let Ok(rel) = p.strip_prefix(root) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
}

fn sha1_file(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = sha1::Sha1::new();
    std::io::copy(&mut f, &mut h)?;
    Ok(hex::encode(h.finalize()))
}

/// Makes a save point of the instance as it is now.
pub async fn create(launcher: &Launcher, inst: &Instance, label: &str, auto: bool) -> Result<SavePoint> {
    let dir = inst.dir(launcher);
    let paths = covered(&dir).await?;
    let store = launcher.clone();
    let dir2 = dir.clone();
    let (files, size) = tokio::task::spawn_blocking(move || -> Result<(BTreeMap<String, String>, u64)> {
        let mut files = BTreeMap::new();
        let mut size = 0;
        for rel in paths {
            let full = dir2.join(&rel);
            let Ok(meta) = std::fs::metadata(&full) else { continue };
            let sha = sha1_file(&full)?;
            let obj = object(&store, &sha);
            if !obj.exists() {
                std::fs::create_dir_all(obj.parent().unwrap())?;
                let tmp = obj.with_extension("part");
                std::fs::copy(&full, &tmp)?;
                std::fs::rename(&tmp, &obj)?;
            }
            size += meta.len();
            files.insert(rel, sha);
        }
        Ok((files, size))
    })
    .await
    .map_err(|e| Error::other(e.to_string()))??;

    let mods = files.keys().filter(|p| p.starts_with("mods/") && (p.ends_with(".jar") || p.ends_with(".jar.disabled"))).count();
    let now = chrono::Utc::now();
    let point = SavePoint {
        id: format!("{}-{:04x}", now.format("%Y%m%d-%H%M%S"), rand_u16()),
        label: label.trim().chars().take(80).collect::<String>(),
        auto,
        created_at: now.timestamp(),
        game_version: inst.game_version.clone(),
        loader: inst.loader,
        loader_version: inst.loader_version.clone(),
        mods,
        size,
    };
    let dir = manifests_dir(launcher, &inst.id);
    tokio::fs::create_dir_all(&dir).await?;
    write_json_atomic(&dir.join(format!("{}.json", point.id)), &Manifest { point: point.clone(), files }).await?;
    if auto {
        prune(launcher, &inst.id).await?;
    }
    Ok(point)
}

fn rand_u16() -> u16 {
    (uuid::Uuid::new_v4().as_u128() & 0xffff) as u16
}

async fn manifests(launcher: &Launcher, instance_id: &str) -> Vec<(PathBuf, Manifest)> {
    let mut out = vec![];
    let Ok(mut rd) = tokio::fs::read_dir(manifests_dir(launcher, instance_id)).await else { return out };
    while let Ok(Some(e)) = rd.next_entry().await {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "json") {
            if let Some(m) = tokio::fs::read(&p).await.ok().and_then(|b| serde_json::from_slice::<Manifest>(&b).ok()) {
                out.push((p, m));
            }
        }
    }
    out.sort_by_key(|(_, m)| std::cmp::Reverse(m.point.created_at));
    out
}

/// Newest first.
pub async fn list(launcher: &Launcher, instance_id: &str) -> Vec<SavePoint> {
    manifests(launcher, instance_id).await.into_iter().map(|(_, m)| m.point).collect()
}

/// Puts the instance back the way it was at the save point. Makes an automatic save point
/// of the current state first, so a restore can be undone.
pub async fn restore(launcher: &Launcher, instance_id: &str, point_id: &str) -> Result<Instance> {
    let (_, manifest) = manifests(launcher, instance_id)
        .await
        .into_iter()
        .find(|(_, m)| m.point.id == point_id)
        .ok_or_else(|| Error::NotFound(format!("save point {point_id}")))?;
    let mut inst = instance::get(launcher, instance_id).await?;
    create(launcher, &inst, &format!("Before going back to \"{}\"", manifest.point.label), true).await?;

    let dir = inst.dir(launcher);
    // Everything the save point covers that it doesn't have goes; the rest is written back.
    let now = covered(&dir).await?;
    let launcher2 = launcher.clone();
    let files = manifest.files.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        for rel in now {
            if !files.contains_key(&rel) {
                let _ = std::fs::remove_file(dir.join(&rel));
            }
        }
        for (rel, sha) in &files {
            let dest = crate::util::safe_join(&dir, rel)?;
            if dest.is_file() && sha1_file(&dest).is_ok_and(|s| s == *sha) {
                continue;
            }
            let obj = object(&launcher2, sha);
            if !obj.exists() {
                return Err(Error::other(format!("This save point is missing a file ({rel}). It can't be restored.")));
            }
            if let Some(p) = dest.parent() {
                std::fs::create_dir_all(p)?;
            }
            let tmp = dest.with_extension("alloy-restore");
            std::fs::copy(&obj, &tmp)?;
            std::fs::rename(&tmp, &dest)?;
        }
        Ok(())
    })
    .await
    .map_err(|e| Error::other(e.to_string()))??;

    let p = &manifest.point;
    if inst.game_version != p.game_version || inst.loader != p.loader || inst.loader_version != p.loader_version {
        inst.game_version = p.game_version.clone();
        inst.loader = p.loader;
        inst.loader_version = p.loader_version.clone();
        inst.install_state = InstallState::NotInstalled;
    }
    inst.save(launcher).await?;
    Ok(inst)
}

pub async fn rename(launcher: &Launcher, instance_id: &str, point_id: &str, label: &str) -> Result<()> {
    let (path, mut m) = manifests(launcher, instance_id)
        .await
        .into_iter()
        .find(|(_, m)| m.point.id == point_id)
        .ok_or_else(|| Error::NotFound(format!("save point {point_id}")))?;
    m.point.label = label.trim().chars().take(80).collect();
    // Named by you now: keep it.
    m.point.auto = false;
    write_json_atomic(&path, &m).await
}

pub async fn delete(launcher: &Launcher, instance_id: &str, point_id: &str) -> Result<()> {
    if let Some((path, _)) = manifests(launcher, instance_id).await.into_iter().find(|(_, m)| m.point.id == point_id) {
        tokio::fs::remove_file(path).await?;
    }
    collect_garbage(launcher).await
}

/// Drops automatic save points beyond the newest few.
async fn prune(launcher: &Launcher, instance_id: &str) -> Result<()> {
    let autos: Vec<PathBuf> = manifests(launcher, instance_id).await.into_iter().filter(|(_, m)| m.point.auto).map(|(p, _)| p).collect();
    if autos.len() <= KEEP_AUTO {
        return Ok(());
    }
    for p in &autos[KEEP_AUTO..] {
        let _ = tokio::fs::remove_file(p).await;
    }
    collect_garbage(launcher).await
}

/// Deletes every save point of an instance (when the instance itself is deleted for good).
pub async fn forget_instance(launcher: &Launcher, instance_id: &str) -> Result<()> {
    let _ = tokio::fs::remove_dir_all(manifests_dir(launcher, instance_id)).await;
    collect_garbage(launcher).await
}

/// Removes stored files no save point refers to any more.
pub async fn collect_garbage(launcher: &Launcher) -> Result<()> {
    let root = store(launcher);
    let mut used: HashSet<String> = HashSet::new();
    if let Ok(mut rd) = tokio::fs::read_dir(root.join("instances")).await {
        while let Ok(Some(e)) = rd.next_entry().await {
            let id = e.file_name().to_string_lossy().to_string();
            for (_, m) in manifests(launcher, &id).await {
                used.extend(m.files.into_values());
            }
        }
    }
    let objects = root.join("objects");
    tokio::task::spawn_blocking(move || {
        let Ok(rd) = std::fs::read_dir(&objects) else { return };
        for shard in rd.flatten() {
            let Ok(files) = std::fs::read_dir(shard.path()) else { continue };
            for f in files.flatten() {
                let name = f.file_name().to_string_lossy().to_string();
                if !used.contains(&name) {
                    let _ = std::fs::remove_file(f.path());
                }
            }
        }
    })
    .await
    .map_err(|e| Error::other(e.to_string()))
}

/// What the save points of an instance take on disk, counting shared files once.
pub async fn disk_usage(launcher: &Launcher, instance_id: &str) -> u64 {
    let mut seen = HashSet::new();
    let mut total = 0;
    for (_, m) in manifests(launcher, instance_id).await {
        for sha in m.files.values() {
            if seen.insert(sha.clone()) {
                total += tokio::fs::metadata(object(launcher, sha)).await.map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instance::NewInstance;
    use crate::paths::Dirs;

    async fn setup() -> (tempfile::TempDir, Launcher, Instance) {
        let tmp = tempfile::tempdir().unwrap();
        let launcher = Launcher::open(Dirs::new(tmp.path())).await.unwrap();
        let inst = instance::create(
            &launcher,
            NewInstance { name: "Test".into(), game_version: "1.21.1".into(), loader: Loader::Vanilla, loader_version: None, icon: None, group: None, fabric_api: None },
        )
        .await
        .unwrap();
        (tmp, launcher, inst)
    }

    #[tokio::test]
    async fn save_change_restore() {
        let (_tmp, launcher, inst) = setup().await;
        let dir = inst.dir(&launcher);
        std::fs::write(dir.join("mods/a.jar"), b"mod a v1").unwrap();
        std::fs::write(dir.join("mods/b.jar.disabled"), b"mod b").unwrap();
        std::fs::create_dir_all(dir.join("config/deep")).unwrap();
        std::fs::write(dir.join("config/deep/x.toml"), b"x = 1").unwrap();
        std::fs::write(dir.join("options.txt"), b"fov:70").unwrap();
        std::fs::create_dir_all(dir.join("saves/World")).unwrap();
        std::fs::write(dir.join("saves/World/level.dat"), b"world v1").unwrap();

        let p = create(&launcher, &inst, "Before the update", false).await.unwrap();
        assert_eq!(p.mods, 2);

        // An update goes wrong.
        std::fs::write(dir.join("mods/a.jar"), b"mod a v2").unwrap();
        std::fs::write(dir.join("mods/c.jar"), b"new mod").unwrap();
        std::fs::remove_file(dir.join("config/deep/x.toml")).unwrap();
        std::fs::write(dir.join("saves/World/level.dat"), b"world v2").unwrap();

        restore(&launcher, &inst.id, &p.id).await.unwrap();
        assert_eq!(std::fs::read(dir.join("mods/a.jar")).unwrap(), b"mod a v1");
        assert!(dir.join("mods/b.jar.disabled").exists());
        assert!(!dir.join("mods/c.jar").exists());
        assert_eq!(std::fs::read(dir.join("config/deep/x.toml")).unwrap(), b"x = 1");
        // Worlds are left alone.
        assert_eq!(std::fs::read(dir.join("saves/World/level.dat")).unwrap(), b"world v2");

        // The restore itself made a save point, so it can be undone.
        let all = list(&launcher, &inst.id).await;
        assert_eq!(all.len(), 2);
        let undo = all.iter().find(|p| p.auto).unwrap();
        restore(&launcher, &inst.id, &undo.id).await.unwrap();
        assert_eq!(std::fs::read(dir.join("mods/a.jar")).unwrap(), b"mod a v2");
        assert!(dir.join("mods/c.jar").exists());
    }

    #[tokio::test]
    async fn files_are_stored_once_and_collected() {
        let (_tmp, launcher, inst) = setup().await;
        let dir = inst.dir(&launcher);
        std::fs::write(dir.join("mods/a.jar"), vec![7u8; 10_000]).unwrap();
        let p1 = create(&launcher, &inst, "one", false).await.unwrap();
        let p2 = create(&launcher, &inst, "two", false).await.unwrap();
        assert_eq!(disk_usage(&launcher, &inst.id).await, 10_000);
        delete(&launcher, &inst.id, &p1.id).await.unwrap();
        assert_eq!(disk_usage(&launcher, &inst.id).await, 10_000);
        delete(&launcher, &inst.id, &p2.id).await.unwrap();
        let objects = store(&launcher).join("objects");
        let left: usize = std::fs::read_dir(&objects).map(|rd| rd.flatten().map(|s| std::fs::read_dir(s.path()).unwrap().count()).sum()).unwrap_or(0);
        assert_eq!(left, 0);
    }

    #[tokio::test]
    async fn old_automatic_points_are_dropped() {
        let (_tmp, launcher, inst) = setup().await;
        let mine = create(&launcher, &inst, "mine", false).await.unwrap();
        for i in 0..(KEEP_AUTO + 3) {
            create(&launcher, &inst, &format!("auto {i}"), true).await.unwrap();
        }
        let all = list(&launcher, &inst.id).await;
        assert_eq!(all.iter().filter(|p| p.auto).count(), KEEP_AUTO);
        assert!(all.iter().any(|p| p.id == mine.id));
    }
}
