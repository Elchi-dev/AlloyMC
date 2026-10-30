//! Scans and checks an existing instance.
//!
//!   cargo run -p alloy-core --example check -- <data-dir> <instance-id>

use std::time::Instant;

use alloy_core::content::{check, scan};
use alloy_core::paths::Dirs;
use alloy_core::{install, instance, Launcher};

#[tokio::main]
async fn main() -> alloy_core::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let launcher = Launcher::open(Dirs::new(&args[1])).await?;
    let inst = instance::get(&launcher, &args[2]).await?;
    let resolved = install::resolve(&launcher, &inst).await?;
    let t = Instant::now();
    let files = scan::scan(&launcher, &inst).await?;
    let scan_time = t.elapsed();
    let t = Instant::now();
    let report = check::check(&inst, &files, Some(resolved.json.java_major()));
    println!("{} files scanned in {scan_time:.1?}; checked in {:.1?}: {} problems", files.len(), t.elapsed(), report.problems.len());
    for p in &report.problems {
        println!("  {:?}: {}", p.severity, p.message);
    }
    Ok(())
}
