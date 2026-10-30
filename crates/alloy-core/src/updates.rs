//! Signed, one-click updates from a local `.alloyupdate` file.
//!
//! An `.alloyupdate` is a zip made by the release workflow (scripts/pack_update.py):
//!
//! ```text
//! manifest.json        { product, version, platform, kind, file, sha256, notes, date }
//! manifest.json.sig    minisign signature of manifest.json
//! <installer>          Alloy_x.y.z_x64-setup.exe (Windows) or the AppImage (Linux)
//! <installer>.sig      minisign signature of the installer (made by the Tauri bundler)
//! ```
//!
//! Alloy accepts a file only if both signatures verify against the public key compiled in
//! below, the sha256 matches, and the platform is this one. The private key exists only in
//! the repository's secrets. Later the server serves the same files; nothing here changes.

use std::io::Read;
use std::path::{Path, PathBuf};

use minisign_verify::{PublicKey, Signature};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{Error, Result};

/// Public half of the updater key (`tauri signer` format: base64 of the minisign key file).
pub const PUBLIC_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDNFMEY5QTZFMDJFOEI2QzUKUldURnR1Z0NicG9QUHRBaGR6RjNPZWl1ajZodzV2VmUxem5yaVpWZnI1VlJUaW03Mlh6RlQzZlkK";

pub const EXTENSION: &str = "alloyupdate";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateManifest {
    pub product: String,
    pub version: String,
    /// e.g. "windows-x86_64", "linux-x86_64"
    pub platform: String,
    /// "nsis" (Windows installer) or "appimage"
    pub kind: String,
    /// Installer file name inside the archive.
    pub file: String,
    pub sha256: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub date: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundUpdate {
    pub path: PathBuf,
    pub manifest: UpdateManifest,
    pub current: String,
    pub newer: bool,
}

/// This build's platform id, matching `manifest.platform`.
pub fn platform() -> String {
    let os = match std::env::consts::OS {
        "macos" => "macos",
        other => other,
    };
    format!("{os}-{}", std::env::consts::ARCH)
}

fn verify_sig(public_key_b64: &str, data: &[u8], sig_file: &[u8], what: &str) -> Result<()> {
    use base64::Engine;
    let bad = |e: String| Error::other(format!("Update rejected: {what} signature is not valid ({e})"));
    let pk = PublicKey::from_base64(
        std::str::from_utf8(&base64::engine::general_purpose::STANDARD.decode(public_key_b64).map_err(|e| bad(e.to_string()))?)
            .map_err(|e| bad(e.to_string()))?
            .lines()
            .nth(1)
            .ok_or_else(|| bad("malformed public key".into()))?,
    )
    .map_err(|e| bad(e.to_string()))?;
    // Tauri .sig files hold the minisign signature text, base64-encoded once more.
    let text = base64::engine::general_purpose::STANDARD
        .decode(std::str::from_utf8(sig_file).map_err(|e| bad(e.to_string()))?.trim())
        .map_err(|e| bad(e.to_string()))?;
    let sig = Signature::decode(std::str::from_utf8(&text).map_err(|e| bad(e.to_string()))?).map_err(|e| bad(e.to_string()))?;
    pk.verify(data, &sig, true).map_err(|e| bad(e.to_string()))
}

fn read_entry(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Result<Vec<u8>> {
    let mut f = zip.by_name(name).map_err(|_| Error::other(format!("Update rejected: {name} is missing from the file")))?;
    let mut buf = Vec::with_capacity(f.size() as usize);
    f.read_to_end(&mut buf)?;
    Ok(buf)
}

fn compare(a: &str, b: &str) -> std::cmp::Ordering {
    match (semver::Version::parse(a), semver::Version::parse(b)) {
        (Ok(a), Ok(b)) => a.cmp(&b),
        _ => a.cmp(b),
    }
}

/// Fully verifies an `.alloyupdate` file and returns what it contains. Reads the installer
/// into memory (tens of MB) to check its hash and signature.
pub fn inspect(path: &Path, current_version: &str) -> Result<FoundUpdate> {
    inspect_with_key(path, current_version, PUBLIC_KEY)
}

pub fn inspect_with_key(path: &Path, current_version: &str, public_key: &str) -> Result<FoundUpdate> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path)?).map_err(|_| Error::other("That file is not an Alloy update"))?;
    let manifest_bytes = read_entry(&mut zip, "manifest.json")?;
    let manifest_sig = read_entry(&mut zip, "manifest.json.sig")?;
    verify_sig(public_key, &manifest_bytes, &manifest_sig, "manifest")?;
    let manifest: UpdateManifest = serde_json::from_slice(&manifest_bytes)?;

    if manifest.product != "alloy" {
        return Err(Error::other("That file is not an Alloy update"));
    }
    if manifest.platform != platform() {
        return Err(Error::other(format!("This update is for {}, but this is {}", manifest.platform, platform())));
    }
    let installer = read_entry(&mut zip, &manifest.file)?;
    let installer_sig = read_entry(&mut zip, &format!("{}.sig", manifest.file))?;
    let hash = hex::encode(Sha256::digest(&installer));
    if !hash.eq_ignore_ascii_case(&manifest.sha256) {
        return Err(Error::other("Update rejected: the installer does not match its manifest"));
    }
    verify_sig(public_key, &installer, &installer_sig, "installer")?;

    Ok(FoundUpdate {
        path: path.to_path_buf(),
        newer: compare(&manifest.version, current_version).is_gt(),
        current: current_version.to_string(),
        manifest,
    })
}

