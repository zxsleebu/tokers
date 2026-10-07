//! Comments of one video: the side panel in wide windows, the bottom sheet otherwise.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Entity, EventEmitter, FontWeight, Hsla, ListAlignment, ListScrollEvent, ListState,
    MouseButton, ObjectFit, Pixels, Render, SharedString, Window, div, img, list, px,
};
use tokers::TikTok;
use tokers::models::{Aweme, Comment, UrlList};

use crate::media::Images;
use crate::scrollbar::Scrollbar;
use crate::theme::{ActiveTheme as _, Text};
use crate::ui::{Button, compact, icon, skeleton, spinner};

const PAGE: u32 = 30;
/// Fetch the next page when the last loaded comment is this many rows away.
const PREFETCH_ROWS: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    Panel,
    Sheet,
}

pub enum CommentsEvent {
    Close,
}

#[derive(Default)]
struct Thread {
    open: bool,
    items: Vec<Comment>,
    cursor: u64,
    has_more: bool,
    loading: bool,
}

pub struct CommentsView {
    tiktok: TikTok,
    io: tokio::runtime::Handle,
    aweme: Aweme,
    items: Vec<Comment>,
    cursor: u64,
    has_more: bool,
    loading: bool,
    error: Option<SharedString>,
    total: u64,
    threads: HashMap<String, Thread>,
    /// Virtual list: only the rows on screen are built.
    list: ListState,
    scrollbar: Entity<Scrollbar>,
    desc_open: bool,
    variant: Variant,
}

impl EventEmitter<CommentsEvent> for CommentsView {}

impl CommentsView {
    pub fn new(tiktok: TikTok, io: tokio::runtime::Handle, aweme: Aweme, cx: &mut Context<Self>) -> Self {
        cx.observe(&Images::entity(cx), |_, _, cx| cx.notify()).detach();
        let total = aweme.statistics.comment_count;
        let list = ListState::new(2, ListAlignment::Top, px(400.));
        let scrollbar = Scrollbar::list(&list, cx.entity_id(), cx);
        list.set_scroll_handler(cx.listener(|this: &mut Self, e: &ListScrollEvent, _, cx| {
            this.scrollbar.update(cx, |bar, cx| bar.wake(cx));
            if e.visible_range.end + PREFETCH_ROWS >= this.row_count() {
                this.load_more(cx);
            }
        }));
        let mut this = CommentsView {
            tiktok,
            io,
            aweme,
            items: Vec::new(),
            cursor: 0,
            has_more: true,
            loading: false,
            error: None,
            total,
            threads: HashMap::new(),
            list,
            scrollbar,
            desc_open: false,
            variant: Variant::Panel,
        };
        this.load_more(cx);
        this
    }

    pub fn aweme_id(&self) -> &str {
        &self.aweme.aweme_id
    }

    pub fn set_variant(&mut self, variant: Variant, cx: &mut Context<Self>) {
        if self.variant != variant {
            self.variant = variant;
            self.list.reset(self.row_count());
            cx.notify();
        }
    }

    /// Rows before the comments: the author block (panel only).
    fn lead(&self) -> usize {
        usize::from(self.variant == Variant::Panel)
    }

    fn row_count(&self) -> usize {
        self.lead() + self.items.len() + 1
    }

    fn footer_ix(&self) -> usize {
        self.lead() + self.items.len()
    }

    fn remeasure(&self, ix: usize) {
        self.list.remeasure_items(ix..ix + 1);
    }

    fn comment_ix(&self, cid: &str) -> Option<usize> {
        self.items.iter().position(|c| c.cid == cid).map(|i| i + self.lead())
    }

