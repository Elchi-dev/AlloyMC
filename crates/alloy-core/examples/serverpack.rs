//! Makes an instance of the given loader, adds a few mods (one client-only), builds a
//! server pack from it and starts that server until it's ready, then stops it.
//!
//!   cargo run -p alloy-core --example serverpack -- <data-dir> <fabric|forge|neoforge|quilt> <game> [--start]

use std::time::{Duration, Instant};

use alloy_core::content::{manage, modrinth};
use alloy_core::instance::{self, NewInstance};
use alloy_core::meta::loaders::{self, Loader};
use alloy_core::paths::Dirs;
use alloy_core::{serverpack, Launcher};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

#[tokio::main]
async fn main() -> alloy_core::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let launcher = Launcher::open(Dirs::new(&args[1])).await?;
    let loader = match args[2].as_str() {
        "forge" => Loader::Forge,
        "neoforge" => Loader::NeoForge,
        "quilt" => Loader::Quilt,
        _ => Loader::Fabric,
    };
    let game = args[3].clone();
    let lv = loaders::recommended(&launcher, loader, &game).await?;
    let inst = instance::create(&launcher, NewInstance { name: format!("{loader:?} {game}"), game_version: game.clone(), loader, loader_version: lv, icon: None, group: None, fabric_api: None }).await?;
    // A server-side mod and a client-only one.
    let modrinth_loader = loader.modrinth_name().unwrap();
    for slug in ["ferrite-core", "sodium", "jei"] {
        let vs = modrinth::project_versions(&launcher, slug, &[modrinth_loader], &[game.as_str()]).await?;
        if let Some(v) = vs.first() {
            let out = manage::install_modrinth(&launcher, &inst, &v.id).await?;
            println!("installed {slug}: {:?}", out.installed);
        }
    }
    let dest = std::path::PathBuf::from(&args[1]).join("server");
    let _ = std::fs::remove_dir_all(&dest);
    let t = Instant::now();
    let progress: alloy_core::progress::ProgressFn = std::sync::Arc::new(|p: alloy_core::progress::Progress| println!("  {:?}", p.message));
    let r = serverpack::build(&launcher, &inst, &dest, true, None, &progress).await?;
    println!("built in {:.1?}: {} mods, left out {:?}, start with {}", t.elapsed(), r.mods, r.left_out, r.start);
    for e in std::fs::read_dir(&dest)? {
        println!("  {}", e?.file_name().to_string_lossy());
    }
    if args.iter().any(|a| a == "--start") {
        let java = alloy_core::java::ensure(&launcher, &alloy_core::meta::mojang::version_json(&launcher, &game).await?.java_component(), r.java_major, &alloy_core::progress::noop()).await?;
        let script = if dest.join("run.sh").exists() { "run.sh" } else { "start.sh" };
        let path = format!("{}:{}", java.parent().unwrap().display(), std::env::var("PATH").unwrap_or_default());
        let mut child = tokio::process::Command::new("sh")
            .arg(script)
            .current_dir(&dest)
            .env("PATH", path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()?;
        let mut lines = tokio::io::BufReader::new(child.stdout.take().unwrap()).lines();
        let started = tokio::time::timeout(Duration::from_secs(600), async {
            while let Ok(Some(l)) = lines.next_line().await {
                if l.contains("Done (") {
                    println!("SERVER READY: {l}");
                    return true;
                }
                if l.contains("ERROR") || l.contains("Exception") {
                    println!("  {l}");
                }
            }
            false
        })
        .await
        .unwrap_or(false);
        let mut stdin = child.stdin.take().unwrap();
        let _ = stdin.write_all(b"stop\n").await;
        let _ = tokio::time::timeout(Duration::from_secs(60), child.wait()).await;
        let _ = child.kill().await;
        println!("started: {started}");
    }
    Ok(())
}
