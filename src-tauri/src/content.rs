//! Commands for what's inside instances: search, project pages, installs with dependencies,
//! the pre-launch check and its fixes, updates, links and dropped files, CurseForge's
//! blocked downloads, and instance icons.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use alloy_core::content::check::{self, Problem, Report, Severity};
use alloy_core::content::links::{self, DroppedKind, Link};
use alloy_core::content::manage::{self, BlockedFile, Fix, InstallOutcome, Suggestion, Update, UpdateResult};
use alloy_core::content::{
    self, blocked, curseforge, identify, jarmeta, modrinth, scan, ContentKind, ContentMeta, SearchPage, SearchQuery,
};
use alloy_core::instance::{self, InstallState, Instance, LinkedPack, PackSource};
use alloy_core::modpack::{self, ImportResult};
use alloy_core::{install, system, Error, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::commands::{changed, do_install, progress_emitter};
use crate::jobs::{self, Retry};
use crate::state::AppState;

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Modrinth,
    CurseForge,
}

// ---------- search & project pages ----------

#[tauri::command]
pub async fn search_content(state: State<'_, AppState>, platform: Platform, query: SearchQuery) -> Result<SearchPage> {
    match platform {
        Platform::Modrinth => modrinth::search(&state.launcher, &query).await,
        Platform::CurseForge => curseforge::search(&state.launcher, &query).await,
    }
}

#[tauri::command]
pub async fn modrinth_project(state: State<'_, AppState>, id: String) -> Result<modrinth::Project> {
    modrinth::project(&state.launcher, &id).await
}

