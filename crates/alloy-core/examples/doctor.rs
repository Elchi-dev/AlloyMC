//! Exercises the crash doctor on a real game:
//!
//!   cargo run -p alloy-core --example doctor -- <data-dir> prepare 1.21.1 sodium lithium modmenu ferrite-core
//!     creates a Fabric instance with those Modrinth mods and prints its id and folder
//!   cargo run -p alloy-core --example doctor -- <data-dir> run <instance-id> [--bisect]
//!     starts it, diagnoses the crash, and with --bisect finds the mod by testing
//!
//! Needs a display (xvfb-run works). Drop a crashing mod into the instance between the two.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use alloy_core::auth::{Accounts, FileSecretStore};
use alloy_core::bisect::{self, Graph, Outcome};
use alloy_core::content::{modrinth, scan};
use alloy_core::crash;
use alloy_core::instance::{self, NewInstance};
use alloy_core::launch::{self, LaunchCommand, LaunchOptions};
use alloy_core::meta::loaders::Loader;
use alloy_core::paths::Dirs;
use alloy_core::{install, Launcher};

#[tokio::main]
async fn main() -> alloy_core::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let dir = args[1].clone();
    let launcher = Launcher::open(Dirs::new(&dir)).await?;
    let accounts = Accounts::new(launcher.clone(), Arc::new(FileSecretStore::new(launcher.dirs.root().join("secrets.json"))));

    if args[2] == "prepare" {
        let game = args[3].clone();
        let mut inst = instance::create(
            &launcher,
            NewInstance { name: format!("doctor {game}"), game_version: game, loader: Loader::Fabric, loader_version: None, icon: None, group: None, fabric_api: Some(true) },
        )
        .await?;
        install::install(&launcher, &mut inst, &alloy_core::progress::noop(), false).await?;
        for slug in &args[4..] {
            let versions = modrinth::project_versions(&launcher, slug, &modrinth::compatible_loaders(inst.loader), &[inst.game_version.as_str()]).await?;
            let v = versions.first().expect("no compatible version");
            println!("+ {slug}: {:?}", modrinth::install_version(&launcher, &inst, &v.id).await?);
        }
        println!("INSTANCE {}", inst.id);
        println!("DIR {}", inst.dir(&launcher).display());
        return Ok(());
    }

    let id = args[3].clone();
    let inst = instance::get(&launcher, &id).await?;
    let account = match accounts.list().await?.1 {
        Some(a) => a,
        None => accounts.add_offline("DoctorTest").await?.id,
    };
    let session = accounts.session(&account).await?;
    let resolved = install::resolve(&launcher, &inst).await?;
    let cmd = launch::prepare(&launcher, &inst, &resolved, &session, &LaunchOptions::default()).await?;

    // One normal start, as Play would do it.
    let started = chrono::Utc::now().timestamp() - 1;
    let (outcome, code, lines) = start(&cmd).await?;
    println!("first start: {outcome:?}, exit {code:?}, {} lines", lines.len());
    let evidence = crash::gather(&launcher, &inst, started, code, lines).await;
    println!("crash report: {:?}", evidence.crash_report.as_ref().map(|(p, _)| p));
    let files = scan::scan(&launcher, &inst).await?;
    let d = crash::diagnose(&evidence, &files, inst.memory_mb.unwrap_or(4096), 8192, 0);
    println!("{}", serde_json::to_string_pretty(&d).unwrap());

    if args.iter().any(|a| a == "--bisect") {
        let graph = Graph::new(&files);
        bisect::begin(&launcher, &inst, &graph).await?;
        let trial = |on: BTreeSet<String>| {
            let (launcher, inst, graph, cmd) = (&launcher, &inst, &graph, &cmd);
            async move {
                bisect::apply(launcher, inst, graph, &on).await?;
                let names: Vec<String> = on.iter().map(|p| graph.name(p)).collect();
                let (o, _, _) = start(cmd).await?;
                println!("  on: {names:?} -> {o:?}");
                Ok(o)
            }
        };
        let found = bisect::run(&graph, trial, |s| println!("step {:?}", s)).await;
        bisect::restore(&launcher, &inst).await?;
        println!("FOUND {:?}", found?);
        let after = scan::scan(&launcher, &inst).await?;
        println!("mods on after restore: {}/{}", after.iter().filter(|f| f.enabled && f.kind == alloy_core::content::ContentKind::Mod).count(), graph.mods.len());
    }
    Ok(())
}

/// Starts the game and waits for a crash or the title screen (then stops it).
async fn start(cmd: &LaunchCommand) -> alloy_core::Result<(Outcome, Option<i32>, Vec<String>)> {
    let lines = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let (tx, mut rx) = tokio::sync::watch::channel(false);
    let keep = lines.clone();
    let mut child = launch::spawn(
        cmd,
        Arc::new(move |l| {
            if crash::reached_menu(&l.line) {
                let _ = tx.send(true);
            }
            keep.lock().unwrap().push(l.line);
        }),
    )?;
    let (outcome, code) = tokio::select! {
        s = child.wait() => (Outcome::Crashed, s.ok().and_then(|s| s.code())),
        _ = async { let _ = rx.wait_for(|v| *v).await; } => {
            match tokio::time::timeout(Duration::from_secs(8), child.wait()).await {
                Ok(Ok(s)) if !s.success() => (Outcome::Crashed, s.code()),
                _ => (Outcome::Started, None),
            }
        }
        _ = tokio::time::sleep(Duration::from_secs(600)) => (Outcome::Started, None),
    };
    let _ = child.kill().await;
    let _ = child.wait().await;
    let out = lines.lock().unwrap().clone();
    Ok((outcome, code, out))
}
