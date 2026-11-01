# Roadmap

Each phase ends when its gate works on a real machine (Windows and Arch).

## Open right now

Shipped: 0.3.0 (AlloyMC sign-in and Minecraft linking, CurseForge through the server,
crash doctor with auto-bisect, save points, export, imports from other launchers) and 0.3.1
(updates arrive by themselves). 0.3.2: the CurseForge window, right-click menus, "About this
pack", a check that follows what the loader really loads, and a launcher that no longer
goes blank while the game runs. 0.4.0: the Claude assistant (server relay with budgets,
tools in Alloy, approvals, saved conversations), 0.4.1 and 0.4.2 (design, batches, copy buttons).

Next, in this order:

1. Shipped in 0.5.0: assistant pack plans, project cards, undo per reply, crash
   explanations in the crash doctor, the crash search as a tool, memory, scripts, and
   server packs. Next for the assistant: a test start at the end of a pack build by
   default, and sharing a conversation.
2. **Modpack updates** with a preview of what changes (added, removed, updated mods),
   keeping configs, a save point first. **Party packs and share links:**
   `alloymc.dev/p/<pack>` opens in Alloy, previews in Discord, keeps friends' copies in sync,
   and offers a direct `.mrpack`/CurseForge zip link for Prism and other launchers (mods by
   reference only; configs and small files bundled; size cap; reports to the admin view).
3. **Worlds and screenshots:** worlds tab with backups, play straight into a world or a
   server, a server browser, the world time machine (snapshots stored like save points),
   a screenshots page with an editor.
4. **Warm standby** (opt-in per instance, one or two at a time) with its settings, and the
   cheap layers for every pack (pre-reading files, Java's startup cache, ModernFix hint).
5. **Accounts and social:** skin and cape manager with 3D preview and a skin library, sync
   between PCs, posts with screenshots, likes, comments, feed, presence.
6. **App:** command palette (Ctrl+K), Discord Rich Presence, system tray, desktop
   shortcuts for instances, a Files tab per instance, pause and resume in downloads,
   changelogs in "Update all", changing mods while the game runs (applied next start).
7. **Extensions:** the developer extension API, a marketplace, and themes (built-in and
   custom) shipped through it.

Also on the list: crash-log sharing through AlloyMC, memory autopilot, mod loadouts,
version upgrade assistant, shared storage for identical jars, performance preset, a
dependency graph view, pre/post launch hooks, syncing `options.txt`, server lists, hotbars
and command history across instances, Discover filters ("works with", "in a modpack"),
collections and update notifications for followed projects, Adoptium Java where Mojang has
none, crash bisect that starts straight into a world, code-signed Windows installer, a
refreshed demo on launcher.alloymc.dev.

| Phase | Scope | Gate |
| --- | --- | --- |
| **0. Foundation** (done) | Core crate, Microsoft + offline sign-in, vanilla/Fabric/Quilt install and launch, Java runtimes, Modrinth install, modpack import, UI shell | Vanilla 26.2 and Fabric + Sodium reach the title screen |
| **1. Alloy foundation** (done) | Rename, repo, CI for Windows and Arch, install-time textures, traced logo and forge, Ori, motion system, speed pass | This release |
| **2. Daily driver** (done, 0.2.0) | Forge and NeoForge (installer processors), content updates (hash-based), download manager panel, CurseForge in Discover with dependencies, project pages, pre-launch compatibility check | A big Forge/NeoForge pack runs unchanged |
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