/// Versions of a project; with an instance id, only ones that run on that instance.
#[tauri::command]
pub async fn modrinth_versions(
    state: State<'_, AppState>,
    project_id: String,
    instance_id: Option<String>,
) -> Result<Vec<modrinth::Version>> {
    let launcher = &state.launcher;
    match instance_id {
        Some(iid) => {
            let inst = instance::get(launcher, &iid).await?;
            let project = modrinth::project(launcher, &project_id).await?;
            let loaders =
                if matches!(project.project_type.as_str(), "mod" | "modpack") { modrinth::compatible_loaders(inst.loader) } else { vec![] };
            modrinth::project_versions(launcher, &project_id, &loaders, &[inst.game_version.as_str()]).await
        }
        None => modrinth::project_versions(launcher, &project_id, &[], &[]).await,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dependency {
    pub project_id: String,
    pub title: String,
    pub icon_url: Option<String>,
    pub kind: String,
    pub slug: String,
}

/// The projects a Modrinth version depends on, with names and icons, for the project page.
#[tauri::command]
pub async fn modrinth_dependencies(state: State<'_, AppState>, version_id: String) -> Result<Vec<Dependency>> {
    let launcher = &state.launcher;
    let v = modrinth::version(launcher, &version_id).await?;
    let kinds: HashMap<String, String> =
        v.dependencies.iter().filter_map(|d| Some((d.project_id.clone()?, d.dependency_type.clone()))).collect();
    let ids: Vec<String> = kinds.keys().cloned().collect();
    let projects = modrinth::projects(launcher, &ids).await?;
    let mut out: Vec<Dependency> = projects
        .into_iter()
        .map(|p| Dependency {
            kind: kinds.get(&p.id).cloned().unwrap_or_default(),
            project_id: p.id,
            title: p.title,
            icon_url: p.icon_url,
            slug: p.slug,
        })
        .collect();
    out.sort_by_key(|d| (d.kind != "required", d.title.to_lowercase()));
    Ok(out)
}

#[tauri::command]
pub async fn curseforge_project(state: State<'_, AppState>, id: u64) -> Result<curseforge::CfMod> {
    curseforge::get_mod(&state.launcher, id).await
}

#[tauri::command]
pub async fn curseforge_description(state: State<'_, AppState>, id: u64) -> Result<String> {
    curseforge::description(&state.launcher, id).await
}

#[tauri::command]
pub async fn curseforge_files(state: State<'_, AppState>, mod_id: u64, instance_id: Option<String>) -> Result<Vec<curseforge::CfFile>> {
    let launcher = &state.launcher;
    match instance_id {
        Some(iid) => {
            let inst = instance::get(launcher, &iid).await?;
            let m = curseforge::get_mod(launcher, mod_id).await?;
            let loader = matches!(m.class_id, Some(6) | Some(4471) | None).then_some(inst.loader);
            curseforge::mod_files(launcher, mod_id, Some(&inst.game_version), loader).await
        }
        None => curseforge::mod_files(launcher, mod_id, None, None).await,
    }
}

// ---------- installs ----------

fn outcome_toast(app: &AppHandle, instance_id: &str, outcome: &InstallOutcome) {
    if !outcome.blocked.is_empty() {
        let _ = app.emit("blocked-changed", serde_json::json!({ "instanceId": instance_id }));
    }
    changed(app, "content-changed");
}

#[tauri::command]
pub async fn install_modrinth_version(
    app: AppHandle,
    instance_id: String,
    version_id: String,
    title: Option<String>,
    job_id: Option<String>,
) -> Result<InstallOutcome> {
    let job = job_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let name = instance_name(&app, &instance_id).await;
    let label = format!("Adding {} to {name}", title.unwrap_or_else(|| "a project".into()));
    let (a, i, v) = (app.clone(), instance_id.clone(), version_id.clone());
    let out = jobs::run(
        &app,
        &job,
        label,
        Some(instance_id.clone()),
        Some(Retry::ModrinthContent { instance_id: instance_id.clone(), version_id }),
        async move {
            let state = a.state::<AppState>();
            let inst = instance::get(&state.launcher, &i).await?;
            manage::install_modrinth(&state.launcher, &inst, &v).await
        },
    )
    .await?;
    outcome_toast(&app, &instance_id, &out);
    Ok(out)
}

#[tauri::command]
pub async fn install_curseforge_file(
    app: AppHandle,
    instance_id: String,
    mod_id: u64,
    file_id: u64,
    title: Option<String>,
    job_id: Option<String>,
) -> Result<InstallOutcome> {
    let job = job_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let name = instance_name(&app, &instance_id).await;
    let label = format!("Adding {} to {name}", title.unwrap_or_else(|| "a project".into()));
    let (a, i) = (app.clone(), instance_id.clone());
    let out = jobs::run(
        &app,
        &job,
        label,
        Some(instance_id.clone()),
        Some(Retry::CurseForgeContent { instance_id: instance_id.clone(), mod_id, file_id }),
        async move {
            let state = a.state::<AppState>();
            let inst = instance::get(&state.launcher, &i).await?;
            let out = manage::install_curseforge(&state.launcher, &inst, mod_id, file_id).await?;
            blocked::add(&state.launcher, &inst, &out.blocked).await?;
            Ok(out)
        },
    )
    .await?;
    outcome_toast(&app, &instance_id, &out);
    Ok(out)
}

/// Instances a failed or cancelled modpack job left half-made go to the trash, so a retry
/// starts clean instead of creating "pack-2".
async fn trash_unfinished(app: &AppHandle, before: &[String], started: i64) {
    let state = app.state::<AppState>();
    let Ok(now) = instance::list(&state.launcher).await else { return };
    for inst in now {
        if !before.contains(&inst.id) && inst.created_at >= started && inst.install_state != InstallState::Installed {
            // The task may have been stopped mid-write; give the filesystem a moment.
            tokio::time::sleep(Duration::from_millis(200)).await;
            if let Err(e) = instance::trash(&state.launcher, &inst.id).await {
                tracing::warn!("couldn't clean up {}: {e}", inst.id);
            }
        }
    }
    changed(app, "instances-changed");
}

async fn existing_ids(app: &AppHandle) -> Vec<String> {
    instance::list(&app.state::<AppState>().launcher).await.unwrap_or_default().into_iter().map(|i| i.id).collect()
}

/// Runs a job that creates an instance, cleaning up after a failure or cancel.
async fn creating<T: Send + 'static>(app: &AppHandle, run: impl std::future::Future<Output = Result<T>>) -> Result<T> {
    let before = existing_ids(app).await;
    let started = chrono::Utc::now().timestamp() - 1;
    let r = run.await;
    if r.is_err() {
        trash_unfinished(app, &before, started).await;
    }
    r
}

async fn instance_name(app: &AppHandle, id: &str) -> String {
    instance::get(&app.state::<AppState>().launcher, id).await.map(|i| i.name).unwrap_or_else(|_| id.to_string())
}

/// Installs a Modrinth modpack version as a new instance.
#[tauri::command]
pub async fn install_modrinth_modpack(app: AppHandle, version_id: String, job_id: String, title: Option<String>) -> Result<ImportResult> {
    let label = format!("Installing {}", title.unwrap_or_else(|| "a modpack".into()));
    let (a, v, j) = (app.clone(), version_id.clone(), job_id.clone());
    creating(
        &app,
        jobs::run(&app, &job_id, label, None, Some(Retry::ModrinthModpack { version_id }), async move {
            let launcher = a.state::<AppState>().launcher.clone();
            let version = modrinth::version(&launcher, &v).await?;
            let project = modrinth::project(&launcher, &version.project_id).await?;
            let file = version.primary_file().ok_or_else(|| Error::NotFound("modpack file".into()))?;
            let progress = progress_emitter(&a, &j, None);
            let mut result = modpack::import_url(&launcher, &file.url, &progress).await?;
            result.instance.name = project.title.clone();
            result.instance.icon = project.icon_url.clone();
            result.instance.linked_pack = Some(LinkedPack {
                source: PackSource::Modrinth,
                project_id: Some(project.id),
                version_id: Some(version.id),
                name: Some(project.title),
                version_name: Some(version.version_number),
            });
            result.instance.save(&launcher).await?;
            changed(&a, "instances-changed");
            let id = result.instance.id.clone();
            do_install(&a, &id, &j, false).await?;
            Ok(result)
        }),
    )
    .await
}

/// Installs a CurseForge modpack file as a new instance.
#[tauri::command]
pub async fn install_curseforge_modpack(
    app: AppHandle,
    mod_id: u64,
    file_id: u64,
    job_id: String,
    title: Option<String>,
) -> Result<ImportResult> {
    let label = format!("Installing {}", title.unwrap_or_else(|| "a modpack".into()));
    let (a, j) = (app.clone(), job_id.clone());
    creating(
        &app,
        jobs::run(&app, &job_id, label, None, Some(Retry::CurseForgeModpack { mod_id, file_id }), async move {
            let launcher = a.state::<AppState>().launcher.clone();
            let m = curseforge::get_mod(&launcher, mod_id).await?;
            let f = curseforge::file(&launcher, mod_id, file_id).await?;
            let url = f.download_url.clone().ok_or_else(|| {
                Error::other(format!(
                    "{} can only be downloaded on CurseForge's website: {}",
                    m.name,
                    curseforge::manual_download_page(&m, f.id)
                ))
            })?;
            let progress = progress_emitter(&a, &j, None);
            let mut result = modpack::import_url(&launcher, &url, &progress).await?;
            result.instance.icon = m.logo.as_ref().map(|l| l.url.clone());
            result.instance.linked_pack = Some(LinkedPack {
                source: PackSource::CurseForge,
                project_id: Some(m.id.to_string()),
                version_id: Some(f.id.to_string()),
                name: Some(m.name.clone()),
                version_name: Some(f.display_name.clone()),
            });
            result.instance.save(&launcher).await?;
            changed(&a, "instances-changed");
            let id = result.instance.id.clone();
            do_install(&a, &id, &j, false).await?;
            Ok(result)
        }),
    )
    .await
}

/// Imports a .mrpack / CurseForge zip from a local path or an https URL, then installs it.
#[tauri::command]
pub async fn import_modpack(app: AppHandle, source: String, job_id: String) -> Result<ImportResult> {
    let name = Path::new(&source).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| source.clone());
    let (a, s, j) = (app.clone(), source.clone(), job_id.clone());
    creating(
        &app,
        jobs::run(&app, &job_id, format!("Importing {name}"), None, Some(Retry::ImportModpack { source }), async move {
            let launcher = a.state::<AppState>().launcher.clone();
            let progress = progress_emitter(&a, &j, None);
            let result = if s.starts_with("http://") || s.starts_with("https://") {
                modpack::import_url(&launcher, &s, &progress).await?
            } else {
                modpack::import_file(&launcher, Path::new(&s), &progress).await?
            };
            changed(&a, "instances-changed");
            let id = result.instance.id.clone();
            do_install(&a, &id, &j, false).await?;
            Ok(result)
        }),
    )
    .await
}

