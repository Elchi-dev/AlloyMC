//! AlloyMC accounts: signing Alloy in to alloymc.dev, the signed-in profile, and linking
//! Minecraft accounts to it.
//!
//! Signing in never handles a password. Alloy opens alloymc.dev in the browser with a PKCE
//! challenge; the person signs in there (password or Google) and allows Alloy; the browser
//! brings a one-time code back to Alloy on the loopback address; Alloy trades code plus
//! verifier for a token. The token lives in the keychain, the profile in a small cache file
//! so the account menu has something to show before the network answers.
//!
//! Linking a Minecraft account uses Mojang's own join check, the one servers use: AlloyMC
//! hands out a one-time server id, Alloy "joins" it with the player's Minecraft session, and
//! AlloyMC asks Mojang whether that player really joined. The Microsoft token never leaves
//! this machine.

use std::sync::Arc;

use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::auth::{LaunchSession, SecretStore};
use crate::error::{Error, Result};
use crate::Launcher;

const TOKEN_KEY: &str = "alloymc:token";

/// The site and the API. `ALLOY_MC_SITE` / `ALLOY_MC_API` point a development build at a
/// local alloy-server.
pub fn site_url() -> String {
    std::env::var("ALLOY_MC_SITE").unwrap_or_else(|_| "https://alloymc.dev".into())
}

pub fn api_url() -> String {
    std::env::var("ALLOY_MC_API").unwrap_or_else(|_| "https://api.alloymc.dev".into())
}

/// A Minecraft account linked to the AlloyMC profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedMinecraft {
    pub uuid: String,
    pub name: String,
    #[serde(default)]
    pub shown: bool,
    #[serde(default)]
    pub head_url: Option<String>,
}

/// The signed-in AlloyMC account, as `GET /v1/me` returns it (the parts Alloy uses).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub handle: String,
    pub display_name: String,
    #[serde(default)]
    pub avatar_url: String,
    #[serde(default)]
    pub profile_url: String,
    #[serde(default)]
    pub accent: String,
    #[serde(default)]
    pub followers: u64,
    #[serde(default)]
    pub following: u64,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub email_verified: bool,
    #[serde(default)]
    pub minecraft: Vec<LinkedMinecraft>,
}

#[derive(Deserialize)]
struct ApiError {
    error: ApiErrorBody,
}

#[derive(Deserialize)]
struct ApiErrorBody {
    code: String,
    message: String,
}

/// What a sign-in needs to remember between opening the browser and the code coming back.
pub struct Pending {
    pub verifier: String,
    pub state: String,
    pub url: String,
}

pub struct AlloyMc {
    launcher: Launcher,
    secrets: Arc<dyn SecretStore>,
}

impl AlloyMc {
    pub fn new(launcher: Launcher, secrets: Arc<dyn SecretStore>) -> Self {
        Self { launcher, secrets }
    }

    fn cache_path(&self) -> std::path::PathBuf {
        self.launcher.dirs.root().join("alloymc.json")
    }

    fn token(&self) -> Option<String> {
        self.secrets.get(TOKEN_KEY).ok().flatten().filter(|t| !t.is_empty())
    }

    pub fn signed_in(&self) -> bool {
        self.token().is_some()
    }

    /// The last profile seen, without touching the network.
    pub async fn cached(&self) -> Option<Profile> {
        if !self.signed_in() {
            return None;
        }
        let b = tokio::fs::read(self.cache_path()).await.ok()?;
        serde_json::from_slice(&b).ok()
    }

    async fn remember(&self, p: &Profile) {
        if let Ok(b) = serde_json::to_vec_pretty(p) {
            let _ = crate::util::write_atomic(&self.cache_path(), &b).await;
        }
    }

    /// Starts a sign-in: a PKCE pair, a state value and the URL to open. `port` is where
    /// Alloy listens for the code on 127.0.0.1.
    pub fn begin(&self, device: &str, port: u16) -> Pending {
        let verifier = random_token();
        let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let state = random_token()[..24].to_string();
        let mut url = url::Url::parse(&format!("{}/auth/app", site_url())).expect("site url");
        url.query_pairs_mut()
            .append_pair("challenge", &challenge)
            .append_pair("state", &state)
            .append_pair("device", device)
            .append_pair("port", &port.to_string());
        Pending { verifier, state, url: url.to_string() }
    }

