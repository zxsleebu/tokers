use serde::{Deserialize, Serialize};

use super::{AwemeListPage, Endpoint};
use crate::models::{Music, Status};
use crate::params::Params;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MusicDetailResponse {
    #[serde(flatten)]
    pub status: Status,
    pub music_info: Option<Music>,
}

/// Sound page.
#[derive(Clone, Debug)]
pub struct MusicDetail {
    pub music_id: String,
}

impl Endpoint for MusicDetail {
    type Response = MusicDetailResponse;
    const PATH: &'static str = "/aweme/v1/music/detail/";

    fn params(&self) -> Params {
        Params::new().with("music_id", &self.music_id)
    }
}

/// Videos that use a sound.
#[derive(Clone, Debug)]
pub struct MusicVideos {
    pub music_id: String,
    pub cursor: u64,
    pub count: u32,
}

impl Endpoint for MusicVideos {
    type Response = AwemeListPage;
    const PATH: &'static str = "/aweme/v1/music/aweme/";

    fn params(&self) -> Params {
        Params::new()
            .with("music_id", &self.music_id)
            .with("cursor", self.cursor)
            .with("count", self.count)
            .with("type", 6)
    }
}
