use std::collections::HashMap;
use std::sync::Mutex;

use alloy_core::auth::Accounts;
use alloy_core::Launcher;
use tokio::sync::oneshot;

pub struct RunningGame {
    pub started_at: i64,
    /// Send to kill the process.
    pub kill: Option<oneshot::Sender<()>>,
}

pub struct AppState {
    pub launcher: Launcher,
    pub accounts: Accounts,
    pub running: Mutex<HashMap<String, RunningGame>>,
    /// Instances with an install job in flight, so double-clicks don't start two.
    pub installing: Mutex<std::collections::HashSet<String>>,
}

impl AppState {
    pub fn new(launcher: Launcher, accounts: Accounts) -> Self {
        Self {
            launcher,
            accounts,
            running: Mutex::new(HashMap::new()),
            installing: Mutex::new(Default::default()),
        }
    }
}
