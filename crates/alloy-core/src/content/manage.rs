//! Changing what's in an instance: installing with dependencies, updating, fixing what the
//! pre-launch check finds, and cleaning up. Every change goes through the same rules:
//!
//! - Dependencies come from the platform *and* from the jar itself; anything already
//!   present (by project or by mod id, including copies bundled in other jars) is reused.
//! - Libraries pulled in for another mod are marked, so they can be cleaned up once nothing
//!   needs them.
//! - Updates are staged first and checked as a whole: an update that would break another
//!   mod (a library moving past what its dependents accept) is held back, with the reason.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::check::{self, ProblemKind, Report, Severity};
use super::jarmeta::{self, Format};
use super::scan::{self, display_name, Scanned};
use super::{curseforge, identify, load_index, modrinth, save_index, ContentKind, ContentMeta, Source};
use crate::download::{download_with_retry, DownloadTask};
use crate::instance::Instance;
use crate::meta::loaders::Loader;
use crate::{Error, Launcher, Result};

/// A CurseForge file the author won't let launchers download. Alloy watches the Downloads
/// folder for it once the user fetches it from the page.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BlockedFile {
    pub title: String,
    pub file_name: String,
    /// The page with the download button.
    pub page_url: String,
    pub sha1: Option<String>,
    pub size: u64,
    pub mod_id: u64,
    pub file_id: u64,
    /// Folder inside the instance (`mods`, `resourcepacks`, ...).
    pub folder: String,
    pub icon_url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallOutcome {
    /// Titles of everything installed, the requested project first.
    pub installed: Vec<String>,
    /// Required dependencies no platform had a compatible version of.
    pub unresolved: Vec<String>,
    pub blocked: Vec<BlockedFile>,
}

fn loaders_for(instance: &Instance, kind: ContentKind) -> Vec<&'static str> {
    match kind {
        ContentKind::Mod | ContentKind::Modpack => modrinth::compatible_loaders(instance.loader),
        _ => vec![],
    }
}

/// Picks the newest release, else the newest of any kind (the list is newest first).
fn pick(versions: Vec<modrinth::Version>) -> Option<modrinth::Version> {
    let release = versions.iter().position(|v| v.version_type == "release");
    match release {
        Some(i) => versions.into_iter().nth(i),
        None => versions.into_iter().next(),
    }
}

/// What's present: project ids per platform and every mod id a jar provides.
struct Present {
    projects: HashSet<String>,
    mod_ids: HashSet<String>,
}

async fn present(launcher: &Launcher, instance: &Instance) -> Result<(Vec<Scanned>, Present)> {
    let files = scan::scan(launcher, instance).await?;
    let _ = identify::identify(launcher, instance, &files).await;
    let files = scan::scan(launcher, instance).await?;
    let mut projects = HashSet::new();
    let mut mod_ids = HashSet::new();
    for f in &files {
        if let Some(id) = f.index.as_ref().and_then(|m| m.project_id.clone()) {
            projects.insert(id);
        }
        if let Some(jar) = &f.jar {
            for (id, _) in jar.provided() {
                mod_ids.insert(id.to_string());
            }
        }
    }
    Ok((files, Present { projects, mod_ids }))
}

// ---------- Modrinth ----------

/// Installs a Modrinth version and everything it needs.
pub async fn install_modrinth(launcher: &Launcher, instance: &Instance, version_id: &str) -> Result<InstallOutcome> {
    let (_, mut have) = present(launcher, instance).await?;
    let mut out = InstallOutcome::default();
    let root = modrinth::version(launcher, version_id).await?;
    let mut queue: Vec<(modrinth::Version, bool)> = vec![(root, false)];
    let games = [instance.game_version.as_str()];

    while let Some((v, auto)) = queue.pop() {
        if auto && have.projects.contains(&v.project_id) {
            continue;
        }
        have.projects.insert(v.project_id.clone());
        let project = modrinth::project(launcher, &v.project_id).await?;
        let kind = ContentKind::from_modrinth(&project.project_type);
        let loaders = loaders_for(instance, kind);
        for dep in v.dependencies.iter().filter(|d| d.dependency_type == "required") {
            if dep.project_id.as_ref().is_some_and(|p| have.projects.contains(p)) {
                continue;
            }
            let found = match (&dep.version_id, &dep.project_id) {
                (Some(vid), _) => modrinth::version(launcher, vid).await.ok(),
                (None, Some(pid)) => modrinth::project_versions(launcher, pid, &loaders, &games).await.ok().and_then(pick),
                _ => None,
            };
            match found {
                Some(dv) if !have.projects.contains(&dv.project_id) => queue.push((dv, true)),
                Some(_) => {}
                None => out.unresolved.push(dep.project_id.clone().unwrap_or_default()),
            }
        }
        place_modrinth(launcher, instance, &project, &v, auto).await?;
        out.installed.push(project.title);
    }

    // Dependencies only the jar itself declares (platform metadata is often incomplete).
    let follow_up = complete_from_jars(launcher, instance, &mut have).await?;
    out.installed.extend(follow_up.installed);
    out.unresolved.extend(follow_up.unresolved);
    out.unresolved.sort();
    out.unresolved.dedup();
    out.unresolved.retain(|u| !u.is_empty());
    Ok(out)
}

