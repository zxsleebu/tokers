//! Comments of one video: the side panel in wide windows, the bottom sheet otherwise.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui::prelude::*;
use gpui::{
    AnyElement, Bounds, Context, Entity, EventEmitter, FontWeight, Hsla, ListAlignment, ListScrollEvent,
    ListState, MouseButton, ObjectFit, Pixels, Render, ScrollWheelEvent, SharedString, Window, anchored,
    canvas, deferred, div, img, list, point, px,
};
use tokers::TikTok;
use tokers::models::{Aweme, Comment, UrlList};

use crate::media::Images;
use crate::motion::Rising as _;
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

/// How the loaded comments are shown. The API of the captured app version has no
/// sorting or filters (later versions do), so this works on what has been fetched;
/// a filter keeps fetching a few pages while it has little to show.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Order {
    Top,
    Newest,
    Media,
    Creator,
}

impl Order {
    const ALL: [Order; 4] = [Order::Top, Order::Newest, Order::Media, Order::Creator];

    fn label(self) -> &'static str {
        match self {
            Order::Top => "Популярные",
            Order::Newest => "Сначала новые",
            Order::Media => "С медиа",
            Order::Creator => "От автора",
        }
    }

    fn filters(self) -> bool {
        matches!(self, Order::Media | Order::Creator)
    }
}

/// A filter fetches on its own until it shows this many comments...
const FILTER_FILL: usize = 12;
/// ...or has gone through this many pages since it was picked.
const FILTER_PAGES: u32 = 8;

/// The reader's mark on a comment (kept for this session, not sent anywhere).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Vote {
    Up,
    Down,
}

/// Comment rows, measured off the phone app: avatar 12 from the left, likes 14 from the right.
const ROW_LEFT: f32 = 12.;
const ROW_RIGHT: f32 = 14.;

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
    votes: HashMap<String, Vote>,
    /// The first page is not asked for yet (see `new`): the rows show as loading.
    waiting: bool,
    order: Order,
    /// The rows: indices into `items` in the order and under the filter chosen.
    shown: Vec<usize>,
    /// Pages a filter has fetched by itself since it was picked.
    filter_pages: u32,
    sort_open: bool,
    /// Where the title (the way to the menu, which hangs under it) was painted.
    title_at: Rc<Cell<Bounds<Pixels>>>,
}

impl EventEmitter<CommentsEvent> for CommentsView {}

impl CommentsView {
    /// `fetch_after`: how long to wait before asking for the first page (paging past a video
    /// drops its view, and the request with it, before it goes out). Everything the video
    /// itself carries (author, description, sound) shows at once.
    pub fn new(
        tiktok: TikTok,
        io: tokio::runtime::Handle,
        aweme: Aweme,
        fetch_after: std::time::Duration,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&Images::entity(cx), |_, _, cx| cx.notify()).detach();
        let total = aweme.statistics.comment_count;
        let list = ListState::new(2, ListAlignment::Top, px(400.));
        let scrollbar = Scrollbar::list(&list, cx.entity_id(), cx);
        list.set_scroll_handler(cx.listener(|this: &mut Self, e: &ListScrollEvent, _, cx| {
            this.scrollbar.update(cx, |bar, cx| bar.wake(cx));
            if this.sort_open {
                // the menu would be left hanging where the button was
                this.sort_open = false;
                cx.notify();
            }
            if e.visible_range.end + PREFETCH_ROWS >= this.row_count() {
                // The list calls this while it holds its own state, and loading touches the
                // list (the footer is remeasured): load once it has let go.
                cx.spawn(async move |this, cx| this.update(cx, |this, cx| this.load_more(cx)).ok()).detach();
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
            votes: HashMap::new(),
            waiting: false,
            order: Order::Top,
            shown: Vec::new(),
            filter_pages: 0,
            sort_open: false,
            title_at: Rc::default(),
        };
        if fetch_after.is_zero() {
            this.load_more(cx);
        } else {
            this.waiting = true;
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(fetch_after).await;
                this.update(cx, |this, cx| {
                    this.waiting = false;
                    this.load_more(cx);
                })
                .ok();
            })
            .detach();
        }
        this
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
        self.lead() + self.shown.len() + 1
    }

