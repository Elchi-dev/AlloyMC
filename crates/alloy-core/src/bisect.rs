//! Finds the mod behind a startup crash by starting the game with half of the suspects
//! turned off, then half of what's left, until one mod is left; then starts the game with
//! only that mod (and what it needs) to confirm. About log2(mods) + 1 starts.
//!
//! Every mod a test turns on brings the mods it requires, so no test crashes for a missing
//! dependency. Which mods were on is written to `.launcher/bisect.json` before anything
//! changes, and put back afterwards, even if Alloy is closed halfway (see `recover`).

use std::collections::{BTreeSet, HashMap};
use std::future::Future;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::content::jarmeta::DepKind;
use crate::content::scan::{display_name, Scanned};
use crate::content::{self, ContentKind};
use crate::instance::{self, Instance};
use crate::settings::write_json_atomic;
use crate::{Launcher, Result};

/// How one test start ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Crashed,
    /// Reached the title screen.
    Started,
}

/// Where a bisect is, for the progress line.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    /// 1-based.
    pub trial: u32,
    /// Starts it will take in all, give or take one.
    pub estimate: u32,
    /// Mods on in this start.
    pub enabled: usize,
    /// Mods it could still be.
    pub suspects: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    /// Paths of the mods that together make it crash: one, usually. Empty when the game
    /// started fine with everything on (the crash didn't happen again).
    pub culprits: Vec<String>,
    pub names: Vec<String>,
    /// One mod, crashing with only itself and its requirements on.
    pub alone: bool,
    pub trials: u32,
}

/// The mods currently on, with what each requires (as paths of other mods that are on).
pub struct Graph {
    pub mods: Vec<String>,
    names: HashMap<String, String>,
    requires: HashMap<String, BTreeSet<String>>,
}

impl Graph {
    pub fn new(files: &[Scanned]) -> Graph {
        let on: Vec<&Scanned> = files.iter().filter(|f| f.kind == ContentKind::Mod && f.enabled).collect();
        let mut provider: HashMap<String, String> = HashMap::new();
        let mut by_project: HashMap<String, String> = HashMap::new();
        for f in &on {
            if let Some(j) = &f.jar {
                for (id, _) in j.provided() {
                    let own = j.mods.iter().any(|m| m.id == id);
                    if own || !provider.contains_key(id) {
                        provider.insert(id.to_string(), f.path.clone());
                    }
                }
            }
            if let Some(p) = f.index.as_ref().and_then(|m| m.project_id.clone()) {
                by_project.insert(p, f.path.clone());
            }
        }
        let mut requires: HashMap<String, BTreeSet<String>> = HashMap::new();
        for f in &on {
            let mut need = BTreeSet::new();
            if let Some(j) = &f.jar {
                for m in &j.mods {
                    for d in m.deps.iter().filter(|d| d.kind == DepKind::Required && !d.server_only) {
                        if let Some(p) = provider.get(&d.id).filter(|p| **p != f.path) {
                            need.insert(p.clone());
                        }
                    }
                }
            }
            for p in f.index.as_ref().and_then(|m| m.requires.clone()).unwrap_or_default() {
                if let Some(path) = by_project.get(&p).filter(|p| **p != f.path) {
                    need.insert(path.clone());
                }
            }
            requires.insert(f.path.clone(), need);
        }
        let mut mods: Vec<String> = on.iter().map(|f| f.path.clone()).collect();
        mods.sort_by_key(|p| p.to_lowercase());
        let names = on.iter().map(|f| (f.path.clone(), display_name(f))).collect();
        Graph { mods, names, requires }
    }

    /// These mods plus everything they need, transitively.
    pub fn closure<'a>(&self, start: impl IntoIterator<Item = &'a String>) -> BTreeSet<String> {
        let mut out: BTreeSet<String> = BTreeSet::new();
        let mut todo: Vec<String> = start.into_iter().cloned().collect();
        while let Some(p) = todo.pop() {
            if out.insert(p.clone()) {
                todo.extend(self.requires.get(&p).into_iter().flatten().cloned());
            }
        }
        out
    }

    pub fn name(&self, path: &str) -> String {
        self.names.get(path).cloned().unwrap_or_else(|| path.rsplit('/').next().unwrap_or(path).to_string())
    }
}

