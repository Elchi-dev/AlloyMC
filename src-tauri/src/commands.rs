//! The IPC surface. Every function here is callable from the UI via `invoke()`.
//! Long jobs report through events:
//!   job-progress  { jobId, instanceId?, progress }
//!   game-state    { instanceId, state: "starting" | "running" | "exited", exitCode? }
//!   game-log      { instanceId, stream, line }
//!   instances-changed / accounts-changed  (no payload; UI refetches)

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use alloy_core::auth::microsoft::MsClient;
use alloy_core::auth::Account;
use alloy_core::content::{self, curseforge, modrinth, InstalledContent, SearchPage, SearchQuery};
use alloy_core::instance::{self, InstallState, Instance, LinkedPack, NewInstance, PackSource};
use alloy_core::java::{self, JavaInstall};
use alloy_core::launch::{self, LaunchOptions, QuickPlay};
use alloy_core::meta::loaders::{self, Loader, LoaderVersion};
use alloy_core::meta::{mojang, GameVersion};
use alloy_core::modpack::{self, ImportResult};
use alloy_core::progress::{Progress, ProgressFn, Stage};
use alloy_core::settings::{OnLaunch, Settings};
use alloy_core::{install, Error, Result};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::state::{AppState, RunningGame};

// ---------- helpers ----------

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct JobProgress {
    job_id: String,
    instance_id: Option<String>,
    progress: Progress,
}

/// Emits progress events, at most every 60 ms within a stage (asset installs report thousands of files).
fn progress_emitter(app: &AppHandle, job_id: &str, instance_id: Option<&str>) -> ProgressFn {
    let app = app.clone();
    let job_id = job_id.to_string();
    let instance_id = instance_id.map(str::to_string);
    let last: Mutex<(Instant, Option<Stage>)> = Mutex::new((Instant::now() - Duration::from_secs(1), None));
    Arc::new(move |p: Progress| {
        {
            let mut l = last.lock().unwrap();
            let stage_changed = l.1 != Some(p.stage);
            let finished = p.current >= p.total;
            if !stage_changed && !finished && l.0.elapsed() < Duration::from_millis(60) {
                return;
            }
            *l = (Instant::now(), Some(p.stage));
        }
        let _ = app.emit(
            "job-progress",
            JobProgress {
                job_id: job_id.clone(),
                instance_id: instance_id.clone(),
                progress: p,
            },
        );
    })
}

fn changed(app: &AppHandle, what: &str) {
    let _ = app.emit(what, ());
}

async fn run_install(app: &AppHandle, state: &AppState, id: &str, job_id: &str, repair: bool) -> Result<Instance> {
    if !state.installing.lock().unwrap().insert(id.to_string()) {
        return Err(Error::other("This instance is already installing"));
    }
    let result = async {
        let mut inst = instance::get(&state.launcher, id).await?;
        changed(app, "instances-changed");
        let progress = progress_emitter(app, job_id, Some(id));
        install::install(&state.launcher, &mut inst, &progress, repair).await?;
        Ok(inst)
    }
    .await;
    state.installing.lock().unwrap().remove(id);
    changed(app, "instances-changed");
    result
}

// ---------- app & settings ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: String,
    data_dir: PathBuf,
    os: String,
    /// Private builds carry a CurseForge key; public builds show CurseForge as work in progress.
    curseforge_builtin: bool,
}

#[tauri::command]
pub fn app_info(state: State<'_, AppState>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION").into(),
        data_dir: state.launcher.dirs.root().to_path_buf(),
        os: std::env::consts::OS.into(),
        curseforge_builtin: curseforge::builtin_key().is_some(),
    }
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<Settings> {
    Ok(state.launcher.settings.read().await.clone())
}

#[tauri::command]
pub async fn save_settings(state: State<'_, AppState>, settings: Settings) -> Result<()> {
    state.launcher.save_settings(settings).await
}

/// Block textures from Mojang's jar as data URLs (empty until they've been fetched).
#[tauri::command]
pub async fn get_textures(state: State<'_, AppState>) -> Result<std::collections::HashMap<String, String>> {
    launcher_textures(&state).await
}

async fn launcher_textures(state: &AppState) -> Result<std::collections::HashMap<String, String>> {
    alloy_core::textures::data_urls(&state.launcher).await
}

// ---------- metadata ----------

#[tauri::command]
pub async fn list_game_versions(state: State<'_, AppState>, include_snapshots: bool) -> Result<Vec<GameVersion>> {
    let manifest = mojang::manifest(&state.launcher).await?;
    Ok(manifest
        .versions
        .into_iter()
        .filter(|v| include_snapshots || v.kind == "release")
        .collect())
}

