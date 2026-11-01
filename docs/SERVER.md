# Alloy server

`alloy-server` is closed source and lives in its own private repository,
[Elchi-dev/alloy-server](https://github.com/Elchi-dev/alloy-server): one Go binary behind Caddy
on kvm-45.

## Running it

- It serves **alloymc.dev** (the AlloyMC home) and **launcher.alloymc.dev** (Alloy's site,
  downloads and `/api/v1/releases/latest`), and a 404 page on every other subdomain.
- It listens on `127.0.0.1:13100`; AlloyMC owns ports 13100 to 13109 on kvm-45.
- Caddy serves `alloymc.dev, *.alloymc.dev` from `/etc/caddy/conf.d/60-alloymc.caddy`, with one
  wildcard certificate over DNS-01. Cloudflare is DNS only.
- Every push to its `main` deploys itself: CI signs the build with an Ed25519 key that only
  exists in that repository's secrets, and the server accepts only signed, newer builds.
- First-time setup: `docs/SETUP.md` in that repository.

## What it provides

| Feature | Notes |
| --- | --- |
| Alloy accounts | Opt-in. Email + password (Argon2) and "Continue with Google". Needs a domain and a privacy notice for Google's consent screen. |
| CurseForge proxy | Live at `api.alloymc.dev/v1/curseforge/*`. Builds without a key call the server; the key never ships in a binary. A fixed list of read-only Minecraft calls, cached in memory for everyone. |
| Sync | Settings, instance list and screenshots between your PCs |
| Social | Posts with screenshots (world, coordinates, pack attached), likes, comments, follows, feed, profiles with 3D skins, presence |
| Share links | `alloymc.dev/p/<pack>` opens in Alloy (`alloy://`) and previews in Discord |
| Claude assistant | The server holds the API key; per-user daily and monthly budgets, rate limits, an admin view. Tools run inside each user's Alloy, so files never pass through the server. |
| Play-together relay | Open a singleplayer world to friends without port forwarding |
| Releases | Update channel for Alloy's self-updater |
