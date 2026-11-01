//! End-to-end check of AlloyMC sign-in against a running alloy-server:
//!   ALLOY_MC_SITE=http://alloymc.localhost:8737 ALLOY_MC_API=http://api.alloymc.localhost:8737 \
//!   cargo run --example alloymc -- <data-dir>
//! Prints the sign-in URL; something (a person, or a test driving a browser) opens it and
//! allows Alloy. Then prints the profile, refreshes it, and signs out.

use std::sync::Arc;

use alloy_core::alloymc::{self, AlloyMc};
use alloy_core::auth::FileSecretStore;
use alloy_core::paths::Dirs;
use alloy_core::Launcher;

#[tokio::main]
async fn main() -> alloy_core::Result<()> {
    let dir = std::env::args().nth(1).unwrap_or_else(|| "/tmp/alloymc-example".into());
    let launcher = Launcher::open(Dirs::new(dir.clone())).await?;
    let secrets = Arc::new(FileSecretStore::new(std::path::PathBuf::from(&dir).join("secrets.json")));
    let mc = AlloyMc::new(launcher, secrets);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let pending = mc.begin("Alloy example", port);
    println!("URL {}", pending.url);
    let done = format!("{}/auth/app/done", alloymc::site_url());
    let (code, state) = alloymc::wait_for_code(&listener, &done).await?.map_err(alloy_core::Error::other)?;
    assert_eq!(state, pending.state, "state must round-trip");
    let p = mc.finish(&code, &pending.verifier).await?;
    println!("SIGNED_IN @{} ({}) followers={}", p.handle, p.display_name, p.followers);
    let cached = mc.cached().await.expect("cached profile");
    println!("CACHED @{}", cached.handle);
    let again = mc.refresh().await?.expect("still signed in");
    println!("REFRESHED @{}", again.handle);
    mc.sign_out().await?;
    println!("SIGNED_OUT refresh={:?}", mc.refresh().await?.map(|p| p.handle));
    Ok(())
}
