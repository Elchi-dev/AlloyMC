# Dependencies

Mod dependencies are where most indie launchers break: they trust platform metadata, which
is often incomplete. Alloy's resolver (Phase 2 and 4) is built on what the jars themselves say.

## Sources of truth, in order

1. **The jar:** `fabric.mod.json`, `quilt.mod.json`, `META-INF/mods.toml`,
   `META-INF/neoforge.mods.toml`. Real mod IDs, version ranges, `breaks` / `conflicts`,
   `provides` aliases, and jars bundled inside the jar.
2. **The platform:** Modrinth and CurseForge dependency lists, used to find and download.
3. **Alloy's lockfile:** what is installed right now.

## What the resolver does

| Case | Behaviour |
| --- | --- |
| Required dependency missing | Installed automatically, from either platform, matched by mod ID |
| Library bundled inside another mod's jar | Not installed again |
| Two mods conflict (declared) | Shown before launch, with the fix |
| Removing a library others need | Warns which mods depend on it |
| Library no longer needed | Offered for removal |
| "Update all" | Never moves a library past what its dependents allow |
| Version ranges | Fabric/Quilt semver ranges and Maven ranges (Forge/NeoForge) |

## alloy.lock

Each instance keeps `alloy.lock`: every file with its source, project, version and hash.
Instances are reproducible, party packs install exactly what you have, and the
pre-launch check reads it instead of rescanning.

## Alloy's own dependencies

- Windows: the installer bootstraps WebView2 if missing.
- Arch: the package depends on `webkit2gtk-4.1`; keychain support is optional.
- Java: Mojang's runtimes, installed per version (Adoptium fallback planned for Linux arm64).
