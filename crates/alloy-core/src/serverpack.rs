//! Server packs: a dedicated server built from an instance, in a folder the player picks.
//!
//! The server gets the same Minecraft and loader version, the instance's turned-on mods
//! minus the ones that only work on a client (shaders, renderers, menus: the jar says
//! `environment: client`, or Modrinth says the project doesn't run on servers), and the
//! configs and scripts. Start scripts are written next to it; `eula.txt` is only set to
//! true when the player said they accept Minecraft's EULA.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::content::{modrinth, scan, ContentKind, Source};
use crate::error::{Error, Result};
use crate::instance::Instance;
use crate::meta::loaders::Loader;
use crate::meta::{forge, mojang};
use crate::progress::{Progress, ProgressFn, Stage, Unit};
use crate::Launcher;

/// Folders a server needs from the instance.
const FOLDERS: &[&str] = &["config", "defaultconfigs", "kubejs", "scripts", "global_packs", "datapacks"];

/// Mods that only change how the game looks or feels on a client, by id, for jars that
/// don't say so themselves.
const CLIENT_ONLY_IDS: &[&str] = &[
    "sodium", "iris", "oculus", "embeddium", "rubidium", "optifine", "indium", "sodium-extra", "sodiumextra", "reeses-sodium-options",
    "reeses_sodium_options", "continuity", "entityculling", "dynamic_fps", "dynamicfps", "modmenu", "betterf3", "zoomify", "okzoomer",
    "cull-leaves", "cullleaves", "lambdynlights", "fpsreducer", "betterfpsdist", "enhancedvisuals", "mousetweaks", "controlling",
    "jeresources", "ambientsounds", "presencefootsteps", "soundphysics", "falling_leaves", "fallingleaves", "visuality", "notenoughanimations",
    "skinlayers3d", "3dskinlayers", "eatinganimation", "immediatelyfast", "chat_heads", "chatheads", "legendarytooltips", "itemphysiclite",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub dir: PathBuf,
    pub mods: usize,
    /// Mods left out because they only run on a client, by name.
    pub left_out: Vec<String>,
    /// How to start it, e.g. `./start.sh` (Linux and macOS) or `start.bat` (Windows).
    pub start: String,
    pub java_major: u32,
    pub eula_accepted: bool,
}

fn step(progress: &ProgressFn, stage: Stage, current: u64, total: u64, message: &str) {
    progress(Progress { stage, current, total, unit: Unit::Steps, message: Some(message.to_string()), files: None });
}

