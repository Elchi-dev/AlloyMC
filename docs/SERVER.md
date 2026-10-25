# Alloy server

One Rust binary, `alloy-server`, that you run on your VPS behind Caddy. Phase 3.

## Running it

```caddyfile
alloy.example.com {
    reverse_proxy 127.0.0.1:8737
}
```

`alloy-server` listens on `127.0.0.1` only, trusts Caddy's forwarded headers, and keeps its
data in SQLite next to the binary. WebSockets (presence, notifications) pass through Caddy
without extra setup. The server address is a field in Alloy's Settings, so changing domains
is one edit. Credentials live in `config/server-secrets.toml`.

## What it provides

| Feature | Notes |
| --- | --- |
| Alloy accounts | Opt-in. Email + password (Argon2) and "Continue with Google". Needs a domain and a privacy notice for Google's consent screen. |
| CurseForge proxy | Shared builds call the server; the key never ships in a binary. Responses are cached for everyone. |
| Sync | Settings, instance list and screenshots between your PCs |
| Social | Posts with screenshots (world, coordinates, pack attached), likes, comments, follows, feed, profiles with 3D skins, presence |
| Share links | `alloy.example.com/p/<pack>` opens in Alloy (`alloy://`) and previews in Discord |
| Claude assistant | The server holds the API key; per-user daily and monthly budgets, rate limits, an admin view. Tools run inside each user's Alloy, so files never pass through the server. |
| Play-together relay | Open a singleplayer world to friends without port forwarding |
| Releases | Update channel for Alloy's self-updater |
