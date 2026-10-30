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
| `content` | Modrinth and CurseForge clients and the content engine: `jarmeta` (what jars say), `scan` (cached per file, writes `alloy.lock`), `check` (pre-launch check and dependency graph), `manage` (installs, updates, fixes), `identify` (names unknown files), `listed` and `blocked` (files packs list, CurseForge-only downloads), `links` |
| `system` | Installed memory and other machine facts |
| `modpack` | `.mrpack` and CurseForge `.zip` import from file or URL |
| `textures` | Block textures read from Mojang's client jar with HTTP range requests |
| `download` | Parallel, retrying, hash-checked downloads written via `.part` + rename |

`examples/` holds runnable checks: `smoke` (create, install, launch any loader), `pack`
(install, scan and check a Modrinth pack), `check`, `content` (install, fixes, updates,
blocked files), `launch`, `bench`, `textures`, `curseforge`, `fingerprint`.

## src-tauri

`commands.rs` (instances, launching, accounts) and `content.rs` (everything inside
instances) are the IPC surface. `jobs.rs` runs long work as jobs the downloads panel lists,
cancellable and retryable. Long jobs report through events:

| Event | Payload |
| --- | --- |
| `job-progress` | `{ jobId, instanceId?, progress: { stage, current, total, unit } }` |
| `game-state` | `{ instanceId, state: starting / running / exited, exitCode?, error? }` |
| `game-log` | `{ instanceId, stream, line }` |
| `jobs-changed` | the full job list |
| `net-speed` | `{ bytesPerSec }`, twice a second while anything downloads |
| `blocked-collected` | `{ instanceId, titles }` when a CurseForge file arrived in Downloads |
| `instances-changed`, `accounts-changed`, `content-changed`, `blocked-changed`, `textures-ready` | none; the UI refetches |

`ms_login.rs` opens Microsoft's page in a private webview and reads the auth code from the
redirect. `secrets.rs` stores refresh tokens in the OS keychain with a file fallback.
`alloy --fetch-assets [dir]` runs headless for installers.

## Frontend (src/)

- `lib/api.ts`: typed command wrappers; falls back to `lib/mock.ts` outside Tauri.
- `lib/queries.ts`: TanStack Query hooks, persisted to localStorage so the first frame has data.
- `lib/store.ts`: Zustand for navigation, progress, game state, logs, toasts, textures.
- `components/`: `Backdrop` (forge scene), `Logo`, `PlayButton` + `MoltenFx`, `ori/Ori`.
- `pages/`: Home and Library load eagerly; Instance, Discover and Settings lazily, prefetched when idle.

## On disk

```
<data>/                         %APPDATA%/Alloy, ~/.local/share/alloy, or $ALLOY_DATA_DIR
  settings.json  accounts.json  secrets.json (fallback only)
  meta/  versions/  libraries/  assets/  natives/  java/  textures/
  instances/<id>/instance.json  + the game folder (mods/, saves/, ...)
  instances/<id>/alloy.lock               every file with its hash and source
  instances/<id>/.launcher/content.json   what each installed file is and where it came from
  instances/<id>/.launcher/scan.json      cached jar metadata and hashes
  instances/<id>/.launcher/blocked.json   CurseForge files waiting in Downloads
  meta/installers/                        Forge/NeoForge installers and their processor results
  trash/                          deleted instances, kept until you empty it
```

## Art pipeline

The logo and background are traced from the original artwork, not redrawn:

- `scripts/trace_logo.py`: snaps the logo to its flat colours, traces straight-edged
  polygons, recolours copper to gold, tags the halves for animation.
- `scripts/trace_background.py`: traces the scene, rebuilds the monument with exact
  polygons, adds back the original's soft light as a blurred layer, renders a 4K webp,
  and writes the molten and fire shapes to an overlay SVG the UI animates.

Sources live in `assets-src/`.
