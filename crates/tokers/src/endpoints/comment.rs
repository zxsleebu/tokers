use serde::{Deserialize, Serialize};

use super::Endpoint;
use crate::de;
use crate::identity::Identity;
use crate::models::{Comment, Status};
use crate::params::Params;
use crate::template::{PreparedRequest, RequestTemplate};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CommentPage {
    #[serde(flatten)]
    pub status: Status,
    #[serde(deserialize_with = "de::nullable")]
    pub comments: Vec<Comment>,
    #[serde(deserialize_with = "de::u64")]
    pub cursor: u64,
    #[serde(deserialize_with = "de::bool")]
    pub has_more: bool,
    #[serde(deserialize_with = "de::u64")]
    pub total: u64,
}

/// Top-level comments of a video.
///
/// Never ask for more than 50: the server serves at most 50 positions but echoes
/// `cursor + count`, so a larger count silently skips comments. The first ~1000
/// positions are personalised per pass (built from what was already served);
/// past that the cursor is a plain offset.
#[derive(Clone, Debug)]
pub struct CommentList {
    pub aweme_id: String,
    pub cursor: u64,
    pub count: u32,
}

impl CommentList {
    pub const MAX_COUNT: u32 = 50;
}

impl Endpoint for CommentList {
    type Response = CommentPage;
    const PATH: &'static str = "/aweme/v2/comment/list/";

    fn params(&self) -> Params {
        Params::new().with("aweme_id", &self.aweme_id).with("cursor", self.cursor).with("count", self.count)
    }

    /// The captured request *is* a comment/list call: reuse all of it.
    fn prepare(&self, template: &RequestTemplate, identity: &Identity, now_ms: u64) -> PreparedRequest {
        template.comment_list(&self.aweme_id, self.cursor, self.count, identity, now_ms)
    }
}

/// Replies of one comment. A page holds up to ~45 and the returned cursor is
/// honest (= items served), so `count = 50` is safe.
/// (`/aweme/v2/comment/list/reply/` answers with an empty body.)
#[derive(Clone, Debug)]
pub struct CommentReplies {
    pub aweme_id: String,
    pub comment_id: String,
    pub cursor: u64,
    pub count: u32,
}

impl Endpoint for CommentReplies {
    type Response = CommentPage;
    const PATH: &'static str = "/aweme/v1/comment/list/reply/";

    fn params(&self) -> Params {
        Params::new()
            .with("item_id", &self.aweme_id)
            .with("comment_id", &self.comment_id)
            .with("cursor", self.cursor)
            .with("count", self.count)
    }
}
