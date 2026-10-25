# Updates

Alloy updates in place with one click. Nothing to uninstall; your instances, worlds,
accounts and settings stay where they are.

## How it works today

1. Download `Alloy_<version>_windows-x86_64.alloyupdate` from the
   [releases](https://github.com/Elchi-dev/AlloyMC/releases) into your Downloads folder
   (or drop it onto Alloy's window, or pick it in Settings → Updates).
2. Alloy notices it when it opens or comes back into focus, verifies it, and shows a gold
   update button in the sidebar.
3. Click **Restart and update**. Alloy backs up its records, hands over to the installer in
   update mode, and starts again on the new version. A toast confirms it.

When the Alloy server exists, step 1 happens by itself; everything after stays the same.

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
