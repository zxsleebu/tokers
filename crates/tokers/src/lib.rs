//! TikTok client library.
//!
//! Layers, bottom up:
//!   * [`template`]   — the captured app request (URL params + headers) every mobile
//!     request is cut from; [`identity`] picks whose device ids it carries.
//!   * [`signer`]     — the signing interfaces ([`Signer`], [`WebSigner`]) and
//!     [`Backend`] (template + signers), which a front end receives from outside.
//!   * [`transport`]  — HTTP with a browser TLS fingerprint, per-egress pacing.
//!   * [`TikTok`]     — the client: `call(&endpoint)` for any [`Endpoint`], plus
//!     one convenience method per endpoint. Cheap to clone, `Send + Sync`.
//!   * [`endpoints`] / [`models`] — typed requests and responses.
//!   * [`web`]        — the www.tiktok.com API (signed query + msToken).
//!
//! Adding an endpoint = one struct implementing [`Endpoint`] (path, params,
//! response type). Raw access for anything unmodelled: [`TikTok::get_raw`].

pub mod client;
pub mod de;
pub mod endpoints;
pub mod error;
pub mod identity;
pub mod models;
pub mod params;
pub mod proxy;
pub mod signer;
pub mod template;
pub mod transport;
pub mod web;

pub use client::{ClientConfig, TikTok, TikTokBuilder};
pub use endpoints::Endpoint;
pub use error::{Error, Result};
pub use identity::Identity;
pub use params::Params;
pub use proxy::ProxyUrl;
pub use signer::{Backend, NoSigner, Signer, WebSigner};
pub use template::{PreparedRequest, RequestTemplate};
