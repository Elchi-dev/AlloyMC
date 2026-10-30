//! Forge and NeoForge, installed from their official installers the way the installer
//! itself does it, without running its GUI:
//!
//! 1. Download the installer jar from the loader's Maven (checked against Maven's `.sha1`).
//! 2. Read `install_profile.json` and the launch profile (`version.json`) inside it.
//! 3. Copy the libraries bundled in the installer (`maven/`) into the shared libraries folder
//!    and download the rest.
//! 4. Run the installer's processors (deobfuscating and patching the Minecraft jar) with the
//!    game's Java. The results are shared: a second instance of the same version skips them.
//!
//! Three installer generations are handled: NeoForge and Forge 1.13+ (`spec` 1, processors),
//! Forge 1.12.2's late installers (`spec` 0, no processors) and legacy Forge 1.6 to 1.12
//! (`install` + `versionInfo`). Forge for 1.5.2 and older patched the game jar itself and
//! is not supported.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};

use super::loaders::Loader;
use super::version::{Library, VersionJson};
use crate::download::{fetch_bytes, DownloadTask};
use crate::progress::{emit, ProgressFn, Stage, Unit};
use crate::util::{maven_path, sha1_file};
use crate::{Error, Launcher, Result};

const FORGE_MAVEN: &str = "https://maven.minecraftforge.net";
const NEO_MAVEN: &str = "https://maven.neoforged.net/releases";
const FORGE_METADATA_TTL_SECS: u64 = 6 * 3600;

const CP_SEP: &str = if cfg!(windows) { ";" } else { ":" };

/// Which installer: loader, Minecraft version and loader version as the version lists show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub loader: Loader,
    pub game: String,
    pub version: String,
}

impl Target {
    pub fn new(loader: Loader, game: &str, version: &str) -> Result<Self> {
        if !matches!(loader, Loader::Forge | Loader::NeoForge) {
            return Err(Error::other(format!("{loader:?} has no installer")));
        }
        Ok(Self { loader, game: game.into(), version: version.into() })
    }

    /// Stable name for cached files, e.g. `forge-47.4.28-1.20.1`.
    pub fn cache_id(&self) -> String {
        let name = if self.loader == Loader::Forge { "forge" } else { "neoforge" };
        format!("{name}-{}-{}", self.version, self.game)
    }

    fn label(&self) -> &'static str {
        if self.loader == Loader::Forge {
            "Forge"
        } else {
            "NeoForge"
        }
    }
}

// ---------- install_profile.json ----------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstallProfile {
    #[serde(default)]
    json: Option<String>,
    #[serde(default)]
    data: HashMap<String, SidedValue>,
    #[serde(default)]
    processors: Vec<Processor>,
    #[serde(default)]
    libraries: Vec<Library>,
    /// Legacy installers (1.6 to 1.12).
    #[serde(default)]
    install: Option<LegacyInstall>,
    #[serde(default)]
    version_info: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct SidedValue {
    #[serde(default)]
    client: String,
}

#[derive(Debug, Clone, Deserialize)]
struct Processor {
    jar: String,
    #[serde(default)]
    classpath: Vec<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    outputs: HashMap<String, String>,
    #[serde(default)]
    sides: Option<Vec<String>>,
}

