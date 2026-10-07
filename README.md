# tokers

Native TikTok client for Linux, written in Rust with [gpui](https://github.com/zxsleebu/gpui).

```
crates/
  tokers/       API client library: endpoints, typed models, HTTP transport
  tokers-gui/   the app (gpui)
```

## Request signing is not part of this repository

TikTok's API only answers requests carrying app signatures. This repository
contains the client and the UI, not the signing implementation: requests are
signed through the `tokers::Signer` / `tokers::WebSigner` traits, and the app
receives a `tokers::Backend` (request template + signers) from the binary that
links it.

A build from these sources runs, but every API call fails with
"this build has no request signer". Official builds are published under Releases.

## Build

```sh
cargo run -p tokers-gui        # the app (unsigned)
cargo test
```

Linux build dependencies:
- gpui: wayland, libxkbcommon, xcb, fontconfig, freetype, vulkan loader;
- video: GStreamer 1.22+ with gst-plugins-base/good and gst-libav (or VA-API) for H.264/H.265;
- images: libheif (avatars are served as HEIC only).

## Library

```rust
let tiktok = backend.tiktok().build();          // backend: tokers::Backend
let page = tiktok.feed(&Feed::default()).await?;
let next = tiktok.feed(&Feed { max_cursor: page.max_cursor, ..Feed::default() }).await?;
let comments = tiktok.comments(&page.aweme_list[0].aweme_id, 0, 20).await?;
```

`TikTok` is `Clone + Send + Sync`; clones share connections and request pacing.
The UI runs requests on a tokio runtime and awaits them from gpui tasks
(`crates/tokers-gui/src/feed.rs`).

### Adding an endpoint

A struct in `crates/tokers/src/endpoints/` implementing `Endpoint` (`PATH`,
`params()`, `type Response`); responses are `#[serde(default)]` structs with the
fields you need (lenient deserializers in `de.rs`). Then
`tiktok.call(&MyEndpoint { .. })`. Unmodelled endpoints: `tiktok.get_raw(path, &params)`.