fn estimate(n: usize) -> u32 {
    (usize::BITS - n.saturating_sub(1).leading_zeros()) + 1
}

/// Runs tests through `trial`, remembering answers so no set of mods is started twice.
struct Tester<'g, F, S> {
    graph: &'g Graph,
    trial: F,
    on_step: S,
    seen: HashMap<BTreeSet<String>, Outcome>,
    trials: u32,
    estimate: u32,
}

impl<F, Fut, S> Tester<'_, F, S>
where
    F: FnMut(BTreeSet<String>) -> Fut + Send,
    Fut: Future<Output = Result<Outcome>> + Send,
    S: Fn(&Step) + Send,
{
    async fn crashes(&mut self, on: BTreeSet<String>, suspects: usize) -> Result<bool> {
        if let Some(o) = self.seen.get(&on) {
            return Ok(*o == Outcome::Crashed);
        }
        self.trials += 1;
        self.estimate = self.estimate.max(self.trials);
        (self.on_step)(&Step { trial: self.trials, estimate: self.estimate, enabled: on.len(), suspects });
        let o = (self.trial)(on.clone()).await?;
        self.seen.insert(on, o);
        Ok(o == Outcome::Crashed)
    }

    /// The smallest part of `cands` that, on with `required`, still crashes, assuming
    /// `required` + all of `cands` does. Handles crashes that need several mods together,
    /// at the cost of more starts than `fast`.
    fn needed<'s>(&'s mut self, required: BTreeSet<String>, cands: Vec<String>) -> std::pin::Pin<Box<dyn Future<Output = Result<Vec<String>>> + Send + 's>> {
        Box::pin(async move {
            if cands.len() <= 1 {
                return Ok(cands);
            }
            let (a, b) = cands.split_at(cands.len() / 2);
            let (a, b) = (a.to_vec(), b.to_vec());
            let with = |part: &[String]| self.graph.closure(required.iter().chain(part.iter()));
            let (on_a, on_b) = (with(&a), with(&b));
            let all: BTreeSet<String> = self.graph.closure(required.iter().chain(cands.iter()));
            // Requirements tie the halves together: nothing to split.
            if on_a == all && on_b == all {
                return Ok(cands);
            }
            if on_a != all && self.crashes(on_a.clone(), cands.len()).await? {
                let a: Vec<String> = cands.iter().filter(|c| on_a.contains(*c)).cloned().collect();
                return self.needed(required, a).await;
            }
            if on_b != all && self.crashes(on_b.clone(), cands.len()).await? {
                let b: Vec<String> = cands.iter().filter(|c| on_b.contains(*c)).cloned().collect();
                return self.needed(required, b).await;
            }
            // Something in each half is needed.
            let mut req_b = required.clone();
            req_b.extend(b.iter().cloned());
            let from_a = self.needed(req_b, a).await?;
            let mut req_a = required.clone();
            req_a.extend(from_a.iter().cloned());
            let from_b = self.needed(req_a, b).await?;
            Ok(from_a.into_iter().chain(from_b).collect())
        })
    }
}

/// Runs the search. `trial` starts the game with exactly these mods on and says how it went.
///
/// First the fast way, which assumes one mod is at fault and halves the suspects with each
/// start. If the mod it lands on doesn't crash on its own, the crash needs mods together, and
/// a slower search finds the smallest set that does.
pub async fn run<F, Fut>(graph: &Graph, trial: F, on_step: impl Fn(&Step) + Send) -> Result<Found>
where
    F: FnMut(BTreeSet<String>) -> Fut + Send,
    Fut: Future<Output = Result<Outcome>> + Send,
{
    let mut t = Tester { graph, trial, on_step, seen: HashMap::new(), trials: 0, estimate: estimate(graph.mods.len()) };
    let mut suspects: Vec<String> = graph.mods.clone();
    while suspects.len() > 1 {
        let mid = suspects.len() / 2;
        let (a, b) = suspects.split_at(mid);
        // A half whose requirements pull in every suspect can't split anything; try the other.
        let pick = [a, b].into_iter().map(|h| graph.closure(h)).find(|on| !suspects.iter().all(|s| on.contains(s)));
        let Some(on) = pick else { break };
        let n = suspects.len();
        if t.crashes(on.clone(), n).await? {
            suspects.retain(|s| on.contains(s));
        } else {
            suspects.retain(|s| !on.contains(s));
        }
    }
    if suspects.len() == 1 && t.crashes(graph.closure([&suspects[0]]), 1).await? {
        let c = suspects.remove(0);
        return Ok(Found { names: vec![graph.name(&c)], culprits: vec![c], alone: true, trials: t.trials });
    }

    // Not one mod alone. Make sure everything on still crashes, then find what's needed.
    let all = graph.closure(graph.mods.iter());
    if !t.crashes(all, graph.mods.len()).await? {
        return Ok(Found { culprits: vec![], names: vec![], alone: false, trials: t.trials });
    }
    t.estimate = t.trials + 2 * estimate(graph.mods.len());
    let needed = t.needed(BTreeSet::new(), graph.mods.clone()).await?;
    Ok(Found { names: needed.iter().map(|c| graph.name(c)).collect(), alone: needed.len() == 1, culprits: needed, trials: t.trials })
}