/// Builds the server into `dest`, which must be empty or not exist yet.
pub async fn build(launcher: &Launcher, inst: &Instance, dest: &Path, accept_eula: bool, java: Option<PathBuf>, progress: &ProgressFn) -> Result<Report> {
    if dest.exists() {
        let mut rd = tokio::fs::read_dir(dest).await?;
        if rd.next_entry().await?.is_some() {
            return Err(Error::other("Pick an empty folder for the server."));
        }
    }
    tokio::fs::create_dir_all(dest).await?;
    let vanilla = mojang::version_json(launcher, &inst.game_version).await?;
    let java_major = vanilla.java_major();
    let memory = match inst.memory_mb {
        Some(m) => m,
        None => launcher.settings.read().await.default_memory_mb,
    }
    .max(2048);

    // 1. The server itself.
    step(progress, Stage::Loader, 0, 4, "Getting the server");
    let launch_jar = match inst.loader {
        Loader::Vanilla => {
            let server = vanilla
                .downloads
                .as_ref()
                .and_then(|d| d.server.clone())
                .ok_or_else(|| Error::other(format!("Mojang has no server for Minecraft {}.", inst.game_version)))?;
            let task = crate::download::DownloadTask::new(server.url, dest.join("server.jar")).sha1(server.sha1).size(server.size);
            crate::download::download_with_retry(&launcher.http, &task).await?;
            Some("server.jar".to_string())
        }
        Loader::Fabric => {
            let loader = inst.loader_version.clone().ok_or_else(|| Error::other("This instance has no Fabric Loader version."))?;
            #[derive(Deserialize)]
            struct Installer {
                version: String,
                #[serde(default)]
                stable: bool,
            }
            let list: Vec<Installer> = launcher.http.get("https://meta.fabricmc.net/v2/versions/installer").send().await?.error_for_status()?.json().await?;
            let installer = list.iter().find(|i| i.stable).or(list.first()).ok_or_else(|| Error::other("Fabric's installer list is empty."))?;
            let url = format!("https://meta.fabricmc.net/v2/versions/loader/{}/{loader}/{}/server/jar", inst.game_version, installer.version);
            crate::download::download_with_retry(&launcher.http, &crate::download::DownloadTask::new(url, dest.join("server.jar"))).await?;
            Some("server.jar".to_string())
        }
        Loader::Quilt => {
            let loader = inst.loader_version.clone().ok_or_else(|| Error::other("This instance has no Quilt Loader version."))?;
            let java = java_for(launcher, &vanilla, java.clone()).await?;
            let meta = String::from_utf8_lossy(
                &crate::download::fetch_bytes(&launcher.http, "https://maven.quiltmc.org/repository/release/org/quiltmc/quilt-installer/maven-metadata.xml").await?,
            )
            .to_string();
            let v = between(&meta, "<release>", "</release>").ok_or_else(|| Error::other("Couldn't find Quilt's installer."))?;
            let url = format!("https://maven.quiltmc.org/repository/release/org/quiltmc/quilt-installer/{v}/quilt-installer-{v}.jar");
            let installer = launcher.dirs.meta().join("installers").join(format!("quilt-installer-{v}.jar"));
            crate::download::download_with_retry(&launcher.http, &crate::download::DownloadTask::new(url, &installer)).await?;
            run_java(
                &java,
                &[
                    "-jar".into(),
                    installer.to_string_lossy().into(),
                    "install".into(),
                    "server".into(),
                    inst.game_version.clone(),
                    loader,
                    "--download-server".into(),
                    format!("--install-dir={}", dest.to_string_lossy()),
                ],
                dest,
            )
            .await?;
            Some("quilt-server-launch.jar".to_string())
        }
        Loader::Forge | Loader::NeoForge => {
            let version = inst.loader_version.clone().ok_or_else(|| Error::other("This instance has no loader version."))?;
            let target = forge::Target::new(inst.loader, &inst.game_version, &version)?;
            let installer = forge::installer(launcher, &target).await?;
            let java = java_for(launcher, &vanilla, java.clone()).await?;
            step(progress, Stage::Loader, 1, 4, "Installing the loader on the server (this downloads its libraries)");
            run_java(&java, &["-jar".into(), installer.to_string_lossy().into(), "--installServer".into(), dest.to_string_lossy().into()], dest).await?;
            if dest.join("run.sh").exists() {
                // Modern Forge and NeoForge start through run.sh / run.bat and read memory here.
                tokio::fs::write(dest.join("user_jvm_args.txt"), format!("# Memory for the server, set by Alloy from the instance.\n-Xmx{memory}M\n")).await?;
                None
            } else {
                Some(find_forge_jar(dest).await.ok_or_else(|| Error::other("The loader's installer didn't leave a server jar."))?)
            }
        }
    };
    // The installers leave logs behind; they're no use on the server.
    if let Ok(mut rd) = tokio::fs::read_dir(dest).await {
        while let Ok(Some(e)) = rd.next_entry().await {
            if e.file_name().to_string_lossy().ends_with(".log") {
                let _ = tokio::fs::remove_file(e.path()).await;
            }
        }
    }

    // 2. Mods, without the client-only ones.
    step(progress, Stage::Content, 2, 4, "Copying mods");
    let files = scan::scan(launcher, inst).await?;
    let mods: Vec<&scan::Scanned> = files.iter().filter(|f| f.kind == ContentKind::Mod && f.enabled).collect();
    let modrinth_ids: Vec<String> = mods
        .iter()
        .filter_map(|m| m.index.as_ref().filter(|i| i.source == Source::Modrinth).and_then(|i| i.project_id.clone()))
        .collect();
    let server_unsupported: std::collections::HashSet<String> = modrinth::projects(launcher, &modrinth_ids)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.server_side.as_deref() == Some("unsupported"))
        .map(|p| p.id)
        .collect();
    let src = inst.dir(launcher);
    tokio::fs::create_dir_all(dest.join("mods")).await?;
    let mut left_out = vec![];
    let mut copied = 0;
    for m in &mods {
        let name = m.index.as_ref().and_then(|i| i.title.clone()).or_else(|| m.jar.as_ref().and_then(|j| j.mods.first()).and_then(|e| e.name.clone())).unwrap_or_else(|| m.file_name.clone());
        if client_only(m, &server_unsupported) {
            left_out.push(name);
            continue;
        }
        tokio::fs::copy(src.join(&m.path), dest.join("mods").join(&m.file_name)).await?;
        copied += 1;
    }
    left_out.sort_by_key(|n| n.to_lowercase());

    // 3. Configs and scripts.
    step(progress, Stage::Content, 3, 4, "Copying configs");
    for f in FOLDERS {
        let from = src.join(f);
        if from.is_dir() {
            copy_dir(from, dest.join(f)).await?;
        }
    }

    // 4. Start scripts and the EULA.
    let start = match &launch_jar {
        Some(jar) => {
            let cmd = format!("java -Xmx{memory}M -jar {jar} nogui");
            write_script(&dest.join("start.sh"), &format!("#!/bin/sh\n# Needs Java {java_major}.\ncd \"$(dirname \"$0\")\"\n{cmd}\n")).await?;
            tokio::fs::write(dest.join("start.bat"), format!("@echo off\r\nrem Needs Java {java_major}.\r\ncd /d \"%~dp0\"\r\n{cmd}\r\npause\r\n")).await?;
            "start.sh (Linux, macOS) or start.bat (Windows)".to_string()
        }
        None => "run.sh (Linux, macOS) or run.bat (Windows)".to_string(),
    };
    tokio::fs::write(
        dest.join("eula.txt"),
        format!(
            "# Minecraft's EULA: https://aka.ms/MinecraftEULA\n# {}\neula={accept_eula}\n",
            if accept_eula { "Accepted in Alloy when this server was made." } else { "Change this to true to accept it; the server won't start before that." }
        ),
    )
    .await?;
    step(progress, Stage::Done, 4, 4, "Done");
    Ok(Report { dir: dest.to_path_buf(), mods: copied, left_out, start, java_major, eula_accepted: accept_eula })
}

