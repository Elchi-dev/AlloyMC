//! Mojang's library/argument rules: allow or disallow by OS, arch and launcher features.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub action: RuleAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<OsRule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub features: Option<HashMap<String, bool>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RuleAction {
    Allow,
    Disallow,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsRule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Feature flags the launcher reports when evaluating rules.
#[derive(Debug, Clone, Default)]
pub struct Features {
    pub is_demo_user: bool,
    pub has_custom_resolution: bool,
    pub has_quick_plays_support: bool,
    pub is_quick_play_singleplayer: bool,
    pub is_quick_play_multiplayer: bool,
    pub is_quick_play_realms: bool,
}

impl Features {
    fn get(&self, name: &str) -> bool {
        match name {
            "is_demo_user" => self.is_demo_user,
            "has_custom_resolution" => self.has_custom_resolution,
            "has_quick_plays_support" => self.has_quick_plays_support,
            "is_quick_play_singleplayer" => self.is_quick_play_singleplayer,
            "is_quick_play_multiplayer" => self.is_quick_play_multiplayer,
            "is_quick_play_realms" => self.is_quick_play_realms,
            _ => false,
        }
    }
}

/// Mojang's OS names: `windows`, `osx`, `linux`.
pub fn os_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    }
}

pub fn os_arch() -> &'static str {
    if cfg!(target_arch = "x86") {
        "x86"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x86_64"
    }
}

/// Rules are evaluated in order; the last matching rule wins. No rules = allowed.
pub fn rules_allow(rules: Option<&[Rule]>, features: &Features) -> bool {
    let Some(rules) = rules else { return true };
    if rules.is_empty() {
        return true;
    }
    let mut allowed = false;
    for rule in rules {
        if rule_matches(rule, features) {
            allowed = rule.action == RuleAction::Allow;
        }
    }
    allowed
}

fn rule_matches(rule: &Rule, features: &Features) -> bool {
    if let Some(os) = &rule.os {
        if let Some(name) = &os.name {
            if name != os_name() {
                return false;
            }
        }
        if let Some(arch) = &os.arch {
            // Mojang only ever uses "x86" here, meaning 32-bit.
            if arch == "x86" && os_arch() != "x86" {
                return false;
            }
            if arch != "x86" && arch != os_arch() {
                return false;
            }
        }
    }
    if let Some(wanted) = &rule.features {
        if wanted.iter().any(|(k, v)| features.get(k) != *v) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(action: RuleAction, os: Option<&str>) -> Rule {
        Rule { action, os: os.map(|n| OsRule { name: Some(n.into()), arch: None, version: None }), features: None }
    }

    #[test]
    fn last_match_wins() {
        let rules = vec![rule(RuleAction::Allow, None), rule(RuleAction::Disallow, Some(os_name()))];
        assert!(!rules_allow(Some(&rules), &Features::default()));
        let rules = vec![rule(RuleAction::Allow, Some("plan9"))];
        assert!(!rules_allow(Some(&rules), &Features::default()));
        assert!(rules_allow(None, &Features::default()));
    }

    #[test]
    fn features() {
        let mut f = HashMap::new();
        f.insert("has_custom_resolution".to_string(), true);
        let rules = vec![Rule { action: RuleAction::Allow, os: None, features: Some(f) }];
        assert!(!rules_allow(Some(&rules), &Features::default()));
        let on = Features { has_custom_resolution: true, ..Default::default() };
        assert!(rules_allow(Some(&rules), &on));
    }
}