impl Processor {
    fn runs_on_client(&self) -> bool {
        self.sides.as_ref().is_none_or(|s| s.iter().any(|s| s == "client"))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyInstall {
    /// Maven coordinate of the universal jar.
    path: String,
    /// The universal jar's name inside the installer.
    file_path: String,
}

/// What `prepare` leaves behind once every processor succeeded, so later installs can skip them.
#[derive(Debug, Serialize, Deserialize)]
struct Done {
    files: Vec<(String, u64, String)>,
}

// ---------- locating the installer ----------

/// Forge's Maven version for a loader version: usually `<game>-<version>`, sometimes with a
/// branch suffix (`1.7.10-10.13.4.1614-1.7.10`).
async fn forge_maven_version(launcher: &Launcher, game: &str, version: &str) -> Result<String> {
    let path = launcher.dirs.meta().join("forge").join("maven-metadata.xml");
    let fresh = tokio::fs::metadata(&path)
        .await
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age.as_secs() < FORGE_METADATA_TTL_SECS);
    let xml = match (fresh, tokio::fs::read_to_string(&path).await) {
        (true, Ok(xml)) => xml,
        (_, cached) => match fetch_bytes(&launcher.http, &format!("{FORGE_MAVEN}/net/minecraftforge/forge/maven-metadata.xml")).await {
            Ok(bytes) => {
                if let Some(parent) = path.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                tokio::fs::write(&path, &bytes).await?;
                String::from_utf8_lossy(&bytes).into_owned()
            }
            Err(e) => cached.map_err(|_| e)?,
        },
    };
    pick_forge_version(&xml, game, version).ok_or_else(|| Error::NotFound(format!("Forge {version} for Minecraft {game}")))
}

fn pick_forge_version(xml: &str, game: &str, version: &str) -> Option<String> {
    let exact = format!("{game}-{version}");
    let prefixed = format!("{exact}-");
    let mut best = None;
    for v in xml.split("<version>").skip(1).filter_map(|s| s.split("</version>").next()) {
        let v = v.trim();
        if v == exact {
            return Some(v.to_string());
        }
        if v.starts_with(&prefixed) {
            best = Some(v.to_string());
        }
    }
    best
}

/// The installer's URL on the loader's own Maven.
pub async fn installer_url(launcher: &Launcher, t: &Target) -> Result<String> {
    Ok(match t.loader {
        Loader::Forge => {
            let mv = forge_maven_version(launcher, &t.game, &t.version).await?;
            format!("{FORGE_MAVEN}/net/minecraftforge/forge/{mv}/forge-{mv}-installer.jar")
        }
        // NeoForge for 1.20.1 still used Forge's artifact name.
        _ if t.game == "1.20.1" => {
            let v = format!("1.20.1-{}", t.version.trim_start_matches("1.20.1-"));
            format!("{NEO_MAVEN}/net/neoforged/forge/{v}/forge-{v}-installer.jar")
        }
        _ => format!("{NEO_MAVEN}/net/neoforged/neoforge/{0}/neoforge-{0}-installer.jar", t.version),
    })
}

fn installer_path(launcher: &Launcher, t: &Target) -> PathBuf {
    launcher.dirs.meta().join("installers").join(format!("{}-installer.jar", t.cache_id()))
}

/// Downloads the installer once, verified against the `.sha1` Maven publishes next to it.
pub async fn installer(launcher: &Launcher, t: &Target) -> Result<PathBuf> {
    let path = installer_path(launcher, t);
    if is_zip(&path).await {
        return Ok(path);
    }
    let url = installer_url(launcher, t).await?;
    let sha1 = match fetch_bytes(&launcher.http, &format!("{url}.sha1")).await {
        Ok(b) => String::from_utf8_lossy(&b).split_whitespace().next().filter(|h| h.len() == 40).map(str::to_string),
        Err(_) => None,
    };
    crate::download::download_with_retry(&launcher.http, &DownloadTask::new(&url, &path).sha1(sha1)).await.map_err(|e| match e {
        Error::Download { reason, .. } if reason.contains("404") => Error::Unsupported(format!(
            "{} {} for Minecraft {} has no installer (Forge for 1.5.2 and older isn't supported)",
            t.label(),
            t.version,
            t.game
        )),
        e => e,
    })?;
    Ok(path)
}

async fn is_zip(path: &Path) -> bool {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || std::fs::File::open(&path).ok().and_then(|f| zip::ZipArchive::new(f).ok()).is_some())
        .await
        .unwrap_or(false)
}

fn read_text(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Result<String> {
    let mut entry = zip.by_name(name.trim_start_matches('/')).map_err(|_| Error::other(format!("The installer has no {name}")))?;
    let mut s = String::new();
    entry.read_to_string(&mut s)?;
    Ok(s)
}

async fn read_profile(installer: &Path) -> Result<(InstallProfile, VersionJson)> {
    let installer = installer.to_path_buf();
    tokio::task::spawn_blocking(move || -> Result<(InstallProfile, VersionJson)> {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&installer)?)?;
        let profile: InstallProfile = serde_json::from_str(&read_text(&mut zip, "install_profile.json")?)?;
        let version = match (&profile.version_info, &profile.json) {
            (Some(info), _) => legacy_version(info.clone())?,
            (None, Some(json)) => serde_json::from_str(&read_text(&mut zip, json)?)?,
            (None, None) => serde_json::from_str(&read_text(&mut zip, "version.json")?)?,
        };
        Ok((profile, version))
    })
    .await
    .map_err(|e| Error::other(e.to_string()))?
}

