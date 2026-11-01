//! Exports an instance as a modpack: Modrinth's `.mrpack` or a CurseForge `.zip`.
//!
//! Mods, resource packs and shaders the platform knows are listed by reference (Modrinth by
//! SHA-1, CurseForge by fingerprint or by what Alloy installed), so the pack stays small and
//! authors get their downloads. Anything the platform doesn't know is bundled in
//! `overrides/`, and the report names those files: their licences decide whether the pack
//! may be shared.

use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::Digest;

use crate::content::scan::{self, display_name, Scanned};
use crate::content::{curseforge, modrinth, Source};
use crate::instance::Instance;
use crate::meta::loaders::Loader;
use crate::progress::{Progress, ProgressFn, Stage, Unit};
use crate::{Error, Launcher, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Format {
    Mrpack,
    CurseForge,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Options {
    pub format: Format,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    /// Top-level folders and files to bundle besides the mods (`config`, `options.txt`...).
    pub include: Vec<String>,
    pub dest: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub path: String,
    pub size: u64,
    /// Files listed by reference.
    pub linked: usize,
    /// Content the platform doesn't know, bundled as files: check their licences.
    pub bundled: Vec<String>,
}

/// A top-level folder or file that could go in the pack.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub path: String,
    pub size: u64,
    pub files: usize,
    /// Ticked unless it's personal (options, servers) or big and unusual (worlds).
    pub suggested: bool,
}

/// Content folders: their files are listed by reference where possible.
const CONTENT: &[&str] = &["mods", "resourcepacks", "shaderpacks"];
/// Never in a pack: Alloy's own files, the game's caches and logs, personal data.
const NEVER: &[&str] = &[
    ".launcher", "instance.json", "alloy.lock", "logs", "crash-reports", "screenshots", ".cache", "natives", ".fabric", ".quilt",
    "usercache.json", "usernamecache.json", "realms_persistence.json", "command_history.txt", "downloads", ".mixin.out", "hs_err",
];
/// Bundled by default.
const SUGGESTED: &[&str] = &["config", "defaultconfigs", "kubejs", "scripts", "global_packs", "openloader", "datapacks", "resources"];

/// What else the instance has that could be bundled, for the export dialog.
pub async fn candidates(launcher: &Launcher, inst: &Instance) -> Result<Vec<Candidate>> {
    let dir = inst.dir(launcher);
    tokio::task::spawn_blocking(move || {
        let mut out = vec![];
        let Ok(rd) = std::fs::read_dir(&dir) else { return out };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if NEVER.iter().any(|n| name == *n || name.starts_with(n)) || CONTENT.contains(&name.as_str()) {
                continue;
            }
            let (size, files) = usage(&e.path());
            if files == 0 {
                continue;
            }
            out.push(Candidate { suggested: SUGGESTED.contains(&name.as_str()), path: name, size, files });
        }
        out.sort_by(|a, b| b.suggested.cmp(&a.suggested).then(a.path.cmp(&b.path)));
        out
    })
    .await
    .map_err(|e| Error::other(e.to_string()))
}

fn usage(p: &Path) -> (u64, usize) {
    if p.is_file() {
        return (std::fs::metadata(p).map(|m| m.len()).unwrap_or(0), 1);
    }
    let mut total = (0, 0);
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            let (s, n) = usage(&e.path());
            total.0 += s;
            total.1 += n;
        }
    }
    total
}

/// One file going into the pack, by reference or bundled.
enum Entry {
    Modrinth { path: String, url: String, sha1: String, sha512: String, size: u64 },
    CurseForge { project: u64, file: u64, name: String },
    Bundled { path: String, name: Option<String> },
}

pub async fn export(launcher: &Launcher, inst: &Instance, opts: &Options, progress: &ProgressFn) -> Result<Report> {
    let step = |current: u64, message: &str| {
        progress(Progress { stage: Stage::Content, current, total: 4, unit: Unit::Steps, message: Some(message.into()), files: None })
    };
    step(0, "Looking at the files");
    let files: Vec<Scanned> = scan::scan(launcher, inst).await?.into_iter().filter(|f| f.enabled).collect();

    step(1, match opts.format {
        Format::Mrpack => "Finding them on Modrinth",
        Format::CurseForge => "Finding them on CurseForge",
    });
    let entries = match opts.format {
        Format::Mrpack => modrinth_entries(launcher, inst, &files).await?,
        Format::CurseForge => curseforge_entries(launcher, &files).await?,
    };

    step(2, "Writing the pack");
    let dir = inst.dir(launcher);
    let opts2 = opts.clone();
    let inst2 = inst.clone();
    let report = tokio::task::spawn_blocking(move || write_pack(&dir, &inst2, &opts2, &entries))
        .await
        .map_err(|e| Error::other(e.to_string()))??;
    step(4, "Done");
    Ok(report)
}

