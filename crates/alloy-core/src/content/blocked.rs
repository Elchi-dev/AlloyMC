//! CurseForge files whose authors block launcher downloads. Alloy keeps a list per instance,
//! the user downloads each from its page, and Alloy picks the file up from the Downloads
//! folder by itself: matched by name (browsers add " (1)" to duplicates) and verified by hash.

use std::path::{Path, PathBuf};

use super::manage::{accept_blocked, BlockedFile};
use crate::instance::Instance;
use crate::settings::write_json_atomic;
use crate::{Launcher, Result};

fn list_path(launcher: &Launcher, instance: &Instance) -> PathBuf {
    instance.dir(launcher).join(".launcher").join("blocked.json")
}

pub async fn pending(launcher: &Launcher, instance: &Instance) -> Vec<BlockedFile> {
    tokio::fs::read(list_path(launcher, instance)).await.ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

async fn save(launcher: &Launcher, instance: &Instance, list: &[BlockedFile]) -> Result<()> {
    let path = list_path(launcher, instance);
    if list.is_empty() {
        let _ = tokio::fs::remove_file(&path).await;
        return Ok(());
    }
    write_json_atomic(&path, &list).await
}

pub async fn add(launcher: &Launcher, instance: &Instance, files: &[BlockedFile]) -> Result<()> {
    if files.is_empty() {
        return Ok(());
    }
    let mut list = pending(launcher, instance).await;
    for f in files {
        if !list.iter().any(|x| x.file_id == f.file_id) {
            list.push(f.clone());
        }
    }
    save(launcher, instance, &list).await
}

pub async fn dismiss(launcher: &Launcher, instance: &Instance, file_id: u64) -> Result<()> {
    let mut list = pending(launcher, instance).await;
    list.retain(|f| f.file_id != file_id);
    save(launcher, instance, &list).await
}

/// `name.jar`, `name (1).jar`, `name(2).jar` all count as the same download.
fn matches_name(candidate: &str, wanted: &str) -> bool {
    if candidate == wanted {
        return true;
    }
    let (stem, ext) = wanted.rsplit_once('.').unwrap_or((wanted, ""));
    let Some(rest) = candidate.strip_prefix(stem) else { return false };
    let rest = rest.strip_suffix(&format!(".{ext}")).unwrap_or(rest);
    let rest = rest.trim();
    rest.starts_with('(') && rest.ends_with(')') && rest[1..rest.len() - 1].chars().all(|c| c.is_ascii_digit())
}

/// Looks for pending files in `dirs` and installs the ones it finds. Returns their titles.
pub async fn collect(launcher: &Launcher, instance: &Instance, dirs: &[PathBuf]) -> Result<Vec<String>> {
    let mut list = pending(launcher, instance).await;
    if list.is_empty() {
        return Ok(vec![]);
    }
    let mut found = vec![];
    for dir in dirs {
        let Ok(mut rd) = tokio::fs::read_dir(dir).await else { continue };
        let mut entries = vec![];
        while let Ok(Some(e)) = rd.next_entry().await {
            entries.push(e.path());
        }
        let mut i = 0;
        while i < list.len() {
            let b = &list[i];
            let hit = entries.iter().find(|p: &&PathBuf| {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                matches_name(name, &b.file_name) && size_ok(p, b.size)
            });
            if let Some(path) = hit {
                match accept_blocked(launcher, instance, b, path).await {
                    Ok(()) => {
                        found.push(b.title.clone());
                        list.remove(i);
                        continue;
                    }
                    Err(e) => tracing::warn!("{} in Downloads isn't the expected file: {e}", b.file_name),
                }
            }
            i += 1;
        }
    }
    save(launcher, instance, &list).await?;
    Ok(found)
}

fn size_ok(path: &Path, size: u64) -> bool {
    size == 0 || std::fs::metadata(path).is_ok_and(|m| m.len() == size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_renamed_downloads_match() {
        assert!(matches_name("jei-1.20.1.jar", "jei-1.20.1.jar"));
        assert!(matches_name("jei-1.20.1 (1).jar", "jei-1.20.1.jar"));
        assert!(matches_name("jei-1.20.1(2).jar", "jei-1.20.1.jar"));
        assert!(!matches_name("jei-1.20.1-extra.jar", "jei-1.20.1.jar"));
        assert!(!matches_name("other.jar", "jei-1.20.1.jar"));
    }
}