// ---------- the content tab ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentItem {
    pub path: String,
    pub file_name: String,
    pub kind: ContentKind,
    pub enabled: bool,
    pub size: u64,
    pub title: String,
    pub meta: Option<ContentMeta>,
    /// From the jar: mod id, version, description, which loaders it's for.
    pub mod_id: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
    pub formats: Vec<jarmeta::Format>,
    pub has_jar_icon: bool,
    pub requires: Vec<String>,
    pub required_by: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub items: Vec<ContentItem>,
    pub report: Report,
    pub blocked: Vec<BlockedFile>,
    /// The pack lists files it couldn't include that aren't here yet.
    pub listed_missing: bool,
}

async fn java_major(state: &AppState, inst: &Instance) -> Option<u32> {
    if inst.java_path.is_some() || inst.install_state != InstallState::Installed {
        return None;
    }
    install::resolve(&state.launcher, inst).await.ok().map(|r| r.json.java_major())
}

/// Everything the Content tab shows, in one call. Identifying unknown files happens in the
/// background afterwards; `content-changed` fires if anything new was learned.
#[tauri::command]
pub async fn content_overview(app: AppHandle, state: State<'_, AppState>, instance_id: String) -> Result<Overview> {
    let launcher = &state.launcher;
    let inst = instance::get(launcher, &instance_id).await?;
    let files = scan::scan(launcher, &inst).await?;
    let report = check::check(&inst, &files, java_major(&state, &inst).await);
    let pending = blocked::pending(launcher, &inst).await;
    let items = files
        .iter()
        .map(|f| {
            let primary = f.jar.as_ref().and_then(|j| j.primary());
            ContentItem {
                path: f.path.clone(),
                file_name: f.file_name.clone(),
                kind: f.kind,
                enabled: f.enabled,
                size: f.size,
                title: scan::display_name(f),
                meta: f.index.clone(),
                mod_id: primary.map(|m| m.id.clone()),
                version: primary.map(|m| m.version.clone()).filter(|v| !v.is_empty()),
                description: primary.and_then(|m| m.description.clone()),
                formats: f.jar.as_ref().map(|j| j.formats.clone()).unwrap_or_default(),
                has_jar_icon: primary.is_some_and(|m| m.icon.is_some()),
                requires: report.requires.get(&f.path).cloned().unwrap_or_default(),
                required_by: report.required_by.get(&f.path).cloned().unwrap_or_default(),
            }
        })
        .collect();

    let needs_identify = files
        .iter()
        .any(|f| f.index.is_none() || f.index.as_ref().is_some_and(|m| m.requires.is_none() && m.source == content::Source::Modrinth));
    if needs_identify {
        let app2 = app.clone();
        tauri::async_runtime::spawn(async move {
            let state = app2.state::<AppState>();
            if let Ok(inst) = instance::get(&state.launcher, &instance_id).await {
                match identify::identify(&state.launcher, &inst, &files).await {
                    Ok(n) if n > 0 => changed(&app2, "content-changed"),
                    Ok(_) => {}
                    Err(e) => tracing::warn!("identifying files failed: {e}"),
                }
            }
        });
    }
    let listed_missing = content::listed::has_missing(launcher, &inst).await;
    Ok(Overview { items, report, blocked: pending, listed_missing })
}

