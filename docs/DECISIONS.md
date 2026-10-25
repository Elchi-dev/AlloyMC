# Decisions

Short records of choices that shape Alloy, with what was rejected and why.

## ADR-001: Rust core, Tauri shell, React UI

**Decision:** a Tauri 2 app whose logic lives in a separate, UI-free Rust crate.
**Why:** native speed and small installers; the core is testable and reusable (a CLI, the
server). Modrinth App proves the stack.
**Rejected:** Electron (size, memory); putting logic in Tauri commands (untestable).

## ADR-002: Sign in with the official launcher's client ID

**Decision:** default to `00000000402b5328` on `login.live.com`, in a private webview that
reads the code from the redirect. A custom Azure ID can be set in Settings.
**Why:** Microsoft's Minecraft API approval is slow and opaque; this ID is widely used.
**Rejected:** device-code flow (does not work with this ID); a local HTTP callback server.
**Risk:** Microsoft could block the ID; the custom ID is the fallback.

## ADR-003: Only the refresh token is a secret on disk

**Decision:** the keychain holds `{refresh_token, client}`; the Minecraft token lives in
memory and is refreshed when it has under 5 minutes left.
**Why:** Windows Credential Manager caps entries at 2560 bytes; short-lived tokens do not
need to persist.

## ADR-004: Textures come from Mojang at install time

**Decision:** the installer (NSIS hook, PKGBUILD) runs `alloy --fetch-assets`, which reads
26 block PNGs out of the client jar with HTTP range requests (116 KB). The app fetches them
itself if they are missing.
**Rejected:** bundling them (that would redistribute Mojang's art once Alloy is shared);
downloading the whole jar (41 MB).

## ADR-005: Trust file sizes on install, hash on repair

**Decision:** existing files whose size matches are trusted; Repair re-hashes everything.
Assets are content-addressed by hash, so a size match is a strong signal.
**Measured:** a second instance of 26.2 went from 47.2 s to 58 ms.

## ADR-006: Art is traced from the original, not reinterpreted

**Decision:** the logo and background are traced to vectors from the provided artwork, with
the copper recoloured to gold. The monument is rebuilt from exact polygons; the scene's soft
light is a blurred raster layer.
**Why:** the artwork is the design; a redraw looked worse. Vectors give 4K sharpness and
animatable parts.

## ADR-007: No backdrop blur, no blend modes for effects

**Decision:** the forge is pre-blurred at build time; molten highlights use plain light
fills, not `mix-blend-mode`.
**Why:** WebKitGTK (Linux) renders live blur slowly and ignores blend modes on SVG
children. Both looks then match on Windows and Arch.

## ADR-008: JSON files now, SQLite later

**Decision:** `instance.json` per instance and a content index per instance.
**Why:** human-readable, survives partial failure, enough for Phase 0. SQLite arrives with
caching of project metadata (Phase 1).

## ADR-009: WebGL for the molten hover, with a checked fallback

**Decision:** the Play button's molten face is a WebGL shader mounted only while hot. If the
program fails to link or a test pixel reads back empty, a CSS gradient version takes over.
**Why:** some drivers report WebGL and draw nothing.
