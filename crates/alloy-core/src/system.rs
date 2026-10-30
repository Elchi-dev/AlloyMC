//! Facts about the machine that shape advice: how much memory it has, and whether the
//! tools old game versions rely on are present.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    /// Installed memory in MB.
    pub total_mb: u64,
    /// The most a game should get: what's left after keeping some for the system.
    pub max_game_mb: u64,
}

/// Leaves room for the OS and the launcher: 2 GB below 8 GB of RAM, 3 GB up to 16 GB, 4 GB above.
pub fn memory() -> Option<Memory> {
    let total_mb = total_memory_bytes()? / (1024 * 1024);
    let reserve = match total_mb {
        0..=8192 => 2048,
        8193..=16384 => 3072,
        _ => 4096,
    };
    Some(Memory { total_mb, max_game_mb: total_mb.saturating_sub(reserve).max(1024) })
}

/// Memory to suggest for an instance with this many mods.
pub fn recommended_mb(mods: usize, total_mb: u64) -> u64 {
    let want: u64 = match mods {
        0 => 2048,
        1..=30 => 3072,
        31..=100 => 4096,
        101..=200 => 6144,
        _ => 8192,
    };
    want.min(total_mb / 2).max(1024)
}

#[cfg(target_os = "linux")]
fn total_memory_bytes() -> Option<u64> {
    let info = std::fs::read_to_string("/proc/meminfo").ok()?;
    let kb: u64 = info.lines().find(|l| l.starts_with("MemTotal:"))?.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb * 1024)
}

#[cfg(windows)]
fn total_memory_bytes() -> Option<u64> {
    #[repr(C)]
    struct MemoryStatusEx {
        length: u32,
        memory_load: u32,
        total_phys: u64,
        avail_phys: u64,
        total_page_file: u64,
        avail_page_file: u64,
        total_virtual: u64,
        avail_virtual: u64,
        avail_extended_virtual: u64,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GlobalMemoryStatusEx(buffer: *mut MemoryStatusEx) -> i32;
    }
    let mut status = MemoryStatusEx {
        length: std::mem::size_of::<MemoryStatusEx>() as u32,
        memory_load: 0,
        total_phys: 0,
        avail_phys: 0,
        total_page_file: 0,
        avail_page_file: 0,
        total_virtual: 0,
        avail_virtual: 0,
        avail_extended_virtual: 0,
    };
    // SAFETY: the struct has the documented MEMORYSTATUSEX layout and its length is set.
    let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
    (ok != 0).then_some(status.total_phys)
}

#[cfg(target_os = "macos")]
fn total_memory_bytes() -> Option<u64> {
    let out = std::process::Command::new("sysctl").args(["-n", "hw.memsize"]).output().ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
fn total_memory_bytes() -> Option<u64> {
    None
}

/// LWJGL 2 (Minecraft 1.12.2 and older) reads display modes through the `xrandr` program
/// on Linux and crashes at start without it.
pub fn missing_xrandr(json: &crate::meta::VersionJson) -> bool {
    if !cfg!(target_os = "linux") {
        return false;
    }
    let uses_lwjgl2 = json.libraries.iter().any(|l| l.name.starts_with("org.lwjgl.lwjgl:lwjgl:2."));
    uses_lwjgl2 && !on_path("xrandr")
}

fn on_path(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_is_known_here() {
        if cfg!(any(target_os = "linux", windows, target_os = "macos")) {
            let m = memory().expect("memory");
            assert!(m.total_mb > 256 && m.max_game_mb <= m.total_mb);
        }
    }

    #[test]
    fn recommendations_scale_with_mods() {
        assert_eq!(recommended_mb(0, 32768), 2048);
        assert_eq!(recommended_mb(330, 32768), 8192);
        assert_eq!(recommended_mb(330, 8192), 4096);
    }
}
