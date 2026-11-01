//! The assistant's commands: talking to AlloyMC's relay, the file tools' guarded access to
//! an instance, and saved conversations. The other tools reuse Alloy's own commands from the
//! chat, so an install the assistant makes is the same install the Add button makes.

use std::collections::HashMap;
use std::sync::Mutex;

use alloy_core::assistant::{self, Chat, ChatSummary, Delta, Reply, Status};
use alloy_core::instance;
use alloy_core::{Error, Result};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::oneshot;

use crate::state::AppState;

/// Replies being streamed, by request id; sending stops one.
#[derive(Default)]
pub struct Streams(Mutex<HashMap<String, oneshot::Sender<()>>>);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeltaEvent<'a> {
    request_id: &'a str,
    delta: &'a Delta,
}

#[tauri::command]
pub async fn assistant_status(state: State<'_, AppState>) -> Result<Status> {
    state.alloymc.assistant_status().await
}

/// Sends the conversation; the reply streams as `assistant-delta` events and comes back whole.
#[tauri::command]
pub async fn assistant_send(
    app: AppHandle,
    state: State<'_, AppState>,
    streams: State<'_, Streams>,
    request_id: String,
    mode: String,
    messages: serde_json::Value,
) -> Result<Reply> {
    let (tx, rx) = oneshot::channel();
    streams.0.lock().unwrap().insert(request_id.clone(), tx);
    let id = request_id.clone();
    let send = state.alloymc.assistant_send(&mode, &messages, |d| {
        let _ = app.emit("assistant-delta", DeltaEvent { request_id: &id, delta: &d });
    });
    let out = tokio::select! {
        r = send => r,
        _ = rx => Err(Error::other("Stopped")),
    };
    streams.0.lock().unwrap().remove(&request_id);
    out
}

#[tauri::command]
pub fn assistant_cancel(streams: State<'_, Streams>, request_id: String) {
    if let Some(tx) = streams.0.lock().unwrap().remove(&request_id) {
        let _ = tx.send(());
    }
}

#[tauri::command]
pub async fn assistant_read_file(state: State<'_, AppState>, instance_id: String, path: String) -> Result<String> {
    let inst = instance::get(&state.launcher, &instance_id).await?;
    assistant::read_file(&state.launcher, &inst, &path).await
}

#[tauri::command]
pub async fn assistant_current_text(state: State<'_, AppState>, instance_id: String, path: String) -> Result<Option<String>> {
    let inst = instance::get(&state.launcher, &instance_id).await?;
    assistant::current_text(&state.launcher, &inst, &path).await
}

#[tauri::command]
pub async fn assistant_write_file(state: State<'_, AppState>, instance_id: String, path: String, content: String) -> Result<()> {
    let inst = instance::get(&state.launcher, &instance_id).await?;
    assistant::write_file(&state.launcher, &inst, &path, &content).await
}

#[tauri::command]
pub async fn assistant_read_log(state: State<'_, AppState>, instance_id: String, lines: Option<usize>, contains: Option<String>) -> Result<String> {
    let inst = instance::get(&state.launcher, &instance_id).await?;
    assistant::read_log(&state.launcher, &inst, lines.unwrap_or(120), contains.as_deref()).await
}

/// An automatic save point before the assistant's first change to an instance in a conversation.
#[tauri::command]
pub async fn assistant_checkpoint(app: AppHandle, state: State<'_, AppState>, instance_id: String) -> Result<()> {
    if state.running.lock().unwrap().contains_key(&instance_id) {
        return Ok(());
    }
    let inst = instance::get(&state.launcher, &instance_id).await?;
    crate::packs::auto_savepoint(&app, &inst, "Before the assistant's changes").await;
    Ok(())
}

#[tauri::command]
pub async fn assistant_chats(state: State<'_, AppState>) -> Result<Vec<ChatSummary>> {
    Ok(assistant::chats(&state.launcher).await)
}

#[tauri::command]
pub async fn assistant_chat(state: State<'_, AppState>, id: String) -> Result<Chat> {
    assistant::chat(&state.launcher, &id).await
}

#[tauri::command]
pub async fn assistant_chat_save(state: State<'_, AppState>, chat: Chat) -> Result<()> {
    assistant::save_chat(&state.launcher, &chat).await
}

#[tauri::command]
pub async fn assistant_chat_delete(state: State<'_, AppState>, id: String) -> Result<()> {
    assistant::delete_chat(&state.launcher, &id).await
}