/// A mod's icon from inside its jar, as a data URL (for files no platform knows).
#[tauri::command]
pub async fn content_icon(state: State<'_, AppState>, instance_id: String, path: String) -> Result<Option<String>> {
    let inst = instance::get(&state.launcher, &instance_id).await?;
    let dir = inst.dir(&state.launcher);
    let mut full = alloy_core::util::safe_join(&dir, &path)?;
    if !full.exists() {
        full = dir.join(format!("{path}.disabled"));
    }
    tokio::task::spawn_blocking(move || {
        let meta = jarmeta::read_path(&full).ok()?;
        let icon = meta.primary()?.icon.clone()?;
        let bytes = jarmeta::read_entry(&full, &icon, 512 * 1024)?;
        let mime = if icon.ends_with(".jpg") || icon.ends_with(".jpeg") { "image/jpeg" } else { "image/png" };
        Some(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
    })
    .await
    .map_err(|e| Error::other(e.to_string()))
}

#[tauri::command]
pub async fn set_content_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    paths: Vec<String>,
    enabled: bool,
) -> Result<()> {
    let inst = instance::get(&state.launcher, &instance_id).await?;
    for p in &paths {
        content::set_enabled(&state.launcher, &inst, p, enabled).await?;
    }
    changed(&app, "content-changed");
    Ok(())
}

