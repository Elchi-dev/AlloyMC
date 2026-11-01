//! Brings instances over from other launchers: Prism Launcher, MultiMC, the CurseForge App
//! and the Modrinth App. Finds them where those launchers keep them (or in a folder you
//! pick), reads their version and loader, and copies the game folder (mods, configs,
//! worlds, everything) into a new Alloy instance. The original is never changed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::instance::{self, Instance, NewInstance};
use crate::meta::loaders::Loader;
use crate::progress::{Progress, ProgressFn, Stage, Unit};
use crate::{Error, Launcher, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum From {
    Prism,
    MultiMc,
    CurseForgeApp,
    ModrinthApp,
}

impl From {
    pub fn label(self) -> &'static str {
        match self {
            From::Prism => "Prism Launcher",
            From::MultiMc => "MultiMC",
            From::CurseForgeApp => "CurseForge App",
            From::ModrinthApp => "Modrinth App",
        }
    }
}

/// An instance another launcher has.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    pub from: From,
    pub name: String,
    /// The game folder (mods/, saves/, config/...).
    pub game_dir: PathBuf,
    pub game_version: String,
    pub loader: Loader,
    pub loader_version: Option<String>,
    pub mods: usize,
    pub worlds: usize,
    /// From the other launcher's settings, when it sets one.
    pub memory_mb: Option<u32>,
}

/// Not copied: logs, caches, and the other launcher's own files.
const SKIP: &[&str] = &[
    "logs", "crash-reports", ".cache", "natives", "minecraftinstance.json", ".curseclient", "profile.json", "instance.cfg", "mmc-pack.json",
    ".fabric", ".quilt", ".mixin.out", "downloads", "essential", ".index",
];

/// Every instance of every launcher Alloy knows where to look for.
pub async fn discover() -> Vec<Found> {
    tokio::task::spawn_blocking(|| {
        let mut out = vec![];
        for (from, dir) in roots() {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                if e.file_type().is_ok_and(|t| t.is_dir()) {
                    if let Some(f) = read(from, &e.path()) {
                        out.push(f);
                    }
                }
            }
        }
        out.sort_by(|a, b| (a.from as u8).cmp(&(b.from as u8)).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        out.dedup_by(|a, b| a.game_dir == b.game_dir);
        out
    })
    .await
    .unwrap_or_default()
}

/// A folder someone picked: an instance of any of these launchers, or a folder of them.
pub async fn inspect(path: PathBuf) -> Vec<Found> {
    tokio::task::spawn_blocking(move || {
        for from in [From::Prism, From::CurseForgeApp, From::ModrinthApp] {
            if let Some(f) = read(from, &path) {
                return vec![f];
            }
        }
        // A folder of instances.
        let mut out = vec![];
        if let Ok(rd) = std::fs::read_dir(&path) {
            for e in rd.flatten().filter(|e| e.path().is_dir()) {
                for from in [From::Prism, From::CurseForgeApp, From::ModrinthApp] {
                    if let Some(f) = read(from, &e.path()) {
                        out.push(f);
                        break;
                    }
                }
            }
        }
        out
    })
    .await
    .unwrap_or_default()
}

/// Where each launcher keeps its instances on this system.
fn roots() -> Vec<(From, PathBuf)> {
    let mut out = vec![];
    let data = dirs::data_dir();
    let home = dirs::home_dir();
    let prism_dirs: Vec<PathBuf> = [
        data.as_ref().map(|d| d.join("PrismLauncher")),
        home.as_ref().map(|h| h.join(".var/app/org.prismlauncher.PrismLauncher/data/PrismLauncher")),
    ]
    .into_iter()
    .flatten()
    .collect();
    for d in prism_dirs {
        out.push((From::Prism, instance_dir(&d, "prismlauncher.cfg")));
    }
    if let Some(d) = data.as_ref().map(|d| d.join("multimc")) {
        out.push((From::MultiMc, instance_dir(&d, "multimc.cfg")));
    }
    for base in [home.clone(), dirs::document_dir()].into_iter().flatten() {
        out.push((From::CurseForgeApp, base.join("curseforge").join("minecraft").join("Instances")));
    }
    for name in ["ModrinthApp", "com.modrinth.theseus"] {
        if let Some(d) = data.as_ref().map(|d| d.join(name)) {
            out.push((From::ModrinthApp, d.join("profiles")));
        }
    }
    out
}

