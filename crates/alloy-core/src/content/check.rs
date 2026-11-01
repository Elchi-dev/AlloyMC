//! The pre-launch check: everything that would make the game refuse to start or crash while
//! loading, found in milliseconds from the jars' own metadata instead of after a two-minute
//! wait for a crash report. It never guesses: a range it can't read, or a mod without
//! metadata, produces no problem.
//!
//! The same pass builds the dependency graph the Content tab uses: which mods each mod needs,
//! and which mods would break if one were removed.

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;

use super::jarmeta::{DepKind, Format, ModEntry};
use super::scan::{display_name, Scanned};
use super::version_range::{compare, Range};
use super::ContentKind;
use crate::instance::Instance;
use crate::meta::loaders::Loader;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// The game won't start or will crash while loading.
    Error,
    /// It starts, but something is likely to go wrong.
    Warning,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum ProblemKind {
    /// A required mod isn't installed.
    Missing { dep_id: String, wanted: String },
    /// A required mod is installed, but not a version this mod accepts.
    WrongVersion { dep_id: String, dep_path: Option<String>, wanted: String, found: String },
    /// This mod declares it doesn't work with another installed mod.
    Breaks { other_path: Option<String>, other_id: String, severe: bool },
    /// The same mod is installed twice.
    Duplicate { mod_id: String, other_path: String },
    /// Made for another loader.
    WrongLoader { made_for: Vec<Format> },
    /// Made for another Minecraft version.
    WrongGame { wanted: String },
    /// The loader is too old (or too new) for this mod.
    WrongLoaderVersion { wanted: String, found: String },
    /// Needs a newer Java than the instance uses.
    WrongJava { wanted: String, found: u32 },
    /// A server-only mod in a client instance.
    ServerOnly,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    pub severity: Severity,
    /// The mod the problem is about (its path in the instance).
    pub path: String,
    pub mod_name: String,
    #[serde(flatten)]
    pub kind: ProblemKind,
    /// One plain sentence for the UI.
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub problems: Vec<Problem>,
    /// path -> paths of the mods it needs.
    pub requires: BTreeMap<String, Vec<String>>,
    /// path -> paths of the mods that need it.
    pub required_by: BTreeMap<String, Vec<String>>,
    /// Mods Alloy installed only as a dependency that nothing needs any more.
    pub orphans: Vec<String>,
    pub mod_count: usize,
}

impl Report {
    pub fn errors(&self) -> usize {
        self.problems.iter().filter(|p| p.severity == Severity::Error).count()
    }
}

/// What the instance itself provides: the game, the loader, Java.
struct Platform {
    loader: Loader,
    game: String,
    loader_version: Option<String>,
    java: Option<u32>,
    /// Sinytra Connector is installed: Fabric mods load on (Neo)Forge too.
    bridge: bool,
}

impl Platform {
    /// Ids the loader answers to, with the version to check ranges against (None: unknown).
    fn builtin(&self, id: &str) -> Option<Option<String>> {
        let lv = self.loader_version.clone();
        match (self.loader, id) {
            (_, "minecraft") => Some(Some(self.game.clone())),
            (_, "java") => Some(self.java.map(|j| j.to_string())),
            (Loader::Fabric, "fabricloader") => Some(lv),
            (Loader::Quilt, "quilt_loader") => Some(lv),
            // Quilt answers to Fabric's id at a version of its own choosing.
            (Loader::Quilt, "fabricloader") => Some(None),
            (Loader::Forge, "forge") => Some(lv),
            (Loader::Forge, "fml" | "javafml" | "lowcodefml" | "mclanguage") => Some(None),
            (Loader::NeoForge, "neoforge") => Some(lv),
            // NeoForge 1.20.1 is a Forge fork and still answers to "forge".
            (Loader::NeoForge, "forge") if self.game == "1.20.1" => Some(lv),
            (Loader::NeoForge, "fml" | "javafml" | "lowcodefml" | "mclanguage") => Some(None),
            // Connector answers to Fabric Loader's id.
            (Loader::Forge | Loader::NeoForge, "fabricloader") if self.bridge => Some(None),
            _ => None,
        }
    }