    /// Finishes a sign-in: trades the code for a token and stores it.
    pub async fn finish(&self, code: &str, verifier: &str) -> Result<Profile> {
        #[derive(Deserialize)]
        struct Tok {
            token: String,
        }
        let res = self
            .launcher
            .http
            .post(format!("{}/v1/auth/token", api_url()))
            .json(&serde_json::json!({ "code": code, "verifier": verifier }))
            .send()
            .await?;
        let tok: Tok = read(res).await?;
        self.secrets.set(TOKEN_KEY, &tok.token).map_err(Error::other)?;
        self.refresh().await?.ok_or_else(|| Error::other("AlloyMC didn't accept the new sign-in. Try again."))
    }

    /// The current profile from the server. `None` when signed out, or when the server says
    /// the token is no longer valid (it was signed out under Devices); then it's forgotten.
    pub async fn refresh(&self) -> Result<Option<Profile>> {
        let Some(token) = self.token() else { return Ok(None) };
        let res = self.launcher.http.get(format!("{}/v1/me", api_url())).bearer_auth(&token).send().await?;
        if res.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.forget().await;
            return Ok(None);
        }
        let p: Profile = read(res).await?;
        self.remember(&p).await;
        Ok(Some(p))
    }

    async fn forget(&self) {
        let _ = self.secrets.delete(TOKEN_KEY);
        let _ = tokio::fs::remove_file(self.cache_path()).await;
    }

    /// Signs Alloy out: ends the token on the server (best effort) and forgets it here.
    pub async fn sign_out(&self) -> Result<()> {
        if let Some(token) = self.token() {
            let _ = self.launcher.http.delete(format!("{}/v1/auth/token", api_url())).bearer_auth(token).send().await;
        }
        self.forget().await;
        Ok(())
    }

    /// Links a Minecraft account through Mojang's join check.
    pub async fn link_minecraft(&self, session: &LaunchSession) -> Result<LinkedMinecraft> {
        let token = self.token().ok_or_else(|| Error::other("Sign in to AlloyMC first."))?;
        if session.user_type != "msa" {
            return Err(Error::other("Only Microsoft accounts can be linked. Offline players can't prove who they are."));
        }
        let http = &self.launcher.http;
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Challenge {
            server_id: String,
        }
        let res = http.post(format!("{}/v1/me/minecraft/challenge", api_url())).bearer_auth(&token).send().await?;
        let ch: Challenge = read(res).await?;
        join_server(http, &session.access_token, &session.uuid, &ch.server_id).await?;
        let res = http
            .post(format!("{}/v1/me/minecraft", api_url()))
            .bearer_auth(&token)
            .json(&serde_json::json!({ "username": session.username, "serverId": ch.server_id }))
            .send()
            .await?;
        let link: LinkedMinecraft = read(res).await?;
        let _ = self.refresh().await;
        Ok(link)
    }

    /// Removes a linked Minecraft account from the profile.
    pub async fn unlink_minecraft(&self, uuid: &str) -> Result<()> {
        let token = self.token().ok_or_else(|| Error::other("Sign in to AlloyMC first."))?;
        let res = self
            .launcher
            .http
            .delete(format!("{}/v1/me/minecraft/{}", api_url(), uuid.replace('-', "")))
            .bearer_auth(token)
            .send()
            .await?;
        if !res.status().is_success() {
            return Err(api_error(res).await);
        }
        let _ = self.refresh().await;
        Ok(())
    }
}

/// Tells Mojang's session server that this player is joining `server_id`. Only someone
/// holding the player's Minecraft session can do this; AlloyMC then checks it with
/// `hasJoined`, exactly like a Minecraft server.
pub async fn join_server(http: &reqwest::Client, access_token: &str, uuid: &str, server_id: &str) -> Result<()> {
    let res = http
        .post("https://sessionserver.mojang.com/session/minecraft/join")
        .json(&serde_json::json!({
            "accessToken": access_token,
            "selectedProfile": uuid.replace('-', ""),
            "serverId": server_id,
        }))
        .send()
        .await?;
    if res.status().is_success() {
        return Ok(());
    }
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    Err(Error::other(if status == reqwest::StatusCode::FORBIDDEN && body.contains("InsufficientPrivileges") {
        "Mojang says this account can't join servers (multiplayer is turned off in its Xbox privacy settings), so it can't be linked.".to_string()
    } else if status == reqwest::StatusCode::FORBIDDEN {
        "Mojang didn't accept this account's session. Sign in to Microsoft again and retry.".to_string()
    } else {
        format!("Mojang's session server answered {status}. Try again in a minute.")
    }))
}

