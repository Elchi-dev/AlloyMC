//! Looks at what's really in an instance: every mod, resource pack and shader on disk, with
//! its hashes and (for mods) the jar's own metadata. Results are cached per file by size and
//! modification time, so a 300-mod pack rescans in milliseconds.
//!
//! Every scan also refreshes `alloy.lock` in the instance folder: the exact files, their
//! hashes and where they came from, which makes an instance reproducible.

use std::collections::BTreeMap;
use std::path::Path;

use futures::{StreamExt, TryStreamExt};
use serde::{Deserialize, Serialize};
use sha1::Digest;

use super::jarmeta::{self, JarMeta};
use super::{load_index, ContentKind, ContentMeta, Source};
use crate::instance::Instance;
use crate::settings::write_json_atomic;
use crate::{Launcher, Result};

pub const LOCK_FILE: &str = "alloy.lock";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scanned {
    /// Relative to the instance, without `.disabled`.
    pub path: String,
    pub file_name: String,
    pub kind: ContentKind,
    pub enabled: bool,
    pub size: u64,
    pub sha1: String,
    /// CurseForge's fingerprint (MurmurHash2 of the file without whitespace bytes).
    pub murmur2: u32,
    /// What the jar says about itself (mods only).
    pub jar: Option<JarMeta>,
    /// Where Alloy installed it from, if it did.
    pub index: Option<ContentMeta>,
}

/// Bump when what's cached per file changes, so old caches are rebuilt.
const CACHE_VERSION: u32 = 3;

#[derive(Debug, Default, Serialize, Deserialize)]
struct Cache {
    #[serde(default)]
    version: u32,
    files: BTreeMap<String, CacheEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntry {
    size: u64,
    modified: i64,
    sha1: String,
    murmur2: u32,
    jar: Option<JarMeta>,
}

fn cache_path(launcher: &Launcher, instance: &Instance) -> std::path::PathBuf {
    instance.dir(launcher).join(".launcher").join("scan.json")
}

const KINDS: [ContentKind; 3] = [ContentKind::Mod, ContentKind::ResourcePack, ContentKind::Shader];

/// Every content file in the instance, enabled or not, sorted by kind then name.
pub async fn scan(launcher: &Launcher, instance: &Instance) -> Result<Vec<Scanned>> {
    let dir = instance.dir(launcher);
    let index = load_index(launcher, instance).await;
    let cache_file = cache_path(launcher, instance);
    let cache: Cache = tokio::fs::read(&cache_file)
        .await
        .ok()
        .and_then(|b| serde_json::from_slice::<Cache>(&b).ok())
        .filter(|c| c.version == CACHE_VERSION)
        .unwrap_or_default();

    // What's on disk now.
    let mut found = vec![];
    for kind in KINDS {
        let Ok(mut rd) = tokio::fs::read_dir(dir.join(kind.folder())).await else {
            continue;
        };
        while let Some(e) = rd.next_entry().await? {
            let Ok(meta) = e.metadata().await else {
                continue;
            };
            if !meta.is_file() {
                continue;
            }
            let raw = e.file_name().to_string_lossy().to_string();
            let enabled = !raw.ends_with(".disabled");
            let file_name = raw.trim_end_matches(".disabled").to_string();
            let is_content = match kind {
                ContentKind::Mod => file_name.ends_with(".jar") || file_name.ends_with(".litemod"),
                _ => file_name.ends_with(".zip") || file_name.ends_with(".jar"),
            };
            if !is_content {
                continue;
            }
            let modified =
                meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as i64).unwrap_or(0);
            found.push((kind, file_name, enabled, e.path(), meta.len(), modified));
        }
    }

    let cache = &cache;
    let index = &index;
    let mut scanned: Vec<(Scanned, CacheEntry)> =
        futures::stream::iter(found.into_iter().map(|(kind, file_name, enabled, full, size, modified)| async move {
            let path = format!("{}/{file_name}", kind.folder());
            let cached = cache.files.get(&path).filter(|c| c.size == size && c.modified == modified).cloned();
            let entry = match cached {
                Some(c) => c,
                None => {
                    let is_mod = kind == ContentKind::Mod;
                    tokio::task::spawn_blocking(move || -> Result<CacheEntry> {
                        let (sha1, murmur2) = hashes(&full)?;
                        let jar = if is_mod { jarmeta::read_path(&full).ok() } else { None };
                        Ok(CacheEntry { size, modified, sha1, murmur2, jar })
                    })
                    .await
                    .map_err(|e| crate::Error::other(e.to_string()))??
                }
            };
            let s = Scanned {
                index: index.files.get(&path).cloned(),
                path,
                file_name,
                kind,
                enabled,
                size,
                sha1: entry.sha1.clone(),
                murmur2: entry.murmur2,
                jar: entry.jar.clone(),
            };
            Ok::<_, crate::Error>((s, entry))
        }))
        .buffer_unordered(8)
        .try_collect()
        .await?;