/// Whether a mod only runs on a client.
fn client_only(m: &scan::Scanned, server_unsupported: &std::collections::HashSet<String>) -> bool {
    if let Some(id) = m.index.as_ref().and_then(|i| i.project_id.as_ref()) {
        if server_unsupported.contains(id) {
            return true;
        }
    }
    let Some(jar) = &m.jar else { return false };
    let Some(first) = jar.mods.first() else { return false };
    first.environment.as_deref() == Some("client") || CLIENT_ONLY_IDS.contains(&first.id.to_lowercase().as_str())
}

async fn java_for(launcher: &Launcher, vanilla: &crate::meta::version::VersionJson, given: Option<PathBuf>) -> Result<PathBuf> {
    match given {
        Some(j) => Ok(j),
        None => crate::java::ensure(launcher, &vanilla.java_component(), vanilla.java_major(), &crate::progress::noop()).await,
    }
}

async fn run_java(java: &Path, args: &[String], cwd: &Path) -> Result<()> {
    let mut cmd = tokio::process::Command::new(java);
    cmd.args(args).current_dir(cwd).stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        cmd.creation_flags(0x0800_0000); // no console window
    }
    let out = cmd.output().await?;
    if !out.status.success() {
        let tail: String = String::from_utf8_lossy(&out.stdout).lines().chain(String::from_utf8_lossy(&out.stderr).lines()).rev().take(6).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
        return Err(Error::other(format!("The installer stopped:\n{tail}")));
    }
    Ok(())
}

/// Legacy Forge leaves `forge-<mc>-<version>.jar` (or `...-universal.jar`) to start.
async fn find_forge_jar(dir: &Path) -> Option<String> {
    let mut rd = tokio::fs::read_dir(dir).await.ok()?;
    let mut best = None;
    while let Ok(Some(e)) = rd.next_entry().await {
        let n = e.file_name().to_string_lossy().to_string();
        if n.starts_with("forge-") && n.ends_with(".jar") && !n.contains("installer") {
            best = Some(n);
        }
    }
    best
}

fn between<'a>(s: &'a str, a: &str, b: &str) -> Option<&'a str> {
    let start = s.find(a)? + a.len();
    let end = s[start..].find(b)? + start;
    Some(s[start..end].trim())
}

async fn write_script(path: &Path, text: &str) -> Result<()> {
    tokio::fs::write(path, text).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).await?;
    }
    Ok(())
}

async fn copy_dir(from: PathBuf, to: PathBuf) -> Result<()> {
    tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        for entry in walk(&from) {
            let rel = entry.strip_prefix(&from).unwrap_or(&entry);
            let target = to.join(rel);
            if let Some(p) = target.parent() {
                std::fs::create_dir_all(p)?;
            }
            std::fs::copy(&entry, &target)?;
        }
        Ok(())
    })
    .await
    .map_err(|e| Error::other(e.to_string()))??;
    Ok(())
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_quilt_release() {
        let xml = "<metadata><versioning><latest>0.9.2</latest><release>0.9.2</release></versioning></metadata>";
        assert_eq!(between(xml, "<release>", "</release>"), Some("0.9.2"));
    }

    #[tokio::test]
    async fn vanilla_server_with_eula_choice() {
        // Network: Mojang's manifest and server jar. Skipped offline.
        if std::env::var_os("ALLOY_NET_TESTS").is_none() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let launcher = Launcher::open(crate::paths::Dirs::new(tmp.path().join("data"))).await.unwrap();
        let inst = crate::instance::create(
            &launcher,
            crate::instance::NewInstance { name: "S".into(), game_version: "1.20.1".into(), loader: Loader::Vanilla, loader_version: None, icon: None, group: None, fabric_api: None },
        )
        .await
        .unwrap();
        std::fs::create_dir_all(inst.dir(&launcher).join("config/x")).unwrap();
        std::fs::write(inst.dir(&launcher).join("config/x/a.toml"), "a = 1").unwrap();
        let dest = tmp.path().join("server");
        let r = build(&launcher, &inst, &dest, false, None, &crate::progress::noop()).await.unwrap();
        assert!(dest.join("server.jar").metadata().unwrap().len() > 1_000_000);
        assert!(std::fs::read_to_string(dest.join("eula.txt")).unwrap().contains("eula=false"));
        assert_eq!(std::fs::read_to_string(dest.join("config/x/a.toml")).unwrap(), "a = 1");
        assert!(std::fs::read_to_string(dest.join("start.sh")).unwrap().contains("-jar server.jar nogui"));
        assert_eq!(r.java_major, 17);
        assert!(build(&launcher, &inst, &dest, false, None, &crate::progress::noop()).await.is_err(), "not into a full folder");
    }
}
