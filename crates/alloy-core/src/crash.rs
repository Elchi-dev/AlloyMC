//! The crash doctor: after a crash, reads what the game left behind (the crash report, the
//! log, a JVM `hs_err` file, the last lines it printed) and says what went wrong in plain
//! words, which mod it points at and how sure that is, and what would fix it.
//!
//! Like the pre-launch check, it never invents a culprit: a mod is named only when the files
//! say so (a loader naming it, a mixin of it failing, its code at the top of the stack).
//! When nothing does, the answer is "unknown" and the bisect in `bisect` can find it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::content::scan::{display_name, Scanned};
use crate::content::ContentKind;
use crate::instance::Instance;
use crate::settings::write_json_atomic;
use crate::{Launcher, Result};

/// Everything the game left behind after one run.
#[derive(Debug, Clone, Default)]
pub struct Evidence {
    pub exit_code: Option<i32>,
    /// The last lines the game printed (stdout and stderr), oldest first.
    pub output: Vec<String>,
    /// A crash report written during this run.
    pub crash_report: Option<(PathBuf, String)>,
    /// `logs/latest.log`, if it was written during this run.
    pub latest_log: Option<String>,
    /// A JVM fatal error log (`hs_err_pid*.log`) from this run.
    pub native: Option<(PathBuf, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CrashKind {
    OutOfMemory,
    WrongJava,
    GraphicsDriver,
    NativeCrash,
    MixinFailed,
    MissingDependency,
    DuplicateMod,
    ModError,
    Killed,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

/// A mod the evidence points at.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Suspect {
    /// Path relative to the instance, without `.disabled`.
    pub path: String,
    pub name: String,
    pub mod_id: Option<String>,
    pub confidence: Confidence,
    /// Why, in a sentence.
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CrashFix {
    /// Turn this mod off.
    DisableMod { path: String },
    /// Give the game this much memory.
    RaiseMemory { mb: u32 },
    /// This Java major version runs it.
    UseJava { major: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnosis {
    pub kind: CrashKind,
    /// One line: what happened.
    pub title: String,
    /// A few sentences: why, and what to do.
    pub detail: String,
    pub suspects: Vec<Suspect>,
    pub fixes: Vec<CrashFix>,
    /// The lines that gave it away, to show under "Details".
    pub evidence: Vec<String>,
    /// The crash report or JVM error log, to open or upload.
    pub report_path: Option<String>,
    /// The game got to the title screen before it crashed.
    pub after_start: bool,
    /// Finding the mod by starting the game with halves of the mods off could work here.
    pub can_bisect: bool,
    pub exit_code: Option<i32>,
    /// Unix seconds.
    pub at: i64,
}

// ---------- gathering ----------

/// Lines that mean the game reached the title screen.
pub fn reached_menu(line: &str) -> bool {
    line.contains("Sound engine started") || line.contains("OpenAL initialized") || line.contains("Created: 1024x")
}

/// Reads what this run left in the instance folder. `since` is when the game started (unix
/// seconds); older files belong to earlier runs.
pub async fn gather(launcher: &Launcher, instance: &Instance, since: i64, exit_code: Option<i32>, output: Vec<String>) -> Evidence {
    let dir = instance.dir(launcher);
    let newest = |folder: PathBuf, pick: fn(&str) -> bool| async move {
        let mut best: Option<(i64, PathBuf)> = None;
        let mut rd = tokio::fs::read_dir(&folder).await.ok()?;
        while let Ok(Some(e)) = rd.next_entry().await {
            let name = e.file_name().to_string_lossy().to_string();
            if !pick(&name) {
                continue;
            }
            let Ok(m) = e.metadata().await else { continue };
            let t = modified(&m);
            if t + 2 >= since && best.as_ref().is_none_or(|(bt, _)| t > *bt) {
                best = Some((t, e.path()));
            }
        }
        let (_, path) = best?;
        let text = read_text(&path).await?;
        Some((path, text))
    };
    let crash_report = newest(dir.join("crash-reports"), |n| n.starts_with("crash-") && n.ends_with(".txt")).await;
    let native = newest(dir.clone(), |n| n.starts_with("hs_err_pid") && n.ends_with(".log")).await;
    let latest_log = newest(dir.join("logs"), |n| n == "latest.log").await.map(|(_, t)| t);
    Evidence { exit_code, output, crash_report, latest_log, native }
}

fn modified(m: &std::fs::Metadata) -> i64 {
    m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// The end of a text file, at most 2 MB, lossily decoded.
async fn read_text(path: &Path) -> Option<String> {
    let bytes = tokio::fs::read(path).await.ok()?;
    let start = bytes.len().saturating_sub(2 << 20);
    Some(String::from_utf8_lossy(&bytes[start..]).into_owned())
}

// ---------- stack traces ----------

#[derive(Debug, Clone)]
struct Trace {
    exception: String,
    message: String,
    /// (class, method) from the `at` lines, top first.
    frames: Vec<(String, String)>,
    caused_by: bool,
}

static RE_EXC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:Caused by: |Exception in thread .*? )?((?:[a-zA-Z_$][\w$]*\.)+[\w$]*(?:Exception|Error|Throwable|Failure)[\w$]*)(?::\s?(.*))?$").unwrap()
});
static RE_AT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*at (?:[\w.\-@]+/{1,2})?([\w$.]+)\.([\w$<>]+)\(").unwrap());

fn traces(text: &str) -> Vec<Trace> {
    let mut out: Vec<Trace> = vec![];
    // Frames belong to the exception right above them; anything else in between ends it.
    let mut open = false;
    for raw in text.lines() {
        // Log lines carry a "[12:00:00] [Render thread/ERROR]: " prefix; traces don't.
        let line = raw.trim_end();
        let bare = line.trim_start();
        if let Some(c) = RE_AT.captures(line) {
            if let (Some(t), true) = (out.last_mut(), open) {
                if t.frames.len() < 200 {
                    t.frames.push((c[1].to_string(), c[2].to_string()));
                }
            }
            continue;
        }
        if bare.starts_with("...") {
            continue;
        }
        open = false;
        if let Some(c) = RE_EXC.captures(bare) {
            open = true;
            out.push(Trace {
                exception: c[1].to_string(),
                message: c.get(2).map(|m| m.as_str().to_string()).unwrap_or_default(),
                frames: vec![],
                caused_by: bare.starts_with("Caused by:"),
            });
        }
    }
    out.retain(|t| !t.frames.is_empty() || !t.message.is_empty());
    out
}

/// Packages that belong to Java, Minecraft, the loaders or libraries everyone bundles:
/// their frames say nothing about which mod is at fault.
const PLATFORM: &[&str] = &[
    "java.", "javax.", "jdk.", "sun.", "com.sun.", "net.minecraft.", "com.mojang.", "org.lwjgl.", "org.apache.", "com.google.",
    "it.unimi.", "io.netty.", "org.slf4j.", "kotlin.", "kotlinx.", "org.spongepowered.", "org.objectweb.", "net.fabricmc.loader.",
    "net.fabricmc.api.", "cpw.mods.", "net.minecraftforge.fml.", "net.minecraftforge.eventbus.", "net.minecraftforge.common.",
    "net.neoforged.fml.", "net.neoforged.bus.", "net.neoforged.neoforge.common.", "org.quiltmc.loader.", "oshi.", "joptsimple.",
    "com.electronwill.", "org.jetbrains.", "org.joml.", "com.llamalad7.mixinextras.", "MC-BOOTSTRAP", "net.minecraftforge.server.",
    "org.openjdk.", "io.github.zekerzhayard.", "com.mojang.",
];

fn is_platform(class: &str) -> bool {
    PLATFORM.iter().any(|p| class.starts_with(p))
}

/// Mods that sit under everything (APIs, libraries): their frames show up in traces of
/// crashes they didn't cause, so they count for less.
fn is_library(id: &str) -> bool {
    id.starts_with("fabric")
        || matches!(
            id,
            "fabric_api" | "forge" | "neoforge" | "minecraft" | "java" | "architectury" | "cloth_config" | "cloth-config" | "owo" | "geckolib"
                | "balm" | "puzzleslib" | "moonlight" | "framework" | "resourcefullib" | "bookshelf" | "collective" | "kotlinforforge"
                | "fabric_language_kotlin" | "forgeconfigapiport" | "cardinal-components" | "playeranimator" | "curios" | "trinkets"
                | "citadel" | "terrablender" | "lithostitched" | "yungsapi" | "cristellib" | "mixinextras" | "connectormod" | "kotlinlangforge"
        )
}

static RE_MIXIN_HANDLER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:handler|redirect|modify\w*|wrap\w*|localvar|inject\w*|md[0-9a-f]+)\$[\w]*?\$([a-z][a-z0-9_\-]{1,63})\$").unwrap()
});

// ---------- the diagnosis ----------

struct Mods<'a> {
    by_id: HashMap<String, &'a Scanned>,
    /// (package, jar), longest package first.
    packages: Vec<(String, &'a Scanned)>,
    by_file: HashMap<String, &'a Scanned>,
}

impl<'a> Mods<'a> {
    fn new(files: &'a [Scanned]) -> Self {
        let mut by_id = HashMap::new();
        let mut packages = vec![];
        let mut by_file = HashMap::new();
        for s in files.iter().filter(|s| s.kind == ContentKind::Mod) {
            by_file.insert(s.file_name.to_lowercase(), s);
            if let Some(j) = &s.jar {
                for (id, _) in j.provided() {
                    // A mod's own entry wins over another jar bundling it.
                    let own = j.mods.iter().any(|m| m.id == id);
                    if own || !by_id.contains_key(id) {
                        by_id.insert(id.to_string(), s);
                    }
                }
                for p in &j.packages {
                    packages.push((format!("{p}."), s));
                }
            }
        }
        packages.sort_by_key(|p| std::cmp::Reverse(p.0.len()));
        Mods { by_id, packages, by_file }
    }

