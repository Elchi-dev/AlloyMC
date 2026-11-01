//! A window onto CurseForge's own website, for files whose authors only allow downloads
//! there. It opens each file's download page in turn; CurseForge's page starts the download
//! itself, Alloy catches it, checks it against CurseForge's hash, installs it, and moves on
//! to the next page. When the last file is in, the window closes.
//!
//! The page is CurseForge's, as in any browser: the person sees it, its countdown and its
//! ads, and CurseForge's page asks for the file. Alloy takes that request (the address the
//! page itself went to) and fetches it, so it doesn't depend on how each system's webview
//! handles downloads: WebView2 holds a `.jar` back as a "file that can harm your device"
//! with no one to answer, and WebKitGTK ignores files it can't show. The webview's own
//! download is caught too, as a second way in. The window has no access to Alloy's
//! commands (only the main window has capabilities), and only files from CurseForge's
//! servers are accepted.

use std::path::PathBuf;
use std::sync::Mutex;

use alloy_core::content::blocked;
use alloy_core::content::manage::{accept_blocked, BlockedFile};
use alloy_core::instance;
use alloy_core::{Error, Result};
use serde::Serialize;
use tauri::webview::DownloadEvent;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::state::AppState;

const LABEL: &str = "curseforge";

/// The files the window is working through.
#[derive(Default)]
pub struct CfQueue(Mutex<Option<Session>>);

struct Session {
    instance_id: String,
    files: Vec<BlockedFile>,
    /// Index of the file whose page is open.
    at: usize,
    done: Vec<String>,
    /// Files being fetched or already in, so a download seen twice is handled once.
    taken: std::collections::HashSet<u64>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress {
    instance_id: String,
    done: usize,
    total: usize,
    current: Option<String>,
    error: Option<String>,
    finished: bool,
}

fn emit(app: &AppHandle, s: &Session, error: Option<String>, finished: bool) {
    let _ = app.emit(
        "curseforge-window",
        Progress {
            instance_id: s.instance_id.clone(),
            done: s.done.len(),
            total: s.files.len(),
            current: s.files.get(s.at).map(|f| f.title.clone()),
            error,
            finished,
        },
    );
}

fn title(s: &Session) -> String {
    match s.files.get(s.at) {
        Some(f) => format!("{} ({} of {}) · CurseForge · Alloy", f.title, s.at + 1, s.files.len()),
        None => "CurseForge · Alloy".into(),
    }
}

fn staging(app: &AppHandle) -> PathBuf {
    app.state::<AppState>().launcher.dirs.root().join("downloads").join("curseforge")
}

/// CurseForge serves files from its own CDN; nothing else may be saved through this window.
fn allowed(url: &tauri::Url) -> bool {
    url.scheme() == "https"
        && url.host_str().is_some_and(|h| h.ends_with(".forgecdn.net") || h == "forgecdn.net" || h.ends_with(".curseforge.com") || h == "curseforge.com")
}

/// Opens (or refocuses) the window for an instance's waiting files.
#[tauri::command]
pub async fn curseforge_window(app: AppHandle, instance_id: String) -> Result<()> {
    open(&app, instance_id).await
}

pub async fn open(app: &AppHandle, instance_id: String) -> Result<()> {
    let app = app.clone();
    let state = app.state::<AppState>();
    let queue = app.state::<CfQueue>();
    let inst = instance::get(&state.launcher, &instance_id).await?;
    let files = blocked::pending(&state.launcher, &inst).await;
    let Some(first) = files.first().cloned() else {
        return Err(Error::other("Nothing is waiting for CurseForge"));
    };
    tokio::fs::create_dir_all(staging(&app)).await?;
    let session = Session { instance_id, files, at: 0, done: vec![], taken: Default::default() };
    let window_title = title(&session);
    let page: tauri::Url = first.page_url.parse().map_err(|_| Error::other("CurseForge gave a page address Alloy can't open"))?;
    emit(&app, &session, None, false);
    *queue.0.lock().unwrap() = Some(session);

    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.navigate(page);
        let _ = w.set_title(&window_title);
        let _ = w.unminimize();
        let _ = w.set_focus();
        return Ok(());
    }

    let handler_app = app.clone();
    let window = WebviewWindowBuilder::new(&app, LABEL, WebviewUrl::External(page))
        .title(window_title)
        .inner_size(1120.0, 820.0)
        .min_inner_size(720.0, 520.0)
        .center()
        .on_download(move |_webview, event| on_download(&handler_app, event))
        .on_navigation({
            let app = app.clone();
            move |url| on_navigation(&app, url)
        })
        .build()
        .map_err(|e| Error::other(format!("Couldn't open the CurseForge window: {e}")))?;
    // WebKitGTK shows or ignores responses it can't display unless told to download them;
    // WebView2 (Windows) downloads them by itself.
    #[cfg(target_os = "linux")]
    let _ = window.with_webview(|wv| {
        use webkit2gtk::{PolicyDecisionType, ResponsePolicyDecision, URIResponseExt, WebViewExt};
        use webkit2gtk::glib::prelude::Cast;
        use webkit2gtk::PolicyDecisionExt;
        use webkit2gtk::ResponsePolicyDecisionExt;
        wv.inner().connect_decide_policy(|_, decision, kind| {
            if kind != PolicyDecisionType::Response {
                return false;
            }
            let Some(r) = decision.dynamic_cast_ref::<ResponsePolicyDecision>() else { return false };
            let attachment = r
                .response()
                .and_then(|resp| resp.http_headers())
                .and_then(|h| h.one("Content-Disposition"))
                .is_some_and(|v| v.to_ascii_lowercase().starts_with("attachment"));
            if attachment || !r.is_mime_type_supported() {
                r.download();
                return true;
            }
            false
        });
    });
    let closed_app = app.clone();
    window.on_window_event(move |e| {
        if let tauri::WindowEvent::Destroyed = e {
            let q = closed_app.state::<CfQueue>();
            let left = q.0.lock().unwrap().take();
            if let Some(s) = left {
                emit(&closed_app, &s, None, true);
            }
        }
    });
    Ok(())
}

