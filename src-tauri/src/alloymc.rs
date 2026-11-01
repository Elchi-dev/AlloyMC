//! AlloyMC account commands. Signing in runs in the person's own browser (so a session they
//! already have, and Google, just work) and the code comes back to a one-shot listener on
//! 127.0.0.1 (RFC 8252): no URL scheme to register, nothing for other programs to catch.

use std::sync::Mutex;
use std::time::Duration;

use alloy_core::alloymc::{self, LinkedMinecraft, Profile};
use alloy_core::Error;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_opener::OpenerExt;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use crate::state::AppState;

type Result<T> = std::result::Result<T, Error>;

/// A sign-in waiting for the browser; dropping the sender cancels it.
#[derive(Default)]
pub struct SignIn(pub Mutex<Option<oneshot::Sender<()>>>);

fn changed(app: &AppHandle) {
    let _ = app.emit("alloymc-changed", ());
}

/// The profile Alloy last saw, instantly; `refresh` asks the server.
#[tauri::command]
pub async fn alloymc_profile(state: State<'_, AppState>, refresh: Option<bool>) -> Result<Option<Profile>> {
    if refresh.unwrap_or(false) {
        // Offline is fine: keep showing what we had.
        match state.alloymc.refresh().await {
            Ok(p) => return Ok(p),
            Err(e) => tracing::info!("alloymc refresh failed, using cache: {e}"),
        }
    }
    Ok(state.alloymc.cached().await)
}

#[tauri::command]
pub async fn alloymc_sign_in(app: AppHandle, state: State<'_, AppState>, signin: State<'_, SignIn>) -> Result<Profile> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let pending = state.alloymc.begin(&alloymc::device_name(), port);
    let (cancel_tx, mut cancel_rx) = oneshot::channel();
    *signin.0.lock().unwrap() = Some(cancel_tx);

    app.opener().open_url(&pending.url, None::<&str>).map_err(|e| Error::other(format!("Couldn't open the browser: {e}")))?;

    let done = format!("{}/auth/app/done", alloymc::site_url());
    let wait = alloymc::wait_for_code(&listener, &done);
    let outcome = tokio::select! {
        r = wait => r?,
        _ = &mut cancel_rx => return Err(Error::other("Sign-in cancelled")),
        _ = tokio::time::sleep(Duration::from_secs(600)) => return Err(Error::other("Sign-in timed out. Try again.")),
    };
    *signin.0.lock().unwrap() = None;
    let (code, got_state) = outcome.map_err(|_| Error::other("Sign-in cancelled"))?;
    if got_state != pending.state {
        return Err(Error::other("That sign-in didn't come from this Alloy. Try again."));
    }
    let profile = state.alloymc.finish(&code, &pending.verifier).await?;
    changed(&app);
    Ok(profile)
}

#[tauri::command]
pub fn alloymc_cancel_sign_in(signin: State<'_, SignIn>) {
    // Dropping the sender wakes the waiting sign-in with "cancelled".
    if let Some(tx) = signin.0.lock().unwrap().take() {
        let _ = tx.send(());
    }
}

#[tauri::command]
pub async fn alloymc_sign_out(app: AppHandle, state: State<'_, AppState>) -> Result<()> {
    state.alloymc.sign_out().await?;
    changed(&app);
    Ok(())
}

/// Links one of the Microsoft accounts in Alloy to the AlloyMC profile.
#[tauri::command]
pub async fn alloymc_link_minecraft(app: AppHandle, state: State<'_, AppState>, account_id: String) -> Result<LinkedMinecraft> {
    let session = state.accounts.session(&account_id).await?;
    let link = state.alloymc.link_minecraft(&session).await?;
    changed(&app);
    Ok(link)
}

#[tauri::command]
pub async fn alloymc_unlink_minecraft(app: AppHandle, state: State<'_, AppState>, uuid: String) -> Result<()> {
    state.alloymc.unlink_minecraft(&uuid).await?;
    changed(&app);
    Ok(())
}