#[tauri::command]
pub async fn remove_content(app: AppHandle, state: State<'_, AppState>, instance_id: String, paths: Vec<String>) -> Result<()> {
    let inst = instance::get(&state.launcher, &instance_id).await?;
    for p in &paths {
        content::remove(&state.launcher, &inst, p).await?;
    }
    changed(&app, "content-changed");
    Ok(())
}

#[tauri::command]
pub async fn content_fixes(state: State<'_, AppState>, instance_id: String) -> Result<Vec<Suggestion>> {
    let launcher = &state.launcher;
    let inst = instance::get(launcher, &instance_id).await?;
    let files = scan::scan(launcher, &inst).await?;
    let report = check::check(&inst, &files, java_major(&state, &inst).await);
    Ok(manage::suggest(launcher, &inst, &report, &files).await)
}

#[tauri::command]
pub async fn apply_fix(app: AppHandle, state: State<'_, AppState>, instance_id: String, fix: Fix) -> Result<InstallOutcome> {
    let launcher = &state.launcher;
    let mut inst = instance::get(launcher, &instance_id).await?;
    if let Fix::SetLoaderVersion { version } = &fix {
        inst.loader_version = Some(version.clone());
        inst.install_state = InstallState::NotInstalled;
        inst.save(launcher).await?;
        changed(&app, "instances-changed");
        changed(&app, "content-changed");
        return Ok(InstallOutcome::default());
    }
    let out = manage::apply_fix(launcher, &inst, &fix).await?;
    blocked::add(launcher, &inst, &out.blocked).await?;
    outcome_toast(&app, &instance_id, &out);
    Ok(out)
}

#[tauri::command]
pub async fn check_content_updates(state: State<'_, AppState>, instance_id: String) -> Result<Vec<Update>> {
    let inst = instance::get(&state.launcher, &instance_id).await?;
    manage::check_updates(&state.launcher, &inst).await
}