    scanned.sort_by(|(a, _), (b, _)| {
        (a.kind as u8).cmp(&(b.kind as u8)).then_with(|| display_name(a).to_lowercase().cmp(&display_name(b).to_lowercase()))
    });

    // Save the cache only when it changed.
    let new_cache = Cache { version: CACHE_VERSION, files: scanned.iter().map(|(s, e)| (s.path.clone(), e.clone())).collect() };
    let out: Vec<Scanned> = scanned.into_iter().map(|(s, _)| s).collect();
    if cache.version != CACHE_VERSION || serde_json::to_vec(&new_cache.files).ok() != serde_json::to_vec(&cache.files).ok() {
        let _ = write_json_atomic(&cache_file, &new_cache).await;
    }
    let _ = write_lock(launcher, instance, &out).await;
    Ok(out)
}

pub fn display_name(s: &Scanned) -> String {
    s.index
        .as_ref()
        .and_then(|m| m.title.clone())
        .or_else(|| s.jar.as_ref().and_then(|j| j.primary()).and_then(|m| m.name.clone()))
        .unwrap_or_else(|| s.file_name.trim_end_matches(".jar").trim_end_matches(".zip").to_string())
}

/// SHA-1 and CurseForge's MurmurHash2 fingerprint in one read.
fn hashes(path: &Path) -> Result<(String, u32)> {
    let bytes = std::fs::read(path)?;
    let sha1 = hex::encode(sha1::Sha1::digest(&bytes));
    Ok((sha1, cf_fingerprint(&bytes)))
}

/// CurseForge fingerprint: MurmurHash2 (seed 1) over the file with bytes 9, 10, 13 and 32 removed.
pub fn cf_fingerprint(bytes: &[u8]) -> u32 {
    let filtered: Vec<u8> = bytes.iter().copied().filter(|b| !matches!(b, 9 | 10 | 13 | 32)).collect();
    murmur2(&filtered, 1)
}

fn murmur2(data: &[u8], seed: u32) -> u32 {
    const M: u32 = 0x5bd1_e995;
    let len = data.len();
    let mut h: u32 = seed ^ (len as u32);
    let mut chunks = data.chunks_exact(4);
    for c in &mut chunks {
        let mut k = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
        k = k.wrapping_mul(M);
        k ^= k >> 24;
        k = k.wrapping_mul(M);
        h = h.wrapping_mul(M);
        h ^= k;
    }
    let rest = chunks.remainder();
    if rest.len() >= 3 {
        h ^= (rest[2] as u32) << 16;
    }
    if rest.len() >= 2 {
        h ^= (rest[1] as u32) << 8;
    }
    if !rest.is_empty() {
        h ^= rest[0] as u32;
        h = h.wrapping_mul(M);
    }
    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^= h >> 15;
    h
}

// ---------- alloy.lock ----------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Lock {
    pub lock_version: u32,
    pub minecraft: String,
    pub loader: crate::meta::loaders::Loader,
    pub loader_version: Option<String>,
    pub files: Vec<LockedFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LockedFile {
    pub path: String,
    pub enabled: bool,
    pub size: u64,
    pub sha1: String,
    pub source: Source,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mod_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

async fn write_lock(launcher: &Launcher, instance: &Instance, files: &[Scanned]) -> Result<()> {
    let lock = Lock {
        lock_version: 1,
        minecraft: instance.game_version.clone(),
        loader: instance.loader,
        loader_version: instance.loader_version.clone(),
        files: files
            .iter()
            .map(|s| {
                let primary = s.jar.as_ref().and_then(|j| j.primary());
                LockedFile {
                    path: s.path.clone(),
                    enabled: s.enabled,
                    size: s.size,
                    sha1: s.sha1.clone(),
                    source: s.index.as_ref().map(|m| m.source).unwrap_or(Source::Local),
                    project_id: s.index.as_ref().and_then(|m| m.project_id.clone()),
                    version_id: s.index.as_ref().and_then(|m| m.version_id.clone()),
                    mod_id: primary.map(|m| m.id.clone()),
                    version: primary.map(|m| m.version.clone()).filter(|v| !v.is_empty()),
                }
            })
            .collect(),
    };
    let path = instance.dir(launcher).join(LOCK_FILE);
    let current: Option<Lock> = tokio::fs::read(&path).await.ok().and_then(|b| serde_json::from_slice(&b).ok());
    if current.as_ref() != Some(&lock) {
        // write_json_atomic picks a temp name from the extension; the lock has none it knows.
        let tmp = path.with_extension("lock.tmp");
        tokio::fs::write(&tmp, serde_json::to_vec_pretty(&lock)?).await?;
        tokio::fs::rename(&tmp, &path).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curseforge_fingerprint_matches_known_values() {
        // Empty input, worked by hand: h = 1, then the final mix gives 0x5bd15e36.
        assert_eq!(murmur2(b"", 1), 0x5bd1_5e36);
        assert_eq!(cf_fingerprint(b"a b\tc\n"), murmur2(b"abc", 1));
    }
}
