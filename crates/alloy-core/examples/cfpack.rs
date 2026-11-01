//! Installs a CurseForge modpack file, scans and checks it, and lists files only
//! CurseForge's website hands out.
//!
//!   cargo run -p alloy-core --example cfpack -- <data-dir> <mod-id> <file-id>

use std::time::Instant;

use alloy_core::content::{check, curseforge, scan};
use alloy_core::paths::Dirs;
use alloy_core::{install, modpack, Launcher};

#[tokio::main]
async fn main() -> alloy_core::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let launcher = Launcher::open(Dirs::new(&args[1])).await?;
    let (mod_id, file_id): (u64, u64) = (args[2].parse().unwrap(), args[3].parse().unwrap());
    let f = curseforge::file(&launcher, mod_id, file_id).await?;
    println!("{}: {}", f.display_name, f.download_url.as_deref().unwrap_or("no download url"));
    let t = Instant::now();
    let result = modpack::import_url(&launcher, f.download_url.as_deref().unwrap(), &alloy_core::progress::noop()).await?;
    println!("imported in {:.1?}; {} files only on CurseForge's website:", t.elapsed(), result.blocked.len());
    for b in &result.blocked {
        println!("  {} ({})", b.title, b.file_name);
    }
    let inst = result.instance;
    let resolved = install::resolve(&launcher, &inst).await.ok();
    let t = Instant::now();
    let files = scan::scan(&launcher, &inst).await?;
    println!("{} files scanned in {:.1?}", files.len(), t.elapsed());
    let report = check::check(&inst, &files, resolved.map(|r| r.json.java_major()));
    println!("{} problems", report.problems.len());
    for p in &report.problems {
        println!("  {:?}: {}", p.severity, p.message);
    }
    Ok(())
}
