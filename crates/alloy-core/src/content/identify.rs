//! Names files Alloy didn't install itself (dropped in by hand, or from a modpack): asks
//! Modrinth by SHA-1 and CurseForge by fingerprint which project each belongs to, and
//! records the answer, so every file gets its real name, icon, source and updates.
//!
//! A file neither platform knows is remembered as "checked" (its hash stored with a Local
//! source) and not asked about again until it changes.

use std::collections::{HashMap, HashSet};

use super::scan::Scanned;
use super::{curseforge, load_index, modrinth, save_index, ContentMeta, Source};
use crate::instance::Instance;
use crate::{Launcher, Result};

/// Returns how many files were newly identified. Network failures leave files as they were.
pub async fn identify(launcher: &Launcher, instance: &Instance, files: &[Scanned]) -> Result<usize> {
    let mut index = load_index(launcher, instance).await;
    let needs_lookup = |s: &Scanned| match index.files.get(&s.path) {
        None => true,
        // Replaced by hand since we last looked.
        Some(m) if m.sha1.as_deref().is_some_and(|h| !h.eq_ignore_ascii_case(&s.sha1)) => true,
        Some(_) => false,
    };
    let unknown: Vec<&Scanned> = files.iter().filter(|s| needs_lookup(s)).collect();
    // Known project, but no title yet (modpack imports record ids only).
    let untitled: Vec<&Scanned> =
        files.iter().filter(|s| index.files.get(&s.path).is_some_and(|m| m.title.is_none() && m.project_id.is_some())).collect();
    // Known Modrinth version, but its dependencies were never recorded.
    let no_deps: Vec<(String, String)> = files
        .iter()
        .filter_map(|s| {
            let m = index.files.get(&s.path)?;
            (m.source == Source::Modrinth && m.requires.is_none()).then(|| Some((s.path.clone(), m.version_id.clone()?)))?
        })
        .collect();
    if unknown.is_empty() && untitled.is_empty() && no_deps.is_empty() {
        return Ok(0);
    }
    if !no_deps.is_empty() {
        let ids: Vec<String> = no_deps.iter().map(|(_, v)| v.clone()).collect();
        if let Ok(versions) = modrinth::versions(launcher, &ids).await {
            let by_id: HashMap<&str, &modrinth::Version> = versions.iter().map(|v| (v.id.as_str(), v)).collect();
            for (path, vid) in &no_deps {
                if let (Some(v), Some(m)) = (by_id.get(vid.as_str()), index.files.get_mut(path)) {
                    m.requires = Some(v.required_projects());
                }
            }
        }
    }

    let mut found = 0;
    let mut by_path: HashMap<String, ContentMeta> = HashMap::new();

    // Modrinth, by SHA-1.
    let sha1s: Vec<String> = unknown.iter().map(|s| s.sha1.clone()).collect();
    if let Ok(versions) = modrinth::versions_from_hashes(launcher, &sha1s).await {
        for s in &unknown {
            if let Some(v) = versions.get(&s.sha1) {
                by_path.insert(
                    s.path.clone(),
                    ContentMeta {
                        source: Source::Modrinth,
                        project_id: Some(v.project_id.clone()),
                        version_id: Some(v.id.clone()),
                        title: None,
                        version_name: Some(v.version_number.clone()),
                        icon_url: None,
                        sha1: Some(s.sha1.clone()),
                        auto: index.files.get(&s.path).is_some_and(|m| m.auto),
                        requires: Some(v.required_projects()),
                    },
                );
            }
        }
    }

    // CurseForge, by fingerprint, for what Modrinth didn't know.
    if curseforge::has_key(launcher).await {
        let rest: Vec<&&Scanned> = unknown.iter().filter(|s| !by_path.contains_key(&s.path)).collect();
        let prints: Vec<u32> = rest.iter().map(|s| s.murmur2).collect();
        if let Ok(matches) = curseforge::match_fingerprints(launcher, &prints).await {
            let by_print: HashMap<u32, &curseforge::CfFile> =
                matches.iter().filter_map(|f| f.file_fingerprint.map(|fp| (fp as u32, f))).collect();
            let mod_ids: Vec<u64> = matches.iter().map(|f| f.mod_id).collect::<HashSet<_>>().into_iter().collect();
            let mods: HashMap<u64, curseforge::CfMod> =
                curseforge::mods(launcher, &mod_ids).await.unwrap_or_default().into_iter().map(|m| (m.id, m)).collect();
            for s in rest {
                let Some(f) = by_print.get(&s.murmur2) else {
                    continue;
                };
                let m = mods.get(&f.mod_id);
                by_path.insert(
                    s.path.clone(),
                    ContentMeta {
                        source: Source::CurseForge,
                        project_id: Some(f.mod_id.to_string()),
                        version_id: Some(f.id.to_string()),
                        title: m.map(|m| m.name.clone()),
                        version_name: Some(f.display_name.clone()),
                        icon_url: m.and_then(|m| m.logo.as_ref().map(|l| l.thumbnail_url.clone().unwrap_or(l.url.clone()))),
                        sha1: Some(s.sha1.clone()),
                        auto: index.files.get(&s.path).is_some_and(|m| m.auto),
                        requires: Some(f.required_mods()),
                    },
                );
            }
        }
    }

    // Titles and icons for everything Modrinth identified, plus older untitled entries.
    let mut need_titles: Vec<(String, String)> = by_path
        .iter()
        .filter(|(_, m)| m.source == Source::Modrinth)
        .filter_map(|(p, m)| m.project_id.clone().map(|id| (p.clone(), id)))
        .collect();
    for s in &untitled {
        if let Some(m) = index.files.get(&s.path) {
            if m.source == Source::Modrinth {
                if let Some(id) = &m.project_id {
                    need_titles.push((s.path.clone(), id.clone()));
                }
            }
        }
    }
    let ids: Vec<String> = need_titles.iter().map(|(_, id)| id.clone()).collect::<HashSet<_>>().into_iter().collect();
    if let Ok(projects) = modrinth::projects(launcher, &ids).await {
        let by_id: HashMap<&str, &modrinth::Project> = projects.iter().map(|p| (p.id.as_str(), p)).collect();
        for (path, id) in need_titles {
            let Some(p) = by_id.get(id.as_str()) else {
                continue;
            };
            let entry = by_path.entry(path.clone()).or_insert_with(|| index.files.get(&path).cloned().unwrap_or_else(ContentMeta::local));
            entry.title = Some(p.title.clone());
            entry.icon_url = p.icon_url.clone();
        }
    }

    for s in unknown {
        match by_path.remove(&s.path) {
            Some(meta) => {
                found += 1;
                index.files.insert(s.path.clone(), meta);
            }
            None => {
                // Checked: neither platform has it.
                let mut m = index.files.get(&s.path).cloned().unwrap_or_else(ContentMeta::local);
                m.source = Source::Local;
                m.sha1 = Some(s.sha1.clone());
                index.files.insert(s.path.clone(), m);
            }
        }
    }
    for (path, meta) in by_path {
        index.files.insert(path, meta);
    }
    save_index(launcher, instance, &index).await?;
    Ok(found)
}
