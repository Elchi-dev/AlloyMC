# Updates

Alloy updates in place with one click. Nothing to uninstall; your instances, worlds,
accounts and settings stay where they are.

## How it works

1. From 0.3.1, Alloy asks AlloyMC (`api.alloymc.dev/v1/launcher/releases/latest`) for the
   newest release 20 seconds after it starts and every 6 hours, and downloads the update
   file for your system from the GitHub release into `<data>/updates`. Turn this off in
   Settings → Updates, or press **Check now** there. A Linux build installed by a package
   manager skips this; update it through the package manager.
   You can still download `Alloy_<version>_<platform>.alloyupdate` from the
   [releases](https://github.com/Elchi-dev/AlloyMC/releases) yourself, into your Downloads
   folder, onto Alloy's window, or picked in Settings → Updates.
2. Alloy verifies it (the hash AlloyMC announced, then both signatures) and shows a gold
   update button in the sidebar.
3. Click **Restart and update**. Alloy backs up its records, hands over to the installer in
   update mode, and starts again on the new version. A toast confirms it.

AlloyMC only says where the newest file is. Alloy fetches only from the AlloyMC GitHub
releases and installs only what the release key signed, so a compromised server can't ship
code.

## Why it is safe

| Check | Protects against |
| --- | --- |
| The manifest is signed (minisign, Ed25519) | someone editing the version, notes or hash |
| The installer is signed with the same key | a swapped or modified installer |
| The installer's SHA-256 matches the signed manifest | a damaged download |
| The platform must match this build | a Linux file on Windows, and the reverse |
| Only newer versions are offered | accidental downgrades (allowed only when you confirm) |
| The installer is re-checked after extraction | disk errors while staging |
| Settings, accounts and instance records are backed up first | a bad update; the last five backups are kept in `backups/` |
| A pending marker is checked on the next start | an installer that never finished; Alloy tells you and lets you retry |

The public key is compiled into Alloy (`crates/alloy-core/src/updates.rs`). The private key
exists only in the private repository's secrets, so only release builds can sign updates.

## Platforms

| Build | Update path |
| --- | --- |
| Windows installer | `.alloyupdate` → NSIS installer in passive update mode (`/P /UPDATE /R`) |
| Linux AppImage | `.alloyupdate` → the AppImage is replaced atomically, then restarted |
| Arch package | `makepkg -si` in `packaging/arch` (pacman owns the files) |

Installing a newer setup `.exe` over the old one also works; it is the same installer.
