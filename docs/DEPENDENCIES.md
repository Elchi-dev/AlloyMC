# Dependencies

Mod dependencies are where most indie launchers break: they trust platform metadata, which
is often incomplete. Alloy's resolver is built on what the jars themselves say, with the
platforms filling the gaps. Code: `crates/alloy-core/src/content/` (`jarmeta`, `check`,
`manage`, `scan`, `identify`, `version_range`).

## Sources of truth

1. **The jar:** `fabric.mod.json`, `quilt.mod.json`, `META-INF/mods.toml`,
   `META-INF/neoforge.mods.toml`, `mcmod.info`. Real mod IDs, version ranges, `breaks` /
   `conflicts` / `incompatible` / `discouraged`, `provides` aliases, and jars bundled inside
   the jar (Fabric's `jars`, Forge's `jarjar`, and manifest-embedded jars).
2. **The platform:** Modrinth and CurseForge dependency lists. Many mods declare their
   libraries only there, so they count for the graph and for installing.
3. **What is on disk:** every scan reads the folders, so files added by hand are included.

A multi-loader jar (Fabric and Forge metadata in one file) is judged only by the metadata
the instance's loader reads. With Sinytra Connector installed, Forge and NeoForge also read
Fabric metadata.

## Never invent a problem

A range Alloy can't parse, a mod without metadata, a snapshot game version: all count as
fine. A pre-launch check that cries wolf gets ignored, so it only reports what it can prove.

## What the resolver does

| Case | Behaviour |
| --- | --- |
| Required dependency missing | Installed automatically, from either platform, matched by mod ID |
| Library bundled inside another mod's jar | Not installed again |
| Two mods conflict (declared) | Shown before launch, with the fix |
| Removing a library others need | Warns which mods depend on it |
| Library no longer needed | Offered for removal |
| "Update all" | Moves a library only as far as its dependents allow; holds back the rest, with the reason |
| Version ranges | Fabric/Quilt semver ranges and Maven ranges (Forge/NeoForge) |
| Duplicates | The same mod in two jars is an error; the fix keeps the newer one |
| Wrong loader, game or Java | Reported before launch, with a fix where there is one |
| Files a pack lists but couldn't ship | Fetched from CurseForge where allowed, else via Downloads |

## Update all, in detail

1. Every update is downloaded to `.launcher/staging/`, nothing is replaced yet.
2. The instance is checked as it would be with all of them in place.
3. If an update breaks something (a library moving past what another mod accepts), Alloy
   tries older versions of that library, newest first, and takes the newest that fits.
4. Whatever still breaks something is held back, with the sentence explaining why.
5. The rest move in, keeping each file's enabled or disabled state.

Example from testing: Iris 1.7.3 and Sodium 0.5.11 on Fabric 1.21.1. Iris's newest release
accepts only Sodium 0.6.x, so Update all installs Iris 1.8.8 with Sodium 0.6.13 instead of
the newest Sodium 0.8.13, and the instance stays playable.

## alloy.lock

Each instance keeps `alloy.lock`: every file with its path, size, SHA-1, source, project,
version, mod id and enabled state, plus the game and loader versions. It's rewritten on
every scan when something changed. Instances are reproducible and party packs can install
exactly what you have.

## Speed

Scans are cached per file by size and modification time: 400 files take about 6 s the
first time (hashing every jar) and 30 ms after that; the check itself takes about 1 ms.
After an install Alloy scans in the background, so the Content tab opens instantly.

## Alloy's own dependencies

- Windows: the installer bootstraps WebView2 if missing.
- Arch: the package depends on `webkit2gtk-4.1`; keychain support is optional.
- Java: Mojang's runtimes, installed per version (Adoptium fallback planned for Linux arm64).