async fn place_modrinth(
    launcher: &Launcher,
    instance: &Instance,
    project: &modrinth::Project,
    v: &modrinth::Version,
    auto: bool,
) -> Result<()> {
    let file = v.primary_file().ok_or_else(|| Error::NotFound(format!("file for {}", v.name)))?;
    let kind = ContentKind::from_modrinth(&project.project_type);
    if kind == ContentKind::Modpack {
        return Err(Error::other("Modpacks install as a new instance, not into one"));
    }
    place(
        launcher,
        instance,
        Placement {
            url: file.url.clone(),
            file_name: file.filename.clone(),
            kind,
            sha1: file.hashes.get("sha1").cloned(),
            size: Some(file.size),
        },
        ContentMeta {
            source: Source::Modrinth,
            project_id: Some(project.id.clone()),
            version_id: Some(v.id.clone()),
            title: Some(project.title.clone()),
            version_name: Some(v.version_number.clone()),
            icon_url: project.icon_url.clone(),
            sha1: file.hashes.get("sha1").cloned(),
            auto,
            requires: Some(v.required_projects()),
        },
    )
    .await
}

struct Placement {
    url: String,
    file_name: String,
    kind: ContentKind,
    sha1: Option<String>,
    size: Option<u64>,
}

/// Downloads a file into the instance, replacing an older file of the same project and
/// keeping its enabled/disabled state.
async fn place(launcher: &Launcher, instance: &Instance, p: Placement, meta: ContentMeta) -> Result<()> {
    let rel = format!("{}/{}", p.kind.folder(), p.file_name);
    let dir = instance.dir(launcher);
    let mut index = load_index(launcher, instance).await;
    // Already there in exactly this version: nothing to download.
    let same = index.files.get(&rel).is_some_and(|m| m.version_id.is_some() && m.version_id == meta.version_id && m.source == meta.source);
    if same && (dir.join(&rel).exists() || dir.join(format!("{rel}.disabled")).exists()) {
        return Ok(());
    }
    let mut keep_disabled = false;
    let mut replaced = vec![];
    for (old_path, old) in &index.files {
        if old.project_id.is_some() && old.project_id == meta.project_id && old.source == meta.source && *old_path != rel {
            replaced.push(old_path.clone());
            keep_disabled |= crate::util::safe_join(&dir, &format!("{old_path}.disabled")).is_ok_and(|p| p.exists());
        }
    }
    let dest = crate::util::safe_join(&dir, &rel)?;
    let staging = dir.join(".launcher").join("staging").join(&p.file_name);
    download_with_retry(&launcher.http, &DownloadTask::new(&p.url, &staging).sha1(p.sha1.clone()).size(p.size)).await?;
    for old in &replaced {
        let _ = super::remove(launcher, instance, old).await;
        index.files.remove(old);
    }
    let final_dest = if keep_disabled { dest.with_file_name(format!("{}.disabled", p.file_name)) } else { dest };
    if let Some(parent) = final_dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::rename(&staging, &final_dest).await?;
    // Re-read: `remove` above saved its own copy of the index.
    let mut index = load_index(launcher, instance).await;
    // A replaced file's "auto" mark carries over; a direct install clears it.
    index.files.insert(rel, meta);
    save_index(launcher, instance, &index).await
}

/// Reads the jars, and installs required dependencies they declare that nothing provides.
async fn complete_from_jars(launcher: &Launcher, instance: &Instance, have: &mut Present) -> Result<InstallOutcome> {
    let mut out = InstallOutcome::default();
    // A few rounds: a dependency may have dependencies of its own.
    for _ in 0..4 {
        let files = scan::scan(launcher, instance).await?;
        let report = check::check(instance, &files, None);
        let missing: Vec<String> = report
            .problems
            .iter()
            .filter_map(|p| match &p.kind {
                ProblemKind::Missing { dep_id, .. } if !have.mod_ids.contains(dep_id) => Some(dep_id.clone()),
                _ => None,
            })
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        if missing.is_empty() {
            break;
        }
        let mut progressed = false;
        for id in missing {
            have.mod_ids.insert(id.clone());
            match find_mod(launcher, instance, &id).await {
                Some(Found::Modrinth(project, version)) if !have.projects.contains(&project.id) => {
                    have.projects.insert(project.id.clone());
                    place_modrinth(launcher, instance, &project, &version, true).await?;
                    out.installed.push(project.title);
                    progressed = true;
                }
                Some(Found::CurseForge(m, f)) if !have.projects.contains(&m.id.to_string()) => {
                    have.projects.insert(m.id.to_string());
                    match place_curseforge(launcher, instance, &m, &f, true).await? {
                        Some(b) => out.blocked.push(b),
                        None => {
                            out.installed.push(m.name.clone());
                            progressed = true;
                        }
                    }
                }
                Some(_) => {}
                None => out.unresolved.push(check::pretty_id(&id)),
            }
        }
        if !progressed {
            break;
        }
    }
    Ok(out)
}

// ---------- finding a mod by its id ----------

pub enum Found {
    Modrinth(Box<modrinth::Project>, Box<modrinth::Version>),
    CurseForge(Box<curseforge::CfMod>, Box<curseforge::CfFile>),
}