    /// The metadata formats this loader reads, in the order it prefers them. A jar is judged
    /// only by the first of these it carries.
    fn formats(&self) -> Vec<Format> {
        let neo_new = self.loader == Loader::NeoForge && compare(&self.game, "1.20.5") != std::cmp::Ordering::Less;
        let mut f = match self.loader {
            Loader::Vanilla => vec![],
            Loader::Fabric => vec![Format::Fabric],
            Loader::Quilt => vec![Format::Quilt, Format::Fabric],
            // Forge 1.12.2 and older read only mcmod.info; 1.13 and newer only mods.toml.
            Loader::Forge if comparable_game(&self.game) && compare(&self.game, "1.13") == std::cmp::Ordering::Less => vec![Format::LegacyForge],
            Loader::Forge if comparable_game(&self.game) => vec![Format::Forge],
            Loader::Forge => vec![Format::Forge, Format::LegacyForge],
            Loader::NeoForge if neo_new => vec![Format::NeoForge],
            Loader::NeoForge => vec![Format::NeoForge, Format::Forge],
        };
        if self.bridge && matches!(self.loader, Loader::Forge | Loader::NeoForge) {
            f.push(Format::Fabric);
        }
        f
    }
}

fn active_entries<'a>(m: &'a Scanned, formats: &[Format]) -> Vec<&'a ModEntry> {
    m.jar.as_ref().map(|j| j.entries_for(formats)).unwrap_or_default()
}

/// Snapshots and other odd game versions can't be compared reliably; skip version checks there.
fn comparable_game(game: &str) -> bool {
    game.chars().all(|c| c.is_ascii_digit() || c == '.')
}