    fn load_more(&mut self, cx: &mut Context<Self>) {
        if self.loading || !self.has_more {
            return;
        }
        self.loading = true;
        self.error = None;
        self.remeasure(self.footer_ix());
        let (tiktok, id, cursor) = (self.tiktok.clone(), self.aweme.aweme_id.clone(), self.cursor);
        let job = self.io.spawn(async move { tiktok.comments(&id, cursor, PAGE).await });
        cx.spawn(async move |this, cx| {
            let page = match job.await {
                Ok(page) => page.and_then(|p| p.status.check().map(|()| p)).map_err(|e| e.to_string()),
                Err(e) => Err(e.to_string()),
            };
            this.update(cx, |this, cx| {
                this.loading = false;
                let footer = this.footer_ix();
                match page {
                    Ok(page) => {
                        let known: std::collections::HashSet<String> =
                            this.items.iter().map(|c| c.cid.clone()).collect();
                        let before = this.items.len();
                        this.items.extend(page.comments.into_iter().filter(|c| !known.contains(&c.cid)));
                        // the new rows go where the footer was; the footer follows them
                        this.list.splice(footer..footer + 1, this.items.len() - before + 1);
                        this.cursor = page.cursor;
                        this.has_more = page.has_more;
                        if page.total > 0 {
                            this.total = page.total;
                        }
                    }
                    Err(e) => {
                        this.error = Some(e.into());
                        this.remeasure(footer);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn toggle_thread(&mut self, cid: &str, cx: &mut Context<Self>) {
        let thread = self.thread(cid);
        thread.open = !thread.open;
        if thread.open && thread.items.is_empty() {
            self.load_replies(cid.to_string(), cx);
        }
        if let Some(ix) = self.comment_ix(cid) {
            self.remeasure(ix);
        }
        cx.notify();
    }

    /// Hovering "Ответы" fetches the first page, so a click opens it at once.
    fn preload_thread(&mut self, cid: &str, cx: &mut Context<Self>) {
        if self.thread(cid).items.is_empty() {
            self.load_replies(cid.to_string(), cx);
        }
    }

    fn thread(&mut self, cid: &str) -> &mut Thread {
        self.threads.entry(cid.to_string()).or_insert_with(|| Thread { has_more: true, ..Thread::default() })
    }

    fn load_replies(&mut self, cid: String, cx: &mut Context<Self>) {
        let Some(thread) = self.threads.get_mut(&cid) else { return };
        if thread.loading || !thread.has_more {
            return;
        }
        thread.loading = true;
        let (open, cursor) = (thread.open, thread.cursor);
        if open && let Some(ix) = self.comment_ix(&cid) {
            self.remeasure(ix);
        }
        let (tiktok, id) = (self.tiktok.clone(), self.aweme.aweme_id.clone());
        let comment_id = cid.clone();
        let job = self.io.spawn(async move { tiktok.comment_replies(&id, &comment_id, cursor, 20).await });
        cx.spawn(async move |this, cx| {
            let page = job.await.ok().and_then(|r| r.ok()).filter(|p| p.status.is_ok());
            this.update(cx, |this, cx| {
                if let Some(thread) = this.threads.get_mut(&cid) {
                    thread.loading = false;
                    match page {
                        Some(page) => {
                            thread.items.extend(page.comments);
                            thread.cursor = page.cursor;
                            thread.has_more = page.has_more;
                        }
                        None => thread.has_more = false,
                    }
                    if thread.open
                        && let Some(ix) = this.comment_ix(&cid)
                    {
                        this.remeasure(ix);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn title(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = *cx.theme();
        div()
            .flex()
            .items_center()
            .justify_between()
            .px_5()
            .h(px(44.))
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Комментарии")
                    .child(div().text_color(theme.muted_foreground).child(compact(self.total))),
            )
            .when(self.variant == Variant::Sheet, |el| {
                el.child(
                    Button::new("close-comments")
                        .icon("icons/x.svg")
                        .small()
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(CommentsEvent::Close))),
                )
            })
            .into_any_element()
    }

    fn render_row(&mut self, ix: usize, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if self.variant == Variant::Panel && ix == 0 {
            return div().child(self.header(cx)).child(self.title(cx)).mb_4().into_any_element();
        }
        if ix >= self.footer_ix() {
            return div().px_5().pb_4().child(self.footer(cx)).into_any_element();
        }
        let comment = self.items[ix - self.lead()].clone();
        div().px_5().pb_5().child(self.row(&comment, 0, cx)).into_any_element()
    }

    fn header(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = *cx.theme();
        let author = &self.aweme.author;
        let avatar = Images::get(&author.avatar_thumb, 96, cx);
        let profile = format!("https://www.tiktok.com/@{}", author.handle());
        let music = &self.aweme.music;
        div()
            .flex()
            .flex_col()
            .gap_3()
            .px_5()
            .pt_5()
            .pb_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(avatar_el(
                        avatar,
                        px(40.),
                        &author.nickname,
                        theme.secondary,
                        theme.muted_foreground,
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .flex_1()
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .truncate()
                                    .child(author.nickname.clone()),
                            )
                            .child(
                                div()
                                    .text_size(theme.text(Text::Small))
                                    .text_color(theme.muted_foreground)
                                    .truncate()
                                    .child(format!("@{} · {}", author.handle(), ago(self.aweme.create_time))),
                            ),
                    )
                    .child(
                        Button::new("follow")
                            .primary()
                            .small()
                            .label("Профиль")
                            .on_click(move |_, _, cx| cx.open_url(&profile)),
                    ),
            )
            .when(!self.aweme.desc.is_empty(), |el| {
                let long = is_long(&self.aweme.desc);
                let open = self.desc_open;
                el.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(div().when(long && !open, |d| d.line_clamp(4)).child(rich(
                            &self.aweme.desc,
                            theme.foreground,
                            theme.primary,
                        )))
                        .when(long, |el| {
                            el.child(
                                more_toggle("desc-toggle", open, theme.muted_foreground, theme.foreground)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.desc_open = !this.desc_open;
                                        this.remeasure(0);
                                        cx.notify();
                                    })),
                            )
                        }),
                )
            })
            .when(!music.title.is_empty(), |el| {
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_size(theme.text(Text::Small))
                        .text_color(theme.muted_foreground)
                        .child(icon("icons/music-2.svg").size(px(14.)).text_color(theme.muted_foreground))
                        .child(div().truncate().child(format!("{} — {}", music.title, music.author))),
                )
            })
            .into_any_element()
    }

    fn row(&self, comment: &Comment, depth: usize, cx: &mut Context<Self>) -> AnyElement {
        let theme = *cx.theme();
        let size = if depth == 0 { px(32.) } else { px(24.) };
        let avatar = Images::get(&comment.user.avatar_thumb, 64, cx);
        let media: Vec<_> = comment
            .media()
            .into_iter()
            .filter(|m| !m.url.is_empty())
            .map(|m| Images::get(&UrlList { url_list: vec![m.url], ..UrlList::default() }, 320, cx))
            .collect();
        let cid = comment.cid.clone();
        let thread = self.threads.get(&cid);
        let replies = comment.reply_comment_total;
        let open = thread.is_some_and(|t| t.open);

        let mut body = div()
            .flex()
            .flex_col()
            .gap_1()
            .min_w_0()
            .flex_1()
            .child(
                div()
                    .text_size(theme.text(Text::Small))
                    .text_color(theme.muted_foreground)
                    .font_weight(FontWeight::MEDIUM)
                    .child(comment.user.nickname.clone()),
            )
            .when(!comment.text.is_empty(), |el| el.child(div().child(comment.text.clone())))
            .children(media.into_iter().map(|image| {
                // a fixed box: the row keeps its height whether or not the image is in yet
                div()
                    .size(px(160.))
                    .rounded(theme.radius)
                    .overflow_hidden()
                    .bg(theme.secondary)
                    .when_some(image, |el, image| {
                        el.child(img(image).size_full().object_fit(ObjectFit::Cover))
                    })
            }))
            .child(
                div()
                    .text_size(theme.text(Text::Tiny))
                    .text_color(theme.muted_foreground.opacity(0.75))
                    .child(ago(comment.create_time)),
            );

        if depth == 0 && replies > 0 {
            let label = if open {
                "Скрыть ответы".to_string()
            } else {
                format!("Ответы: {}", compact(replies))
            };
            let toggle_id = cid.clone();
            let hover_id = cid.clone();
            body = body.child(
                div()
                    .id(SharedString::from(format!("thread-{cid}")))
                    .flex()
                    .items_center()
                    .gap_2()
                    .mt_1()
                    .text_size(theme.text(Text::Small))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.muted_foreground)
                    .cursor_pointer()
                    .hover(|s| s.text_color(theme.foreground))
                    .child(div().w(px(20.)).h(px(1.)).bg(theme.border))
                    .child(label)
                    .child(
                        icon(if open { "icons/chevron-up.svg" } else { "icons/chevron-down.svg" })
                            .size(px(14.))
                            .text_color(theme.muted_foreground),
                    )
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.preload_thread(&hover_id, cx);
                        }
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_thread(&toggle_id, cx))),
            );
            if let Some(thread) = thread.filter(|t| t.open) {
                let replies: Vec<AnyElement> = thread.items.iter().map(|r| self.row(r, 1, cx)).collect();
                body = body.child(div().flex().flex_col().gap_3().mt_2().children(replies));
                if thread.loading {
                    body = body.child(div().py_1().child(spinner(
                        SharedString::from(format!("spin-{cid}")),
                        px(14.),
                        theme.muted_foreground,
                    )));
                } else if thread.has_more && !thread.items.is_empty() {
                    let more_id = cid.clone();
                    body = body.child(
                        div()
                            .id(SharedString::from(format!("more-{cid}")))
                            .text_size(theme.text(Text::Small))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.muted_foreground)
                            .cursor_pointer()
                            .hover(|s| s.text_color(theme.foreground))
                            .child("Ещё ответы")
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.load_replies(more_id.clone(), cx)),
                            ),
                    );
                }
            }
        }

        div()
            .flex()
            .gap_3()
            .child(avatar_el(avatar, size, &comment.user.nickname, theme.secondary, theme.muted_foreground))
            .child(body)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_0p5()
                    .pt_4()
                    .text_size(theme.text(Text::Tiny))
                    .text_color(theme.muted_foreground)
                    .child(icon("icons/heart.svg").size(px(14.)).text_color(theme.muted_foreground))
                    .when(comment.digg_count > 0, |el| el.child(compact(comment.digg_count))),
            )
            .into_any_element()
    }
}

