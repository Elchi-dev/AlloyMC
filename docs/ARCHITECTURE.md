# Architecture

Alloy is a Cargo workspace with two crates and a React frontend. The rule: **all Minecraft
logic lives in `alloy-core`, which knows nothing about Tauri.** The app shell only wires it
to the window, the keychain and the UI.

```
React UI (TypeScript) ── invoke() / events ──► src-tauri (app shell) ──► crates/alloy-core
                                                                              │
                                     Mojang · Modrinth · CurseForge · Fabric/Quilt meta · Microsoft/Xbox
```

## crates/alloy-core

| Module | Does |
| --- | --- |
| `auth` | Microsoft sign-in chain (MS → Xbox Live → XSTS → Minecraft), offline accounts, `SecretStore` trait |
| `meta` | Mojang manifest and version JSON, rules, library resolution, loader version lists; `meta::forge` runs Forge and NeoForge installers |
| `install` | Client jar, libraries, natives, assets, logging config; quick (size) or repair (hash) verification |
| `java` | Mojang's Java runtimes per version component; system Java detection |
| `launch` | Argument building, legacy natives, process spawn with line-by-line output |
| `content` | Modrinth and CurseForge clients and the content engine: `jarmeta` (what jars say), `annotations` (the `@Mod` ids Forge 1.12 really loads, read from class files), `scan` (cached per file, writes `alloy.lock`), `check` (pre-launch check and dependency graph), `manage` (installs, updates, fixes), `identify` (names unknown files), `listed` and `blocked` (files packs list, CurseForge-only downloads), `links` |
| `system` | Installed memory and other machine facts |
| `modpack` | `.mrpack` and CurseForge `.zip` import from file or URL |
| `textures` | Block textures read from Mojang's client jar with HTTP range requests |
| `alloymc` | AlloyMC account: browser sign-in (PKCE, loopback redirect), profile cache, token in the `SecretStore`, Minecraft linking through Mojang's session join |
| `crash` | The crash doctor: gathers the crash report, `latest.log`, `hs_err` and the output tail; names the kind of crash and the mods it points at, with fixes |
| `bisect` | Finds the mod behind a startup crash by starting the game with halves of the mods off; requirements come along; restores mods even after a crash of Alloy |
| `savepoint` | Save points: the setup folders of an instance, stored by SHA-1 under `<data>/savepoints`, restore with an automatic undo point, garbage collection |
| `export` | `.mrpack` and CurseForge `.zip` export: files listed by reference where the platform knows them, the rest bundled |
| `importers` | Finds and copies instances of Prism Launcher, MultiMC, the CurseForge App and the Modrinth App (SQLite `app.db`, read-only) |
| `assistant` | The assistant's side in Alloy: the relay client (streams Claude's reply from AlloyMC and assembles whole content blocks), guarded file access for its tools (reads in configs, logs and crash reports; writes to configs only), saved conversations |
| `serverpack` | A dedicated server from an instance: the vanilla, Fabric, Quilt, Forge or NeoForge server, mods without client-only ones (jar `environment`, Modrinth `server_side`, a short list of renderers), configs and scripts, start scripts, `eula.txt` |
| `download` | Parallel, retrying, hash-checked downloads written via `.part` + rename |

`examples/` holds runnable checks: `smoke` (create, install, launch any loader), `pack`
(install, scan and check a Modrinth pack), `check`, `content` (install, fixes, updates,
blocked files), `launch`, `bench`, `textures`, `curseforge`, `fingerprint`, `update` (ask AlloyMC for a release, download and verify it), `cfpack` (install a CurseForge pack file and check it), `serverpack` (make an instance, build its server and start it until it's ready), `export` (export as both formats and import back), `doctor` (a real crash diagnosed and bisected; needs a display or
xvfb-run), `alloymc` (sign in
to a running alloy-server and back out).

## src-tauri

`commands.rs` (instances, launching, accounts) and `content.rs` (everything inside
instances) are the IPC surface. `jobs.rs` runs long work as jobs the downloads panel lists,
cancellable and retryable. Long jobs report through events:

