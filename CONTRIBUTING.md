# Contributing

- `cargo test -p alloy-core` before pushing; CI runs it on Linux and Windows.
- Minecraft logic goes in `crates/alloy-core`, never in `src-tauri`.
- New UI follows `docs/MOTION.md`: tokens, not ad-hoc durations.
- Record decisions that change direction in `docs/DECISIONS.md`.
- Commit messages: imperative, short subject, why in the body.
