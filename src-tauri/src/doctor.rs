//! The crash doctor's commands: the last diagnosis, its fixes, and the bisect that starts the
//! game with halves of the mods off to find the one behind a crash.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use alloy_core::bisect::{self, Found, Graph, Outcome, Step};
use alloy_core::content::scan;
use alloy_core::crash::{self, CrashFix, Diagnosis};
use alloy_core::instance::{self, Instance};
use alloy_core::launch::{self, LaunchOptions};
use alloy_core::progress::{Progress, Stage, Unit};
use alloy_core::{install, java, system, Error, Launcher, Result};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::commands::{changed, progress_emitter};
use crate::jobs;
use crate::state::AppState;

/// Lines of game output kept for the doctor.
pub const KEEP_LINES: usize = 3000;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CrashEvent<'a> {
    instance_id: &'a str,
    diagnosis: &'a Diagnosis,
}

/// After a run: reads what it left behind and, if it crashed, saves and announces a diagnosis.
pub async fn after_exit(app: &AppHandle, inst: &Instance, started: i64, exit_code: Option<i32>, output: Vec<String>) {
    let state = app.state::<AppState>();
    let launcher = &state.launcher;
    let evidence = crash::gather(launcher, inst, started, exit_code, output).await;
    if exit_code == Some(0) && evidence.crash_report.is_none() {
        crash::forget(launcher, inst).await;
        return;
    }
    let files = scan::scan(launcher, inst).await.unwrap_or_default();
    let memory = memory_of(launcher, inst).await;
    let max = system::memory().map(|m| m.max_game_mb as u32).unwrap_or(memory);
    let d = crash::diagnose(&evidence, &files, memory, max, chrono::Utc::now().timestamp());
    tracing::info!("crash doctor: {:?} {}", d.kind, d.title);
    let _ = crash::save(launcher, inst, &d).await;
    let _ = app.emit("crash-diagnosis", CrashEvent { instance_id: &inst.id, diagnosis: &d });
}

async fn memory_of(launcher: &Launcher, inst: &Instance) -> u32 {
    match inst.memory_mb {
        Some(m) => m,
        None => launcher.settings.read().await.default_memory_mb,
    }
}

#[tauri::command]
pub async fn crash_last(state: State<'_, AppState>, instance_id: String) -> Result<Option<Diagnosis>> {
    let inst = instance::get(&state.launcher, &instance_id).await?;
    Ok(crash::last(&state.launcher, &inst).await)
}

#[tauri::command]
pub async fn crash_dismiss(state: State<'_, AppState>, instance_id: String) -> Result<()> {
    let inst = instance::get(&state.launcher, &instance_id).await?;
    crash::forget(&state.launcher, &inst).await;
    Ok(())
}

/// Mojang's runtime component for a Java major version.
fn component_for(major: u32) -> Option<&'static str> {
    Some(match major {
        8 => "jre-legacy",
        16 => "java-runtime-alpha",
        17 => "java-runtime-gamma",
        21 => "java-runtime-delta",
        25 => "java-runtime-epsilon",
        _ => return None,
    })
}

#[tauri::command]
pub async fn crash_apply_fix(app: AppHandle, state: State<'_, AppState>, instance_id: String, fix: CrashFix) -> Result<()> {
    let launcher = &state.launcher;
    let mut inst = instance::get(launcher, &instance_id).await?;
    crate::packs::auto_savepoint(&app, &inst, "Before the crash doctor's fix").await;
    match fix {
        CrashFix::DisableMod { path } => {
            alloy_core::content::set_enabled(launcher, &inst, &path, false).await?;
            changed(&app, "content-changed");
        }
        CrashFix::RaiseMemory { mb } => {
            inst.memory_mb = Some(mb);
            inst.save(launcher).await?;
            changed(&app, "instances-changed");
        }
        CrashFix::UseJava { major } => {
            let installed = java::detect_all(launcher).await.into_iter().find(|j| j.major == Some(major)).map(|j| j.path);
            let path = match installed {
                Some(p) => p,
                None => {
                    let component = component_for(major)
                        .ok_or_else(|| Error::other(format!("Install Java {major}, then choose it in this instance's settings.")))?;
                    java::ensure(launcher, component, major, &alloy_core::progress::noop()).await?
                }
            };
            inst.java_path = Some(path);
            inst.save(launcher).await?;
            changed(&app, "instances-changed");
        }
    }
    crash::forget(launcher, &inst).await;
    Ok(())
}

// ---------- bisect ----------

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BisectEvent<'a> {
    instance_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    step: Option<&'a Step>,
    #[serde(skip_serializing_if = "Option::is_none")]
    found: Option<&'a Found>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Puts the mods back however the bisect ends, cancelled included.
struct Restore {
    dir: std::path::PathBuf,
    armed: bool,
    app: AppHandle,
    id: String,
}

