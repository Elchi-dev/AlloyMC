//! A window onto CurseForge's own website, for files whose authors only allow downloads
//! there. It opens each file's download page in turn; CurseForge's page starts the download
//! itself, Alloy catches it, checks it against CurseForge's hash, installs it, and moves on
//! to the next page. When the last file is in, the window closes.
//!
//! The page is CurseForge's, as in any browser: the person sees it, its countdown and its
//! ads, and CurseForge starts the download. The window has no access to Alloy's commands
//! (only the main window has capabilities), and only downloads from CurseForge's file
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
    let session = Session { instance_id, files, at: 0, done: vec![] };
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

fn on_download(app: &AppHandle, event: DownloadEvent<'_>) -> bool {
    match event {
        DownloadEvent::Requested { url, destination } => {
            tracing::info!("CurseForge window: download requested from {url}");
            if !allowed(&url) {
                tracing::warn!("CurseForge window: refused a download from {url}");
                return false;
            }
            let q = app.state::<CfQueue>();
            let guard = q.0.lock().unwrap();
            let Some(s) = guard.as_ref() else { return false };
            let Some(f) = s.files.get(s.at) else { return false };
            *destination = staging(app).join(&f.file_name);
            true
        }
        DownloadEvent::Finished { url, path, success } => {
            tracing::info!("CurseForge window: download of {url} finished (ok: {success}) at {path:?}");
            // WebKitGTK doesn't always ask first (Requested), so the source is checked here too.
            let ok = success && allowed(&url);
            let app = app.clone();
            tauri::async_runtime::spawn(async move { finished(app, path, ok).await });
            true
        }
        _ => true,
    }
}

async fn finished(app: AppHandle, path: Option<PathBuf>, success: bool) {
    let (instance_id, file) = {
        let q = app.state::<CfQueue>();
        let guard = q.0.lock().unwrap();
        let Some(s) = guard.as_ref() else { return };
        let Some(f) = s.files.get(s.at).cloned() else { return };
        (s.instance_id.clone(), f)
    };
    // macOS doesn't report the path; it's where Requested put it.
    let path = path.unwrap_or_else(|| staging(&app).join(&file.file_name));
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
            s.done.push(file.title.clone());
            let _ = app.emit("blocked-changed", serde_json::json!({ "instanceId": instance_id }));
            s.at += 1;
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
            tracing::warn!("CurseForge window: {} didn't check out: {e}", file.file_name);
            emit(&app, s, Some(format!("{} didn't match CurseForge's file. Try its download again.", file.title)), false);
        }
    }
}