/// Legacy `versionInfo`: drops libraries only the server needs.
fn legacy_version(mut info: serde_json::Value) -> Result<VersionJson> {
    if let Some(libs) = info.get_mut("libraries").and_then(|l| l.as_array_mut()) {
        libs.retain(|l| l.get("clientreq").and_then(|v| v.as_bool()) != Some(false));
        for lib in libs.iter_mut() {
            // Old profiles list checksums for files the Maven no longer serves the same way.
            if let Some(obj) = lib.as_object_mut() {
                obj.remove("checksums");
            }
        }
    }
    Ok(serde_json::from_value(info)?)
}

// ---------- public steps ----------

fn profile_cache(launcher: &Launcher, t: &Target) -> PathBuf {
    launcher.dirs.versions().join(t.cache_id()).join("profile.json")
}

/// The launch profile, ready to merge onto vanilla. Cached, so launching never opens the installer.
pub async fn profile(launcher: &Launcher, t: &Target) -> Result<VersionJson> {
    let cache = profile_cache(launcher, t);
    if let Ok(bytes) = tokio::fs::read(&cache).await {
        if let Ok(v) = serde_json::from_slice(&bytes) {
            return Ok(v);
        }
    }
    let path = installer(launcher, t).await?;
    let (_, version) = read_profile(&path).await?;
    crate::settings::write_json_atomic(&cache, &version).await?;
    Ok(version)
}

/// Copies the libraries bundled in the installer into the libraries folder, and returns
/// download tasks for the libraries the processors need (they never go on the classpath).
pub async fn prepare_files(launcher: &Launcher, t: &Target) -> Result<Vec<DownloadTask>> {
    let path = installer(launcher, t).await?;
    let (profile, _) = read_profile(&path).await?;
    let libs = launcher.dirs.libraries();

    let legacy = profile.install.clone();
    let p = path.clone();
    let l = libs.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&p)?)?;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i)?;
            let name = entry.name().to_string();
            let Some(rel) = name.strip_prefix("maven/") else {
                continue;
            };
            if rel.is_empty() || entry.is_dir() {
                continue;
            }
            let out = crate::util::safe_join(&l, rel)?;
            if std::fs::metadata(&out).is_ok_and(|m| m.len() == entry.size()) {
                continue;
            }
            write_entry(&mut entry, &out)?;
        }
        if let Some(legacy) = legacy {
            let rel = maven_path(&legacy.path).ok_or_else(|| Error::other("Bad Forge library name"))?;
            let out = crate::util::safe_join(&l, &rel)?;
            let mut entry = zip.by_name(&legacy.file_path)?;
            if !std::fs::metadata(&out).is_ok_and(|m| m.len() == entry.size()) {
                write_entry(&mut entry, &out)?;
            }
        }
        Ok(())
    })
    .await
    .map_err(|e| Error::other(e.to_string()))??;

    let features = super::rules::Features::default();
    Ok(profile
        .libraries
        .iter()
        .flat_map(|lib| lib.files(&features))
        .filter(|f| !f.url.is_empty())
        .map(|f| DownloadTask::new(&f.url, libs.join(&f.path)).sha1(f.sha1).size(f.size))
        .collect())
}

fn write_entry(entry: &mut impl Read, out: &Path) -> Result<()> {
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = out.with_extension("part");
    {
        let mut file = std::fs::File::create(&tmp)?;
        std::io::copy(entry, &mut file)?;
    }
    std::fs::rename(&tmp, out)?;
    Ok(())
}