impl CommentsView {
    fn footer(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = *cx.theme();
        if self.loading && self.items.is_empty() {
            div()
                .flex()
                .flex_col()
                .gap_5()
                .children((0..6usize).map(|i| {
                    div()
                        .flex()
                        .gap_3()
                        .child(skeleton(("sk-a", i), |d| d.size(px(32.)).rounded_full(), cx))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .flex_1()
                                .child(skeleton(("sk-b", i), |d| d.h(px(10.)).w(px(90.)), cx))
                                .child(skeleton(
                                    ("sk-c", i),
                                    |d| d.h(px(12.)).w(gpui::relative(0.5 + (i % 3) as f32 * 0.15)),
                                    cx,
                                )),
                        )
                }))
                .into_any_element()
        } else if self.loading {
            div()
                .flex()
                .justify_center()
                .py_3()
                .child(spinner("comments-spin", px(16.), theme.muted_foreground))
                .into_any_element()
        } else if let Some(err) = &self.error {
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap_2()
                .py_4()
                .text_size(theme.text(Text::Small))
                .text_color(theme.muted_foreground)
                .child(err.clone())
                .child(Button::new("retry-comments").secondary().small().label("Повторить").on_click(
                    cx.listener(|this, _, _, cx| {
                        this.error = None;
                        this.load_more(cx);
                    }),
                ))
                .into_any_element()
        } else if self.items.is_empty() {
            div()
                .py_6()
                .flex()
                .justify_center()
                .text_color(theme.muted_foreground)
                .child("Комментариев пока нет")
                .into_any_element()
        } else if self.has_more {
            // a short page leaves nothing to scroll: offer the next one
            div()
                .flex()
                .justify_center()
                .py_2()
                .child(
                    Button::new("more-comments")
                        .secondary()
                        .small()
                        .label("Ещё комментарии")
                        .on_click(cx.listener(|this, _, _, cx| this.load_more(cx))),
                )
                .into_any_element()
        } else {
            div().h(px(8.)).into_any_element()
        }
    }
}

