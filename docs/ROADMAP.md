# Roadmap

Each phase ends when its gate works on a real machine (Windows and Arch).

| Phase | Scope | Gate |
| --- | --- | --- |
| **0. Foundation** (done) | Core crate, Microsoft + offline sign-in, vanilla/Fabric/Quilt install and launch, Java runtimes, Modrinth install, modpack import, UI shell | Vanilla 26.2 and Fabric + Sodium reach the title screen |
| **1. Alloy foundation** (done) | Rename, repo, CI for Windows and Arch, install-time textures, traced logo and forge, Ori, motion system, speed pass | This release |
| **2. Daily driver** | Forge and NeoForge (installer processors), content updates (hash-based), download manager panel, CurseForge in Discover with dependencies, project pages, pre-launch compatibility check | A big Forge/NeoForge pack runs unchanged |
| **3. Alloy server v1** | `alloy-server` behind Caddy: Alloy accounts (email + Google), CurseForge proxy, settings and instance sync between PCs, social feed | Your Arch and Windows machines share a library |
| **4. Only in Alloy** | Crash doctor + auto-bisect, instance save points, world time machine, party packs, memory autopilot, warm standby (experimental), dependency resolver UI | A crashing pack finds its culprit by itself |
| **5. Assistant and more** | Claude assistant through the server with per-user limits, skins with 3D preview, worlds tab, screenshots, command palette, mclo.gs upload, options.txt sync, Discord presence, tray, self-update, importers from other launchers, export | |

## Every planned feature

**Accounts:** Microsoft and offline, multi-account switcher, per-instance account, skin and
cape manager with 3D preview and a saved skin library.

**Instances:** all loaders, groups, search and sort, duplicate, rename, custom icons,
per-instance memory, Java, JVM arguments, window size, environment variables, pre/post
hooks, export (.mrpack, CurseForge zip), import from Prism, MultiMC, CurseForge App,
Modrinth App and vanilla, play time, crash detection, repair.

**Content:** Discover across Modrinth and CurseForge, project pages (gallery, description,
versions), dependency resolution on both platforms, update one or all with hash checks,
install from URL and drag-and-drop, CurseForge blocked-download flow that watches Downloads,
modpack updates with a diff preview that keeps configs.

**Game:** Java manager, log viewer with search and mclo.gs upload, worlds tab with backups
and quick-play, screenshots gallery, options.txt sync, performance preset.

**Only in Alloy:** warm standby, world time machine, instance save points, crash doctor with
auto-bisect, pre-launch compatibility check, party packs, memory autopilot, shared storage
for identical jars, mod loadouts, version upgrade assistant, cross-PC sync.

**App:** command palette (Ctrl+K), download manager, backgrounds, motion settings, Discord
Rich Presence, system tray, self-update from private releases.

**Server:** Alloy accounts (email and Google), social feed (posts with screenshots, likes,
comments, follows, presence), CurseForge proxy and shared caching, share links that open in
Alloy, play-together relay, Claude assistant with per-user budgets.

**Not planned:** deploying to servers from the launcher.
