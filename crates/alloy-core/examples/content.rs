//! Exercises content management on an existing instance.
//!
//!   cargo run -p alloy-core --example content -- <data-dir> <instance> install <modrinth-slug> [version-number]
//!   cargo run -p alloy-core --example content -- <data-dir> <instance> fixes
//!   cargo run -p alloy-core --example content -- <data-dir> <instance> updates [apply]

use std::time::Instant;

use alloy_core::content::{check, manage, modrinth, scan};
use alloy_core::paths::Dirs;
use alloy_core::{instance, Launcher};

#[tokio::main]
async fn main() -> alloy_core::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let launcher = Launcher::open(Dirs::new(&args[1])).await?;
    let inst = instance::get(&launcher, &args[2]).await?;
    let t = Instant::now();
    match args[3].as_str() {
        "install" => {
            let versions =
                modrinth::project_versions(&launcher, &args[4], &modrinth::compatible_loaders(inst.loader), &[inst.game_version.as_str()])
                    .await?;
            let v = match args.get(5) {
                Some(n) => versions.iter().find(|v| &v.version_number == n).expect("no such version"),
                None => versions.first().expect("no compatible version"),
            };
            let out = manage::install_modrinth(&launcher, &inst, &v.id).await?;
            println!("installed {:?}, unresolved {:?}, blocked {}", out.installed, out.unresolved, out.blocked.len());
        }
        "fixes" => {
            let files = scan::scan(&launcher, &inst).await?;
            let report = check::check(&inst, &files, None);
            let fixes = manage::suggest(&launcher, &inst, &report, &files).await;
            for (i, p) in report.problems.iter().enumerate() {
                let f: Vec<_> = fixes.iter().filter(|f| f.problem == i).map(|f| f.label.clone()).collect();
                println!("  {} -> {:?}", p.message, f);
            }
        }
        "fixall" => {
            // Applies every install/remove fix once (not "disable"), like the UI's button.
            let files = scan::scan(&launcher, &inst).await?;
            let report = check::check(&inst, &files, None);
            let fixes = manage::suggest(&launcher, &inst, &report, &files).await;
            let mut seen = std::collections::HashSet::new();
            for f in fixes {
                if matches!(f.fix, manage::Fix::Disable { .. }) || !seen.insert(serde_json::to_string(&f.fix).unwrap()) {
                    continue;
                }
                let out = manage::apply_fix(&launcher, &inst, &f.fix).await?;
                println!("{} -> installed {:?}, unresolved {:?}, blocked {}", f.label, out.installed, out.unresolved, out.blocked.len());
            }
        }
        "collect" => {
            let dirs = vec![std::path::PathBuf::from(&args[4])];
            let found = alloy_core::content::blocked::collect(&launcher, &inst, &dirs).await?;
            println!("collected {found:?}; still waiting for {}", alloy_core::content::blocked::pending(&launcher, &inst).await.len());
        }
        "updates" => {
            let ups = manage::check_updates(&launcher, &inst).await?;
            for u in &ups {
                println!("  {}: {:?} -> {}{}", u.title, u.current, u.version_name, if u.prerelease { " (beta)" } else { "" });
            }
            println!("{} updates found in {:.1?}", ups.len(), t.elapsed());
            if args.get(4).map(String::as_str) == Some("apply") {
                let r = manage::apply_updates(&launcher, &inst, ups).await?;
                println!("updated {}, held back {:?}, failed {:?}", r.updated.len(), r.held_back, r.failed);
            }
        }
        other => panic!("unknown command {other}"),
    }
    let files = scan::scan(&launcher, &inst).await?;
    let report = check::check(&inst, &files, None);
    println!("now {} mods, {} problems, {} orphans ({:.1?})", report.mod_count, report.problems.len(), report.orphans.len(), t.elapsed());
    for f in files.iter().filter(|f| f.kind == alloy_core::content::ContentKind::Mod) {
        let m = f.index.as_ref();
        println!(
            "  {:<40} {:<10} auto={} needs={:?}",
            scan::display_name(f),
            format!("{:?}", m.map(|m| m.source)),
            m.is_some_and(|m| m.auto),
            report.requires.get(&f.path).map(|v| v.len()).unwrap_or(0)
        );
    }
    Ok(())
}
