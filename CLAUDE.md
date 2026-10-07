# tokers

Native TikTok client in Rust: `crates/tokers` (API client library) and
`crates/tokers-gui` (the app, built on the gpui fork github.com/zxsleebu/gpui,
pinned by rev in `Cargo.toml`).

## Request signing lives outside this repository

The API only answers signed requests. This repo defines the interfaces
(`Signer`, `WebSigner`, `Backend` in `crates/tokers/src/signer.rs`) and ships
`NoSigner`; the app receives a `Backend` from the binary that links it
(`tokers_gui::run(backend)`).

Never add signing algorithms, keys, captured request templates or real device
ids to this repo, and never add a dependency (even optional) on a crate that
provides them. If a feature needs something new from the signer, extend the
traits here and leave the implementation out.

## Layout

- `crates/tokers/src/endpoints/` — one struct per endpoint implementing `Endpoint`
  (`PATH`, `params()`, `type Response`). Call with `tiktok.call(&X { .. })`;
  unmodelled paths with `tiktok.get_raw(path, &params)`.
- `crates/tokers/src/models.rs` — `#[serde(default)]` structs; the API mixes
  `"1"`/`1`, `0`/`false` and `null`, so use the deserializers in `de.rs`.
- `crates/tokers/src/template.rs` — requests are cut from a captured app request;
  `RequestTemplate::placeholder()` is a synthetic stand-in with fake ids.
- `crates/tokers/src/transport.rs` — `wreq` with a Chrome TLS fingerprint and
  per-egress pacing.
- `crates/tokers-gui` — tokio runs the requests (`tokers` is tokio-based), gpui
  tasks await the `JoinHandle`s (see `feed.rs`).

## Commands

```sh
cargo build && cargo test && cargo clippy --all-targets
cargo run -p tokers-gui        # unsigned: UI works, API calls fail with "no request signer"
```