/// The newest valid update in `dirs` that is newer than `current_version`.
/// Invalid or foreign files are skipped (and logged), never surfaced as errors.
pub fn scan(dirs: &[PathBuf], current_version: &str) -> Option<FoundUpdate> {
    let mut best: Option<FoundUpdate> = None;
    for dir in dirs {
        let Ok(rd) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some(EXTENSION) {
                continue;
            }
            match inspect(&path, current_version) {
                Ok(found) if found.newer => {
                    if best.as_ref().map_or(true, |b| compare(&found.manifest.version, &b.manifest.version).is_gt()) {
                        best = Some(found);
                    }
                }
                Ok(_) => {}
                Err(e) => tracing::warn!("ignoring {}: {e}", path.display()),
            }
        }
    }
    best
}

/// Extracts the verified installer into `staging/<version>/` and checks it again on disk.
pub fn stage(found: &FoundUpdate, staging: &Path) -> Result<PathBuf> {
    let dir = staging.join(&found.manifest.version);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&found.path)?)?;
    let bytes = read_entry(&mut zip, &found.manifest.file)?;
    let out = dir.join(&found.manifest.file);
    std::fs::write(&out, &bytes)?;
    let check = hex::encode(Sha256::digest(std::fs::read(&out)?));
    if !check.eq_ignore_ascii_case(&found.manifest.sha256) {
        return Err(Error::other("Update rejected: the extracted installer is damaged"));
    }
    Ok(out)
}

/// Written just before handing over to the installer; read on the next start.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pending {
    pub from: String,
    pub to: String,
    pub notes: String,
    pub started_at: i64,
    pub backup: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateOutcome {
    pub ok: bool,
    pub from: String,
    pub to: String,
    pub notes: String,
}

pub fn write_pending(updates_dir: &Path, p: &Pending) -> Result<()> {
    std::fs::create_dir_all(updates_dir)?;
    std::fs::write(updates_dir.join("pending.json"), serde_json::to_vec_pretty(p)?)?;
    Ok(())
}

/// On start: did the last update land? Clears the marker once answered.
pub fn take_outcome(updates_dir: &Path, current_version: &str) -> Option<UpdateOutcome> {
    let path = updates_dir.join("pending.json");
    let p: Pending = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
    let ok = compare(current_version, &p.to).is_ge();
    let age = chrono::Utc::now().timestamp() - p.started_at;
    if ok || age > 600 {
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(updates_dir.join("staging"));
        return Some(UpdateOutcome { ok, from: p.from, to: p.to, notes: p.notes });
    }
    // The installer may still be running; ask again next start.
    None
}

/// Copies Alloy's own records (not game files) to `backups/<stamp>-<version>/` before an
/// update, keeping the newest five.
pub fn backup(data_root: &Path, version: &str) -> Result<PathBuf> {
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let root = data_root.join("backups");
    let dest = root.join(format!("{stamp}-before-{version}"));
    std::fs::create_dir_all(&dest)?;
    for name in ["settings.json", "accounts.json"] {
        let src = data_root.join(name);
        if src.exists() {
            std::fs::copy(&src, dest.join(name))?;
        }
    }
    if let Ok(rd) = std::fs::read_dir(data_root.join("instances")) {
        for e in rd.flatten() {
            let id = e.file_name();
            for rel in ["instance.json", ".launcher/content.json"] {
                let src = e.path().join(rel);
                if src.exists() {
                    let to = dest.join("instances").join(&id).join(rel);
                    std::fs::create_dir_all(to.parent().unwrap())?;
                    std::fs::copy(&src, &to)?;
                }
            }
        }
    }
    let mut all: Vec<_> = std::fs::read_dir(&root)?.flatten().map(|e| e.path()).collect();
    all.sort();
    while all.len() > 5 {
        let _ = std::fs::remove_dir_all(all.remove(0));
    }
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_semantically() {
        assert!(compare("0.10.0", "0.9.0").is_gt());
        assert!(compare("0.2.0", "0.2.0").is_eq());
        assert!(compare("1.0.0-beta.2", "1.0.0").is_lt());
    }

    #[test]
    fn public_key_parses() {
        use base64::Engine;
        let text = String::from_utf8(base64::engine::general_purpose::STANDARD.decode(PUBLIC_KEY).unwrap()).unwrap();
        assert!(PublicKey::from_base64(text.lines().nth(1).unwrap()).is_ok());
    }

    #[test]
    fn rejects_garbage() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.alloyupdate");
        std::fs::write(&p, b"not a zip").unwrap();
        assert!(inspect(&p, "0.1.0").is_err());
        assert!(scan(&[dir.path().to_path_buf()], "0.1.0").is_none());
    }

    #[test]
    fn outcome_reports_success_once() {
        let dir = tempfile::tempdir().unwrap();
        write_pending(
            dir.path(),
            &Pending {
                from: "0.1.0".into(),
                to: "0.2.0".into(),
                notes: String::new(),
                started_at: chrono::Utc::now().timestamp(),
                backup: PathBuf::new(),
            },
        )
        .unwrap();
        assert!(take_outcome(dir.path(), "0.1.0").is_none(), "installer still running");
        let o = take_outcome(dir.path(), "0.2.0").unwrap();
        assert!(o.ok);
        assert!(take_outcome(dir.path(), "0.2.0").is_none());
    }
}
