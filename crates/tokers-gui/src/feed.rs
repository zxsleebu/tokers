//! The watch screen: a vertical pager of videos (or one list, like favourites),
//! the action buttons beside them and the comments, placed by [`Layout`].

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::AnimationExt as _;
use gpui::prelude::*;
use gpui::{
    AnyElement, AnyView, Bounds, Context, Entity, EventEmitter, FontWeight, Hsla, MouseButton, ObjectFit,
    Pixels, Point, Render, ScrollDelta, ScrollWheelEvent, SharedString, Size, SpringConfig, StyleRefinement,
    Task, TouchPhase, Window, canvas, div, img, linear_color_stop, linear_gradient, px, size,
};
use tokers::TikTok;
use tokers::endpoints::Feed;
use tokers::models::Aweme;

use crate::ambilight::Ambilight;
use crate::comments::{CommentsEvent, CommentsView, Variant, avatar_el, is_long, more_toggle, rich};
use crate::layout::{Layout, Mode, Rect, TITLEBAR};
use crate::media::Images;
use crate::motion::{Motion, Motioned as _, Rising as _, Spring, Springs, mix};
use crate::player::{Glimpse, VideoPlayer};
use crate::state::{CommentsMode, Store, downloads_dir};
use crate::theme::{ActiveTheme as _, Text};
use crate::ui::{Button, compact, icon, spinner};

/// Fetch the next feed page when this few videos are left ahead.
const LOOKAHEAD: usize = 3;
const WHEEL_COOLDOWN: Duration = Duration::from_millis(260);
/// Touchpad: how far (share of the video height) a swipe goes before it turns the page.
const SWIPE_COMMIT: f32 = 0.18;
const GESTURE_GAP: Duration = Duration::from_millis(160);
/// The comments panel has no background of its own: the glow beside the video dies out
/// this far into it (px), on its own curve, rather than lying under the text.
const GLOW_INTO_PANEL: f32 = 24.;
/// The cover-coloured backdrop (`ambient`) shows through the panel, dimmed from this far
/// left of its edge...
const VEIL_LEAD: f32 = 40.;
/// ...over this share of the panel (past the lead)...
const VEIL_REACH: f32 = 0.5;
/// ...to this dimness (share of the window background); the titlebar above keeps its own
/// lighter one, its border line marks the step.
const VEIL_DIM: f32 = 0.7;
/// Past the panel's right edge (a very wide window has room there) it lets go over this.
const VEIL_TAIL: f32 = 160.;
/// The dimming is eased (gradients are linear, two stops): drawn in this many pieces a ramp.
const VEIL_STEPS: usize = 12;
/// Comments (with their images) of this many videos already watched stay loaded...
const RECENT_KEPT: usize = 10;
/// ...and those of this many videos ahead are fetched once the current one has stayed.
const COMMENTS_AHEAD: usize = 2;
/// How long a video stays current before its comments are fetched.
const COMMENTS_DELAY: Duration = Duration::from_millis(350);
/// Light under a glyph or count (see `Glimpse::light_in`) where its shadow starts...
pub const SHADE_FROM: f32 = 0.32;
/// ...and where it is at full strength.
pub const SHADE_FULL: f32 = 0.6;
/// How hard the button faces blur what is behind them: little, liquid glass is mostly clear.
pub const FACE_BLUR: gpui::Pixels = px(5.);
/// Frosted faces (liquid glass turned off) blur harder instead.
pub const FROST_BLUR: gpui::Pixels = px(10.);
/// The button faces' lens: the rim pulls the picture in from behind, with a glint on top.
const FACE_GLASS: gpui::Glass =
    gpui::Glass { refraction: px(16.), bevel: px(10.), dispersion: 0.2, highlight: 2.5 };
/// The drop behind the hovered button bends harder.
const LENS_GLASS: gpui::Glass =
    gpui::Glass { refraction: px(22.), bevel: px(14.), dispersion: 0.3, highlight: 3. };
const FACE: f32 = 48.;
/// A button's height in the stack (padding, face, gap, count) before it has been painted once.
const COMMENTS_ROW: f32 = 8. + FACE + 4. + 15. + 8.;
/// The drop behind the hovered button is this much wider than a face.
const LENS_GROW: f32 = 10.;
/// The drop's slide between buttons: a little underdamped, so it overshoots as it lands.
const LENS_SLIDE: SpringConfig = SpringConfig::new(420., 26., 1.);
/// Its size: settles in with only a slight swell when it appears, and sinks on a press.
const LENS_POP: SpringConfig = SpringConfig::new(520., 30., 1.);
/// How much smaller the drop gets at full speed between buttons.
const LENS_SHRINK: f32 = 0.18;
/// Its width and height apart: loosely damped, so the drop jiggles like jelly after it moves.
const LENS_JELLY: SpringConfig = SpringConfig::new(650., 12., 1.);
/// Speed (px/s) at which the drop is stretched by half.
const LENS_STRETCH_SPEED: f32 = 1600.;
/// A pressed face squeezes in and springs back past its size.
const SQUISH: Duration = Duration::from_millis(560);

/// The glass drop behind the hovered button of the current stack, moved by springs as in
/// Sonora (state stepped every frame, velocity kept across retargets): it flows from button to
/// button stretched by its own speed, jiggles when it stops, pops in and out, and sinks under a
/// press.
struct Lens {
    hovered: Option<&'static str>,
    y: Spring,
    pop: Spring,
    wide: Spring,
    tall: Spring,
    /// Where each face of the current stack was painted, and the stack itself.
    faces: Rc<RefCell<HashMap<&'static str, Bounds<Pixels>>>>,
    origin: Rc<Cell<Point<Pixels>>>,
    /// Where each count under a face was painted.
    labels: Rc<RefCell<HashMap<&'static str, Bounds<Pixels>>>>,
    x: f32,
    /// This frame's drop.
    now: Drop,
}

/// One frame of the drop, relative to the stack.
#[derive(Clone, Copy)]
struct Drop {
    x: f32,
    y: f32,
    size: f32,
    wide: f32,
    tall: f32,
}

impl Lens {
    fn new() -> Self {
        Lens {
            hovered: None,
            y: Spring::new(LENS_SLIDE, 0.),
            pop: Spring::new(LENS_POP, 0.),
            wide: Spring::new(LENS_JELLY, 1.),
            tall: Spring::new(LENS_JELLY, 1.),
            faces: Rc::default(),
            origin: Rc::default(),
            labels: Rc::default(),
            x: 0.,
            now: Drop { x: 0., y: 0., size: 0., wide: 1., tall: 1. },
        }
    }

    fn tick(&mut self, window: &mut Window, cx: &gpui::App) {
        let origin = self.origin.get();
        let face = self.hovered.and_then(|key| self.faces.borrow().get(key).copied());
        if let Some(b) = face {
            // the drop sits on the face's centre (faces differ in size: the avatar is larger)
            let centre = b.center();
            let target = f32::from(centre.y - origin.y);
            self.x = f32::from(centre.x - origin.x);
            if self.pop.value() < 0.05 {
                // appear where the pointer is rather than slide in from the last spot
                self.y.snap(target);
            }
            self.y.set(target);
        }
        let y = self.y.tick(window, cx);
        let speed = (self.y.velocity().abs() / LENS_STRETCH_SPEED).min(1.2);
        // it draws in a little on the way between buttons and fills out as it lands
        self.pop.set(if face.is_some() { 1. - LENS_SHRINK * speed.min(1.) } else { 0. });
        // stretched along its way by its own speed, thinned to keep its volume
        let stretch = 1. + 0.5 * speed;
        self.tall.set(stretch);
        self.wide.set(1. / stretch.sqrt());
        self.now = Drop {
            x: self.x,
            y,
            size: self.pop.tick(window, cx).max(0.),
            wide: self.wide.tick(window, cx),
            tall: self.tall.tick(window, cx),
        };
    }

    /// A press: the drop sinks and splats sideways, then bounces back.
    fn press(&mut self) {
        self.pop.kick(-4.);
        self.wide.kick(5.);
        self.tall.kick(-5.);
    }
}