impl Drop for Restore {
    fn drop(&mut self) {
        if self.armed {
            bisect::restore_blocking(&self.dir);
        }
        self.app.state::<AppState>().bisecting.lock().unwrap().remove(&self.id);
        changed(&self.app, "content-changed");
    }
}

/// Kills a test game if the bisect is dropped while it runs.
struct KillOnDrop(Option<tokio::process::Child>);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        if let Some(c) = self.0.as_mut() {
            let _ = c.start_kill();
        }
    }
}

/// How long a test start may take to reach the title screen before it counts as started.
const TRIAL_LIMIT: Duration = Duration::from_secs(600);
/// After the title screen, how long it must keep running to count as started.
const SETTLE: Duration = Duration::from_secs(8);

#[tauri::command]
pub async fn bisect_start(app: AppHandle, state: State<'_, AppState>, instance_id: String) -> Result<Found> {
    if state.running.lock().unwrap().contains_key(&instance_id) {
        return Err(Error::other("Close the game first"));
    }
    if !state.bisecting.lock().unwrap().insert(instance_id.clone()) {
        return Err(Error::other("Already looking for the mod"));
    }
    let job_id = format!("bisect:{instance_id}");
    let inst = match instance::get(&state.launcher, &instance_id).await {
        Ok(i) => i,
        Err(e) => {
            state.bisecting.lock().unwrap().remove(&instance_id);
            return Err(e);
        }
    };
    let title = format!("Finding the mod that crashes {}", inst.name);
    let a = app.clone();
    let id = instance_id.clone();
    let result = jobs::run(&app, &job_id.clone(), title, None, None, async move { run_bisect(a, id, job_id).await }).await;
    state.bisecting.lock().unwrap().remove(&instance_id);
    let found = result.as_ref().ok();
    let _ = app.emit(
        "bisect-progress",
        BisectEvent { instance_id: &instance_id, step: None, found, error: result.as_ref().err().map(|e| e.to_string()) },
    );
    result
}

async fn run_bisect(app: AppHandle, id: String, job_id: String) -> Result<Found> {
    let state = app.state::<AppState>();
    let launcher = state.launcher.clone();
    let inst = instance::get(&launcher, &id).await?;
    let mut guard = Restore { dir: inst.dir(&launcher), armed: false, app: app.clone(), id: id.clone() };

    let account_id = match inst.account.clone() {
        Some(a) => a,
        None => state.accounts.list().await?.1.ok_or_else(|| Error::other("Add an account before playing"))?,
    };
    let session = state.accounts.session(&account_id).await?;
    let resolved = install::resolve(&launcher, &inst).await?;
    let cmd = launch::prepare(&launcher, &inst, &resolved, &session, &LaunchOptions::default()).await?;

    let files = scan::scan(&launcher, &inst).await?;
    let graph = Graph::new(&files);
    if graph.mods.len() < 2 {
        return Err(Error::other("There are too few mods turned on to test them in halves."));
    }
    bisect::begin(&launcher, &inst, &graph).await?;
    guard.armed = true;

    let progress = progress_emitter(&app, &job_id, None);
    let step_app = app.clone();
    let step_id = id.clone();
    let on_step = move |s: &Step| {
        progress(Progress {
            stage: Stage::Starting,
            current: s.trial as u64 - 1,
            total: s.estimate as u64,
            unit: Unit::Steps,
            message: Some(format!("Start {} of about {}: {} mods on, {} suspects", s.trial, s.estimate, s.enabled, s.suspects)),
            files: None,
        });
        let _ = step_app.emit("bisect-progress", BisectEvent { instance_id: &step_id, step: Some(s), found: None, error: None });
    };

    let trial = |on: BTreeSet<String>| {
        let (launcher, inst, graph, cmd, app) = (&launcher, &inst, &graph, &cmd, &app);
        async move {
            bisect::apply(launcher, inst, graph, &on).await?;
            run_trial(app, &inst.id, cmd).await
        }
    };
    let found = bisect::run(&graph, trial, on_step).await;

    // Back the way it was, then let the guard stand down.
    bisect::restore(&launcher, &inst).await?;
    guard.armed = false;
    let found = found?;
    tracing::info!("bisect for {}: {:?} in {} starts", inst.name, found.names, found.trials);
    Ok(found)
}

/// Starts the game once and watches it: a crash before the title screen, or the title screen.
async fn run_trial(app: &AppHandle, instance_id: &str, cmd: &launch::LaunchCommand) -> Result<Outcome> {
    run_trial_kept(app, instance_id, cmd, None).await.map(|(o, _)| o)
}

