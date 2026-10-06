use serde::{Deserialize, Serialize};

use super::{AwemeListPage, Endpoint, UserRef};
use crate::de;
use crate::models::{Status, User};
use crate::params::Params;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UserProfileResponse {
    #[serde(flatten)]
    pub status: Status,
    pub user: Option<User>,
}

/// Public profile. (`/aweme/v1/user/` needs login; this one does not.)
#[derive(Clone, Debug)]
pub struct UserProfile {
    pub user: UserRef,
}

impl Endpoint for UserProfile {
    type Response = UserProfileResponse;
    const PATH: &'static str = "/aweme/v1/user/profile/other/";

    fn params(&self) -> Params {
        self.user.params()
    }
}

/// A user's own videos. Exact app request shape (47.1.4): the app puts the sec
/// uid in `user_id` and sends no `sec_user_id`. Currently rejected server-side
/// (see the module docs).
#[derive(Clone, Debug)]
pub struct UserPosts {
    pub user: UserRef,
    pub max_cursor: i64,
    pub count: u32,
}

impl Endpoint for UserPosts {
    type Response = AwemeListPage;
    const PATH: &'static str = "/aweme/v1/aweme/post/";

    fn params(&self) -> Params {
        let who = if self.user.sec_user_id.is_empty() { &self.user.user_id } else { &self.user.sec_user_id };
        Params::new()
            .with("source", 0)
            .with("locate_new_style", "true")
            .with("before_count", 7)
            .with("after_count", 7)
            .with("reverse", "false")
            .with("user_avatar_shrink", "96_96")
            .with("video_cover_shrink", "248_330")
            .with("screen_reader_enable", "false")
            .with("creator_assistant_banner_enable", 0)
            .with("sov_client_enable", 1)
            .with("max_cursor", self.max_cursor)
            .with("user_id", who)
            .with("count", self.count)
            .with("sort_type", 0)
            .with("following_experiment_param", r#"{"enable_profile_button":false}"#)
    }
}

/// Followers / followings page.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UserListPage {
    #[serde(flatten)]
    pub status: Status,
    #[serde(alias = "followers", alias = "followings", deserialize_with = "de::nullable")]
    pub users: Vec<User>,
    #[serde(deserialize_with = "de::bool")]
    pub has_more: bool,
    #[serde(deserialize_with = "de::i64")]
    pub max_time: i64,
    #[serde(deserialize_with = "de::i64")]
    pub min_time: i64,
    #[serde(deserialize_with = "de::u64")]
    pub total: u64,
}

#[derive(Clone, Debug)]
pub struct Followers {
    pub user: UserRef,
    /// `min_time` of the previous page; 0 for the first.
    pub max_time: i64,
    pub count: u32,
}

impl Endpoint for Followers {
    type Response = UserListPage;
    const PATH: &'static str = "/aweme/v1/user/follower/list/";

    fn params(&self) -> Params {
        self.user.params().with("count", self.count).with("max_time", self.max_time)
    }
}

#[derive(Clone, Debug)]
pub struct Following {
    pub user: UserRef,
    pub max_time: i64,
    pub count: u32,
}

impl Endpoint for Following {
    type Response = UserListPage;
    const PATH: &'static str = "/aweme/v1/user/following/list/";

    fn params(&self) -> Params {
        self.user.params().with("count", self.count).with("max_time", self.max_time)
    }
}
