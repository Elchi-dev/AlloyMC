use std::time::Duration;

use serde::de::DeserializeOwned;

use crate::Result;

pub const USER_AGENT: &str = concat!("Alloy/", env!("CARGO_PKG_VERSION"), " (private build)");

pub fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .pool_max_idle_per_host(16)
        .build()?)
}

/// GET a URL and parse JSON, turning non-2xx into a readable error.
pub async fn get_json<T: DeserializeOwned>(http: &reqwest::Client, url: &str) -> Result<T> {
    let resp = http.get(url).send().await?.error_for_status()?;
    Ok(resp.json().await?)
}