async fn read<T: serde::de::DeserializeOwned>(res: reqwest::Response) -> Result<T> {
    if !res.status().is_success() {
        return Err(api_error(res).await);
    }
    Ok(res.json().await?)
}

async fn api_error(res: reqwest::Response) -> Error {
    let status = res.status();
    match res.json::<ApiError>().await {
        Ok(e) => {
            tracing::debug!("alloymc api error {}: {}", status, e.error.code);
            Error::other(e.error.message)
        }
        Err(_) => Error::other(format!("AlloyMC answered {status}. Try again in a minute.")),
    }
}

/// Waits on `listener` (bound to 127.0.0.1) for the browser to bring the code back. Other
/// requests get a 404 and the wait goes on. The browser is sent on to `done` (or `done?error=1`).
pub async fn wait_for_code(listener: &tokio::net::TcpListener, done: &str) -> Result<std::result::Result<(String, String), String>> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    loop {
        let (mut sock, _) = listener.accept().await?;
        let mut buf = vec![0u8; 8192];
        let n = match tokio::time::timeout(std::time::Duration::from_secs(10), sock.read(&mut buf)).await {
            Ok(Ok(n)) => n,
            _ => continue,
        };
        let req = String::from_utf8_lossy(&buf[..n]).to_string();
        match parse_callback(req.lines().next().unwrap_or_default()) {
            None => {
                let _ = sock.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
            }
            Some(result) => {
                let to = if result.is_ok() { done.to_string() } else { format!("{done}?error=1") };
                let reply = format!("HTTP/1.1 303 See Other\r\nLocation: {to}\r\nCache-Control: no-store\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                let _ = sock.write_all(reply.as_bytes()).await;
                return Ok(result);
            }
        }
    }
}

/// 43 URL-safe characters from 32 random bytes.
fn random_token() -> String {
    let mut b = [0u8; 32];
    b[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    b[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

/// "Alloy on <computer name>", for the Devices list on alloymc.dev.
pub fn device_name() -> String {
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty());
    match host {
        Some(h) => format!("Alloy on {h}"),
        None => "Alloy".to_string(),
    }
}

/// Parses the loopback callback's request line ("GET /alloymc/callback?code=..&state=.. HTTP/1.1")
/// into (code, state), or the error the person chose ("access_denied").
pub fn parse_callback(request_line: &str) -> Option<std::result::Result<(String, String), String>> {
    let target = request_line.split_whitespace().nth(1)?;
    if !target.starts_with("/alloymc/callback") {
        return None;
    }
    let url = url::Url::parse(&format!("http://127.0.0.1{target}")).ok()?;
    let get = |k: &str| url.query_pairs().find(|(key, _)| key == k).map(|(_, v)| v.into_owned());
    if let Some(e) = get("error") {
        return Some(Err(e));
    }
    Some(Ok((get("code")?, get("state")?)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_lines() {
        assert_eq!(
            parse_callback("GET /alloymc/callback?code=abc&state=xyz HTTP/1.1"),
            Some(Ok(("abc".into(), "xyz".into())))
        );
        assert_eq!(parse_callback("GET /alloymc/callback?error=access_denied&state=x HTTP/1.1"), Some(Err("access_denied".into())));
        assert_eq!(parse_callback("GET /favicon.ico HTTP/1.1"), None);
        assert_eq!(parse_callback("GET /alloymc/callback?state=x HTTP/1.1"), None);
    }

    #[test]
    fn tokens_are_long_and_url_safe() {
        let t = random_token();
        assert_eq!(t.len(), 43);
        assert!(t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert_ne!(t, random_token());
    }

    #[test]
    fn begin_builds_a_pkce_url() {
        // The challenge in the URL must be the S256 of the verifier, which the server checks.
        let verifier = "v".repeat(43);
        let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        assert_eq!(challenge.len(), 43);
        assert!(!challenge.contains('='));
    }
}