/// Mod ids whose Modrinth/CurseForge slug differs from the id.
fn slug_aliases(id: &str) -> Vec<String> {
    let known: &[(&str, &str)] = &[
        ("fabric", "fabric-api"),
        ("architectury", "architectury-api"),
        ("cloth_config", "cloth-config"),
        ("owo", "owo-lib"),
        ("yet_another_config_lib_v3", "yacl"),
        ("yet-another-config-lib", "yacl"),
        ("forgeconfigapiport", "forge-config-api-port"),
        ("puzzleslib", "puzzles-lib"),
        ("kotlinforforge", "kotlin-for-forge"),
        ("fabric_language_kotlin", "fabric-language-kotlin"),
        ("connectormod", "connector"),
        ("ftbquests", "ftb-quests-forge"),
        ("ftblibrary", "ftb-library-forge"),
        ("ftbteams", "ftb-teams-forge"),
        ("twilightforest", "the-twilight-forest"),
        ("geckolib", "geckolib"),
        ("cardinal-components", "cardinal-components-api"),
        ("playeranimator", "playeranimator"),
        ("resourcefullib", "resourceful-lib"),
        ("bookshelf", "bookshelf-lib"),
        ("citadel", "citadel"),
        ("terrablender", "terrablender"),
        ("moonlight", "moonlight"),
        ("supermartijn642corelib", "supermartijn642s-core-lib"),
        ("supermartijn642configlib", "supermartijn642s-config-lib"),
        ("balm", "balm"),
        ("jei", "jei"),
        ("curios", "curios"),
        ("trinkets", "trinkets"),
        ("accessories", "accessories"),
        ("iceberg", "iceberg"),
        ("prism", "prism-lib"),
        ("temporalapi", "temporal-api"),
        ("flib", "flib"),
        ("reeses-sodium-options", "reeses-sodium-options"),
    ];
    let mut out = vec![];
    for (k, v) in known {
        if *k == id {
            out.push(v.to_string());
        }
    }
    out.push(id.to_string());
    out.push(id.replace('_', "-"));
    out.dedup();
    out
}

fn normalize(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}

/// Finds a compatible version of the mod with this id: Modrinth first (by slug, then by
/// search), CurseForge second when a key is set.
pub async fn find_mod(launcher: &Launcher, instance: &Instance, mod_id: &str) -> Option<Found> {
    let loaders = modrinth::compatible_loaders(instance.loader);
    let games = [instance.game_version.as_str()];
    let slugs = slug_aliases(mod_id);
    for slug in &slugs {
        if let Ok(project) = modrinth::project(launcher, slug).await {
            if project.project_type != "mod" {
                continue;
            }
            if let Some(v) = modrinth::project_versions(launcher, &project.id, &loaders, &games).await.ok().and_then(pick) {
                return Some(Found::Modrinth(Box::new(project), Box::new(v)));
            }
        }
    }
    // Search, accepting only a hit whose slug or title is the id.
    let q = super::SearchQuery {
        query: mod_id.replace(['_', '-'], " "),
        kind: Some(ContentKind::Mod),
        game_version: Some(instance.game_version.clone()),
        loader: instance.loader.modrinth_name().map(str::to_string),
        limit: 10,
        ..Default::default()
    };
    let want = normalize(mod_id);
    if let Ok(page) = modrinth::search(launcher, &q).await {
        if let Some(hit) = page.hits.iter().find(|h| normalize(&h.slug) == want || normalize(&h.title) == want) {
            if let (Ok(project), Some(v)) = (
                modrinth::project(launcher, &hit.id).await,
                modrinth::project_versions(launcher, &hit.id, &loaders, &games).await.ok().and_then(pick),
            ) {
                return Some(Found::Modrinth(Box::new(project), Box::new(v)));
            }
        }
    }
    if curseforge::has_key(launcher).await {
        for slug in &slugs {
            if let Ok(Some(m)) = curseforge::mod_by_slug(launcher, slug, Some(ContentKind::Mod)).await {
                if let Some(f) = cf_pick(launcher, &m, instance).await {
                    return Some(Found::CurseForge(Box::new(m), Box::new(f)));
                }
            }
        }
        if let Ok(page) = curseforge::search(launcher, &q).await {
            if let Some(hit) = page.hits.iter().find(|h| normalize(&h.slug) == want || normalize(&h.title) == want) {
                if let Ok(m) = curseforge::get_mod(launcher, hit.id.parse().unwrap_or(0)).await {
                    if let Some(f) = cf_pick(launcher, &m, instance).await {
                        return Some(Found::CurseForge(Box::new(m), Box::new(f)));
                    }
                }
            }
        }
    }
    None
}

/// The newest CurseForge file for the instance, preferring releases.
async fn cf_pick(launcher: &Launcher, m: &curseforge::CfMod, instance: &Instance) -> Option<curseforge::CfFile> {
    let loader =
        (curseforge::class_id(ContentKind::Mod) == m.class_id.unwrap_or(6)).then_some(instance.loader).filter(|l| *l != Loader::Vanilla);
    let files = curseforge::mod_files(launcher, m.id, Some(&instance.game_version), loader).await.ok()?;
    let release = files.iter().position(|f| f.release_type == 1);
    match release {
        Some(i) => files.into_iter().nth(i),
        None => files.into_iter().next(),
    }
}

// ---------- CurseForge ----------

