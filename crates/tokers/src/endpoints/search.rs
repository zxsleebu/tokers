use serde::{Deserialize, Serialize};

use super::Endpoint;
use crate::de;
use crate::models::{Aweme, Challenge, Music, Status, User};
use crate::params::Params;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Suggestion {
    #[serde(deserialize_with = "de::string")]
    pub content: String,
    #[serde(deserialize_with = "de::string")]
    pub sug_type: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SuggestResponse {
    #[serde(flatten)]
    pub status: Status,
    #[serde(deserialize_with = "de::nullable")]
    pub sug_list: Vec<Suggestion>,
}

/// Query autocompletion.
#[derive(Clone, Debug)]
pub struct SearchSuggest {
    pub keyword: String,
}

impl Endpoint for SearchSuggest {
    type Response = SuggestResponse;
    const PATH: &'static str = "/aweme/v1/search/sug/";

    fn params(&self) -> Params {
        Params::new().with("keyword", &self.keyword).with("source", "search_sug")
    }
}

/// One row of mixed search results: a video or some other card.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralItem {
    #[serde(rename = "type", deserialize_with = "de::i64")]
    pub kind: i64,
    #[serde(deserialize_with = "de::string")]
    pub doc_id: String,
    pub aweme_info: Option<Aweme>,
    pub card_profile: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralSearchResponse {
    #[serde(flatten)]
    pub status: Status,
    #[serde(deserialize_with = "de::nullable")]
    pub data: Vec<GeneralItem>,
    #[serde(deserialize_with = "de::bool")]
    pub has_more: bool,
    #[serde(deserialize_with = "de::u64")]
    pub cursor: u64,
}

/// Mixed results (videos + cards), offset pagination. Template identity only.
#[derive(Clone, Debug)]
pub struct SearchGeneral {
    pub keyword: String,
    pub offset: u64,
    pub count: u32,
    pub search_source: String,
}

impl SearchGeneral {
    pub fn new(keyword: impl Into<String>) -> Self {
        SearchGeneral { keyword: keyword.into(), offset: 0, count: 10, search_source: "normal_search".into() }
    }
}

impl Endpoint for SearchGeneral {
    type Response = GeneralSearchResponse;
    const PATH: &'static str = "/aweme/v1/general/search/single/";

    fn params(&self) -> Params {
        Params::new()
            .with("keyword", &self.keyword)
            .with("offset", self.offset)
            .with("count", self.count)
            .with("search_source", &self.search_source)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UserSearchItem {
    #[serde(deserialize_with = "de::nullable")]
    pub user_info: User,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UserSearchResponse {
    #[serde(flatten)]
    pub status: Status,
    #[serde(deserialize_with = "de::nullable")]
    pub user_list: Vec<UserSearchItem>,
    #[serde(deserialize_with = "de::bool")]
    pub has_more: bool,
    #[serde(deserialize_with = "de::u64")]
    pub cursor: u64,
}

#[derive(Clone, Debug)]
pub struct SearchUsers {
    pub keyword: String,
    pub cursor: u64,
    pub count: u32,
}

impl Endpoint for SearchUsers {
    type Response = UserSearchResponse;
    const PATH: &'static str = "/aweme/v1/discover/search/";

    fn params(&self) -> Params {
        Params::new()
            .with("keyword", &self.keyword)
            .with("cursor", self.cursor)
            .with("count", self.count)
            .with("type", 1)
            .with("search_source", "normal_search")
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoSearchItem {
    pub aweme_info: Option<Aweme>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoSearchResponse {
    #[serde(flatten)]
    pub status: Status,
    #[serde(deserialize_with = "de::nullable")]
    pub search_item_list: Vec<VideoSearchItem>,
    #[serde(deserialize_with = "de::bool")]
    pub has_more: bool,
    #[serde(deserialize_with = "de::u64")]
    pub cursor: u64,
}

/// Video search, offset pagination. Template identity only.
#[derive(Clone, Debug)]
pub struct SearchVideos {
    pub keyword: String,
    pub offset: u64,
    pub count: u32,
    pub sort_type: u32,
    pub publish_time: u32,
}

impl Endpoint for SearchVideos {
    type Response = VideoSearchResponse;
    const PATH: &'static str = "/aweme/v1/search/item/";

    fn params(&self) -> Params {
        Params::new()
            .with("keyword", &self.keyword)
            .with("offset", self.offset)
            .with("count", self.count)
            .with("sort_type", self.sort_type)
            .with("publish_time", self.publish_time)
            .with("search_source", "normal_search")
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChallengeSearchItem {
    #[serde(deserialize_with = "de::nullable")]
    pub challenge_info: Challenge,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChallengeSearchResponse {
    #[serde(flatten)]
    pub status: Status,
    #[serde(deserialize_with = "de::nullable")]
    pub challenge_list: Vec<ChallengeSearchItem>,
    #[serde(deserialize_with = "de::bool")]
    pub has_more: bool,
    #[serde(deserialize_with = "de::u64")]
    pub cursor: u64,
}

#[derive(Clone, Debug)]
pub struct SearchHashtags {
    pub keyword: String,
    pub cursor: u64,
    pub count: u32,
}

impl Endpoint for SearchHashtags {
    type Response = ChallengeSearchResponse;
    const PATH: &'static str = "/aweme/v1/challenge/search/";

    fn params(&self) -> Params {
        Params::new().with("keyword", &self.keyword).with("cursor", self.cursor).with("count", self.count)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MusicSearchResponse {
    #[serde(flatten)]
    pub status: Status,
    #[serde(deserialize_with = "de::nullable")]
    pub music: Vec<Music>,
    #[serde(deserialize_with = "de::bool")]
    pub has_more: bool,
    #[serde(deserialize_with = "de::u64")]
    pub cursor: u64,
}

#[derive(Clone, Debug)]
pub struct SearchMusic {
    pub keyword: String,
    pub cursor: u64,
    pub count: u32,
}

impl Endpoint for SearchMusic {
    type Response = MusicSearchResponse;
    const PATH: &'static str = "/aweme/v1/music/search/";

    fn params(&self) -> Params {
        Params::new().with("keyword", &self.keyword).with("cursor", self.cursor).with("count", self.count)
    }
}
