//! Every long task (installs, modpack imports, content installs, updates) is a job: it shows
//! in the downloads panel with its progress, can be cancelled, and can be retried after a
//! failure. Jobs run on their own task, so cancelling aborts the work wherever it is;
//! downloads write to `.part` files, so nothing half-written is ever mistaken for complete.
//!
//! Events: `jobs-changed` (the whole list) and `net-speed` ({ bytesPerSec }) while anything runs.

use std::future::Future;
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use std::time::Duration;

use alloy_core::progress::Progress;
use alloy_core::{Error, Result};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    Running,
    Done,
    Failed,
    Cancelled,
}

/// What to run again when the user presses Retry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Retry {
    Install { instance_id: String, repair: bool },
    ImportModpack { source: String },
    ModrinthModpack { version_id: String },
    CurseForgeModpack { mod_id: u64, file_id: u64 },
    ModrinthContent { instance_id: String, version_id: String },
    CurseForgeContent { instance_id: String, mod_id: u64, file_id: u64 },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    pub title: String,
    pub instance_id: Option<String>,
    pub state: JobState,
    pub progress: Option<Progress>,
    pub error: Option<String>,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub can_retry: bool,
}

struct Entry {
    job: Job,
    abort: Option<tauri::async_runtime::JoinHandle<()>>,
    retry: Option<Retry>,
}

#[derive(Default)]
pub struct Jobs {
    entries: Mutex<Vec<Entry>>,
}

/// Finished jobs kept in the panel.
const KEEP_FINISHED: usize = 30;

impl Jobs {
    pub fn list(&self) -> Vec<Job> {
        self.entries.lock().unwrap().iter().map(|e| e.job.clone()).collect()
    }

    pub fn running(&self) -> bool {
        self.entries.lock().unwrap().iter().any(|e| e.job.state == JobState::Running)
    }

    pub fn set_progress(&self, id: &str, p: &Progress) {
        if let Some(e) = self.entries.lock().unwrap().iter_mut().find(|e| e.job.id == id) {
            e.job.progress = Some(p.clone());
        }
    }

    fn finish(&self, id: &str, state: JobState, error: Option<String>) {
        let mut entries = self.entries.lock().unwrap();
        if let Some(e) = entries.iter_mut().find(|e| e.job.id == id) {
            // A cancel already decided the outcome.
            if e.job.state == JobState::Running {
                e.job.state = state;
                e.job.error = error;
                e.job.finished_at = Some(chrono::Utc::now().timestamp());
                e.job.can_retry = e.retry.is_some() && state != JobState::Done;
            }
            e.abort = None;
        }
        let finished: Vec<usize> = entries.iter().enumerate().filter(|(_, e)| e.job.state != JobState::Running).map(|(i, _)| i).collect();
        if finished.len() > KEEP_FINISHED {
            let drop: Vec<usize> = finished[..finished.len() - KEEP_FINISHED].to_vec();
            let mut i = 0;
            entries.retain(|_| {
                let keep = !drop.contains(&i);
                i += 1;
                keep
            });
        }
    }

    pub fn cancel(&self, id: &str) -> bool {
        let mut entries = self.entries.lock().unwrap();
        let Some(e) = entries.iter_mut().find(|e| e.job.id == id && e.job.state == JobState::Running) else { return false };
        if let Some(h) = e.abort.take() {
            h.abort();
        }
        e.job.state = JobState::Cancelled;
        e.job.finished_at = Some(chrono::Utc::now().timestamp());
        e.job.can_retry = e.retry.is_some();
        true
    }

    pub fn retry_spec(&self, id: &str) -> Option<Retry> {
        self.entries.lock().unwrap().iter().find(|e| e.job.id == id).and_then(|e| e.retry.clone())
    }

    pub fn clear_finished(&self) {
        self.entries.lock().unwrap().retain(|e| e.job.state == JobState::Running);
    }
}

pub fn changed(app: &AppHandle) {
    let jobs = app.state::<Jobs>().list();
    let _ = app.emit("jobs-changed", jobs);
}

/// Runs `work` as a job and waits for it. Cancelling the job makes this return an error.
pub async fn run<T, F>(app: &AppHandle, id: &str, title: String, instance_id: Option<String>, retry: Option<Retry>, work: F) -> Result<T>
where
    T: Send + 'static,
    F: Future<Output = Result<T>> + Send + 'static,
{
    let (tx, rx) = tokio::sync::oneshot::channel::<Result<T>>();
    let handle = tauri::async_runtime::spawn(async move {
        let _ = tx.send(work.await);
    });
    {
        let jobs = app.state::<Jobs>();
        let mut entries = jobs.entries.lock().unwrap();
        // Retrying reuses the id: replace the old entry, keeping its title.
        let title = entries.iter().find(|e| e.job.id == id).map(|e| e.job.title.clone()).unwrap_or(title);
        entries.retain(|e| e.job.id != id);
        entries.push(Entry {
            job: Job {
                id: id.to_string(),
                title,
                instance_id,
                state: JobState::Running,
                progress: None,
                error: None,
                started_at: chrono::Utc::now().timestamp(),
                finished_at: None,
                can_retry: false,
            },
            abort: Some(handle),
            retry,
        });
    }
    changed(app);
    let result = match rx.await {
        Ok(r) => r,
        Err(_) => Err(Error::other("Cancelled")),
    };
    let jobs = app.state::<Jobs>();
    match &result {
        Ok(_) => jobs.finish(id, JobState::Done, None),
        Err(e) => jobs.finish(id, JobState::Failed, Some(e.to_string())),
    }
    changed(app);
    result
}

/// Emits the download speed twice a second while any job runs.
pub fn start_speed_meter(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut last = alloy_core::download::BYTES_RECEIVED.load(Ordering::Relaxed);
        let mut was_running = false;
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let now = alloy_core::download::BYTES_RECEIVED.load(Ordering::Relaxed);
            let running = app.state::<Jobs>().running();
            if running || was_running {
                let per_sec = if running { (now.saturating_sub(last)) * 2 } else { 0 };
                let _ = app.emit("net-speed", serde_json::json!({ "bytesPerSec": per_sec }));
            }
            was_running = running;
            last = now;
        }
    });
}