/// Runs the installer's client processors. Skipped entirely when a previous run left the
/// same results in place (`quick` trusts sizes, otherwise every file is re-hashed).
pub async fn run_processors(launcher: &Launcher, t: &Target, java: &Path, progress: &ProgressFn, quick: bool) -> Result<()> {
    let path = installer(launcher, t).await?;
    let (profile, _) = read_profile(&path).await?;
    let processors: Vec<&Processor> = profile.processors.iter().filter(|p| p.runs_on_client()).collect();
    if processors.is_empty() {
        return Ok(());
    }

    let done_path = launcher.dirs.meta().join("installers").join(format!("{}.done.json", t.cache_id()));
    if done_matches(&done_path, quick).await {
        return Ok(());
    }

    let libs = launcher.dirs.libraries();
    let work = launcher.dirs.meta().join("installers").join(format!("{}-work", t.cache_id()));
    let _ = tokio::fs::remove_dir_all(&work).await;
    tokio::fs::create_dir_all(&work).await?;

    // Values processors refer to as {KEY}.
    let mut vars: HashMap<String, String> = HashMap::new();
    vars.insert("SIDE".into(), "client".into());
    vars.insert("MINECRAFT_JAR".into(), launcher.dirs.client_jar(&t.game).display().to_string());
    vars.insert("MINECRAFT_VERSION".into(), t.game.clone());
    vars.insert("ROOT".into(), launcher.dirs.root().display().to_string());
    vars.insert("INSTALLER".into(), path.display().to_string());
    vars.insert("LIBRARY_DIR".into(), libs.display().to_string());
    let mut produced: Vec<PathBuf> = vec![];
    for (key, value) in &profile.data {
        let v = &value.client;
        let resolved = if let Some(coord) = v.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            let p = libs.join(maven_path(coord).ok_or_else(|| Error::other(format!("Bad library name {coord}")))?);
            produced.push(p.clone());
            p.display().to_string()
        } else if let Some(lit) = v.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
            lit.to_string()
        } else if v.starts_with('/') {
            extract_data(&path, v, &work).await?.display().to_string()
        } else {
            v.clone()
        };
        vars.insert(key.clone(), resolved);
    }

    let log_path = launcher.dirs.meta().join("installers").join(format!("{}.log", t.cache_id()));
    let mut log = String::new();
    let total = processors.len() as u64;
    for (i, proc) in processors.iter().enumerate() {
        emit(progress, Stage::Loader, i as u64, total, Unit::Steps, Some(format!("Setting up {} ({}/{total})", t.label(), i + 1)));
        if outputs_match(proc, &vars, &libs).await? {
            continue;
        }
        run_one(proc, &vars, &libs, java, &work, &mut log).await.inspect_err(|_| {
            let _ = std::fs::write(&log_path, &log);
        })?;
        verify_outputs(proc, &vars, &libs).await?;
    }
    emit(progress, Stage::Loader, total, total, Unit::Steps, None);
    let _ = tokio::fs::write(&log_path, &log).await;
    let _ = tokio::fs::remove_dir_all(&work).await;

    // Remember what the processors made, so the next instance of this version skips them.
    let mut files = vec![];
    for p in produced {
        if let Ok(meta) = tokio::fs::metadata(&p).await {
            if meta.is_file() {
                files.push((p.display().to_string(), meta.len(), sha1_file(&p).await?));
            }
        }
    }
    crate::settings::write_json_atomic(&done_path, &Done { files }).await?;
    Ok(())
}

async fn done_matches(path: &Path, quick: bool) -> bool {
    let Ok(bytes) = tokio::fs::read(path).await else {
        return false;
    };
    let Ok(done) = serde_json::from_slice::<Done>(&bytes) else {
        return false;
    };
    if done.files.is_empty() {
        return false;
    }
    for (file, size, sha1) in &done.files {
        let p = Path::new(file);
        match tokio::fs::metadata(p).await {
            Ok(m) if m.len() == *size => {}
            _ => return false,
        }
        if !quick && !sha1_file(p).await.is_ok_and(|h| h.eq_ignore_ascii_case(sha1)) {
            return false;
        }
    }
    true
}

async fn extract_data(installer: &Path, name: &str, work: &Path) -> Result<PathBuf> {
    let installer = installer.to_path_buf();
    let name = name.trim_start_matches('/').to_string();
    let out = crate::util::safe_join(work, &name)?;
    let o = out.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&installer)?)?;
        let mut entry = zip.by_name(&name)?;
        write_entry(&mut entry, &o)
    })
    .await
    .map_err(|e| Error::other(e.to_string()))??;
    Ok(out)
}

