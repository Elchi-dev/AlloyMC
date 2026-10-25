//! cargo run -p alloy-core --example textures -- <data-dir>
use alloy_core::{paths::Dirs, textures, Launcher};

#[tokio::main]
async fn main() -> alloy_core::Result<()> {
    let dir = std::env::args().nth(1).unwrap_or_else(|| "./tex-data".into());
    let launcher = Launcher::open(Dirs::new(&dir)).await?;
    let t = std::time::Instant::now();
    let path = textures::ensure(&launcher).await?;
    let loaded = textures::load(&launcher).await?;
    println!("{} textures in {:.2?} at {}", loaded.len(), t.elapsed(), path.display());
    Ok(())
}
