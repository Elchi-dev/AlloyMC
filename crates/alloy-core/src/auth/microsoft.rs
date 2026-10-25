//! Microsoft → Xbox Live → XSTS → Minecraft sign-in.
//!
//! By default this uses the official Minecraft Launcher's public client ID with the
//! `login.live.com` flow, so no Azure app or Xbox approval is needed. The UI opens
//! [`MsClient::authorize_url`] in a webview and hands back the URL it redirects to.
//! A custom Azure client ID (Settings → Advanced) switches to the v2.0 endpoints.

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::AuthError;

pub const OFFICIAL_CLIENT_ID: &str = "00000000402b5328";
const LIVE_AUTHORIZE: &str = "https://login.live.com/oauth20_authorize.srf";
const LIVE_TOKEN: &str = "https://login.live.com/oauth20_token.srf";
const LIVE_REDIRECT: &str = "https://login.live.com/oauth20_desktop.srf";
const LIVE_SCOPE: &str = "service::user.auth.xboxlive.com::MBI_SSL";

const AZURE_AUTHORIZE: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize";
const AZURE_TOKEN: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const AZURE_REDIRECT: &str = "https://login.microsoftonline.com/common/oauth2/nativeclient";
const AZURE_SCOPE: &str = "XboxLive.signin offline_access";

const XBL_AUTH: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_AUTH: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MC_LOGIN: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
const MC_PROFILE: &str = "https://api.minecraftservices.com/minecraft/profile";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MsClient {
    pub client_id: String,
    /// true = Azure v2.0 endpoints (custom app), false = login.live.com (official ID).
    pub azure: bool,
}

impl MsClient {
    pub fn official() -> Self {
        Self {
            client_id: OFFICIAL_CLIENT_ID.into(),
            azure: false,
        }
    }

    pub fn from_settings(custom: Option<&str>) -> Self {
        match custom.map(str::trim).filter(|s| !s.is_empty()) {
            Some(id) => Self {
                client_id: id.into(),
                azure: true,
            },
            None => Self::official(),
        }
    }