/// `[group:artifact:version]` becomes a library path; `{KEY}` anywhere becomes its value.
fn expand(arg: &str, vars: &HashMap<String, String>, libs: &Path) -> Result<String> {
    if let Some(coord) = arg.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        let rel = maven_path(coord).ok_or_else(|| Error::other(format!("Bad library name {coord}")))?;
        return Ok(libs.join(rel).display().to_string());
    }
    let mut out = String::with_capacity(arg.len());
    let mut rest = arg;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('}') {
            Some(end) => {
                let key = &after[..end];
                match vars.get(key) {
                    Some(v) => out.push_str(v),
                    None => out.push_str(&rest[start..start + end + 2]),
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
    Ok(out)
}

fn output_pairs(proc: &Processor, vars: &HashMap<String, String>, libs: &Path) -> Result<Vec<(PathBuf, String)>> {
    proc.outputs
        .iter()
        .map(|(k, v)| Ok((PathBuf::from(expand(k, vars, libs)?), expand(v, vars, libs)?.trim_matches('\'').to_string())))
        .collect()
}

async fn outputs_match(proc: &Processor, vars: &HashMap<String, String>, libs: &Path) -> Result<bool> {
    let pairs = output_pairs(proc, vars, libs)?;
    if pairs.is_empty() {
        return Ok(false);
    }
    for (path, sha1) in pairs {
        if !sha1_file(&path).await.is_ok_and(|h| h.eq_ignore_ascii_case(&sha1)) {
            return Ok(false);
        }
    }
    Ok(true)
}

async fn verify_outputs(proc: &Processor, vars: &HashMap<String, String>, libs: &Path) -> Result<()> {
    for (path, expected) in output_pairs(proc, vars, libs)? {
        let actual = sha1_file(&path).await.unwrap_or_default();
        if !actual.eq_ignore_ascii_case(&expected) {
            let _ = tokio::fs::remove_file(&path).await;
            return Err(Error::HashMismatch { path, expected, actual });
        }
    }
    Ok(())
}

/// Reads `Main-Class` from a jar's manifest.
fn main_class(jar: &Path) -> Result<String> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(jar)?)?;
    let text = read_text(&mut zip, "META-INF/MANIFEST.MF")?;
    // Manifest lines wrap at 72 bytes; a continuation starts with one space.
    let joined = text.replace("\r\n", "\n").replace("\n ", "");
    joined
        .lines()
        .find_map(|l| l.strip_prefix("Main-Class:").map(|v| v.trim().to_string()))
        .ok_or_else(|| Error::other(format!("{} has no Main-Class", jar.display())))
}

async fn run_one(proc: &Processor, vars: &HashMap<String, String>, libs: &Path, java: &Path, work: &Path, log: &mut String) -> Result<()> {
    let jar = libs.join(maven_path(&proc.jar).ok_or_else(|| Error::other(format!("Bad processor {}", proc.jar)))?);
    let j = jar.clone();
    let main = tokio::task::spawn_blocking(move || main_class(&j)).await.map_err(|e| Error::other(e.to_string()))??;
    let mut cp = vec![jar.display().to_string()];
    for c in &proc.classpath {
        cp.push(libs.join(maven_path(c).ok_or_else(|| Error::other(format!("Bad library name {c}")))?).display().to_string());
    }
    let args = proc.args.iter().map(|a| expand(a, vars, libs)).collect::<Result<Vec<_>>>()?;

    log.push_str(&format!("\n> {} {}\n", proc.jar, args.join(" ")));
    let mut cmd = tokio::process::Command::new(java);
    cmd.arg("-cp")
        .arg(cp.join(CP_SEP))
        .arg(&main)
        .args(&args)
        .current_dir(work)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let mut child = cmd.spawn()?;
    let (o, e, status) = tokio::join!(read_lines(child.stdout.take()), read_lines(child.stderr.take()), child.wait());
    let lines: Vec<String> = o.into_iter().chain(e).collect();
    for l in &lines {
        log.push_str(l);
        log.push('\n');
    }
    let status = status?;
    if !status.success() {
        let last: Vec<String> = lines.iter().rev().take(12).rev().cloned().collect();
        return Err(Error::other(format!(
            "A Forge install step failed ({}, exit {}):\n{}",
            proc.jar.split(':').nth(1).unwrap_or(&proc.jar),
            status.code().unwrap_or(-1),
            last.join("\n")
        )));
    }
    Ok(())
}

async fn read_lines(stream: Option<impl tokio::io::AsyncRead + Unpin>) -> Vec<String> {
    let mut out = vec![];
    if let Some(s) = stream {
        let mut lines = BufReader::new(s).lines();
        while let Ok(Some(l)) = lines.next_line().await {
            out.push(l);
        }
    }
    out
}

