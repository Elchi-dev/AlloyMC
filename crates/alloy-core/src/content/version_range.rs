//! Version comparison and the two range syntaxes mods use:
//!
//! - Fabric and Quilt: `*`, `>=1.2`, `<2`, `=1.2`, `1.20.x`, `~1.2.3`, `^1.2`, several
//!   predicates in one string (all must hold) and arrays (any may hold).
//! - Forge and NeoForge: Maven ranges such as `[47,)`, `[1.20.1,1.21)`, `(,2.0]`, `[1.0]`,
//!   or several ranges joined by commas.
//!
//! Mod versions are messy (`mc1.20.1-0.5.3+build.12`), so comparison is lenient: numbers
//! compare as numbers, words as words, and a trailing `-beta`/`-rc`/`-pre`/`-alpha`/
//! `-snapshot` sorts before the release. When a range can't be read, it counts as satisfied:
//! the compatibility check must never invent a problem.

use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Chunk {
    Num(u64),
    Word(String),
}

fn chunks(v: &str) -> Vec<Chunk> {
    let v = v.trim().trim_start_matches(['v', 'V']);
    // Build metadata (`+build.5`) never affects ordering.
    let v = v.split('+').next().unwrap_or(v);
    let mut out = vec![];
    let mut cur = String::new();
    let mut digits = false;
    let flush = |cur: &mut String, digits: bool, out: &mut Vec<Chunk>| {
        if cur.is_empty() {
            return;
        }
        out.push(if digits { Chunk::Num(cur.parse().unwrap_or(u64::MAX)) } else { Chunk::Word(cur.to_lowercase()) });
        cur.clear();
    };
    for c in v.chars() {
        if c.is_ascii_digit() {
            if !digits {
                flush(&mut cur, digits, &mut out);
                digits = true;
            }
            cur.push(c);
        } else if c.is_alphanumeric() {
            if digits {
                flush(&mut cur, digits, &mut out);
                digits = false;
            }
            cur.push(c);
        } else {
            flush(&mut cur, digits, &mut out);
            digits = false;
            if c == '-' {
                out.push(Chunk::Word("-".into()));
            }
        }
    }
    flush(&mut cur, digits, &mut out);
    out
}

fn is_prerelease_word(w: &str) -> bool {
    ["alpha", "beta", "pre", "rc", "snapshot", "dev", "a", "b"].iter().any(|p| w.starts_with(p))
}

/// Total order over version strings, lenient enough for mod versions.
pub fn compare(a: &str, b: &str) -> Ordering {
    let (a, b) = (chunks(a), chunks(b));
    let mut i = 0;
    loop {
        match (a.get(i), b.get(i)) {
            (None, None) => return Ordering::Equal,
            (Some(x), None) => return tail_order(&a[i..], x),
            (None, Some(y)) => return tail_order(&b[i..], y).reverse(),
            (Some(Chunk::Num(x)), Some(Chunk::Num(y))) if x != y => return x.cmp(y),
            (Some(Chunk::Word(x)), Some(Chunk::Word(y))) if x != y => {
                // "-" then a prerelease word sorts below the release, like 1.0-beta < 1.0.1.
                return match (is_prerelease_word(x), is_prerelease_word(y)) {
                    (true, false) => Ordering::Less,
                    (false, true) => Ordering::Greater,
                    _ => x.cmp(y),
                };
            }
            (Some(Chunk::Num(_)), Some(Chunk::Word(w))) => {
                return if w == "-" || is_prerelease_word(w) { Ordering::Greater } else { Ordering::Less };
            }
            (Some(Chunk::Word(w)), Some(Chunk::Num(_))) => {
                return if w == "-" || is_prerelease_word(w) { Ordering::Less } else { Ordering::Greater };
            }
            _ => i += 1,
        }
    }
}