/// Installs a CurseForge file and its required dependencies.
pub async fn install_curseforge(launcher: &Launcher, instance: &Instance, mod_id: u64, file_id: u64) -> Result<InstallOutcome> {
    let (_, mut have) = present(launcher, instance).await?;
    let mut out = InstallOutcome::default();
    let root_mod = curseforge::get_mod(launcher, mod_id).await?;
    let root_file = curseforge::file(launcher, mod_id, file_id).await?;
    let mut queue = vec![(root_mod, root_file, false)];
    while let Some((m, f, auto)) = queue.pop() {
        if auto && have.projects.contains(&m.id.to_string()) {
            continue;
        }
        have.projects.insert(m.id.to_string());
        for dep in f.dependencies.iter().filter(|d| d.relation_type == 3) {
            if have.projects.contains(&dep.mod_id.to_string()) {
                continue;
            }
            match curseforge::get_mod(launcher, dep.mod_id).await {
                Ok(dm) => match cf_pick(launcher, &dm, instance).await {
                    Some(df) => queue.push((dm, df, true)),
                    None => out.unresolved.push(dm.name),
                },
                Err(_) => out.unresolved.push(dep.mod_id.to_string()),
            }
        }
        match place_curseforge(launcher, instance, &m, &f, auto).await? {
            Some(b) => out.blocked.push(b),
            None => out.installed.push(m.name.clone()),
        }
    }
    let follow_up = complete_from_jars(launcher, instance, &mut have).await?;
    out.installed.extend(follow_up.installed);
    out.unresolved.extend(follow_up.unresolved);
    out.blocked.extend(follow_up.blocked);
    Ok(out)
}

fn cf_kind(m: &curseforge::CfMod) -> ContentKind {
    match m.class_id {
        Some(12) => ContentKind::ResourcePack,
        Some(6552) => ContentKind::Shader,
        Some(6945) => ContentKind::DataPack,
        _ => ContentKind::Mod,
    }
}

/// Places a CurseForge file, or returns it as blocked when the author forbids launcher downloads.
async fn place_curseforge(
    launcher: &Launcher,
    instance: &Instance,
    m: &curseforge::CfMod,
    f: &curseforge::CfFile,
    auto: bool,
) -> Result<Option<BlockedFile>> {
    let kind = cf_kind(m);
    let sha1 = f.hashes.iter().find(|h| h.algo == 1).map(|h| h.value.clone());
    let icon = m.logo.as_ref().map(|l| l.thumbnail_url.clone().unwrap_or(l.url.clone()));
    let Some(url) = f.download_url.clone() else {
        return Ok(Some(BlockedFile {
            title: m.name.clone(),
            file_name: f.file_name.clone(),
            page_url: curseforge::manual_download_page(m, f.id),
            sha1,
            size: f.file_length,
            mod_id: m.id,
            file_id: f.id,
            folder: kind.folder().into(),
            icon_url: icon,
        }));
    };
    place(
        launcher,
        instance,
        Placement { url, file_name: f.file_name.clone(), kind, sha1: sha1.clone(), size: Some(f.file_length).filter(|s| *s > 0) },
        ContentMeta {
            source: Source::CurseForge,
            project_id: Some(m.id.to_string()),
            version_id: Some(f.id.to_string()),
            title: Some(m.name.clone()),
            version_name: Some(f.display_name.clone()),
            icon_url: icon,
            sha1,
            auto,
            requires: Some(f.required_mods()),
        },
    )
    .await?;
    Ok(None)
}

/// Copies a file the user downloaded by hand into the instance, after checking it's the right one.
pub async fn accept_blocked(launcher: &Launcher, instance: &Instance, blocked: &BlockedFile, downloaded: &std::path::Path) -> Result<()> {
    if let Some(expected) = &blocked.sha1 {
        let actual = crate::util::sha1_file(downloaded).await?;
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(Error::HashMismatch { path: downloaded.to_path_buf(), expected: expected.clone(), actual });
        }
    }
    let rel = format!("{}/{}", blocked.folder, blocked.file_name);
    let dest = crate::util::safe_join(&instance.dir(launcher), &rel)?;
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::copy(downloaded, &dest).await?;
    super::record(
        launcher,
        instance,
        rel,
        ContentMeta {
            source: Source::CurseForge,
            project_id: Some(blocked.mod_id.to_string()),
            version_id: Some(blocked.file_id.to_string()),
            title: Some(blocked.title.clone()),
            version_name: None,
            icon_url: blocked.icon_url.clone(),
            sha1: blocked.sha1.clone(),
            auto: false,
            requires: None,
        },
    )
    .await
}

// ---------- updates ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Update {
    pub path: String,
    pub title: String,
    pub icon_url: Option<String>,
    pub source: Source,
    pub project_id: String,
    pub current: Option<String>,
    pub version_id: String,
    pub version_name: String,
    pub file_name: String,
    /// Empty when CurseForge blocks launcher downloads for this file.
    pub url: String,
    pub sha1: Option<String>,
    pub size: u64,
    pub date: Option<String>,
    pub prerelease: bool,
    pub kind: ContentKind,
    #[serde(default)]
    pub requires: Vec<String>,
}