pub const COVER_PX: u32 = 720;

pub enum FeedEvent {
    Toast(SharedString),
    /// The video on screen changed: its cover drives the theme tint.
    Current,
}

pub enum Source {
    ForYou { max_cursor: i64 },
    List,
}

pub struct FeedView {
    tiktok: TikTok,
    io: tokio::runtime::Handle,
    source: Source,
    items: Vec<Aweme>,
    has_more: bool,
    loading: bool,
    error: Option<SharedString>,
    index: usize,
    /// Pager position in items (spring toward `index`, pulled by a swipe).
    pager: Spring,
    players: HashMap<String, Entity<VideoPlayer>>,
    comments: Option<Entity<CommentsView>>,
    /// Sheet openness, 0..1.
    sheet: Spring,
    sheet_open: bool,
    /// Window width before comments widened it (Expand mode).
    widened_from: Option<Size<Pixels>>,
    wheel_at: Instant,
    swipe: f32,
    swipe_done: bool,
    swipe_gen: u64,
    photo: HashMap<String, usize>,
    pops: HashMap<&'static str, (String, usize)>,
    lens: Lens,
    ambilight: Ambilight,
    /// How much each face's glyph (`false`) and count (`true`) needs a shadow, by how light it
    /// is right under it (0..1, eased per element).
    shades: HashMap<(&'static str, bool), Spring>,
    /// Presses per button (and the video they were on), keying each squish animation.
    presses: HashMap<&'static str, (String, usize)>,
    pub sidebar: bool,
    viewport: Size<Pixels>,
    current_since: Instant,
    /// The video `current_since` counts from.
    current_id: Option<String>,
    /// Comments views by video: the current one, the ones ahead, the recently watched.
    comment_views: HashMap<String, Entity<CommentsView>>,
    /// Videos that were current, the latest last (at most `RECENT_KEPT`).
    recent: VecDeque<String>,
    /// Fetches the comments ahead once the current video has stayed a moment.
    comments_ahead: Option<Task<()>>,
    /// The overlay caption shows its whole description.
    desc_open: bool,
    /// Window scale factor at the last render.
    scale: f32,
    /// The previous video, still on screen while the pager moves.
    leaving: Option<String>,
    /// The last layout's mode (its thresholds hold a little longer); `None` before the first.
    mode: Option<Mode>,
    /// 0 = medium arrangement, 1 = wide (with the panel); springs on a switch.
    wide_anim: Spring,
    /// 0 = buttons on the video, 1 = beside it; springs on a switch.
    beside_anim: Spring,
    /// The buttons are on the video, or on their way to or from it: the caption keeps
    /// clear of them (it rewraps once, not on every frame of the move).
    clear_buttons: bool,
    /// How far into wide mode (0..1, the spring): the panel shows the comments, so their
    /// button folds away.
    wide_t: f32,
}

impl EventEmitter<FeedEvent> for FeedView {}

impl FeedView {
    pub fn for_you(tiktok: TikTok, io: tokio::runtime::Handle, cx: &mut Context<Self>) -> Self {
        let mut this = Self::with(tiktok, io, Source::ForYou { max_cursor: 0 }, Vec::new(), 0, cx);
        this.load_more(cx);
        this
    }

    pub fn list(
        tiktok: TikTok,
        io: tokio::runtime::Handle,
        items: Vec<Aweme>,
        start: usize,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self::with(tiktok, io, Source::List, items, start, cx);
        this.has_more = false;
        this.sync_players(cx);
        this
    }

    fn with(
        tiktok: TikTok,
        io: tokio::runtime::Handle,
        source: Source,
        items: Vec<Aweme>,
        index: usize,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&Images::entity(cx), |_, _, cx| cx.notify()).detach();
        cx.observe(&Store::entity(cx), |this, _, cx| this.apply_volume(cx)).detach();
        FeedView {
            tiktok,
            io,
            source,
            items,
            has_more: true,
            loading: false,
            error: None,
            index,
            pager: Spring::new(Springs::PAGE, index as f32),
            players: HashMap::new(),
            comments: None,
            sheet: Spring::new(Springs::PANEL, 0.),
            sheet_open: false,
            widened_from: None,
            wheel_at: Instant::now() - WHEEL_COOLDOWN,
            swipe: 0.,
            swipe_done: false,
            swipe_gen: 0,
            photo: HashMap::new(),
            pops: HashMap::new(),
            lens: Lens::new(),
            ambilight: Ambilight::new(),
            shades: HashMap::new(),
            presses: HashMap::new(),
            sidebar: true,
            viewport: size(px(720.), px(1280.)),
            current_since: Instant::now(),
            current_id: None,
            comment_views: HashMap::new(),
            recent: VecDeque::new(),
            comments_ahead: None,
            desc_open: false,
            scale: 1.,
            leaving: None,
            mode: None,
            wide_anim: Spring::new(Springs::PANEL, 0.),
            beside_anim: Spring::new(Springs::PANEL, 1.),
            clear_buttons: false,
            wide_t: 0.,
        }
    }

    pub fn current(&self) -> Option<&Aweme> {
        self.items.get(self.index)
    }