impl Render for CommentsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        if self.list.item_count() != self.row_count() {
            self.list.reset(self.row_count());
        }
        div()
            .id("comments")
            .size_full()
            .flex()
            .flex_col()
            .min_h_0()
            .text_size(theme.text(Text::Body))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                let hovered = *hovered;
                this.scrollbar.update(cx, |bar, cx| bar.set_hovered(hovered, cx));
            }))
            .when(self.variant == Variant::Sheet, |el| el.child(self.title(cx)))
            .child(
                // the author block is a row of the list: a long description scrolls away with it
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        list(
                            self.list.clone(),
                            cx.processor(|this, ix: usize, window, cx| this.render_row(ix, window, cx)),
                        )
                        .size_full(),
                    )
                    .child(self.scrollbar.clone()),
            )
    }
}

pub fn avatar_el(
    image: Option<std::sync::Arc<gpui::RenderImage>>,
    size: Pixels,
    name: &str,
    bg: Hsla,
    fg: Hsla,
) -> AnyElement {
    match image {
        Some(image) => img(image).size(size).flex_none().rounded_full().into_any_element(),
        None => div()
            .size(size)
            .flex_none()
            .rounded_full()
            .bg(bg)
            .flex()
            .items_center()
            .justify_center()
            .text_size(size * 0.4)
            .text_color(fg)
            .child(name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default())
            .into_any_element(),
    }
}