#[tauri::command]
pub async fn list_loader_versions(state: State<'_, AppState>, loader: Loader, game_version: String) -> Result<Vec<LoaderVersion>> {
    loaders::versions(&state.launcher, loader, &game_version).await
}

#[tauri::command]
pub async fn list_java(state: State<'_, AppState>) -> Result<Vec<JavaInstall>> {
    Ok(java::detect_all(&state.launcher).await)
}

// ---------- instances ----------

#[tauri::command]
pub async fn list_instances(state: State<'_, AppState>) -> Result<Vec<Instance>> {
    instance::list(&state.launcher).await
}

#[tauri::command]
pub async fn get_instance(state: State<'_, AppState>, id: String) -> Result<Instance> {
    instance::get(&state.launcher, &id).await
}

/// Creates the instance and starts installing it in the background. Returns immediately.
#[tauri::command]
pub async fn create_instance(app: AppHandle, state: State<'_, AppState>, new: NewInstance) -> Result<Instance> {
    let inst = instance::create(&state.launcher, new).await?;
    changed(&app, "instances-changed");
    let id = inst.id.clone();
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app2.state::<AppState>();
        if let Err(e) = run_install(&app2, &state, &id, &format!("install:{id}"), false).await {
            tracing::error!("install of {id} failed: {e}");
        }
    });
    Ok(inst)
}

#[tauri::command]
pub async fn update_instance(app: AppHandle, state: State<'_, AppState>, instance: Instance) -> Result<()> {
    // Changing version or loader means files must be fetched again.
    let old = instance::get(&state.launcher, &instance.id).await?;
    let mut inst = instance;
    if old.game_version != inst.game_version || old.loader != inst.loader || old.loader_version != inst.loader_version {
        inst.install_state = InstallState::NotInstalled;
    }
    inst.save(&state.launcher).await?;
    changed(&app, "instances-changed");
    Ok(())
}

#[tauri::command]
pub async fn delete_instance(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<()> {
    if state.running.lock().unwrap().contains_key(&id) {
        return Err(Error::other("Stop the game before deleting this instance"));
    }
    instance::trash(&state.launcher, &id).await?;
    changed(&app, "instances-changed");
    Ok(())
}

#[tauri::command]
pub async fn duplicate_instance(app: AppHandle, state: State<'_, AppState>, id: String, name: String) -> Result<Instance> {
    let copy = instance::duplicate(&state.launcher, &id, &name).await?;
    changed(&app, "instances-changed");
    Ok(copy)
}

/// Installs an instance, or with `repair` re-checks every file by hash.
/// Progress arrives as `job-progress` with this job id.
#[tauri::command]
pub async fn install_instance(app: AppHandle, state: State<'_, AppState>, id: String, job_id: String, repair: Option<bool>) -> Result<()> {
    run_install(&app, &state, &id, &job_id, repair.unwrap_or(false)).await.map(|_| ())
}

#[tauri::command]
pub async fn open_instance_folder(app: AppHandle, state: State<'_, AppState>, id: String, sub: Option<String>) -> Result<()> {
    use tauri_plugin_opener::OpenerExt;
    let mut dir = state.launcher.dirs.instance(&id);
    if let Some(sub) = sub {
        dir = alloy_core::util::safe_join(&dir, &sub)?;
        tokio::fs::create_dir_all(&dir).await?;
    }
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| Error::other(e.to_string()))
}

