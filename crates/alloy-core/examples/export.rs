//! Exports an instance as .mrpack and CurseForge .zip, imports both back, and compares:
//!   cargo run -p alloy-core --example export -- <data-dir> <instance-id> <out-dir>

use std::collections::BTreeSet;

use alloy_core::content::scan;
use alloy_core::export::{self, Format, Options};
use alloy_core::paths::Dirs;
use alloy_core::{instance, modpack, Launcher};

#[tokio::main]
async fn main() -> alloy_core::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let launcher = Launcher::open(Dirs::new(&args[1])).await?;
    let inst = instance::get(&launcher, &args[2]).await?;
    let out = std::path::PathBuf::from(&args[3]);
    let names = |files: Vec<scan::Scanned>| -> BTreeSet<String> {
        files.into_iter().filter(|f| f.enabled && f.kind == alloy_core::content::ContentKind::Mod).map(|f| f.sha1).collect()
    };
    let before = names(scan::scan(&launcher, &inst).await?);
    println!("candidates: {:?}", export::candidates(&launcher, &inst).await?.iter().map(|c| (&c.path, c.suggested)).collect::<Vec<_>>());

    for format in [Format::Mrpack, Format::CurseForge] {
        let opts = Options {
            format,
            name: "Doctor Test".into(),
            version: "1.0".into(),
            author: Some("Elchi".into()),
            summary: Some("Exported by Alloy".into()),
            include: vec!["config".into()],
            dest: out.join(export::file_name("Doctor Test", "1.0", format)),
        };
        let report = export::export(&launcher, &inst, &opts, &alloy_core::progress::noop()).await?;
        println!("{format:?}: {} bytes, {} linked, bundled {:?}", report.size, report.linked, report.bundled);

        let imported = modpack::import_file(&launcher, std::path::Path::new(&report.path), &alloy_core::progress::noop()).await?;
        let after = names(scan::scan(&launcher, &imported.instance).await?);
        println!(
            "  re-imported as {} ({} {:?} {:?}): same mods = {}, missing {:?}",
            imported.instance.id,
            imported.instance.game_version,
            imported.instance.loader,
            imported.instance.loader_version,
            before == after,
            before.difference(&after).collect::<Vec<_>>()
        );
        let cfg = imported.instance.dir(&launcher).join("config");
        println!("  config files: {}", std::fs::read_dir(cfg).map(|r| r.count()).unwrap_or(0));
    }
    Ok(())
}