/// How the longer version's extra part compares to "nothing": `1.0-beta` < `1.0` < `1.0.1`.
fn tail_order(rest: &[Chunk], first: &Chunk) -> Ordering {
    match first {
        Chunk::Num(0) if rest.iter().all(|c| *c == Chunk::Num(0)) => Ordering::Equal,
        Chunk::Num(_) => Ordering::Greater,
        Chunk::Word(w) if w == "-" => match rest.get(1) {
            Some(Chunk::Word(w)) if is_prerelease_word(w) => Ordering::Less,
            Some(_) => Ordering::Greater,
            None => Ordering::Equal,
        },
        Chunk::Word(w) if is_prerelease_word(w) => Ordering::Less,
        Chunk::Word(_) => Ordering::Greater,
    }
}

/// A requirement on a version, in whichever syntax the mod used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Range {
    Any,
    /// Fabric/Quilt: any of these groups; each group's predicates must all hold.
    Fabric(Vec<Vec<Pred>>),
    /// Maven: any of these intervals.
    Maven(Vec<Interval>),
    /// Couldn't read it; treated as satisfied.
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pred {
    Ge(String),
    Gt(String),
    Le(String),
    Lt(String),
    Eq(String),
    /// `1.20.x`: same leading parts.
    Prefix(String),
    /// `~1.2.3`: >= 1.2.3 and < 1.3.
    Tilde(String),
    /// `^1.2.3`: >= 1.2.3 and < 2.
    Caret(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interval {
    pub min: Option<(String, bool)>,
    pub max: Option<(String, bool)>,
}

impl Range {
    pub fn fabric_any_of<'a>(items: impl IntoIterator<Item = &'a str>) -> Range {
        let mut groups = vec![];
        for item in items {
            match parse_fabric_group(item) {
                Some(g) if g.is_empty() => return Range::Any,
                Some(g) => groups.push(g),
                None => return Range::Unknown(item.to_string()),
            }
        }
        if groups.is_empty() {
            Range::Any
        } else {
            Range::Fabric(groups)
        }
    }

    pub fn fabric(s: &str) -> Range {
        Range::fabric_any_of([s])
    }

    pub fn maven(s: &str) -> Range {
        let s = s.trim();
        if s.is_empty() || s == "*" {
            return Range::Any;
        }
        if !s.starts_with(['[', '(']) {
            // A bare version is Maven's "soft" requirement: anything goes.
            return Range::Any;
        }
        let mut intervals = vec![];
        let mut rest = s;
        while !rest.is_empty() {
            rest = rest.trim_start_matches([',', ' ']);
            if rest.is_empty() {
                break;
            }
            let open = rest.chars().next().unwrap();
            let Some(end) = rest.find([']', ')']) else {
                return Range::Unknown(s.into());
            };
            let close = rest.as_bytes()[end] as char;
            let inner = &rest[1..end];
            rest = &rest[end + 1..];
            let (lo, hi) = match inner.split_once(',') {
                Some((lo, hi)) => (lo.trim(), hi.trim()),
                None => (inner.trim(), inner.trim()),
            };
            if !matches!(open, '[' | '(') {
                return Range::Unknown(s.into());
            }
            intervals.push(Interval {
                min: (!lo.is_empty()).then(|| (lo.to_string(), open == '[')),
                max: (!hi.is_empty()).then(|| (hi.to_string(), close == ']')),
            });
        }
        if intervals.is_empty() {
            Range::Any
        } else {
            Range::Maven(intervals)
        }
    }

    pub fn matches(&self, version: &str) -> bool {
        match self {
            Range::Any | Range::Unknown(_) => true,
            Range::Fabric(groups) => groups.iter().any(|g| g.iter().all(|p| p.matches(version))),
            Range::Maven(intervals) => intervals.iter().any(|i| i.matches(version)),
        }
    }

    /// Short human form for messages, e.g. "1.20.1 or newer".
    pub fn describe(&self) -> String {
        match self {
            Range::Any => "any version".into(),
            Range::Unknown(s) => s.clone(),
            Range::Fabric(groups) => {
                groups.iter().map(|g| g.iter().map(Pred::describe).collect::<Vec<_>>().join(" and ")).collect::<Vec<_>>().join(" or ")
            }
            Range::Maven(intervals) => intervals.iter().map(Interval::describe).collect::<Vec<_>>().join(" or "),
        }
    }
}

fn parse_fabric_group(s: &str) -> Option<Vec<Pred>> {
    let s = s.trim();
    if s.is_empty() || s == "*" {
        return Some(vec![]);
    }
    let mut preds = vec![];
    // Some mods write ">= 1.2" with a space; join operator and version first.
    let mut tokens: Vec<String> = vec![];
    for t in s.split_whitespace() {
        if let Some(last) = tokens.last_mut() {
            if matches!(last.as_str(), ">=" | "<=" | ">" | "<" | "=" | "~" | "^") {
                last.push_str(t);
                continue;
            }
        }
        tokens.push(t.to_string());
    }
    for t in tokens {
        let t = t.as_str();
        let p = if let Some(v) = t.strip_prefix(">=") {
            Pred::Ge(v.into())
        } else if let Some(v) = t.strip_prefix("<=") {
            Pred::Le(v.into())
        } else if let Some(v) = t.strip_prefix('>') {
            Pred::Gt(v.into())
        } else if let Some(v) = t.strip_prefix('<') {
            Pred::Lt(v.into())
        } else if let Some(v) = t.strip_prefix('~') {
            Pred::Tilde(v.into())
        } else if let Some(v) = t.strip_prefix('^') {
            Pred::Caret(v.into())
        } else {
            let v = t.strip_prefix('=').unwrap_or(t);
            if v == "*" {
                continue;
            }
            let stripped = v.trim_end_matches(".x").trim_end_matches(".X").trim_end_matches(".*");
            if stripped.len() != v.len() {
                Pred::Prefix(stripped.into())
            } else {
                Pred::Eq(v.into())
            }
        };
        if p.version().is_empty() {
            return None;
        }
        preds.push(p);
    }
    Some(preds)
}

fn numeric_parts(v: &str) -> Vec<u64> {
    chunks(v).into_iter().take_while(|c| matches!(c, Chunk::Num(_))).map(|c| if let Chunk::Num(n) = c { n } else { 0 }).collect()
}

impl Pred {
    fn version(&self) -> &str {
        match self {
            Pred::Ge(v) | Pred::Gt(v) | Pred::Le(v) | Pred::Lt(v) | Pred::Eq(v) | Pred::Prefix(v) | Pred::Tilde(v) | Pred::Caret(v) => v,
        }
    }

    fn matches(&self, version: &str) -> bool {
        use Ordering::*;
        match self {
            Pred::Ge(v) => compare(version, v) != Less,
            Pred::Gt(v) => compare(version, v) == Greater,
            Pred::Le(v) => compare(version, v) != Greater,
            Pred::Lt(v) => compare(version, v) == Less,
            Pred::Eq(v) => compare(version, v) == Equal,
            Pred::Prefix(p) => {
                let want = numeric_parts(p);
                let have = numeric_parts(version);
                have.len() >= want.len() && have[..want.len()] == want[..]
            }
            Pred::Tilde(v) => {
                let parts = numeric_parts(v);
                let upper = match parts.as_slice() {
                    [major] => format!("{}", major + 1),
                    [major, minor, ..] => format!("{major}.{}", minor + 1),
                    [] => return true,
                };
                compare(version, v) != Less && compare(version, &upper) == Less
            }
            Pred::Caret(v) => {
                let parts = numeric_parts(v);
                let Some(major) = parts.first() else {
                    return true;
                };
                compare(version, v) != Less && compare(version, &format!("{}", major + 1)) == Less
            }
        }
    }

    fn describe(&self) -> String {
        match self {
            Pred::Ge(v) => format!("{v} or newer"),
            Pred::Gt(v) => format!("newer than {v}"),
            Pred::Le(v) => format!("{v} or older"),
            Pred::Lt(v) => format!("older than {v}"),
            Pred::Eq(v) => v.clone(),
            Pred::Prefix(v) => format!("{v}.x"),
            Pred::Tilde(v) | Pred::Caret(v) => format!("{v} or a compatible newer one"),
        }
    }
}

impl Interval {
    fn matches(&self, version: &str) -> bool {
        use Ordering::*;
        if let Some((min, inclusive)) = &self.min {
            match compare(version, min) {
                Less => return false,
                Equal if !inclusive => return false,
                _ => {}
            }
        }
        if let Some((max, inclusive)) = &self.max {
            match compare(version, max) {
                Greater => return false,
                Equal if !inclusive => return false,
                _ => {}
            }
        }
        true
    }

    fn describe(&self) -> String {
        match (&self.min, &self.max) {
            (Some((a, _)), Some((b, _))) if a == b => a.clone(),
            (Some((a, true)), None) => format!("{a} or newer"),
            (Some((a, false)), None) => format!("newer than {a}"),
            (None, Some((b, true))) => format!("{b} or older"),
            (None, Some((b, false))) => format!("older than {b}"),
            (Some((a, _)), Some((b, true))) => format!("{a} to {b}"),
            (Some((a, _)), Some((b, false))) => format!("{a} up to (not including) {b}"),
            (None, None) => "any version".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Ordering::*;

    #[test]
    fn compares_like_people_expect() {
        assert_eq!(compare("1.20.1", "1.20"), Greater);
        assert_eq!(compare("1.20", "1.20.0"), Equal);
        assert_eq!(compare("1.9", "1.10"), Less);
        assert_eq!(compare("1.0-beta.2", "1.0"), Less);
        assert_eq!(compare("1.0-rc1", "1.0-beta"), Greater);
        assert_eq!(compare("0.5.3+build.12", "0.5.3"), Equal);
        assert_eq!(compare("47.4.28", "47.4.3"), Greater);
        assert_eq!(compare("26.1", "1.21.11"), Greater);
        assert_eq!(compare("v2.0", "2.0"), Equal);
    }

    #[test]
    fn fabric_ranges() {
        assert!(Range::fabric(">=0.15.0").matches("0.16.5"));
        assert!(!Range::fabric(">=0.15.0").matches("0.14.21"));
        assert!(Range::fabric("1.20.x").matches("1.20.1"));
        assert!(!Range::fabric("1.20.x").matches("1.21"));
        assert!(Range::fabric("~1.20.1").matches("1.20.4"));
        assert!(!Range::fabric("~1.20.1").matches("1.21"));
        assert!(Range::fabric(">=1.20 <1.21").matches("1.20.6"));
        assert!(!Range::fabric(">=1.20 <1.21").matches("1.21"));
        assert!(Range::fabric(">= 1.2").matches("1.3"));
        assert!(Range::fabric("*").matches("anything"));
        assert!(Range::fabric("1.21.1").matches("1.21.1"));
        assert!(!Range::fabric("1.21.1").matches("1.21"));
        assert!(Range::fabric_any_of(["1.20.1", "1.20.4"]).matches("1.20.4"));
        assert!(Range::fabric("^2.1").matches("2.9"));
        assert!(!Range::fabric("^2.1").matches("3.0"));
    }

    #[test]
    fn maven_ranges() {
        assert!(Range::maven("[47,)").matches("47.4.28"));
        assert!(!Range::maven("[47,)").matches("46.0.1"));
        assert!(Range::maven("[1.20.1,1.21)").matches("1.20.4"));
        assert!(!Range::maven("[1.20.1,1.21)").matches("1.21"));
        assert!(Range::maven("[1.20.1]").matches("1.20.1"));
        assert!(!Range::maven("[1.20.1]").matches("1.20.2"));
        assert!(Range::maven("(,2.0]").matches("2.0"));
        assert!(Range::maven("[1.0,2.0),[3.0,)").matches("3.1"));
        assert!(!Range::maven("[1.0,2.0),[3.0,)").matches("2.5"));
        assert!(Range::maven("1.0").matches("0.1"), "bare versions are soft");
        assert!(Range::maven("*").matches("x"));
        assert_eq!(Range::maven("[47,)").describe(), "47 or newer");
    }

    #[test]
    fn unreadable_ranges_never_fail() {
        assert!(Range::maven("[oops").matches("1"));
        assert!(matches!(Range::maven("[oops"), Range::Unknown(_)));
    }
}
