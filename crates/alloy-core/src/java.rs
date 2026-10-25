//! Java runtimes: Mojang's managed runtimes (exactly what the official launcher uses),
//! plus discovery of system installs as a fallback.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::download::{download_all, DownloadTask};
use crate::progress::{ProgressFn, Stage};
use crate::{Error, Launcher, Result};

const RUNTIME_INDEX: &str =
    "https://piston-meta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";

#[derive(Debug, Clone, Serialize)]
pub struct JavaInstall {
    pub path: PathBuf,
    pub major: Option<u32>,
    pub version: Option<String>,
    pub managed: bool,
}

type RuntimeIndex = HashMap<String, HashMap<String, Vec<RuntimeEntry>>>;

#[derive(Deserialize)]
struct RuntimeEntry {
    manifest: ManifestRef,
    version: RuntimeVersion,
}
#[derive(Deserialize)]
struct ManifestRef {
    sha1: String,
    url: String,
}
#[derive(Deserialize)]
struct RuntimeVersion {
    name: String,
}
#[derive(Deserialize)]
struct RuntimeManifest {
    files: HashMap<String, RuntimeFile>,
}
#[derive(Deserialize)]
struct RuntimeFile {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    executable: bool,
    #[serde(default)]
    downloads: Option<RuntimeDownloads>,
    #[serde(default)]
    target: Option<String>,
}
#[derive(Deserialize)]
struct RuntimeDownloads {
    raw: RawDownload,
}
#[derive(Deserialize)]
struct RawDownload {
    sha1: String,
    size: u64,
    url: String,
}

/// Mojang's platform key for this machine, if it ships runtimes for it.
fn platform_key() -> Option<&'static str> {
    Some(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "windows-x64",
        ("windows", "x86") => "windows-x86",
        ("windows", "aarch64") => "windows-arm64",
        ("macos", "x86_64") => "mac-os",
        ("macos", "aarch64") => "mac-os-arm64",
        ("linux", "x86_64") => "linux",
        ("linux", "x86") => "linux-i386",
        _ => return None,
    })
}

fn java_binary(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        let bundle = home.join("jre.bundle/Contents/Home/bin/java");
        if bundle.exists() {
            return bundle;
        }
        home.join("bin/java")
    } else if cfg!(target_os = "windows") {
        // javaw has no console window.
        home.join("bin").join("javaw.exe")
    } else {
        home.join("bin/java")
    }
}

/// Returns a Java binary for `component`/`major`, installing Mojang's runtime if needed.
pub async fn ensure(launcher: &Launcher, component: &str, major: u32, progress: &ProgressFn) -> Result<PathBuf> {
    let home = launcher.dirs.java().join(component);
    let marker = home.join(".installed");
    if marker.exists() {
        return Ok(java_binary(&home));
    }

    match install_mojang(launcher, component, &home, progress).await {
        Ok(path) => {
            tokio::fs::write(&marker, component).await?;
            Ok(path)
        }
        Err(e) => {
            tracing::warn!("Mojang runtime {component} unavailable: {e}; looking for a system Java {major}");
            // TODO(phase 1): Adoptium fallback for platforms Mojang doesn't cover (Linux arm64).
            detect_system()
                .await
                .into_iter()
                .find(|j| j.major == Some(major))
                .map(|j| j.path)
                .ok_or_else(|| {
                    Error::NotFound(format!(
                        "Java {major}. Install it or set a Java path in the instance settings"
                    ))
                })
        }
    }
}

