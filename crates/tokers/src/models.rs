//! Typed views of the aweme API objects. Only the stable fields a client needs are
//! modelled; every endpoint is also available raw ([`crate::TikTok::call_raw`]).
//! All fields default when missing or `null`.

use serde::{Deserialize, Serialize};

use crate::de;
use crate::error::{Error, Result};

/// The `status_code` / `status_msg` envelope every response carries.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Status {
    #[serde(deserialize_with = "de::i64")]
    pub status_code: i64,
    #[serde(deserialize_with = "de::string")]
    pub status_msg: String,
}

impl Status {
    pub fn is_ok(&self) -> bool {
        self.status_code == 0
    }

    pub fn check(&self) -> Result<()> {
        if self.is_ok() {
            Ok(())
        } else {
            Err(Error::Api { code: self.status_code, message: self.status_msg.clone() })
        }
    }
}

/// A CDN resource: same object under several mirror URLs.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UrlList {
    #[serde(deserialize_with = "de::string")]
    pub uri: String,
    #[serde(deserialize_with = "de::nullable")]
    pub url_list: Vec<String>,
    #[serde(deserialize_with = "de::u64")]
    pub width: u64,
    #[serde(deserialize_with = "de::u64")]
    pub height: u64,
    #[serde(deserialize_with = "de::u64")]
    pub data_size: u64,
}