/// Newer compatible versions for everything installed from (or identified on) a platform.
/// Stays on the release channel unless the installed file is itself a beta or alpha.
pub async fn check_updates(launcher: &Launcher, instance: &Instance) -> Result<Vec<Update>> {
    let (files, _) = present(launcher, instance).await?;
    let mut out = vec![];
    let games = [instance.game_version.clone()];

    // Modrinth, one bulk call per content kind (they use different loader filters).
    let mr: Vec<&Scanned> = files.iter().filter(|f| f.index.as_ref().is_some_and(|m| m.source == Source::Modrinth)).collect();
    let current_ids: Vec<String> = mr.iter().filter_map(|f| f.index.as_ref().and_then(|m| m.version_id.clone())).collect();
    let current: HashMap<String, modrinth::Version> =
        modrinth::versions(launcher, &current_ids).await.unwrap_or_default().into_iter().map(|v| (v.id.clone(), v)).collect();
    for kind in [ContentKind::Mod, ContentKind::ResourcePack, ContentKind::Shader] {
        let group: Vec<&&Scanned> = mr.iter().filter(|f| f.kind == kind).collect();
        if group.is_empty() {
            continue;
        }
        let sha1s: Vec<String> = group.iter().map(|f| f.sha1.clone()).collect();
        let loaders = loaders_for(instance, kind);
        let game_refs: Vec<&str> = games.iter().map(String::as_str).collect();
        let Ok(newest) = modrinth::updates_from_hashes(launcher, &sha1s, &loaders, &game_refs).await else {
            continue;
        };
        for f in group {
            let Some(meta) = &f.index else { continue };
            let Some(mut new) = newest.get(&f.sha1).cloned() else {
                continue;
            };
            let cur = meta.version_id.as_ref().and_then(|id| current.get(id));
            if Some(&new.id) == meta.version_id.as_ref() {
                continue;
            }
            let on_release = cur.is_none_or(|c| c.version_type == "release");
            if on_release && new.version_type != "release" {
                // Look for the newest release instead.
                let Some(pid) = &meta.project_id else {
                    continue;
                };
                let Some(rel) = modrinth::project_versions(launcher, pid, &loaders, &game_refs)
                    .await
                    .ok()
                    .and_then(|vs| vs.into_iter().find(|v| v.version_type == "release"))
                else {
                    continue;
                };
                if Some(&rel.id) == meta.version_id.as_ref() {
                    continue;
                }
                new = rel;
            }
            // Never "update" to something older than what's there.
            if let Some(c) = cur {
                if new.date_published <= c.date_published {
                    continue;
                }
            }
            let Some(file) = new.primary_file() else {
                continue;
            };
            out.push(Update {
                path: f.path.clone(),
                title: display_name(f),
                icon_url: meta.icon_url.clone(),
                source: Source::Modrinth,
                project_id: new.project_id.clone(),
                current: meta.version_name.clone(),
                version_id: new.id.clone(),
                version_name: new.version_number.clone(),
                file_name: file.filename.clone(),
                url: file.url.clone(),
                sha1: file.hashes.get("sha1").cloned(),
                size: file.size,
                date: Some(new.date_published.clone()),
                prerelease: new.version_type != "release",
                kind,
                requires: new.required_projects(),
            });
        }
    }

    // CurseForge, from each mod's latest-file index.
    if curseforge::has_key(launcher).await {
        let cf: Vec<&Scanned> = files.iter().filter(|f| f.index.as_ref().is_some_and(|m| m.source == Source::CurseForge)).collect();
        let ids: Vec<u64> = cf.iter().filter_map(|f| f.index.as_ref()?.project_id.as_ref()?.parse().ok()).collect();
        let current_files: Vec<u64> = cf.iter().filter_map(|f| f.index.as_ref()?.version_id.as_ref()?.parse().ok()).collect();
        let mods: HashMap<u64, curseforge::CfMod> =
            curseforge::mods(launcher, &ids).await.unwrap_or_default().into_iter().map(|m| (m.id, m)).collect();
        let cur_files: HashMap<u64, curseforge::CfFile> =
            curseforge::files(launcher, &current_files).await.unwrap_or_default().into_iter().map(|f| (f.id, f)).collect();
        let loader_type = curseforge::loader_type(instance.loader);
        let mut wanted: Vec<(&Scanned, u64)> = vec![];
        for f in &cf {
            let Some(meta) = &f.index else { continue };
            let (Some(mid), Some(fid)) = (
                meta.project_id.as_ref().and_then(|p| p.parse::<u64>().ok()),
                meta.version_id.as_ref().and_then(|v| v.parse::<u64>().ok()),
            ) else {
                continue;
            };
            let Some(m) = mods.get(&mid) else { continue };
            let channel = cur_files.get(&fid).map(|c| c.release_type).unwrap_or(1);
            let best = m
                .latest_files_indexes
                .iter()
                .filter(|i| i.game_version == instance.game_version)
                .filter(|i| f.kind != ContentKind::Mod || i.mod_loader.is_none() || i.mod_loader == loader_type)
                .filter(|i| i.release_type <= channel.max(1))
                .max_by_key(|i| i.file_id);
            if let Some(b) = best {
                if b.file_id > fid {
                    wanted.push((f, b.file_id));
                }
            }
        }
        let new_files: HashMap<u64, curseforge::CfFile> =
            curseforge::files(launcher, &wanted.iter().map(|(_, id)| *id).collect::<Vec<_>>())
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|f| (f.id, f))
                .collect();
        for (f, id) in wanted {
            let Some(nf) = new_files.get(&id) else {
                continue;
            };
            let meta = f.index.as_ref().unwrap();
            out.push(Update {
                path: f.path.clone(),
                title: display_name(f),
                icon_url: meta.icon_url.clone(),
                source: Source::CurseForge,
                project_id: nf.mod_id.to_string(),
                current: meta.version_name.clone(),
                version_id: nf.id.to_string(),
                version_name: nf.display_name.clone(),
                file_name: nf.file_name.clone(),
                url: nf.download_url.clone().unwrap_or_default(),
                sha1: nf.hashes.iter().find(|h| h.algo == 1).map(|h| h.value.clone()),
                size: nf.file_length,
                date: nf.file_date.clone(),
                prerelease: nf.release_type != 1,
                kind: f.kind,
                requires: nf.required_mods(),
            });
        }
    }
    out.sort_by_key(|a| a.title.to_lowercase());
    Ok(out)
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateResult {
    pub updated: Vec<String>,
    /// Title and the reason it was held back.
    pub held_back: Vec<(String, String)>,
    pub failed: Vec<(String, String)>,
}

