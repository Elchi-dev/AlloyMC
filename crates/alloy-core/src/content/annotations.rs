//! What Forge 1.12 and older really load: the `@Mod` annotations in a jar's classes.
//!
//! Those Forges find mods by scanning classes for `@Mod`; `mcmod.info` only adds a name and
//! a description to the ids found there, and its `requiredMods` are ignored unless a mod
//! opts in. Copy-pasted `mcmod.info` files are common (DragonMurder ships PlaneFix's, with
//! the id `planefix`), so judging 1.12 packs by `mcmod.info` alone shows mods installed
//! twice, or missing requirements, that Forge never sees.
//!
//! ForgeGradle also writes `META-INF/fml_cache_annotation.json`, but it can be partial
//! (Integrated Dynamics' lists only its compat mod), so the classes themselves are read.
//! Only the ids are taken from here: a coremod can register ids without any `@Mod`, so the
//! annotations can't say what's missing, only what's really there.

use std::collections::HashMap;
use std::io::{Read, Seek};

/// One `@Mod` annotation: its string values (`modid`, `name`, `version`, `dependencies`...).
pub type ModAnnotation = HashMap<String, String>;

const MOD_TYPES: &[&str] = &["Lnet/minecraftforge/fml/common/Mod;", "Lcpw/mods/fml/common/Mod;"];
/// Jars with more classes than this aren't scanned class by class (they're libraries).
const MAX_CLASSES: usize = 30_000;
const MAX_CLASS_BYTES: u64 = 4 * 1024 * 1024;

