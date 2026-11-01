//! Searches CurseForge and walks every other call Alloy makes on the first hit:
//!   cargo run -p alloy-core --example curseforge -- "all the mods"
//! With no key in Settings this goes through the AlloyMC proxy (ALLOY_MC_API to point it at
//! a local alloy-server).
use alloy_core::content::{curseforge, ContentKind, SearchQuery};
use alloy_core::{paths::Dirs, Launcher};

#[tokio::main]
async fn main() -> alloy_core::Result<()> {
    let q = std::env::args().nth(1).unwrap_or_else(|| "create".into());
    let launcher = Launcher::open(Dirs::new("/tmp/cf-test")).await?;
    let page =
        curseforge::search(&launcher, &SearchQuery { query: q, kind: Some(ContentKind::Modpack), limit: 5, ..Default::default() }).await?;
    for h in &page.hits {
        println!("{:>12} downloads  {}", h.downloads, h.title);
    }
    let Some(first) = page.hits.first() else { return Ok(()) };
    let id: u64 = first.id.parse().unwrap();

    let m = curseforge::get_mod(&launcher, id).await?;
    println!("mod        {} ({})", m.name, m.slug);
    let by_slug = curseforge::mod_by_slug(&launcher, &m.slug, Some(ContentKind::Modpack)).await?;
    println!("by slug    {:?}", by_slug.map(|m| m.id));
    let files = curseforge::mod_files(&launcher, id, None, None).await?;
    println!("files      {} (newest {})", files.len(), files.first().map(|f| f.file_name.as_str()).unwrap_or("-"));
    if let Some(f) = files.first() {
        let one = curseforge::file(&launcher, id, f.id).await?;
        println!("file       {} bytes", one.file_length);
        let batch = curseforge::files(&launcher, &[f.id]).await?;
        println!("batch      {} file(s)", batch.len());
        let fp = curseforge::match_fingerprints(&launcher, &one.file_fingerprint.map(|f| vec![f as u32]).unwrap_or_default()).await?;
        println!("fingerprint matches {}", fp.len());
    }
    let mods = curseforge::mods(&launcher, &[id]).await?;
    println!("mods       {}", mods.len());
    let desc = curseforge::description(&launcher, id).await?;
    println!("description {} chars", desc.len());
    Ok(())
}