/// Downloads every update to a staging folder, checks the instance as it would be with all
/// of them, holds back any update that would cause a new problem, and moves the rest in.
pub async fn apply_updates(launcher: &Launcher, instance: &Instance, updates: Vec<Update>) -> Result<UpdateResult> {
    let mut result = UpdateResult::default();
    let dir = instance.dir(launcher);
    let staging = dir.join(".launcher").join("staging");
    tokio::fs::create_dir_all(&staging).await?;

    // 1. Download.
    let concurrency = launcher.settings.read().await.max_concurrent_downloads.min(8);
    let staged: Vec<(Update, Result<PathBuf>)> = futures::StreamExt::collect(futures::StreamExt::buffer_unordered(
        futures::stream::iter(updates.into_iter().map(|u| {
            let staging = staging.clone();
            async move {
                if u.url.is_empty() {
                    return (u, Err(Error::other("CurseForge only allows this download on its website")));
                }
                let path = staging.join(&u.file_name);
                let r = download_with_retry(
                    &launcher.http,
                    &DownloadTask::new(&u.url, &path).sha1(u.sha1.clone()).size(Some(u.size).filter(|s| *s > 0)),
                )
                .await
                .map(|_| path);
                (u, r)
            }
        })),
        concurrency.max(1),
    ))
    .await;

    let mut ready: Vec<(Update, PathBuf, Option<jarmeta::JarMeta>)> = vec![];
    for (u, r) in staged {
        match r {
            Ok(p) => {
                let p2 = p.clone();
                let meta = if u.kind == ContentKind::Mod {
                    tokio::task::spawn_blocking(move || jarmeta::read_path(&p2).ok()).await.ok().flatten()
                } else {
                    None
                };
                ready.push((u, p, meta));
            }
            Err(e) => result.failed.push((u.title.clone(), e.to_string())),
        }
    }

    // 2. Simulate: the instance with every staged update in place.
    let files = scan::scan(launcher, instance).await?;
    let baseline: HashSet<String> = error_keys(&check::check(instance, &files, None));
    let mut tried_gentler: HashSet<String> = HashSet::new();
    loop {
        let simulated: Vec<Scanned> = files
            .iter()
            .map(|f| match ready.iter().find(|(u, _, _)| u.path == f.path) {
                Some((u, _, meta)) => {
                    Scanned { path: f.path.clone(), file_name: u.file_name.clone(), jar: meta.clone().or(f.jar.clone()), ..f.clone() }
                }
                None => f.clone(),
            })
            .collect();
        let report = check::check(instance, &simulated, None);
        let new_problems: Vec<&check::Problem> =
            report.problems.iter().filter(|p| p.severity == Severity::Error && !baseline.contains(&error_key(p))).collect();
        if new_problems.is_empty() {
            break;
        }
        // First, move a library only as far as the mods that need it allow.
        let mut softened = false;
        for p in &new_problems {
            let ProblemKind::WrongVersion { dep_id, dep_path: Some(d), .. } = &p.kind else {
                continue;
            };
            if !tried_gentler.insert(d.clone()) {
                continue;
            }
            let Some(pos) = ready.iter().position(|(u, _, _)| &u.path == d) else {
                continue;
            };
            let range = simulated
                .iter()
                .find(|f| f.path == p.path)
                .and_then(|f| f.jar.as_ref())
                .and_then(|j| j.mods.iter().chain(j.nested.iter()).flat_map(|m| m.deps.iter()).find(|dep| dep.id == *dep_id))
                .map(|dep| dep.parsed());
            let installed = files.iter().find(|f| &f.path == d).and_then(|f| f.index.as_ref()).and_then(|m| m.version_id.clone());
            let Some(range) = range else { continue };
            if let Some(alt) = gentler(launcher, instance, &ready[pos].0, installed.as_deref(), dep_id, &range, &staging).await {
                let _ = std::fs::remove_file(&ready[pos].1);
                ready[pos] = alt;
                softened = true;
            }
        }
        if softened {
            continue;
        }
        // Hold back the updates involved, then try again with the rest.
        let mut culprits: HashSet<String> = HashSet::new();
        for p in &new_problems {
            let mut involved = vec![p.path.clone()];
            match &p.kind {
                ProblemKind::WrongVersion { dep_path: Some(d), .. } => involved.push(d.clone()),
                ProblemKind::Breaks { other_path: Some(o), .. } => involved.push(o.clone()),
                ProblemKind::Duplicate { other_path, .. } => involved.push(other_path.clone()),
                _ => {}
            }
            for path in involved {
                if ready.iter().any(|(u, _, _)| u.path == path) {
                    culprits.insert(path.clone());
                    let title = ready.iter().find(|(u, _, _)| u.path == path).map(|(u, _, _)| u.title.clone()).unwrap_or_default();
                    if !result.held_back.iter().any(|(t, _)| *t == title) {
                        result.held_back.push((title, p.message.clone()));
                    }
                }
            }
        }
        if culprits.is_empty() {
            break;
        }
        ready.retain(|(u, path, _)| {
            let keep = !culprits.contains(&u.path);
            if !keep {
                let _ = std::fs::remove_file(path);
            }
            keep
        });
    }

    // 3. Move the survivors in.
    let mut index = load_index(launcher, instance).await;
    for (u, staged_path, _) in ready {
        let old_full = crate::util::safe_join(&dir, &u.path)?;
        let old_disabled = dir.join(format!("{}.disabled", u.path));
        let was_disabled = old_disabled.exists();
        let rel = format!("{}/{}", u.kind.folder(), u.file_name);
        let dest = crate::util::safe_join(&dir, &rel)?;
        let dest = if was_disabled { dest.with_file_name(format!("{}.disabled", u.file_name)) } else { dest };
        let _ = tokio::fs::remove_file(&old_full).await;
        let _ = tokio::fs::remove_file(&old_disabled).await;
        tokio::fs::rename(&staged_path, &dest).await?;
        let old = index.files.remove(&u.path);
        index.files.insert(
            rel,
            ContentMeta {
                source: u.source,
                project_id: Some(u.project_id.clone()),
                version_id: Some(u.version_id.clone()),
                title: old.as_ref().and_then(|o| o.title.clone()).or(Some(u.title.clone())),
                version_name: Some(u.version_name.clone()),
                icon_url: u.icon_url.clone(),
                sha1: u.sha1.clone(),
                auto: old.as_ref().is_some_and(|o| o.auto),
                requires: Some(u.requires.clone()),
            },
        );
        result.updated.push(u.title);
    }
    save_index(launcher, instance, &index).await?;
    Ok(result)
}

