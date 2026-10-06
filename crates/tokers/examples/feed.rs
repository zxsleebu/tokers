//! cargo run -p tokers --example feed -- <capture.json>
//!
//! The library surface a GUI uses: one shared client, typed pages, cursors.
//! Without a signer every call ends in `Error::NoSigner`; a real `Backend`
//! comes from the signing implementation.

use tokers::endpoints::Feed;
use tokers::{Backend, Identity, RequestTemplate};

#[tokio::main]
async fn main() -> tokers::Result<()> {
    let mut backend = Backend::unsigned();
    if let Some(path) = std::env::args().nth(1) {
        backend.template = RequestTemplate::load(path)?;
    }
    let tiktok = backend.tiktok().build();

    let page = tiktok.feed(&Feed { count: 4, ..Feed::default() }).await?;
    page.status.check()?;
    for aweme in &page.aweme_list {
        let stream = aweme.video.play_addr.first().unwrap_or("-");
        println!("@{} {:?}\n    {stream}", aweme.author.handle(), aweme.desc);
    }

    // Same connections and pacing, different device ids.
    let other = tiktok.with_identity(Identity::random());
    if let Some(first) = page.aweme_list.first() {
        let comments = other.comments(&first.aweme_id, 0, 5).await?;
        for c in &comments.comments {
            println!("  [{}] @{}: {}", c.digg_count, c.user.unique_id, c.text);
        }
    }
    Ok(())
}