async fn modrinth_entries(launcher: &Launcher, inst: &Instance, files: &[Scanned]) -> Result<Vec<Entry>> {
    let sha1s: Vec<String> = files.iter().map(|f| f.sha1.clone()).collect();
    let found = modrinth::versions_from_hashes(launcher, &sha1s).await?;
    let dir = inst.dir(launcher);
    let mut out = vec![];
    for f in files {
        let hit = found.get(&f.sha1).and_then(|v| v.files.iter().find(|vf| vf.hashes.get("sha1").is_some_and(|h| h.eq_ignore_ascii_case(&f.sha1))));
        match hit {
            Some(vf) => {
                let sha512 = match vf.hashes.get("sha512") {
                    Some(h) => h.clone(),
                    None => sha512_file(&dir.join(&f.path))?,
                };
                out.push(Entry::Modrinth { path: f.path.clone(), url: vf.url.clone(), sha1: f.sha1.clone(), sha512, size: f.size });
            }
            None => out.push(Entry::Bundled { path: f.path.clone(), name: Some(display_name(f)) }),
        }
    }
    Ok(out)
}

async fn curseforge_entries(launcher: &Launcher, files: &[Scanned]) -> Result<Vec<Entry>> {
    let mut out = vec![];
    let mut ask: Vec<&Scanned> = vec![];
    for f in files {
        let known = f.index.as_ref().filter(|m| m.source == Source::CurseForge).and_then(|m| {
            let project = m.project_id.as_ref()?.parse::<u64>().ok()?;
            let file = m.version_id.as_ref()?.parse::<u64>().ok()?;
            Some((project, file))
        });
        match known {
            Some((project, file)) => out.push(Entry::CurseForge { project, file, name: display_name(f) }),
            None => ask.push(f),
        }
    }
    let prints: Vec<u32> = ask.iter().map(|f| f.murmur2).collect();
    let matched: HashMap<u32, curseforge::CfFile> = curseforge::match_fingerprints(launcher, &prints)
        .await?
        .into_iter()
        .filter_map(|cf| cf.file_fingerprint.map(|fp| (fp as u32, cf)))
        .collect();
    for f in ask {
        match matched.get(&f.murmur2) {
            Some(cf) => out.push(Entry::CurseForge { project: cf.mod_id, file: cf.id, name: display_name(f) }),
            None => out.push(Entry::Bundled { path: f.path.clone(), name: Some(display_name(f)) }),
        }
    }
    Ok(out)
}

fn sha512_file(path: &Path) -> Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = sha2::Sha512::new();
    std::io::copy(&mut f, &mut h)?;
    Ok(hex::encode(h.finalize()))
}

fn loader_dependency(loader: Loader) -> Option<&'static str> {
    match loader {
        Loader::Vanilla => None,
        Loader::Fabric => Some("fabric-loader"),
        Loader::Quilt => Some("quilt-loader"),
        Loader::Forge => Some("forge"),
        Loader::NeoForge => Some("neoforge"),
    }
}

