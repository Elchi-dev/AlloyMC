//! Save points, exporting modpacks, and importing from other launchers.

use std::path::PathBuf;

use alloy_core::export::{self, Candidate, Options, Report};
use alloy_core::importers::{self, Found};
use alloy_core::instance::{self, Instance};
use alloy_core::savepoint::{self, SavePoint};
use alloy_core::Result;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::commands::{changed, progress_emitter};
use crate::jobs;
use crate::state::AppState;

fn not_running(state: &AppState, id: &str) -> Result<()> {
    if state.running.lock().unwrap().contains_key(id) || state.bisecting.lock().unwrap().contains(id) {
        return Err(alloy_core::Error::other("Close the game first"));
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavePoints {
    points: Vec<SavePoint>,
    /// What they take on disk, shared files counted once.
    disk_bytes: u64,
}

#[tauri::command]
pub async fn savepoints(state: State<'_, AppState>, instance_id: String) -> Result<SavePoints> {
    let points = savepoint::list(&state.launcher, &instance_id).await;
    let disk_bytes = savepoint::disk_usage(&state.launcher, &instance_id).await;
    Ok(SavePoints { points, disk_bytes })
}

#[tauri::command]
pub async fn savepoint_create(app: AppHandle, state: State<'_, AppState>, instance_id: String, label: String) -> Result<SavePoint> {
    let inst = instance::get(&state.launcher, &instance_id).await?;
    let label = if label.trim().is_empty() { "Save point".to_string() } else { label };
    let p = savepoint::create(&state.launcher, &inst, &label, false).await?;
    changed(&app, "savepoints-changed");
    Ok(p)
}

/// Made by Alloy before a change it's about to make; failures never block the change.
pub async fn auto_savepoint(app: &AppHandle, inst: &Instance, label: &str) {
    let state = app.state::<AppState>();
    if let Err(e) = savepoint::create(&state.launcher, inst, label, true).await {
        tracing::warn!("automatic save point failed: {e}");
    }
    changed(app, "savepoints-changed");
}

#[tauri::command]
pub async fn savepoint_restore(app: AppHandle, state: State<'_, AppState>, instance_id: String, point_id: String) -> Result<Instance> {
    not_running(&state, &instance_id)?;
    let inst = savepoint::restore(&state.launcher, &instance_id, &point_id).await?;
    changed(&app, "savepoints-changed");
    changed(&app, "content-changed");
    changed(&app, "instances-changed");
    Ok(inst)
}

#[tauri::command]
pub async fn savepoint_rename(app: AppHandle, state: State<'_, AppState>, instance_id: String, point_id: String, label: String) -> Result<()> {
    savepoint::rename(&state.launcher, &instance_id, &point_id, &label).await?;
    changed(&app, "savepoints-changed");
    Ok(())
}

#[tauri::command]
pub async fn savepoint_delete(app: AppHandle, state: State<'_, AppState>, instance_id: String, point_id: String) -> Result<()> {
    savepoint::delete(&state.launcher, &instance_id, &point_id).await?;
    changed(&app, "savepoints-changed");
    Ok(())
}

// ---------- export ----------

#[tauri::command]
pub async fn export_candidates(state: State<'_, AppState>, instance_id: String) -> Result<Vec<Candidate>> {
    let inst = instance::get(&state.launcher, &instance_id).await?;
    export::candidates(&state.launcher, &inst).await
}

#[tauri::command]
pub async fn export_instance(app: AppHandle, instance_id: String, options: Options) -> Result<Report> {
    let job_id = format!("export:{instance_id}");
    let title = format!("Exporting {}", options.name);
    let (a, j) = (app.clone(), job_id.clone());
    jobs::run(&app, &job_id, title, None, None, async move {
        let state = a.state::<AppState>();
        let inst = instance::get(&state.launcher, &instance_id).await?;
        export::export(&state.launcher, &inst, &options, &progress_emitter(&a, &j, None)).await
    })
    .await
}

// ---------- import from other launchers ----------

#[tauri::command]
pub async fn import_discover() -> Vec<Found> {
    importers::discover().await
}

#[tauri::command]
pub async fn import_inspect(path: PathBuf) -> Vec<Found> {
    importers::inspect(path).await
}

#[tauri::command]
pub async fn import_instance(app: AppHandle, found: Found) -> Result<Instance> {
    let job_id = format!("import:{}", found.game_dir.to_string_lossy());
    let title = format!("Importing {} from {}", found.name, found.from.label());
    let (a, j) = (app.clone(), job_id.clone());
    let inst = jobs::run(&app, &job_id, title, None, None, async move {
        let state = a.state::<AppState>();
        importers::import(&state.launcher, &found, &progress_emitter(&a, &j, None)).await
    })
    .await?;
    changed(&app, "instances-changed");
    Ok(inst)
}

// ---------- the pack an instance came from ----------

/// The public pack this instance came from, looked up once for instances imported from a
/// file before Alloy did that on import. `None` when it isn't a pack or can't be found.
#[tauri::command]
pub async fn pack_source(app: AppHandle, state: State<'_, AppState>, instance_id: String) -> Result<Option<alloy_core::instance::LinkedPack>> {
    let mut inst = instance::get(&state.launcher, &instance_id).await?;
    let Some(mut link) = inst.linked_pack.clone() else { return Ok(None) };
    if link.project_id.is_none() {
        if !alloy_core::modpack::resolve_source(&state.launcher, &mut link, None).await {
            return Ok(None);
        }
        inst.linked_pack = Some(link.clone());
        inst.save(&state.launcher).await?;
        changed(&app, "instances-changed");
    }
    Ok(Some(link))
}
