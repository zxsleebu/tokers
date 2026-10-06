//! The mobile API client.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::endpoints::*;
use crate::error::{Error, Result};
use crate::identity::Identity;
use crate::params::Params;
use crate::proxy::ProxyUrl;
use crate::signer::Signer;
use crate::template::{PreparedRequest, RequestTemplate};
use crate::transport::{Transport, TransportConfig};

/// Edge cookies the app sends on these hosts.
pub const TTNET_COOKIES: &str = "store-idc=useast5; tt-target-idc=useast5";

#[derive(Clone, Debug)]
pub struct ClientConfig {
    /// Extra attempts after a transient failure (each re-signs with a fresh ts).
    pub retries: u32,
    pub transport: TransportConfig,
}

impl Default for ClientConfig {
    fn default() -> Self {
        ClientConfig { retries: 4, transport: TransportConfig::default() }
    }
}

struct Shared {
    transport: Arc<Transport>,
    template: RequestTemplate,
    signer: Arc<dyn Signer>,
    retries: u32,
}

/// Mobile aweme API client.
///
/// Cloning is cheap; clones share the HTTP connections and pacing.
/// [`TikTok::with_identity`] / [`TikTok::with_proxy`] give views that send as
/// another device or through another egress over the same shared state.
#[derive(Clone)]
pub struct TikTok {
    shared: Arc<Shared>,
    identity: Identity,
    proxy: Option<ProxyUrl>,
}

pub struct TikTokBuilder {
    template: RequestTemplate,
    signer: Arc<dyn Signer>,
    transport: Option<Arc<Transport>>,
    config: ClientConfig,
    identity: Identity,
    proxy: Option<ProxyUrl>,
}

impl TikTokBuilder {
    pub fn template(mut self, template: RequestTemplate) -> Self {
        self.template = template;
        self
    }

    pub fn signer(mut self, signer: Arc<dyn Signer>) -> Self {
        self.signer = signer;
        self
    }

    /// Share an existing transport (e.g. with a [`crate::web::WebClient`]).
    pub fn transport(mut self, transport: Arc<Transport>) -> Self {
        self.transport = Some(transport);
        self
    }

    pub fn config(mut self, config: ClientConfig) -> Self {
        self.config = config;
        self
    }

    pub fn retries(mut self, retries: u32) -> Self {
        self.config.retries = retries;
        self
    }

    pub fn min_interval(mut self, interval: Duration) -> Self {
        self.config.transport.min_interval = interval;
        self
    }

    pub fn identity(mut self, identity: Identity) -> Self {
        self.identity = identity;
        self
    }

    pub fn proxy(mut self, proxy: Option<ProxyUrl>) -> Self {
        self.proxy = proxy;
        self
    }

    pub fn build(self) -> TikTok {
        let transport =
            self.transport.unwrap_or_else(|| Arc::new(Transport::new(self.config.transport.clone())));
        TikTok {
            shared: Arc::new(Shared {
                transport,
                template: self.template,
                signer: self.signer,
                retries: self.config.retries,
            }),
            identity: self.identity,
            proxy: self.proxy,
        }
    }
}

impl TikTok {
    /// See also [`crate::Backend::tiktok`].
    pub fn builder(template: RequestTemplate, signer: Arc<dyn Signer>) -> TikTokBuilder {
        TikTokBuilder {
            template,
            signer,
            transport: None,
            config: ClientConfig::default(),
            identity: Identity::Template,
            proxy: None,
        }
    }

    pub fn with_identity(&self, identity: Identity) -> Self {
        TikTok { identity, ..self.clone() }
    }

    pub fn with_proxy(&self, proxy: Option<ProxyUrl>) -> Self {
        TikTok { proxy, ..self.clone() }
    }

    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    pub fn proxy(&self) -> Option<&ProxyUrl> {
        self.proxy.as_ref()
    }

    pub fn template(&self) -> &RequestTemplate {
        &self.shared.template
    }

    pub fn transport(&self) -> &Arc<Transport> {
        &self.shared.transport
    }

    /// Sign a request: `(url, headers)` ready to send.
    pub fn sign_request(&self, req: &PreparedRequest) -> Result<(String, Vec<(String, String)>)> {
        let mut headers = req.headers.clone();
        headers.push(("User-Agent".into(), req.user_agent.clone()));
        headers.extend(self.shared.signer.sign(req, b"")?);
        headers.push(("Cookie".into(), TTNET_COOKIES.into()));
        Ok((req.url(), headers))
    }

    /// Signed URL + headers for an endpoint, without sending.
    pub fn prepare<E: Endpoint>(&self, endpoint: &E) -> Result<(String, Vec<(String, String)>)> {
        let req = endpoint.prepare(&self.shared.template, &self.identity, now_ms());
        self.sign_request(&req)
    }

    /// Send an endpoint request, return the raw JSON.
    pub async fn call_raw<E: Endpoint>(&self, endpoint: &E) -> Result<Value> {
        self.send(|now| endpoint.prepare(&self.shared.template, &self.identity, now)).await
    }

