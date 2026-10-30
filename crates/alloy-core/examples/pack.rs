//! Installs a Modrinth modpack, scans and checks it, and optionally launches it.
//!
//!   cargo run -p alloy-core --example pack -- <data-dir> <modrinth-slug> [--game 1.20.1] [--launch 180]

use std::sync::Arc;
use std::time::{Duration, Instant};

use alloy_core::auth::{Accounts, FileSecretStore};
use alloy_core::content::{check, modrinth, scan};
use alloy_core::launch::{self, LaunchOptions};
use alloy_core::paths::Dirs;
use alloy_core::{install, modpack, Launcher};

#[tokio::main]
async fn main() -> alloy_core::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let launcher = Launcher::open(Dirs::new(&args[1])).await?;
    let slug = &args[2];

    let games: Vec<String> = flag("--game").into_iter().collect();
    let games: Vec<&str> = games.iter().map(String::as_str).collect();
    let versions = modrinth::project_versions(&launcher, slug, &[], &games).await?;
    let v = versions.first().expect("no version");
    let file = v.primary_file().expect("no file");
    println!("{slug} {} ({} MB)", v.version_number, file.size / 1_000_000);

    let t = Instant::now();
    let progress: alloy_core::progress::ProgressFn = Arc::new(|_| {});
    let mut result = modpack::import_url(&launcher, &file.url, &progress).await?;
    println!("imported in {:.1?}; {} blocked", t.elapsed(), result.blocked.len());
    let t = Instant::now();
    let resolved = install::install(&launcher, &mut result.instance, &progress, false).await?;
    println!(
        "installed {} {:?} {:?} in {:.1?}",
        result.instance.game_version,
        result.instance.loader,
        result.instance.loader_version,
        t.elapsed()
    );

    let t = Instant::now();
    let files = scan::scan(&launcher, &result.instance).await?;
    let cold = t.elapsed();
    let t = Instant::now();
    let files2 = scan::scan(&launcher, &result.instance).await?;
    let warm = t.elapsed();
    let with_meta = files.iter().filter(|f| f.jar.as_ref().is_some_and(|j| !j.mods.is_empty())).count();
    println!("scan: {} files ({with_meta} with metadata), cold {cold:.1?}, cached {warm:.1?}", files2.len());

    let t = Instant::now();
    let report = check::check(&result.instance, &files, Some(resolved.json.java_major()));
    println!(
        "check in {:.1?}: {} mods, {} problems, {} edges",
        t.elapsed(),
        report.mod_count,
        report.problems.len(),
        report.requires.values().map(Vec::len).sum::<usize>()
    );
    for p in &report.problems {
        println!("  {:?}: {}", p.severity, p.message);
    }

    if let Some(secs) = flag("--launch").and_then(|s| s.parse::<u64>().ok()) {
        let accounts = Accounts::new(launcher.clone(), Arc::new(FileSecretStore::new(launcher.dirs.root().join("secrets.json"))));
        let account = accounts.add_offline("PackTest").await?;
        let session = accounts.session(&account.id).await?;
        let cmd = launch::prepare(&launcher, &result.instance, &resolved, &session, &LaunchOptions::default()).await?;
        let started = Instant::now();
        let mut child = launch::spawn(
            &cmd,
            Arc::new(move |l| {
                let line = &l.line;
                if line.contains("Created:") && line.contains("atlas")
                    || line.contains("Exception")
                    || line.contains("ERROR")
                    || line.contains("Loading Minecraft")
                    || line.contains("mod loading")
                    || line.contains("Sound engine started")
                {
                    println!("  [{:>5.1}s] {}", started.elapsed().as_secs_f32(), line.chars().take(220).collect::<String>());
                }
            }),
        )?;
        match tokio::time::timeout(Duration::from_secs(secs), child.wait()).await {
            Ok(s) => println!("game exited: {:?}", s?.code()),
            Err(_) => {
                println!("game still running after {secs} s, stopping it");
                child.kill().await?;
            }
        }
    }
    Ok(())
}
