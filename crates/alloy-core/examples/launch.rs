//! Launches an existing instance offline and prints the interesting log lines.
//!
//!   cargo run -p alloy-core --example launch -- <data-dir> <instance-id> [seconds]

use std::sync::Arc;
use std::time::{Duration, Instant};

use alloy_core::auth::{Accounts, FileSecretStore};
use alloy_core::launch::{self, LaunchOptions};
use alloy_core::paths::Dirs;
use alloy_core::{install, instance, Launcher};

#[tokio::main]
async fn main() -> alloy_core::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let launcher = Launcher::open(Dirs::new(&args[1])).await?;
    let inst = instance::get(&launcher, &args[2]).await?;
    let secs: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(180);
    let resolved = install::resolve(&launcher, &inst).await?;
    let accounts = Accounts::new(launcher.clone(), Arc::new(FileSecretStore::new(launcher.dirs.root().join("secrets.json"))));
    let account = accounts.add_offline("LaunchTest").await?;
    let session = accounts.session(&account.id).await?;
    let cmd = launch::prepare(&launcher, &inst, &resolved, &session, &LaunchOptions::default()).await?;
    let started = Instant::now();
    let mut child = launch::spawn(
        &cmd,
        Arc::new(move |l| {
            let line = &l.line;
            let keep = [
                "Created:",
                "Exception",
                "ERROR",
                "mod loading",
                "Missing",
                "requires",
                "LoadingFailed",
                "Sound engine",
                "Reloading ResourceManager",
                "Mod ID",
                "Failure message",
                "dependencies",
            ];
            if std::env::var_os("FULL").is_some() || keep.iter().any(|k| line.contains(k)) && !line.contains("OpenAL") {
                println!("[{:>5.1}s] {}", started.elapsed().as_secs_f32(), line.chars().take(240).collect::<String>());
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
    Ok(())
}