    pub fn redirect_uri(&self) -> &'static str {
        if self.azure {
            AZURE_REDIRECT
        } else {
            LIVE_REDIRECT
        }
    }

    fn scope(&self) -> &'static str {
        if self.azure {
            AZURE_SCOPE
        } else {
            LIVE_SCOPE
        }
    }

    /// The page to open in the login window. `prompt=select_account` lets you add a second account.
    pub fn authorize_url(&self) -> String {
        let base = if self.azure { AZURE_AUTHORIZE } else { LIVE_AUTHORIZE };
        let mut url = url::Url::parse(base).expect("static url");
        url.query_pairs_mut()
            .append_pair("client_id", &self.client_id)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", self.redirect_uri())
            .append_pair("scope", self.scope())
            .append_pair("prompt", "select_account");
        url.to_string()
    }

    /// Given any URL the login window navigated to: `None` if it is not the redirect yet,
    /// otherwise the auth code or the error Microsoft reported.
    pub fn parse_redirect(&self, navigated: &str) -> Option<Result<String, AuthError>> {
        if !navigated.starts_with(self.redirect_uri()) {
            return None;
        }
        let url = url::Url::parse(navigated).ok()?;
        let (mut code, mut error, mut description) = (None, None, None);
        for (k, v) in url.query_pairs() {
            match k.as_ref() {
                "code" => code = Some(v.into_owned()),
                "error" => error = Some(v.into_owned()),
                "error_description" => description = Some(v.into_owned()),
                _ => {}
            }
        }
        Some(match (code, error) {
            (Some(code), _) => Ok(code),
            (None, Some(e)) if e == "access_denied" => Err(AuthError::Cancelled),
            (None, Some(e)) => Err(AuthError::Microsoft(description.unwrap_or(e))),
            (None, None) => Err(AuthError::Microsoft("no code in redirect".into())),
        })
    }

    fn token_url(&self) -> &'static str {
        if self.azure {
            AZURE_TOKEN
        } else {
            LIVE_TOKEN
        }
    }

    pub async fn exchange_code(&self, http: &reqwest::Client, code: &str) -> Result<MsTokens, AuthError> {
        self.token_request(
            http,
            &[
                ("client_id", self.client_id.as_str()),
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", self.redirect_uri()),
                ("scope", self.scope()),
            ],
        )
        .await
    }

    pub async fn refresh(&self, http: &reqwest::Client, refresh_token: &str) -> Result<MsTokens, AuthError> {
        self.token_request(
            http,
            &[
                ("client_id", self.client_id.as_str()),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
                ("redirect_uri", self.redirect_uri()),
                ("scope", self.scope()),
            ],
        )
        .await
        .map_err(|e| match e {
            AuthError::Microsoft(_) => AuthError::Expired,
            other => other,
        })
    }

    async fn token_request(&self, http: &reqwest::Client, form: &[(&str, &str)]) -> Result<MsTokens, AuthError> {
        let resp = http.post(self.token_url()).form(form).send().await?;
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AuthError::Microsoft(body));
        }
        Ok(resp.json().await?)
    }

    /// RpsTicket format differs: live.com tokens go raw, Azure tokens need a `d=` prefix.
    fn rps_ticket(&self, ms_access_token: &str) -> String {
        if self.azure {
            format!("d={ms_access_token}")
        } else {
            ms_access_token.to_string()
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct MsTokens {
    pub access_token: String,
    pub refresh_token: String,
    #[serde(default)]
    pub expires_in: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinecraftSession {
    /// Profile UUID without dashes.
    pub uuid: String,
    pub username: String,
    pub access_token: String,
    /// Unix seconds.
    pub expires_at: i64,
    pub xuid: Option<String>,
    pub skin_url: Option<String>,
    pub skin_variant: Option<String>,
    pub cape_url: Option<String>,
}

#[derive(Deserialize)]
struct XblResponse {
    #[serde(rename = "Token")]
    token: String,
    #[serde(rename = "DisplayClaims")]
    display_claims: DisplayClaims,
}
#[derive(Deserialize)]
struct DisplayClaims {
    xui: Vec<Xui>,
}
#[derive(Deserialize)]
struct Xui {
    uhs: String,
}
#[derive(Deserialize)]
struct XstsError {
    #[serde(rename = "XErr")]
    xerr: u64,
}
#[derive(Deserialize)]
struct McLogin {
    access_token: String,
    expires_in: i64,
}
#[derive(Deserialize)]
struct McProfile {
    id: String,
    name: String,
    #[serde(default)]
    skins: Vec<Texture>,
    #[serde(default)]
    capes: Vec<Texture>,
}
#[derive(Deserialize)]
struct Texture {
    state: String,
    url: String,
    #[serde(default)]
    variant: Option<String>,
}

/// Runs the full chain from a Microsoft access token to a Minecraft session.
pub async fn login_minecraft(
    http: &reqwest::Client,
    client: &MsClient,
    ms_access_token: &str,
) -> Result<MinecraftSession, AuthError> {
    // 1. Xbox Live user token
    let xbl: XblResponse = http
        .post(XBL_AUTH)
        .header("Accept", "application/json")
        .json(&json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": client.rps_ticket(ms_access_token),
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT",
        }))
        .send()
        .await?
        .error_for_status()
        .map_err(|e| AuthError::Xbox(format!("Xbox Live sign-in failed ({e})")))?
        .json()
        .await?;

    // 2. XSTS token for Minecraft services
    let resp = http
        .post(XSTS_AUTH)
        .header("Accept", "application/json")
        .json(&json!({
            "Properties": { "SandboxId": "RETAIL", "UserTokens": [xbl.token] },
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT",
        }))
        .send()
        .await?;
    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        let code = resp.json::<XstsError>().await.map(|e| e.xerr).unwrap_or(0);
        return Err(AuthError::Xbox(xsts_message(code).into()));
    }
    let xsts: XblResponse = resp
        .error_for_status()
        .map_err(|e| AuthError::Xbox(format!("XSTS failed ({e})")))?
        .json()
        .await?;
    let uhs = xsts
        .display_claims
        .xui
        .first()
        .map(|x| x.uhs.clone())
        .ok_or_else(|| AuthError::Xbox("missing user hash".into()))?;

    // 3. Minecraft token
    let mc: McLogin = http
        .post(MC_LOGIN)
        .json(&json!({ "identityToken": format!("XBL3.0 x={uhs};{}", xsts.token) }))
        .send()
        .await?
        .error_for_status()
        .map_err(|e| AuthError::Minecraft(format!("Minecraft services refused the login ({e})")))?
        .json()
        .await?;

    // 4. Profile (404 = no Java Edition on this account, or Game Pass user without a name yet)
    let resp = http.get(MC_PROFILE).bearer_auth(&mc.access_token).send().await?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(AuthError::NoGame);
    }
    let profile: McProfile = resp
        .error_for_status()
        .map_err(|e| AuthError::Minecraft(format!("could not load profile ({e})")))?
        .json()
        .await?;

    let skin = profile.skins.iter().find(|s| s.state == "ACTIVE");
    let cape = profile.capes.iter().find(|s| s.state == "ACTIVE");
    Ok(MinecraftSession {
        uuid: profile.id,
        username: profile.name,
        xuid: jwt_claim(&mc.access_token, "xuid"),
        expires_at: chrono::Utc::now().timestamp() + mc.expires_in,
        access_token: mc.access_token,
        skin_url: skin.map(|s| s.url.clone()),
        skin_variant: skin.and_then(|s| s.variant.clone()),
        cape_url: cape.map(|c| c.url.clone()),
    })
}

