# Changelog

## 0.4.0

**Claude, inside Alloy.** Open the assistant from the rail (or Ctrl+J), the crash doctor,
the problems dialog or an instance's right-click menu, and ask about your Minecraft.

- **It looks before it answers.** Claude can see your instances and everything in them,
  the pre-launch check and its fixes, the crash doctor's diagnosis, the game's log and
  config files, and search Modrinth and CurseForge filtered to your version and loader.
- **It asks before it changes anything.** Creating an instance, installing a mod with what
  it needs, turning mods on or off, applying Alloy's fixes, updating, setting memory,
  editing a config (shown as a diff) and a test start each show a card first: Allow or
  Decline. Deleting always asks. The first change to an instance makes a save point.
- **Quick or Think harder.** Quick answers with Claude Haiku; Think harder uses Claude
  Sonnet for knotty crashes and pack building.
- **Free with an AlloyMC account** (confirmed email, linked Minecraft account). Each person
  has a daily and monthly share, shown under the message box; the key and the budget stay
  on AlloyMC's server, your files stay on your computer except what a tool's answer says.
- **Conversations are kept** on this computer; pick up an earlier one from the history.

Fixes:

- **Long mod lists no longer go blank.** In packs with more than 60 mods, scrolling past the
  first screenful showed empty space instead of the rest of the list.
- **No more "installed twice" in 1.12.2 packs that start fine.** Forge 1.12 loads the mods
  its classes declare (`@Mod`); `mcmod.info` only describes them, and copied ones are
  common: DragonMurder ships PlaneFix's, so SkyFactory 4 showed PlaneFix twice. Alloy now
  reads the classes too. SkyFactory 4.2.4 and RLCraft 2.9.3 check clean.
- **The CurseForge window is offered as soon as a pack is unpacked**, not only after the
  game files are installed (which can take minutes for Forge), and the offer closes by
  itself if the files turn up in your Downloads folder in the meantime.
- **Less lag while Alloy works.** The game's output reaches the window in batches ten
  times a second instead of an event per line (a modded start prints thousands), and
  download progress updates eight times a second instead of sixteen.

## 0.3.3

- **The CurseForge window downloads on Windows.** It used to show CurseForge's
  "Downloading…" and then nothing: Windows' webview holds a `.jar` back as a file that "can
  harm your device", with no one there to answer. Alloy now takes the file request
  CurseForge's page makes and fetches that file itself, the same way on every system,
  then checks it against CurseForge's hash as before.
- **No more false errors in Forge 1.12.2 packs.** Many 1.12.2 jars carry a leftover
  `mods.toml` for a newer Forge next to the `mcmod.info` their own Forge reads, so RLCraft
  showed three errors (EnhancedVisuals wanting CreativeCore 2, Spartan Shields wanting
  Forge 25 and Minecraft 1.13.2). Forge instances now read `mcmod.info` below 1.13 and
  `mods.toml` from 1.13 on. RLCraft 2.9.3 checks clean.

## 0.3.2

Fixes from your first days with 0.3, and the CurseForge window.

- **Files only CurseForge's website hands out, fetched in Alloy.** After an install that
  leaves such files, Alloy offers to open them in a CurseForge window: it shows each file's
  real page, catches the download CurseForge starts, checks it against CurseForge's hash,
  adds it and moves on, then closes when they're all in. Downloading in your own browser
  still works too.
- **No more false problems in working packs.** The check before launch now follows what the
  loader really loads. Fabric and Quilt use the newest copy of a mod and ignore the rest,
  so two copies are a note, not an error, and old copies bundled inside other mods (like
  the Fabric API inside CoroUtil) no longer count against the real one. Homestead, which
  starts fine, showed three errors before; now it shows one note.
- **The launcher no longer goes blank while the game runs.** Restoring it mid-game showed
  an empty window: pausing animations to save GPU time also froze things on their first,
  invisible frame. Only decorative loops pause now.
- **Right-click menus everywhere:** instances, mods (or a whole selection), Discover
  results, log lines, save points, text fields and links.
- **About this pack.** Instances from a Modrinth or CurseForge pack link to its page in
  Alloy, with description, gallery and versions. Packs imported from a file are found by
  their hash or name.