/// Prism and MultiMC let you move the instances folder; their config says where.
fn instance_dir(base: &Path, cfg: &str) -> PathBuf {
    let custom = std::fs::read_to_string(base.join(cfg)).ok().and_then(|t| ini(&t).get("InstanceDir").cloned()).filter(|v| !v.is_empty());
    match custom {
        Some(v) if Path::new(&v).is_absolute() => PathBuf::from(v),
        Some(v) => base.join(v),
        None => base.join("instances"),
    }
}

fn ini(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter(|l| !l.starts_with('[') && !l.starts_with('#') && !l.starts_with(';'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().trim_matches('"').to_string()))
        .collect()
}

fn read(from: From, dir: &Path) -> Option<Found> {
    match from {
        From::Prism | From::MultiMc => read_prism(from, dir),
        From::CurseForgeApp => read_curseforge(dir),
        From::ModrinthApp => read_modrinth(dir),
    }
}

fn counts(game: &Path) -> (usize, usize) {
    let n = |sub: &str, f: fn(&std::fs::DirEntry) -> bool| std::fs::read_dir(game.join(sub)).map(|rd| rd.flatten().filter(f).count()).unwrap_or(0);
    (n("mods", |e| e.file_name().to_string_lossy().ends_with(".jar")), n("saves", |e| e.path().is_dir()))
}

#[derive(Deserialize)]
struct MmcPack {
    components: Vec<MmcComponent>,
}
#[derive(Deserialize)]
struct MmcComponent {
    uid: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default, rename = "cachedVersion")]
    cached_version: Option<String>,
}

fn read_prism(from: From, dir: &Path) -> Option<Found> {
    let cfg = ini(&std::fs::read_to_string(dir.join("instance.cfg")).ok()?);
    let pack: MmcPack = serde_json::from_slice(&std::fs::read(dir.join("mmc-pack.json")).ok()?).ok()?;
    let version = |uid: &str| pack.components.iter().find(|c| c.uid == uid).and_then(|c| c.version.clone().or(c.cached_version.clone()));
    let game_version = version("net.minecraft")?;
    let (loader, loader_version) = [
        ("net.fabricmc.fabric-loader", Loader::Fabric),
        ("org.quiltmc.quilt-loader", Loader::Quilt),
        ("net.neoforged", Loader::NeoForge),
        ("net.minecraftforge", Loader::Forge),
    ]
    .into_iter()
    .find_map(|(uid, l)| version(uid).map(|v| (l, Some(v))))
    .unwrap_or((Loader::Vanilla, None));
    let game_dir = [".minecraft", "minecraft"].iter().map(|d| dir.join(d)).find(|d| d.is_dir()).unwrap_or_else(|| dir.join(".minecraft"));
    let (mods, worlds) = counts(&game_dir);
    let memory_mb = (cfg.get("OverrideMemory").map(String::as_str) == Some("true")).then(|| cfg.get("MaxMemAlloc").and_then(|m| m.parse().ok())).flatten();
    Some(Found {
        from,
        name: cfg.get("name").cloned().unwrap_or_else(|| dir.file_name().unwrap_or_default().to_string_lossy().into_owned()),
        game_dir,
        game_version,
        loader,
        loader_version,
        mods,
        worlds,
        memory_mb,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfInstance {
    name: String,
    game_version: String,
    #[serde(default)]
    base_mod_loader: Option<CfLoader>,
    #[serde(default)]
    allocated_memory: Option<u32>,
}
#[derive(Deserialize)]
struct CfLoader {
    name: String,
}

fn read_curseforge(dir: &Path) -> Option<Found> {
    let i: CfInstance = serde_json::from_slice(&std::fs::read(dir.join("minecraftinstance.json")).ok()?).ok()?;
    let (loader, loader_version) = match &i.base_mod_loader {
        Some(l) => {
            let (name, v) = l.name.split_once('-').unwrap_or((&l.name, ""));
            (Loader::from_modrinth(name).unwrap_or(Loader::Vanilla), Some(v.to_string()).filter(|v| !v.is_empty()))
        }
        None => (Loader::Vanilla, None),
    };
    let (mods, worlds) = counts(dir);
    Some(Found {
        from: From::CurseForgeApp,
        name: i.name,
        game_dir: dir.to_path_buf(),
        game_version: i.game_version,
        loader,
        loader_version,
        mods,
        worlds,
        memory_mb: i.allocated_memory.filter(|m| *m >= 512),
    })
}

/// The Modrinth App keeps profiles in a SQLite database next to the profiles folder (older
/// versions wrote a `profile.json` in each).
fn read_modrinth(dir: &Path) -> Option<Found> {
    let folder = dir.file_name()?.to_string_lossy().to_string();
    let (name, game_version, loader, loader_version) = modrinth_profile_json(dir).or_else(|| modrinth_db(dir, &folder))?;
    let (mods, worlds) = counts(dir);
    Some(Found {
        from: From::ModrinthApp,
        name,
        game_dir: dir.to_path_buf(),
        game_version,
        loader: Loader::from_modrinth(&loader).unwrap_or(Loader::Vanilla),
        loader_version: loader_version.filter(|v| !v.is_empty()),
        mods,
        worlds,
        memory_mb: None,
    })
}

type Meta = (String, String, String, Option<String>);

fn modrinth_profile_json(dir: &Path) -> Option<Meta> {
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join("profile.json")).ok()?).ok()?;
    let m = v.get("metadata")?;
    Some((
        m.get("name")?.as_str()?.to_string(),
        m.get("game_version")?.as_str()?.to_string(),
        m.get("loader").and_then(|l| l.as_str()).unwrap_or("vanilla").to_string(),
        m.get("loader_version").and_then(|l| l.get("id")).and_then(|i| i.as_str()).map(str::to_string),
    ))
}

fn modrinth_db(dir: &Path, folder: &str) -> Option<Meta> {
    let db = dir.parent()?.parent()?.join("app.db");
    if !db.is_file() {
        return None;
    }
    // immutable: read even while the Modrinth App has it open, and never write.
    let uri = format!("file:{}?immutable=1", db.to_string_lossy().replace('?', "%3f").replace('#', "%23"));
    let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = rusqlite::Connection::open_with_flags(uri, flags).ok()?;
    conn.query_row(
        "SELECT name, game_version, mod_loader, mod_loader_version FROM profiles WHERE path = ?1",
        [folder],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, Option<String>>(3)?)),
    )
    .ok()
}