/// The `@Mod` annotations in a jar, or `None` when it couldn't be read.
pub fn read<R: Read + Seek>(zip: &mut zip::ZipArchive<R>) -> Option<Vec<ModAnnotation>> {
    let classes: Vec<String> = zip.file_names().filter(|n| n.ends_with(".class")).map(str::to_string).collect();
    if classes.len() > MAX_CLASSES {
        return None;
    }
    let needles: Vec<&[u8]> = MOD_TYPES.iter().map(|t| t.as_bytes()).collect();
    let mut out = vec![];
    let mut buf = Vec::new();
    for name in classes {
        let Ok(mut f) = zip.by_name(&name) else { continue };
        if f.size() > MAX_CLASS_BYTES {
            continue;
        }
        buf.clear();
        if f.read_to_end(&mut buf).is_err() {
            continue;
        }
        // Cheap test first: the annotation's type name sits in the constant pool as is.
        if !needles.iter().any(|n| contains(&buf, n)) {
            continue;
        }
        out.extend(class_annotations(&buf).unwrap_or_default());
    }
    Some(out)
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// Reads a class file far enough to get its class-level `@Mod` annotations.
pub fn class_annotations(b: &[u8]) -> Option<Vec<ModAnnotation>> {
    let mut c = Cursor { b, at: 0 };
    if c.u4()? != 0xCAFE_BABE {
        return None;
    }
    c.skip(4)?;
    let count = c.u2()? as usize;
    let mut utf8: HashMap<usize, String> = HashMap::new();
    let mut i = 1;
    while i < count {
        let tag = c.u1()?;
        match tag {
            1 => {
                let len = c.u2()? as usize;
                let bytes = c.take(len)?;
                // Modified UTF-8; ids and versions are plain ASCII in practice.
                utf8.insert(i, String::from_utf8_lossy(bytes).into_owned());
            }
            3 | 4 => c.skip(4)?,
            5 | 6 => {
                c.skip(8)?;
                i += 1; // longs and doubles take two slots
            }
            7 | 8 | 16 | 19 | 20 => c.skip(2)?,
            9..=12 | 17 | 18 => c.skip(4)?,
            15 => c.skip(3)?,
            _ => return None,
        }
        i += 1;
    }
    c.skip(6)?; // access, this, super
    let interfaces = c.u2()? as usize;
    c.skip(interfaces * 2)?;
    for _ in 0..2 {
        // fields, then methods
        let n = c.u2()?;
        for _ in 0..n {
            c.skip(6)?;
            skip_attributes(&mut c)?;
        }
    }
    let mut out = vec![];
    let n = c.u2()?;
    for _ in 0..n {
        let name = c.u2()? as usize;
        let len = c.u4()? as usize;
        let body = c.take(len)?;
        if utf8.get(&name).map(String::as_str) != Some("RuntimeVisibleAnnotations") {
            continue;
        }
        let mut a = Cursor { b: body, at: 0 };
        let count = a.u2()?;
        for _ in 0..count {
            let ty = a.u2()? as usize;
            let mut values = ModAnnotation::new();
            let pairs = a.u2()?;
            for _ in 0..pairs {
                let key = a.u2()? as usize;
                if let Some(v) = element(&mut a, &utf8)? {
                    if let Some(k) = utf8.get(&key) {
                        values.insert(k.clone(), v);
                    }
                }
            }
            if utf8.get(&ty).is_some_and(|t| MOD_TYPES.contains(&t.as_str())) && values.contains_key("modid") {
                out.push(values);
            }
        }
    }
    Some(out)
}

/// An element value: `Some(text)` for strings, `None` for anything else (skipped).
fn element(c: &mut Cursor, utf8: &HashMap<usize, String>) -> Option<Option<String>> {
    let tag = c.u1()?;
    Some(match tag {
        b's' => utf8.get(&(c.u2()? as usize)).cloned(),
        b'B' | b'C' | b'D' | b'F' | b'I' | b'J' | b'S' | b'Z' | b'c' => {
            c.skip(2)?;
            None
        }
        b'e' => {
            c.skip(4)?;
            None
        }
        b'@' => {
            c.skip(2)?;
            let pairs = c.u2()?;
            for _ in 0..pairs {
                c.skip(2)?;
                element(c, utf8)?;
            }
            None
        }
        b'[' => {
            let n = c.u2()?;
            for _ in 0..n {
                element(c, utf8)?;
            }
            None
        }
        _ => return None,
    })
}

fn skip_attributes(c: &mut Cursor) -> Option<()> {
    let n = c.u2()?;
    for _ in 0..n {
        c.skip(2)?;
        let len = c.u4()? as usize;
        c.skip(len)?;
    }
    Some(())
}

struct Cursor<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(s)
    }
    fn skip(&mut self, n: usize) -> Option<()> {
        self.take(n).map(|_| ())
    }
    fn u1(&mut self) -> Option<u8> {
        self.take(1).map(|s| s[0])
    }
    fn u2(&mut self) -> Option<u16> {
        self.take(2).map(|s| u16::from_be_bytes([s[0], s[1]]))
    }
    fn u4(&mut self) -> Option<u32> {
        self.take(4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLASS: &[u8] = include_bytes!("testdata/LegacyMod.class");

    #[test]
    fn reads_mod_annotation_from_class() {
        let found = class_annotations(CLASS).unwrap();
        assert_eq!(found.len(), 1);
        let m = &found[0];
        assert_eq!(m["modid"], "dragonmurder");
        assert_eq!(m["version"], "1.0.1");
        assert_eq!(m["name"], "Dragon Murder");
        assert!(m["dependencies"].starts_with("required-after:forge@"));
    }

    #[test]
    fn junk_is_none() {
        assert!(class_annotations(b"not a class").is_none());
        assert!(class_annotations(&CLASS[..CLASS.len() / 2]).is_none());
    }

    fn jar(files: &[(&str, &[u8])]) -> zip::ZipArchive<std::io::Cursor<Vec<u8>>> {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, data) in files {
            w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut w, data).unwrap();
        }
        zip::ZipArchive::new(std::io::Cursor::new(w.finish().unwrap().into_inner())).unwrap()
    }

    #[test]
    fn reads_jars_by_classes() {
        let mut z = jar(&[("shadows/dragon/DragonMurder.class", CLASS), ("other/Plain.class", b"\xCA\xFE\xBA\xBE")]);
        let found = read(&mut z).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0]["modid"], "dragonmurder");
    }
}