    fn footer_ix(&self) -> usize {
        self.lead() + self.shown.len()
    }

    fn matches(&self, comment: &Comment) -> bool {
        let author = &self.aweme.author.uid;
        match self.order {
            Order::Top | Order::Newest => true,
            Order::Media => !comment.media().is_empty(),
            Order::Creator => {
                &comment.user.uid == author || comment.reply_comment.iter().any(|r| &r.user.uid == author)
            }
        }
    }

    /// Rebuilds the rows for the current order, keeping the reader where they were.
    fn arrange(&mut self) {
        let mut shown: Vec<usize> = (0..self.items.len()).filter(|&i| self.matches(&self.items[i])).collect();
        if self.order == Order::Newest {
            shown.sort_by_key(|&i| std::cmp::Reverse(self.items[i].create_time));
        }
        self.shown = shown;
        let top = self.list.logical_scroll_top();
        self.list.reset(self.row_count());
        self.list.scroll_to(top);
    }

    fn set_order(&mut self, order: Order, cx: &mut Context<Self>) {
        self.sort_open = false;
        if self.order != order {
            self.order = order;
            self.filter_pages = 0;
            self.arrange();
            self.list.scroll_to(gpui::ListOffset::default());
            self.fill_filter(cx);
        }
        cx.notify();
    }

    /// A filter with little to show fetches the next page by itself.
    fn fill_filter(&mut self, cx: &mut Context<Self>) {
        if self.order.filters() && self.shown.len() < FILTER_FILL && self.filter_pages < FILTER_PAGES {
            self.filter_pages += 1;
            self.load_more(cx);
        }
    }

    fn remeasure(&self, ix: usize) {
        self.list.remeasure_items(ix..ix + 1);
    }

