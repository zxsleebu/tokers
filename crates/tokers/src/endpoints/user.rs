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

/// Public profile. Live app shape (47.1.4, measured 2026-10-11):
/// `GET /tiktok/user/profile/other/v1` on api16-core/api19-va with
/// `scene_id=201&user_id&from=0&profile_*_exp&sec_user_id` head.
#[derive(Clone, Debug)]
pub struct UserProfile {
    pub user: UserRef,
}

impl Endpoint for UserProfile {
    type Response = UserProfileResponse;
    const PATH: &'static str = "/tiktok/user/profile/other/v1";

    fn params(&self) -> Params {
        // Live shape: sec_user_id second when known (no user_id at all then).
        let mut p = Params::new().with("scene_id", 201);
        if !self.user.sec_user_id.is_empty() {
            p = p.with("sec_user_id", &self.user.sec_user_id);
        } else {
            p = p.with("user_id", &self.user.user_id);
        }
        p.with("from", 0)
            .with("profile_optimize_exp", 4)
            .with("header_simplify_exp", 8)
            .with("left_align_exp", 11)
            .with("profile_bg_in_allow_list", 0)
            .with("profile_bg_enable_consumption_group", 3)
    }
}

/// A user's own videos. Live app shape (47.1.4, measured 2026-10-12): the head
/// is source/locate_new_style/before_count/after_count/reverse + shrink,
/// screen-reader, banner and sov flags, then max_cursor, both known ids
/// (user_id and sec_user_id — the app sends both when both are known),
/// count, locate_item_id on the first page, sort_type and
/// following_experiment_param.
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
        let mut p = Params::new()
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
            .with("max_cursor", self.max_cursor);
        if !self.user.user_id.is_empty() {
            p = p.with("user_id", &self.user.user_id);
        }
        if !self.user.sec_user_id.is_empty() {
            p = p.with("sec_user_id", &self.user.sec_user_id);
        }
        p.with("count", self.count).with("sort_type", 0).with(
            "following_experiment_param",
            r#"{"enable_profile_button":false}"#,
        )
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
