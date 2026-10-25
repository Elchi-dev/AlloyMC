//! The per-version JSON format shared by Mojang, Fabric, Quilt and (with extras) Forge.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::rules::{rules_allow, Features, Rule};
use crate::util::maven_path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionJson {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inherits_from: Option<String>,
    #[serde(default)]
    pub main_class: String,
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Arguments>,
    /// Pre-1.13 versions put all game arguments in one string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minecraft_arguments: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_index: Option<AssetIndexRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assets: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downloads: Option<VersionDownloads>,
    #[serde(default)]
    pub libraries: Vec<Library>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java_version: Option<JavaVersion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logging: Option<Logging>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_time: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<Argument>,
    #[serde(default)]
    pub jvm: Vec<Argument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Argument {
    Plain(String),
    Conditional { rules: Vec<Rule>, value: ArgValue },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ArgValue {
    One(String),
    Many(Vec<String>),
}

impl Argument {
    /// The argument's strings if its rules allow it on this machine.
    pub fn resolve(&self, features: &Features) -> Vec<String> {
        match self {
            Argument::Plain(s) => vec![s.clone()],
            Argument::Conditional { rules, value } => {
                if !rules_allow(Some(rules), features) {
                    return vec![];
                }
                match value {
                    ArgValue::One(s) => vec![s.clone()],
                    ArgValue::Many(v) => v.clone(),
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetIndexRef {
    pub id: String,
    pub sha1: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub total_size: u64,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionDownloads {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<Artifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default)]
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersion {
    pub component: String,
    pub major_version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Logging {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<LoggingConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub argument: String,
    pub file: LoggingFile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingFile {
    pub id: String,
    pub sha1: String,
    #[serde(default)]
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Library {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downloads: Option<LibraryDownloads>,
    /// Maven repository base used by Fabric/Quilt profiles instead of `downloads`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub natives: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules: Option<Vec<Rule>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extract: Option<Extract>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryDownloads {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<Artifact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classifiers: Option<HashMap<String, Artifact>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Extract {
    #[serde(default)]
    pub exclude: Vec<String>,
}

/// A resolved file a library needs on disk.
#[derive(Debug, Clone)]
pub struct LibraryFile {
    /// Path relative to the libraries folder.
    pub path: String,
    pub url: String,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    /// Legacy natives jar that must be extracted before launch.
    pub native: bool,
    pub exclude: Vec<String>,
}

impl Library {
    /// `group:artifact` (+ classifier) without the version, used to let a loader override vanilla libs.
    pub fn key(&self) -> String {
        let parts: Vec<&str> = self.name.split('@').next().unwrap_or("").split(':').collect();
        match parts.as_slice() {
            [g, a, _v, c, ..] => format!("{g}:{a}:{c}"),
            [g, a, ..] => format!("{g}:{a}"),
            _ => self.name.clone(),
        }
    }

    /// The files this library contributes on this OS, or none if its rules exclude it.
    pub fn files(&self, features: &Features) -> Vec<LibraryFile> {
        if !rules_allow(self.rules.as_deref(), features) {
            return vec![];
        }
        let mut out = vec![];
        let exclude = self.extract.as_ref().map(|e| e.exclude.clone()).unwrap_or_default();

        // Main artifact
        if let Some(art) = self.downloads.as_ref().and_then(|d| d.artifact.as_ref()) {
            if let Some(path) = art.path.clone().or_else(|| maven_path(&self.name)) {
                out.push(LibraryFile {
                    path,
                    url: art.url.clone(),
                    sha1: art.sha1.clone(),
                    size: art.size,
                    native: false,
                    exclude: vec![],
                });
            }
        } else if self.natives.is_none() {
            // Fabric-style: name + repository URL.
            if let Some(path) = maven_path(&self.name) {
                let base = self
                    .url
                    .clone()
                    .unwrap_or_else(|| "https://libraries.minecraft.net/".into());
                let base = if base.ends_with('/') { base } else { format!("{base}/") };
                out.push(LibraryFile {
                    url: format!("{base}{path}"),
                    path,
                    sha1: self.sha1.clone(),
                    size: self.size,
                    native: false,
                    exclude: vec![],
                });
            }
        }

        // Legacy natives (pre-1.19): a classifier per OS, extracted at launch.
        if let Some(natives) = &self.natives {
            if let Some(classifier) = natives.get(super::rules::os_name()) {
                let bits = if cfg!(target_pointer_width = "64") { "64" } else { "32" };
                let classifier = classifier.replace("${arch}", bits);
                if let Some(art) = self
                    .downloads
                    .as_ref()
                    .and_then(|d| d.classifiers.as_ref())
                    .and_then(|c| c.get(&classifier))
                {
                    let path = art
                        .path
                        .clone()
                        .or_else(|| maven_path(&format!("{}:{classifier}", self.name)))
                        .unwrap_or_default();
                    out.push(LibraryFile {
                        path,
                        url: art.url.clone(),
                        sha1: art.sha1.clone(),
                        size: art.size,
                        native: true,
                        exclude,
                    });
                }
            }
        }
        out
    }
}

impl VersionJson {
    /// Merges a loader profile (`self`) onto the vanilla version it inherits from.
    /// Loader libraries win over vanilla ones with the same `group:artifact`.
    pub fn merge_onto(self, parent: VersionJson) -> VersionJson {
        let child_keys: HashSet<String> = self.libraries.iter().map(|l| l.key()).collect();
        let mut libraries = self.libraries;
        libraries.extend(parent.libraries.into_iter().filter(|l| !child_keys.contains(&l.key())));

        let arguments = match (parent.arguments, self.arguments) {
            (Some(mut p), Some(c)) => {
                p.game.extend(c.game);
                p.jvm.extend(c.jvm);
                Some(p)
            }
            (p, c) => c.or(p),
        };

        VersionJson {
            id: self.id,
            inherits_from: None,
            main_class: if self.main_class.is_empty() { parent.main_class } else { self.main_class },
            kind: parent.kind,
            arguments,
            minecraft_arguments: self.minecraft_arguments.or(parent.minecraft_arguments),
            asset_index: self.asset_index.or(parent.asset_index),
            assets: self.assets.or(parent.assets),
            downloads: parent.downloads,
            libraries,
            java_version: self.java_version.or(parent.java_version),
            logging: self.logging.or(parent.logging),
            release_time: parent.release_time,
        }
    }

    /// Java major version this game version wants (Mojang defaults to 8 when unspecified).
    pub fn java_major(&self) -> u32 {
        self.java_version.as_ref().map(|j| j.major_version).unwrap_or(8)
    }

    pub fn java_component(&self) -> String {
        self.java_version
            .as_ref()
            .map(|j| j.component.clone())
            .unwrap_or_else(|| "jre-legacy".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modern_arguments() {
        let json = r#"{
            "id": "x", "mainClass": "a.B", "type": "release",
            "arguments": { "game": ["--username", "${auth_player_name}",
                {"rules":[{"action":"allow","features":{"has_custom_resolution":true}}],
                 "value":["--width","${resolution_width}"]}],
              "jvm": [{"rules":[{"action":"allow","os":{"name":"osx"}}],"value":"-XstartOnFirstThread"}] },
            "libraries": []
        }"#;
        let v: VersionJson = serde_json::from_str(json).unwrap();
        let args = v.arguments.unwrap();
        assert_eq!(args.game.len(), 3);
        let f = Features::default();
        assert!(args.game[2].resolve(&f).is_empty());
    }

    #[test]
    fn loader_libraries_override_vanilla() {
        let lib = |name: &str| Library {
            name: name.into(),
            downloads: None,
            url: None,
            sha1: None,
            size: None,
            natives: None,
            rules: None,
            extract: None,
        };
        let parent: VersionJson = serde_json::from_str(r#"{"id":"1.21","mainClass":"net.minecraft.client.main.Main","type":"release"}"#).unwrap();
        let parent = VersionJson { libraries: vec![lib("org.ow2.asm:asm:9.3"), lib("com.google:gson:2.10")], ..parent };
        let child: VersionJson = serde_json::from_str(r#"{"id":"fabric","inheritsFrom":"1.21","mainClass":"net.fabricmc.loader.impl.launch.knot.KnotClient"}"#).unwrap();
        let child = VersionJson { libraries: vec![lib("org.ow2.asm:asm:9.7")], ..child };
        let merged = child.merge_onto(parent);
        let names: Vec<_> = merged.libraries.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, vec!["org.ow2.asm:asm:9.7", "com.google:gson:2.10"]);
        assert_eq!(merged.main_class, "net.fabricmc.loader.impl.launch.knot.KnotClient");
        assert_eq!(merged.kind, "release");
    }
}
