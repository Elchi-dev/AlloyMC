use std::path::{Component, Path, PathBuf};

use sha1::{Digest, Sha1};
use sha2::Sha512;
use tokio::io::AsyncReadExt;

use crate::{Error, Result};

pub async fn sha1_file(path: &Path) -> Result<String> {
    hash_file::<Sha1>(path).await
}

pub async fn sha512_file(path: &Path) -> Result<String> {
    hash_file::<Sha512>(path).await
}

async fn hash_file<D: Digest>(path: &Path) -> Result<String> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = D::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Joins a relative path from untrusted data (modpack index, zip entry) onto `base`,
/// refusing absolute paths and `..` so nothing escapes the instance folder.
pub fn safe_join(base: &Path, relative: &str) -> Result<PathBuf> {
    let rel = Path::new(relative);
    let mut out = base.to_path_buf();
    for comp in rel.components() {
        match comp {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            _ => return Err(Error::other(format!("Unsafe path in archive: {relative}"))),
        }
    }
    if out == base {
        return Err(Error::other(format!("Empty path in archive: {relative}")));
    }
    Ok(out)
}

/// Turns a display name into a folder-safe slug: "My Pack 1.21!" -> "my-pack-1-21".
pub fn slugify(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    let mut dash = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash && !slug.is_empty() {
            slug.push('-');
            dash = true;
        }
    }
    let slug = slug.trim_end_matches('-').to_string();
    if slug.is_empty() {
        "instance".into()
    } else {
        slug
    }
}

/// Converts a Maven coordinate (`group:artifact:version[:classifier][@ext]`) into its repository path.
pub fn maven_path(coord: &str) -> Option<String> {
    let (coord, ext) = match coord.split_once('@') {
        Some((c, e)) => (c, e),
        None => (coord, "jar"),
    };
    let parts: Vec<&str> = coord.split(':').collect();
    if parts.len() < 3 {
        return None;
    }
    let group = parts[0].replace('.', "/");
    let artifact = parts[1];
    let version = parts[2];
    let file = match parts.get(3) {
        Some(classifier) => format!("{artifact}-{version}-{classifier}.{ext}"),
        None => format!("{artifact}-{version}.{ext}"),
    };
    Some(format!("{group}/{artifact}/{version}/{file}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maven_paths() {
        assert_eq!(maven_path("net.fabricmc:fabric-loader:0.16.5").unwrap(), "net/fabricmc/fabric-loader/0.16.5/fabric-loader-0.16.5.jar");
        assert_eq!(maven_path("org.lwjgl:lwjgl:3.3.3:natives-windows").unwrap(), "org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3-natives-windows.jar");
        assert_eq!(
            maven_path("de.oceanlabs.mcp:mcp_config:1.20.1@zip").unwrap(),
            "de/oceanlabs/mcp/mcp_config/1.20.1/mcp_config-1.20.1.zip"
        );
    }

    #[test]
    fn safe_join_blocks_escape() {
        let base = Path::new("/inst");
        assert!(safe_join(base, "mods/a.jar").is_ok());
        assert!(safe_join(base, "../evil").is_err());
        assert!(safe_join(base, "/etc/passwd").is_err());
    }

    #[test]
    fn slugs() {
        assert_eq!(slugify("My Pack 1.21!"), "my-pack-1-21");
        assert_eq!(slugify("???"), "instance");
    }
}