pub fn check(instance: &Instance, files: &[Scanned], java_major: Option<u32>) -> Report {
    let mods: Vec<&Scanned> = files.iter().filter(|s| s.kind == ContentKind::Mod && s.enabled).collect();
    let platform = Platform {
        loader: instance.loader,
        game: instance.game_version.clone(),
        loader_version: instance.loader_version.clone(),
        java: java_major,
        bridge: mods.iter().any(|m| m.jar.as_ref().is_some_and(|j| j.loads_fabric_mods)),
    };
    let mut report = Report { mod_count: mods.len(), ..Default::default() };
    if instance.loader == Loader::Vanilla {
        return report;
    }
    let formats = platform.formats();

    // Who provides what. Being generous here can only hide a problem, never invent one.
    let mut providers: HashMap<&str, Vec<(&Scanned, &str, bool)>> = HashMap::new();
    for m in &mods {
        let Some(jar) = &m.jar else { continue };
        let entries = active_entries(m, &formats);
        // A jar we can't place: count every entry, any format.
        let entries = if entries.is_empty() { jar.mods.iter().collect() } else { entries };
        for e in entries {
            providers.entry(e.id.as_str()).or_default().push((m, e.version.as_str(), true));
            for p in &e.provides {
                providers.entry(p.as_str()).or_default().push((m, e.version.as_str(), true));
            }
        }
        for e in &jar.nested {
            providers.entry(e.id.as_str()).or_default().push((m, e.version.as_str(), false));
            for p in &e.provides {
                providers.entry(p.as_str()).or_default().push((m, e.version.as_str(), false));
            }
        }
    }

    let push = |report: &mut Report, severity, m: &Scanned, kind: ProblemKind, message: String| {
        report.problems.push(Problem { severity, path: m.path.clone(), mod_name: display_name(m), kind, message });
    };

    // Duplicates: two top-level jars declaring the same mod. Fabric and Quilt load the newest
    // copy and ignore the rest, so that's untidy but fine; Forge and NeoForge refuse to start.
    let fabric_like = matches!(instance.loader, Loader::Fabric | Loader::Quilt);
    let mut by_id: BTreeMap<&str, Vec<(&Scanned, &str)>> = BTreeMap::new();
    for m in &mods {
        if let Some(primary) = active_entries(m, &formats).first() {
            by_id.entry(primary.id.as_str()).or_default().push((m, primary.version.as_str()));
        }
    }
    // Jars the loader won't load because a newer copy of the same mod is there.
    let mut ignored: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for (id, copies) in &by_id {
        if copies.len() < 2 {
            continue;
        }
        let (keep, keep_v) = *copies.iter().max_by(|a, b| compare(a.1, b.1)).unwrap();
        for (m, v) in copies.iter().filter(|(m, _)| m.path != keep.path) {
            let name = display_name(m);
            if fabric_like {
                ignored.insert(m.path.as_str());
                push(
                    &mut report,
                    Severity::Warning,
                    m,
                    ProblemKind::Duplicate { mod_id: id.to_string(), other_path: keep.path.clone() },
                    format!(
                        "{name} is installed twice ({} and {}). {} loads the newer one ({keep_v}) and ignores {v}, so you can remove it.",
                        m.file_name,
                        keep.file_name,
                        loader_name(instance.loader)
                    ),
                );
            } else {
                push(
                    &mut report,
                    Severity::Error,
                    m,
                    ProblemKind::Duplicate { mod_id: id.to_string(), other_path: keep.path.clone() },
                    format!("{name} is installed twice ({} and {}). Keep one.", m.file_name, keep.file_name),
                );
            }
        }
    }

    for m in &mods {
        let Some(jar) = &m.jar else { continue };
        let name = display_name(m);
        if jar.formats.is_empty() || ignored.contains(m.path.as_str()) {
            continue;
        }
        let entries = active_entries(m, &formats);
        if entries.is_empty() {
            let made_for = jar.formats.clone();
            let label = made_for.iter().map(|f| format_name(*f)).collect::<Vec<_>>().join(" or ");
            // Forge on both sides of 1.13: say which Minecraft it's for, not "made for Forge".
            let era = match (instance.loader, made_for.as_slice()) {
                (Loader::Forge, [Format::Forge]) => Some("Minecraft 1.13 or newer"),
                (Loader::Forge, [Format::LegacyForge]) => Some("Minecraft 1.12.2 or older"),
                _ => None,
            };
            if let Some(era) = era {
                push(
                    &mut report,
                    Severity::Error,
                    m,
                    ProblemKind::WrongGame { wanted: era.into() },
                    format!("{name} is made for {era}, not {}.", instance.game_version),
                );
                continue;
            }
            push(
                &mut report,
                Severity::Error,
                m,
                ProblemKind::WrongLoader { made_for },
                format!("{name} is made for {label}, and this instance runs {}.", loader_name(instance.loader)),
            );
            // Its dependencies are for the other loader; listing them would only add noise.
            continue;
        }
        // Bundled sub-mods are loaded like any other mod, so their dependencies count too.
        let nested: Vec<&ModEntry> = jar.nested.iter().filter(|e| e.format.is_none_or(|f| formats.contains(&f))).collect();
        let top_ids: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
        for (n, entry) in entries.iter().copied().chain(nested.iter().copied()).enumerate() {
            let is_top = n < top_ids.len();
            if is_top && entry.environment.as_deref() == Some("server") {
                push(
                    &mut report,
                    Severity::Warning,
                    m,
                    ProblemKind::ServerOnly,
                    format!("{name} only runs on servers; in a client it does nothing or crashes."),
                );
            }
            for dep in &entry.deps {
                if dep.server_only || dep.id == entry.id {
                    continue;
                }
                let range = dep.parsed();
                match dep.kind {
                    DepKind::Required => check_required(&mut report, &platform, &providers, m, &name, dep.id.as_str(), &range, push),
                    DepKind::Breaks | DepKind::Conflicts => {
                        let severe = dep.kind == DepKind::Breaks;
                        // The loader picks one copy of each mod (the newest that fits, and copies
                        // bundled inside other jars only when nothing better is there). It's a
                        // problem only if every copy it could pick is one this mod refuses.
                        let copies: Vec<_> = providers
                            .get(dep.id.as_str())
                            .into_iter()
                            .flatten()
                            .filter(|(p, _, _)| !ignored.contains(p.path.as_str()))
                            .collect();
                        // Copies inside this same jar count as choices (Fabric API bundles the
                        // newest of its own modules), but it can't be in conflict with itself.
                        let all_refused = copies.iter().any(|(p, _, _)| p.path != m.path) && copies.iter().all(|(_, v, _)| range.matches(v));
                        let hits = copies
                            .iter()
                            .filter(|(p, v, _)| all_refused && p.path != m.path && range.matches(v))
                            .max_by_key(|(_, _, top)| *top)
                            .map(|(p, v, t)| (*p, *v, *t));
                        if let Some((other, version, _)) = hits {
                            let other_name = display_name(other);
                            let what = if matches!(range, Range::Any) { String::new() } else { format!(" {version}") };
                            push(
                                &mut report,
                                if severe { Severity::Error } else { Severity::Warning },
                                m,
                                ProblemKind::Breaks { other_path: Some(other.path.clone()), other_id: dep.id.clone(), severe },
                                if severe {
                                    format!("{name} doesn't work with {other_name}{what}. Remove one of them.")
                                } else {
                                    format!("{name} and {other_name}{what} are known to cause trouble together.")
                                },
                            );
                        }
                    }
                    DepKind::Optional => {}
                }
                // The graph, for required dependencies another installed jar satisfies.
                if dep.kind == DepKind::Required {
                    if let Some((provider, _, _)) = providers.get(dep.id.as_str()).and_then(|v| v.iter().find(|(p, _, _)| p.path != m.path))
                    {
                        let req = report.requires.entry(m.path.clone()).or_default();
                        if !req.contains(&provider.path) {
                            req.push(provider.path.clone());
                            report.required_by.entry(provider.path.clone()).or_default().push(m.path.clone());
                        }
                    }
                }
            }
        }
    }

    // What the platforms say on top of the jars (many mods declare dependencies only there).
    let by_project: HashMap<(&str, super::Source), &Scanned> = mods
        .iter()
        .filter_map(|m| {
            let i = m.index.as_ref()?;
            Some(((i.project_id.as_deref()?, i.source), *m))
        })
        .collect();
    for m in &mods {
        let Some(i) = &m.index else { continue };
        for pid in i.requires.iter().flatten() {
            if let Some(dep) = by_project.get(&(pid.as_str(), i.source)) {
                if dep.path == m.path {
                    continue;
                }
                let req = report.requires.entry(m.path.clone()).or_default();
                if !req.contains(&dep.path) {
                    req.push(dep.path.clone());
                    report.required_by.entry(dep.path.clone()).or_default().push(m.path.clone());
                }
            }
        }
    }

    // Libraries Alloy pulled in for something that's gone now.
    for m in &mods {
        if m.index.as_ref().is_some_and(|i| i.auto) && !report.required_by.contains_key(&m.path) {
            report.orphans.push(m.path.clone());
        }
    }
    // Sub-mods often repeat their parent's requirements; say each thing once.
    let mut said = std::collections::HashSet::new();
    report.problems.retain(|p| said.insert((p.path.clone(), p.message.clone())));
    report.problems.sort_by(|a, b| a.severity.cmp(&b.severity).then_with(|| a.mod_name.to_lowercase().cmp(&b.mod_name.to_lowercase())));
    report
}