## 0.3.1

The last update you install by hand.

- **Updates arrive by themselves.** Alloy asks AlloyMC for the newest version when it
  starts and every six hours, downloads it in the background and shows the gold update
  button; installing still waits for your click. Every file is checked against the hash
  AlloyMC announced and against Alloy's release key before it's offered, exactly like a
  file you download yourself. Turn it off, or check right away, in Settings → Updates.

## 0.3.0

Alloy and AlloyMC meet: sign in, link Minecraft, CurseForge without a key. Plus a crash
doctor, save points, modpack export and imports from other launchers. Installs as a
one-click update from 0.2.x.

- **Sign in to AlloyMC.** The account menu and Settings have an optional AlloyMC sign-in.
  It opens your own browser, so Google and saved passwords work, and comes back to Alloy
  by itself. Alloy shows your profile and keeps working offline with what it last saw.
- **Link your Minecraft accounts.** Any Microsoft account in Alloy links to your AlloyMC
  profile in one click: Alloy proves you own it through Mojang's session server, the same
  check a multiplayer server does. Unlink them in Settings.
- **The crash doctor.** When the game crashes, Alloy reads the crash report, the log and
  any JVM error file and says what happened in plain words: which mod it points at and
  how sure it is (the loader naming it, a failed mixin, its code at the top of the crash),
  or that memory ran out, the wrong Java was used or the graphics driver crashed. One
  click turns the mod off, gives the game more memory or switches Java. An instance's
  page shows its last crash until it's dealt with.
- **Find the mod by testing.** When nothing points at one mod, Alloy starts the game with
  halves of the mods turned off (each mod brings what it requires) and closes it at the
  title screen, until one mod is left: about log2(mods) + 1 starts. Crashes that take two
  mods together are found too. Your mods are put back afterwards, even if Alloy is closed
  halfway.
- **Save points.** A copy of an instance's mods, configs, resource packs, shaders and
  settings, to go back to with one click. Alloy makes one before updating mods and before
  a crash doctor fix, and before going back, so that can be undone too. Worlds aren't
  included. Files are stored once across all save points, so they take little space.
- **Export as a modpack.** Any instance becomes a Modrinth `.mrpack` or a CurseForge
  `.zip`. Mods the platform knows are listed by reference (Modrinth by SHA-1, CurseForge
  by fingerprint), so packs stay small; anything else is bundled, and Alloy names those
  files so you can check their licences before sharing.
- **Import from other launchers.** Alloy finds instances of Prism Launcher, MultiMC, the
  CurseForge App and the Modrinth App where they live (or in a folder you choose), reads
  their version, loader and memory setting, and copies them over with their worlds. The
  originals stay as they are.
- **CurseForge for everyone.** Search, installs, updates and CurseForge modpacks work in
  every build without a key of your own. Alloy asks the AlloyMC server, which holds the key
  and caches answers for everyone. A key in Settings still makes Alloy call CurseForge
  directly.

## 0.2.1

Fixes from the first days of 0.2. Installs as a one-click update from 0.2.0 or 0.1.0.

- **Player heads show again.** Mojang hands out skin links as `http://`, which the app may
  not load, so Microsoft accounts showed an empty square. Heads now load over HTTPS and are
  drawn the way the game draws them (old 64x32 skins with a solid hat layer included). An
  account whose skin can't load shows its initial instead of nothing.
- **Downloads show progress on slow connections.** Modpack files and mods now report every
  byte instead of jumping once a whole file is done, with amounts and time left in the
  downloads panel, a percentage on the install button and the Play button, and a "Show"
  button on the "Installing" notice that opens the panel.
- **New Fabric and Quilt instances come with Fabric API**, the build made for that
  Minecraft version, shown in the New instance dialog with a box to leave it out.
- **The right loader version for each Minecraft version.** Forge now gets Forge's own
  recommended build for each version (14.23.5.2859 on 1.12.2, where the newest build is 2864). The dialog
  explains that Fabric Loader is one version for every Minecraft release and Fabric API is
  the part that changes. Versions Fabric doesn't support (1.12.2) now say so instead of
  showing an empty picker.
- Release builds keep their own build cache, so the Windows build no longer compiles
  everything from scratch.

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