impl UrlList {
    pub fn first(&self) -> Option<&str> {
        self.url_list.first().map(String::as_str)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct User {
    #[serde(deserialize_with = "de::string")]
    pub uid: String,
    #[serde(deserialize_with = "de::string")]
    pub sec_uid: String,
    #[serde(deserialize_with = "de::string")]
    pub unique_id: String,
    #[serde(deserialize_with = "de::string")]
    pub nickname: String,
    #[serde(deserialize_with = "de::string")]
    pub signature: String,
    #[serde(deserialize_with = "de::nullable")]
    pub avatar_thumb: UrlList,
    #[serde(deserialize_with = "de::nullable")]
    pub avatar_larger: UrlList,
    #[serde(deserialize_with = "de::u64")]
    pub follower_count: u64,
    #[serde(deserialize_with = "de::u64")]
    pub following_count: u64,
    #[serde(deserialize_with = "de::u64")]
    pub aweme_count: u64,
    #[serde(deserialize_with = "de::u64")]
    pub favoriting_count: u64,
    #[serde(deserialize_with = "de::u64")]
    pub total_favorited: u64,
    #[serde(deserialize_with = "de::i64")]
    pub verification_type: i64,
    #[serde(deserialize_with = "de::string")]
    pub custom_verify: String,
}

impl User {
    pub fn verified(&self) -> bool {
        self.verification_type != 0 || !self.custom_verify.is_empty()
    }

    /// `@unique_id`, falling back to the numeric uid.
    pub fn handle(&self) -> &str {
        if self.unique_id.is_empty() { &self.uid } else { &self.unique_id }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Statistics {
    #[serde(deserialize_with = "de::u64")]
    pub digg_count: u64,
    #[serde(deserialize_with = "de::u64")]
    pub comment_count: u64,
    #[serde(deserialize_with = "de::u64")]
    pub share_count: u64,
    #[serde(deserialize_with = "de::u64")]
    pub play_count: u64,
    #[serde(deserialize_with = "de::u64")]
    pub collect_count: u64,
    #[serde(deserialize_with = "de::u64")]
    pub download_count: u64,
}

/// A span in a text: @mention (`user_id` set) or #hashtag (`hashtag_name` set).
/// `start`/`end` are UTF-16 offsets.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TextExtra {
    #[serde(deserialize_with = "de::i64")]
    pub start: i64,
    #[serde(deserialize_with = "de::i64")]
    pub end: i64,
    #[serde(rename = "type", deserialize_with = "de::i64")]
    pub kind: i64,
    #[serde(deserialize_with = "de::string")]
    pub user_id: String,
    #[serde(deserialize_with = "de::string")]
    pub sec_uid: String,
    #[serde(deserialize_with = "de::string")]
    pub hashtag_name: String,
    #[serde(deserialize_with = "de::string")]
    pub hashtag_id: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BitRate {
    #[serde(deserialize_with = "de::string")]
    pub gear_name: String,
    #[serde(deserialize_with = "de::i64")]
    pub quality_type: i64,
    #[serde(deserialize_with = "de::u64")]
    pub bit_rate: u64,
    #[serde(deserialize_with = "de::i64")]
    pub is_h265: i64,
    #[serde(deserialize_with = "de::nullable")]
    pub play_addr: UrlList,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Video {
    /// Milliseconds.
    #[serde(deserialize_with = "de::u64")]
    pub duration: u64,
    #[serde(deserialize_with = "de::u64")]
    pub width: u64,
    #[serde(deserialize_with = "de::u64")]
    pub height: u64,
    #[serde(deserialize_with = "de::string")]
    pub ratio: String,
    #[serde(deserialize_with = "de::nullable")]
    pub cover: UrlList,
    #[serde(deserialize_with = "de::nullable")]
    pub origin_cover: UrlList,
    #[serde(deserialize_with = "de::nullable")]
    pub dynamic_cover: UrlList,
    #[serde(deserialize_with = "de::nullable")]
    pub play_addr: UrlList,
    #[serde(deserialize_with = "de::nullable")]
    pub download_addr: UrlList,
    #[serde(deserialize_with = "de::nullable")]
    pub download_no_watermark_addr: UrlList,
    #[serde(deserialize_with = "de::nullable")]
    pub bit_rate: Vec<BitRate>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Music {
    #[serde(deserialize_with = "de::string")]
    pub id: String,
    #[serde(deserialize_with = "de::string")]
    pub id_str: String,
    #[serde(deserialize_with = "de::string")]
    pub title: String,
    #[serde(deserialize_with = "de::string")]
    pub author: String,
    #[serde(deserialize_with = "de::string")]
    pub album: String,
    /// Seconds.
    #[serde(deserialize_with = "de::u64")]
    pub duration: u64,
    #[serde(deserialize_with = "de::u64")]
    pub user_count: u64,
    #[serde(deserialize_with = "de::nullable")]
    pub cover_thumb: UrlList,
    #[serde(deserialize_with = "de::nullable")]
    pub cover_medium: UrlList,
    #[serde(deserialize_with = "de::nullable")]
    pub cover_large: UrlList,
    #[serde(deserialize_with = "de::nullable")]
    pub play_url: UrlList,
}

impl Music {
    /// `id_str` when present (exact), else `id`.
    pub fn music_id(&self) -> &str {
        if self.id_str.is_empty() { &self.id } else { &self.id_str }
    }

    pub fn cover(&self) -> &UrlList {
        if self.cover_medium.url_list.is_empty() { &self.cover_large } else { &self.cover_medium }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Challenge {
    #[serde(deserialize_with = "de::string")]
    pub cid: String,
    #[serde(deserialize_with = "de::string")]
    pub cha_name: String,
    #[serde(deserialize_with = "de::string")]
    pub desc: String,
    #[serde(deserialize_with = "de::u64")]
    pub use_count: u64,
    #[serde(deserialize_with = "de::u64")]
    pub user_count: u64,
    #[serde(deserialize_with = "de::u64")]
    pub view_count: u64,
    #[serde(deserialize_with = "de::string")]
    pub schema: String,
}

/// A video (or photo post).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Aweme {
    #[serde(deserialize_with = "de::string")]
    pub aweme_id: String,
    #[serde(deserialize_with = "de::string")]
    pub desc: String,
    #[serde(deserialize_with = "de::i64")]
    pub create_time: i64,
    #[serde(deserialize_with = "de::i64")]
    pub aweme_type: i64,
    #[serde(deserialize_with = "de::string")]
    pub share_url: String,
    #[serde(deserialize_with = "de::nullable")]
    pub author: User,
    #[serde(deserialize_with = "de::nullable")]
    pub statistics: Statistics,
    #[serde(deserialize_with = "de::nullable")]
    pub cha_list: Vec<Challenge>,
    #[serde(deserialize_with = "de::nullable")]
    pub text_extra: Vec<TextExtra>,
    #[serde(deserialize_with = "de::nullable")]
    pub music: Music,
    #[serde(deserialize_with = "de::nullable")]
    pub video: Video,
    /// Photo posts: the images; `video.play_addr` is then the soundtrack.
    #[serde(deserialize_with = "de::nullable")]
    pub image_post_info: ImagePostInfo,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ImagePostInfo {
    #[serde(deserialize_with = "de::nullable")]
    pub images: Vec<PostImage>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PostImage {
    #[serde(deserialize_with = "de::nullable")]
    pub display_image: UrlList,
}

impl Aweme {
    /// A photo post (slideshow) rather than a video.
    pub fn is_photo(&self) -> bool {
        !self.image_post_info.images.is_empty()
    }

    /// Hashtags from `cha_list`, then any extra ones from `text_extra`, deduplicated.
    pub fn hashtags(&self) -> Vec<&str> {
        let mut tags: Vec<&str> =
            self.cha_list.iter().map(|c| c.cha_name.as_str()).filter(|n| !n.is_empty()).collect();
        for t in &self.text_extra {
            if !t.hashtag_name.is_empty() && !tags.contains(&t.hashtag_name.as_str()) {
                tags.push(&t.hashtag_name);
            }
        }
        tags
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CommentImage {
    #[serde(deserialize_with = "de::nullable")]
    pub origin_url: UrlList,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Media {
    pub kind: MediaKind,
    /// Signed CDN URL, expires (~30 days).
    pub url: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Photo,
    Sticker,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Comment {
    #[serde(deserialize_with = "de::string")]
    pub cid: String,
    #[serde(deserialize_with = "de::string")]
    pub aweme_id: String,
    #[serde(deserialize_with = "de::string")]
    pub text: String,
    #[serde(deserialize_with = "de::i64")]
    pub create_time: i64,
    #[serde(deserialize_with = "de::u64")]
    pub digg_count: u64,
    #[serde(deserialize_with = "de::u64")]
    pub reply_comment_total: u64,
    /// Inline preview of the first replies.
    #[serde(deserialize_with = "de::nullable")]
    pub reply_comment: Vec<Comment>,
    /// `"0"` (or empty) on top-level comments; the parent comment id on replies.
    #[serde(deserialize_with = "de::string")]
    pub reply_id: String,
    /// On a reply to a reply: the reply it answers.
    #[serde(deserialize_with = "de::string")]
    pub reply_to_reply_id: String,
    #[serde(deserialize_with = "de::nullable")]
    pub user: User,
    #[serde(deserialize_with = "de::nullable")]
    pub text_extra: Vec<TextExtra>,
    #[serde(deserialize_with = "de::nullable")]
    pub image_list: Vec<CommentImage>,
    pub cmt_sticker_struct: Option<serde_json::Value>,
}

impl Comment {
    pub fn is_reply(&self) -> bool {
        !matches!(self.reply_id.as_str(), "" | "0")
    }

    /// What the comment carries besides text: photos and/or a sticker.
    pub fn media(&self) -> Vec<Media> {
        let mut out: Vec<Media> = self
            .image_list
            .iter()
            .map(|i| Media { kind: MediaKind::Photo, url: i.origin_url.first().unwrap_or("").to_string() })
            .collect();
        if let Some(sticker) = self.cmt_sticker_struct.as_ref().filter(|v| is_present(v)) {
            let url = sticker
                .pointer("/static_url/mid_resolution_url/url_list/0")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            out.push(Media { kind: MediaKind::Sticker, url: url.to_string() });
        }
        out
    }
}

fn is_present(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Null => false,
        serde_json::Value::Object(m) => !m.is_empty(),
        _ => true,
    }
}