    fn load_more(&mut self, cx: &mut Context<Self>) {
        let Source::ForYou { max_cursor } = self.source else { return };
        if self.loading || !self.has_more {
            return;
        }
        self.loading = true;
        self.error = None;
        cx.notify();
        let tiktok = self.tiktok.clone();
        let req = Feed { count: 8, max_cursor, is_cold_start: max_cursor == 0, ..Feed::default() };
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
                        this.source = Source::ForYou { max_cursor: page.max_cursor };
                        this.has_more = page.has_more;
                        let seen: std::collections::HashSet<String> =
                            this.items.iter().map(|a| a.aweme_id.clone()).collect();
                        this.items.extend(
                            page.aweme_list
                                .into_iter()
                                .filter(|a| !seen.contains(&a.aweme_id) && playable(a)),
                        );
                        this.sync_players(cx);
                        cx.emit(FeedEvent::Current);
                    }
                    Err(e) => this.error = Some(e.into()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Keep players for the current video (playing) and the next (prerolled, paused).
    fn sync_players(&mut self, cx: &mut Context<Self>) {
        let mut keep: Vec<String> = [self.index, self.index + 1]
            .iter()
            .filter_map(|&i| self.items.get(i))
            .map(|a| a.aweme_id.clone())
            .collect();
        if let Some(leaving) = self.leaving.clone().filter(|id| !keep.contains(id)) {
            keep.push(leaving);
        }
        let stale: Vec<String> = self.players.keys().filter(|id| !keep.contains(id)).cloned().collect();
        for id in stale {
            if let Some(player) = self.players.remove(&id) {
                player.update(cx, |p, cx| p.shutdown(None, cx));
            }
        }
        let prefs = Store::prefs(cx).clone();
        // decode for the size the column is shown at, in steps so a resize doesn't matter much
        let max_height = ((f32::from(self.viewport.height) * self.scale).ceil() as u32)
            .next_multiple_of(240)
            .clamp(480, 1920);
        for (slot, id) in keep.iter().enumerate() {
            // the clip sliding out keeps playing, silently, until it is gone
            let leaving = self.leaving.as_ref() == Some(id) && slot != 0;
            let play = slot == 0 || leaving;
            if let Some(player) = self.players.get(id) {
                player.update(cx, |p, cx| {
                    p.set_silent(leaving);
                    p.set_playing(play, cx);
                });
                continue;
            }
            let Some(aweme) = self.items.iter().find(|a| &a.aweme_id == id) else { continue };
            let Some(uri) = aweme.video.play_addr.first().map(str::to_string) else { continue };
            let photo = aweme.is_photo();
            let player = cx.new(|cx| {
                let mut p = VideoPlayer::new(&uri, max_height, play, prefs.volume as f64, prefs.muted, cx);
                p.chrome = !photo;
                p
            });
            cx.observe(&player, |_, _, cx| cx.notify()).detach();
            self.players.insert(id.clone(), player);
        }
        // a new current video: its comments start over. (Comparing with the comments view
        // alone restarted the wait on every call while there was none yet, and with calls
        // coming often enough the comments never came.)
        let current = self.current().map(|a| a.aweme_id.clone());
        if current != self.current_id {
            if let Some(left) = self.current_id.take() {
                self.recent.retain(|id| *id != left);
                self.recent.push_back(left);
                while self.recent.len() > RECENT_KEPT {
                    self.recent.pop_front();
                }
            }
            self.current_id = current.clone();
            // back on a video seen a moment ago: its comments are as they were left
            self.comments = current.as_ref().and_then(|id| self.comment_views.get(id).cloned());
            self.current_since = Instant::now();
            self.keep_only_recent(cx);
            self.comments_ahead = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(COMMENTS_DELAY).await;
                this.update(cx, |this, cx| {
                    if this.current_id == current {
                        this.fetch_comments_ahead(cx);
                    }
                })
                .ok();
            }));
        }
        if self.items.len().saturating_sub(self.index + 1) < LOOKAHEAD {
            self.load_more(cx);
        }
    }

    /// Comments of the current video, created once they are on screen and the
    /// video has stayed current a moment (paging through doesn't fetch each one).
    /// The current video's comments: the view at once (the video's own author, description
    /// and sound show straight away), the first page only once the video has stayed current
    /// a moment, so paging through doesn't fetch each one.
    fn ensure_comments(&mut self, cx: &mut Context<Self>) {
        if self.comments.is_some() {
            return;
        }
        let Some(aweme) = self.current().cloned() else { return };
        let wait = COMMENTS_DELAY.saturating_sub(self.current_since.elapsed());
        self.comments = Some(self.comments_view(aweme, wait, cx));
    }

    /// The comments view of a video, made (and kept) if there is none yet.
    fn comments_view(&mut self, aweme: Aweme, wait: Duration, cx: &mut Context<Self>) -> Entity<CommentsView> {
        if let Some(view) = self.comment_views.get(&aweme.aweme_id) {
            return view.clone();
        }
        let id = aweme.aweme_id.clone();
        let height = f32::from(self.viewport.height);
        // the comments being looked at (or about to be) go ahead of feed prefetches
        let view =
            cx.new(|cx| CommentsView::new(self.tiktok.urgent(), self.io.clone(), aweme, wait, height, cx));
        cx.subscribe(&view, |this, _, event, cx| match event {
            CommentsEvent::Close => this.close_comments(cx),
        })
        .detach();
        self.comment_views.insert(id, view.clone());
        view
    }

    /// The current video's comments and the next ones', first page and images, so they
    /// are there when opened or paged to.
    fn fetch_comments_ahead(&mut self, cx: &mut Context<Self>) {
        let ahead: Vec<Aweme> = self.items.iter().skip(self.index).take(1 + COMMENTS_AHEAD).cloned().collect();
        for aweme in ahead {
            self.comments_view(aweme, Duration::ZERO, cx);
        }
    }

    /// Videos whose comments (and images) stay: the recently watched, the current one and
    /// those ahead.
    fn kept_ids(&self) -> HashSet<String> {
        let ahead = self.items.iter().skip(self.index).take(1 + COMMENTS_AHEAD).map(|a| a.aweme_id.clone());
        self.recent.iter().cloned().chain(ahead).collect()
    }

    fn keep_only_recent(&mut self, cx: &mut Context<Self>) {
        let kept = self.kept_ids();
        self.comment_views.retain(|id, _| kept.contains(id));
        Images::hold_only(kept.iter().map(String::as_str), cx);
    }

    fn apply_volume(&mut self, cx: &mut Context<Self>) {
        let prefs = Store::prefs(cx).clone();
        for player in self.players.values() {
            player.update(cx, |p, _| p.set_volume(prefs.volume as f64, prefs.muted));
        }
        cx.notify();
    }

    pub fn go(&mut self, step: isize, cx: &mut Context<Self>) {
        let last = self.items.len().saturating_sub(1);
        let to = (self.index as isize + step).clamp(0, last as isize) as usize;
        if to == self.index || self.items.is_empty() {
            self.pager.set(self.index as f32);
            cx.notify();
            return;
        }
        self.leaving = self.current().map(|a| a.aweme_id.clone());
        self.index = to;
        self.desc_open = false;
        self.pager.set(to as f32);
        self.sync_players(cx);
        cx.emit(FeedEvent::Current);
        cx.notify();
    }

    /// Leaving the page: stop sound and picture.
    pub fn pause_all(&mut self, cx: &mut Context<Self>) {
        for player in self.players.values() {
            player.update(cx, |p, cx| p.set_playing(false, cx));
        }
    }

    /// Back on the page: the current video plays again.
    pub fn resume(&mut self, cx: &mut Context<Self>) {
        if let Some(p) = self.current_player().cloned() {
            p.update(cx, |p, cx| p.set_playing(true, cx));
        }
    }

    /// Done with this list for good: free the pipelines.
    pub fn release(&mut self, cx: &mut Context<Self>) {
        for (_, player) in self.players.drain() {
            player.update(cx, |p, cx| p.shutdown(None, cx));
        }
    }

    fn current_player(&self) -> Option<&Entity<VideoPlayer>> {
        self.players.get(&self.current()?.aweme_id)
    }

    pub fn toggle_play(&mut self, cx: &mut Context<Self>) {
        if let Some(p) = self.current_player().cloned() {
            p.update(cx, |p, cx| p.toggle(cx));
        }
    }

    pub fn seek_by(&mut self, seconds: f64, cx: &mut Context<Self>) {
        if let Some(aweme) = self.current().filter(|a| a.is_photo()) {
            let count = aweme.image_post_info.images.len();
            let id = aweme.aweme_id.clone();
            let at = self.photo.entry(id).or_default();
            *at = (*at as isize + seconds.signum() as isize).clamp(0, count as isize - 1) as usize;
            cx.notify();
            return;
        }
        if let Some(p) = self.current_player().cloned() {
            p.update(cx, |p, _| p.seek_by(seconds));
        }
    }

    pub fn toggle_like(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.current().map(|a| a.aweme_id.clone()) else { return };
        let liked = Store::toggle_like(&id, cx);
        self.pop("like", &id, liked);
        cx.notify();
    }

    pub fn toggle_favourite(&mut self, cx: &mut Context<Self>) {
        let Some(aweme) = self.current().cloned() else { return };
        let saved = Store::toggle_favourite(&aweme, cx);
        self.pop("save", &aweme.aweme_id, saved);
        cx.emit(FeedEvent::Toast(
            if saved {
                "Добавлено в избранное"
            } else {
                "Убрано из избранного"
            }
            .into(),
        ));
        cx.notify();
    }

    fn pop(&mut self, what: &'static str, id: &str, on: bool) {
        let entry = self.pops.entry(what).or_insert((id.to_string(), 0));
        *entry = (id.to_string(), entry.1 + 1);
        if !on {
            self.pops.remove(what);
        }
    }

    pub fn copy_link(&mut self, cx: &mut Context<Self>) {
        let Some(aweme) = self.current() else { return };
        let url = if aweme.share_url.is_empty() {
            format!("https://www.tiktok.com/@{}/video/{}", aweme.author.handle(), aweme.aweme_id)
        } else {
            aweme.share_url.split('?').next().unwrap_or(&aweme.share_url).to_string()
        };
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(url));
        cx.emit(FeedEvent::Toast("Ссылка скопирована".into()));
    }

    pub fn download(&mut self, cx: &mut Context<Self>) {
        let Some(aweme) = self.current().cloned() else { return };
        let url =
            [&aweme.video.download_no_watermark_addr, &aweme.video.play_addr, &aweme.video.download_addr]
                .into_iter()
                .find_map(|l| l.first().map(str::to_string));
        let Some(url) = url.filter(|_| !aweme.is_photo()) else {
            cx.emit(FeedEvent::Toast("Это не видео".into()));
            return;
        };
        let transport = self.tiktok.transport().clone();
        let path = downloads_dir().join(format!("{}_{}.mp4", aweme.author.handle(), aweme.aweme_id));
        cx.emit(FeedEvent::Toast("Скачиваю…".into()));
        let job = self.io.spawn(async move {
            let resp = transport.get(&url, &[], None, false).await.map_err(|e| e.to_string())?;
            if resp.status != 200 || resp.body.is_empty() {
                return Err(format!("HTTP {}", resp.status));
            }
            std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::write(&path, resp.body).map_err(|e| e.to_string())?;
            Ok(path)
        });
        cx.spawn(async move |this, cx| {
            let result = job.await.map_err(|e| e.to_string()).and_then(|r| r);
            this.update(cx, |_, cx| {
                let message = match result {
                    Ok(path) => format!("Сохранено: {}", path.display()),
                    Err(e) => format!("Не скачалось: {e}"),
                };
                cx.emit(FeedEvent::Toast(message.into()));
            })
            .ok();
        })
        .detach();
    }

    /// Comments key: in wide windows the panel is always there; otherwise it opens
    /// as a sheet, or by widening the window, as the settings say.
    pub fn toggle_comments(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let size = window.viewport_size();
        let layout = Layout::compute(
            f32::from(size.width),
            f32::from(size.height),
            self.sidebar,
            false,
            self.mode.unwrap_or(Mode::Medium),
        );
        if self.sheet_open || self.widened_from.is_some() {
            self.close_comments_in(window, cx);
            return;
        }
        if layout.mode == Mode::Wide {
            return;
        }
        let expand = Store::prefs(cx).comments_mode == CommentsMode::Expand;
        if expand && !window.is_maximized() && !window.is_fullscreen() {
            let wide = Layout::wide_width(f32::from(size.height), self.sidebar) + 1.;
            self.widened_from = Some(size);
            window.resize(gpui::size(px(wide), size.height));
            // The compositor may refuse (tiling, maximised): then fall back to the sheet.
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(Duration::from_millis(350)).await;
                this.update_in(cx, |this, window, cx| {
                    let now = window.viewport_size();
                    let l = Layout::compute(
                        f32::from(now.width),
                        f32::from(now.height),
                        this.sidebar,
                        false,
                        Mode::Medium,
                    );
                    if this.widened_from.is_some() && l.mode != Mode::Wide {
                        this.widened_from = None;
                        this.sheet_open = true;
                        this.sheet.set(1.);
                        cx.notify();
                    }
                })
                .ok();
            })
            .detach();
        } else {
            self.sheet_open = true;
            self.sheet.set(1.);
        }
        cx.notify();
    }

    fn close_comments(&mut self, cx: &mut Context<Self>) {
        self.sheet_open = false;
        self.sheet.set(0.);
        cx.notify();
    }

    pub fn close_comments_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(size) = self.widened_from.take() {
            let now = window.viewport_size();
            window.resize(gpui::size(size.width, now.height));
        }
        self.close_comments(cx);
    }

    pub fn has_open_comments(&self) -> bool {
        self.sheet_open || self.widened_from.is_some()
    }

    fn on_wheel(&mut self, event: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event.delta {
            ScrollDelta::Lines(lines) => {
                if lines.y.abs() < 0.01 || self.wheel_at.elapsed() < WHEEL_COOLDOWN {
                    return;
                }
                self.wheel_at = Instant::now();
                self.go(if lines.y < 0. { 1 } else { -1 }, cx);
            }
            ScrollDelta::Pixels(delta) => {
                let height = self.viewport.height.max(px(1.));
                if event.touch_phase == TouchPhase::Ended {
                    self.end_swipe(cx);
                    return;
                }
                if self.swipe_done {
                    self.arm_swipe_end(window, cx);
                    return;
                }
                self.swipe += f32::from(delta.y);
                let share = self.swipe / f32::from(height);
                if share.abs() > SWIPE_COMMIT {
                    self.swipe_done = true;
                    self.swipe = 0.;
                    self.go(if share < 0. { 1 } else { -1 }, cx);
                } else {
                    // follow the fingers
                    self.pager.set(self.index as f32 - share * 0.9);
                    cx.notify();
                }
                self.arm_swipe_end(window, cx);
            }
        }
    }

    /// No touchpad events for a moment = the gesture (and its momentum) is over.
    fn arm_swipe_end(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.swipe_gen += 1;
        let generation = self.swipe_gen;
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(GESTURE_GAP).await;
            this.update(cx, |this, cx| {
                if this.swipe_gen == generation {
                    this.end_swipe(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn end_swipe(&mut self, cx: &mut Context<Self>) {
        self.swipe = 0.;
        self.swipe_done = false;
        self.pager.set(self.index as f32);
        cx.notify();
    }

    // ── rendering ────────────────────────────────────────────────────────

    fn slide(
        &self,
        i: usize,
        column: Rect,
        chrome: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = *cx.theme();
        let aweme = &self.items[i];
        let current = i == self.index;
        let cover = Images::get_for(&aweme.aweme_id, cover(aweme), COVER_PX, cx);
        let player = self.players.get(&aweme.aweme_id).cloned();
        let has_frame = player.as_ref().is_some_and(|p| p.read(cx).has_frame());

        let mut slide =
            div().absolute().left_0().w(px(column.w)).h(px(column.h)).bg(gpui::black()).overflow_hidden();
        if aweme.is_photo() {
            let images = &aweme.image_post_info.images;
            let at =
                self.photo.get(&aweme.aweme_id).copied().unwrap_or(0).min(images.len().saturating_sub(1));
            let image = Images::get_for(&aweme.aweme_id, &images[at].display_image, 1280, cx);
            slide = slide
                .when_some(image, |el, image| {
                    el.child(
                        img(image).absolute().inset_0().size_full().object_fit(ObjectFit::Contain).motion(
                            SharedString::from(format!("photo-{}-{at}", aweme.aweme_id)),
                            Motion::Base,
                            |el, t| el.opacity(t),
                        ),
                    )
                })
                .when(images.len() > 1, |el| el.child(photo_dots(images.len(), at, theme.primary)))
                .when_some(player.filter(|_| current), |el, p| el.child(div().absolute().size_0().child(p)));
        } else {
            slide = slide
                .when_some(cover.filter(|_| !has_frame), |el, cover| {
                    el.child(img(cover).absolute().inset_0().size_full().object_fit(ObjectFit::Contain))
                })
                .when_some(player, |el, p| el.child(div().absolute().inset_0().child(p)));
        }
        let _ = window;
        if chrome > 0.001 {
            slide = slide.child(
                div().absolute().inset_0().opacity(chrome).child(self.caption(aweme, current, column.h, cx)),
            );
        }
        let id = aweme.aweme_id.clone();
        slide
            .id(SharedString::from(format!("slide-{id}")))
            .on_click(cx.listener(move |this, _, _, cx| {
                if this.current().is_some_and(|a| a.aweme_id == id) {
                    this.toggle_play(cx);
                }
            }))
            .into_any_element()
    }

    /// Eases each glyph's and count's shadow toward how light it is right under it: the
    /// picture's cells there while the buttons cover the video (`over`), the ambient light's
    /// field beside it. Bounds come from the last paint; a few cells per element.
    fn shade_buttons(
        &mut self,
        picture: Option<(Rect, std::sync::Arc<Glimpse>)>,
        glow: Option<Rect>,
        origin: (f32, f32),
        over: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // shares of `area` (feed coordinates) covered by window-space bounds
        let share = |b: Bounds<Pixels>, area: Rect| {
            let x = (f32::from(b.origin.x) - origin.0 - area.x) / area.w;
            let y = (f32::from(b.origin.y) - origin.1 - area.y) / area.h;
            (x, y, x + f32::from(b.size.width) / area.w, y + f32::from(b.size.height) / area.h)
        };
        let light = |b: Bounds<Pixels>| {
            let on_video = picture.as_ref().map_or(0., |(at, frame)| {
                let (u0, v0, u1, v1) = share(b, *at);
                frame.light_in(u0, v0, u1, v1)
            });
            let on_glow = glow.map_or(0., |at| {
                let (u0, v0, u1, v1) = share(b, at);
                self.ambilight.light_in(u0, v0, u1, v1)
            });
            over * on_video + (1. - over) * on_glow
        };
        let mut targets: Vec<((&'static str, bool), f32)> = Vec::new();
        for (label, bounds) in [(false, &self.lens.faces), (true, &self.lens.labels)] {
            for (&key, &b) in bounds.borrow().iter() {
                // a shadow from fairly light up, in full by a pale backdrop
                targets.push((
                    (key, label),
                    ((light(b) - SHADE_FROM) / (SHADE_FULL - SHADE_FROM)).clamp(0., 1.),
                ));
            }
        }
        for (key, target) in targets {
            let spring = self.shades.entry(key).or_insert_with(|| Spring::new(Springs::PANEL, target));
            spring.set(target);
            spring.tick(window, cx);
        }
    }

    /// The button stack; `beside` 1 = beside the video (themed faces), 0 = on it (white on dark glass).
    fn actions(&self, aweme: &Aweme, beside: f32, current: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = *cx.theme();
        let id = aweme.aweme_id.clone();
        let liked = Store::is_liked(&id, cx);
        let saved = Store::is_favourite(&id, cx);
        let avatar = Images::get_for(&aweme.aweme_id, &aweme.author.avatar_thumb, 96, cx);
        let music_cover = Images::get_for(&aweme.aweme_id, aweme.music.cover(), 96, cx);
        let s = &aweme.statistics;
        let profile = format!("https://www.tiktok.com/@{}", aweme.author.handle());
        let music = if aweme.music.music_id().is_empty() {
            None
        } else {
            Some(format!("https://www.tiktok.com/music/x-{}", aweme.music.music_id()))
        };
        let fg = mix(gpui::white(), theme.foreground, beside);
        let over = 1. - beside;
        let prefs = Store::prefs(cx).clone();
        let (liquid, clarity) = (prefs.liquid_glass, prefs.button_clarity.clamp(0., 1.));
        // How light it is right under each glyph and each count (see `FeedView::shade_buttons`):
        // glyphs, counts and faces all answer to it, each on its own.
        let shade_of =
            |key: &'static str, label: bool| self.shades.get(&(key, label)).map_or(0., |s| s.value());
        let face_bg = |shade: f32| {
            mix(
                gpui::black().opacity(0.32 * (1. - clarity) + 0.22 * shade),
                theme.secondary.opacity(1. - 0.55 * clarity + 0.3 * shade),
                beside,
            )
        };
        let face_hover = mix(gpui::white().opacity(0.14), theme.secondary_active, beside);
        // the counts are white wherever the buttons sit, like their glyphs
        let label = mix(gpui::white(), theme.foreground, beside);
        // the lens draws its own glint; beside the video a hairline keeps the face on flat paint
        let rim = theme.border.opacity(theme.border.a * beside);
        // the counts: a light shadow always (the picture or the light it throws is under
        // them), a deeper one when it is light
        let shadows = |shade: f32| {
            [
                gpui::TextShadow {
                    color: gpui::black().opacity((0.5 + 0.1 * over + 0.4 * shade).min(0.9)),
                    offset: gpui::point(px(0.), px(1.)),
                    blur: px(2.),
                },
                gpui::TextShadow {
                    color: gpui::black().opacity(0.6 * shade),
                    offset: gpui::point(px(0.), px(1.)),
                    blur: px(6.),
                },
            ]
        };
        let labels = self.lens.labels.clone();
        let pop = |what: &str| self.pops.get(what).filter(|(pid, _)| *pid == id).map(|(_, n)| *n);
        let faces = self.lens.faces.clone();
        let drop = self.lens.now;
        let origin_at = self.lens.origin.get();
        // A face lights up as the drop arrives under it, not on its own hover: by how close
        // the drop is and how full it has grown.
        let glow = |key: &'static str| {
            let Some(face) = self.lens.faces.borrow().get(key).copied() else { return 0. };
            let at = f32::from(face.center().y - origin_at.y);
            (1. - (drop.y - at).abs() / (FACE * 0.8)).clamp(0., 1.) * drop.size.clamp(0., 1.)
        };

        let button = |key: &'static str,
                      path: &'static str,
                      count: String,
                      color: Hsla,
                      popped: Option<usize>,
                      cx: &mut Context<Self>| {
            let glyph = icon(path).size(px(24.)).text_color(color);
            let lit = if current { glow(key) } else { 0. };
            let (shade, label_shade) = (shade_of(key, false), shade_of(key, true));
            let glass = div()
                .absolute()
                .inset_0()
                .rounded_full()
                .overflow_hidden()
                .flex()
                .items_center()
                .justify_center()
                .bg(mix(face_bg(shade), face_hover, lit))
                .backdrop_blur(if liquid { FACE_BLUR } else { FROST_BLUR })
                .when(liquid, |el| el.backdrop_glass(FACE_GLASS))
                .border_1()
                .border_color(rim)
                // a soft shadow under the glyph for white on white: a dark copy, blurred, with a
                // tighter one for the shape and a wide one for the haze
                .when(shade > 0.01, |el| {
                    let copy = |blur: f32, down: f32, alpha: f32| {
                        div()
                            .absolute()
                            .inset_0()
                            .mt(px(down))
                            .flex()
                            .items_center()
                            .justify_center()
                            .blur(px(blur))
                            .child(icon(path).size(px(24.)).text_color(gpui::black().opacity(alpha * shade)))
                    };
                    el.child(copy(3.5, 1., 0.6)).child(copy(1.5, 1., 0.5))
                })
                .child(match popped {
                    Some(n) => glyph
                        .with_animation(
                            SharedString::from(format!("pop-{key}-{n}")),
                            gpui::Animation::new(Duration::from_millis(420)),
                            |el, t| {
                                // a quick overshoot: 1 → 1.35 → 1
                                let s = 1. + 0.35 * (t * std::f32::consts::PI).sin() * (1. - t);
                                el.with_transformation(gpui::Transformation::scale(gpui::size(s, s)))
                            },
                        )
                        .into_any_element(),
                    None => glyph.into_any_element(),
                });
            let pressed = self.presses.get(key).filter(|(pid, _)| *pid == id).map(|(_, n)| *n);
            let glass = match pressed {
                // squeezed in, then a damped bounce back past its size
                Some(n) => glass
                    .with_animation(
                        SharedString::from(format!("squish-{key}-{n}")),
                        gpui::Animation::new(SQUISH),
                        |el, t| {
                            let s = 1. - 0.16 * (-5. * t).exp() * (3. * std::f32::consts::PI * t).cos();
                            let d = px((1. - s) * FACE / 2.);
                            el.top(d).left(d).right(d).bottom(d)
                        },
                    )
                    .into_any_element(),
                _ => glass.into_any_element(),
            };
            let faces = faces.clone();
            div()
                .id(key)
                // the gap between buttons is half each one's padding, not space between them:
                // the hover areas meet, so the drop flows from one to the next without letting go
                .py_2()
                .w(px(FACE + 16.))
                .flex()
                .flex_col()
                .items_center()
                .gap_1()
                .cursor_pointer()
                .child(
                    div()
                        .relative()
                        .size(px(FACE))
                        .when(current, |el| {
                            el.child(
                                canvas(
                                    move |bounds, _, _| {
                                        faces.borrow_mut().insert(key, bounds);
                                    },
                                    |_, _, _, _| {},
                                )
                                .absolute()
                                .inset_0(),
                            )
                        })
                        .child(glass),
                )
                .child(
                    div()
                        .text_size(theme.text(Text::Tiny))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(label)
                        .text_shadow(shadows(label_shade))
                        .when(current, |el| {
                            let labels = labels.clone();
                            el.relative().child(
                                canvas(
                                    move |bounds, _, _| {
                                        labels.borrow_mut().insert(key, bounds);
                                    },
                                    |_, _, _, _| {},
                                )
                                .absolute()
                                .inset_0(),
                            )
                        })
                        .child(count),
                )
                .when(current, |el| {
                    el.on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.lens.hovered = Some(key);
                        } else if this.lens.hovered == Some(key) {
                            this.lens.hovered = None;
                        }
                        cx.notify();
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            let Some(id) = this.current().map(|a| a.aweme_id.clone()) else { return };
                            let entry = this.presses.entry(key).or_insert((id.clone(), 0));
                            *entry = (id, entry.1 + 1);
                            this.lens.press();
                            cx.notify();
                        }),
                    )
                })
        };

        // The drop: wider than a face, its size and shape on springs (see `Lens`). Painted by
        // hand at the springs' exact values, off the pixel grid: laid out, it would walk in
        // whole pixels and tick into place as the springs let go.
        let lens = (current && drop.size > 0.01).then(|| {
            let fill = 0.04 + 0.14 * (1. - clarity);
            let color = mix(gpui::white().opacity(fill), theme.foreground.opacity(fill * 0.6), beside);
            let (blur, glass) = if liquid { (px(3.), Some(LENS_GLASS)) } else { (px(12.), None) };
            canvas(
                |_, _, _| {},
                move |stack, _, window, _| {
                    let base = (FACE + LENS_GROW) * drop.size;
                    let size = gpui::size(px(base * drop.wide), px(base * drop.tall));
                    let centre = stack.origin + gpui::point(px(drop.x), px(drop.y));
                    let bounds = Bounds::new(centre - gpui::point(size.width / 2., size.height / 2.), size);
                    let radius = gpui::Corners::all(size.width.min(size.height) / 2.);
                    window.with_subpixel_paint(|window| {
                        // the whole drop fades as it shrinks away, its lens included
                        window.with_opacity(drop.size.min(1.), |window| {
                            window.paint_glass_backdrop(bounds, radius, blur, glass);
                            window.paint_quad(gpui::fill(bounds, color).corner_radii(radius));
                        })
                    });
                },
            )
            .absolute()
            .inset_0()
        });
        let origin = self.lens.origin.clone();

        let accent = theme.primary;
        div()
            .id(SharedString::from(format!("buttons-{id}")))
            // over the video, a click is for the button, not the play toggle under it;
            // the wheel still pages
            .block_mouse_except_scroll()
            .relative()
            .flex()
            .flex_col()
            .items_center()
            .when(current, |el| {
                el.child(
                    canvas(move |bounds, _, _| origin.set(bounds.origin), |_, _, _, _| {})
                        .absolute()
                        .inset_0(),
                )
            })
            .children(lens)
            .child({
                let faces = self.lens.faces.clone();
                div()
                    .id("author")
                    // like the buttons: its padding meets theirs, so the drop flows on down
                    .pb_4()
                    .w(px(FACE + 16.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .cursor_pointer()
                    .child(
                        div()
                            .relative()
                            .child(div().rounded_full().border_2().border_color(fg).child(avatar_el(
                                avatar,
                                px(46.),
                                &aweme.author.nickname,
                                theme.secondary,
                                theme.muted_foreground,
                            )))
                            .child(
                                div()
                                    .absolute()
                                    .bottom(px(-8.))
                                    .left(px(15.))
                                    .size(px(20.))
                                    .rounded_full()
                                    .bg(accent)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        icon("icons/plus.svg")
                                            .size(px(13.))
                                            .text_color(theme.primary_foreground),
                                    ),
                            )
                            .when(current, |el| {
                                el.child(
                                    canvas(
                                        move |bounds, _, _| {
                                            faces.borrow_mut().insert("author", bounds);
                                        },
                                        |_, _, _, _| {},
                                    )
                                    .absolute()
                                    .inset_0(),
                                )
                            }),
                    )
                    .when(current, |el| {
                        el.on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                            if *hovered {
                                this.lens.hovered = Some("author");
                            } else if this.lens.hovered == Some("author") {
                                this.lens.hovered = None;
                            }
                            cx.notify();
                        }))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.lens.press();
                                cx.notify();
                            }),
                        )
                    })
                    .on_click(move |_, _, cx| cx.open_url(&profile))
            })
            .child(
                button(
                    "like",
                    if liked { "icons/heart-filled.svg" } else { "icons/heart.svg" },
                    compact(s.digg_count + u64::from(liked)),
                    if liked { accent } else { fg },
                    pop("like"),
                    cx,
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_like(cx))),
            )
            .when(self.wide_t < 0.999, |el| {
                // the panel shows the comments in wide mode: the button folds away
                let fold = 1. - self.wide_t;
                // its height as last painted (face top to count bottom, plus its padding)
                let row = match (
                    self.lens.faces.borrow().get("comments"),
                    self.lens.labels.borrow().get("comments"),
                ) {
                    (Some(face), Some(label)) => f32::from(label.bottom() - face.top()) + 16.,
                    _ => COMMENTS_ROW,
                };
                el.child(
                    div().when(fold < 0.999, |d| d.h(px(row * fold)).overflow_hidden()).opacity(fold).child(
                        button(
                            "comments",
                            "icons/message-circle.svg",
                            compact(s.comment_count),
                            fg,
                            None,
                            cx,
                        )
                        .on_click(cx.listener(|this, _, window, cx| this.toggle_comments(window, cx))),
                    ),
                )
            })
            .child(
                button(
                    "save",
                    if saved { "icons/bookmark-filled.svg" } else { "icons/bookmark.svg" },
                    compact(s.collect_count + u64::from(saved)),
                    if saved { accent } else { fg },
                    pop("save"),
                    cx,
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_favourite(cx))),
            )
            .child(
                button("share", "icons/forward.svg", compact(s.share_count), fg, None, cx)
                    .on_click(cx.listener(|this, _, _, cx| this.copy_link(cx))),
            )
            .child({
                // the sound takes the drop like the buttons; its padding meets theirs
                let faces = self.lens.faces.clone();
                div()
                    .id("sound-row")
                    .pt_2()
                    .w(px(FACE + 16.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(
                        div()
                            .relative()
                            .child(sound_disc(
                                music_cover,
                                music,
                                current && self.current_player().is_some_and(|p| p.read(cx).is_playing()),
                                theme.secondary,
                            ))
                            .when(current, |el| {
                                el.child(
                                    canvas(
                                        move |bounds, _, _| {
                                            faces.borrow_mut().insert("sound", bounds);
                                        },
                                        |_, _, _, _| {},
                                    )
                                    .absolute()
                                    .inset_0(),
                                )
                            }),
                    )
                    .when(current, |el| {
                        el.on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                            if *hovered {
                                this.lens.hovered = Some("sound");
                            } else if this.lens.hovered == Some("sound") {
                                this.lens.hovered = None;
                            }
                            cx.notify();
                        }))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.lens.press();
                                cx.notify();
                            }),
                        )
                    })
            })
            .into_any_element()
    }
}