/// Description text with #hashtags and @mentions in the accent colour.
pub fn rich(text: &str, color: Hsla, accent: Hsla) -> impl IntoElement {
    let text = &tidy(text);
    let mut runs: Vec<(String, bool)> = Vec::new();
    for word in text.split_inclusive(char::is_whitespace) {
        let tag = word.starts_with('#') || word.starts_with('@');
        match runs.last_mut() {
            Some((s, t)) if *t == tag => s.push_str(word),
            _ => runs.push((word.to_string(), tag)),
        }
    }
    let highlights: Vec<(std::ops::Range<usize>, gpui::HighlightStyle)> = {
        let mut at = 0;
        let mut out = Vec::new();
        for (s, tag) in &runs {
            if *tag {
                out.push((
                    at..at + s.trim_end().len(),
                    gpui::HighlightStyle { color: Some(accent), ..Default::default() },
                ));
            }
            at += s.len();
        }
        out
    };
    div().text_color(color).child(gpui::StyledText::new(text.to_string()).with_highlights(highlights))
}

/// Trimmed, with runs of blank lines (a common way to push hashtags out of
/// sight in the app) collapsed to one.
pub fn tidy(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blank = 0;
    for line in text.trim().lines() {
        let line = line.trim_end();
        if line.trim().is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line);
    }
    out
}

/// Worth folding: more than a few lines' worth.
pub fn is_long(text: &str) -> bool {
    let text = tidy(text);
    text.chars().count() > 140 || text.lines().count() > 3
}

/// "Ещё" / "Свернуть" under a folded text.
pub fn more_toggle(id: &'static str, open: bool, color: Hsla, hover: Hsla) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_1()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(color)
        .cursor_pointer()
        .hover(move |s| s.text_color(hover))
        .child(if open { "Свернуть" } else { "Ещё" })
        .child(
            icon(if open { "icons/chevron-up.svg" } else { "icons/chevron-down.svg" })
                .size(px(14.))
                .text_color(color),
        )
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

pub fn ago(ts: i64) -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(ts);
    let s = (now - ts).max(0);
    match s {
        0..60 => "только что".into(),
        60..3600 => format!("{} мин", s / 60),
        3600..86400 => format!("{} ч", s / 3600),
        86400..604800 => format!("{} дн", s / 86400),
        604800..2_592_000 => format!("{} нед", s / 604800),
        2_592_000..31_536_000 => format!("{} мес", s / 2_592_000),
        _ => format!("{} г", s / 31_536_000),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn tidy_collapses_blank_runs() {
        assert_eq!(super::tidy("  a\n\n\n\n.\n\n#tag  "), "a\n\n.\n\n#tag");
        assert!(!super::is_long("a\n\n\n\n\nb"));
    }
}