    fn comment_ix(&self, cid: &str) -> Option<usize> {
        self.shown.iter().position(|&i| self.items[i].cid == cid).map(|i| i + self.lead())
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
                        this.cursor = page.cursor;
                        this.has_more = page.has_more;
                        if page.total > 0 {
                            this.total = page.total;
                        }
                        if this.order == Order::Top {
                            // the new rows go where the footer was; the footer follows them
                            this.shown.extend(before..this.items.len());
                            this.list.splice(footer..footer + 1, this.items.len() - before + 1);
                        } else {
                            this.arrange();
                            this.fill_filter(cx);
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

    /// A second click on the same mark takes it back; the other mark replaces it.
    fn vote(&mut self, cid: &str, vote: Vote, cx: &mut Context<Self>) {
        if self.votes.get(cid) == Some(&vote) {
            self.votes.remove(cid);
        } else {
            self.votes.insert(cid.to_string(), vote);
        }
        cx.notify();
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

    /// The order icon, "Комментарии" and the count, which together open the order menu: on
    /// the left of the panel, centred on the sheet (with its close button on the right).
    fn title(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = *cx.theme();
        let sheet = self.variant == Variant::Sheet;
        let sorted = self.order != Order::Top;
        let at = self.title_at.clone();
        let label = div()
            .id("comments-title")
            .relative()
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .py_1()
            .rounded(theme.radius)
            .cursor_pointer()
            .hover(|s| s.bg(theme.secondary_hover))
            .font_weight(FontWeight::SEMIBOLD)
            .child(div().mr_1().child(icon("icons/list-filter.svg").size(px(16.)).text_color(if sorted {
                theme.primary
            } else {
                theme.muted_foreground
            })))
            .child("Комментарии")
            .child(div().ml_1().text_color(theme.muted_foreground).child(compact(self.total)))
            .child(canvas(move |bounds, _, _| at.set(bounds), |_, _, _, _| {}).absolute().inset_0())
            .on_click(cx.listener(|this, _, _, cx| {
                cx.stop_propagation();
                this.sort_open = !this.sort_open;
                cx.notify();
            }));
        div()
            .relative()
            .flex()
            .items_center()
            .when(sheet, |el| el.justify_center())
            .px(px(ROW_LEFT - 8.))
            .h(px(44.))
            .border_b_1()
            .border_color(theme.border)
            .child(label)
            .when(sheet, |el| {
                el.child(
                    div().absolute().right(px(ROW_RIGHT)).top_0().bottom_0().flex().items_center().child(
                        Button::new("close-comments")
                            .icon("icons/x.svg")
                            .small()
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(CommentsEvent::Close))),
                    ),
                )
            })
            .into_any_element()
    }

    /// The order menu, hung under the title: centred on the sheet, from its left edge in the panel.
    fn sort_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = *cx.theme();
        let at = self.title_at.get();
        let centred = self.variant == Variant::Sheet;
        let x = if centred { at.origin.x + at.size.width / 2. - px(100.) } else { at.origin.x };
        let current = self.order;
        let menu = div()
            .id("sort-menu")
            .occlude()
            .min_w(px(200.))
            .p_1()
            .flex()
            .flex_col()
            .rounded(theme.radius)
            .bg(theme.popover)
            .border_1()
            .border_color(theme.border)
            .shadow_lg()
            .text_size(theme.text(Text::Label))
            .on_mouse_down_out(cx.listener(|this, e: &gpui::MouseDownEvent, _, cx| {
                // the sort button toggles the menu itself
                if !this.title_at.get().contains(&e.position) {
                    this.sort_open = false;
                    cx.notify();
                }
            }))
            .children(Order::ALL.into_iter().map(|order| {
                let on = order == current;
                div()
                    .id(SharedString::from(format!("sort-{}", order.label())))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .px_3()
                    .py_2()
                    .rounded(theme.radius)
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.secondary))
                    .text_color(if on { theme.foreground } else { theme.muted_foreground })
                    .child(order.label())
                    .when(on, |el| el.child(icon("icons/check.svg").size(px(16.)).text_color(theme.primary)))
                    .on_click(cx.listener(move |this, _, _, cx| this.set_order(order, cx)))
            }))
            .rising("sort-menu-in");
        deferred(
            anchored()
                .position(point(x, at.origin.y + at.size.height + px(6.)))
                .snap_to_window_with_margin(px(8.))
                .child(menu),
        )
        .with_priority(1)
        .into_any_element()
    }

    fn render_row(&mut self, ix: usize, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        // padding, not margins: the list measures a row by its box
        if self.variant == Variant::Panel && ix == 0 {
            return div().child(self.header(cx)).child(self.title(cx)).into_any_element();
        }
        // room under the title for the first row
        let first = ix == self.lead();
        if ix >= self.footer_ix() {
            return div()
                .pl(px(ROW_LEFT))
                .pr(px(ROW_RIGHT))
                .pb_4()
                .when(first, |el| el.pt_4())
                .child(self.footer(cx))
                .into_any_element();
        }
        let comment = self.items[self.shown[ix - self.lead()]].clone();
        div()
            .pl(px(ROW_LEFT))
            .pr(px(ROW_RIGHT))
            .pb_5()
            .when(first, |el| el.pt_4())
            .child(self.row(&comment, 0, cx))
            .into_any_element()
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
        let size = if depth == 0 { px(36.) } else { px(24.) };
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
            .child(self.meta(comment, cx));

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
                let rows: Vec<AnyElement> = thread.items.iter().map(|r| self.row(r, 1, cx)).collect();
                body = body.child(div().flex().flex_col().gap_3().mt_2().children(rows));
                if thread.loading {
                    body = body.child(div().py_1().child(spinner(
                        SharedString::from(format!("spin-{cid}")),
                        px(14.),
                        theme.muted_foreground,
                    )));
                } else if thread.has_more && !thread.items.is_empty() {
                    let more_id = cid.clone();
                    let left = replies.saturating_sub(thread.items.len() as u64);
                    body = body.child(
                        div()
                            .id(SharedString::from(format!("more-{cid}")))
                            .text_size(theme.text(Text::Small))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.muted_foreground)
                            .cursor_pointer()
                            .hover(|s| s.text_color(theme.foreground))
                            .child(match left {
                                0 => "Ещё ответы".to_string(),
                                n => format!("Ещё ответы: {}", compact(n)),
                            })
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.load_replies(more_id.clone(), cx)),
                            ),
                    );
                }
            }
        }

        div()
            .flex()
            .gap_2()
            .child(avatar_el(avatar, size, &comment.user.nickname, theme.secondary, theme.muted_foreground))
            .child(body)
            .into_any_element()
    }

    /// Age on the left; like (with its count) and dislike on the right, as in the app.
    fn meta(&self, comment: &Comment, cx: &mut Context<Self>) -> AnyElement {
        let theme = *cx.theme();
        let vote = self.votes.get(&comment.cid).copied();
        let liked = vote == Some(Vote::Up);
        let likes = comment.digg_count + u64::from(liked);
        let mark = |key: &str, path: &'static str, on: bool, vote: Vote, cx: &mut Context<Self>| {
            let cid = comment.cid.clone();
            let color = if on { theme.primary } else { theme.muted_foreground };
            div()
                .id(SharedString::from(format!("{key}-{}", comment.cid)))
                .flex()
                .items_center()
                .gap_1()
                .cursor_pointer()
                .text_color(color)
                .hover(|s| s.text_color(if on { theme.primary } else { theme.foreground }))
                .child(icon(path).size(px(20.)).text_color(color))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.vote(&cid, vote, cx);
                }))
        };
        div()
            .flex()
            .items_center()
            .mt_1()
            .text_size(theme.text(Text::Small))
            .text_color(theme.muted_foreground)
            .child(
                div()
                    .flex_1()
                    .text_color(theme.muted_foreground.opacity(0.75))
                    .child(ago(comment.create_time)),
            )
            .child(
                // a fixed slot, so the dislikes line up whatever the count
                div().w(px(64.)).flex().child(
                    mark(
                        "like",
                        if liked { "icons/heart-filled.svg" } else { "icons/heart.svg" },
                        liked,
                        Vote::Up,
                        cx,
                    )
                    .when(likes > 0, |el| el.child(compact(likes))),
                ),
            )
            .child(mark(
                "dislike",
                if vote == Some(Vote::Down) {
                    "icons/thumbs-down-filled.svg"
                } else {
                    "icons/thumbs-down.svg"
                },
                vote == Some(Vote::Down),
                Vote::Down,
                cx,
            ))
            .into_any_element()
    }
}

impl CommentsView {
    fn footer(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = *cx.theme();
        if (self.loading || self.waiting) && self.items.is_empty() {
            div()
                .flex()
                .flex_col()
                .gap_5()
                .children((0..6usize).map(|i| {
                    div()
                        .flex()
                        .gap_2()
                        .child(skeleton(("sk-a", i), |d| d.size(px(36.)).rounded_full(), cx))
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
        } else if self.shown.is_empty() && !self.has_more {
            div()
                .py_6()
                .flex()
                .justify_center()
                .text_color(theme.muted_foreground)
                .child("Таких комментариев нет")
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
                        .label(if self.order.filters() {
                            "Искать дальше"
                        } else {
                            "Ещё комментарии"
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.filter_pages = 0;
                            this.load_more(cx);
                        })),
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
        self.scrollbar.read(cx).sync();
        let bar = self.scrollbar.clone();
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
            .when(self.sort_open, |el| el.child(self.sort_menu(cx)))
            .child(
                // the author block is a row of the list: a long description scrolls away with it
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .on_scroll_wheel(move |e: &ScrollWheelEvent, window, cx| {
                        bar.update(cx, |bar, cx| bar.wheel(e, window, cx))
                    })
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