/// A file address the page navigates to: CurseForge's download endpoint or its file servers.
fn is_file(url: &tauri::Url) -> bool {
    if !allowed(url) {
        return false;
    }
    let host = url.host_str().unwrap_or_default();
    let path = url.path();
    host.ends_with("forgecdn.net") || (path.starts_with("/api/v1/mods/") && path.ends_with("/download"))
}

/// The page asks for the file: take the request and fetch it ourselves.
fn on_navigation(app: &AppHandle, url: &tauri::Url) -> bool {
    if !is_file(url) {
        return true;
    }
    let Some(file_id) = current_file(app) else { return true };
    tracing::info!("CurseForge window: the page asked for {url}");
    let (app, url) = (app.clone(), url.to_string());
    tauri::async_runtime::spawn(async move {
        let path = staging(&app).join(format!("{file_id}.part"));
        let state = app.state::<AppState>();
        let got = alloy_core::download::download_with_retry(&state.launcher.http, &alloy_core::download::DownloadTask::new(&url, &path)).await;
        if let Err(e) = &got {
            tracing::warn!("CurseForge window: fetching {url} failed: {e}");
        }
        finished(app, file_id, Some(path), got.is_ok()).await;
    });
    false
}

fn current_file(app: &AppHandle) -> Option<u64> {
    let q = app.state::<CfQueue>();
    let guard = q.0.lock().unwrap();
    let s = guard.as_ref()?;
    s.files.get(s.at).map(|f| f.file_id)
}

fn on_download(app: &AppHandle, event: DownloadEvent<'_>) -> bool {
    match event {
        DownloadEvent::Requested { url, destination } => {
            tracing::info!("CurseForge window: download requested from {url}");
            if !allowed(&url) {
                tracing::warn!("CurseForge window: refused a download from {url}");
                return false;
            }
            let Some(file_id) = current_file(app) else { return false };
            *destination = staging(app).join(format!("{file_id}.download"));
            true
        }
        DownloadEvent::Finished { url, path, success } => {
            tracing::info!("CurseForge window: download of {url} finished (ok: {success}) at {path:?}");
            // WebKitGTK doesn't always ask first (Requested), so the source is checked here too.
            let ok = success && allowed(&url);
            if let Some(file_id) = current_file(app) {
                let app = app.clone();
                tauri::async_runtime::spawn(async move { finished(app, file_id, path, ok).await });
            }
            true
        }
        _ => true,
    }
}

async fn finished(app: AppHandle, file_id: u64, path: Option<PathBuf>, success: bool) {
    let (instance_id, file) = {
        let q = app.state::<CfQueue>();
        let mut guard = q.0.lock().unwrap();
        let Some(s) = guard.as_mut() else { return };
        let Some(f) = s.files.iter().find(|f| f.file_id == file_id).cloned() else { return };
        // Already fetched, or being fetched, another way.
        if success && !s.taken.insert(file_id) {
            if let Some(p) = &path {
                let p = p.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = tokio::fs::remove_file(p).await;
                });
            }
            return;
        }
        (s.instance_id.clone(), f)
    };
    let Some(path) = path else {
        tracing::warn!("CurseForge window: {} finished without a file", file.file_name);
        return;
    };
    let state = app.state::<AppState>();
    let result = async {
        if !success {
            return Err(Error::other("The download didn't finish"));
        }
        let inst = instance::get(&state.launcher, &instance_id).await?;
        accept_blocked(&state.launcher, &inst, &file, &path).await?;
        blocked::dismiss(&state.launcher, &inst, file.file_id).await
    }
    .await;
    let _ = tokio::fs::remove_file(&path).await;

    let q = app.state::<CfQueue>();
    let mut guard = q.0.lock().unwrap();
    let Some(s) = guard.as_mut() else { return };
    match result {
        Ok(()) => {
            tracing::info!("CurseForge window: {} is in", file.file_name);
            s.done.push(file.title.clone());
            let _ = app.emit("blocked-changed", serde_json::json!({ "instanceId": instance_id }));
            s.at = s.files.iter().position(|f| !s.taken.contains(&f.file_id)).unwrap_or(s.files.len());
            let window = app.get_webview_window(LABEL);
            match s.files.get(s.at) {
                Some(next) => {
                    emit(&app, s, None, false);
                    if let (Some(w), Ok(url)) = (window, next.page_url.parse::<tauri::Url>()) {
                        let _ = w.set_title(&title(s));
                        let _ = w.navigate(url);
                    }
                }
                None => {
                    emit(&app, s, None, true);
                    let _ = app.emit("blocked-collected", serde_json::json!({ "instanceId": instance_id, "titles": s.done }));
                    *guard = None;
                    if let Some(w) = window {
                        let _ = w.close();
                    }
                }
            }
        }
        Err(e) => {
            s.taken.remove(&file.file_id);
            tracing::warn!("CurseForge window: {} didn't check out: {e}", file.file_name);
            emit(&app, s, Some(format!("{} didn't match CurseForge's file. Press \"try again\" on the page.", file.title)), false);
        }
    }
}