impl Render for FeedView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let viewport = window.viewport_size();
        let sheet_t = self.sheet.tick(window, cx).clamp(0., 1.);
        let (w, h) = (f32::from(viewport.width), f32::from(viewport.height));
        // A change of mode is one animated move: every arrangement follows the
        // window live, the springs only blend between them.
        let mode = Layout::mode(w, h, self.sidebar, self.mode.unwrap_or(Mode::Medium));
        let (wide_to, beside_to) =
            (if mode == Mode::Wide { 1. } else { 0. }, if mode == Mode::Overlay { 0. } else { 1. });
        if self.mode.is_none() {
            self.wide_anim.snap(wide_to);
            self.beside_anim.snap(beside_to);
        }
        self.mode = Some(mode);
        self.wide_anim.set(wide_to);
        self.beside_anim.set(beside_to);
        let wt = self.wide_anim.tick(window, cx).clamp(0., 1.);
        self.wide_t = wt;
        if wt > 0.999 {
            // folded away: nothing to hover, nowhere for the drop to go
            if self.lens.hovered == Some("comments") {
                self.lens.hovered = None;
            }
        }
        let bt = self.beside_anim.tick(window, cx).clamp(0., 1.);
        let sidebar = self.sidebar;
        let arrange = |sheet, mode| Layout::arrange(w, h, sidebar, sheet, mode);
        let roomy = arrange(false, Mode::Wide);
        // Always blended: on a switch's first frame the springs have barely moved
        // (0.9999), and cutting to the target layout there flashes it for a frame.
        let mut closed = arrange(false, mode);
        let narrow = blend(arrange(false, Mode::Overlay), arrange(false, Mode::Medium), bt);
        let blended = blend(narrow, roomy, wt);
        (closed.video, closed.actions, closed.beside) = (blended.video, blended.actions, blended.beside);
        // the panel slides in from (and out to) the right edge
        let right_edge = roomy.content.x + roomy.content.w;
        closed.comments_panel = roomy.comments_panel.filter(|_| wt > 0.001).map(|mut p| {
            p.x += (1. - wt) * (right_edge - p.x);
            p
        });
        let open = arrange(true, if mode == Mode::Wide { Mode::Medium } else { mode });
        self.clear_buttons = closed.beside < 0.999;
        if closed.mode == Mode::Wide && self.sheet_open {
            self.sheet_open = false;
            self.sheet.snap(0.);
        }
        if closed.mode == Mode::Wide || self.sheet_open {
            self.ensure_comments(cx);
        }
        let origin = (closed.content.x, TITLEBAR);
        let column = lerp_rect(closed.video, open.video, sheet_t);
        let column = Rect::new(column.x - origin.0, column.y - origin.1, column.w, column.h);
        self.viewport = size(px(column.w), px(column.h));
        self.scale = window.scale_factor();
        let pos = self.pager.tick(window, cx);
        if self.leaving.is_some() && (pos - self.index as f32).abs() < 0.002 {
            // the old clip has slid fully out of view
            self.leaving = None;
            self.sync_players(cx);
        }

        if self.items.is_empty() {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(self.empty_state(cx))
                .into_any_element();
        }

        // slides around the position: the current one, its neighbours, whatever the pager crosses
        let lo = (pos.floor() as isize - 1).max(0) as usize;
        let hi = ((pos.ceil() as usize) + 1).min(self.items.len() - 1);
        // a slide's caption and buttons ride with it; only the ones in view are drawn
        let in_view = |i: usize| (i as f32 - pos).abs() < 1.;
        // the caption fades out under the panel and the sheet rather than vanishing
        let chrome = (1. - wt) * (1. - sheet_t * 2.).clamp(0., 1.);
        let slides: Vec<AnyElement> = (lo..=hi)
            .map(|i| {
                let top = (i as f32 - pos) * column.h;
                let chrome = if in_view(i) { chrome } else { 0. };
                div()
                    .absolute()
                    .left_0()
                    .top(px(top))
                    .w(px(column.w))
                    .h(px(column.h))
                    .child(self.slide(i, column, chrome, window, cx))
                    .into_any_element()
            })
            .collect();

        let mut stage = div()
            .id("stage")
            .absolute()
            .left(px(column.x))
            .top(px(column.y))
            .w(px(column.w))
            .h(px(column.h))
            .overflow_hidden()
            .bg(gpui::black())
            .children(slides)
            .on_scroll_wheel(
                cx.listener(|this, e: &ScrollWheelEvent, window, cx| this.on_wheel(e, window, cx)),
            );
        if self.loading && self.index + 1 >= self.items.len() {
            stage = stage.child(
                div().absolute().bottom(px(18.)).left_0().right_0().flex().justify_center().child(spinner(
                    "feed-spin",
                    px(18.),
                    gpui::white(),
                )),
            );
        }

        // the video's own colours thrown around it, behind it
        let prefs = Store::prefs(cx).clone();
        let frame = self.current_player().and_then(|p| p.read(cx).glimpse());
        // the picture as the column shows it (fitted, bars and all), the glow around that
        let fitted = |aspect: f32| {
            let (w, h) = if aspect > column.w / column.h {
                (column.w, column.w / aspect)
            } else {
                (column.h * aspect, column.h)
            };
            Rect::new(column.x + (column.w - w) / 2., column.y + (column.h - h) / 2., w, h)
        };
        // beside the comments panel the glow fades out where the comments begin
        let room = closed.comments_panel.and_then(|panel| {
            let aspect = frame.as_ref().map(|f| f.w as f32 / f.h as f32).or(self.ambilight.aspect())?;
            let pic = fitted(aspect);
            Some((panel.x - origin.0 + GLOW_INTO_PANEL - (pic.x + pic.w)) / pic.h)
        });
        let thrown = self.ambilight.update(
            frame.clone().filter(|_| prefs.ambilight),
            prefs.ambilight_reach(),
            prefs.ambilight_strength,
            room,
            window,
        );
        let glow_at = thrown.as_ref().map(|(_, reach)| {
            let pic = fitted(reach.aspect);
            Rect::new(
                pic.x + reach.left * pic.w,
                pic.y + reach.top * pic.h,
                reach.wide * pic.w,
                reach.tall * pic.h,
            )
        });
        let glow = thrown.zip(glow_at).map(|((image, _), at)| {
            // kept to the feed: thrown into the titlebar it only muddied it
            div().absolute().inset_0().overflow_hidden().child(
                img(image)
                    .absolute()
                    .left(px(at.x))
                    .top(px(at.y))
                    .w(px(at.w))
                    .h(px(at.h))
                    .object_fit(ObjectFit::Fill)
                    // the grid is coarse; a blur about a cell wide hides its steps
                    .blur(px(at.h / 64.)),
            )
        });
        // under the glow: it dims the backdrop, not the video's light
        let veil = closed.comments_panel.map(|panel| {
            // a linear ramp shows an edge at both ends; smootherstep sets off and lands softly
            let dark = |t: f32| {
                let t = t.clamp(0., 1.);
                let eased = t * t * t * (t * (t * 6. - 15.) + 10.);
                theme.background.opacity(eased * VEIL_DIM * wt)
            };
            // whole pixels, so the pieces meet without a seam
            let ramp = |from: f32, span: f32, rising: bool| {
                let step = (span / VEIL_STEPS as f32).round().max(1.);
                (0..VEIL_STEPS).map(move |i| {
                    let (t0, t1) = (i as f32 / VEIL_STEPS as f32, (i + 1) as f32 / VEIL_STEPS as f32);
                    let (a, b) = if rising { (t0, t1) } else { (1. - t0, 1. - t1) };
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(px(from + i as f32 * step))
                        .w(px(step))
                        .bg(linear_gradient(90., linear_color_stop(dark(a), 0.), linear_color_stop(dark(b), 1.)))
                })
            };
            let from = (panel.x - origin.0 - VEIL_LEAD).round();
            let rise = ((VEIL_LEAD + panel.w * VEIL_REACH) / VEIL_STEPS as f32).round().max(1.) * VEIL_STEPS as f32;
            let end = (panel.x - origin.0 + panel.w).round();
            let tail = (VEIL_TAIL / VEIL_STEPS as f32).round() * VEIL_STEPS as f32;
            div()
                .absolute()
                .inset_0()
                .children(ramp(from, rise, true))
                .child(div().absolute().top_0().bottom_0().left(px(from + rise)).w(px((end - from - rise).max(0.))).bg(dark(1.)))
                .children(ramp(end, tail, false))
        });
        let mut root = div().size_full().relative().children(veil).children(glow).child(stage);

        self.lens.tick(window, cx);
        let picture = frame.map(|f| (fitted(f.w as f32 / f.h as f32), f));
        self.shade_buttons(picture, glow_at, origin, 1. - closed.beside, window, cx);
        if let Some(actions) = closed.actions.filter(|_| sheet_t < 0.99) {
            let stacks: Vec<AnyElement> = (lo..=hi)
                .filter(|&i| in_view(i))
                .map(|i| {
                    let aweme = &self.items[i];
                    div()
                        .id(SharedString::from(format!("acts-{}", aweme.aweme_id)))
                        .absolute()
                        .left_0()
                        .top(px((i as f32 - pos) * column.h))
                        .size_full()
                        .flex()
                        .flex_col()
                        .justify_end()
                        .items_center()
                        .pb(px(closed.actions_lift))
                        .child(self.actions(aweme, closed.beside, i == self.index, cx))
                        .into_any_element()
                })
                .collect();
            root = root.child(
                div()
                    .id("actions-column")
                    .absolute()
                    .left(px(actions.x - origin.0))
                    .top(px(actions.y - origin.1))
                    .w(px(actions.w))
                    .h(px(actions.h))
                    .overflow_hidden()
                    .opacity(1. - sheet_t)
                    .children(stacks)
                    // the whole column turns the page too, not just the video; once (over the
                    // video the stage under it would take the same wheel again)
                    .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, window, cx| {
                        cx.stop_propagation();
                        this.on_wheel(e, window, cx);
                    })),
            );
        }

        if let Some(panel) = closed.comments_panel {
            // the panel stays while the next video's comments are on their way
            let body = match &self.comments {
                Some(comments) => {
                    comments.update(cx, |c, cx| c.set_variant(Variant::Panel, cx));
                    div()
                        .size_full()
                        .child(AnyView::from(comments.clone()).cached(StyleRefinement::default().size_full()))
                        .into_any_element()
                }
                None => div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(spinner("panel-wait", px(18.), theme.muted_foreground))
                    .into_any_element(),
            };
            root = root.child(
                div()
                    .absolute()
                    .left(px(panel.x - origin.0))
                    .top(px(panel.y - origin.1))
                    .w(px(panel.w))
                    .h(px(panel.h))
                    .child(body),
            );
        }

        if sheet_t > 0.001
            && let (Some(sheet), Some(comments)) = (open.comments_sheet, &self.comments)
        {
            comments.update(cx, |c, cx| c.set_variant(Variant::Sheet, cx));
            let slide = (1. - sheet_t) * sheet.h;
            root = root.child(
                div()
                    .absolute()
                    .left(px(sheet.x - origin.0))
                    .top(px(sheet.y - origin.1 + slide))
                    .w(px(sheet.w))
                    .h(px(sheet.h))
                    .bg(theme.popover)
                    .rounded_t(px(14.))
                    .border_t_1()
                    .border_color(theme.border)
                    .shadow_lg()
                    .occlude()
                    .child(AnyView::from(comments.clone()).cached(StyleRefinement::default().size_full())),
            );
        }
        root.into_any_element()
    }
}

