//! Turning what people paste or drop into something to install: Modrinth and CurseForge
//! page links, direct modpack links, and local files of every kind.

use std::io::Read;
use std::path::Path;

use serde::Serialize;

use super::ContentKind;
use crate::{Error, Result};

/// What a pasted link points at.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum Link {
    /// A Modrinth project page, optionally a specific version.
    Modrinth { slug: String, version: Option<String> },
    /// A CurseForge project page, optionally a specific file.
    CurseForge { slug: String, section: String, file_id: Option<u64> },
    /// Any other https link: treated as a modpack archive to download.
    Archive { url: String },
}

pub fn parse_link(input: &str) -> Result<Link> {
    let input = input.trim();
    let url = url::Url::parse(input)
        .or_else(|_| url::Url::parse(&format!("https://{input}")))
        .map_err(|_| Error::other("That doesn't look like a link"))?;
    if !matches!(url.scheme(), "https" | "http") {
        return Err(Error::other("Only web links can be installed"));
    }
    let host = url.host_str().unwrap_or("").trim_start_matches("www.").to_lowercase();
    let segs: Vec<&str> = url.path_segments().map(|s| s.filter(|x| !x.is_empty()).collect()).unwrap_or_default();
    match host.as_str() {
        "modrinth.com" => {
            // /mod/<slug>, /modpack/<slug>/version/<v>, /project/<slug>
            if segs.len() >= 2 && matches!(segs[0], "mod" | "modpack" | "resourcepack" | "shader" | "datapack" | "plugin" | "project") {
                let version = match segs.get(2) {
                    Some(&"version") => segs.get(3).map(|s| s.to_string()),
                    _ => None,
                };
                return Ok(Link::Modrinth { slug: segs[1].to_string(), version });
            }
            Err(Error::other("That Modrinth link isn't a project page"))
        }
        "curseforge.com" | "legacy.curseforge.com" => {
            // /minecraft/<section>/<slug>[/files/<id>|/download/<id>]
            if segs.len() >= 3 && segs[0] == "minecraft" {
                let file_id = match segs.get(3) {
                    Some(&"files") | Some(&"download") => segs.get(4).and_then(|s| s.parse().ok()),
                    _ => None,
                };
                return Ok(Link::CurseForge { section: segs[1].to_string(), slug: segs[2].to_string(), file_id });
            }
            Err(Error::other("That CurseForge link isn't a Minecraft project page"))
        }
        _ => Ok(Link::Archive { url: url.to_string() }),
    }
}

impl Link {
    /// The content kind a CurseForge section name stands for.
    pub fn curseforge_kind(section: &str) -> Option<ContentKind> {
        Some(match section {
            "mc-mods" => ContentKind::Mod,
            "modpacks" => ContentKind::Modpack,
            "texture-packs" => ContentKind::ResourcePack,
            "shaders" => ContentKind::Shader,
            "data-packs" => ContentKind::DataPack,
            _ => return None,
        })
    }
}

/// What a dropped file is.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DroppedKind {
    Modpack,
    Mod,
    ResourcePack,
    Shader,
    DataPack,
    /// A world save (a zip with level.dat).
    World,
    Unknown,
}

/// Looks inside the file rather than trusting its extension.
pub fn classify(path: &Path) -> DroppedKind {
    let Ok(file) = std::fs::File::open(path) else { return DroppedKind::Unknown };
    let Ok(mut zip) = zip::ZipArchive::new(file) else { return DroppedKind::Unknown };
    let names: Vec<String> = (0..zip.len().min(20_000)).filter_map(|i| zip.by_index_raw(i).ok().map(|e| e.name().to_string())).collect();
    let has = |n: &str| names.iter().any(|x| x == n);
    let any_prefix = |p: &str| names.iter().any(|x| x.starts_with(p));
    if has("modrinth.index.json") {
        return DroppedKind::Modpack;
    }
    if has("manifest.json") {
        // CurseForge packs have a manifest with "minecraft" and "files".
        if let Ok(mut f) = zip.by_name("manifest.json") {
            let mut s = String::new();
            if f.read_to_string(&mut s).is_ok() && s.contains("\"minecraft\"") && s.contains("\"files\"") {
                return DroppedKind::Modpack;
            }
        }
    }
    if has("fabric.mod.json")
        || has("quilt.mod.json")
        || has("META-INF/mods.toml")
        || has("META-INF/neoforge.mods.toml")
        || has("mcmod.info")
    {
        return DroppedKind::Mod;
    }
    if names.iter().any(|n| n.ends_with("level.dat")) {
        return DroppedKind::World;
    }
    if any_prefix("shaders/") || names.iter().any(|n| n.contains("/shaders/") && n.ends_with(".fsh")) {
        return DroppedKind::Shader;
    }
    if has("pack.mcmeta") {
        return if any_prefix("data/") && !any_prefix("assets/") { DroppedKind::DataPack } else { DroppedKind::ResourcePack };
    }
    if names.iter().any(|n| n.ends_with(".class")) {
        // A jar without metadata: very old mods and some libraries.
        return DroppedKind::Mod;
    }
    DroppedKind::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modrinth_links() {
        assert_eq!(parse_link("https://modrinth.com/mod/sodium").unwrap(), Link::Modrinth { slug: "sodium".into(), version: None });
        assert_eq!(
            parse_link("modrinth.com/modpack/fabulously-optimized/version/v6.4.0").unwrap(),
            Link::Modrinth { slug: "fabulously-optimized".into(), version: Some("v6.4.0".into()) }
        );
        assert!(parse_link("https://modrinth.com/discover/mods").is_err());
    }

    #[test]
    fn curseforge_links() {
        assert_eq!(
            parse_link("https://www.curseforge.com/minecraft/mc-mods/jei/files/9298210").unwrap(),
            Link::CurseForge { slug: "jei".into(), section: "mc-mods".into(), file_id: Some(9298210) }
        );
        assert_eq!(Link::curseforge_kind("modpacks"), Some(ContentKind::Modpack));
    }

    #[test]
    fn other_links_are_archives() {
        assert!(matches!(parse_link("https://example.com/pack.mrpack").unwrap(), Link::Archive { .. }));
        assert!(parse_link("ftp://x").is_err());
        assert!(parse_link("not a link at all").is_err());
    }
}
