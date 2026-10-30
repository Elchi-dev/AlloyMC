//! Files a modpack names but couldn't ship. Modrinth packs may only bundle files hosted on
//! Modrinth, so some list the CurseForge-only files they need in
//! `config/missing_mods_checker.json` (the Missing Mods Checker mod's format), which then
//! asks the player to download each by hand on every start.
//!
//! Alloy reads that list and gets the files itself: straight from CurseForge's API where the
//! author allows it, otherwise through the blocked-download flow (Alloy watches Downloads).

use std::path::Path;

use serde::Deserialize;

use super::manage::BlockedFile;
use super::{curseforge, load_index, save_index, ContentMeta, Source};
use crate::download::{download_with_retry, DownloadTask};
use crate::instance::Instance;
use crate::{Launcher, Result};

pub const LIST_FILE: &str = "config/missing_mods_checker.json";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    display_name: String,
    /// The exact file name the pack expects.
    pattern: String,
    url: String,
    #[serde(default = "mods")]
    destination: String,
}

fn mods() -> String {
    "mods".into()
}

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListedOutcome {
    pub installed: Vec<String>,
    pub blocked: Vec<BlockedFile>,
}

/// CurseForge file ids from `…/download/5010620` or `…/files/5010620`.
fn file_id(url: &str) -> Option<u64> {
    let mut parts = url.trim_end_matches('/').rsplit('/');
    let last = parts.next()?;
    let before = parts.next()?;
    matches!(before, "download" | "files").then(|| last.parse().ok()).flatten()
}

/// Entries that are still missing from the instance.
async fn missing(launcher: &Launcher, instance: &Instance) -> Vec<Entry> {
    let dir = instance.dir(launcher);
    let Ok(text) = tokio::fs::read_to_string(dir.join(LIST_FILE)).await else { return vec![] };
    let Ok(entries) = serde_json::from_str::<Vec<Entry>>(&text) else { return vec![] };
    entries
        .into_iter()
        .filter(|e| {
            let Ok(folder) = crate::util::safe_join(&dir, &e.destination) else { return false };
            !folder.join(&e.pattern).exists() && !folder.join(format!("{}.disabled", e.pattern)).exists()
        })
        .collect()
}

pub async fn has_missing(launcher: &Launcher, instance: &Instance) -> bool {
    !missing(launcher, instance).await.is_empty()
}

/// Gets everything the pack's list says is missing. Files saved under the exact name the
/// list expects, so the pack's own checker is satisfied too.
pub async fn fetch(launcher: &Launcher, instance: &Instance) -> Result<ListedOutcome> {
    let entries = missing(launcher, instance).await;
    let mut out = ListedOutcome::default();
    if entries.is_empty() {
        return Ok(out);
    }
    let dir = instance.dir(launcher);
    let ids: Vec<u64> = entries.iter().filter_map(|e| file_id(&e.url)).collect();
    let files = if curseforge::has_key(launcher).await { curseforge::files(launcher, &ids).await.unwrap_or_default() } else { vec![] };
    let mut index = load_index(launcher, instance).await;
    for e in entries {
        let fid = file_id(&e.url);
        let f = fid.and_then(|id| files.iter().find(|f| f.id == id));
        let sha1 = f.and_then(|f| f.hashes.iter().find(|h| h.algo == 1).map(|h| h.value.clone()));
        let folder = crate::util::safe_join(&dir, &e.destination)?;
        match f.and_then(|f| f.download_url.clone()) {
            Some(url) => {
                let dest = folder.join(&e.pattern);
                download_with_retry(&launcher.http, &DownloadTask::new(url, &dest).sha1(sha1.clone())).await?;
                let rel = format!("{}/{}", e.destination.trim_end_matches('/'), e.pattern);
                index.files.insert(
                    rel,
                    ContentMeta {
                        source: Source::CurseForge,
                        project_id: f.map(|f| f.mod_id.to_string()),
                        version_id: fid.map(|i| i.to_string()),
                        title: Some(e.display_name.clone()),
                        version_name: f.map(|f| f.display_name.clone()),
                        icon_url: None,
                        sha1,
                        auto: false,
                        requires: f.map(|f| f.required_mods()),
                    },
                );
                out.installed.push(e.display_name);
            }
            None => out.blocked.push(BlockedFile {
                title: e.display_name.clone(),
                file_name: e.pattern.clone(),
                page_url: e.url.clone(),
                sha1,
                size: f.map(|f| f.file_length).unwrap_or(0),
                mod_id: f.map(|f| f.mod_id).unwrap_or(0),
                file_id: fid.unwrap_or(0),
                folder: e.destination.trim_end_matches('/').to_string(),
                icon_url: None,
            }),
        }
    }
    save_index(launcher, instance, &index).await?;
    super::blocked::add(launcher, instance, &out.blocked).await?;
    Ok(out)
}

pub fn list_path(dir: &Path) -> std::path::PathBuf {
    dir.join(LIST_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_ids_from_links() {
        assert_eq!(file_id("https://www.curseforge.com/minecraft/mc-mods/flib/download/8070669"), Some(8070669));
        assert_eq!(file_id("https://www.curseforge.com/minecraft/mc-mods/flib/files/8070669/"), Some(8070669));
        assert_eq!(file_id("https://www.curseforge.com/minecraft/mc-mods/flib"), None);
    }
}
