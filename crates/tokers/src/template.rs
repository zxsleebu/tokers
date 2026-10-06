//! The captured app request every mobile request is cut from.
//!
//! The signer signs any URL, but the edge only answers requests shaped like the
//! app's: ~74 query params and ~21 headers taken from a live `v2/comment/list`
//! capture. Endpoint requests keep the template's device/app params (from
//! `device_platform` on) and headers, with fresh `ts`/`_rticket`/`x-ss-req-ticket`.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::identity::Identity;
use crate::params::Params;

/// Synthetic stand-in with the shape of a real capture and fake ids. Requests
/// built from it are well-formed but the edge will not answer them; a real
/// capture comes with the signing backend.
const PLACEHOLDER: &str = r#"{
  "url": "https://api16-normal-useast5.tiktokv.us/aweme/v2/comment/list/?aweme_id=0&cursor=0&count=20&author_id=1&aweme_type=0&device_platform=android&os=android&channel=googleplay&aid=1233&app_name=musical_ly&_rticket=0&ts=0&iid=1000000000000000001&device_id=1000000000000000002",
  "cin": {"accept-encoding": "[gzip", "sdk-version": "[2", "user-agent": "[tokers-placeholder]", "x-ss-req-ticket": "0"}
}"#;

/// Template params that describe the *captured video*, not the request. A stale
/// `author_id` makes comment/list answer status=5 with no comments for any other
/// video; the rest are dropped for the same reason.
pub const VIDEO_BOUND_PARAMS: &[&str] =
    &["author_id", "aweme_type", "comment_top_word", "comment_top_word_id", "suggest_words", "shown_cnt"];

/// First param of the device/app block shared by every endpoint.
const COMMON_PARAMS_START: &str = "device_platform";

#[derive(Clone, Debug)]
pub struct RequestTemplate {
    origin: String,
    path: String,
    params: Params,
    headers: Vec<(String, String)>,
}

/// A request ready to sign and send.
#[derive(Clone, Debug)]
pub struct PreparedRequest {
    /// `https://host`
    pub origin: String,
    pub path: String,
    pub params: Params,
    /// Without signatures and without User-Agent.
    pub headers: Vec<(String, String)>,
    pub user_agent: String,
}

impl PreparedRequest {
    pub fn query(&self) -> String {
        self.params.encode()
    }

    pub fn url(&self) -> String {
        format!("{}{}?{}", self.origin, self.path, self.query())
    }

    /// X-Khronos: the `ts` the URL carries.
    pub fn khronos(&self) -> u64 {
        self.params.get("ts").and_then(|v| v.parse().ok()).unwrap_or(0)
    }
}

impl RequestTemplate {
    /// See [`PLACEHOLDER`].
    pub fn placeholder() -> Self {
        Self::from_capture_json(PLACEHOLDER).expect("placeholder template is valid")
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let raw = std::fs::read_to_string(path.as_ref())?;
        Self::from_capture_json(&raw)
    }

    /// `{"url": "...", "cin": {header: value}}` (signer-input capture; values may
    /// carry the Java `[...]` list brackets) or `{"url", "headers"}`.
    pub fn from_capture_json(raw: &str) -> Result<Self> {
        #[derive(Deserialize)]
        struct Capture {
            url: String,
            #[serde(alias = "headers")]
            cin: serde_json::Map<String, serde_json::Value>,
        }
        let cap: Capture =
            serde_json::from_str(raw).map_err(|e| Error::Template(format!("bad capture json: {e}")))?;
        let (origin, rest) = split_origin(&cap.url)
            .ok_or_else(|| Error::Template(format!("not an absolute URL: {}", cap.url)))?;
        let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
        let headers = cap
            .cin
            .into_iter()
            .map(|(k, v)| {
                let v = match v {
                    serde_json::Value::String(s) => s,
                    other => other.to_string(),
                };
                (k, strip_java_list(&v))
            })
            .collect();
        Ok(RequestTemplate {
            origin: origin.to_string(),
            path: path.to_string(),
            params: Params::parse(query),
            headers,
        })
    }

    pub fn origin(&self) -> &str {
        &self.origin
    }

    pub fn params(&self) -> &Params {
        &self.params
    }