    fn owner(&self, class: &str) -> Option<&'a Scanned> {
        if is_platform(class) {
            return None;
        }
        let hits: Vec<&(String, &Scanned)> = self.packages.iter().filter(|(p, _)| class.starts_with(p.as_str())).collect();
        let first = hits.first()?;
        // Two jars with the same package (one bundling the other's code): no verdict.
        if hits.iter().any(|(p, s)| p.len() == first.0.len() && s.path != first.1.path) {
            return None;
        }
        Some(first.1)
    }

    fn id(&self, id: &str) -> Option<&'a Scanned> {
        let id = id.trim_matches(|c| c == '\'' || c == '"' || c == '`');
        self.by_id.get(id).or_else(|| self.by_id.get(&id.replace('-', "_"))).copied()
    }
}

fn primary_id(s: &Scanned) -> Option<String> {
    s.jar.as_ref().and_then(|j| j.primary()).map(|m| m.id.clone())
}

#[derive(Default)]
struct Board {
    scores: HashMap<String, (f32, Confidence, String)>,
    order: Vec<String>,
}

impl Board {
    fn add(&mut self, s: &Scanned, score: f32, conf: Confidence, reason: String) {
        let e = self.scores.entry(s.path.clone()).or_insert_with(|| {
            self.order.push(s.path.clone());
            (0.0, Confidence::Low, reason.clone())
        });
        e.0 += score;
        if conf > e.1 {
            e.1 = conf;
            e.2 = reason;
        }
    }

