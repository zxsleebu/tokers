//! For You feed: a scrolling list of video cards with paging.

use gpui::{Context, IntoElement, ParentElement, Render, SharedString, Styled, Window, div, prelude::*, rgb};
use tokers::TikTok;
use tokers::endpoints::Feed;
use tokers::models::Aweme;

pub struct FeedView {
    tiktok: TikTok,
    io: tokio::runtime::Handle,
    items: Vec<Aweme>,
    max_cursor: i64,
    has_more: bool,
    loading: bool,
    error: Option<SharedString>,
}

impl FeedView {
    pub fn new(tiktok: TikTok, io: tokio::runtime::Handle, cx: &mut Context<Self>) -> Self {
        let mut view = FeedView {
            tiktok,
            io,
            items: Vec::new(),
            max_cursor: 0,
            has_more: true,
            loading: false,
            error: None,
        };
        view.load_more(cx);
        view
    }

    fn load_more(&mut self, cx: &mut Context<Self>) {
        if self.loading || !self.has_more {
            return;
        }
        self.loading = true;
        self.error = None;
        cx.notify();

        let tiktok = self.tiktok.clone();
        let req = Feed { count: 6, max_cursor: self.max_cursor, ..Feed::default() };
        let job = self.io.spawn(async move { tiktok.feed(&req).await });
        cx.spawn(async move |this, cx| {
            let result = match job.await {
                Ok(page) => page.and_then(|p| p.status.check().map(|()| p)).map_err(|e| e.to_string()),
                Err(e) => Err(e.to_string()),
            };
            this.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok(page) => {
                        this.max_cursor = page.max_cursor;
                        this.has_more = page.has_more;
                        this.items.extend(page.aweme_list);
                    }
                    Err(e) => this.error = Some(e.into()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

fn card(index: usize, aweme: &Aweme) -> impl IntoElement {
    let s = &aweme.statistics;
    let tags: Vec<String> = aweme.hashtags().iter().take(4).map(|t| format!("#{t}")).collect();
    div()
        .id(("aweme", index))
        .flex()
        .flex_col()
        .gap_1()
        .p_3()
        .rounded_md()
        .bg(rgb(0x1e1e22))
        .child(div().text_sm().text_color(rgb(0x9fa8ff)).child(format!("@{}", aweme.author.handle())))
        .when(!aweme.desc.is_empty(), |el| el.child(aweme.desc.clone()))
        .when(!tags.is_empty(), |el| {
            el.child(div().text_xs().text_color(rgb(0x7fb4ff)).child(tags.join(" ")))
        })
        .child(div().text_xs().text_color(rgb(0x8a8a8a)).child(format!(
            "♥ {}   💬 {}   ▶ {}   {:.0}s",
            s.digg_count,
            s.comment_count,
            s.play_count,
            aweme.video.duration as f64 / 1000.0
        )))
}

impl Render for FeedView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let footer = if self.loading {
            div().text_color(rgb(0x8a8a8a)).child("Loading…")
        } else if let Some(err) = &self.error {
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(div().text_color(rgb(0xff7b7b)).child(err.clone()))
                .child(button("retry", "Retry", cx))
        } else if self.has_more {
            div().child(button("load-more", "Load more", cx))
        } else {
            div().text_color(rgb(0x8a8a8a)).child("End of feed")
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x121214))
            .text_color(rgb(0xececec))
            .child(div().px_4().py_3().text_lg().child("For You"))
            .child(
                div()
                    .id("feed")
                    .flex_1()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .px_3()
                    .pb_3()
                    .children(self.items.iter().enumerate().map(|(i, a)| card(i, a)))
                    .child(footer),
            )
    }
}

fn button(id: &'static str, label: &'static str, cx: &mut Context<FeedView>) -> impl IntoElement {
    div()
        .id(id)
        .px_3()
        .py_1()
        .rounded_md()
        .bg(rgb(0x2c2c32))
        .hover(|s| s.bg(rgb(0x3a3a42)))
        .cursor_pointer()
        .child(label)
        .on_click(cx.listener(|this, _, _, cx| this.load_more(cx)))
}
