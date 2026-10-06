use serde::{Deserialize, Serialize};

use super::Endpoint;
use crate::de;
use crate::models::{Aweme, Status};
use crate::params::Params;

/// A page of videos (feed, a user's posts, a hashtag's or a sound's videos).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AwemeListPage {
    #[serde(flatten)]
    pub status: Status,
    #[serde(deserialize_with = "de::nullable")]
    pub aweme_list: Vec<Aweme>,
    #[serde(deserialize_with = "de::bool")]
    pub has_more: bool,
    #[serde(deserialize_with = "de::u64")]
    pub cursor: u64,
    #[serde(deserialize_with = "de::i64")]
    pub max_cursor: i64,
    #[serde(deserialize_with = "de::i64")]
    pub min_cursor: i64,
}

/// For You feed.
#[derive(Clone, Debug)]
pub struct Feed {
    pub count: u32,
    pub max_cursor: i64,
    pub min_cursor: i64,
    pub pull_type: u32,
    pub is_cold_start: bool,
}

impl Default for Feed {
    fn default() -> Self {
        Feed { count: 6, max_cursor: 0, min_cursor: 0, pull_type: 1, is_cold_start: true }
    }
}

impl Endpoint for Feed {
    type Response = AwemeListPage;
    const PATH: &'static str = "/aweme/v1/feed/";

    fn params(&self) -> Params {
        Params::new()
            .with("type", 0)
            .with("max_cursor", self.max_cursor)
            .with("min_cursor", self.min_cursor)
            .with("count", self.count)
            .with("pull_type", self.pull_type)
            .with("is_cold_start", u8::from(self.is_cold_start))
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AwemeDetailResponse {
    #[serde(flatten)]
    pub status: Status,
    pub aweme_detail: Option<Aweme>,
}

/// One video with author, music, stream URLs.
#[derive(Clone, Debug)]
pub struct AwemeDetail {
    pub aweme_id: String,
}

impl Endpoint for AwemeDetail {
    type Response = AwemeDetailResponse;
    const PATH: &'static str = "/aweme/v1/aweme/detail/";

    fn params(&self) -> Params {
        Params::new().with("aweme_id", &self.aweme_id)
    }
}