/// Copies an instance into Alloy. Returns the new instance; installing the game files
/// happens on first Play like any other instance.
pub async fn import(launcher: &Launcher, found: &Found, progress: &ProgressFn) -> Result<Instance> {
    if !found.game_dir.is_dir() {
        return Err(Error::NotFound(format!("{}'s game folder", found.name)));
    }
    let mut inst = instance::create(
        launcher,
        NewInstance {
            name: found.name.clone(),
            game_version: found.game_version.clone(),
            loader: found.loader,
            loader_version: found.loader_version.clone(),
            icon: None,
            group: None,
            fabric_api: Some(false),
        },
    )
    .await?;
    if let Some(m) = found.memory_mb {
        inst.memory_mb = Some(m);
        inst.save(launcher).await?;
    }
    let from = found.game_dir.clone();
    let to = inst.dir(launcher);
    let progress = progress.clone();
    let result = tokio::task::spawn_blocking(move || {
        let total = tree_size(&from);
        let done = AtomicU64::new(0);
        let report = |n: u64| {
            let now = done.fetch_add(n, Ordering::Relaxed) + n;
            progress(Progress { stage: Stage::Extracting, current: now, total, unit: Unit::Bytes, message: Some("Copying the instance".into()), files: None });
        };
        copy_tree(&from, &to, true, &report)
    })
    .await
    .map_err(|e| Error::other(e.to_string()))?;
    if let Err(e) = result {
        // Half a copy is worse than none.
        let _ = tokio::fs::remove_dir_all(inst.dir(launcher)).await;
        return Err(e);
    }
    Ok(inst)
}

fn tree_size(p: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(p) else { return 0 };
    rd.flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => tree_size(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map(|m| m.len()).unwrap_or(0),
            _ => 0,
        })
        .sum()
}