// ---------- turning mods on and off, and back ----------

#[derive(Debug, Serialize, Deserialize)]
struct Saved {
    /// Every mod that was on before the bisect.
    on: Vec<String>,
}

fn saved_path(launcher: &Launcher, instance: &Instance) -> PathBuf {
    instance.dir(launcher).join(".launcher").join("bisect.json")
}

/// Remembers which mods are on; call before the first `apply`.
pub async fn begin(launcher: &Launcher, instance: &Instance, graph: &Graph) -> Result<()> {
    let p = saved_path(launcher, instance);
    if p.exists() {
        // An earlier bisect never finished: its record is the truth.
        restore(launcher, instance).await?;
    }
    if let Some(dir) = p.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    write_json_atomic(&p, &Saved { on: graph.mods.clone() }).await
}

/// Turns exactly `on` on among the mods the bisect covers.
pub async fn apply(launcher: &Launcher, instance: &Instance, graph: &Graph, on: &BTreeSet<String>) -> Result<()> {
    for m in &graph.mods {
        content::set_enabled(launcher, instance, m, on.contains(m)).await?;
    }
    Ok(())
}

/// Puts every mod back the way it was before the bisect, and forgets the record.
pub async fn restore(launcher: &Launcher, instance: &Instance) -> Result<()> {
    let p = saved_path(launcher, instance);
    let Ok(bytes) = tokio::fs::read(&p).await else { return Ok(()) };
    if let Ok(saved) = serde_json::from_slice::<Saved>(&bytes) {
        for m in &saved.on {
            // A file removed meanwhile is fine to skip.
            let _ = content::set_enabled(launcher, instance, m, true).await;
        }
    }
    tokio::fs::remove_file(&p).await?;
    Ok(())
}

/// The same, without async, for when a bisect is dropped halfway (Alloy cancelling the job).
pub fn restore_blocking(dir: &std::path::Path) {
    let p = dir.join(".launcher").join("bisect.json");
    let Ok(bytes) = std::fs::read(&p) else { return };
    if let Ok(saved) = serde_json::from_slice::<Saved>(&bytes) {
        for m in &saved.on {
            let Ok(base) = crate::util::safe_join(dir, m) else { continue };
            let mut off = base.clone().into_os_string();
            off.push(".disabled");
            if !base.exists() {
                let _ = std::fs::rename(&off, &base);
            }
        }
    }
    let _ = std::fs::remove_file(&p);
}