    fn suspects(self, files: &[Scanned]) -> Vec<Suspect> {
        let mut list: Vec<(String, f32, Confidence, String)> =
            self.order.into_iter().filter_map(|p| self.scores.get(&p).map(|(s, c, r)| (p.clone(), *s, *c, r.clone()))).collect();
        list.sort_by(|a, b| b.2.cmp(&a.2).then(b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)));
        list.into_iter()
            .take(3)
            .filter_map(|(path, _, confidence, reason)| {
                let s = files.iter().find(|f| f.path == path)?;
                Some(Suspect { path, name: display_name(s), mod_id: primary_id(s), confidence, reason })
            })
            .collect()
    }
}

static RE: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
    [
        ("mixin_mod", r"Mixin apply (?:for mod|failed for) ([\w\-]+)"),
        ("mixin_from", r"(?:from|in) mod ([\w\-]+)\]?"),
        ("mixin_config", r"mixin config ([\w\-.]+?)\.mixins?\.json"),
        ("provided_by", r"provided by '([\w\-]+)'"),
        ("mod_file", r"(?i:Mod File): (?:.*[/\\])?([^/\\]+\.jar)"),
        ("failed_load", r"([\w\-]+) \(([\w\-]+)\) has failed to load correctly"),
        ("loading_issue", r"-- Mod loading issue for: ([\w\-]+) --"),
        ("suspected", r"^\s+.+ \(([\w\-]+)\), Version: "),
        ("dup", r"(?:Mod ID: '([\w\-]+)' from mod files|Duplicate mods? (?:found|with id) '?([\w\-]+))"),
        ("class_version", r"class file version (\d+)(?:\.\d+)?\).*?(?:up to|recognizes class file versions up to) (\d+)"),
        ("class_version_any", r"class file version (\d+)\.0"),
        ("requires", r"(?:Mod '?[^']*'? \(([\w\-]+)\) [\d.\-+\w]* requires|requires (?:any version|version [^ ]+) of '?([\w\-]+)'?)"),
        ("missing_class", r"(?:NoClassDefFoundError|ClassNotFoundException): ([\w/$.]+)"),
    ]
    .into_iter()
    .map(|(k, r)| (k, Regex::new(&format!("(?m){r}")).unwrap()))
    .collect()
});