fn copy_tree(from: &Path, to: &Path, top: bool, report: &dyn Fn(u64)) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if top && SKIP.contains(&name.as_str()) {
            continue;
        }
        let Ok(t) = e.file_type() else { continue };
        let dest = to.join(&name);
        if t.is_dir() {
            copy_tree(&e.path(), &dest, false, report)?;
        } else if t.is_file() {
            let n = std::fs::copy(e.path(), &dest)?;
            report(n);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_prism_instance() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path().join("ATM");
        std::fs::create_dir_all(d.join(".minecraft/mods")).unwrap();
        std::fs::create_dir_all(d.join(".minecraft/saves/World")).unwrap();
        std::fs::write(d.join(".minecraft/mods/a.jar"), b"").unwrap();
        std::fs::write(d.join("instance.cfg"), "[General]\nname=All the Mods 10\nOverrideMemory=true\nMaxMemAlloc=8192\n").unwrap();
        std::fs::write(
            d.join("mmc-pack.json"),
            r#"{"components":[{"uid":"org.lwjgl3","version":"3.3.3"},{"uid":"net.minecraft","version":"1.21.1"},{"uid":"net.neoforged","version":"21.1.77"}],"formatVersion":1}"#,
        )
        .unwrap();
        let f = read(From::Prism, &d).unwrap();
        assert_eq!(f.name, "All the Mods 10");
        assert_eq!(f.game_version, "1.21.1");
        assert_eq!(f.loader, Loader::NeoForge);
        assert_eq!(f.loader_version.as_deref(), Some("21.1.77"));
        assert_eq!((f.mods, f.worlds), (1, 1));
        assert_eq!(f.memory_mb, Some(8192));
    }

    #[test]
    fn reads_a_curseforge_app_instance() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        std::fs::write(
            d.join("minecraftinstance.json"),
            r#"{"name":"Create Above","gameVersion":"1.20.1","baseModLoader":{"name":"forge-47.2.0","minecraftVersion":"1.20.1"},"allocatedMemory":6144,"installedAddons":[]}"#,
        )
        .unwrap();
        let f = read(From::CurseForgeApp, d).unwrap();
        assert_eq!((f.loader, f.loader_version.as_deref()), (Loader::Forge, Some("47.2.0")));
        assert_eq!(f.memory_mb, Some(6144));
    }

    #[test]
    fn reads_a_modrinth_app_profile_from_its_database() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("profiles/Fabulously Optimized/mods")).unwrap();
        let conn = rusqlite::Connection::open(root.join("app.db")).unwrap();
        conn.execute_batch(
            "CREATE TABLE profiles (path TEXT PRIMARY KEY, name TEXT, game_version TEXT, mod_loader TEXT, mod_loader_version TEXT);
             INSERT INTO profiles VALUES ('Fabulously Optimized', 'Fabulously Optimized', '1.21.1', 'fabric', '0.16.5');",
        )
        .unwrap();
        drop(conn);
        let f = read(From::ModrinthApp, &root.join("profiles/Fabulously Optimized")).unwrap();
        assert_eq!(f.name, "Fabulously Optimized");
        assert_eq!((f.loader, f.loader_version.as_deref()), (Loader::Fabric, Some("0.16.5")));
    }

    #[tokio::test]
    async fn imports_a_prism_instance_into_alloy() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("prism/instances/Pack");
        std::fs::create_dir_all(src.join(".minecraft/mods")).unwrap();
        std::fs::create_dir_all(src.join(".minecraft/saves/World")).unwrap();
        std::fs::write(src.join(".minecraft/mods/a.jar"), b"jar").unwrap();
        std::fs::write(src.join(".minecraft/saves/World/level.dat"), b"lvl").unwrap();
        std::fs::write(src.join("instance.cfg"), "name=My Pack\n").unwrap();
        std::fs::write(src.join("mmc-pack.json"), r#"{"components":[{"uid":"net.minecraft","version":"1.20.1"}]}"#).unwrap();
        let found = inspect(tmp.path().join("prism/instances")).await;
        assert_eq!(found.len(), 1);
        let launcher = Launcher::open(crate::paths::Dirs::new(tmp.path().join("alloy"))).await.unwrap();
        let inst = import(&launcher, &found[0], &crate::progress::noop()).await.unwrap();
        assert_eq!(inst.name, "My Pack");
        assert_eq!(inst.loader, Loader::Vanilla);
        let dir = inst.dir(&launcher);
        assert_eq!(std::fs::read(dir.join("mods/a.jar")).unwrap(), b"jar");
        assert!(dir.join("saves/World/level.dat").exists());
        // The original is untouched.
        assert!(src.join(".minecraft/mods/a.jar").exists());
    }

    #[test]
    fn copies_without_the_other_launchers_files() {
        let tmp = tempfile::tempdir().unwrap();
        let from = tmp.path().join("from");
        std::fs::create_dir_all(from.join("mods")).unwrap();
        std::fs::create_dir_all(from.join("logs")).unwrap();
        std::fs::write(from.join("mods/a.jar"), b"abc").unwrap();
        std::fs::write(from.join("logs/latest.log"), b"x").unwrap();
        std::fs::write(from.join("minecraftinstance.json"), b"{}").unwrap();
        let to = tmp.path().join("to");
        let n = AtomicU64::new(0);
        copy_tree(&from, &to, true, &|b| {
            n.fetch_add(b, Ordering::Relaxed);
        })
        .unwrap();
        assert!(to.join("mods/a.jar").exists());
        assert!(!to.join("logs").exists());
        assert!(!to.join("minecraftinstance.json").exists());
        assert_eq!(n.load(Ordering::Relaxed), 3);
    }
}
