//! alloy-core: everything needed to sign in, install and run Minecraft.
//!
//! The crate is UI-agnostic. The Tauri shell owns a [`Launcher`] and calls into it;
//! long-running work reports through a [`progress::ProgressFn`] callback.

pub mod auth;
pub mod content;
pub mod download;
pub mod error;
pub mod http;
pub mod install;
pub mod instance;
pub mod java;
pub mod launch;
pub mod meta;
pub mod modpack;
pub mod paths;
pub mod progress;
pub mod settings;
pub mod textures;
pub mod updates;
pub mod util;

pub use error::{Error, Result};

use std::sync::Arc;

use paths::Dirs;
use settings::Settings;
use tokio::sync::RwLock;

/// Shared launcher context. Cheap to clone.
#[derive(Clone)]
pub struct Launcher {
    pub dirs: Dirs,
    pub http: reqwest::Client,
    pub settings: Arc<RwLock<Settings>>,
}

impl Launcher {
    /// Opens (or creates) a launcher data directory.
    pub async fn open(dirs: Dirs) -> Result<Self> {
        dirs.ensure().await?;
        let settings = Settings::load(&dirs).await?;
        Ok(Self {
            http: http::client()?,
            dirs,
            settings: Arc::new(RwLock::new(settings)),
        })
    }

    pub async fn save_settings(&self, settings: Settings) -> Result<()> {
        settings.save(&self.dirs).await?;
        *self.settings.write().await = settings;
        Ok(())
    }
}
