//! HTTP with a browser TLS/HTTP2 fingerprint and per-egress pacing.
//!
//! The edge silently drops requests whose fingerprint is not browser-like: the
//! same signed request returns `200 / 0 bytes` under plain curl but a full body
//! under Chrome impersonation. So every request goes out as Chrome 120.

use std::collections::HashMap;
use std::io::Read;
use std::sync::Mutex;
use std::time::Duration;

use tokio::time::Instant;
use wreq::header::{HeaderMap, HeaderName, HeaderValue};
use wreq_util::Profile;

use crate::error::{Error, Result};
use crate::proxy::ProxyUrl;

#[derive(Clone, Debug)]
pub struct TransportConfig {
    pub timeout: Duration,
    /// Minimum gap between request starts on one egress (proxy or direct).
    /// Fresh identities get edge-throttled fast when hammered.
    pub min_interval: Duration,
    pub emulation: Profile,
}

impl Default for TransportConfig {
    fn default() -> Self {
        TransportConfig {
            timeout: Duration::from_secs(20),
            min_interval: Duration::from_secs(1),
            emulation: Profile::Chrome120,
        }
    }
}

/// How a request waits for its turn on its egress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pace {
    /// Goes at once (CDN media, pages that are not the API).
    Free,
    /// Starts at least `min_interval` after the egress's last start.
    Queued,
    /// Goes at once, past the queue (and without holding it up): something the user is
    /// waiting on right now (the comments they opened), not a prefetch. Only other urgent
    /// requests space it, by [`URGENT_GAP`]. Measured on comment/list: back to back and
    /// eight at once, no more empty answers than at 3 s apart.
    Urgent,
}

/// The least gap between two urgent requests on one egress.
pub const URGENT_GAP: Duration = Duration::from_millis(250);

impl From<bool> for Pace {
    fn from(throttle: bool) -> Self {
        if throttle { Pace::Queued } else { Pace::Free }
    }
}

/// Pacing of one egress: the last start of a queued request and of an urgent one.
#[derive(Default)]
struct Lane {
    queued: Option<Instant>,
    urgent: Option<Instant>,
}

/// One keep-alive client per egress, shared by every caller.
pub struct Transport {
    config: TransportConfig,
    min_interval: Mutex<Duration>,
    clients: Mutex<HashMap<Option<ProxyUrl>, wreq::Client>>,
    lanes: Mutex<HashMap<Option<ProxyUrl>, Lane>>,
}

#[derive(Debug)]
pub struct RawResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

impl Transport {
    pub fn new(config: TransportConfig) -> Self {
        Transport {
            min_interval: Mutex::new(config.min_interval),
            config,
            clients: Mutex::default(),
            lanes: Mutex::default(),
        }
    }

    pub fn set_min_interval(&self, interval: Duration) {
        *self.min_interval.lock().unwrap() = interval;
    }

    /// Wait for this request's start slot: queued starts on one egress are `min_interval`
    /// apart, across all tasks; urgent ones only [`URGENT_GAP`] apart from each other.
    /// Returns how long it waited.
    pub async fn wait_slot(&self, proxy: Option<&ProxyUrl>, pace: Pace) -> Duration {
        if pace == Pace::Free {
            return Duration::ZERO;
        }
        let slot = {
            let gap = match pace {
                Pace::Urgent => URGENT_GAP,
                _ => *self.min_interval.lock().unwrap(),
            };
            let mut lanes = self.lanes.lock().unwrap();
            let lane = lanes.entry(proxy.cloned()).or_default();
            let last = if pace == Pace::Urgent { &mut lane.urgent } else { &mut lane.queued };
            let now = Instant::now();
            let slot = last.map_or(now, |last| (last + gap).max(now));
            *last = Some(slot);
            slot
        };
        let began = Instant::now();
        tokio::time::sleep_until(slot).await;
        began.elapsed()
    }

    fn client(&self, proxy: Option<&ProxyUrl>) -> Result<wreq::Client> {
        let mut clients = self.clients.lock().unwrap();
        if let Some(c) = clients.get(&proxy.cloned()) {
            return Ok(c.clone());
        }
        let mut builder =
            wreq::Client::builder().emulation(self.config.emulation).timeout(self.config.timeout);
        builder = match proxy {
            Some(p) => {
                let mut px = wreq::Proxy::all(p.base()).map_err(|e| Error::Proxy(e.to_string()))?;
                if let Some((user, pass)) = p.auth() {
                    px = px.basic_auth(user, pass);
                }
                builder.proxy(px)
            }
            None => builder.no_proxy(),
        };
        let client = builder.build()?;
        clients.insert(proxy.cloned(), client.clone());
        Ok(client)
    }

    /// GET with exactly these headers (on top of the emulated browser defaults).
    pub async fn get(
        &self,
        url: &str,
        headers: &[(String, String)],
        proxy: Option<&ProxyUrl>,
        pace: impl Into<Pace>,
    ) -> Result<RawResponse> {
        self.wait_slot(proxy, pace.into()).await;
        let client = self.client(proxy)?;
        let resp = client.get(url).headers(header_map(headers)?).send().await?;
        let status = resp.status().as_u16();
        let mut body = resp.bytes().await?.to_vec();
        if body.starts_with(&[0x1f, 0x8b]) {
            let mut out = Vec::new();
            flate2::read::GzDecoder::new(&body[..]).read_to_end(&mut out)?;
            body = out;
        }
        Ok(RawResponse { status, body })
    }

    /// GET and parse JSON. An empty body is [`Error::EmptyBody`].
    pub async fn get_json(
        &self,
        url: &str,
        headers: &[(String, String)],
        proxy: Option<&ProxyUrl>,
        pace: impl Into<Pace>,
    ) -> Result<serde_json::Value> {
        let resp = self.get(url, headers, proxy, pace).await?;
        if resp.body.is_empty() {
            // the edge's silent "no": a bad signature, a rejected identity or a random drop.
            // The path only: the query carries the signatures and the device ids.
            let path = url.split('?').next().unwrap_or(url);
            log::warn!("empty response (HTTP {}) from {path}", resp.status);
        }
        parse_json(resp)
    }
}

pub fn parse_json(resp: RawResponse) -> Result<serde_json::Value> {
    if resp.body.is_empty() {
        return Err(Error::EmptyBody { status: resp.status });
    }
    let text = String::from_utf8_lossy(&resp.body);
    serde_json::from_str(&text).map_err(|source| Error::Json {
        status: resp.status,
        head: text.chars().take(120).collect(),
        source,
    })
}

fn header_map(headers: &[(String, String)]) -> Result<HeaderMap> {
    let mut map = HeaderMap::with_capacity(headers.len());
    for (k, v) in headers {
        let name = HeaderName::from_bytes(k.as_bytes())
            .map_err(|e| Error::InvalidArgument(format!("header name {k:?}: {e}")))?;
        let value =
            HeaderValue::from_str(v).map_err(|e| Error::InvalidArgument(format!("header {k}: {e}")))?;
        map.insert(name, value);
    }
    Ok(map)
}