#[allow(clippy::too_many_arguments)]
fn check_required(
    report: &mut Report,
    platform: &Platform,
    providers: &HashMap<&str, Vec<(&Scanned, &str, bool)>>,
    m: &Scanned,
    name: &str,
    dep_id: &str,
    range: &Range,
    push: impl Fn(&mut Report, Severity, &Scanned, ProblemKind, String),
) {
    if let Some(builtin) = platform.builtin(dep_id) {
        let Some(found) = builtin else { return };
        if range.matches(&found) {
            return;
        }
        let wanted = range.describe();
        match dep_id {
            "minecraft" if comparable_game(&found) => push(
                report,
                Severity::Error,
                m,
                ProblemKind::WrongGame { wanted: wanted.clone() },
                format!("{name} is made for Minecraft {wanted}, not {found}."),
            ),
            "minecraft" => {}
            "java" => {
                let found_n: u32 = found.parse().unwrap_or(0);
                push(
                    report,
                    Severity::Error,
                    m,
                    ProblemKind::WrongJava { wanted: wanted.clone(), found: found_n },
                    format!("{name} needs Java {wanted}; this instance uses Java {found}."),
                )
            }
            _ => push(
                report,
                Severity::Error,
                m,
                ProblemKind::WrongLoaderVersion { wanted: wanted.clone(), found: found.clone() },
                format!("{name} needs {} {wanted}; this instance has {found}.", loader_name(platform.loader)),
            ),
        }
        return;
    }
    let candidates: Vec<_> = providers.get(dep_id).into_iter().flatten().filter(|(p, _, _)| p.path != m.path).collect();
    // Bundled inside the mod itself counts as present.
    let self_provided = providers.get(dep_id).into_iter().flatten().any(|(p, _, _)| p.path == m.path);
    if candidates.is_empty() {
        if self_provided {
            return;
        }
        let wanted = range.describe();
        let detail = if matches!(range, Range::Any) { String::new() } else { format!(" ({wanted})") };
        push(
            report,
            Severity::Error,
            m,
            ProblemKind::Missing { dep_id: dep_id.into(), wanted },
            format!("{name} needs {}{detail}, which isn't installed.", pretty_id(dep_id)),
        );
        return;
    }
    if candidates.iter().any(|(_, v, _)| v.is_empty() || range.matches(v)) || self_provided {
        return;
    }
    // Prefer reporting against the top-level jar the user can act on.
    let (dep, found, _) = candidates.iter().find(|(_, _, top)| *top).or(candidates.first()).unwrap();
    let wanted = range.describe();
    push(
        report,
        Severity::Error,
        m,
        ProblemKind::WrongVersion {
            dep_id: dep_id.into(),
            dep_path: Some(dep.path.clone()),
            wanted: wanted.clone(),
            found: found.to_string(),
        },
        format!("{name} needs {} {wanted}; {found} is installed.", display_name(dep)),
    );
}

