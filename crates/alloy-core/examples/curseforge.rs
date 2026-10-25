//! cargo run -p alloy-core --example curseforge -- "all the mods"
use alloy_core::content::{curseforge, ContentKind, SearchQuery};
use alloy_core::{paths::Dirs, Launcher};

#[tokio::main]
async fn main() -> alloy_core::Result<()> {
    let q = std::env::args().nth(1).unwrap_or_else(|| "create".into());
    let launcher = Launcher::open(Dirs::new("/tmp/cf-test")).await?;
    let page = curseforge::search(&launcher, &SearchQuery { query: q, kind: Some(ContentKind::Modpack), limit: 5, ..Default::default() }).await?;
    for h in page.hits {
        println!("{:>12} downloads  {}", h.downloads, h.title);
    }
    Ok(())
}