// ---------- launching ----------

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", content = "target", rename_all = "lowercase")]
pub enum QuickPlayArg {
    World(String),
    Server(String),
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GameState {
    instance_id: String,
    state: &'static str,
    exit_code: Option<i32>,
    error: Option<String>,
}

fn game_state(app: &AppHandle, id: &str, state: &'static str, exit_code: Option<i32>, error: Option<String>) {
    let _ = app.emit(
        "game-state",
        GameState {
            instance_id: id.into(),
            state,
            exit_code,
            error,
        },
    );
}

#[tauri::command]
pub async fn launch_instance(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    quick_play: Option<QuickPlayArg>,
) -> Result<()> {
    if state.running.lock().unwrap().contains_key(&id) {
        return Err(Error::other("Already running"));
    }
    game_state(&app, &id, "starting", None, None);
    let result = launch_inner(&app, &state, &id, quick_play).await;
    if let Err(e) = &result {
        game_state(&app, &id, "exited", None, Some(e.to_string()));
    }
    result
}

async fn launch_inner(app: &AppHandle, state: &AppState, id: &str, quick_play: Option<QuickPlayArg>) -> Result<()> {
    let launcher = &state.launcher;
    let mut inst = instance::get(launcher, id).await?;

    // Account first: a failed sign-in shouldn't cost a 300 MB download.
    let account_id = match inst.account.clone() {
        Some(a) => a,
        None => state
            .accounts
            .list()
            .await?
            .1
            .ok_or_else(|| Error::other("Add an account before playing"))?,
    };
    let session = state.accounts.session(&account_id).await?;

    let resolved = if inst.install_state == InstallState::Installed {
        install::resolve(launcher, &inst).await?
    } else {
        run_install(app, state, id, &format!("launch:{id}"), false).await?;
        inst = instance::get(launcher, id).await?;
        install::resolve(launcher, &inst).await?
    };

    let opts = LaunchOptions {
        quick_play: quick_play.map(|q| match q {
            QuickPlayArg::World(w) => QuickPlay::Singleplayer(w),
            QuickPlayArg::Server(s) => QuickPlay::Multiplayer(s),
        }),
    };
    let cmd = launch::prepare(launcher, &inst, &resolved, &session, &opts).await?;

    let log_app = app.clone();
    let log_id = id.to_string();
    let mut child = launch::spawn(
        &cmd,
        Arc::new(move |line| {
            let _ = log_app.emit(
                "game-log",
                serde_json::json!({ "instanceId": log_id, "stream": line.stream, "line": line.line }),
            );
        }),
    )?;

    let started = chrono::Utc::now().timestamp();
    inst.last_played = Some(started);
    inst.save(launcher).await?;
    changed(app, "instances-changed");

    let (kill_tx, kill_rx) = tokio::sync::oneshot::channel::<()>();
    state.running.lock().unwrap().insert(
        id.to_string(),
        RunningGame {
            started_at: started,
            kill: Some(kill_tx),
        },
    );
    game_state(app, id, "running", None, None);

    let on_launch = launcher.settings.read().await.on_launch;
    if let Some(main) = app.get_webview_window("main") {
        match on_launch {
            OnLaunch::Minimize => {
                let _ = main.minimize();
            }
            OnLaunch::Close => {
                let _ = main.hide();
            }
            OnLaunch::KeepOpen => {}
        }
    }

    let app2 = app.clone();
    let id2 = id.to_string();
    tauri::async_runtime::spawn(async move {
        let status = tokio::select! {
            s = child.wait() => s.ok(),
            _ = kill_rx => { let _ = child.kill().await; child.wait().await.ok() }
        };
        let state = app2.state::<AppState>();
        let started = state
            .running
            .lock()
            .unwrap()
            .remove(&id2)
            .map(|r| r.started_at)
            .unwrap_or(started);
        if let Ok(mut inst) = instance::get(&state.launcher, &id2).await {
            inst.play_time_secs += (chrono::Utc::now().timestamp() - started).max(0) as u64;
            let _ = inst.save(&state.launcher).await;
        }
        game_state(&app2, &id2, "exited", status.and_then(|s| s.code()), None);
        changed(&app2, "instances-changed");
        if let Some(main) = app2.get_webview_window("main") {
            let _ = main.show();
            let _ = main.unminimize();
        }
    });
    Ok(())
}

/// Called when the pointer rests on Play: refreshes the account token and reads the version
/// files now, so the click itself only has to start Java.
#[tauri::command]
pub async fn prewarm_launch(state: State<'_, AppState>, id: String) -> Result<()> {
    let launcher = &state.launcher;
    let inst = instance::get(launcher, &id).await?;
    if inst.install_state != InstallState::Installed {
        return Ok(());
    }
    let account = match inst.account.clone() {
        Some(a) => Some(a),
        None => state.accounts.list().await?.1,
    };
    let (resolved, _) = tokio::join!(install::resolve(launcher, &inst), async {
        if let Some(a) = account {
            let _ = state.accounts.session(&a).await;
        }
    });
    resolved.map(|_| ())
}

#[tauri::command]
pub fn stop_instance(state: State<'_, AppState>, id: String) -> Result<()> {
    let mut running = state.running.lock().unwrap();
    match running.get_mut(&id).and_then(|r| r.kill.take()) {
        Some(tx) => {
            let _ = tx.send(());
            Ok(())
        }
        None => Err(Error::other("Not running")),
    }
}

#[tauri::command]
pub fn running_instances(state: State<'_, AppState>) -> Vec<String> {
    state.running.lock().unwrap().keys().cloned().collect()
}

// ---------- accounts ----------

#[derive(Serialize)]
pub struct AccountList {
    accounts: Vec<Account>,
    active: Option<String>,
}

#[tauri::command]
pub async fn list_accounts(state: State<'_, AppState>) -> Result<AccountList> {
    let (accounts, active) = state.accounts.list().await?;
    Ok(AccountList { accounts, active })
}

#[tauri::command]
pub async fn add_offline_account(app: AppHandle, state: State<'_, AppState>, name: String) -> Result<Account> {
    let a = state.accounts.add_offline(&name).await?;
    changed(&app, "accounts-changed");
    Ok(a)
}

/// Opens the Microsoft login window and finishes the Xbox/Minecraft chain.
#[tauri::command]
pub async fn login_microsoft(app: AppHandle, state: State<'_, AppState>) -> Result<Account> {
    let custom = state.launcher.settings.read().await.custom_msa_client_id.clone();
    let client = MsClient::from_settings(custom.as_deref());
    let code = crate::ms_login::get_code(&app, &client).await?;
    let account = state.accounts.add_microsoft(&client, &code).await?;
    changed(&app, "accounts-changed");
    Ok(account)
}

#[tauri::command]
pub async fn remove_account(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<()> {
    state.accounts.remove(&id).await?;
    changed(&app, "accounts-changed");
    Ok(())
}

#[tauri::command]
pub async fn set_active_account(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<()> {
    state.accounts.set_active(&id).await?;
    changed(&app, "accounts-changed");
    Ok(())
}

// ---------- content ----------

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Modrinth,
    CurseForge,
}

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
            // Loader filters only make sense for mods and modpacks.
            let loaders = if matches!(project.project_type.as_str(), "mod" | "modpack") {
                modrinth::compatible_loaders(inst.loader)
            } else {
                vec![]
            };
            modrinth::project_versions(launcher, &project_id, &loaders, &[inst.game_version.as_str()]).await
        }
        None => modrinth::project_versions(launcher, &project_id, &[], &[]).await,
    }
}

