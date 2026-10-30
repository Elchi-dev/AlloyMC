//! Block textures for Alloy's UI, taken from Mojang's own client jar at install time.
//!
//! Instead of downloading the whole jar (~40 MB) we read only its zip index with an HTTP
//! range request, then fetch each wanted PNG's few hundred bytes. Nothing from the game is
//! bundled with Alloy; every machine gets the files straight from Mojang.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use futures::{StreamExt, TryStreamExt};
use serde::{Deserialize, Serialize};

use crate::meta::mojang;
use crate::{Error, Launcher, Result};

/// The textures Alloy uses: ores for instance icons, metals and lava for brand moments.
pub const WANTED: &[&str] = &[
    "stone",
    "deepslate",
    "coal_ore",
    "iron_ore",
    "copper_ore",
    "gold_ore",
    "redstone_ore",
    "emerald_ore",
    "lapis_ore",
    "diamond_ore",
    "deepslate_coal_ore",
    "deepslate_iron_ore",
    "deepslate_copper_ore",
    "deepslate_gold_ore",
    "deepslate_redstone_ore",
    "deepslate_emerald_ore",
    "deepslate_lapis_ore",
    "deepslate_diamond_ore",
    "nether_gold_ore",
    "nether_quartz_ore",
    "ancient_debris_side",
    "gold_block",
    "raw_gold_block",
    "iron_block",
    "netherite_block",
    "lava_still",
];

const PREFIX: &str = "assets/minecraft/textures/block/";
const MARKER: &str = "textures.json";

#[derive(Debug, Serialize, Deserialize)]
struct Marker {
    game_version: String,
    files: Vec<String>,
}

/// Where textures live: the user's data dir, or a system-wide copy an Arch package fetched.
pub fn search_dirs(launcher: &Launcher) -> Vec<PathBuf> {
    let mut dirs = vec![launcher.dirs.root().join("textures")];
    if cfg!(target_os = "linux") {
        dirs.push(PathBuf::from("/usr/share/alloy/textures"));
    }
    dirs
}

/// The directory that already holds a complete set, if any.
pub fn installed_dir(launcher: &Launcher) -> Option<PathBuf> {
    search_dirs(launcher).into_iter().find(|d| d.join(MARKER).exists())
}

/// All installed textures as PNG bytes, keyed by name ("gold_ore").
pub async fn load(launcher: &Launcher) -> Result<HashMap<String, Vec<u8>>> {
    let Some(dir) = installed_dir(launcher) else {
        return Ok(HashMap::new());
    };
    let mut out = HashMap::new();
    for name in WANTED {
        if let Ok(bytes) = tokio::fs::read(dir.join(format!("{name}.png"))).await {
            out.insert(name.to_string(), bytes);
        }
    }
    Ok(out)
}

/// Textures as `data:image/png;base64,...` URLs for the UI (about 150 KB in total).
pub async fn data_urls(launcher: &Launcher) -> Result<HashMap<String, String>> {
    use base64::Engine;
    Ok(load(launcher)
        .await?
        .into_iter()
        .map(|(k, v)| (k, format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(v))))
        .collect())
}

/// Fetches the textures into `<data>/textures` unless a complete set exists somewhere.
pub async fn ensure(launcher: &Launcher) -> Result<PathBuf> {
    if let Some(dir) = installed_dir(launcher) {
        return Ok(dir);
    }
    let dest = launcher.dirs.root().join("textures");
    fetch_into(&launcher.http, launcher, &dest).await?;
    Ok(dest)
}