#[tauri::command]
pub async fn apply_content_updates(app: AppHandle, instance_id: String, updates: Vec<Update>, job_id: String) -> Result<UpdateResult> {
    let name = instance_name(&app, &instance_id).await;
    let n = updates.len();
    let (a, i) = (app.clone(), instance_id.clone());
    let r = jobs::run(
        &app,
        &job_id,
        format!("Updating {n} {} in {name}", if n == 1 { "file" } else { "files" }),
        Some(instance_id),
        None,
        async move {
            let state = a.state::<AppState>();
            let inst = instance::get(&state.launcher, &i).await?;
            manage::apply_updates(&state.launcher, &inst, updates).await
        },
    )
    .await?;
    changed(&app, "content-changed");
    Ok(r)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preflight {
    pub errors: Vec<Problem>,
    pub warnings: Vec<Problem>,
    /// Problems with the computer rather than the mods.
    pub system: Vec<String>,
}

/// What would stop the game from starting, checked in milliseconds before launching.
#[tauri::command]
pub async fn preflight(state: State<'_, AppState>, instance_id: String) -> Result<Preflight> {
    let launcher = &state.launcher;
    let inst = instance::get(launcher, &instance_id).await?;
    let mut system_issues = vec![];
    let mut java = None;
    if inst.install_state == InstallState::Installed {
        if let Ok(resolved) = install::resolve(launcher, &inst).await {
            if inst.java_path.is_none() {
                java = Some(resolved.json.java_major());
            }
            if system::missing_xrandr(&resolved.json) {
                system_issues.push(
                    "Minecraft versions this old need the xrandr program on Linux. Install it with: sudo pacman -S xorg-xrandr".into(),
                );
            }
        }
    }
    let files = scan::scan(launcher, &inst).await?;
    let report = check::check(&inst, &files, java);
    let (errors, warnings) = report.problems.into_iter().partition(|p| p.severity == Severity::Error);
    Ok(Preflight { errors, warnings, system: system_issues })
}

/// Gets the files the modpack lists but couldn't include (see `content::listed`).
#[tauri::command]
pub async fn fetch_listed_files(app: AppHandle, instance_id: String) -> Result<content::listed::ListedOutcome> {
    let name = instance_name(&app, &instance_id).await;
    let (a, i) = (app.clone(), instance_id.clone());
    let out = jobs::run(
        &app,
        &uuid::Uuid::new_v4().to_string(),
        format!("Getting the files {name} lists"),
        Some(instance_id.clone()),
        None,
        async move {
            let state = a.state::<AppState>();
            let inst = instance::get(&state.launcher, &i).await?;
            content::listed::fetch(&state.launcher, &inst).await
        },
    )
    .await?;
    let _ = app.emit("blocked-changed", serde_json::json!({ "instanceId": instance_id }));
    changed(&app, "content-changed");
    Ok(out)
}

#[tauri::command]
pub async fn cleanup_orphans(app: AppHandle, state: State<'_, AppState>, instance_id: String) -> Result<Vec<String>> {
    let launcher = &state.launcher;
    let inst = instance::get(launcher, &instance_id).await?;
    let files = scan::scan(launcher, &inst).await?;
    let report = check::check(&inst, &files, None);
    let mut removed = vec![];
    for path in &report.orphans {
        if let Some(f) = files.iter().find(|f| &f.path == path) {
            removed.push(scan::display_name(f));
        }
        content::remove(launcher, &inst, path).await?;
    }
    changed(&app, "content-changed");
    Ok(removed)
}

// ---------- CurseForge blocked downloads ----------

#[tauri::command]
pub async fn blocked_files(state: State<'_, AppState>, instance_id: String) -> Result<Vec<BlockedFile>> {
    let inst = instance::get(&state.launcher, &instance_id).await?;
    Ok(blocked::pending(&state.launcher, &inst).await)
}

#[tauri::command]
pub async fn dismiss_blocked(app: AppHandle, state: State<'_, AppState>, instance_id: String, file_id: u64) -> Result<()> {
    let inst = instance::get(&state.launcher, &instance_id).await?;
    blocked::dismiss(&state.launcher, &inst, file_id).await?;
    let _ = app.emit("blocked-changed", serde_json::json!({ "instanceId": instance_id }));
    Ok(())
}

pub fn download_dirs() -> Vec<PathBuf> {
    dirs::download_dir().or_else(|| dirs::home_dir().map(|h| h.join("Downloads"))).into_iter().collect()
}

/// Every few seconds, while any instance waits for blocked CurseForge files, looks for them in
/// the Downloads folder and installs what it finds.
pub fn start_blocked_watcher(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(3)).await;
            let state = app.state::<AppState>();
            let Ok(instances) = instance::list(&state.launcher).await else { continue };
            for inst in instances {
                if !inst.dir(&state.launcher).join(".launcher").join("blocked.json").exists() {
                    continue;
                }
                match blocked::collect(&state.launcher, &inst, &download_dirs()).await {
                    Ok(found) if !found.is_empty() => {
                        let _ = app.emit("blocked-collected", serde_json::json!({ "instanceId": inst.id, "titles": found }));
                        let _ = app.emit("blocked-changed", serde_json::json!({ "instanceId": inst.id }));
                        changed(&app, "content-changed");
                    }
                    Ok(_) => {}
                    Err(e) => tracing::warn!("blocked files for {}: {e}", inst.id),
                }
            }
        }
    });
}

// ---------- links & dropped files ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkPreview {
    pub platform: String,
    pub kind: ContentKind,
    pub title: String,
    pub icon_url: Option<String>,
    pub author: Option<String>,
    pub project_id: String,
    /// A specific version or file the link pointed at.
    pub version_id: Option<String>,
    pub url: String,
}