impl FeedView {
    /// Author, description and sound over the bottom of the video.
    fn caption(&self, aweme: &Aweme, current: bool, height: f32, cx: &mut Context<Self>) -> AnyElement {
        let theme = *cx.theme();
        let white = gpui::white();
        let long = is_long(&aweme.desc);
        let open = self.desc_open && long && current;
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .pt_16()
            .pb_5()
            .pl_4()
            // room for the buttons while they sit on the video
            .pr(px(if self.clear_buttons { 80. } else { 16. }))
            .flex()
            .flex_col()
            .gap_1p5()
            .bg(linear_gradient(
                180.,
                linear_color_stop(gpui::black().opacity(0.), 0.),
                linear_color_stop(
                    gpui::black().opacity(if open { 0.85 } else { 0.7 }),
                    if open { 0.25 } else { 1. },
                ),
            ))
            .text_color(white)
            .child(div().font_weight(FontWeight::SEMIBOLD).child(format!("@{}", aweme.author.handle())))
            .when(!aweme.desc.is_empty(), |el| {
                el.child(
                    div()
                        .id("caption-desc")
                        .text_size(theme.text(Text::Label))
                        .when(!open, |d| d.line_clamp(3))
                        .when(open, |d| {
                            // read it all without turning the page
                            d.max_h(px(height * 0.5))
                                .overflow_y_scroll()
                                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                        })
                        .child(rich(&aweme.desc, white, white.opacity(0.85))),
                )
                .when(long, |el| {
                    el.child(more_toggle("caption-more", open, white.opacity(0.8), white).on_click(
                        cx.listener(|this, _, _, cx| {
                            cx.stop_propagation();
                            this.desc_open = !this.desc_open;
                            cx.notify();
                        }),
                    ))
                })
            })
            .when(!aweme.music.title.is_empty(), |el| {
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .text_size(theme.text(Text::Small))
                        .text_color(white.opacity(0.85))
                        .child(icon("icons/music-2.svg").size(px(13.)).text_color(white.opacity(0.85)))
                        .child(
                            div().truncate().child(format!("{} — {}", aweme.music.title, aweme.music.author)),
                        ),
                )
            })
            .into_any_element()
    }

    fn empty_state(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = *cx.theme();
        match &self.error {
            Some(err) => div()
                .flex()
                .flex_col()
                .items_center()
                .gap_3()
                .max_w(px(420.))
                .child(icon("icons/circle-alert.svg").size(px(28.)).text_color(theme.muted_foreground))
                .child(div().text_center().text_color(theme.muted_foreground).child(err.clone()))
                .child(Button::new("retry-feed").secondary().label("Повторить").on_click(cx.listener(
                    |this, _, _, cx| {
                        this.error = None;
                        this.load_more(cx);
                    },
                )))
                .rising("feed-error")
                .into_any_element(),
            None if matches!(self.source, Source::List) => {
                div().text_color(theme.muted_foreground).child("Здесь пока пусто").into_any_element()
            }
            None => div()
                .flex()
                .flex_col()
                .items_center()
                .gap_3()
                .text_color(theme.muted_foreground)
                .child(spinner("feed-first", px(22.), theme.muted_foreground))
                .child("Загружаю ленту…")
                .into_any_element(),
        }
    }
}

