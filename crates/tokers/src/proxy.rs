//! Proxy URLs, `.env` and `proxy.txt`.
//!
//! Accepted proxy forms:
//!   `http://HOST:PORT:USER:PASS` (Evomi style; the password may contain `:`)
//!   `http://user:pass@HOST:PORT`
//!   `socks5://...` in either form.

use std::fmt;
use std::path::Path;
use std::sync::LazyLock;

use rand::Rng;
use regex::Regex;

use crate::error::{Error, Result};

pub const PROXY_ENV_KEYS: &[&str] = &["TIKTOK_PROXY", "HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"];
pub const PROXY_FILE: &str = "proxy.txt";

static SESSION_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(_session-)[A-Za-z0-9]+").unwrap());

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ProxyUrl {
    /// As given; also the pacing lane key.
    raw: String,
    /// `scheme://host:port`
    base: String,
    auth: Option<(String, String)>,
}

impl ProxyUrl {
    pub fn parse(raw: &str) -> Result<Self> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(Error::Proxy("empty proxy URL".into()));
        }
        let (scheme, rest) = raw.split_once("://").unwrap_or(("http", raw));
        let (base, auth) = if let Some((userinfo, hostport)) = rest.rsplit_once('@') {
            let (user, pass) = userinfo.split_once(':').unwrap_or((userinfo, ""));
            (format!("{scheme}://{hostport}"), Some((user.to_string(), pass.to_string())))
        } else {
            let parts: Vec<&str> = rest.splitn(4, ':').collect();
            if parts.len() == 4 {
                (
                    format!("{scheme}://{}:{}", parts[0], parts[1]),
                    Some((parts[2].to_string(), parts[3].to_string())),
                )
            } else {
                (format!("{scheme}://{rest}"), None)
            }
        };
        Ok(ProxyUrl { raw: raw.to_string(), base, auth })
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    pub fn auth(&self) -> Option<(&str, &str)> {
        self.auth.as_ref().map(|(u, p)| (u.as_str(), p.as_str()))
    }

    /// The Evomi `_session-XXXX` id, if the URL has one.
    pub fn session(&self) -> Option<&str> {
        SESSION_RE.captures(&self.raw).map(|c| &c.get(0).unwrap().as_str()["_session-".len()..])
    }

    /// Same proxy with a fresh `_session-` id (= a new exit IP). `None` when the
    /// URL has no session segment.
    pub fn rotated(&self) -> Option<ProxyUrl> {
        self.session()?;
        let id = new_session_id(9);
        let raw = SESSION_RE.replace(&self.raw, format!("${{1}}{id}").as_str()).into_owned();
        ProxyUrl::parse(&raw).ok()
    }
}

/// Never prints credentials.
impl fmt::Debug for ProxyUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.base)?;
        if let Some(s) = self.session() {
            write!(f, " session={s}")?;
        }
        Ok(())
    }
}

impl fmt::Display for ProxyUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

pub fn new_session_id(len: usize) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::rng();
    (0..len).map(|_| ALPHABET[rng.random_range(0..ALPHABET.len())] as char).collect()
}

/// Load `KEY=VALUE` lines into the process env without overriding existing
/// variables. Returns the keys that were set.
pub fn load_dotenv(path: impl AsRef<Path>) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut loaded = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let (key, mut value) = (key.trim(), value.trim());
        if value.len() >= 2 {
            let (first, last) = (value.as_bytes()[0], value.as_bytes()[value.len() - 1]);
            if first == last && (first == b'"' || first == b'\'') {
                value = &value[1..value.len() - 1];
            }
        }
        if !key.is_empty() && std::env::var_os(key).is_none() {
            // SAFETY: called once at startup, before any threads are spawned.
            unsafe { std::env::set_var(key, value) };
            loaded.push(key.to_string());
        }
    }
    loaded
}

/// First non-comment line of `proxy.txt`.
pub fn read_proxy_file(path: impl AsRef<Path>) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()?
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
}

/// Explicit value > `proxy.txt` > TIKTOK_PROXY / HTTP_PROXY / HTTPS_PROXY /
/// ALL_PROXY (upper, then lower case).
pub fn discover(explicit: Option<&str>) -> Option<String> {
    if let Some(v) = explicit.map(str::trim).filter(|v| !v.is_empty()) {
        return Some(v.to_string());
    }
    if let Some(v) = read_proxy_file(PROXY_FILE) {
        return Some(v);
    }
    PROXY_ENV_KEYS.iter().find_map(|key| {
        std::env::var(key)
            .ok()
            .or_else(|| std::env::var(key.to_lowercase()).ok())
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forms() {
        let p = ProxyUrl::parse("http://core.evomi.com:1000:user:pa:ss_session-ABC123").unwrap();
        assert_eq!(p.base(), "http://core.evomi.com:1000");
        assert_eq!(p.auth(), Some(("user", "pa:ss_session-ABC123")));
        assert_eq!(p.session(), Some("ABC123"));
        let r = p.rotated().unwrap();
        assert_ne!(r.session(), Some("ABC123"));
        assert_eq!(r.base(), p.base());

        let p = ProxyUrl::parse("socks5://u:p@1.2.3.4:1080").unwrap();
        assert_eq!(p.base(), "socks5://1.2.3.4:1080");
        assert_eq!(p.auth(), Some(("u", "p")));
        assert!(p.rotated().is_none());
    }
}
