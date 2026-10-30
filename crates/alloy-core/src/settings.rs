use serde::{Deserialize, Serialize};

use crate::paths::Dirs;
use crate::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub theme: Theme,
    /// Hex accent colour, e.g. "#3ddc84".
    pub accent: String,
    /// full: everything moves; calm: transitions only, loops slowed; off: instant changes.
    pub motion: Motion,
    pub background: Background,

    pub default_memory_mb: u32,
    pub default_jvm_args: Vec<String>,
    pub max_concurrent_downloads: usize,

    /// What happens to the launcher window when a game starts.
    pub on_launch: OnLaunch,

    /// Your own Azure app ID. Empty = use the official launcher's public client ID.
    pub custom_msa_client_id: Option<String>,
    /// CurseForge Core API key from console.curseforge.com.
    pub curseforge_api_key: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Dark,
    Light,
    Oled,
    System,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Motion {
    Full,
    Calm,
    Off,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Background {
    /// The animated Alloy forge scene.
    Forge,
    /// Plain dark surfaces.
    Plain,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OnLaunch {
    KeepOpen,
    Minimize,
    Close,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::Dark,
            accent: "#f8c95a".into(),
            motion: Motion::Full,
            background: Background::Forge,
            default_memory_mb: 4096,
            default_jvm_args: vec![],
            max_concurrent_downloads: 16,
            on_launch: OnLaunch::KeepOpen,
            custom_msa_client_id: None,
            curseforge_api_key: None,
        }
    }
}

impl Settings {
    pub async fn load(dirs: &Dirs) -> Result<Self> {
        match tokio::fs::read(dirs.settings_file()).await {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes).unwrap_or_default()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn save(&self, dirs: &Dirs) -> Result<()> {
        write_json_atomic(&dirs.settings_file(), self).await
    }
}

/// Writes JSON to a temp file then renames, so a crash never truncates the real file.
pub async fn write_json_atomic<T: Serialize>(path: &std::path::Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let tmp = path.with_extension("json.tmp");
    tokio::fs::write(&tmp, serde_json::to_vec_pretty(value)?).await?;
    tokio::fs::rename(&tmp, path).await?;
    Ok(())
}
