//! Measures the launch path: install of an already-downloaded version (full hash check vs
//! quick size check) and the time from "Play" to a ready Java command.
//!   cargo run --release -p alloy-core --example bench -- <data-dir> <version>
use std::sync::Arc;
use std::time::Instant;

use alloy_core::auth::{Accounts, FileSecretStore};
use alloy_core::instance::{self, NewInstance};
use alloy_core::launch::{self, LaunchOptions};
use alloy_core::meta::loaders::Loader;
use alloy_core::{install, paths::Dirs, progress, Launcher};

#[tokio::main]
async fn main() -> alloy_core::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let launcher = Launcher::open(Dirs::new(&args[1])).await?;
    let version = args.get(2).cloned().unwrap_or_else(|| "26.2".into());
    let accounts = Accounts::new(launcher.clone(), Arc::new(FileSecretStore::new(launcher.dirs.root().join("secrets.json"))));
    let account = accounts.add_offline("Bench").await?;
    let new = || NewInstance {
        name: format!("bench {version}"),
        game_version: version.clone(),
        loader: Loader::Vanilla,
        loader_version: None,
        icon: None,
        group: None,
        fabric_api: None,
    };

    let mut a = instance::create(&launcher, new()).await?;
    let t = Instant::now();
    install::install(&launcher, &mut a, &progress::noop(), true).await?;
    let full = t.elapsed();

    let mut b = instance::create(&launcher, new()).await?;
    let t = Instant::now();
    install::install(&launcher, &mut b, &progress::noop(), false).await?;
    let quick = t.elapsed();

    let t = Instant::now();
    let resolved = install::resolve(&launcher, &b).await?;
    let session = accounts.session(&account.id).await?;
    let cmd = launch::prepare(&launcher, &b, &resolved, &session, &LaunchOptions::default()).await?;
    let ready = t.elapsed();

    println!("second instance, full hash check : {full:>8.2?}");
    println!("second instance, quick check     : {quick:>8.2?}");
    println!("Play -> Java command ready       : {ready:>8.2?}  ({} args)", cmd.args.len());
    instance::trash(&launcher, &a.id).await?;
    instance::trash(&launcher, &b.id).await?;
    Ok(())
}
