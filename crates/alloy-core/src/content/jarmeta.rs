//! What a mod jar says about itself. Platform metadata is often wrong or missing (local
//! files have none at all), so the jar's own description is the source of truth:
//!
//! | File | Loader |
//! | --- | --- |
//! | `fabric.mod.json` | Fabric (and Quilt, which loads Fabric mods) |
//! | `quilt.mod.json` | Quilt |
//! | `META-INF/neoforge.mods.toml` | NeoForge 1.20.5+ |
//! | `META-INF/mods.toml` | Forge, and NeoForge up to 1.20.4 |
//! | `mcmod.info` | Forge 1.12 and older |
//!
//! Jars bundled inside a jar (Fabric's `META-INF/jars/`, Forge's `META-INF/jarjar/`) are read
//! too, so a mod that ships its own copy of a library counts as providing it.

use std::io::{Cursor, Read, Seek};

use serde::{Deserialize, Serialize};

use super::version_range::Range;

/// Which metadata formats a jar carries.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Fabric,
    Quilt,
    Forge,
    NeoForge,
    LegacyForge,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DepKind {
    /// Must be present, in range.
    Required,
    /// Nice to have; never an error.
    Optional,
    /// Must not be present in range (Fabric `breaks`, NeoForge `incompatible`).
    Breaks,
    /// Works badly together (Fabric `conflicts`, NeoForge `discouraged`).
    Conflicts,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dep {
    pub id: String,
    pub kind: DepKind,
    /// The range as written, for display.
    pub range: String,
    /// Which syntax `range` is in.
    pub maven: bool,
    /// Forge `side = "SERVER"` dependencies don't matter for a client.
    #[serde(default)]
    pub server_only: bool,
}

impl Dep {
    pub fn parsed(&self) -> Range {
        if self.maven {
            Range::maven(&self.range)
        } else {
            Range::fabric_any_of(self.range.split(" || "))
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModEntry {
    pub id: String,
    pub version: String,
    pub name: Option<String>,
    pub description: Option<String>,
    /// Other ids this mod answers to (Fabric `provides`).
    #[serde(default)]
    pub provides: Vec<String>,
    #[serde(default)]
    pub deps: Vec<Dep>,
    /// Path of the icon inside the jar.
    pub icon: Option<String>,
    /// `client`, `server` or `*` (Fabric `environment`).
    pub environment: Option<String>,
    /// Which metadata file this entry came from. Multi-loader jars carry one per loader,
    /// and only the one the instance's loader reads counts.
    #[serde(default)]
    pub format: Option<Format>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JarMeta {
    pub formats: Vec<Format>,
    /// Mods the jar declares (Forge jars may declare several).
    pub mods: Vec<ModEntry>,
    /// Mods inside bundled jars, flattened. They provide ids but aren't separate installs.
    pub nested: Vec<ModEntry>,
    /// The jar is a Sinytra Connector-style bridge that loads Fabric mods on (Neo)Forge.
    #[serde(default)]
    pub loads_fabric_mods: bool,
}

impl JarMeta {
    /// Every id this jar makes available, with its version.
    pub fn provided(&self) -> impl Iterator<Item = (&str, &str)> {
        self.mods.iter().chain(self.nested.iter()).flat_map(|m| {
            std::iter::once((m.id.as_str(), m.version.as_str())).chain(m.provides.iter().map(move |p| (p.as_str(), m.version.as_str())))
        })
    }

    pub fn primary(&self) -> Option<&ModEntry> {
        self.mods.first()
    }

    /// The entries the given loader reads, best first.
    pub fn entries_for(&self, wanted: &[Format]) -> Vec<&ModEntry> {
        for f in wanted {
            let hits: Vec<&ModEntry> = self.mods.iter().filter(|m| m.format == Some(*f)).collect();
            if !hits.is_empty() {
                return hits;
            }
        }
        vec![]
    }
}

const MAX_NESTED_DEPTH: u8 = 3;
/// Bundled jars bigger than this aren't opened (they're libraries, not mods with metadata).
const MAX_NESTED_BYTES: u64 = 32 * 1024 * 1024;

/// Reads a jar from disk. Never fails on bad metadata: an unreadable jar is just "no metadata".
pub fn read_path(path: &std::path::Path) -> std::io::Result<JarMeta> {
    let file = std::fs::File::open(path)?;
    Ok(read(file, 0).unwrap_or_default())
}

pub fn read<R: Read + Seek>(reader: R, depth: u8) -> Option<JarMeta> {
    let mut zip = zip::ZipArchive::new(reader).ok()?;
    let mut meta = JarMeta::default();
    let manifest = text(&mut zip, "META-INF/MANIFEST.MF");
    let manifest_version = manifest.as_deref().and_then(|m| manifest_value(m, "Implementation-Version"));

    let mut nested_paths: Vec<String> = vec![];
    // FML's other convention: the manifest names an embedded mod jar (Sinytra Connector does this).
    if let Some(m) = manifest.as_deref() {
        for key in ["Embedded-Dependencies-Mod", "Embedded-Dependencies-Library"] {
            if let Some(v) = manifest_value(m, key) {
                nested_paths.extend(v.split([',', ' ']).filter(|x| x.ends_with(".jar")).map(str::to_string));
            }
        }
    }

    if let Some(json) = text(&mut zip, "fabric.mod.json") {
        if let Some((entry, jars)) = parse_fabric(&json) {
            meta.formats.push(Format::Fabric);
            nested_paths.extend(jars);
            meta.mods.push(entry);
        }
    }
    if let Some(json) = text(&mut zip, "quilt.mod.json") {
        if let Some((entry, jars)) = parse_quilt(&json) {
            meta.formats.push(Format::Quilt);
            nested_paths.extend(jars);
            meta.mods.push(entry);
        }
    }
    for (file, format) in [("META-INF/neoforge.mods.toml", Format::NeoForge), ("META-INF/mods.toml", Format::Forge)] {
        if let Some(toml) = text(&mut zip, file) {
            if let Some(entries) = parse_mods_toml(&toml, manifest_version.as_deref(), format == Format::NeoForge) {
                meta.formats.push(format);
                meta.mods.extend(entries);
            }
        }
    }
    if meta.mods.is_empty() {
        if let Some(json) = text(&mut zip, "mcmod.info") {
            if let Some(entries) = parse_mcmod_info(&json) {
                meta.formats.push(Format::LegacyForge);
                meta.mods.extend(entries);
            }
        }
    }
    if let Some(json) = text(&mut zip, "META-INF/jarjar/metadata.json") {
        nested_paths.extend(parse_jarjar(&json));
    }
    nested_paths.sort();
    nested_paths.dedup();

    if depth < MAX_NESTED_DEPTH {
        for path in nested_paths {
            let Ok(mut entry) = zip.by_name(&path) else {
                continue;
            };
            if entry.size() > MAX_NESTED_BYTES {
                continue;
            }
            let mut buf = Vec::with_capacity(entry.size() as usize);
            if entry.read_to_end(&mut buf).is_err() {
                continue;
            }
            drop(entry);
            if let Some(inner) = read(Cursor::new(buf), depth + 1) {
                meta.nested.extend(inner.mods);
                meta.nested.extend(inner.nested);
            }
        }
    }
    // Sinytra Connector loads Fabric mods on Forge and NeoForge.
    meta.loads_fabric_mods = meta.mods.iter().chain(meta.nested.iter()).any(|m| matches!(m.id.as_str(), "connectormod" | "connector"));
    Some(meta)
}

/// Reads one small text entry; skips anything implausibly large.
fn text<R: Read + Seek>(zip: &mut zip::ZipArchive<R>, name: &str) -> Option<String> {
    let mut entry = zip.by_name(name).ok()?;
    if entry.size() > 4 * 1024 * 1024 {
        return None;
    }
    let mut bytes = vec![];
    entry.read_to_end(&mut bytes).ok()?;
    // Strip a UTF-8 byte order mark; some jars have one.
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
    Some(String::from_utf8_lossy(bytes).into_owned())
}

fn manifest_value(manifest: &str, key: &str) -> Option<String> {
    let joined = manifest.replace("\r\n", "\n").replace("\n ", "");
    joined.lines().find_map(|l| l.strip_prefix(key).and_then(|r| r.strip_prefix(':')).map(|v| v.trim().to_string()))
}

/// Lenient JSON: some jars put raw newlines or tabs inside strings, which serde rejects.
fn lenient_json(s: &str) -> Option<serde_json::Value> {
    serde_json::from_str(s).ok().or_else(|| {
        let mut out = String::with_capacity(s.len());
        let mut in_str = false;
        let mut escaped = false;
        for c in s.chars() {
            if in_str {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    in_str = false;
                } else if c == '\n' {
                    out.push_str("\\n");
                    continue;
                } else if c == '\r' {
                    continue;
                } else if c == '\t' {
                    out.push_str("\\t");
                    continue;
                }
            } else if c == '"' {
                in_str = true;
            }
            out.push(c);
        }
        serde_json::from_str(&out).ok()
    })
}

fn str_of(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(str::to_string)
}

/// Fabric: `depends`, `recommends`, `suggests`, `breaks`, `conflicts` map ids to a range
/// string or an array of them.
fn parse_fabric(json: &str) -> Option<(ModEntry, Vec<String>)> {
    let v = lenient_json(json)?;
    let id = str_of(&v, "id")?;
    let mut deps = vec![];
    for (key, kind) in
        [("depends", DepKind::Required), ("recommends", DepKind::Optional), ("breaks", DepKind::Breaks), ("conflicts", DepKind::Conflicts)]
    {
        let Some(map) = v.get(key).and_then(|m| m.as_object()) else {
            continue;
        };
        for (dep_id, range) in map {
            let range = match range {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Array(a) => a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(" || "),
                _ => "*".into(),
            };
            deps.push(Dep { id: dep_id.clone(), kind, range, maven: false, server_only: false });
        }
    }
    let icon = match v.get("icon") {
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        // A size → path map; take the largest.
        Some(serde_json::Value::Object(m)) => {
            m.iter().max_by_key(|(k, _)| k.parse::<u32>().unwrap_or(0)).and_then(|(_, p)| p.as_str().map(str::to_string))
        }
        _ => None,
    };
    let provides = v
        .get("provides")
        .and_then(|p| p.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let jars = v.get("jars").and_then(|j| j.as_array()).map(|a| a.iter().filter_map(|j| str_of(j, "file")).collect()).unwrap_or_default();
    Some((
        ModEntry {
            id,
            version: str_of(&v, "version").unwrap_or_default(),
            name: str_of(&v, "name"),
            description: str_of(&v, "description"),
            provides,
            deps,
            icon,
            environment: str_of(&v, "environment"),
            format: Some(Format::Fabric),
        },
        jars,
    ))
}

/// Quilt: `quilt_loader.depends` is a list of ids or `{ id, versions, optional }` objects.
fn parse_quilt(json: &str) -> Option<(ModEntry, Vec<String>)> {
    let v = lenient_json(json)?;
    let ql = v.get("quilt_loader")?;
    let id = str_of(ql, "id")?;
    let meta = ql.get("metadata");
    let mut deps = vec![];
    for (key, kind) in [("depends", DepKind::Required), ("breaks", DepKind::Breaks)] {
        let Some(list) = ql.get(key).and_then(|d| d.as_array()) else {
            continue;
        };
        for d in list {
            let (dep_id, range, optional) = match d {
                serde_json::Value::String(s) => (s.clone(), "*".to_string(), false),
                serde_json::Value::Object(_) => {
                    let Some(dep_id) = str_of(d, "id") else {
                        continue;
                    };
                    let range = match d.get("versions") {
                        Some(serde_json::Value::String(s)) => s.clone(),
                        Some(serde_json::Value::Array(a)) => a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(" || "),
                        _ => "*".into(),
                    };
                    (dep_id, range, d.get("optional").and_then(|o| o.as_bool()).unwrap_or(false))
                }
                _ => continue,
            };
            // Quilt ids may be namespaced (`org.quiltmc:qsl`); the part after ':' is the mod id.
            let dep_id = dep_id.rsplit(':').next().unwrap_or(&dep_id).to_string();
            deps.push(Dep {
                id: dep_id,
                kind: if optional && kind == DepKind::Required { DepKind::Optional } else { kind },
                range,
                maven: false,
                server_only: false,
            });
        }
    }
    let provides = ql
        .get("provides")
        .and_then(|p| p.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string).or_else(|| str_of(x, "id"))).collect())
        .unwrap_or_default();
    let jars = ql
        .get("jars")
        .and_then(|j| j.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    Some((
        ModEntry {
            id,
            version: str_of(ql, "version").unwrap_or_default(),
            name: meta.and_then(|m| str_of(m, "name")),
            description: meta.and_then(|m| str_of(m, "description")),
            provides,
            deps,
            icon: meta.and_then(|m| str_of(m, "icon")),
            environment: None,
            format: Some(Format::Quilt),
        },
        jars,
    ))
}

/// Forge and NeoForge `mods.toml`: `[[mods]]` tables and `[[dependencies.<modId>]]` lists.
fn parse_mods_toml(src: &str, manifest_version: Option<&str>, neoforge: bool) -> Option<Vec<ModEntry>> {
    let doc: toml::Table = src.parse().ok().or_else(|| {
        // Some jars use a key twice or other small sins; try again without comment lines.
        let cleaned: String = src.lines().filter(|l| !l.trim_start().starts_with('#')).collect::<Vec<_>>().join("\n");
        cleaned.parse().ok()
    })?;
    let mods = doc.get("mods")?.as_array()?;
    let deps_table = doc.get("dependencies").and_then(|d| d.as_table());
    let top_logo = doc.get("logoFile").and_then(|l| l.as_str()).map(str::to_string);
    let mut out = vec![];
    for m in mods {
        let Some(id) = m.get("modId").and_then(|i| i.as_str()) else {
            continue;
        };
        let mut version = m.get("version").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if version.contains("${file.jarVersion}") {
            version = manifest_version.unwrap_or("").to_string();
        }
        let mut deps = vec![];
        if let Some(list) = deps_table.and_then(|d| d.get(id)).and_then(|l| l.as_array()) {
            for d in list {
                let Some(dep_id) = d.get("modId").and_then(|i| i.as_str()) else {
                    continue;
                };
                // Forge: `mandatory = true|false`. NeoForge: `type = "required"|"optional"|"incompatible"|"discouraged"`.
                let kind = match d.get("type").and_then(|t| t.as_str()).map(str::to_lowercase).as_deref() {
                    Some("required") => DepKind::Required,
                    Some("optional") => DepKind::Optional,
                    Some("incompatible") => DepKind::Breaks,
                    Some("discouraged") => DepKind::Conflicts,
                    _ => match d.get("mandatory").and_then(|m| m.as_bool()) {
                        Some(true) => DepKind::Required,
                        Some(false) => DepKind::Optional,
                        None if neoforge => DepKind::Required,
                        None => DepKind::Optional,
                    },
                };
                let side = d.get("side").and_then(|s| s.as_str()).unwrap_or("BOTH");
                deps.push(Dep {
                    id: dep_id.to_string(),
                    kind,
                    range: d.get("versionRange").and_then(|r| r.as_str()).unwrap_or("*").to_string(),
                    maven: true,
                    server_only: side.eq_ignore_ascii_case("SERVER"),
                });
            }
        }
        out.push(ModEntry {
            id: id.to_string(),
            version,
            name: m.get("displayName").and_then(|n| n.as_str()).map(str::to_string),
            description: m.get("description").and_then(|n| n.as_str()).map(|s| s.trim().to_string()),
            provides: m
                .get("provides")
                .and_then(|p| p.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
                .unwrap_or_default(),
            deps,
            icon: m.get("logoFile").and_then(|l| l.as_str()).map(str::to_string).or(top_logo.clone()),
            environment: None,
            format: Some(if neoforge { Format::NeoForge } else { Format::Forge }),
        });
    }
    (!out.is_empty()).then_some(out)
}

/// Legacy `mcmod.info`: a list of mods, or `{ "modList": [...] }`.
fn parse_mcmod_info(json: &str) -> Option<Vec<ModEntry>> {
    let v = lenient_json(json)?;
    let list = match &v {
        serde_json::Value::Array(a) => a.clone(),
        serde_json::Value::Object(o) => o.get("modList").and_then(|l| l.as_array()).cloned()?,
        _ => return None,
    };
    let out: Vec<ModEntry> = list
        .iter()
        .filter_map(|m| {
            let id = str_of(m, "modid")?;
            let deps = m
                .get("requiredMods")
                .and_then(|r| r.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str())
                        // "Forge" and "FML" entries mean the loader itself.
                        .filter(|d| !matches!(d.to_lowercase().as_str(), "forge" | "fml" | "mod_minecraftforge"))
                        .map(|d| {
                            let (id, range) = d.split_once('@').unwrap_or((d, "*"));
                            Dep { id: id.to_string(), kind: DepKind::Required, range: range.to_string(), maven: true, server_only: false }
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(ModEntry {
                id,
                version: str_of(m, "version").unwrap_or_default(),
                name: str_of(m, "name"),
                description: str_of(m, "description"),
                provides: vec![],
                deps,
                icon: str_of(m, "logoFile"),
                environment: None,
                format: Some(Format::LegacyForge),
            })
        })
        .collect();
    (!out.is_empty()).then_some(out)
}

fn parse_jarjar(json: &str) -> Vec<String> {
    lenient_json(json)
        .and_then(|v| v.get("jars").and_then(|j| j.as_array()).cloned())
        .map(|a| a.iter().filter_map(|j| str_of(j, "path")).collect())
        .unwrap_or_default()
}

/// Reads one file out of a jar (an icon), at most `max` bytes.
pub fn read_entry(path: &std::path::Path, name: &str, max: u64) -> Option<Vec<u8>> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).ok()?).ok()?;
    let mut entry = zip.by_name(name.trim_start_matches('/')).ok()?;
    if entry.size() > max {
        return None;
    }
    let mut buf = vec![];
    entry.read_to_end(&mut buf).ok()?;
    Some(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn jar(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Cursor::new(vec![]);
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            for (name, data) in files {
                z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
                z.write_all(data).unwrap();
            }
            z.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn reads_fabric_with_nested_jars() {
        let inner = jar(&[("fabric.mod.json", br#"{"id":"fabric-api-base","version":"0.4.42"}"#)]);
        let outer = jar(&[
            (
                "fabric.mod.json",
                br#"{"schemaVersion":1,"id":"fabric-api","version":"0.92.2+1.20.1","name":"Fabric API",
                    "provides":["fabric"],"icon":{"16":"a.png","128":"b.png"},
                    "depends":{"fabricloader":">=0.14.22","minecraft":["1.20.1","1.20.2"]},
                    "breaks":{"optifabric":"<1.13"},
                    "jars":[{"file":"META-INF/jars/base.jar"}]}"#,
            ),
            ("META-INF/jars/base.jar", &inner),
        ]);
        let m = read(Cursor::new(outer), 0).unwrap();
        assert_eq!(m.formats, vec![Format::Fabric]);
        let e = m.primary().unwrap();
        assert_eq!(e.icon.as_deref(), Some("b.png"));
        let mc = e.deps.iter().find(|d| d.id == "minecraft").unwrap();
        assert!(mc.parsed().matches("1.20.2"));
        assert!(!mc.parsed().matches("1.20.4"));
        assert!(e.deps.iter().any(|d| d.id == "optifabric" && d.kind == DepKind::Breaks));
        let provided: Vec<_> = m.provided().map(|(id, _)| id).collect();
        assert_eq!(provided, ["fabric-api", "fabric", "fabric-api-base"]);
    }

    #[test]
    fn reads_forge_mods_toml_with_manifest_version() {
        let toml = br#"
modLoader="javafml"
loaderVersion="[47,)"
[[mods]]
modId="create"
version="${file.jarVersion}"
displayName="Create"
[[dependencies.create]]
    modId="forge"
    mandatory=true
    versionRange="[47.1.3,)"
    side="BOTH"
[[dependencies.create]]
    modId="flywheel"
    mandatory=true
    versionRange="[0.6.10,0.6.11)"
[[dependencies.create]]
    modId="jei"
    mandatory=false
    versionRange="*"
"#;
        let j =
            jar(&[("META-INF/mods.toml", toml), ("META-INF/MANIFEST.MF", b"Manifest-Version: 1.0\r\nImplementation-Version: 0.5.1.f\r\n")]);
        let m = read(Cursor::new(j), 0).unwrap();
        let e = m.primary().unwrap();
        assert_eq!((e.id.as_str(), e.version.as_str()), ("create", "0.5.1.f"));
        let fly = e.deps.iter().find(|d| d.id == "flywheel").unwrap();
        assert_eq!(fly.kind, DepKind::Required);
        assert!(!fly.parsed().matches("0.6.11"));
        assert_eq!(e.deps.iter().find(|d| d.id == "jei").unwrap().kind, DepKind::Optional);
    }

    #[test]
    fn reads_neoforge_types() {
        let toml = br#"
[[mods]]
modId="sodium"
version="0.6.0"
[[dependencies.sodium]]
modId="neoforge"
type="required"
versionRange="[21.1,)"
[[dependencies.sodium]]
modId="embeddium"
type="incompatible"
"#;
        let m = read(Cursor::new(jar(&[("META-INF/neoforge.mods.toml", toml)])), 0).unwrap();
        assert_eq!(m.formats, vec![Format::NeoForge]);
        let e = m.primary().unwrap();
        assert_eq!(e.deps.iter().find(|d| d.id == "embeddium").unwrap().kind, DepKind::Breaks);
    }

    #[test]
    fn reads_quilt_and_legacy() {
        let q = br#"{"schema_version":1,"quilt_loader":{"id":"qsl","version":"6.1","metadata":{"name":"QSL"},
            "depends":["quilt_loader",{"id":"org.quiltmc:minecraft","versions":">=1.20"},{"id":"x","optional":true}]}}"#;
        let m = read(Cursor::new(jar(&[("quilt.mod.json", q)])), 0).unwrap();
        let e = m.primary().unwrap();
        assert_eq!(e.name.as_deref(), Some("QSL"));
        assert!(e.deps.iter().any(|d| d.id == "minecraft" && d.range == ">=1.20"));
        assert_eq!(e.deps.iter().find(|d| d.id == "x").unwrap().kind, DepKind::Optional);

        let legacy = b"[{\"modid\":\"jei\",\"name\":\"JEI\",\"version\":\"4.16\",\"description\":\"line one\nline two\",\"requiredMods\":[\"Forge\",\"core@[1.0,)\"]}]";
        let m = read(Cursor::new(jar(&[("mcmod.info", legacy)])), 0).unwrap();
        assert_eq!(m.formats, vec![Format::LegacyForge]);
        assert_eq!(m.primary().unwrap().deps.len(), 1);
    }

    #[test]
    fn garbage_is_no_metadata() {
        assert!(read(Cursor::new(b"not a zip".to_vec()), 0).is_none());
        let m = read(Cursor::new(jar(&[("fabric.mod.json", b"{broken")])), 0).unwrap();
        assert!(m.mods.is_empty());
    }
}
