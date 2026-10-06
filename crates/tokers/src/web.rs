//! The www.tiktok.com API: signed queries ([`WebSigner`]) plus a browser `msToken`
//! cookie (copy it from tiktok.com cookies; without it most endpoints return
//! bot-detection empty answers). Also author lookup from public video pages.

use std::sync::{Arc, LazyLock};

use regex::Regex;
use serde::Serialize;
use serde_json::Value;

use crate::client::now_ms;
use crate::endpoints::CommentPage;
use crate::error::{Error, Result};
use crate::params::Params;
use crate::proxy::ProxyUrl;
use crate::signer::WebSigner;
use crate::transport::{Transport, TransportConfig};

pub const DEFAULT_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

const PROFILE_URL: &str = "https://www.tiktok.com/api/user/detail/";
const COMMENTS_URL: &str = "https://www.tiktok.com/api/comment/list/";

/// aid/app_name/device_platform as observed on tiktok.com web calls.
const BASE_PARAMS: &[(&str, &str)] = &[
    ("aid", "1988"),
    ("app_name", "tiktok_web"),
    ("device_platform", "webapp"),
    ("version_code", "170400"),
    ("version_name", "17.4.0"),
    ("os", "windows"),
    ("screen_width", "1920"),
    ("screen_height", "1080"),
    ("browser_language", "en-US"),
    ("browser_platform", "Win32"),
    ("browser_name", "Mozilla"),
];

