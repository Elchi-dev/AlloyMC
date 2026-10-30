//! The "Sign in with Microsoft" window.
//!
//! Opens Microsoft's login page in a private webview, watches every navigation, and
//! returns the auth code as soon as the page redirects to the desktop redirect URI.
//! No local web server and no copy-pasting codes.

use std::sync::{Arc, Mutex};

use alloy_core::auth::microsoft::MsClient;
use alloy_core::auth::AuthError;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tokio::sync::oneshot;

const LABEL: &str = "ms-login";

type Reply = Arc<Mutex<Option<oneshot::Sender<Result<String, AuthError>>>>>;

fn send(reply: &Reply, value: Result<String, AuthError>) {
    if let Some(tx) = reply.lock().unwrap().take() {
        let _ = tx.send(value);
    }
}

pub async fn get_code(app: &AppHandle, client: &MsClient) -> Result<String, AuthError> {
    if let Some(old) = app.get_webview_window(LABEL) {
        let _ = old.close();
    }

    let (tx, rx) = oneshot::channel();
    let reply: Reply = Arc::new(Mutex::new(Some(tx)));
    let url = client.authorize_url().parse().map_err(|_| AuthError::Microsoft("bad authorize url".into()))?;

    let nav_client = client.clone();
    let nav_reply = reply.clone();
    let load_client = client.clone();
    let load_reply = reply.clone();

    let mut builder = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::External(url))
        .title("Sign in with Microsoft")
        .inner_size(500.0, 680.0)
        .resizable(false)
        .center()
        // Fresh cookies every time, so "add another account" doesn't reuse the last one.
        .incognito(true)
        .on_navigation(move |u| match nav_client.parse_redirect(u.as_str()) {
            Some(result) => {
                send(&nav_reply, result);
                false
            }
            None => true,
        })
        // Backup: some platforms report the final redirect only as a page load.
        .on_page_load(move |_, payload| {
            if let Some(result) = load_client.parse_redirect(payload.url().as_str()) {
                send(&load_reply, result);
            }
        });
    if let Some(main) = app.get_webview_window("main") {
        builder = builder.parent(&main).map_err(|e| AuthError::Microsoft(e.to_string()))?;
    }
    let window = builder.build().map_err(|e| AuthError::Microsoft(e.to_string()))?;

    let close_reply = reply.clone();
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed | WindowEvent::CloseRequested { .. }) {
            send(&close_reply, Err(AuthError::Cancelled));
        }
    });

    let result = rx.await.unwrap_or(Err(AuthError::Cancelled));
    let _ = window.close();
    result
}