/// [`run_trial`], keeping the game's last lines in `keep` and returning the exit code of a crash.
async fn run_trial_kept(
    app: &AppHandle,
    instance_id: &str,
    cmd: &launch::LaunchCommand,
    keep: Option<Arc<std::sync::Mutex<std::collections::VecDeque<String>>>>,
) -> Result<(Outcome, Option<i32>)> {
    let (menu_tx, mut menu_rx) = tokio::sync::watch::channel(false);
    let game_log = crate::commands::GameLog::start(app, instance_id);
    let child = launch::spawn(
        cmd,
        Arc::new(move |line| {
            if crash::reached_menu(&line.line) {
                let _ = menu_tx.send(true);
            }
            if let Some(k) = &keep {
                let mut t = k.lock().unwrap();
                if t.len() >= KEEP_LINES {
                    t.pop_front();
                }
                t.push_back(line.line.clone());
            }
            game_log.push(&line);
        }),
    )?;
    let mut child = KillOnDrop(Some(child));
    let c = child.0.as_mut().unwrap();
    let outcome = tokio::select! {
        status = c.wait() => {
            // It ended before the title screen: crashed (even "exit 0" there means it gave up).
            (Outcome::Crashed, Some(status.ok().and_then(|s| s.code()).unwrap_or(-1)))
        }
        _ = async { let _ = menu_rx.wait_for(|v| *v).await; } => {
            // Made it; give it a moment in case it falls over right after.
            match tokio::time::timeout(SETTLE, c.wait()).await {
                Ok(Ok(s)) if !s.success() => (Outcome::Crashed, Some(s.code().unwrap_or(-1))),
                _ => (Outcome::Started, None),
            }
        }
        _ = tokio::time::sleep(TRIAL_LIMIT) => (Outcome::Started, None),
    };
    if let Some(mut c) = child.0.take() {
        let _ = c.kill().await;
    }
    Ok(outcome)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestLaunch {
    pub started: bool,
    pub diagnosis: Option<Diagnosis>,
}

/// Starts the instance once to see whether it reaches the title screen, then closes it. A
/// crash gets the crash doctor's diagnosis, saved like any other.
#[tauri::command]
pub async fn test_launch(app: AppHandle, state: State<'_, AppState>, instance_id: String) -> Result<TestLaunch> {
    if state.running.lock().unwrap().contains_key(&instance_id) {
        return Err(Error::other("The game is running. Close it first."));
    }
    if !state.bisecting.lock().unwrap().insert(instance_id.clone()) {
        return Err(Error::other("This instance is already being tested."));
    }
    struct Done(AppHandle, String);
    impl Drop for Done {
        fn drop(&mut self) {
            self.0.state::<AppState>().bisecting.lock().unwrap().remove(&self.1);
        }
    }
    let _done = Done(app.clone(), instance_id.clone());
    let launcher = state.launcher.clone();
    let mut inst = instance::get(&launcher, &instance_id).await?;
    let title = format!("Test-starting {}", inst.name);
    let job_id = format!("test:{instance_id}");
    let a = app.clone();
    jobs::run(&app, &job_id.clone(), title, Some(instance_id.clone()), None, async move {
        let state = a.state::<AppState>();
        let account_id = match inst.account.clone() {
            Some(x) => x,
            None => state.accounts.list().await?.1.ok_or_else(|| Error::other("Add an account before playing"))?,
        };
        let session = state.accounts.session(&account_id).await?;
        if inst.install_state != instance::InstallState::Installed {
            crate::commands::run_install(&a, &inst.id, &job_id, false).await?;
            inst = instance::get(&launcher, &inst.id).await?;
        }
        let resolved = install::resolve(&launcher, &inst).await?;
        let cmd = launch::prepare(&launcher, &inst, &resolved, &session, &LaunchOptions::default()).await?;
        let progress = progress_emitter(&a, &job_id, None);
        progress(Progress { stage: Stage::Starting, current: 0, total: 1, unit: Unit::Steps, message: Some("Waiting for the title screen".into()), files: None });
        let keep: Arc<std::sync::Mutex<std::collections::VecDeque<String>>> = Arc::default();
        let started = chrono::Utc::now().timestamp();
        let (outcome, code) = run_trial_kept(&a, &inst.id, &cmd, Some(keep.clone())).await?;
        if matches!(outcome, Outcome::Started) {
            crash::forget(&launcher, &inst).await;
            return Ok(TestLaunch { started: true, diagnosis: None });
        }
        let lines: Vec<String> = keep.lock().unwrap().drain(..).collect();
        after_exit(&a, &inst, started, code, lines).await;
        Ok(TestLaunch { started: false, diagnosis: crash::last(&launcher, &inst).await })
    })
    .await
}

/// Cleans up after a bisect Alloy was closed during.
pub fn recover_on_start(launcher: Launcher) {
    tauri::async_runtime::spawn(async move { bisect::recover(&launcher).await });
}