fn re(name: &str) -> &'static Regex {
    &RE.iter().find(|(k, _)| *k == name).expect("pattern").1
}

/// Turns the evidence into a diagnosis. `files` is the instance's scan; `memory_mb` what the
/// game had; `max_mb` the most this machine can give it.
pub fn diagnose(e: &Evidence, files: &[Scanned], memory_mb: u32, max_mb: u32, now: i64) -> Diagnosis {
    let mods = Mods::new(files);
    let report = e.crash_report.as_ref().map(|(_, t)| t.as_str()).unwrap_or("");
    let log = e.latest_log.as_deref().unwrap_or("");
    let out = e.output.join("\n");
    let native = e.native.as_ref().map(|(_, t)| t.as_str()).unwrap_or("");
    // Search everything, the crash report first.
    let all = format!("{report}\n{out}\n{log}");
    let after_start = out.lines().chain(log.lines()).any(reached_menu);
    let mut board = Board::default();
    let mut evidence: Vec<String> = vec![];
    let keep = |line: &str, evidence: &mut Vec<String>| {
        let l = line.trim();
        if !l.is_empty() && evidence.len() < 12 && !evidence.iter().any(|x| x == l) {
            evidence.push(l.chars().take(400).collect());
        }
    };
    let line_of = |text: &str, m: regex::Match| -> String {
        let start = text[..m.start()].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let end = text[m.end()..].find('\n').map(|i| m.end() + i).unwrap_or(text.len());
        text[start..end].to_string()
    };

    // What the loaders say outright.
    for (pat, conf, why) in [
        ("mixin_mod", Confidence::High, "A mixin of this mod failed to apply."),
        ("provided_by", Confidence::High, "The loader says the failing code is this mod's."),
        ("failed_load", Confidence::High, "The loader says this mod failed to load."),
        ("loading_issue", Confidence::High, "The loader says this mod failed to load."),
        ("suspected", Confidence::Medium, "The crash report lists it as a suspect."),
    ] {
        for c in re(pat).captures_iter(&all) {
            let id = c.iter().skip(1).flatten().last().map(|m| m.as_str()).unwrap_or_default();
            if let Some(s) = mods.id(id) {
                board.add(s, 10.0, conf, why.into());
                keep(&line_of(&all, c.get(0).unwrap()), &mut evidence);
            }
        }
    }
    for c in re("mod_file").captures_iter(&all) {
        if let Some(s) = mods.by_file.get(&c[1].to_lowercase()) {
            board.add(s, 10.0, Confidence::High, "The loader names this file in its error.".into());
            keep(&line_of(&all, c.get(0).unwrap()), &mut evidence);
        }
    }

    let traces = traces(if report.is_empty() { &all } else { report });
    let traces = if traces.is_empty() { self::traces(&all) } else { traces };
    let mixin_error = traces.iter().any(|t| t.exception.contains("Mixin") || t.message.contains("Mixin") || t.message.contains("mixin"));
    if mixin_error {
        for t in &traces {
            let m = format!("{} {}", t.exception, t.message);
            for c in re("mixin_from").captures_iter(&m).chain(re("mixin_mod").captures_iter(&m)) {
                if let Some(s) = mods.id(&c[1]) {
                    board.add(s, 10.0, Confidence::High, "A mixin of this mod failed to apply.".into());
                }
            }
            if let Some(c) = re("mixin_config").captures(&m) {
                let cfg = c[1].to_lowercase();
                if let Some((_, s)) = mods.by_id.iter().filter(|(id, _)| cfg.starts_with(&id.replace('-', "_")) || cfg.starts_with(id.as_str())).max_by_key(|(id, _)| id.len()) {
                    board.add(s, 8.0, Confidence::High, "A mixin of this mod failed to apply.".into());
                }
            }
        }
    }

    // Whose code is in the stack: the deepest cause matters most, the top frames most of all.
    let root = traces.iter().rposition(|t| t.caused_by).or(if traces.is_empty() { None } else { Some(0) });
    for (ti, t) in traces.iter().enumerate().take(6) {
        let weight = if Some(ti) == root { 1.0 } else { 0.5 };
        let mut seen = 0;
        for (fi, (class, method)) in t.frames.iter().enumerate().take(40) {
            let pos = 1.0 / (1.0 + fi as f32 * 0.5);
            if let Some(c) = RE_MIXIN_HANDLER.captures(method) {
                if let Some(s) = mods.id(&c[1]) {
                    let lib = primary_id(s).as_deref().is_some_and(is_library);
                    board.add(s, 4.0 * pos * weight * if lib { 0.3 } else { 1.0 }, if fi < 3 { Confidence::Medium } else { Confidence::Low }, "Its code (a mixin) is near the top of the crash.".into());
                    seen += 1;
                    continue;
                }
            }
            if let Some(s) = mods.owner(class) {
                let lib = primary_id(s).as_deref().is_some_and(is_library);
                let conf = if seen == 0 && fi < 4 && !lib { Confidence::Medium } else { Confidence::Low };
                board.add(s, 3.0 * pos * weight * if lib { 0.3 } else { 1.0 }, conf, "Its code is at the top of the crash.".into());
                seen += 1;
            }
            if seen >= 4 {
                break;
            }
        }
    }
    if let Some(i) = root {
        let t = &traces[i];
        keep(&format!("{}: {}", t.exception, t.message), &mut evidence);
        for (c, m) in t.frames.iter().take(4) {
            keep(&format!("at {c}.{m}"), &mut evidence);
        }
    }

    let suspects = board.suspects(files);
    let top = suspects.first().filter(|s| s.confidence >= Confidence::Medium).cloned();
    let report_path = e.crash_report.as_ref().map(|(p, _)| p).or(e.native.as_ref().map(|(p, _)| p)).map(|p| p.to_string_lossy().into_owned());
    let enabled_mods = files.iter().filter(|f| f.kind == ContentKind::Mod && f.enabled).count();
    let mut d = Diagnosis {
        kind: CrashKind::Unknown,
        title: String::new(),
        detail: String::new(),
        suspects,
        fixes: vec![],
        evidence: vec![],
        report_path,
        after_start,
        can_bisect: !after_start && enabled_mods >= 2,
        exit_code: e.exit_code,
        at: now,
    };
    let has = |needle: &str| all.contains(needle) || native.contains(needle);

    // ---- what kind of crash ----
    if has("java.lang.OutOfMemoryError") || native.contains("Out of Memory Error") || native.contains("insufficient memory") {
        d.kind = CrashKind::OutOfMemory;
        d.title = "Minecraft ran out of memory".into();
        let want = ((memory_mb.max(2048) + 2048).min(max_mb.max(memory_mb)) / 512) * 512;
        if want > memory_mb {
            d.detail = format!("It had {} GB. Giving it {} GB should be enough for this pack.", gb(memory_mb), gb(want));
            d.fixes.push(CrashFix::RaiseMemory { mb: want });
        } else {
            d.detail = format!("It had {} GB, which is all this PC can spare. Close other programs, or remove a few large mods.", gb(memory_mb));
        }
        d.can_bisect = false;
    } else if let Some(c) = re("class_version").captures(&all).or_else(|| re("class_version_any").captures(&all)).filter(|_| has("UnsupportedClassVersionError") || has("class file version")) {
        let need: u32 = c[1].parse::<u32>().unwrap_or(52).saturating_sub(44);
        d.kind = CrashKind::WrongJava;
        d.title = format!("This needs Java {need}");
        d.detail = match &top {
            Some(s) => format!("{} is built for Java {need}, newer than the Java this instance runs on. Switch the instance to Java {need}, or use a version of the mod made for this Minecraft version.", s.name),
            None => format!("Something in this instance is built for Java {need}, newer than the Java it runs on. Switch the instance to Java {need}."),
        };
        d.fixes.push(CrashFix::UseJava { major: need });
        d.can_bisect = false;
    } else if is_graphics(&all, native) {
        d.kind = CrashKind::GraphicsDriver;
        d.title = "The graphics driver crashed".into();
        d.detail = "The crash happened inside the graphics driver, not in Minecraft or a mod. Update your graphics driver from your GPU maker's site. On laptops with two GPUs, make Java use the dedicated one. Shader packs and mods like Sodium or Iris can trigger driver bugs; if updating doesn't help, try without them.".into();
        d.can_bisect = d.can_bisect && top.is_some();
    } else if mixin_error && top.is_some() {
        let s = top.as_ref().unwrap();
        d.kind = CrashKind::MixinFailed;
        d.title = format!("{} doesn't fit this setup", s.name);
        d.detail = format!("{} changes Minecraft's code, and that change failed. Usually it's built for another Minecraft or loader version, or it clashes with another mod that changes the same code. Update it, or turn it off.", s.name);
    } else if let Some(c) = re("dup").captures(&all).filter(|_| has("uplicate")) {
        let id = c.iter().skip(1).flatten().next().map(|m| m.as_str().to_string()).unwrap_or_default();
        d.kind = CrashKind::DuplicateMod;
        d.title = "A mod is installed twice".into();
        d.detail = format!("Two files both contain the mod \"{id}\". Keep the newer one and remove the other.");
        d.can_bisect = false;
    } else if has("NoClassDefFoundError") || has("ClassNotFoundException") || re("requires").is_match(&all) && has("requires") && !after_start {
        d.kind = CrashKind::MissingDependency;
        let class = re("missing_class").captures(&all).map(|c| c[1].replace('/', "."));
        d.title = match &top {
            Some(s) => format!("{} is missing something it needs", s.name),
            None => "A mod is missing something it needs".into(),
        };
        d.detail = match class {
            Some(c) => format!("It tried to use {c}, which isn't installed. That's usually a library mod missing, or one of the wrong version. Alloy's pre-launch check can't see every requirement; the mod's page lists what it needs."),
            None => "A mod needs another mod that isn't installed, or a different version of it. The mod's page lists what it needs.".into(),
        };
    } else if native.contains("A fatal error has been detected") || matches!(e.exit_code, Some(-1073741819) | Some(-1073740791) | Some(134) | Some(139)) {
        d.kind = CrashKind::NativeCrash;
        d.title = "Java itself crashed".into();
        d.detail = "The Java runtime hit a fatal error, below Minecraft and its mods. Updating the graphics driver fixes most of these; switching the instance to a different Java build fixes most of the rest.".into();
    } else if matches!(e.exit_code, Some(137) | Some(143) | Some(-9)) && report.is_empty() {
        d.kind = CrashKind::Killed;
        d.title = "The game was stopped from outside".into();
        d.detail = "Something ended the game without an error: usually the system running out of memory and stopping the biggest program. Close other programs, or give the game less memory if your PC has little.".into();
        d.can_bisect = false;
    } else if let Some(s) = &top {
        d.kind = CrashKind::ModError;
        d.title = format!("{} crashed the game", s.name);
        d.detail = format!(
            "The crash comes from {}'s code. Update it, or turn it off and start again. If it keeps crashing, its issue tracker will want the crash report.",
            s.name
        );
    } else {
        d.kind = CrashKind::Unknown;
        d.title = "The game crashed".into();
        d.detail = if d.can_bisect {
            "Nothing in the logs points at one mod. Alloy can find it by starting the game with half the mods turned off, again and again, until one is left.".into()
        } else if after_start {
            "Nothing in the logs points at one mod. It crashed after the title screen, so the crash report is the best lead: open it, or share it with whoever made the pack.".into()
        } else {
            "Nothing in the logs points at a cause. The crash report is the best lead: open it, or share it with whoever made the pack.".into()
        };
    }
    if let Some(s) = &top {
        if !matches!(d.kind, CrashKind::OutOfMemory | CrashKind::WrongJava | CrashKind::Killed) && files.iter().any(|f| f.path == s.path && f.enabled) {
            d.fixes.push(CrashFix::DisableMod { path: s.path.clone() });
        }
    }
    d.evidence = evidence;
    if d.evidence.is_empty() {
        for l in e.output.iter().rev().filter(|l| l.contains("ERROR") || l.contains("Exception") || l.contains("FATAL")).take(6) {
            d.evidence.insert(0, l.chars().take(400).collect());
        }
    }
    d
}

