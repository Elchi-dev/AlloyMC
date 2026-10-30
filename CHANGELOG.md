# Changelog

## 0.2.0

Everything for daily use. Installs as a one-click update from 0.1.0.

### Forge and NeoForge
- Installed from their official installers, run the way the installer does it: bundled
  libraries copied, processors run with the game's Java, no installer window.
- The patched game jar is shared: the first instance of a version takes about half a minute,
  every later one about 90 ms.
- NeoForge, Forge 1.13+, Forge 1.12.2 and legacy Forge 1.6 to 1.12. New instances get Forge's
  recommended build and NeoForge's newest stable one.
- Checked: Forge 1.20.1, NeoForge 1.21.1, Forge 1.12.2 and Forge 1.7.10 load into the game;
  Better MC (BMC4, 363 mods) passes the check and Forge loads every mod after Alloy repairs it.

### Dependencies done right
- Alloy reads each jar's own description (`fabric.mod.json`, `quilt.mod.json`,
  `mods.toml`, `neoforge.mods.toml`, `mcmod.info`), including jars bundled inside jars, and
  judges multi-loader jars only by what the instance's loader reads.
- The platform's dependency list counts too: some mods declare their libraries only there.
- Installs pull in every required dependency from Modrinth or CurseForge, reusing whatever
  is already present (also when bundled inside another mod), and mark libraries so they can
  be cleaned up once nothing needs them.
- `alloy.lock` in each instance records every file with its hash and source.

### Check before launch
- Play first checks for missing or wrong-version dependencies, mods for another loader,
  Minecraft or Java version, duplicates and declared incompatibilities, in about a
  millisecond, and never reports what it can't prove.
- Problems come with one-click fixes: install the missing mod, keep the newer duplicate,
  switch the loader version, install Sinytra Connector for Fabric mods on Forge.
- On Linux, Minecraft 1.12.2 and older is checked for `xrandr`, which it needs to start.

### Content
- Mods, resource packs and shaders: enable, disable, remove, multi-select, and for each mod
  what it needs and what needs it. Removing something another mod needs asks first.
- Update all: everything is staged and checked as a set; a library moves only as far as the
  mods that need it allow, and an update that would still break something is held back with
  the reason. Stays on release versions unless you're already on a beta.
- Files Alloy didn't install are identified on Modrinth (by hash) and CurseForge (by
  fingerprint) and get their name, icon and updates.
- Modpacks that list CurseForge-only files they couldn't include get them: from CurseForge
  directly where the author allows it, otherwise through Downloads.
- CurseForge files whose authors block launcher downloads: Alloy lists them with their
  download page and picks each one up from your Downloads folder, checked against its hash.
- CurseForge packs put resource packs and shaders in their own folders.

### Discover and installing
- Project pages: description, gallery, versions with changelogs, dependencies, and an
  "Install into…" picker that shows which of your instances each version fits.
- Paste a Modrinth or CurseForge link (into Discover, or the Link button in an instance).
- Drop files anywhere on the window: modpacks become instances, mods, resource packs and
  shaders go into the instance you're looking at, an image becomes its icon.

### Downloads panel
- Every install, import and update as a job with live progress and speed. Cancel any of
  them; retry what failed. A cancelled pack leaves nothing half-made behind.

### Instances
- Icons from any image (cropped to a square, pixel art kept sharp) or a block texture.
- Memory up to what your PC can spare, with a recommendation from the mod count and a
  warning when the system would run short.
- Library groups, right-click menu: play, rename, duplicate, move to group, open folder,
  repair, trash.

### Fixes
- The pixel font drew "fi" as one glyph ("Arst instance").
- The memory slider stopped at 16 GB.

## 0.1.0

The first public alpha.

### Updates
- One-click updates from signed `.alloyupdate` files: found in Downloads, dropped on the
  window or picked in Settings; signatures, hash and platform verified; settings backed up;
  the installer runs in update mode and restarts Alloy; the result is confirmed on start.

### Alloy foundation
- Renamed to Alloy; repository, CI (Linux + Windows), release workflow, Arch PKGBUILD.
- Logo and forge background traced 1:1 from the original artwork, recoloured gold, animated.
- Ori joins from Orbit with a steel ring and a gold moon.
- Motion system: every button, card, field, tab, dialog, toast and page enters, hovers,
  presses and leaves with shared tokens; Full, Calm and Off settings; loops pause while playing.
- The Play block heats into molten metal on a long hover (WebGL, CSS fallback), with sparks.
- Instance icons are real ore textures, fetched from Mojang at install time (116 KB).
- Speed: second installs 47.2 s → 58 ms; launch command ready in 0.7 ms; window shows on
  first paint with cached data; pages split and prefetched; virtual log list.
- CurseForge is a work in progress in public builds; paste your own key in Settings to try it.

### Phase 0
- Rust core: Microsoft and offline accounts, vanilla/Fabric/Quilt installs, Java runtimes,
  launching, Modrinth and CurseForge clients, .mrpack and CurseForge zip import.
- Tauri shell and React UI: Home, Library, Instance, Discover, Settings.