fn xsts_message(code: u64) -> &'static str {
    match code {
        2148916227 => "This account is banned from Xbox Live.",
        2148916233 => "This Microsoft account has no Xbox profile yet. Sign in once at minecraft.net, then try again.",
        2148916235 => "Xbox Live is not available in this account's country.",
        2148916236 | 2148916237 => "This account needs adult verification on the Xbox website (South Korea).",
        2148916238 => "This is a child account. Add it to a Microsoft Family group first.",
        _ => "Xbox Live refused the sign-in.",
    }
}

/// Reads one string claim from a JWT payload without verifying it (we only need xuid).
fn jwt_claim(token: &str, claim: &str) -> Option<String> {
    use base64::Engine;
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    match value.get(claim)? {
        serde_json::Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authorize_url_uses_official_id() {
        let url = MsClient::official().authorize_url();
        assert!(url.starts_with("https://login.live.com/oauth20_authorize.srf?client_id=00000000402b5328"));
        assert!(url.contains("MBI_SSL"));
    }

    #[test]
    fn parses_redirect() {
        let c = MsClient::official();
        assert!(c.parse_redirect("https://login.live.com/oauth20_authorize.srf?x=1").is_none());
        let code = c
            .parse_redirect("https://login.live.com/oauth20_desktop.srf?code=M.C123&lc=1033")
            .unwrap()
            .unwrap();
        assert_eq!(code, "M.C123");
        let err = c
            .parse_redirect("https://login.live.com/oauth20_desktop.srf?error=access_denied&error_description=The+user+has+denied")
            .unwrap();
        assert!(matches!(err, Err(AuthError::Cancelled)));
    }

    #[test]
    fn custom_client_switches_to_azure() {
        let c = MsClient::from_settings(Some(" abc-123 "));
        assert!(c.azure);
        assert_eq!(c.client_id, "abc-123");
        assert_eq!(MsClient::from_settings(Some("")), MsClient::official());
    }

    #[test]
    fn reads_jwt_claims() {
        use base64::Engine;
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(br#"{"xuid":"2535400000000000"}"#);
        assert_eq!(jwt_claim(&format!("h.{payload}.s"), "xuid").as_deref(), Some("2535400000000000"));
    }
}