/// What a pasted link is, so the UI can show it before installing.
#[tauri::command]
pub async fn resolve_link(state: State<'_, AppState>, url: String) -> Result<LinkPreview> {
    let launcher = &state.launcher;
    match links::parse_link(&url)? {
        Link::Modrinth { slug, version } => {
            let p = modrinth::project(launcher, &slug).await?;
            let version_id = match version {
                Some(v) => modrinth::project_versions(launcher, &p.id, &[], &[])
                    .await?
                    .into_iter()
                    .find(|x| x.id == v || x.version_number == v)
                    .map(|x| x.id),
                None => None,
            };
            Ok(LinkPreview {
                platform: "modrinth".into(),
                kind: ContentKind::from_modrinth(&p.project_type),
                title: p.title,
                icon_url: p.icon_url,
                author: None,
                project_id: p.id,
                version_id,
                url,
            })
        }
        Link::CurseForge { slug, section, file_id } => {
            let kind = Link::curseforge_kind(&section);
            let m =
                curseforge::mod_by_slug(launcher, &slug, kind).await?.ok_or_else(|| Error::NotFound(format!("{slug} on CurseForge")))?;
            Ok(LinkPreview {
                platform: "curseforge".into(),
                kind: kind.unwrap_or(ContentKind::Mod),
                title: m.name.clone(),
                icon_url: m.logo.as_ref().map(|l| l.thumbnail_url.clone().unwrap_or(l.url.clone())),
                author: m.authors.first().map(|a| a.name.clone()),
                project_id: m.id.to_string(),
                version_id: file_id.map(|f| f.to_string()),
                url,
            })
        }
        Link::Archive { url: u } => Ok(LinkPreview {
            platform: "url".into(),
            kind: ContentKind::Modpack,
            title: u.rsplit('/').next().unwrap_or("Modpack").to_string(),
            icon_url: None,
            author: None,
            project_id: String::new(),
            version_id: None,
            url,
        }),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddedFile {
    pub name: String,
    pub kind: DroppedKind,
    /// Where it went: the instance id (content) or the new instance (modpacks).
    pub instance_id: Option<String>,
    pub error: Option<String>,
}

/// Adds dropped files: modpacks become new instances, everything else goes into `instance_id`.
#[tauri::command]
pub async fn add_files(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: Option<String>,
    paths: Vec<String>,
) -> Result<Vec<AddedFile>> {
    let launcher = &state.launcher;
    let mut out = vec![];
    for p in paths {
        let path = PathBuf::from(&p);
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.clone());
        let pc = path.clone();
        let kind = tokio::task::spawn_blocking(move || links::classify(&pc)).await.unwrap_or(DroppedKind::Unknown);
        let folder = match kind {
            DroppedKind::Mod => Some("mods"),
            DroppedKind::ResourcePack => Some("resourcepacks"),
            DroppedKind::Shader => Some("shaderpacks"),
            _ => None,
        };
        let result: Result<Option<String>> = match (kind, folder, &instance_id) {
            (DroppedKind::Modpack, _, _) => {
                let job = uuid::Uuid::new_v4().to_string();
                let a = app.clone();
                let source = p.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = import_modpack(a.clone(), source, job).await {
                        tracing::warn!("import failed: {e}");
                    }
                });
                Ok(None)
            }
            (_, Some(folder), Some(iid)) => {
                async {
                    let inst = instance::get(launcher, iid).await?;
                    let dest = inst.dir(launcher).join(folder).join(&name);
                    tokio::fs::create_dir_all(dest.parent().unwrap()).await?;
                    tokio::fs::copy(&path, &dest).await?;
                    Ok(Some(iid.clone()))
                }
                .await
            }
            (_, Some(_), None) => Err(Error::other("Open an instance first, then drop it there")),
            (DroppedKind::World, _, _) => {
                Err(Error::other("Worlds can't be added this way yet; copy the folder into the instance's saves"))
            }
            (DroppedKind::DataPack, _, _) => Err(Error::other("Data packs belong to a world; copy it into that world's datapacks folder")),
            _ => Err(Error::other("Alloy doesn't recognise this file")),
        };
        out.push(match result {
            Ok(iid) => AddedFile { name, kind, instance_id: iid, error: None },
            Err(e) => AddedFile { name, kind, instance_id: None, error: Some(e.to_string()) },
        });
    }
    changed(&app, "content-changed");
    Ok(out)
}

// ---------- instance icons and memory ----------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum IconInput {
    /// A square PNG, base64 (the UI crops and scales it).
    Image {
        data: String,
    },
    /// One of the block textures, e.g. "diamond_ore".
    Block {
        name: String,
    },
    None,
}

pub const ICON_FILE: &str = "icon.png";

#[tauri::command]
pub async fn set_instance_icon(app: AppHandle, state: State<'_, AppState>, id: String, icon: IconInput) -> Result<()> {
    let launcher = &state.launcher;
    let mut inst = instance::get(launcher, &id).await?;
    let file = inst.dir(launcher).join(ICON_FILE);
    inst.icon = match icon {
        IconInput::Image { data } => {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(data.trim_start_matches("data:image/png;base64,"))
                .map_err(|_| Error::other("That image couldn't be read"))?;
            if bytes.len() > 2 * 1024 * 1024 || !bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
                return Err(Error::other("Icons must be PNG images under 2 MB"));
            }
            tokio::fs::write(&file, bytes).await?;
            Some(ICON_FILE.into())
        }
        IconInput::Block { name } => {
            let _ = tokio::fs::remove_file(&file).await;
            Some(format!("block:{name}"))
        }
        IconInput::None => {
            let _ = tokio::fs::remove_file(&file).await;
            None
        }
    };
    inst.save(launcher).await?;
    changed(&app, "instances-changed");
    Ok(())
}