fn gb(mb: u32) -> String {
    let g = mb as f32 / 1024.0;
    if (g - g.round()).abs() < 0.05 {
        format!("{}", g.round() as u32)
    } else {
        format!("{g:.1}")
    }
}

fn is_graphics(all: &str, native: &str) -> bool {
    const DRIVERS: &[&str] = &[
        "atio6axx", "atioglxx", "amdxc64", "nvoglv64", "nvoglv32", "ig75icd64", "ig9icd64", "ig8icd64", "ig7icd64", "igxelpicd64", "libnvidia-glcore",
        "radeonsi_dri", "iris_dri", "i965_dri", "amdgpu_dri", "libGLX_nvidia", "OPENGL32.dll",
    ];
    let problematic = native.lines().skip_while(|l| !l.contains("Problematic frame")).take(3).collect::<Vec<_>>().join("\n");
    DRIVERS.iter().any(|d| problematic.contains(d))
        || all.contains("Pixel format not accelerated")
        || all.contains("GLFW error 65542")
        || all.contains("GLFW error 65543")
        || all.contains("WGL: The driver does not appear to support OpenGL")
        || all.contains("Failed to create window")
}

// ---------- the last diagnosis, kept per instance ----------

fn path(launcher: &Launcher, instance: &Instance) -> PathBuf {
    instance.dir(launcher).join(".launcher").join("crash.json")
}