async fn install_mojang(launcher: &Launcher, component: &str, home: &Path, progress: &ProgressFn) -> Result<PathBuf> {
    let platform = platform_key().ok_or_else(|| Error::Unsupported("no Mojang Java for this platform".into()))?;
    let index: RuntimeIndex = crate::http::get_json(&launcher.http, RUNTIME_INDEX).await?;
    let entry = index
        .get(platform)
        .and_then(|p| p.get(component))
        .and_then(|list| list.first())
        .ok_or_else(|| Error::NotFound(format!("Java runtime {component} for {platform}")))?;
    tracing::info!("installing Java {} ({component})", entry.version.name);

    let bytes = crate::download::fetch_bytes(&launcher.http, &entry.manifest.url).await?;
    let actual = {
        use sha1::Digest;
        hex::encode(sha1::Sha1::digest(&bytes))
    };
    if !actual.eq_ignore_ascii_case(&entry.manifest.sha1) {
        return Err(Error::other("Java runtime manifest checksum mismatch"));
    }
    let manifest: RuntimeManifest = serde_json::from_slice(&bytes)?;

    let mut tasks = vec![];
    let mut executables = vec![];
    let mut links = vec![];
    for (rel, file) in &manifest.files {
        let dest = crate::util::safe_join(home, rel)?;
        match file.kind.as_str() {
            "directory" => tokio::fs::create_dir_all(&dest).await?,
            "file" => {
                if let Some(dl) = &file.downloads {
                    tasks.push(
                        DownloadTask::new(&dl.raw.url, &dest)
                            .sha1(Some(dl.raw.sha1.clone()))
                            .size(Some(dl.raw.size)),
                    );
                    if file.executable {
                        executables.push(dest);
                    }
                }
            }
            "link" => {
                if let Some(target) = &file.target {
                    links.push((dest, target.clone()));
                }
            }
            _ => {}
        }
    }
    let concurrency = launcher.settings.read().await.max_concurrent_downloads;
    download_all(&launcher.http, tasks, concurrency, Stage::Java, progress).await?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for path in executables {
            tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).await?;
        }
        for (link, target) in links {
            let _ = tokio::fs::remove_file(&link).await;
            if let Some(parent) = link.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::symlink(&target, &link).await?;
        }
    }
    #[cfg(not(unix))]
    let _ = (executables, links);

    Ok(java_binary(home))
}

/// Managed runtimes plus JAVA_HOME and `java` on PATH.
pub async fn detect_all(launcher: &Launcher) -> Vec<JavaInstall> {
    let mut out = vec![];
    if let Ok(mut rd) = tokio::fs::read_dir(launcher.dirs.java()).await {
        while let Ok(Some(entry)) = rd.next_entry().await {
            let bin = java_binary(&entry.path());
            if bin.exists() {
                let (major, version) = probe(&bin).await.unwrap_or((None, None));
                out.push(JavaInstall {
                    path: bin,
                    major,
                    version,
                    managed: true,
                });
            }
        }
    }
    out.extend(detect_system().await);
    out
}

async fn detect_system() -> Vec<JavaInstall> {
    let mut candidates: Vec<PathBuf> = vec![];
    if let Ok(home) = std::env::var("JAVA_HOME") {
        candidates.push(java_binary(Path::new(&home)));
    }
    candidates.push(PathBuf::from(if cfg!(windows) { "java.exe" } else { "java" }));

    let mut out = vec![];
    for path in candidates {
        if let Some((major, version)) = probe(&path).await {
            out.push(JavaInstall {
                path,
                major,
                version,
                managed: false,
            });
        }
    }
    out
}

/// Runs `java -version` and parses the major version from stderr.
pub async fn probe(bin: &Path) -> Option<(Option<u32>, Option<String>)> {
    // javaw prints nothing; probe java.exe next to it instead.
    let bin = if bin.file_name().map(|n| n == "javaw.exe").unwrap_or(false) {
        bin.with_file_name("java.exe")
    } else {
        bin.to_path_buf()
    };
    let out = tokio::process::Command::new(&bin).arg("-version").output().await.ok()?;
    let text = String::from_utf8_lossy(&out.stderr);
    let version = text.split('"').nth(1).map(str::to_string);
    let major = version.as_deref().and_then(parse_major);
    Some((major, version))
}

/// "1.8.0_402" -> 8, "21.0.5" -> 21, "25" -> 25
pub fn parse_major(v: &str) -> Option<u32> {
    let mut parts = v.split(['.', '_', '-', '+']);
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 {
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn majors() {
        assert_eq!(super::parse_major("1.8.0_402"), Some(8));
        assert_eq!(super::parse_major("21.0.5"), Some(21));
        assert_eq!(super::parse_major("25"), Some(25));
    }
}