/// Puts back mods left off by a bisect that Alloy was closed during. Run at startup.
pub async fn recover(launcher: &Launcher) {
    let Ok(list) = instance::list(launcher).await else { return };
    for i in list {
        if saved_path(launcher, &i).exists() {
            tracing::info!("restoring mods after an unfinished bisect in {}", i.name);
            let _ = restore(launcher, &i).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::jarmeta::{Dep, JarMeta, ModEntry};
    use std::sync::Mutex;

    fn m(id: &str, deps: &[&str]) -> Scanned {
        Scanned {
            path: format!("mods/{id}.jar"),
            file_name: format!("{id}.jar"),
            kind: ContentKind::Mod,
            enabled: true,
            size: 1,
            sha1: String::new(),
            murmur2: 0,
            jar: Some(JarMeta {
                mods: vec![ModEntry {
                    id: id.into(),
                    version: "1".into(),
                    deps: deps.iter().map(|d| Dep { id: d.to_string(), kind: DepKind::Required, range: "*".into(), maven: false, server_only: false }).collect(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            index: None,
        }
    }

    fn block_on<F: Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(f)
    }

    fn find(files: &[Scanned], crashes: impl Fn(&BTreeSet<String>) -> bool + Sync) -> (Found, Vec<BTreeSet<String>>) {
        let g = Graph::new(files);
        let tried = Mutex::new(vec![]);
        let found = block_on(run(
            &g,
            |on| {
                tried.lock().unwrap().push(on.clone());
                let r = if crashes(&on) { Outcome::Crashed } else { Outcome::Started };
                async move { Ok(r) }
            },
            |_| {},
        ))
        .unwrap();
        (found, tried.into_inner().unwrap())
    }

    fn many(n: usize) -> Vec<Scanned> {
        (0..n).map(|i| m(&format!("mod{i:03}"), &[])).collect()
    }

    #[test]
    fn finds_one_bad_mod_in_log_steps() {
        let files = many(100);
        let (f, tried) = find(&files, |on| on.contains("mods/mod037.jar"));
        assert_eq!(f.culprits, vec!["mods/mod037.jar"]);
        assert!(f.alone);
        assert!(tried.len() <= 8, "{} starts", tried.len());
    }

    #[test]
    fn requirements_come_along() {
        let mut files = many(10);
        files.push(m("lib", &[]));
        files.push(m("needslib", &["lib"]));
        // needslib crashes; any start with needslib must have lib too.
        let (f, tried) = find(&files, |on| on.contains("mods/needslib.jar"));
        assert_eq!(f.culprits, vec!["mods/needslib.jar"]);
        for on in &tried {
            if on.contains("mods/needslib.jar") {
                assert!(on.contains("mods/lib.jar"));
            }
        }
    }

    #[test]
    fn a_crashing_library_is_found() {
        let mut files = many(6);
        files.push(m("lib", &[]));
        files.push(m("a", &["lib"]));
        files.push(m("b", &["lib"]));
        let (f, _) = find(&files, |on| on.contains("mods/lib.jar"));
        assert_eq!(f.culprits, vec!["mods/lib.jar"]);
        assert!(f.alone);
    }

    #[test]
    fn two_mods_together() {
        let files = many(16);
        let (f, tried) = find(&files, |on| on.contains("mods/mod002.jar") && on.contains("mods/mod013.jar"));
        let mut got = f.culprits.clone();
        got.sort();
        assert_eq!(got, vec!["mods/mod002.jar", "mods/mod013.jar"]);
        assert!(!f.alone);
        assert!(tried.len() <= 20, "{} starts", tried.len());
        let unique: std::collections::HashSet<_> = tried.iter().collect();
        assert_eq!(unique.len(), tried.len(), "a set was started twice");
    }

    #[test]
    fn no_crash_any_more() {
        let files = many(8);
        let (f, _) = find(&files, |_| false);
        assert!(f.culprits.is_empty());
    }

    #[test]
    fn closure_is_transitive() {
        let files = vec![m("a", &["b"]), m("b", &["c"]), m("c", &[]), m("d", &[])];
        let g = Graph::new(&files);
        let c = g.closure([&"mods/a.jar".to_string()]);
        assert_eq!(c.into_iter().collect::<Vec<_>>(), vec!["mods/a.jar", "mods/b.jar", "mods/c.jar"]);
    }

    #[test]
    fn restore_puts_mods_back() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        std::fs::create_dir_all(d.join("mods")).unwrap();
        std::fs::create_dir_all(d.join(".launcher")).unwrap();
        std::fs::write(d.join("mods/a.jar.disabled"), b"").unwrap();
        std::fs::write(d.join("mods/b.jar"), b"").unwrap();
        std::fs::write(d.join(".launcher/bisect.json"), br#"{"on":["mods/a.jar","mods/b.jar"]}"#).unwrap();
        restore_blocking(d);
        assert!(d.join("mods/a.jar").exists());
        assert!(d.join("mods/b.jar").exists());
        assert!(!d.join(".launcher/bisect.json").exists());
    }
}
