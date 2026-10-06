//! Typed aweme API requests.
//!
//! No login needed for any of these (verified against the live edge with the
//! template identity; most also accept fresh random ids):
//!
//! | request              | path                                | items            |
//! |----------------------|-------------------------------------|------------------|
//! | [`Feed`]             | /aweme/v1/feed/                     | aweme_list       |
//! | [`AwemeDetail`]      | /aweme/v1/aweme/detail/             | aweme_detail     |
//! | [`UserProfile`]      | /aweme/v1/user/profile/other/       | user             |
//! | [`UserPosts`]        | /aweme/v1/aweme/post/               | aweme_list       |
//! | [`Followers`]        | /aweme/v1/user/follower/list/       | followers        |
//! | [`Following`]        | /aweme/v1/user/following/list/      | followings       |
//! | [`CommentList`]      | /aweme/v2/comment/list/             | comments         |
//! | [`CommentReplies`]   | /aweme/v1/comment/list/reply/       | comments         |
//! | [`SearchSuggest`]    | /aweme/v1/search/sug/               | sug_list         |
//! | [`SearchGeneral`]    | /aweme/v1/general/search/single/    | data (mixed)     |
//! | [`SearchUsers`]      | /aweme/v1/discover/search/          | user_list        |
//! | [`SearchVideos`]     | /aweme/v1/search/item/              | search_item_list |
//! | [`SearchHashtags`]   | /aweme/v1/challenge/search/         | challenge_list   |
//! | [`SearchMusic`]      | /aweme/v1/music/search/             | music            |
//! | [`ChallengeDetail`]  | /aweme/v1/challenge/detail/         | ch_info          |
//! | [`ChallengeVideos`]  | /aweme/v1/challenge/fresh/aweme/    | aweme_list       |
//! | [`MusicDetail`]      | /aweme/v1/music/detail/             | music_info       |
//! | [`MusicVideos`]      | /aweme/v1/music/aweme/              | aweme_list       |
//!
//! Known edge behaviour:
//! * `SearchGeneral` and `SearchVideos` pass only with the template identity.
//! * `UserPosts` is answered with HTTP 200 and an empty body for every shape tried,
//!   the app included ("Something went wrong"): a server-side gate. The request
//!   keeps the captured app shape for when it recovers.
//! * The edge intermittently drops requests it would otherwise serve (~40-50% on
//!   challenge/fresh/aweme); the client re-signs and retries.
//!
//! Pagination: each page carries its own cursor (`cursor`/`max_cursor`/`max_time`/
//! `offset`) and `has_more`; pass the cursor back in.
//!
//! To add an endpoint: a struct, `impl Endpoint` with its path, params and
//! response type. Override [`Endpoint::prepare`] only if the request is not cut
//! from the template's common params.

mod aweme;
mod challenge;
mod comment;
mod music;
mod search;
mod user;

pub use aweme::*;
pub use challenge::*;
pub use comment::*;
pub use music::*;
pub use search::*;
pub use user::*;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::identity::Identity;
use crate::params::Params;
use crate::template::{PreparedRequest, RequestTemplate};

pub trait Endpoint: Send + Sync {
    type Response: DeserializeOwned + Serialize + Send + 'static;

    const PATH: &'static str;

    fn params(&self) -> Params;

    fn prepare(&self, template: &RequestTemplate, identity: &Identity, now_ms: u64) -> PreparedRequest {
        template.api(Self::PATH, &self.params(), identity, now_ms)
    }
}

/// Who a user endpoint is about: either id works; `sec_user_id` is what the API
/// hands out everywhere.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UserRef {
    pub sec_user_id: String,
    pub user_id: String,
}

impl UserRef {
    pub fn sec_uid(sec_user_id: impl Into<String>) -> Self {
        UserRef { sec_user_id: sec_user_id.into(), user_id: String::new() }
    }

    pub fn uid(user_id: impl Into<String>) -> Self {
        UserRef { sec_user_id: String::new(), user_id: user_id.into() }
    }

    pub fn is_empty(&self) -> bool {
        self.sec_user_id.is_empty() && self.user_id.is_empty()
    }

    fn params(&self) -> Params {
        Params::new().with("sec_user_id", &self.sec_user_id).with("user_id", &self.user_id)
    }
}
