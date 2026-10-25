//! On-disk layout. Everything lives under one root so it can be moved as a unit.
//!
//! ```text
//! <root>/
//!   settings.json, accounts.json
//!   meta/        cached manifests and version JSONs
//!   libraries/   shared Maven-layout libraries
//!   assets/      shared asset indexes + objects
//!   versions/    client jars + version JSONs
//!   natives/     extracted legacy natives, per version
//!   java/        managed Java runtimes, per component
//!   instances/   one folder per instance (instance.json + game dir)
//!   trash/       deleted instances, kept until emptied
//! ```

use std::path::{Path, PathBuf};

use crate::Result;

#[derive(Debug, Clone)]
pub struct Dirs {
    root: PathBuf,
}

impl Dirs {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Platform default: `%APPDATA%/Alloy`, `~/Library/Application Support/Alloy`,
    /// `~/.local/share/alloy`.
    pub fn platform_default() -> Self {
        let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
        let name = if cfg!(target_os = "linux") { "alloy" } else { "Alloy" };
        Self::new(base.join(name))
    }

    pub async fn ensure(&self) -> Result<()> {
        for dir in [
            self.root.clone(),
            self.meta(),
            self.libraries(),
            self.assets(),
            self.versions(),
            self.java(),
            self.instances(),
            self.trash(),
        ] {
            tokio::fs::create_dir_all(dir).await?;
        }
        Ok(())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn settings_file(&self) -> PathBuf {
        self.root.join("settings.json")
    }
    pub fn accounts_file(&self) -> PathBuf {
        self.root.join("accounts.json")
    }
    pub fn meta(&self) -> PathBuf {
        self.root.join("meta")
    }
    pub fn libraries(&self) -> PathBuf {
        self.root.join("libraries")
    }
    pub fn assets(&self) -> PathBuf {
        self.root.join("assets")
    }
    pub fn versions(&self) -> PathBuf {
        self.root.join("versions")
    }
    pub fn version_dir(&self, id: &str) -> PathBuf {
        self.versions().join(id)
    }
    pub fn version_json(&self, id: &str) -> PathBuf {
        self.version_dir(id).join(format!("{id}.json"))
    }
    pub fn client_jar(&self, id: &str) -> PathBuf {
        self.version_dir(id).join(format!("{id}.jar"))
    }
    pub fn natives(&self, version_id: &str) -> PathBuf {
        self.root.join("natives").join(version_id)
    }
    pub fn java(&self) -> PathBuf {
        self.root.join("java")
    }
    pub fn instances(&self) -> PathBuf {
        self.root.join("instances")
    }
    pub fn instance(&self, id: &str) -> PathBuf {
        self.instances().join(id)
    }
    pub fn trash(&self) -> PathBuf {
        self.root.join("trash")
    }
}