pub async fn save(launcher: &Launcher, instance: &Instance, d: &Diagnosis) -> Result<()> {
    let p = path(launcher, instance);
    if let Some(dir) = p.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    write_json_atomic(&p, d).await
}

pub async fn last(launcher: &Launcher, instance: &Instance) -> Option<Diagnosis> {
    serde_json::from_slice(&tokio::fs::read(path(launcher, instance)).await.ok()?).ok()
}

pub async fn forget(launcher: &Launcher, instance: &Instance) {
    let _ = tokio::fs::remove_file(path(launcher, instance)).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::jarmeta::{JarMeta, ModEntry};

    fn jar(id: &str, name: &str, packages: &[&str]) -> Scanned {
        Scanned {
            path: format!("mods/{id}.jar"),
            file_name: format!("{id}.jar"),
            kind: ContentKind::Mod,
            enabled: true,
            size: 1,
            sha1: String::new(),
            murmur2: 0,
            jar: Some(JarMeta {
                mods: vec![ModEntry { id: id.into(), version: "1".into(), name: Some(name.into()), ..Default::default() }],
                packages: packages.iter().map(|s| s.to_string()).collect(),
                ..Default::default()
            }),
            index: None,
        }
    }

    fn pack() -> Vec<Scanned> {
        vec![
            jar("sodium", "Sodium", &["net.caffeinemc.mods.sodium"]),
            jar("lithium", "Lithium", &["net.caffeinemc.mods.lithium"]),
            jar("fabric-api", "Fabric API", &["net.fabricmc.fabric"]),
            jar("create", "Create", &["com.simibubi.create"]),
        ]
    }

    fn ev(report: &str) -> Evidence {
        Evidence { exit_code: Some(-1), crash_report: Some(("crash-reports/crash.txt".into(), report.into())), ..Default::default() }
    }

    #[test]
    fn stack_owner_is_named() {
        let report = "---- Minecraft Crash Report ----\nDescription: Ticking entity\n\njava.lang.NullPointerException: Cannot invoke \"x\" because \"y\" is null\n\tat com.simibubi.create.content.trains.Carriage.tick(Carriage.java:120)\n\tat net.minecraft.world.level.Level.tick(Level.java:1)\n\tat net.fabricmc.fabric.impl.Event.invoke(Event.java:3)\n";
        let d = diagnose(&ev(report), &pack(), 4096, 8192, 0);
        assert_eq!(d.kind, CrashKind::ModError);
        assert_eq!(d.suspects[0].name, "Create");
        assert_eq!(d.suspects[0].confidence, Confidence::Medium);
        assert_eq!(d.fixes, vec![CrashFix::DisableMod { path: "mods/create.jar".into() }]);
    }

    #[test]
    fn mixin_failure_names_mod() {
        let report = "java.lang.RuntimeException: Mixin transformation of net.minecraft.class_123 failed\nCaused by: org.spongepowered.asm.mixin.transformer.throwables.MixinTransformerError: An unexpected critical error was encountered\nCaused by: org.spongepowered.asm.mixin.injection.throwables.InjectionError: Critical injection failure: @Inject annotation on onRender could not find any targets matching 'render' in net.minecraft.class_757. Using refmap lithium-refmap.json [PREINJECT Applicator Phase -> lithium.mixins.json:render.MixinFoo from mod lithium -> Parse]\n\tat org.spongepowered.asm.mixin.injection.struct.InjectionInfo.validateTargets(InjectionInfo.java:1)\n";
        let d = diagnose(&ev(report), &pack(), 4096, 8192, 0);
        assert_eq!(d.kind, CrashKind::MixinFailed);
        assert_eq!(d.suspects[0].name, "Lithium");
        assert_eq!(d.suspects[0].confidence, Confidence::High);
    }

    #[test]
    fn mixin_handler_frames_count() {
        let report = "java.lang.IllegalStateException: bad\n\tat net.minecraft.class_757.handler$zbc000$sodium$onRender(class_757.java:1)\n\tat net.minecraft.class_757.method_3192(class_757.java:2)\n";
        let d = diagnose(&ev(report), &pack(), 4096, 8192, 0);
        assert_eq!(d.suspects[0].name, "Sodium");
    }

    #[test]
    fn out_of_memory_offers_more() {
        let mut e = ev("");
        e.crash_report = None;
        e.output = vec!["Exception in thread \"Render thread\" java.lang.OutOfMemoryError: Java heap space".into()];
        let d = diagnose(&e, &pack(), 4096, 12288, 0);
        assert_eq!(d.kind, CrashKind::OutOfMemory);
        assert_eq!(d.fixes, vec![CrashFix::RaiseMemory { mb: 6144 }]);
        assert!(!d.can_bisect);
    }

    #[test]
    fn wrong_java() {
        let mut e = ev("");
        e.output = vec!["java.lang.UnsupportedClassVersionError: com/simibubi/create/Create has been compiled by a more recent version of the Java Runtime (class file version 65.0), this version of the Java Runtime only recognizes class file versions up to 61.0".into()];
        let d = diagnose(&e, &pack(), 4096, 8192, 0);
        assert_eq!(d.kind, CrashKind::WrongJava);
        assert_eq!(d.fixes, vec![CrashFix::UseJava { major: 21 }]);
    }

    #[test]
    fn graphics_driver() {
        let mut e = ev("");
        e.crash_report = None;
        e.native = Some(("hs_err_pid1.log".into(), "# A fatal error has been detected by the Java Runtime Environment:\n#  EXCEPTION_ACCESS_VIOLATION\n# Problematic frame:\n# C  [atio6axx.dll+0x1b2c3]\n".into()));
        let d = diagnose(&e, &pack(), 4096, 8192, 0);
        assert_eq!(d.kind, CrashKind::GraphicsDriver);
    }

    #[test]
    fn nothing_known_suggests_bisect() {
        let mut e = ev("");
        e.crash_report = None;
        e.output = vec!["[main/INFO]: Loading Minecraft".into(), "Process crashed".into()];
        let d = diagnose(&e, &pack(), 4096, 8192, 0);
        assert_eq!(d.kind, CrashKind::Unknown);
        assert!(d.can_bisect);
        assert!(d.suspects.is_empty());
    }

    #[test]
    fn after_title_screen_no_bisect() {
        let mut e = ev("java.lang.NullPointerException: x\n\tat net.minecraft.client.Foo.bar(Foo.java:1)\n");
        e.output = vec!["[Render thread/INFO]: Sound engine started".into()];
        let d = diagnose(&e, &pack(), 4096, 8192, 0);
        assert!(d.after_start);
        assert!(!d.can_bisect);
    }

    #[test]
    fn fabric_entrypoint_crash() {
        let mut files = pack();
        files.push(jar("crashy", "Crashy", &["dev.elchi.crashy"]));
        let report = "Description: Initializing game\n\njava.lang.RuntimeException: Could not execute entrypoint stage 'client' due to errors, provided by 'crashy' at 'dev.elchi.crashy.Crashy'!\n\tat net.fabricmc.loader.impl.FabricLoaderImpl.lambda$invokeEntrypoints$0(FabricLoaderImpl.java:418)\n\tat knot//net.minecraft.class_310.<init>(class_310.java:477)\nCaused by: java.lang.IllegalStateException: broken\n\tat knot//dev.elchi.crashy.Settings.load(Crashy.java:14)\n\tat knot//dev.elchi.crashy.Crashy.onInitializeClient(Crashy.java:8)\n\t... 6 more\n\n-- Head --\nStacktrace:\n\tat net.fabricmc.loader.impl.FabricLoaderImpl.lambda$invokeEntrypoints$0(FabricLoaderImpl.java:418)\n";
        let d = diagnose(&ev(report), &files, 4096, 8192, 0);
        assert_eq!(d.suspects.len(), 1, "{:?}", d.suspects);
        assert_eq!(d.suspects[0].name, "Crashy");
        assert_eq!(d.suspects[0].confidence, Confidence::High);
        assert!(d.evidence.iter().any(|l| l.contains("dev.elchi.crashy.Settings.load")), "{:?}", d.evidence);
        let t = traces(report);
        assert_eq!(t.len(), 2);
        assert_eq!(t[1].frames.len(), 2, "the Head section's frames aren't the cause's");
    }

    #[test]
    fn forge_mod_file() {
        let report = "-- Mod loading issue for: create --\nDetails:\n\tMod file: /home/x/.minecraft/mods/create.jar\n\tFailure message: Create (create) has failed to load correctly\n\t\tjava.lang.NullPointerException: oops\n";
        let d = diagnose(&ev(report), &pack(), 4096, 8192, 0);
        assert_eq!(d.suspects[0].name, "Create");
        assert_eq!(d.suspects[0].confidence, Confidence::High);
    }
}