fn loader_name(l: Loader) -> &'static str {
    match l {
        Loader::Vanilla => "vanilla Minecraft",
        Loader::Fabric => "Fabric",
        Loader::Quilt => "Quilt",
        Loader::Forge => "Forge",
        Loader::NeoForge => "NeoForge",
    }
}

fn format_name(f: Format) -> &'static str {
    match f {
        Format::Fabric => "Fabric",
        Format::Quilt => "Quilt",
        Format::Forge | Format::LegacyForge => "Forge",
        Format::NeoForge => "NeoForge",
    }
}

/// `cloth-config` -> `Cloth Config`, for messages about mods that aren't installed.
pub fn pretty_id(id: &str) -> String {
    match id {
        "fabric" | "fabric-api" => "Fabric API".into(),
        "fabricloader" => "Fabric Loader".into(),
        "connectormod" | "connector" => "Sinytra Connector".into(),
        "ftbquests" => "FTB Quests".into(),
        "ftblibrary" => "FTB Library".into(),
        "twilightforest" => "The Twilight Forest".into(),
        "temporalapi" => "Temporal API".into(),
        "geckolib" => "GeckoLib".into(),
        "cloth-config" | "cloth_config" => "Cloth Config".into(),
        "forgeconfigapiport" => "Forge Config API Port".into(),
        "kotlinforforge" => "Kotlin for Forge".into(),
        "yet_another_config_lib_v3" => "YetAnotherConfigLib".into(),
        "fabric-language-kotlin" => "Fabric Language Kotlin".into(),
        "jei" => "JEI".into(),
        "rei" | "roughlyenoughitems" => "Roughly Enough Items".into(),
        _ => id
            .split(['-', '_'])
            .filter(|w| !w.is_empty())
            .map(|w| {
                let mut c = w.chars();
                c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::jarmeta::{Dep, JarMeta, ModEntry};
    use crate::content::ContentMeta;

    fn inst(loader: Loader, game: &str, lv: &str) -> Instance {
        serde_json::from_value(serde_json::json!({
            "id": "t", "name": "t", "gameVersion": game, "loader": loader, "loaderVersion": lv, "createdAt": 0
        }))
        .unwrap()
    }

    fn jar(path: &str, format: Format, id: &str, version: &str, deps: Vec<Dep>) -> Scanned {
        Scanned {
            path: format!("mods/{path}"),
            file_name: path.into(),
            kind: ContentKind::Mod,
            enabled: true,
            size: 1,
            sha1: String::new(),
            murmur2: 0,
            jar: Some(JarMeta {
                formats: vec![format],
                mods: vec![ModEntry {
                    id: id.into(),
                    version: version.into(),
                    name: Some(id.into()),
                    deps,
                    format: Some(format),
                    ..Default::default()
                }],
                nested: vec![],
                loads_fabric_mods: false,
                packages: vec![],
            }),
            index: None,
        }
    }

    fn dep(id: &str, range: &str, kind: DepKind) -> Dep {
        Dep { id: id.into(), kind, range: range.into(), maven: false, server_only: false }
    }

    #[test]
    fn finds_missing_and_wrong_versions() {
        let i = inst(Loader::Fabric, "1.20.1", "0.16.5");
        let files = vec![
            jar(
                "sodium.jar",
                Format::Fabric,
                "sodium",
                "0.5.3",
                vec![dep("fabricloader", ">=0.12", DepKind::Required), dep("minecraft", "1.20.x", DepKind::Required)],
            ),
            jar(
                "addon.jar",
                Format::Fabric,
                "addon",
                "1.0",
                vec![dep("sodium", ">=0.6", DepKind::Required), dep("cloth-config", "*", DepKind::Required)],
            ),
        ];
        let r = check(&i, &files, Some(17));
        let kinds: Vec<_> = r.problems.iter().map(|p| &p.kind).collect();
        assert_eq!(r.errors(), 2, "{kinds:?}");
        assert!(r.problems.iter().any(|p| matches!(&p.kind, ProblemKind::Missing { dep_id, .. } if dep_id == "cloth-config")));
        assert!(r.problems.iter().any(|p| matches!(&p.kind, ProblemKind::WrongVersion { found, .. } if found == "0.5.3")));
        assert!(r.problems.iter().find(|p| p.message.contains("Cloth Config")).is_some());
        assert_eq!(r.required_by.get("mods/sodium.jar").unwrap(), &vec!["mods/addon.jar".to_string()]);
    }

    #[test]
    fn wrong_loader_game_java_and_duplicates() {
        let i = inst(Loader::NeoForge, "1.21.1", "21.1.265");
        let mut forge_mod = jar("old.jar", Format::Forge, "old", "1", vec![]);
        forge_mod.path = "mods/old.jar".into();
        let files = vec![
            jar("a.jar", Format::Fabric, "a", "1", vec![]),
            forge_mod,
            jar("b1.jar", Format::NeoForge, "b", "1", vec![Dep { maven: true, ..dep("minecraft", "[1.20.1,1.21)", DepKind::Required) }]),
            jar("b2.jar", Format::NeoForge, "b", "2", vec![Dep { maven: true, ..dep("java", "[25,)", DepKind::Required) }]),
        ];
        let r = check(&i, &files, Some(21));
        let has = |f: &dyn Fn(&ProblemKind) -> bool| r.problems.iter().any(|p| f(&p.kind));
        assert!(has(&|k| matches!(k, ProblemKind::WrongLoader { .. })));
        assert_eq!(
            r.problems.iter().filter(|p| matches!(p.kind, ProblemKind::WrongLoader { .. })).count(),
            2,
            "Forge mods don't load on NeoForge 1.21"
        );
        assert!(has(&|k| matches!(k, ProblemKind::WrongGame { .. })));
        assert!(has(&|k| matches!(k, ProblemKind::WrongJava { found: 21, .. })));
        assert!(has(&|k| matches!(k, ProblemKind::Duplicate { .. })));
    }

    #[test]
    fn bundled_libraries_and_unknowns_are_fine() {
        let i = inst(Loader::Quilt, "1.20.1", "0.26");
        let mut m = jar(
            "m.jar",
            Format::Fabric,
            "m",
            "1",
            vec![
                dep("lib", ">=2", DepKind::Required),
                dep("fabricloader", ">=0.15", DepKind::Required),
                dep("weird", "[[[", DepKind::Breaks),
            ],
        );
        m.jar.as_mut().unwrap().nested.push(ModEntry { id: "lib".into(), version: "2.1".into(), ..Default::default() });
        let r = check(&i, &[m, jar("weird.jar", Format::Fabric, "weird", "1", vec![])], None);
        assert!(r.problems.is_empty(), "{:?}", r.problems);
    }

    /// RLCraft 2.9.3 (Forge 1.12.2) starts fine; some of its jars carry a mods.toml for a
    /// newer Forge next to the mcmod.info Forge 1.12.2 actually reads.
    #[test]
    fn rlcraft_reads_what_forge_1_12_reads() {
        let i = inst(Loader::Forge, "1.12.2", "14.23.5.2860");
        let mut shields = jar("SpartanShields-1.12.2-1.5.5.jar", Format::LegacyForge, "spartanshields", "1.5.5", vec![]);
        shields.jar.as_mut().unwrap().formats.insert(0, Format::Forge);
        shields.jar.as_mut().unwrap().mods.insert(
            0,
            ModEntry {
                id: "spartanshields".into(),
                version: "${file.jarVersion}".into(),
                deps: vec![Dep { maven: true, ..dep("forge", "[25,)", DepKind::Required) }, Dep { maven: true, ..dep("minecraft", "[1.13.2]", DepKind::Required) }],
                format: Some(Format::Forge),
                ..Default::default()
            },
        );
        let mut visuals = jar("EnhancedVisuals_v1.4.4_mc1.12.2.jar", Format::LegacyForge, "enhancedvisuals", "1.3", vec![]);
        visuals.jar.as_mut().unwrap().formats.insert(0, Format::Forge);
        visuals.jar.as_mut().unwrap().mods.insert(
            0,
            ModEntry {
                id: "enhancedvisuals".into(),
                version: "1.3.0".into(),
                deps: vec![Dep { maven: true, ..dep("creativecore", "[2.0.0,)", DepKind::Required) }],
                format: Some(Format::Forge),
                ..Default::default()
            },
        );
        let core = jar("CreativeCore_v1.10.70_mc1.12.2.jar", Format::LegacyForge, "creativecore", "1.10", vec![]);
        let r = check(&i, &[shields, visuals, core], Some(8));
        assert!(r.problems.is_empty(), "{:#?}", r.problems);

        // And a mod that only has a mods.toml really is for a newer Minecraft.
        let new = jar("newmod.jar", Format::Forge, "newmod", "1", vec![]);
        let r = check(&i, &[new], Some(8));
        assert_eq!(r.errors(), 1);
        assert!(r.problems[0].message.contains("1.13 or newer"), "{}", r.problems[0].message);
    }

    /// Homestead 2.0.1 (Fabric 1.20.1, 381 mods) starts fine; these used to be reported as
    /// three errors, and the offered fixes would have broken it.
    #[test]
    fn homestead_starts_like_fabric_says() {
        let i = inst(Loader::Fabric, "1.20.1", "0.19.5");
        let old_fd = jar("farmers-delight-fabric-1.4.3.jar", Format::Fabric, "farmersdelight", "1.20.1-1.4.3", vec![]);
        let new_fd = jar("FarmersDelight-1.20.1-2.4.1+refabricated.jar", Format::Fabric, "farmersdelight", "1.20.1-2.4.1+refabricated", vec![]);
        let slice = jar(
            "sliceanddice-fabric-3.5.2.jar",
            Format::Fabric,
            "sliceanddice",
            "3.5.2-fabric",
            vec![dep("farmersdelight", "<1.20.1-2.0.0", DepKind::Breaks)],
        );
        // Fabric API breaks old copies of its own modules; CoroUtil bundles an old Fabric API.
        let mut api = jar("fabric-api-0.92.12+1.20.1.jar", Format::Fabric, "fabric-api", "0.92.12+1.20.1", vec![]);
        api.jar.as_mut().unwrap().nested.push(ModEntry {
            id: "fabric-networking-api-v1".into(),
            version: "1.3.13+b3afc78b82".into(),
            deps: vec![dep("fabric-networking-api-v1", "<1.0.0", DepKind::Breaks), dep("fabric-api-base", "<0.4.0", DepKind::Breaks)],
            ..Default::default()
        });
        api.jar.as_mut().unwrap().nested.push(ModEntry { id: "fabric-api-base".into(), version: "0.4.31+1802ada577".into(), ..Default::default() });
        let mut coro = jar("coroutil-fabric-1.20.1-1.3.7.jar", Format::Fabric, "coroutil", "1.20.1-1.3.7", vec![]);
        coro.jar.as_mut().unwrap().nested.push(ModEntry { id: "fabric-api-base".into(), version: "0.3.35+b3afc78b82".into(), ..Default::default() });
        let r = check(&i, &[old_fd, new_fd, slice, api, coro], Some(17));
        assert_eq!(r.errors(), 0, "{:#?}", r.problems);
        let dup: Vec<_> = r.problems.iter().filter(|p| matches!(p.kind, ProblemKind::Duplicate { .. })).collect();
        assert_eq!(dup.len(), 1);
        assert_eq!(dup[0].severity, Severity::Warning);
        assert_eq!(dup[0].path, "mods/farmers-delight-fabric-1.4.3.jar", "the ignored, older copy is the one to remove");
    }

    #[test]
    fn breaks_still_found_when_no_good_copy_exists() {
        let i = inst(Loader::Fabric, "1.20.1", "0.16");
        let slice = jar("slice.jar", Format::Fabric, "sliceanddice", "3", vec![dep("farmersdelight", "<1.20.1-2.0.0", DepKind::Breaks)]);
        let old_fd = jar("fd.jar", Format::Fabric, "farmersdelight", "1.20.1-1.4.3", vec![]);
        let r = check(&i, &[slice, old_fd], Some(17));
        assert_eq!(r.errors(), 1);
        assert!(matches!(&r.problems[0].kind, ProblemKind::Breaks { other_path: Some(p), .. } if p == "mods/fd.jar"));
    }

    #[test]
    fn breaks_and_orphans() {
        let i = inst(Loader::Fabric, "1.21.1", "0.16");
        let mut lib = jar("lib.jar", Format::Fabric, "lib", "1", vec![]);
        lib.index = Some(ContentMeta { auto: true, ..ContentMeta::local() });
        let files = vec![
            jar("iris.jar", Format::Fabric, "iris", "1.8", vec![dep("optifabric", "*", DepKind::Breaks)]),
            jar("optifabric.jar", Format::Fabric, "optifabric", "1.14", vec![]),
            lib,
        ];
        let r = check(&i, &files, None);
        assert!(r.problems.iter().any(|p| matches!(p.kind, ProblemKind::Breaks { severe: true, .. })));
        assert_eq!(r.orphans, vec!["mods/lib.jar".to_string()]);
    }
}