fn write_pack(dir: &Path, inst: &Instance, opts: &Options, entries: &[Entry]) -> Result<Report> {
    if let Some(p) = opts.dest.parent() {
        std::fs::create_dir_all(p)?;
    }
    let tmp = opts.dest.with_extension("part");
    let file = std::fs::File::create(&tmp)?;
    let mut zip = zip::ZipWriter::new(std::io::BufWriter::new(file));
    let deflate = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let store = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

    let mut bundled = vec![];
    let mut linked = 0;
    // Overrides: the chosen folders, then content that couldn't be linked.
    let mut overrides: BTreeMap<String, PathBuf> = BTreeMap::new();
    for top in &opts.include {
        if NEVER.iter().any(|n| top == n) || top.contains("..") || CONTENT.contains(&top.as_str()) {
            continue;
        }
        collect(dir, &dir.join(top), &mut overrides);
    }
    for e in entries {
        match e {
            Entry::Bundled { path, name } => {
                overrides.insert(path.clone(), dir.join(path));
                bundled.push(name.clone().unwrap_or_else(|| path.clone()));
            }
            _ => linked += 1,
        }
    }

    match opts.format {
        Format::Mrpack => {
            let files: Vec<serde_json::Value> = entries
                .iter()
                .filter_map(|e| match e {
                    Entry::Modrinth { path, url, sha1, sha512, size } => Some(json!({
                        "path": path,
                        "hashes": { "sha1": sha1, "sha512": sha512 },
                        "downloads": [url],
                        "fileSize": size,
                    })),
                    _ => None,
                })
                .collect();
            let mut deps = serde_json::Map::new();
            deps.insert("minecraft".into(), json!(inst.game_version));
            if let (Some(k), Some(v)) = (loader_dependency(inst.loader), &inst.loader_version) {
                deps.insert(k.into(), json!(v));
            }
            let mut index = json!({
                "formatVersion": 1,
                "game": "minecraft",
                "versionId": opts.version,
                "name": opts.name,
                "files": files,
                "dependencies": deps,
            });
            if let Some(s) = opts.summary.as_ref().filter(|s| !s.trim().is_empty()) {
                index["summary"] = json!(s);
            }
            zip.start_file("modrinth.index.json", deflate).map_err(zerr)?;
            zip.write_all(serde_json::to_string_pretty(&index)?.as_bytes())?;
        }
        Format::CurseForge => {
            let files: Vec<serde_json::Value> = entries
                .iter()
                .filter_map(|e| match e {
                    Entry::CurseForge { project, file, .. } => Some(json!({ "projectID": project, "fileID": file, "required": true })),
                    _ => None,
                })
                .collect();
            let loaders: Vec<serde_json::Value> = match (inst.loader.modrinth_name(), &inst.loader_version) {
                (Some(l), Some(v)) => vec![json!({ "id": format!("{l}-{v}"), "primary": true })],
                _ => vec![],
            };
            let manifest = json!({
                "minecraft": { "version": inst.game_version, "modLoaders": loaders },
                "manifestType": "minecraftModpack",
                "manifestVersion": 1,
                "name": opts.name,
                "version": opts.version,
                "author": opts.author.clone().unwrap_or_default(),
                "files": files,
                "overrides": "overrides",
            });
            zip.start_file("manifest.json", deflate).map_err(zerr)?;
            zip.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())?;
            let mut html = String::from("<ul>\n");
            for e in entries {
                if let Entry::CurseForge { project, name, .. } = e {
                    html.push_str(&format!(
                        "<li><a href=\"https://www.curseforge.com/projects/{project}\">{}</a></li>\n",
                        name.replace('&', "&amp;").replace('<', "&lt;")
                    ));
                }
            }
            html.push_str("</ul>\n");
            zip.start_file("modlist.html", deflate).map_err(zerr)?;
            zip.write_all(html.as_bytes())?;
        }
    }

    for (rel, full) in &overrides {
        // Jars and zips are compressed already.
        let o = if rel.ends_with(".jar") || rel.ends_with(".zip") { store } else { deflate };
        zip.start_file(format!("overrides/{rel}"), o).map_err(zerr)?;
        let mut f = std::fs::File::open(full)?;
        std::io::copy(&mut f, &mut zip)?;
    }
    zip.finish().map_err(zerr)?.flush()?;
    std::fs::rename(&tmp, &opts.dest)?;
    let size = std::fs::metadata(&opts.dest)?.len();
    Ok(Report { path: opts.dest.to_string_lossy().into_owned(), size, linked, bundled })
}

fn zerr(e: zip::result::ZipError) -> Error {
    Error::other(format!("Couldn't write the pack: {e}"))
}

fn collect(root: &Path, p: &Path, out: &mut BTreeMap<String, PathBuf>) {
    if p.is_file() {
        if let Ok(rel) = p.strip_prefix(root) {
            out.insert(rel.to_string_lossy().replace('\\', "/"), p.to_path_buf());
        }
        return;
    }
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            collect(root, &e.path(), out);
        }
    }
}

/// A file name for the pack: "Name 1.0.mrpack".
pub fn file_name(opts_name: &str, version: &str, format: Format) -> String {
    let clean: String = format!("{opts_name} {version}").chars().filter(|c| !"/\\:*?\"<>|".contains(*c)).collect();
    match format {
        Format::Mrpack => format!("{}.mrpack", clean.trim()),
        Format::CurseForge => format!("{}.zip", clean.trim()),
    }
}