    /// Send an endpoint request, return the typed response.
    pub async fn call<E: Endpoint>(&self, endpoint: &E) -> Result<E::Response> {
        let raw = self.call_raw(endpoint).await?;
        serde_json::from_value(raw).map_err(Error::Shape)
    }

    /// Signed GET of any aweme path with these endpoint params (escape hatch for
    /// endpoints without a type yet).
    pub async fn get_raw(&self, path: &str, params: &Params) -> Result<Value> {
        self.send(|now| self.shared.template.api(path, params, &self.identity, now)).await
    }

    async fn send(&self, prepare: impl Fn(u64) -> PreparedRequest) -> Result<Value> {
        let mut attempt = 0;
        loop {
            let (url, headers) = self.sign_request(&prepare(now_ms()))?;
            match self.shared.transport.get_json(&url, &headers, self.proxy.as_ref(), true).await {
                Err(e) if e.is_transient() && attempt < self.shared.retries => {
                    attempt += 1;
                    tokio::time::sleep(Duration::from_millis(500 * u64::from(attempt))).await;
                }
                other => return other,
            }
        }
    }

    // --- convenience wrappers ------------------------------------------------

    pub async fn feed(&self, req: &Feed) -> Result<AwemeListPage> {
        self.call(req).await
    }

    pub async fn aweme_detail(&self, aweme_id: &str) -> Result<AwemeDetailResponse> {
        self.call(&AwemeDetail { aweme_id: aweme_id.into() }).await
    }

    pub async fn user_profile(&self, user: UserRef) -> Result<UserProfileResponse> {
        require_user(&user)?;
        self.call(&UserProfile { user }).await
    }

    pub async fn user_posts(&self, user: UserRef, max_cursor: i64, count: u32) -> Result<AwemeListPage> {
        require_user(&user)?;
        self.call(&UserPosts { user, max_cursor, count }).await
    }

    pub async fn followers(&self, user: UserRef, max_time: i64, count: u32) -> Result<UserListPage> {
        require_user(&user)?;
        self.call(&Followers { user, max_time, count }).await
    }

    pub async fn following(&self, user: UserRef, max_time: i64, count: u32) -> Result<UserListPage> {
        require_user(&user)?;
        self.call(&Following { user, max_time, count }).await
    }

    pub async fn comments(&self, aweme_id: &str, cursor: u64, count: u32) -> Result<CommentPage> {
        let count = count.min(CommentList::MAX_COUNT);
        self.call(&CommentList { aweme_id: aweme_id.into(), cursor, count }).await
    }

    pub async fn comment_replies(
        &self,
        aweme_id: &str,
        comment_id: &str,
        cursor: u64,
        count: u32,
    ) -> Result<CommentPage> {
        self.call(&CommentReplies { aweme_id: aweme_id.into(), comment_id: comment_id.into(), cursor, count })
            .await
    }

    pub async fn search_suggest(&self, keyword: &str) -> Result<SuggestResponse> {
        self.call(&SearchSuggest { keyword: keyword.into() }).await
    }

    pub async fn search_general(
        &self,
        keyword: &str,
        offset: u64,
        count: u32,
    ) -> Result<GeneralSearchResponse> {
        self.call(&SearchGeneral { offset, count, ..SearchGeneral::new(keyword) }).await
    }

    pub async fn search_users(&self, keyword: &str, cursor: u64, count: u32) -> Result<UserSearchResponse> {
        self.call(&SearchUsers { keyword: keyword.into(), cursor, count }).await
    }

    pub async fn search_videos(&self, keyword: &str, offset: u64, count: u32) -> Result<VideoSearchResponse> {
        self.call(&SearchVideos { keyword: keyword.into(), offset, count, sort_type: 0, publish_time: 0 })
            .await
    }

    pub async fn search_hashtags(
        &self,
        keyword: &str,
        cursor: u64,
        count: u32,
    ) -> Result<ChallengeSearchResponse> {
        self.call(&SearchHashtags { keyword: keyword.into(), cursor, count }).await
    }

    pub async fn search_music(&self, keyword: &str, cursor: u64, count: u32) -> Result<MusicSearchResponse> {
        self.call(&SearchMusic { keyword: keyword.into(), cursor, count }).await
    }

    pub async fn challenge_detail(&self, ch_id: &str) -> Result<ChallengeDetailResponse> {
        self.call(&ChallengeDetail { ch_id: ch_id.into() }).await
    }

    pub async fn challenge_videos(&self, ch_id: &str, cursor: u64, count: u32) -> Result<AwemeListPage> {
        self.call(&ChallengeVideos { ch_id: ch_id.into(), cursor, count }).await
    }

    pub async fn music_detail(&self, music_id: &str) -> Result<MusicDetailResponse> {
        self.call(&MusicDetail { music_id: music_id.into() }).await
    }

    pub async fn music_videos(&self, music_id: &str, cursor: u64, count: u32) -> Result<AwemeListPage> {
        self.call(&MusicVideos { music_id: music_id.into(), cursor, count }).await
    }
}

fn require_user(user: &UserRef) -> Result<()> {
    if user.is_empty() {
        return Err(Error::InvalidArgument("sec_user_id or user_id required".into()));
    }
    Ok(())
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}
