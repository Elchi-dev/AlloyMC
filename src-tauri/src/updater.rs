//! One-click updates from a local `.alloyupdate` file.
//!
//! Flow: find (Downloads, <data>/updates, a picked or dropped file) → verify signatures →
//! back up Alloy's records → stage the installer → write a pending marker → start the
//! installer in update mode and quit → the installer restarts Alloy → on start, the marker
//! tells the UI whether the update landed.
//!
//! With "auto update" on (the default), Alloy also asks AlloyMC for the newest release a
//! little after starting, hourly and when its window comes back to the front, downloads it into `<data>/updates` and offers
//! it. Installing always waits for a click.

use std::path::{Path, PathBuf};

use alloy_core::updates::{self, FoundUpdate, Pending, UpdateOutcome};
use alloy_core::{Error, Result};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::state::AppState;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

fn updates_dir(state: &AppState) -> PathBuf {
    state.launcher.dirs.root().join("updates")
}

/// Where Alloy looks for update files without being asked.
fn search_dirs(state: &AppState) -> Vec<PathBuf> {
    let mut dirs = vec![updates_dir(state)];
    // XDG may not define a Downloads dir on Linux; ~/Downloads is the common default.
    if let Some(d) = dirs::download_dir().or_else(|| dirs::home_dir().map(|h| h.join("Downloads"))) {
        dirs.push(d);
    }
    dirs
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tokio::task::spawn_blocking(f).await.map_err(|e| Error::other(e.to_string()))?
}

/// The newest verified update that is newer than this build, if any.
#[tauri::command]
pub async fn check_updates(state: State<'_, AppState>) -> Result<Option<FoundUpdate>> {
    let dirs = search_dirs(&state);
    blocking(move || Ok(updates::scan(&dirs, VERSION))).await
}

/// Builds that can replace themselves. A Linux build installed by a package manager can't.
fn can_self_update() -> bool {
    !cfg!(target_os = "linux") || std::env::var_os("APPIMAGE").is_some()
}

/// Asks AlloyMC for a newer version and downloads it. `None` when there's nothing newer.
async fn fetch_update(app: &AppHandle) -> Result<Option<FoundUpdate>> {
    if !can_self_update() {
        return Ok(None);
    }
    let state = app.state::<AppState>();
    let http = state.launcher.http.clone();
    let Some(offer) = updates::check_remote(&http, &updates::latest_url(), VERSION).await? else {
        return Ok(None);
    };
    tracing::info!("Alloy {} is out; downloading it", offer.version);
    let found = updates::download(&http, &offer, &updates_dir(&state), VERSION).await?;
    Ok(Some(found))
}

/// "Check now" in Settings.
#[tauri::command]
pub async fn update_check_now(app: AppHandle) -> Result<Option<FoundUpdate>> {
    let found = fetch_update(&app).await?;
    if let Some(f) = &found {
        let _ = app.emit("update-ready", f);
    }
    Ok(found)
}

/// When the last automatic check ran (Unix seconds).
static LAST_CHECK: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

/// One automatic check, if auto update is on and the last one is at least `gap` seconds old.
async fn auto_check(app: &AppHandle, gap: i64) {
    let now = chrono::Utc::now().timestamp();
    let last = LAST_CHECK.load(std::sync::atomic::Ordering::Relaxed);
    if now - last < gap || !app.state::<AppState>().launcher.settings.read().await.auto_update {
        return;
    }
    LAST_CHECK.store(now, std::sync::atomic::Ordering::Relaxed);
    match fetch_update(app).await {
        Ok(Some(f)) => {
            let _ = app.emit("update-ready", &f);
        }
        Ok(None) => {}
        Err(e) => tracing::info!("update check failed: {e}"),
    }
}

/// Checks a little after start, then hourly, while auto update is on.
pub fn start_auto(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(20)).await;
        loop {
            auto_check(&app, 0).await;
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        }
    });
}

/// The window came back to the front: check again if the last check is 15 minutes old.
#[tauri::command]
pub async fn update_poke(app: AppHandle) {
    auto_check(&app, 15 * 60).await;
}

/// Verifies a specific file (picked in Settings or dropped on the window).
#[tauri::command]
pub async fn inspect_update(path: PathBuf) -> Result<FoundUpdate> {
    blocking(move || updates::inspect(&path, VERSION)).await
}

/// What happened to the last update, once, right after Alloy starts.
#[tauri::command]
pub fn update_outcome(state: State<'_, AppState>) -> Option<UpdateOutcome> {
    updates::take_outcome(&updates_dir(&state), VERSION)
}

/// Verifies again, backs up, stages, hands over to the installer and quits.
#[tauri::command]
pub async fn apply_update(app: AppHandle, state: State<'_, AppState>, path: PathBuf, allow_downgrade: Option<bool>) -> Result<()> {
    if !state.running.lock().unwrap().is_empty() {
        return Err(Error::other("Close Minecraft first; the update restarts Alloy"));
    }
    let root = state.launcher.dirs.root().to_path_buf();
    let dir = updates_dir(&state);
    let found = blocking(move || updates::inspect(&path, VERSION)).await?;
    if !found.newer && !allow_downgrade.unwrap_or(false) {
        return Err(Error::other(format!("Alloy {} is already installed", VERSION)));
    }

    let staged_dir = dir.join("staging");
    let manifest = found.manifest.clone();
    let (installer, backup) = blocking(move || {
        let backup = updates::backup(&root, VERSION)?;
        let installer = updates::stage(&found, &staged_dir)?;
        Ok((installer, backup))
    })
    .await?;

    updates::write_pending(
        &dir,
        &Pending {
            from: VERSION.into(),
            to: manifest.version.clone(),
            notes: manifest.notes.clone(),
            started_at: chrono::Utc::now().timestamp(),
            backup,
        },
    )?;

    launch_installer(&manifest.kind, &installer)?;
    tracing::info!("handing over to the {} installer for {}", manifest.kind, manifest.version);
    // Let the command's reply reach the UI, then get out of the installer's way.
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        handle.exit(0);
    });
    Ok(())
}

#[cfg(windows)]
fn launch_installer(kind: &str, installer: &Path) -> Result<()> {
    use std::os::windows::process::CommandExt;
    if kind != "nsis" {
        return Err(Error::other(format!("Can't apply a {kind} update on Windows")));
    }
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    // /P passive (progress only, no questions), /UPDATE keep settings and shortcuts,
    // /R start Alloy again when done.
    std::process::Command::new(installer)
        .args(["/P", "/UPDATE", "/R"])
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn()?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn launch_installer(kind: &str, staged: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    // Only the AppImage can replace itself; the Arch package updates through pacman.
    let Some(current) = std::env::var_os("APPIMAGE").map(PathBuf::from) else {
        return Err(Error::other("This Alloy was installed by your package manager; update it there (makepkg -si)"));
    };
    if kind != "appimage" {
        return Err(Error::other(format!("Can't apply a {kind} update to an AppImage")));
    }
    let next = current.with_extension("AppImage.new");
    std::fs::copy(staged, &next)?;
    std::fs::set_permissions(&next, std::fs::Permissions::from_mode(0o755))?;
    std::fs::rename(&next, &current)?; // atomic on the same filesystem
    std::process::Command::new(&current).spawn()?;
    Ok(())
}

#[cfg(not(any(windows, target_os = "linux")))]
fn launch_installer(kind: &str, _: &Path) -> Result<()> {
    Err(Error::other(format!("Can't apply a {kind} update on this platform yet")))
}
