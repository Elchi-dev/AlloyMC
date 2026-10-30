//! Parallel, hash-verified, retrying downloads.
//!
//! Files are written to `<dest>.part` and renamed only after the hash checks out,
//! so a crash never leaves a half-written jar that looks valid.

use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures::{StreamExt, TryStreamExt};
use sha1::{Digest, Sha1};
use tokio::io::AsyncWriteExt;

use crate::progress::{emit, Progress, ProgressFn, Stage, Unit};
use crate::util::sha1_file;
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct DownloadTask {
    pub url: String,
    pub dest: PathBuf,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    /// Trust an existing file whose size matches instead of hashing it (normal installs).
    /// Repair turns this off so every byte is checked.
    pub quick: bool,
}

impl DownloadTask {
    pub fn new(url: impl Into<String>, dest: impl Into<PathBuf>) -> Self {
        Self { url: url.into(), dest: dest.into(), sha1: None, size: None, quick: false }
    }
    pub fn quick(mut self, quick: bool) -> Self {
        self.quick = quick;
        self
    }
    pub fn sha1(mut self, sha1: Option<String>) -> Self {
        self.sha1 = sha1;
        self
    }
    pub fn size(mut self, size: Option<u64>) -> Self {
        self.size = size;
        self
    }
}

/// True if the file already exists and matches the expected hash (or size, if no hash).
pub async fn is_valid(task: &DownloadTask) -> bool {
    let Ok(meta) = tokio::fs::metadata(&task.dest).await else {
        return false;
    };
    if let Some(size) = task.size {
        if meta.len() != size {
            return false;
        }
        if task.quick {
            return true;
        }
    }
    match &task.sha1 {
        Some(expected) => sha1_file(&task.dest).await.map(|h| h.eq_ignore_ascii_case(expected)).unwrap_or(false),
        None => meta.len() > 0,
    }
}

/// Downloads every task, `concurrency` at a time, reporting under `stage`.
/// Already-valid files are skipped without a network request.
///
/// When every task knows its size, progress is in bytes (and moves while a big file is
/// still coming in, so a slow connection never looks frozen); otherwise in files.
pub async fn download_all(
    http: &reqwest::Client,
    tasks: Vec<DownloadTask>,
    concurrency: usize,
    stage: Stage,
    progress: &ProgressFn,
) -> Result<()> {
    let tasks = dedupe(tasks);
    let total = tasks.len() as u64;
    let sized = !tasks.is_empty() && tasks.iter().all(|t| t.size.is_some());
    let total_bytes: u64 = tasks.iter().filter_map(|t| t.size).sum();
    let done = Arc::new(AtomicU64::new(0));
    let got = Arc::new(AtomicI64::new(0));
    let report = {
        let (done, got, progress) = (done.clone(), got.clone(), progress.clone());
        move || {
            let files = done.load(Ordering::Relaxed);
            if sized {
                let current = (got.load(Ordering::Relaxed).max(0) as u64).min(total_bytes);
                progress(Progress { stage, current, total: total_bytes, unit: Unit::Bytes, message: None, files: Some((files, total)) });
            } else {
                emit(&progress, stage, files, total, Unit::Files, None);
            }
        }
    };
    report();

    futures::stream::iter(tasks.into_iter().map(|task| {
        let (done, got, report) = (done.clone(), got.clone(), report.clone());
        async move {
            if is_valid(&task).await {
                got.fetch_add(task.size.unwrap_or(0) as i64, Ordering::Relaxed);
            } else {
                let on_bytes = |delta: i64, _: Option<u64>| {
                    got.fetch_add(delta, Ordering::Relaxed);
                    report();
                };
                download_with_progress(http, &task, &on_bytes).await?;
            }
            done.fetch_add(1, Ordering::Relaxed);
            report();
            Ok::<_, Error>(())
        }
    }))
    .buffer_unordered(concurrency.max(1))
    .try_collect::<Vec<_>>()
    .await?;
    Ok(())
}

fn dedupe(mut tasks: Vec<DownloadTask>) -> Vec<DownloadTask> {
    let mut seen = std::collections::HashSet::new();
    tasks.retain(|t| seen.insert(t.dest.clone()));
    tasks
}

/// Bytes received by every download since the app started; the downloads panel turns it
/// into a speed.
pub static BYTES_RECEIVED: AtomicU64 = AtomicU64::new(0);

pub async fn download_with_retry(http: &reqwest::Client, task: &DownloadTask) -> Result<()> {
    download_with_progress(http, task, &|_, _| {}).await
}

/// Like [`download_with_retry`], calling `on_bytes(delta, content_length)` as data arrives.
/// A failed attempt reports its bytes back as a negative delta, so totals never overshoot.
pub async fn download_with_progress(
    http: &reqwest::Client,
    task: &DownloadTask,
    on_bytes: &(dyn Fn(i64, Option<u64>) + Send + Sync),
) -> Result<()> {
    let mut last_err = None;
    for attempt in 0..4u32 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(400 * 2u64.pow(attempt))).await;
        }
        let got = AtomicI64::new(0);
        let attempt_bytes = |delta: i64, len: Option<u64>| {
            got.fetch_add(delta, Ordering::Relaxed);
            on_bytes(delta, len);
        };
        let result = download_one(http, task, &attempt_bytes).await;
        if result.is_err() {
            on_bytes(-got.load(Ordering::Relaxed), None);
        }
        match result {
            Ok(()) => return Ok(()),
            // A wrong hash will not fix itself by retrying the same URL.
            Err(e @ Error::HashMismatch { .. }) => return Err(e),
            Err(e) => {
                tracing::warn!(url = %task.url, attempt, "download failed: {e}");
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| Error::other("download failed")))
}

async fn download_one(http: &reqwest::Client, task: &DownloadTask, on_bytes: &(dyn Fn(i64, Option<u64>) + Send + Sync)) -> Result<()> {
    if let Some(parent) = task.dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let resp = http.get(&task.url).send().await?;
    if !resp.status().is_success() {
        return Err(Error::Download { url: task.url.clone(), reason: format!("HTTP {}", resp.status()) });
    }

    let part = task.dest.with_extension(match task.dest.extension() {
        Some(ext) => format!("{}.part", ext.to_string_lossy()),
        None => "part".into(),
    });
    let length = resp.content_length().or(task.size);
    let mut file = tokio::fs::File::create(&part).await?;
    let mut hasher = Sha1::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        BYTES_RECEIVED.fetch_add(chunk.len() as u64, Ordering::Relaxed);
        on_bytes(chunk.len() as i64, length);
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    drop(file);

    if let Some(expected) = &task.sha1 {
        let actual = hex::encode(hasher.finalize());
        if !actual.eq_ignore_ascii_case(expected) {
            let _ = tokio::fs::remove_file(&part).await;
            return Err(Error::HashMismatch { path: task.dest.clone(), expected: expected.clone(), actual });
        }
    }
    tokio::fs::rename(&part, &task.dest).await?;
    Ok(())
}

/// Downloads a small file straight into memory (manifests, indexes).
pub async fn fetch_bytes(http: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    Ok(http.get(url).send().await?.error_for_status()?.bytes().await?.to_vec())
}