#[tauri::command]
pub async fn install_modrinth_version(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    version_id: String,
) -> Result<Vec<String>> {
    let inst = instance::get(&state.launcher, &instance_id).await?;
    let installed = modrinth::install_version(&state.launcher, &inst, &version_id).await?;
    changed(&app, "content-changed");
    Ok(installed)
}

/// Installs a Modrinth modpack version as a new instance.
#[tauri::command]
pub async fn install_modrinth_modpack(
    app: AppHandle,
    state: State<'_, AppState>,
    version_id: String,
    job_id: String,
) -> Result<ImportResult> {
    let launcher = &state.launcher;
    let version = modrinth::version(launcher, &version_id).await?;
    let project = modrinth::project(launcher, &version.project_id).await?;
    let file = version
        .primary_file()
        .ok_or_else(|| Error::NotFound("modpack file".into()))?;
    let progress = progress_emitter(&app, &job_id, None);
    let mut result = modpack::import_url(launcher, &file.url, &progress).await?;
    result.instance.name = project.title.clone();
    result.instance.icon = project.icon_url.clone();
    result.instance.linked_pack = Some(LinkedPack {
        source: PackSource::Modrinth,
        project_id: Some(project.id),
        version_id: Some(version.id),
        name: Some(project.title),
        version_name: Some(version.version_number),
    });
    result.instance.save(launcher).await?;
    changed(&app, "instances-changed");
    let id = result.instance.id.clone();
    run_install(&app, &state, &id, &job_id, false).await?;
    Ok(result)
}

/// Imports a .mrpack / CurseForge zip from a local path or an https URL, then installs it.
#[tauri::command]
pub async fn import_modpack(app: AppHandle, state: State<'_, AppState>, source: String, job_id: String) -> Result<ImportResult> {
    let launcher = &state.launcher;
    let progress = progress_emitter(&app, &job_id, None);
    let result = if source.starts_with("http://") || source.starts_with("https://") {
        modpack::import_url(launcher, &source, &progress).await?
    } else {
        modpack::import_file(launcher, std::path::Path::new(&source), &progress).await?
    };
    changed(&app, "instances-changed");
    let id = result.instance.id.clone();
    run_install(&app, &state, &id, &job_id, false).await?;
    Ok(result)
}

#[tauri::command]
pub async fn list_content(state: State<'_, AppState>, instance_id: String) -> Result<Vec<InstalledContent>> {
    let inst = instance::get(&state.launcher, &instance_id).await?;
    content::list_installed(&state.launcher, &inst).await
}

#[tauri::command]
pub async fn set_content_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    path: String,
    enabled: bool,
) -> Result<()> {
    let inst = instance::get(&state.launcher, &instance_id).await?;
    content::set_enabled(&state.launcher, &inst, &path, enabled).await?;
    changed(&app, "content-changed");
    Ok(())
}

#[tauri::command]
pub async fn remove_content(app: AppHandle, state: State<'_, AppState>, instance_id: String, path: String) -> Result<()> {
    let inst = instance::get(&state.launcher, &instance_id).await?;
    content::remove(&state.launcher, &inst, &path).await?;
    changed(&app, "content-changed");
    Ok(())
}
