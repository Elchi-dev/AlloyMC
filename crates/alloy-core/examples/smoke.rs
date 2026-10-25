//! End-to-end smoke test: create an instance, install it, launch it with an offline account.
//!
//!   cargo run -p alloy-core --example smoke -- <data-dir> <game-version> [fabric|quilt] [--modrinth sodium]
//!
//! On a machine with a display the game window opens; headless it gets as far as LWJGL init.

use std::sync::Arc;
use std::time::Duration;

use alloy_core::auth::{Accounts, FileSecretStore};
use alloy_core::content::modrinth;
use alloy_core::instance::{self, NewInstance};
use alloy_core::launch::{self, LaunchOptions};
use alloy_core::meta::loaders::Loader;
use alloy_core::paths::Dirs;
use alloy_core::{install, Launcher};

#[tokio::main]
async fn main() -> alloy_core::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let dir = args.get(1).cloned().unwrap_or_else(|| "./smoke-data".into());
    let game = args.get(2).cloned().unwrap_or_else(|| "26.2".into());
    let loader = match args.get(3).map(String::as_str) {
        Some("fabric") => Loader::Fabric,
        Some("quilt") => Loader::Quilt,
        _ => Loader::Vanilla,
    };
    let mod_slug = args.iter().position(|a| a == "--modrinth").and_then(|i| args.get(i + 1)).cloned();

    let launcher = Launcher::open(Dirs::new(&dir)).await?;
    let accounts = Accounts::new(launcher.clone(), Arc::new(FileSecretStore::new(launcher.dirs.root().join("secrets.json"))));
    let account = accounts.add_offline("SmokeTest").await?;

    let mut inst = instance::create(
        &launcher,
        NewInstance {
            name: format!("smoke {game} {loader:?}"),
            game_version: game,
            loader,
            loader_version: None,
            icon: None,
            group: None,
        },
    )
    .await?;
    println!("instance {} (loader version {:?})", inst.id, inst.loader_version);

    let started = std::time::Instant::now();
    let last = std::sync::Mutex::new(None);
    let progress: alloy_core::progress::ProgressFn = Arc::new(move |p| {
        let mut l = last.lock().unwrap();
        if *l != Some(p.stage) || p.current == p.total {
            println!("  {:?} {}/{}", p.stage, p.current, p.total);
            *l = Some(p.stage);
        }
    });
    let resolved = install::install(&launcher, &mut inst, &progress, false).await?;
    println!("installed in {:.1?}; main class {}", started.elapsed(), resolved.json.main_class);

    if let Some(slug) = mod_slug {
        let versions = modrinth::project_versions(
            &launcher,
            &slug,
            &modrinth::compatible_loaders(inst.loader),
            &[inst.game_version.as_str()],
        )
        .await?;
        let v = versions.first().expect("no compatible version");
        let names = modrinth::install_version(&launcher, &inst, &v.id).await?;
        println!("installed mods: {names:?}");
    }

    let session = accounts.session(&account.id).await?;
    let cmd = launch::prepare(&launcher, &inst, &resolved, &session, &LaunchOptions::default()).await?;
    println!("java: {}", cmd.java.display());

    let mut child = launch::spawn(&cmd, Arc::new(|l| println!("  [{}] {}", l.stream, l.line)))?;
    let status = tokio::time::timeout(Duration::from_secs(60), child.wait()).await;
    match status {
        Ok(s) => println!("game exited: {:?}", s?.code()),
        Err(_) => {
            println!("game still running after 60 s, stopping it");
            child.kill().await?;
        }
    }
    Ok(())
}
