//! Accounts: Microsoft (online) and offline, with secrets kept out of plain JSON.
//!
//! `accounts.json` holds only display data (name, UUID, skin URL). Tokens go through a
//! [`SecretStore`]: the Tauri shell plugs in the OS keychain; [`FileSecretStore`] is the
//! fallback for systems without one.

pub mod microsoft;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

use crate::settings::write_json_atomic;
use crate::{Launcher, Result};
use microsoft::{login_minecraft, MinecraftSession, MsClient};

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("Sign-in was cancelled.")]
    Cancelled,
    #[error("Your Microsoft sign-in expired. Sign in again.")]
    Expired,
    #[error("Microsoft sign-in failed: {0}")]
    Microsoft(String),
    #[error("{0}")]
    Xbox(String),
    #[error("{0}")]
    Minecraft(String),
    #[error("This account doesn't own Minecraft: Java Edition (Game Pass users: start the official launcher once to pick a name).")]
    NoGame,
    #[error("Network error during sign-in: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Invalid username: use 3–16 letters, digits or underscores.")]
    InvalidName,
    #[error("Account not found.")]
    UnknownAccount,
    #[error("Secret storage error: {0}")]
    Secrets(String),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AccountKind {
    Microsoft,
    Offline,
}

/// What the UI sees. No tokens.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    /// Profile UUID without dashes; doubles as the account id.
    pub id: String,
    pub kind: AccountKind,
    pub username: String,
    pub skin_url: Option<String>,
    pub skin_variant: Option<String>,
    pub cape_url: Option<String>,
    pub added_at: i64,
    /// True when the stored refresh token was rejected; the UI shows "sign in again".
    #[serde(default)]
    pub needs_login: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct AccountsFile {
    accounts: Vec<Account>,
    active: Option<String>,
}

/// The long-lived secret for one Microsoft account. Kept small: the Windows credential
/// store caps entries at 2560 bytes, so the short-lived Minecraft token stays in memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct MsSecrets {
    refresh_token: String,
    client: MsClient,
}

#[derive(Debug, Clone)]
struct CachedToken {
    access_token: String,
    expires_at: i64,
    xuid: Option<String>,
}

/// What the game needs to start.
#[derive(Debug, Clone)]
pub struct LaunchSession {
    pub username: String,
    pub uuid: String,
    pub access_token: String,
    pub user_type: &'static str,
    pub xuid: String,
}

pub trait SecretStore: Send + Sync {
    fn get(&self, key: &str) -> std::result::Result<Option<String>, String>;
    fn set(&self, key: &str, value: &str) -> std::result::Result<(), String>;
    fn delete(&self, key: &str) -> std::result::Result<(), String>;
}

/// A JSON file of secrets with owner-only permissions. Used when no OS keychain is available.
pub struct FileSecretStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl FileSecretStore {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            lock: Mutex::new(()),
        }
    }
    fn read(&self) -> HashMap<String, String> {
        std::fs::read(&self.path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }
    fn write(&self, map: &HashMap<String, String>) -> std::result::Result<(), String> {
        let bytes = serde_json::to_vec(map).map_err(|e| e.to_string())?;
        std::fs::write(&self.path, bytes).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }
}

impl SecretStore for FileSecretStore {
    fn get(&self, key: &str) -> std::result::Result<Option<String>, String> {
        let _g = self.lock.lock().unwrap();
        Ok(self.read().get(key).cloned())
    }
    fn set(&self, key: &str, value: &str) -> std::result::Result<(), String> {
        let _g = self.lock.lock().unwrap();
        let mut map = self.read();
        map.insert(key.into(), value.into());
        self.write(&map)
    }
    fn delete(&self, key: &str) -> std::result::Result<(), String> {
        let _g = self.lock.lock().unwrap();
        let mut map = self.read();
        map.remove(key);
        self.write(&map)
    }
}

/// Java's `UUID.nameUUIDFromBytes("OfflinePlayer:" + name)`, which offline-mode servers use.
pub fn offline_uuid(name: &str) -> String {
    let mut bytes: [u8; 16] = Md5::digest(format!("OfflinePlayer:{name}").as_bytes()).into();
    bytes[6] = (bytes[6] & 0x0f) | 0x30;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    hex::encode(bytes)
}