/// The newest version of `u`'s project, newer than what's installed, whose jar provides
/// `dep_id` in `range`. Downloads at most a few candidates to read their jars.
async fn gentler(
    launcher: &Launcher,
    instance: &Instance,
    u: &Update,
    installed: Option<&str>,
    dep_id: &str,
    range: &super::version_range::Range,
    staging: &std::path::Path,
) -> Option<(Update, PathBuf, Option<jarmeta::JarMeta>)> {
    if u.source != Source::Modrinth {
        return None;
    }
    let loaders = loaders_for(instance, u.kind);
    let versions = modrinth::project_versions(launcher, &u.project_id, &loaders, &[instance.game_version.as_str()]).await.ok()?;
    let candidates = versions
        .into_iter()
        .take_while(|v| Some(v.id.as_str()) != installed)
        .filter(|v| v.id != u.version_id && (u.prerelease || v.version_type == "release"))
        .take(6);
    for v in candidates {
        let file = v.primary_file()?.clone();
        let path = staging.join(&file.filename);
        let task = DownloadTask::new(&file.url, &path).sha1(file.hashes.get("sha1").cloned()).size(Some(file.size));
        if download_with_retry(&launcher.http, &task).await.is_err() {
            continue;
        }
        let p2 = path.clone();
        let meta = tokio::task::spawn_blocking(move || jarmeta::read_path(&p2).ok()).await.ok().flatten();
        let fits = meta.as_ref().is_some_and(|m| m.provided().any(|(id, ver)| id == dep_id && range.matches(ver)));
        if fits {
            let alt = Update {
                version_id: v.id.clone(),
                version_name: v.version_number.clone(),
                file_name: file.filename.clone(),
                url: file.url.clone(),
                sha1: file.hashes.get("sha1").cloned(),
                size: file.size,
                date: Some(v.date_published.clone()),
                prerelease: v.version_type != "release",
                requires: v.required_projects(),
                ..u.clone()
            };
            return Some((alt, path, meta));
        }
        let _ = tokio::fs::remove_file(&path).await;
    }
    None
}

fn error_key(p: &check::Problem) -> String {
    format!("{}|{}", p.path, p.message)
}

fn error_keys(r: &Report) -> HashSet<String> {
    r.problems.iter().filter(|p| p.severity == Severity::Error).map(error_key).collect()
}