static SECUID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""secUid":"(MS4wLjABAAAA[^"]+)""#).unwrap());

#[derive(Clone, Debug)]
pub struct WebSession {
    pub ms_token: String,
    pub user_agent: String,
    pub extra_params: Params,
}

impl WebSession {
    pub fn new(ms_token: impl Into<String>) -> Self {
        WebSession { ms_token: ms_token.into(), user_agent: DEFAULT_UA.into(), extra_params: Params::new() }
    }

    /// Base params + msToken + session extras + call params, signed.
    pub fn signed_query(&self, signer: &dyn WebSigner, call: &Params) -> Result<String> {
        let mut params: Params = BASE_PARAMS.iter().copied().collect();
        params.set("msToken", &self.ms_token);
        let params = params.merged(&self.extra_params).merged(call);
        signer.sign_query(&params.encode(), &self.user_agent, "", (now_ms() / 1000) as u32)
    }

    pub fn headers(&self, referer: &str) -> Vec<(String, String)> {
        vec![
            ("User-Agent".into(), self.user_agent.clone()),
            ("Referer".into(), referer.into()),
            ("Accept".into(), "application/json, text/plain, */*".into()),
            ("Cookie".into(), format!("msToken={}", self.ms_token)),
        ]
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct WebProfile {
    pub unique_id: String,
    pub nickname: String,
    pub sec_uid: String,
    pub open_id: String,
    pub verified: bool,
    pub signature: String,
    pub follower_count: u64,
    pub following_count: u64,
    pub video_count: u64,
    pub heart_count: u64,
}

impl WebProfile {
    /// Upstream shape drifts (`userInfo.user` vs `user`, camel vs snake case).
    pub fn from_payload(payload: &Value) -> Self {
        let info = &payload["userInfo"];
        let user = if info["user"].is_object() { &info["user"] } else { &payload["user"] };
        let stats = &info["stats"];
        let s =
            |a: &str, b: &str| user[a].as_str().or_else(|| user[b].as_str()).unwrap_or_default().to_string();
        let n = |v: &Value| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).unwrap_or(0);
        WebProfile {
            unique_id: s("uniqueId", "unique_id"),
            nickname: s("nickname", "nickname"),
            sec_uid: s("secUid", "sec_uid"),
            open_id: s("openId", "id"),
            verified: user["verified"].as_bool().unwrap_or(false),
            signature: s("signature", "signature"),
            follower_count: n(&stats["followerCount"]),
            following_count: n(&stats["followingCount"]),
            video_count: n(&stats["videoCount"]),
            heart_count: n(if stats["heartCount"].is_null() {
                &stats["heart"]
            } else {
                &stats["heartCount"]
            }),
        }
    }
}

/// What a public video page says about its author.
#[derive(Clone, Debug, Serialize)]
pub struct VideoPageAuthor {
    /// Final page URL after redirects.
    pub page: String,
    /// The video author's secUid (first occurrence on the page).
    pub sec_uid: String,
    /// Other distinct secUids on the page.
    pub others: Vec<String>,
}

#[derive(Clone)]
pub struct WebClient {
    transport: Arc<Transport>,
    signer: Arc<dyn WebSigner>,
    session: Option<WebSession>,
    proxy: Option<ProxyUrl>,
}

impl WebClient {
    pub fn new(
        transport: Arc<Transport>,
        signer: Arc<dyn WebSigner>,
        session: Option<WebSession>,
        proxy: Option<ProxyUrl>,
    ) -> Self {
        WebClient { transport, signer, session, proxy }
    }

    /// With its own transport. Page scraping ([`WebClient::video_author`]) needs
    /// no signer or session.
    pub fn standalone(
        signer: Arc<dyn WebSigner>,
        session: Option<WebSession>,
        proxy: Option<ProxyUrl>,
    ) -> Self {
        Self::new(Arc::new(Transport::new(TransportConfig::default())), signer, session, proxy)
    }

    fn session(&self) -> Result<&WebSession> {
        self.session
            .as_ref()
            .ok_or_else(|| Error::InvalidArgument("web API needs an msToken (TIKTOK_MS_TOKEN)".into()))
    }

    pub async fn profile_raw(&self, unique_id: &str) -> Result<Value> {
        let session = self.session()?;
        let query = session.signed_query(
            &*self.signer,
            &Params::new().with("uniqueId", unique_id.trim_start_matches('@')),
        )?;
        let url = format!("{PROFILE_URL}?{query}");
        self.transport
            .get_json(&url, &session.headers("https://www.tiktok.com/"), self.proxy.as_ref(), true)
            .await
    }

    pub async fn profile(&self, unique_id: &str) -> Result<WebProfile> {
        Ok(WebProfile::from_payload(&self.profile_raw(unique_id).await?))
    }

    pub async fn comments_page(&self, aweme_id: &str, cursor: u64, count: u32) -> Result<CommentPage> {
        let session = self.session()?;
        let query = session.signed_query(
            &*self.signer,
            &Params::new()
                .with("aweme_id", aweme_id)
                .with("cursor", cursor)
                .with("count", count)
                .with("comment_style", 2),
        )?;
        let url = format!("{COMMENTS_URL}?{query}");
        let raw = self
            .transport
            .get_json(&url, &session.headers("https://www.tiktok.com/"), self.proxy.as_ref(), true)
            .await?;
        serde_json::from_value(raw).map_err(Error::Shape)
    }

    /// Author secUid of a video, from its public page (oEmbed has no secUid).
    /// `video` is an aweme id or any tiktok.com URL.
    pub async fn video_author(&self, video: &str) -> Result<VideoPageAuthor> {
        let url = self.video_page_url(video).await?;
        let headers = page_headers();
        let resp = self.transport.get(&url, &headers, self.proxy.as_ref(), false).await?;
        let html = String::from_utf8_lossy(&resp.body);
        let mut found: Vec<String> = Vec::new();
        for cap in SECUID_RE.captures_iter(&html) {
            let v = cap[1].to_string();
            if !found.contains(&v) {
                found.push(v);
            }
        }
        if found.is_empty() {
            return Err(Error::InvalidArgument("secUid not found in page (login wall or block?)".into()));
        }
        let sec_uid = found.remove(0);
        Ok(VideoPageAuthor { page: url, sec_uid, others: found })
    }

    /// `/x/video/<id>` redirects without the page state, so resolve the author via
    /// oEmbed first and use the canonical `@author/video/<id>` page.
    async fn video_page_url(&self, video: &str) -> Result<String> {
        if video.starts_with("http") {
            return Ok(video.to_string());
        }
        if video.is_empty() || !video.bytes().all(|b| b.is_ascii_digit()) {
            return Err(Error::InvalidArgument("give an aweme id or a tiktok URL".into()));
        }
        let oembed = format!("https://www.tiktok.com/oembed?url=https://www.tiktok.com/x/video/{video}");
        let author = match self.transport.get(&oembed, &page_headers(), self.proxy.as_ref(), false).await {
            Ok(resp) => serde_json::from_slice::<Value>(&resp.body)
                .ok()
                .and_then(|v| v["author_unique_id"].as_str().map(str::to_string))
                .unwrap_or_default(),
            Err(_) => String::new(),
        };
        Ok(if author.is_empty() {
            format!("https://www.tiktok.com/x/video/{video}")
        } else {
            format!("https://www.tiktok.com/@{author}/video/{video}")
        })
    }
}

fn page_headers() -> Vec<(String, String)> {
    vec![("User-Agent".into(), DEFAULT_UA.into()), ("Accept-Language".into(), "en-US,en;q=0.9".into())]
}