fn valid_offline_name(name: &str) -> bool {
    (3..=16).contains(&name.len()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub struct Accounts {
    launcher: Launcher,
    secrets: Arc<dyn SecretStore>,
    lock: tokio::sync::Mutex<()>,
    tokens: Mutex<HashMap<String, CachedToken>>,
}

impl Accounts {
    pub fn new(launcher: Launcher, secrets: Arc<dyn SecretStore>) -> Self {
        Self {
            launcher,
            secrets,
            lock: tokio::sync::Mutex::new(()),
            tokens: Mutex::new(HashMap::new()),
        }
    }

    async fn load(&self) -> Result<AccountsFile> {
        match tokio::fs::read(self.launcher.dirs.accounts_file()).await {
            Ok(b) => Ok(serde_json::from_slice(&b).unwrap_or_default()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(AccountsFile::default()),
            Err(e) => Err(e.into()),
        }
    }

    async fn save(&self, file: &AccountsFile) -> Result<()> {
        write_json_atomic(&self.launcher.dirs.accounts_file(), file).await
    }

    /// All accounts, Microsoft first, plus the active id.
    pub async fn list(&self) -> Result<(Vec<Account>, Option<String>)> {
        let file = self.load().await?;
        Ok((file.accounts, file.active))
    }

    pub async fn set_active(&self, id: &str) -> Result<()> {
        let _g = self.lock.lock().await;
        let mut file = self.load().await?;
        if !file.accounts.iter().any(|a| a.id == id) {
            return Err(AuthError::UnknownAccount.into());
        }
        file.active = Some(id.into());
        self.save(&file).await
    }

    pub async fn add_offline(&self, name: &str) -> Result<Account> {
        let name = name.trim();
        if !valid_offline_name(name) {
            return Err(AuthError::InvalidName.into());
        }
        let account = Account {
            id: offline_uuid(name),
            kind: AccountKind::Offline,
            username: name.into(),
            skin_url: None,
            skin_variant: None,
            cape_url: None,
            added_at: chrono::Utc::now().timestamp(),
            needs_login: false,
        };
        self.upsert(account.clone(), true).await?;
        Ok(account)
    }

    /// Finishes a Microsoft login from the auth code the login window captured.
    pub async fn add_microsoft(&self, client: &MsClient, code: &str) -> Result<Account> {
        let http = &self.launcher.http;
        let ms = client.exchange_code(http, code).await?;
        let session = login_minecraft(http, client, &ms.access_token).await?;
        self.store_ms(client, &ms.refresh_token, &session)?;
        let account = Account {
            id: session.uuid.clone(),
            kind: AccountKind::Microsoft,
            username: session.username,
            skin_url: session.skin_url,
            skin_variant: session.skin_variant,
            cape_url: session.cape_url,
            added_at: chrono::Utc::now().timestamp(),
            needs_login: false,
        };
        self.upsert(account.clone(), true).await?;
        Ok(account)
    }

    fn store_ms(&self, client: &MsClient, refresh: &str, s: &MinecraftSession) -> Result<()> {
        let secrets = MsSecrets {
            refresh_token: refresh.into(),
            client: client.clone(),
        };
        self.tokens.lock().unwrap().insert(
            s.uuid.clone(),
            CachedToken {
                access_token: s.access_token.clone(),
                expires_at: s.expires_at,
                xuid: s.xuid.clone(),
            },
        );
        self.secrets
            .set(&format!("msa:{}", s.uuid), &serde_json::to_string(&secrets)?)
            .map_err(AuthError::Secrets)?;
        Ok(())
    }

    async fn upsert(&self, account: Account, make_active: bool) -> Result<()> {
        let _g = self.lock.lock().await;
        let mut file = self.load().await?;
        let id = account.id.clone();
        match file.accounts.iter_mut().find(|a| a.id == id) {
            Some(existing) => {
                let added = existing.added_at;
                *existing = Account { added_at: added, ..account };
            }
            None => file.accounts.push(account),
        }
        if make_active || file.active.is_none() {
            file.active = Some(id);
        }
        self.save(&file).await
    }

    pub async fn remove(&self, id: &str) -> Result<()> {
        let _g = self.lock.lock().await;
        let mut file = self.load().await?;
        file.accounts.retain(|a| a.id != id);
        if file.active.as_deref() == Some(id) {
            file.active = file.accounts.first().map(|a| a.id.clone());
        }
        let _ = self.secrets.delete(&format!("msa:{id}"));
        self.save(&file).await
    }

    /// A ready-to-use session for `id`, refreshing Microsoft tokens if they expire within 5 minutes.
    pub async fn session(&self, id: &str) -> Result<LaunchSession> {
        let (accounts, _) = self.list().await?;
        let account = accounts
            .into_iter()
            .find(|a| a.id == id)
            .ok_or(AuthError::UnknownAccount)?;

        if account.kind == AccountKind::Offline {
            return Ok(LaunchSession {
                username: account.username,
                uuid: account.id,
                access_token: "0".into(),
                user_type: "legacy",
                xuid: "0".into(),
            });
        }

        let cached = self.tokens.lock().unwrap().get(id).cloned();
        let token = match cached {
            Some(t) if t.expires_at - chrono::Utc::now().timestamp() > 300 => t,
            _ => self.refresh_ms(id, account.added_at).await?,
        };

        Ok(LaunchSession {
            username: self
                .list()
                .await?
                .0
                .into_iter()
                .find(|a| a.id == id)
                .map(|a| a.username)
                .unwrap_or(account.username),
            uuid: account.id,
            access_token: token.access_token,
            user_type: "msa",
            xuid: token.xuid.unwrap_or_else(|| "0".into()),
        })
    }

    /// Exchanges the stored refresh token for a fresh Minecraft token.
    async fn refresh_ms(&self, id: &str, added_at: i64) -> Result<CachedToken> {
        let raw = self
            .secrets
            .get(&format!("msa:{id}"))
            .map_err(AuthError::Secrets)?
            .ok_or(AuthError::Expired)?;
        let secrets: MsSecrets = serde_json::from_str(&raw).map_err(|_| AuthError::Expired)?;
        let http = &self.launcher.http;
        let refreshed = match secrets.client.refresh(http, &secrets.refresh_token).await {
            Ok(t) => t,
            Err(e) => {
                self.mark_needs_login(id, true).await?;
                return Err(e.into());
            }
        };
        let session = login_minecraft(http, &secrets.client, &refreshed.access_token).await?;
        self.store_ms(&secrets.client, &refreshed.refresh_token, &session)?;
        // Name or skin may have changed since last time.
        self.upsert(Account {
            id: session.uuid.clone(),
            kind: AccountKind::Microsoft,
            username: session.username.clone(),
            skin_url: session.skin_url.clone(),
            skin_variant: session.skin_variant.clone(),
            cape_url: session.cape_url.clone(),
            added_at,
            needs_login: false,
        }, false)
        .await?;
        Ok(CachedToken {
            access_token: session.access_token,
            expires_at: session.expires_at,
            xuid: session.xuid,
        })
    }

    async fn mark_needs_login(&self, id: &str, value: bool) -> Result<()> {
        let _g = self.lock.lock().await;
        let mut file = self.load().await?;
        if let Some(a) = file.accounts.iter_mut().find(|a| a.id == id) {
            a.needs_login = value;
        }
        self.save(&file).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_uuid_matches_java() {
        // Java: UUID.nameUUIDFromBytes("OfflinePlayer:Notch".getBytes(UTF_8))
        assert_eq!(offline_uuid("Notch"), "b50ad385829d3141a2167e7d7539ba7f");
    }

    #[test]
    fn offline_names() {
        assert!(valid_offline_name("Steve_42"));
        assert!(!valid_offline_name("ab"));
        assert!(!valid_offline_name("has space"));
        assert!(!valid_offline_name("waytoolongusername1"));
    }

    #[tokio::test]
    async fn offline_account_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let launcher = Launcher::open(crate::paths::Dirs::new(dir.path())).await.unwrap();
        let secrets = Arc::new(FileSecretStore::new(dir.path().join("secrets.json")));
        let accounts = Accounts::new(launcher, secrets);
        let a = accounts.add_offline("Steve").await.unwrap();
        let (list, active) = accounts.list().await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(active.as_deref(), Some(a.id.as_str()));
        let s = accounts.session(&a.id).await.unwrap();
        assert_eq!(s.user_type, "legacy");
        accounts.remove(&a.id).await.unwrap();
        assert!(accounts.list().await.unwrap().0.is_empty());
    }
}