    pub fn user_agent(&self) -> &str {
        self.header("user-agent").unwrap_or_default()
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    /// `aweme/v2/comment/list`: the captured request itself, with the video-bound
    /// params dropped and aweme_id/cursor/count replaced.
    pub fn comment_list(
        &self,
        aweme_id: &str,
        cursor: u64,
        count: u32,
        identity: &Identity,
        now_ms: u64,
    ) -> PreparedRequest {
        let mut params = self.params.clone();
        for key in VIDEO_BOUND_PARAMS {
            params.remove(key);
        }
        params.set("aweme_id", aweme_id);
        params.set("cursor", cursor.to_string());
        params.set("count", count.to_string());
        self.finish(self.path.clone(), params, identity, now_ms)
    }

    /// Any endpoint: its own params first, then the template's common device/app
    /// params (which win on shared keys).
    pub fn api(&self, path: &str, params: &Params, identity: &Identity, now_ms: u64) -> PreparedRequest {
        let params = params.merged(&self.params.from_key(COMMON_PARAMS_START));
        self.finish(path.to_string(), params, identity, now_ms)
    }

    fn finish(&self, path: String, mut params: Params, identity: &Identity, now_ms: u64) -> PreparedRequest {
        params.set("ts", (now_ms / 1000).to_string());
        params.set("_rticket", now_ms.to_string());
        if let Some((device_id, iid)) = identity.ids() {
            params.set("device_id", device_id);
            params.set("iid", iid);
        }
        let mut headers: Vec<(String, String)> = self
            .headers
            .iter()
            .filter(|(k, _)| {
                !k.eq_ignore_ascii_case("accept-encoding") && !k.eq_ignore_ascii_case("user-agent")
            })
            .cloned()
            .collect();
        match headers.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case("x-ss-req-ticket")) {
            Some(slot) => slot.1 = now_ms.to_string(),
            None => headers.push(("x-ss-req-ticket".into(), now_ms.to_string())),
        }
        PreparedRequest {
            origin: self.origin.clone(),
            path,
            params,
            headers,
            user_agent: self.user_agent().to_string(),
        }
    }

    /// Header map view (diagnostics).
    pub fn headers(&self) -> BTreeMap<&str, &str> {
        self.headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect()
    }
}

fn split_origin(url: &str) -> Option<(&str, &str)> {
    let scheme_end = url.find("://")? + 3;
    let path_start = url[scheme_end..].find('/').map_or(url.len(), |i| scheme_end + i);
    Some((&url[..path_start], &url[path_start..]))
}

/// `"[value"` / `"[value]"` -> `"value"` (Java `List.toString` residue).
fn strip_java_list(v: &str) -> String {
    let v = v.strip_prefix('[').unwrap_or(v);
    match v.strip_suffix(']') {
        Some(inner) => inner.trim_end_matches(']').trim().to_string(),
        None => v.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_790_972_388_123;

    #[test]
    fn api_request_keeps_template_order() {
        let tpl = RequestTemplate::placeholder();
        let params = Params::new().with("count", 6).with("aid", 1).with("keyword", "a b*~é");
        let req = tpl.api("/aweme/v1/feed/", &params, &Identity::Template, NOW);
        assert_eq!(
            req.url(),
            "https://api16-normal-useast5.tiktokv.us/aweme/v1/feed/?count=6&aid=1233&keyword=a+b%2A~%C3%A9\
             &device_platform=android&os=android&channel=googleplay&app_name=musical_ly\
             &_rticket=1790972388123&ts=1790972388&iid=1000000000000000001&device_id=1000000000000000002"
        );
        assert_eq!(req.khronos(), 1_790_972_388);
        assert_eq!(req.user_agent, "tokers-placeholder");
        assert_eq!(
            req.headers,
            vec![("sdk-version".into(), "2".into()), ("x-ss-req-ticket".into(), "1790972388123".into())]
        );
    }

    #[test]
    fn comment_list_drops_video_bound_params() {
        let tpl = RequestTemplate::placeholder();
        let req = tpl.comment_list("777", 50, 50, &Identity::device("1", "2"), NOW);
        assert_eq!(
            req.query(),
            "aweme_id=777&cursor=50&count=50&device_platform=android&os=android&channel=googleplay\
             &aid=1233&app_name=musical_ly&_rticket=1790972388123&ts=1790972388&iid=2&device_id=1"
        );
    }
}
