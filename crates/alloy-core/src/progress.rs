use std::sync::Arc;

use serde::Serialize;

/// Which phase a long job is in. The UI maps these to labels inside the Play button.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Metadata,
    Java,
    Libraries,
    Assets,
    Loader,
    Content,
    Extracting,
    Starting,
    Done,
}

#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    pub stage: Stage,
    /// Units completed (files or bytes, see `unit`).
    pub current: u64,
    pub total: u64,
    pub unit: Unit,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    Files,
    Bytes,
    Steps,
}

pub type ProgressFn = Arc<dyn Fn(Progress) + Send + Sync>;

pub fn noop() -> ProgressFn {
    Arc::new(|_| {})
}

pub fn emit(f: &ProgressFn, stage: Stage, current: u64, total: u64, unit: Unit, message: Option<String>) {
    f(Progress {
        stage,
        current,
        total,
        unit,
        message,
    })
}