/// Downloads the wanted textures from the latest release's client jar into `dest`.
pub async fn fetch_into(http: &reqwest::Client, launcher: &Launcher, dest: &Path) -> Result<()> {
    let manifest = mojang::manifest(launcher).await?;
    let version = mojang::version_json(launcher, &manifest.latest.release).await?;
    let client = version.downloads.and_then(|d| d.client).ok_or_else(|| Error::NotFound("client jar".into()))?;
    let total = client.size.ok_or_else(|| Error::other("client jar has no size"))?;

    let entries = read_central_directory(http, &client.url, total).await?;
    tokio::fs::create_dir_all(dest).await?;

    // Owned values only: borrowed data inside a buffered stream makes the future
    // impossible to spawn on a runtime (higher-ranked lifetime error).
    let wanted: Vec<(String, ZipEntry)> =
        WANTED.iter().filter_map(|n| entries.get(&format!("{PREFIX}{n}.png")).map(|e| (n.to_string(), e.clone()))).collect();

    let fetched: Vec<(String, Vec<u8>)> = futures::stream::iter(wanted.into_iter().map(|(name, entry)| {
        let http = http.clone();
        let url = client.url.clone();
        async move {
            let bytes = fetch_entry(&http, &url, &entry).await?;
            Ok::<_, Error>((name, bytes))
        }
    }))
    .buffer_unordered(8)
    .try_collect()
    .await?;

    let mut files = vec![];
    for (name, bytes) in fetched {
        tokio::fs::write(dest.join(format!("{name}.png")), bytes).await?;
        files.push(name);
    }
    let marker = Marker { game_version: manifest.latest.release, files };
    tokio::fs::write(dest.join(MARKER), serde_json::to_vec_pretty(&marker)?).await?;
    Ok(())
}

#[derive(Debug, Clone)]
struct ZipEntry {
    method: u16,
    compressed: u64,
    uncompressed: u64,
    local_offset: u64,
}

async fn range(http: &reqwest::Client, url: &str, start: u64, end_inclusive: u64) -> Result<Vec<u8>> {
    let resp = http.get(url).header("Range", format!("bytes={start}-{end_inclusive}")).send().await?.error_for_status()?;
    if resp.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(Error::other("server ignored the range request"));
    }
    Ok(resp.bytes().await?.to_vec())
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}
fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

/// Reads the zip's end record and central directory with two range requests.
async fn read_central_directory(http: &reqwest::Client, url: &str, total: u64) -> Result<HashMap<String, ZipEntry>> {
    let tail_len = total.min(65_557);
    let tail = range(http, url, total - tail_len, total - 1).await?;
    let eocd = tail.windows(4).rposition(|w| w == b"PK\x05\x06").ok_or_else(|| Error::other("no zip end record"))?;
    let cd_size = u32_at(&tail, eocd + 12) as u64;
    let cd_offset = u32_at(&tail, eocd + 16) as u64;
    if cd_offset == 0xFFFF_FFFF || cd_size == 0xFFFF_FFFF {
        return Err(Error::Unsupported("zip64 client jar".into()));
    }
    let cd = range(http, url, cd_offset, cd_offset + cd_size - 1).await?;

    let mut out = HashMap::new();
    let mut i = 0usize;
    while i + 46 <= cd.len() && &cd[i..i + 4] == b"PK\x01\x02" {
        let method = u16_at(&cd, i + 10);
        let compressed = u32_at(&cd, i + 20) as u64;
        let uncompressed = u32_at(&cd, i + 24) as u64;
        let name_len = u16_at(&cd, i + 28) as usize;
        let extra_len = u16_at(&cd, i + 30) as usize;
        let comment_len = u16_at(&cd, i + 32) as usize;
        let local_offset = u32_at(&cd, i + 42) as u64;
        let name = String::from_utf8_lossy(&cd[i + 46..i + 46 + name_len]).into_owned();
        if name.starts_with(PREFIX) {
            out.insert(name, ZipEntry { method, compressed, uncompressed, local_offset });
        }
        i += 46 + name_len + extra_len + comment_len;
    }
    Ok(out)
}

/// Fetches one entry: its local header (whose extra field may differ from the index) and data.
async fn fetch_entry(http: &reqwest::Client, url: &str, e: &ZipEntry) -> Result<Vec<u8>> {
    // Local header is 30 bytes + name + extra; over-fetch 512 bytes for those, then cut.
    let blob = range(http, url, e.local_offset, e.local_offset + 30 + 512 + e.compressed).await?;
    if &blob[0..4] != b"PK\x03\x04" {
        return Err(Error::other("bad local zip header"));
    }
    let start = 30 + u16_at(&blob, 26) as usize + u16_at(&blob, 28) as usize;
    let data = blob.get(start..start + e.compressed as usize).ok_or_else(|| Error::other("short zip entry"))?;
    match e.method {
        0 => Ok(data.to_vec()),
        8 => {
            let mut out = Vec::with_capacity(e.uncompressed as usize);
            flate2::read::DeflateDecoder::new(data).read_to_end(&mut out)?;
            Ok(out)
        }
        m => Err(Error::Unsupported(format!("zip method {m}"))),
    }
}
