//! Game and loader metadata: Mojang's manifest, version JSONs, and mod loader profiles.

pub mod loaders;
pub mod mojang;
pub mod rules;
pub mod version;

pub use mojang::{GameVersion, VersionManifest};
pub use version::VersionJson;