/// The instance's own icon file as a data URL, if it has one.
#[tauri::command]
pub async fn instance_icon(state: State<'_, AppState>, id: String) -> Result<Option<String>> {
    let inst = instance::get(&state.launcher, &id).await?;
    if inst.icon.as_deref() != Some(ICON_FILE) {
        return Ok(None);
    }
    let bytes = tokio::fs::read(inst.dir(&state.launcher).join(ICON_FILE)).await.ok();
    Ok(bytes.map(|b| format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(b))))
}

/// A dropped image file as a data URL, for the icon editor.
#[tauri::command]
pub async fn read_image_file(path: String) -> Result<String> {
    let p = PathBuf::from(&path);
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => return Err(Error::other("That isn't an image Alloy can use")),
    };
    let meta = tokio::fs::metadata(&p).await?;
    if meta.len() > 16 * 1024 * 1024 {
        return Err(Error::other("That image is over 16 MB"));
    }
    let bytes = tokio::fs::read(&p).await?;
    Ok(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryAdvice {
    pub total_mb: u64,
    pub max_game_mb: u64,
    /// For this instance's mod count, when one is given.
    pub recommended_mb: Option<u64>,
    pub mods: Option<usize>,
}

#[tauri::command]
pub async fn memory_advice(state: State<'_, AppState>, instance_id: Option<String>) -> Result<MemoryAdvice> {
    let mem = system::memory().unwrap_or(system::Memory { total_mb: 16384, max_game_mb: 12288 });
    let mods = match instance_id {
        Some(id) => {
            let inst = instance::get(&state.launcher, &id).await?;
            let files = scan::scan(&state.launcher, &inst).await?;
            Some(files.iter().filter(|f| f.kind == ContentKind::Mod && f.enabled).count())
        }
        None => None,
    };
    Ok(MemoryAdvice {
        total_mb: mem.total_mb,
        max_game_mb: mem.max_game_mb,
        recommended_mb: mods.map(|m| system::recommended_mb(m, mem.total_mb)),
        mods,
    })
}

// ---------- jobs ----------

#[tauri::command]
pub fn list_jobs(app: AppHandle) -> Vec<jobs::Job> {
    app.state::<jobs::Jobs>().list()
}

#[tauri::command]
pub fn cancel_job(app: AppHandle, id: String) -> bool {
    let ok = app.state::<jobs::Jobs>().cancel(&id);
    jobs::changed(&app);
    ok
}

#[tauri::command]
pub fn clear_jobs(app: AppHandle) {
    app.state::<jobs::Jobs>().clear_finished();
    jobs::changed(&app);
}

/// Runs a failed or cancelled job again under the same id. Returns right away.
#[tauri::command]
pub fn retry_job(app: AppHandle, id: String) -> Result<()> {
    let spec = app.state::<jobs::Jobs>().retry_spec(&id).ok_or_else(|| Error::other("This job can't be retried"))?;
    let a = app.clone();
    tauri::async_runtime::spawn(async move {
        let r = match spec {
            Retry::Install { instance_id, repair } => crate::commands::run_install(&a, &instance_id, &id, repair).await.map(|_| ()),
            Retry::ImportModpack { source } => import_modpack(a.clone(), source, id).await.map(|_| ()),
            Retry::ModrinthModpack { version_id } => install_modrinth_modpack(a.clone(), version_id, id, None).await.map(|_| ()),
            Retry::CurseForgeModpack { mod_id, file_id } => {
                install_curseforge_modpack(a.clone(), mod_id, file_id, id, None).await.map(|_| ())
            }
            Retry::ModrinthContent { instance_id, version_id } => {
                install_modrinth_version(a.clone(), instance_id, version_id, None, Some(id)).await.map(|_| ())
            }
            Retry::CurseForgeContent { instance_id, mod_id, file_id } => {
                install_curseforge_file(a.clone(), instance_id, mod_id, file_id, None, Some(id)).await.map(|_| ())
            }
        };
        if let Err(e) = r {
            tracing::warn!("retry failed: {e}");
        }
    });
    Ok(())
}