/// Forge's launch setup ignores a game jar named after the profile and loads the patched
/// one instead, so the vanilla jar goes on the classpath under that name, as the official
/// launcher does. Hard-linked where possible, copied otherwise.
pub async fn ensure_version_jar(launcher: &Launcher, profile_id: &str, vanilla_id: &str) -> Result<PathBuf> {
    let target = launcher.dirs.client_jar(profile_id);
    let source = launcher.dirs.client_jar(vanilla_id);
    let src_len = tokio::fs::metadata(&source).await?.len();
    if tokio::fs::metadata(&target).await.is_ok_and(|m| m.len() == src_len) {
        return Ok(target);
    }
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let _ = tokio::fs::remove_file(&target).await;
    if tokio::fs::hard_link(&source, &target).await.is_err() {
        tokio::fs::copy(&source, &target).await?;
    }
    Ok(target)
}

#[derive(Deserialize)]
struct Promotions {
    promos: HashMap<String, String>,
}

/// Forge's own pick for a game version: "recommended", else "latest".
pub async fn forge_promoted(launcher: &Launcher, game: &str) -> Option<String> {
    let p: Promotions =
        crate::http::get_json(&launcher.http, "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json")
            .await
            .ok()?;
    p.promos.get(&format!("{game}-recommended")).or_else(|| p.promos.get(&format!("{game}-latest"))).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_forge_maven_versions() {
        let xml = "<versions><version>1.20.1-47.4.28</version><version>1.7.10-10.13.4.1614-1.7.10</version>\
                   <version>1.20.1-47.4.2</version></versions>";
        assert_eq!(pick_forge_version(xml, "1.20.1", "47.4.28").as_deref(), Some("1.20.1-47.4.28"));
        assert_eq!(pick_forge_version(xml, "1.20.1", "47.4.2").as_deref(), Some("1.20.1-47.4.2"));
        assert_eq!(pick_forge_version(xml, "1.7.10", "10.13.4.1614").as_deref(), Some("1.7.10-10.13.4.1614-1.7.10"));
        assert_eq!(pick_forge_version(xml, "1.20.1", "1"), None);
    }

    #[test]
    fn expands_processor_arguments() {
        let libs = Path::new("/libs");
        let mut vars = HashMap::new();
        vars.insert("SIDE".to_string(), "client".to_string());
        vars.insert("MC_SLIM".to_string(), "/libs/slim.jar".to_string());
        assert_eq!(expand("{SIDE}", &vars, libs).unwrap(), "client");
        assert_eq!(expand("--out={MC_SLIM}", &vars, libs).unwrap(), "--out=/libs/slim.jar");
        assert_eq!(expand("{UNKNOWN}", &vars, libs).unwrap(), "{UNKNOWN}");
        assert_eq!(
            expand("[de.oceanlabs.mcp:mcp_config:1.20.1@zip]", &vars, libs).unwrap(),
            Path::new("/libs").join("de/oceanlabs/mcp/mcp_config/1.20.1/mcp_config-1.20.1.zip").display().to_string()
        );
    }

    #[test]
    fn legacy_profile_drops_server_only_libraries() {
        let info = serde_json::json!({
            "id": "1.7.10-Forge", "mainClass": "net.minecraft.launchwrapper.Launch", "inheritsFrom": "1.7.10",
            "libraries": [
                {"name": "net.minecraftforge:forge:1.7.10-10.13.4.1614-1.7.10", "url": "https://maven.minecraftforge.net/"},
                {"name": "net.minecraft:launchwrapper:1.12", "serverreq": true},
                {"name": "com.typesafe:config:1.2.1", "clientreq": true, "checksums": ["x"]},
                {"name": "server:only:1", "clientreq": false}
            ]
        });
        let v = legacy_version(info).unwrap();
        let names: Vec<_> = v.libraries.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(
            names,
            ["net.minecraftforge:forge:1.7.10-10.13.4.1614-1.7.10", "net.minecraft:launchwrapper:1.12", "com.typesafe:config:1.2.1"]
        );
    }

    #[test]
    fn neoforge_installer_urls() {
        let t = Target::new(Loader::NeoForge, "1.21.1", "21.1.265").unwrap();
        assert_eq!(t.cache_id(), "neoforge-21.1.265-1.21.1");
        assert!(Target::new(Loader::Fabric, "1.21.1", "0.16").is_err());
    }
}
