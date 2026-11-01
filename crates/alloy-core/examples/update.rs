//! Asks AlloyMC for the newest release as if this were an older Alloy, downloads it and
//! verifies it like the app does:
//!   cargo run -p alloy-core --example update -- <dir> [pretend-version]
use alloy_core::updates;

#[tokio::main]
async fn main() -> alloy_core::Result<()> {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "/tmp/alloy-update-test".into()));
    let pretend = std::env::args().nth(2).unwrap_or_else(|| "0.0.1".into());
    let http = reqwest::Client::builder().user_agent("Alloy update example").build()?;
    let offer = updates::check_remote(&http, &updates::latest_url(), &pretend).await?;
    println!("offer for {pretend} on {}: {:?}", updates::platform(), offer.as_ref().map(|o| (&o.version, o.file.size)));
    if let Some(o) = offer {
        let t = std::time::Instant::now();
        let found = updates::download(&http, &o, &dir, &pretend).await?;
        println!("downloaded and verified in {:.1?}: {} {} newer={}", t.elapsed(), found.manifest.version, found.manifest.kind, found.newer);
        let again = updates::download(&http, &o, &dir, &pretend).await?;
        println!("second call reuses the file: {}", again.path.display());
    }
    println!("as 0.3.0: {:?}", updates::check_remote(&http, &updates::latest_url(), "0.3.0").await?.map(|o| o.version));
    Ok(())
}
