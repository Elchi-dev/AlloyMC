# Changelog

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
