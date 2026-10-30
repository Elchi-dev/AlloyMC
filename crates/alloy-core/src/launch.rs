//! Builds the Java command line from a resolved version and starts the game.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use serde::Serialize;
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::auth::LaunchSession;
use crate::install::{log_config_path, ResolvedVersion};
use crate::instance::Instance;
use crate::meta::rules::Features;
use crate::{java, progress, Launcher, Result};

#[derive(Debug, Clone, Default)]
pub struct LaunchOptions {
    pub quick_play: Option<QuickPlay>,
}

#[derive(Debug, Clone)]
pub enum QuickPlay {
    /// World folder name inside `saves/`.
    Singleplayer(String),
    /// `host[:port]`
    Multiplayer(String),
}

#[derive(Debug, Clone)]
pub struct LaunchCommand {
    pub java: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
    secret: String,
}

impl LaunchCommand {
    /// The command line with the access token blanked, safe for logs.
    pub fn redacted(&self) -> String {
        let mut parts = vec![self.java.display().to_string()];
        parts.extend(self.args.iter().map(|a| {
            if !self.secret.is_empty() && self.secret != "0" {
                a.replace(&self.secret, "********")
            } else {
                a.clone()
            }
        }));
        parts.join(" ")
    }
}

const CP_SEP: &str = if cfg!(windows) { ";" } else { ":" };

pub async fn prepare(
    launcher: &Launcher,
    instance: &Instance,
    resolved: &ResolvedVersion,
    session: &LaunchSession,
    opts: &LaunchOptions,
) -> Result<LaunchCommand> {
    let json = &resolved.json;
    let dirs = &launcher.dirs;
    let game_dir = instance.dir(launcher);
    let settings = launcher.settings.read().await.clone();

    let java = match &instance.java_path {
        Some(p) => p.clone(),
        None => java::ensure(launcher, &json.java_component(), json.java_major(), &progress::noop()).await?,
    };

    let features = Features {
        has_custom_resolution: instance.resolution.is_some(),
        has_quick_plays_support: opts.quick_play.is_some(),
        is_quick_play_singleplayer: matches!(opts.quick_play, Some(QuickPlay::Singleplayer(_))),
        is_quick_play_multiplayer: matches!(opts.quick_play, Some(QuickPlay::Multiplayer(_))),
        ..Default::default()
    };

    // Classpath + legacy natives
    let natives_dir = dirs.natives(&json.id);
    tokio::fs::create_dir_all(&natives_dir).await?;
    let mut classpath: Vec<String> = vec![];
    let mut seen = std::collections::HashSet::new();
    for lib in &json.libraries {
        for file in lib.files(&Features::default()) {
            let path = dirs.libraries().join(&file.path);
            if file.native {
                extract_natives(&path, &natives_dir, &file.exclude).await?;
            } else if seen.insert(file.path.clone()) {
                classpath.push(path.display().to_string());
            }
        }
    }
    if resolved.forge.is_some() && !resolved.client_jar.exists() {
        crate::meta::forge::ensure_version_jar(launcher, &json.id, &resolved.vanilla_id).await?;
    }
    classpath.push(resolved.client_jar.display().to_string());

    // Legacy "map_to_resources" versions read sounds from <game>/resources.
    let asset_id = json.asset_index.as_ref().map(|a| a.id.clone()).or_else(|| json.assets.clone()).unwrap_or_else(|| "legacy".into());
    let virtual_dir = dirs.assets().join("virtual").join(&asset_id);
    if matches!(asset_id.as_str(), "pre-1.6" | "legacy") && virtual_dir.exists() {
        copy_missing(&virtual_dir, &game_dir.join("resources")).await?;
    }

    let (width, height) =
        instance.resolution.map(|r| (r.width.to_string(), r.height.to_string())).unwrap_or_else(|| ("854".into(), "480".into()));

    let mut vars: HashMap<&str, String> = HashMap::new();
    vars.insert("auth_player_name", session.username.clone());
    vars.insert("auth_uuid", session.uuid.clone());
    vars.insert("auth_access_token", session.access_token.clone());
    vars.insert("auth_session", format!("token:{}:{}", session.access_token, session.uuid));
    vars.insert("auth_xuid", session.xuid.clone());
    vars.insert("user_type", session.user_type.into());
    vars.insert("user_properties", "{}".into());
    vars.insert("clientid", String::new());
    vars.insert("version_name", json.id.clone());
    vars.insert("version_type", json.kind.clone());
    vars.insert("game_directory", game_dir.display().to_string());
    vars.insert("assets_root", dirs.assets().display().to_string());
    vars.insert("game_assets", virtual_dir.display().to_string());
    vars.insert("assets_index_name", asset_id.clone());
    vars.insert("natives_directory", natives_dir.display().to_string());
    vars.insert("library_directory", dirs.libraries().display().to_string());
    vars.insert("classpath_separator", CP_SEP.into());
    vars.insert("classpath", classpath.join(CP_SEP));
    vars.insert("launcher_name", "Alloy".into());
    vars.insert("launcher_version", env!("CARGO_PKG_VERSION").into());
    vars.insert("resolution_width", width.clone());
    vars.insert("resolution_height", height.clone());
    vars.insert("quickPlayPath", game_dir.join("quickPlay/log.json").display().to_string());
    match &opts.quick_play {
        Some(QuickPlay::Singleplayer(world)) => {
            vars.insert("quickPlaySingleplayer", world.clone());
        }
        Some(QuickPlay::Multiplayer(server)) => {
            vars.insert("quickPlayMultiplayer", server.clone());
        }
        None => {}
    }

    // JVM arguments
    let memory = instance.memory_mb.unwrap_or(settings.default_memory_mb).max(512);
    let mut args: Vec<String> = vec![format!("-Xmx{memory}M"), format!("-Xms{}M", (memory / 2).min(2048))];
    args.extend(settings.default_jvm_args.iter().cloned());
    args.extend(instance.jvm_args.iter().cloned());

    match json.arguments.as_ref().filter(|a| !a.jvm.is_empty()) {
        Some(a) => args.extend(a.jvm.iter().flat_map(|arg| arg.resolve(&features))),
        None => {
            if cfg!(target_os = "macos") {
                args.push("-XstartOnFirstThread".into());
            }
            args.push("-Djava.library.path=${natives_directory}".into());
            args.push("-cp".into());
            args.push("${classpath}".into());
        }
    }
    if let Some(log) = json.logging.as_ref().and_then(|l| l.client.as_ref()) {
        let path = log_config_path(launcher, &log.file.id);
        if path.exists() {
            args.push(log.argument.replace("${path}", &path.display().to_string()));
        }
    }
    args.push(json.main_class.clone());

    // Game arguments
    match (&json.arguments, &json.minecraft_arguments) {
        (Some(a), _) if !a.game.is_empty() => {
            args.extend(a.game.iter().flat_map(|arg| arg.resolve(&features)));
        }
        (_, Some(legacy)) => {
            args.extend(legacy.split_whitespace().map(str::to_string));
            if instance.resolution.is_some() {
                args.extend(["--width".into(), width, "--height".into(), height]);
            }
            if let Some(QuickPlay::Multiplayer(server)) = &opts.quick_play {
                let (host, port) = server.split_once(':').unwrap_or((server, "25565"));
                args.extend(["--server".into(), host.into(), "--port".into(), port.into()]);
            }
        }
        _ => {}
    }
    args.extend(instance.game_args.iter().cloned());

    let args = args.into_iter().map(|a| substitute(&a, &vars)).collect();
    Ok(LaunchCommand { java, args, cwd: game_dir, env: instance.env.clone(), secret: session.access_token.clone() })
}