// ---------- fixes ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "action")]
pub enum Fix {
    InstallModrinth { version_id: String, title: String, icon_url: Option<String> },
    InstallCurseForge { mod_id: u64, file_id: u64, title: String, icon_url: Option<String> },
    Remove { path: String, title: String },
    Disable { path: String, title: String },
    SetLoaderVersion { version: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    /// Index into the report's problems.
    pub problem: usize,
    /// The problem's path and message, to match it in any filtered list.
    pub path: String,
    pub message: String,
    pub fix: Fix,
    /// One sentence describing what the fix does.
    pub label: String,
}

/// Turns problems into one-click fixes where Alloy can find one.
pub async fn suggest(launcher: &Launcher, instance: &Instance, report: &Report, files: &[Scanned]) -> Vec<Suggestion> {
    let mut out = vec![];
    let mut found_cache: HashMap<String, Option<Fix>> = HashMap::new();
    let name_of = |path: &str| files.iter().find(|f| f.path == path).map(display_name).unwrap_or_else(|| path.to_string());
    for (i, p) in report.problems.iter().enumerate() {
        let fix = match &p.kind {
            ProblemKind::Missing { dep_id, .. } => {
                if !found_cache.contains_key(dep_id) {
                    let f = match find_mod(launcher, instance, dep_id).await {
                        Some(Found::Modrinth(project, v)) => {
                            Some(Fix::InstallModrinth { version_id: v.id, title: project.title, icon_url: project.icon_url })
                        }
                        Some(Found::CurseForge(m, f)) => Some(Fix::InstallCurseForge {
                            mod_id: m.id,
                            file_id: f.id,
                            icon_url: m.logo.as_ref().map(|l| l.thumbnail_url.clone().unwrap_or(l.url.clone())),
                            title: m.name,
                        }),
                        None => None,
                    };
                    found_cache.insert(dep_id.clone(), f);
                }
                found_cache.get(dep_id).cloned().flatten()
            }
            ProblemKind::Duplicate { other_path, mod_id } => {
                // Keep the newer copy.
                let version = |path: &str| {
                    files
                        .iter()
                        .find(|f| f.path == path)
                        .and_then(|f| f.jar.as_ref())
                        .and_then(|j| j.mods.iter().find(|m| m.id == *mod_id))
                        .map(|m| m.version.clone())
                        .unwrap_or_default()
                };
                let older = if super::version_range::compare(&version(&p.path), &version(other_path)) == std::cmp::Ordering::Less {
                    p.path.clone()
                } else {
                    other_path.clone()
                };
                Some(Fix::Remove { title: format!("{} ({})", name_of(&older), version(&older)), path: older })
            }
            // Fabric mods run on (Neo)Forge with Sinytra Connector; offer that before disabling.
            ProblemKind::WrongLoader { made_for }
                if made_for.contains(&Format::Fabric) && matches!(instance.loader, Loader::Forge | Loader::NeoForge) =>
            {
                let key = "connectormod".to_string();
                if !found_cache.contains_key(&key) {
                    let f = match find_mod(launcher, instance, &key).await {
                        Some(Found::Modrinth(project, v)) => {
                            Some(Fix::InstallModrinth { version_id: v.id, title: project.title, icon_url: project.icon_url })
                        }
                        _ => None,
                    };
                    found_cache.insert(key.clone(), f);
                }
                if let Some(Some(fix)) = found_cache.get(&key) {
                    out.push(Suggestion {
                        problem: i,
                        path: p.path.clone(),
                        message: p.message.clone(),
                        label: "Install Sinytra Connector".into(),
                        fix: fix.clone(),
                    });
                }
                Some(Fix::Disable { path: p.path.clone(), title: p.mod_name.clone() })
            }
            ProblemKind::WrongLoader { .. } | ProblemKind::WrongGame { .. } | ProblemKind::ServerOnly => {
                Some(Fix::Disable { path: p.path.clone(), title: p.mod_name.clone() })
            }
            ProblemKind::Breaks { other_path: Some(o), .. } => Some(Fix::Disable { path: o.clone(), title: name_of(o) }),
            ProblemKind::WrongLoaderVersion { wanted, .. } => {
                let range = files
                    .iter()
                    .find(|f| f.path == p.path)
                    .and_then(|f| f.jar.as_ref())
                    .and_then(|j| {
                        j.mods
                            .iter()
                            .flat_map(|m| m.deps.iter())
                            .find(|d| matches!(d.id.as_str(), "fabricloader" | "quilt_loader" | "forge" | "neoforge"))
                    })
                    .map(|d| d.parsed());
                let _ = wanted;
                let versions = crate::meta::loaders::versions(launcher, instance.loader, &instance.game_version).await.unwrap_or_default();
                versions
                    .iter()
                    .find(|v| v.stable && range.as_ref().is_none_or(|r| r.matches(&v.id)))
                    .or_else(|| versions.iter().find(|v| range.as_ref().is_none_or(|r| r.matches(&v.id))))
                    .map(|v| Fix::SetLoaderVersion { version: v.id.clone() })
            }
            ProblemKind::WrongVersion { dep_id, .. } => {
                // A compatible version of the dependency, if one exists for this game version.
                match find_mod(launcher, instance, dep_id).await {
                    Some(Found::Modrinth(project, v)) => {
                        Some(Fix::InstallModrinth { version_id: v.id, title: project.title, icon_url: project.icon_url })
                    }
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some(fix) = fix {
            let label = match &fix {
                Fix::InstallModrinth { title, .. } | Fix::InstallCurseForge { title, .. } => {
                    format!("Install {title}")
                }
                Fix::Remove { title, .. } => format!("Remove {title}"),
                Fix::Disable { title, .. } => format!("Disable {title}"),
                Fix::SetLoaderVersion { version } => {
                    format!("Switch to {} {version}", loader_label(instance.loader))
                }
            };
            out.push(Suggestion { problem: i, path: p.path.clone(), message: p.message.clone(), fix, label });
        }
    }
    out
}

fn loader_label(l: Loader) -> &'static str {
    match l {
        Loader::Fabric => "Fabric Loader",
        Loader::Quilt => "Quilt Loader",
        Loader::Forge => "Forge",
        Loader::NeoForge => "NeoForge",
        Loader::Vanilla => "Minecraft",
    }
}

/// Applies one fix. Loader changes return the new loader version for the caller to save.
pub async fn apply_fix(launcher: &Launcher, instance: &Instance, fix: &Fix) -> Result<InstallOutcome> {
    match fix {
        Fix::InstallModrinth { version_id, .. } => install_modrinth(launcher, instance, version_id).await,
        Fix::InstallCurseForge { mod_id, file_id, .. } => install_curseforge(launcher, instance, *mod_id, *file_id).await,
        Fix::Remove { path, .. } => {
            super::remove(launcher, instance, path).await?;
            Ok(InstallOutcome::default())
        }
        Fix::Disable { path, .. } => {
            super::set_enabled(launcher, instance, path, false).await?;
            Ok(InstallOutcome::default())
        }
        Fix::SetLoaderVersion { .. } => Ok(InstallOutcome::default()),
    }
}

/// Files a loader would read as mods, for the Formats column.
pub fn formats_label(f: &[Format]) -> String {
    f.iter()
        .map(|f| match f {
            Format::Fabric => "Fabric",
            Format::Quilt => "Quilt",
            Format::Forge | Format::LegacyForge => "Forge",
            Format::NeoForge => "NeoForge",
        })
        .collect::<Vec<_>>()
        .join(", ")
}