fn photo_dots(count: usize, at: usize, accent: Hsla) -> impl IntoElement {
    div().absolute().top(px(12.)).left_0().right_0().flex().justify_center().gap_1().children(
        (0..count.min(20)).map(move |i| {
            div().h(px(4.)).w(px(if i == at { 18. } else { 6. })).rounded_full().bg(if i == at {
                accent
            } else {
                gpui::white().opacity(0.5)
            })
        }),
    )
}

/// The spinning record: the sound's cover in a groove that turns while the video plays.
fn sound_disc(
    cover: Option<std::sync::Arc<gpui::RenderImage>>,
    link: Option<String>,
    spinning: bool,
    ring: Hsla,
) -> impl IntoElement {
    let white = gpui::white();
    let face = match cover {
        Some(cover) => img(cover).size(px(26.)).rounded_full().into_any_element(),
        None => icon("icons/music-2.svg").size(px(18.)).text_color(white).into_any_element(),
    };
    let groove = icon("icons/disc-3.svg").size(px(44.)).text_color(white.opacity(0.2));
    let groove = if spinning {
        groove
            .with_animation("disc-spin", gpui::Animation::new(Duration::from_secs(5)).repeat(), |el, t| {
                el.with_transformation(gpui::Transformation::rotate(gpui::percentage(t)))
            })
            .into_any_element()
    } else {
        groove.into_any_element()
    };
    div()
        .id("sound")
        .relative()
        .size(px(48.))
        .rounded_full()
        .bg(gpui::black())
        .border_2()
        .border_color(ring)
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .child(div().absolute().inset_0().flex().items_center().justify_center().child(groove))
        .child(face)
        .on_click(move |_, _, cx| {
            if let Some(link) = &link {
                cx.open_url(link);
            }
        })
}

pub fn cover(aweme: &Aweme) -> &tokers::models::UrlList {
    if aweme.is_photo() {
        &aweme.image_post_info.images[0].display_image
    } else if aweme.video.cover.url_list.is_empty() {
        &aweme.video.origin_cover
    } else {
        &aweme.video.cover
    }
}

/// Videos with a stream, photo posts with images.
fn playable(aweme: &Aweme) -> bool {
    aweme.is_photo() || aweme.video.play_addr.first().is_some()
}

/// The video, buttons and button style of `a` moved `t` of the way to `b`.
fn blend(a: Layout, b: Layout, t: f32) -> Layout {
    Layout {
        video: lerp_rect(a.video, b.video, t),
        actions: a.actions.zip(b.actions).map(|(x, y)| lerp_rect(x, y, t)),
        beside: a.beside + (b.beside - a.beside) * t,
        ..a
    }
}

fn lerp_rect(a: Rect, b: Rect, t: f32) -> Rect {
    let l = |x: f32, y: f32| x + (y - x) * t;
    Rect::new(l(a.x, b.x), l(a.y, b.y), l(a.w, b.w), l(a.h, b.h))
}