| Event | Payload |
| --- | --- |
| `job-progress` | `{ jobId, instanceId?, progress: { stage, current, total, unit } }` |
| `game-state` | `{ instanceId, state: starting / running / exited, exitCode?, error? }` |
| `game-log` | `{ instanceId, lines: [{ stream, line }] }`, batched ten times a second |
| `curseforge-needed` | `{ instanceId, count }` right after a pack import leaves files only CurseForge's website hands out |
| `jobs-changed` | the full job list |
| `net-speed` | `{ bytesPerSec }`, twice a second while anything downloads |
| `blocked-collected` | `{ instanceId, titles }` when a CurseForge file arrived in Downloads |
| `savepoints-changed` | none; the Save points tab refetches |
| `crash-diagnosis` | `{ instanceId, diagnosis }` after a run that crashed |
| `assistant-delta` | `{ requestId, delta: { type: text, index, text } or { type: tool, index, id, name } }` while Claude writes |
| `bisect-progress` | `{ instanceId, step? , found?, error? }` while the crash doctor tests |
| `instances-changed`, `accounts-changed`, `alloymc-changed`, `content-changed`, `blocked-changed`, `textures-ready` | none; the UI refetches |

`ms_login.rs` opens Microsoft's page in a private webview and reads the auth code from the
redirect. `alloymc.rs` signs in to AlloyMC the other way round: the person's own browser
(Google blocks embedded webviews) with the code coming back to a one-shot listener on
127.0.0.1 (RFC 8252). `assistant.rs` streams the assistant's replies and gives its file tools their guarded access;
`doctor::test_launch` starts an instance once to see if it reaches the title screen.
`secrets.rs` stores refresh tokens in the OS keychain with a file fallback.
`alloy --fetch-assets [dir]` runs headless for installers.

## Frontend (src/)

- `lib/api.ts`: typed command wrappers; falls back to `lib/mock.ts` outside Tauri.
- `lib/queries.ts`: TanStack Query hooks, persisted to localStorage so the first frame has data.
- `lib/store.ts`: Zustand for navigation, progress, game state, logs, toasts, textures.
- `lib/assistant.ts`: the assistant's conversation loop and its tools. Tools run through the
  same commands as Alloy's buttons; tools that change something wait for the player to
  allow them, and the first change to an instance in a conversation makes a save point.
  AlloyMC's server holds the key, the system prompt and the tool definitions, and meters use.
- `components/`: `Assistant` (the panel), `Backdrop` (forge scene), `Logo`, `PlayButton` + `MoltenFx`, `ori/Ori`.
- `pages/`: Home and Library load eagerly; Instance, Discover and Settings lazily, prefetched when idle.

## On disk

```
<data>/                         %APPDATA%/Alloy, ~/.local/share/alloy, or $ALLOY_DATA_DIR
  settings.json  accounts.json  secrets.json (fallback only)
  alloymc.json                    the AlloyMC profile last seen (the token is in the keychain)
  meta/  versions/  libraries/  assets/  natives/  java/  textures/
  instances/<id>/instance.json  + the game folder (mods/, saves/, ...)
  instances/<id>/alloy.lock               every file with its hash and source
  instances/<id>/.launcher/content.json   what each installed file is and where it came from
  instances/<id>/.launcher/scan.json      cached jar metadata and hashes
  instances/<id>/.launcher/blocked.json   CurseForge files waiting in Downloads
  instances/<id>/.launcher/crash.json     the last crash diagnosis, until dismissed
  instances/<id>/.launcher/bisect.json    which mods were on, while the crash doctor tests
  meta/installers/                        Forge/NeoForge installers and their processor results
  assistant/<id>.json             saved assistant conversations
  trash/                          deleted instances, kept until you empty it
  savepoints/objects/             files of every save point, by SHA-1, stored once
  savepoints/instances/<id>/      one manifest per save point
```

## Art pipeline

The logo and background are traced from the original artwork, not redrawn:

- `scripts/trace_logo.py`: snaps the logo to its flat colours, traces straight-edged
  polygons, recolours copper to gold, tags the halves for animation.
- `scripts/trace_background.py`: traces the scene, rebuilds the monument with exact
  polygons, adds back the original's soft light as a blurred layer, renders a 4K webp,
  and writes the molten and fire shapes to an overlay SVG the UI animates.

Sources live in `assets-src/`.