/// Replaces every `${name}` with its value. Unknown placeholders are left as-is.
fn substitute(arg: &str, vars: &HashMap<&str, String>) -> String {
    let mut out = String::with_capacity(arg.len());
    let mut rest = arg;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find('}') {
            Some(end) => {
                let key = &after[..end];
                match vars.get(key) {
                    Some(v) => out.push_str(v),
                    None => out.push_str(&rest[start..start + 3 + end]),
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

async fn extract_natives(jar: &Path, dest: &Path, exclude: &[String]) -> Result<()> {
    let jar = jar.to_path_buf();
    let dest = dest.to_path_buf();
    let exclude = exclude.to_vec();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&jar)?)?;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i)?;
            let name = entry.name().to_string();
            if entry.is_dir() || exclude.iter().any(|e| name.starts_with(e)) || name.starts_with("META-INF") {
                continue;
            }
            let out = crate::util::safe_join(&dest, &name)?;
            if out.exists() {
                continue;
            }
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut buf = vec![];
            entry.read_to_end(&mut buf)?;
            std::fs::write(&out, buf)?;
        }
        Ok(())
    })
    .await
    .map_err(|e| crate::Error::other(e.to_string()))?
}

async fn copy_missing(from: &Path, to: &Path) -> Result<()> {
    let mut stack = vec![(from.to_path_buf(), to.to_path_buf())];
    while let Some((src, dst)) = stack.pop() {
        tokio::fs::create_dir_all(&dst).await?;
        let mut rd = tokio::fs::read_dir(&src).await?;
        while let Some(e) = rd.next_entry().await? {
            let target = dst.join(e.file_name());
            if e.file_type().await?.is_dir() {
                stack.push((e.path(), target));
            } else if !target.exists() {
                tokio::fs::copy(e.path(), &target).await?;
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct LogLine {
    pub stream: &'static str,
    pub line: String,
}

/// Starts the game. Each stdout/stderr line goes to `on_line`; await the returned child for the exit.
pub fn spawn(cmd: &LaunchCommand, on_line: Arc<dyn Fn(LogLine) + Send + Sync>) -> Result<tokio::process::Child> {
    tracing::info!("launching: {}", cmd.redacted());
    let mut command = tokio::process::Command::new(&cmd.java);
    command
        .args(&cmd.args)
        .current_dir(&cmd.cwd)
        .envs(cmd.env.iter().cloned())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(false);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

    let mut child = command.spawn()?;
    if let Some(out) = child.stdout.take() {
        let f = on_line.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(out).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                f(LogLine { stream: "stdout", line });
            }
        });
    }
    if let Some(err) = child.stderr.take() {
        let f = on_line;
        tokio::spawn(async move {
            let mut lines = BufReader::new(err).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                f(LogLine { stream: "stderr", line });
            }
        });
    }
    Ok(child)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitutes_placeholders() {
        let mut vars = HashMap::new();
        vars.insert("a", "1".to_string());
        vars.insert("natives_directory", "/n".to_string());
        assert_eq!(substitute("-Djava.library.path=${natives_directory}", &vars), "-Djava.library.path=/n");
        assert_eq!(substitute("${a}-${a}", &vars), "1-1");
        assert_eq!(substitute("${unknown} ${a}", &vars), "${unknown} 1");
        assert_eq!(substitute("broken ${a", &vars), "broken ${a");
    }
}
