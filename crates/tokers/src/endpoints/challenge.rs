use serde::{Deserialize, Serialize};

use super::{AwemeListPage, Endpoint};
use crate::models::{Challenge, Status};
use crate::params::Params;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChallengeDetailResponse {
    #[serde(flatten)]
    pub status: Status,
    pub ch_info: Option<Challenge>,
}

/// Hashtag page.
#[derive(Clone, Debug)]
pub struct ChallengeDetail {
    pub ch_id: String,
}

impl Endpoint for ChallengeDetail {
    type Response = ChallengeDetailResponse;
    const PATH: &'static str = "/aweme/v1/challenge/detail/";

    fn params(&self) -> Params {
        Params::new().with("ch_id", &self.ch_id)
    }
}

/// Videos of a hashtag. Served by `challenge/fresh/aweme`: the plain
/// `/aweme/v1/challenge/aweme/` is rejected with an empty body in every variant.
#[derive(Clone, Debug)]
pub struct ChallengeVideos {
    pub ch_id: String,
    pub cursor: u64,
    pub count: u32,
}

impl Endpoint for ChallengeVideos {
    type Response = AwemeListPage;
    const PATH: &'static str = "/aweme/v1/challenge/fresh/aweme/";

    fn params(&self) -> Params {
        Params::new().with("ch_id", &self.ch_id).with("cursor", self.cursor).with("count", self.count)
    }
}
