# Security

- Microsoft refresh tokens live in the OS keychain; the short-lived Minecraft token stays in memory.
- Paths from modpacks and zips are joined with `safe_join`, which rejects `..` and absolute paths.
- Downloads are hash-checked when a hash is known and written via `.part` + rename.
- Report issues privately to samuel@elchi.dev.
