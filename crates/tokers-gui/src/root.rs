//! The window: title bar, sidebar (or its drawer), the page, toasts, and the
//! theme that follows the cover on screen.

use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{
    AnyElement, AnyView, App, Context, Entity, FocusHandle, FontWeight, Hsla, MouseButton, MouseDownEvent,
    MouseMoveEvent, ObjectFit, Render, ScrollDelta, ScrollHandle, ScrollWheelEvent, SharedString,
    StyleRefinement, Window, div, img, px,
};
use tokers::TikTok;

use crate::ambient::Ambient;
use crate::feed::{FeedEvent, FeedView};
use crate::layout::{Layout, SIDEBAR, TITLEBAR};
use crate::media::Images;
use crate::motion::{Motion, Motioned as _, Rising as _, Spring, Springs, Veiling as _, ease_out_cubic};
use crate::scrollbar::Scrollbar;
use crate::state::{CommentsMode, Store, downloads_dir};
use crate::theme::{ActiveTheme as _, Palette, Text, Theme};
use crate::ui::{Button, icon, slider, window_controls, window_frame, window_radius};
use crate::*;

const ROW: f32 = 36.;
const ROW_GAP: f32 = 4.;
const NAV_PAD: f32 = 10.;
const TOAST_FOR: Duration = Duration::from_millis(2200);
const THEME_FADE: Duration = Duration::from_millis(450);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    ForYou,
    Following,
    Search,
    Favourites,
    Downloads,
    Profile,
    Settings,
    /// A favourite opened from the grid.
    Watching,
}

const NAV: [(Page, &str, &str); 6] = [
    (Page::ForYou, "icons/house.svg", "Для вас"),
    (Page::Following, "icons/users.svg", "Подписки"),
    (Page::Search, "icons/search.svg", "Поиск"),
    (Page::Favourites, "icons/bookmark.svg", "Избранное"),
    (Page::Downloads, "icons/download.svg", "Загрузки"),
    (Page::Profile, "icons/user-round.svg", "Профиль"),
];

pub struct Root {
    focus: FocusHandle,
    tiktok: TikTok,
    io: tokio::runtime::Handle,
    feed: Entity<FeedView>,
    watching: Option<Entity<FeedView>>,
    page: Page,
    page_turns: usize,
    drawer: Spring,
    drawer_open: bool,
    highlight: Spring,
    toast: Option<(SharedString, usize)>,
    toasts: usize,
    tint: Palette,
    grabbed: bool,
    ambient: Entity<Ambient>,
    /// The ambient field is behind this frame: chrome goes translucent over it.
    ambient_on: bool,
    favourites_scroll: ScrollHandle,
    favourites_bar: Entity<Scrollbar>,
    settings_scroll: ScrollHandle,
    settings_bar: Entity<Scrollbar>,
}

impl Root {
    pub fn new(
        tiktok: TikTok,
        io: tokio::runtime::Handle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let feed = cx.new(|cx| FeedView::for_you(tiktok.clone(), io.clone(), cx));
        cx.subscribe(&feed, Self::on_feed_event).detach();
        cx.observe(&Store::entity(cx), |_, _, cx| cx.notify()).detach();
        cx.observe(&Images::entity(cx), |_, _, cx| cx.notify()).detach();
        let focus = cx.focus_handle();
        let (favourites_scroll, settings_scroll) = (ScrollHandle::new(), ScrollHandle::new());
        let favourites_bar = Scrollbar::area(&favourites_scroll, cx.entity_id(), cx);
        let settings_bar = Scrollbar::area(&settings_scroll, cx.entity_id(), cx);
        focus.focus(window, cx);
        Root {
            focus,
            tiktok,
            io,
            feed,
            watching: None,
            page: Page::ForYou,
            page_turns: 0,
            drawer: Spring::new(Springs::PANEL, 0.),
            drawer_open: false,
            highlight: Spring::new(Springs::RESPONSIVE, 0.),
            toast: None,
            toasts: 0,
            tint: Palette::default(),
            grabbed: false,
            ambient: cx.new(Ambient::new),
            ambient_on: false,
            favourites_scroll,
            favourites_bar,
            settings_scroll,
            settings_bar,
        }
    }

    fn on_feed_event(&mut self, _: Entity<FeedView>, event: &FeedEvent, cx: &mut Context<Self>) {
        match event {
            FeedEvent::Toast(text) => self.show_toast(text.clone(), cx),
            FeedEvent::Current => cx.notify(),
        }
    }

    fn show_toast(&mut self, text: SharedString, cx: &mut Context<Self>) {
        self.toasts += 1;
        let n = self.toasts;
        self.toast = Some((text, n));
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TOAST_FOR).await;
            this.update(cx, |this, cx| {
                if this.toast.as_ref().is_some_and(|(_, id)| *id == n) {
                    this.toast = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn active_feed(&self) -> Option<&Entity<FeedView>> {
        match self.page {
            Page::ForYou => Some(&self.feed),
            Page::Watching => self.watching.as_ref(),
            _ => None,
        }
    }

    fn navigate(&mut self, page: Page, cx: &mut Context<Self>) {
        if page == self.page {
            return;
        }
        // only the page on screen plays
        let playing = |p: Page| matches!(p, Page::ForYou | Page::Watching);
        if playing(self.page)
            && let Some(feed) = self.active_feed().cloned()
        {
            feed.update(cx, |f, cx| f.pause_all(cx));
        }
        if self.page == Page::Watching
            && page != Page::Watching
            && let Some(w) = self.watching.take()
        {
            w.update(cx, |w, cx| w.release(cx));
        }
        self.page = page;
        self.page_turns += 1;
        self.drawer_open = false;
        self.drawer.set(0.);
        if let Some(feed) = self.active_feed().cloned() {
            feed.update(cx, |f, cx| f.resume(cx));
        }
        cx.notify();
    }

    fn open_favourite(&mut self, index: usize, cx: &mut Context<Self>) {
        let items = Store::entity(cx).read(cx).library.favourites.clone();
        self.feed.update(cx, |f, cx| f.pause_all(cx));
        let view = cx.new(|cx| FeedView::list(self.tiktok.clone(), self.io.clone(), items, index, cx));
        cx.subscribe(&view, Self::on_feed_event).detach();
        self.watching = Some(view);
        self.page = Page::Watching;
        self.page_turns += 1;
        cx.notify();
    }

    /// Follow the cover's hue: fade the theme over to its tint.
    fn follow_tint(&mut self, palette: Palette, cx: &mut Context<Self>) {
        let near = |a: Option<Hsla>, b: Option<Hsla>| match (a, b) {
            (Some(a), Some(b)) => (a.h - b.h).abs() < 0.01 && (a.s - b.s).abs() < 0.02,
            (None, None) => true,
            _ => false,
        };
        if near(palette.primary, self.tint.primary) && near(palette.secondary, self.tint.secondary) {
            return;
        }
        self.tint = palette;
        let tint = palette;
        let from = *cx.theme();
        let to = Theme::for_palette(palette);
        cx.spawn(async move |this, cx| {
            let start = Instant::now();
            loop {
                let t = (start.elapsed().as_secs_f32() / THEME_FADE.as_secs_f32()).min(1.);
                let still = this.update(cx, |this, cx| {
                    if this.tint != tint {
                        return false; // a newer fade took over
                    }
                    cx.set_global(from.mix(&to, ease_out_cubic(t)));
                    cx.refresh_windows();
                    true
                });
                if !matches!(still, Ok(true)) || t >= 1. {
                    break;
                }
                cx.background_executor().timer(Duration::from_millis(8)).await;
            }
        })
        .detach();
    }

    fn with_feed(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut FeedView, &mut Context<FeedView>)) {
        if let Some(feed) = self.active_feed().cloned() {
            feed.update(cx, f);
        }
    }

    // ── chrome ───────────────────────────────────────────────────────────

    fn title_bar(
        &self,
        sidebar_shown: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = *cx.theme();
        let prefs = Store::prefs(cx).clone();
        let folded = prefs.sidebar && !sidebar_shown;
        let (toggle_icon, toggle_tip) = if folded || !prefs.sidebar && !sidebar_shown {
            if folded { ("icons/menu.svg", "menu") } else { ("icons/panel-left-open.svg", "open") }
        } else {
            ("icons/panel-left-close.svg", "close")
        };
        let decorated = matches!(window.window_decorations(), gpui::Decorations::Client { .. });
        let title = match self.page {
            Page::Watching => "Избранное".to_string(),
            Page::Settings => "Настройки".to_string(),
            p => NAV.iter().find(|(n, ..)| *n == p).map(|(_, _, l)| l.to_string()).unwrap_or_default(),
        };

        div()
            .flex()
            .items_center()
            .flex_none()
            .w_full()
            .h(px(TITLEBAR))
            .px_2()
            .gap_1()
            .border_b_1()
            .border_color(theme.title_bar_border)
            .bg(theme.background.opacity(if self.ambient_on { 0.35 } else { 1. }))
            .window_control_area(gpui::WindowControlArea::Drag)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, window, _| match e.click_count {
                    1 => this.grabbed = true,
                    2 => window.zoom_window(),
                    _ => {}
                }),
            )
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, _| this.grabbed = false))
            .on_mouse_move(cx.listener(|this, _: &MouseMoveEvent, window, _| {
                if this.grabbed {
                    this.grabbed = false;
                    window.start_window_move();
                }
            }))
            .child(
                Button::new(SharedString::from(format!("sidebar-{toggle_tip}")))
                    .icon(toggle_icon)
                    .small()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if folded {
                            this.drawer_open = !this.drawer_open;
                            this.drawer.set(if this.drawer_open { 1. } else { 0. });
                            cx.notify();
                        } else {
                            Store::update_prefs(cx, |p| p.sidebar = !p.sidebar);
                        }
                    })),
            )
            .when(self.page == Page::Watching, |el| {
                el.child(
                    Button::new("back-favourites")
                        .icon("icons/chevron-up.svg")
                        .small()
                        .label("Назад")
                        .on_click(cx.listener(|this, _, _, cx| this.navigate(Page::Favourites, cx))),
                )
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .pl_1()
                    .child(div().font_weight(FontWeight::BOLD).child("tokers"))
                    .child(
                        div()
                            .text_color(theme.muted_foreground)
                            .text_size(theme.text(Text::Label))
                            .child(title),
                    ),
            )
            .child(div().flex_1())
            .child(self.volume(cx))
            .when(decorated, |el| el.child(div().pl_2().child(window_controls(window, cx))))
    }

    fn volume(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let prefs = Store::prefs(cx).clone();
        let silent = prefs.muted || prefs.volume <= 0.001;
        div()
            .id("volume")
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .h(px(26.))
            .rounded(theme.radius)
            .cursor_pointer()
            .hover(|s| s.bg(theme.secondary_hover))
            .child(
                icon(if silent { "icons/volume-x.svg" } else { "icons/volume-2.svg" })
                    .size(px(16.))
                    .text_color(theme.foreground),
            )
            .child(
                div().w(px(56.)).h(px(4.)).rounded_full().bg(theme.muted).child(
                    div()
                        .h_full()
                        .rounded_full()
                        .bg(if silent { theme.muted_foreground } else { theme.primary })
                        .w(gpui::relative(if prefs.muted { 0. } else { prefs.volume })),
                ),
            )
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(|_, _, cx| Store::update_prefs(cx, |p| p.muted = !p.muted))
            .on_scroll_wheel(|e: &ScrollWheelEvent, _, cx| {
                let step = match e.delta {
                    ScrollDelta::Lines(l) => l.y * 0.05,
                    ScrollDelta::Pixels(p) => f32::from(p.y) * 0.002,
                };
                Store::update_prefs(cx, |p| {
                    p.volume = (p.volume + step).clamp(0., 1.);
                    p.muted = false;
                });
            })
    }

    fn sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let active = match self.page {
            Page::Watching => Some(3),
            Page::Settings => None,
            p => NAV.iter().position(|(n, ..)| *n == p),
        };
        if let Some(i) = active {
            self.highlight.set(i as f32 * (ROW + ROW_GAP));
        }
        let y = self.highlight.tick(window, cx);
        let row = |id: SharedString, path: &'static str, label: &'static str, on: bool| {
            div()
                .id(id)
                .flex()
                .items_center()
                .gap_2p5()
                .h(px(ROW))
                .px_3()
                .rounded(theme.radius)
                .cursor_pointer()
                .text_color(if on { theme.foreground } else { theme.muted_foreground })
                .hover(|s| s.bg(theme.sidebar_accent.opacity(0.5)).text_color(theme.foreground))
                .child(icon(path).size(px(16.)).text_color(if on {
                    theme.foreground
                } else {
                    theme.muted_foreground
                }))
                .child(label)
        };
        div()
            .relative()
            .flex()
            .flex_col()
            .w(px(SIDEBAR))
            .h_full()
            .flex_none()
            .p(px(NAV_PAD))
            .bg(theme.sidebar.opacity(if self.ambient_on { 0.35 } else { 1. }))
            .border_r_1()
            .border_color(theme.sidebar_border)
            .when_some(active, |el, _| {
                el.child(
                    div()
                        .absolute()
                        .left(px(NAV_PAD))
                        .right(px(NAV_PAD))
                        .top(px(NAV_PAD + y))
                        .h(px(ROW))
                        .rounded(theme.radius)
                        .bg(theme.sidebar_accent),
                )
            })
            .child(div().flex().flex_col().gap(px(ROW_GAP)).children(NAV.iter().enumerate().map(
                |(i, (page, path, label))| {
                    let page = *page;
                    row(SharedString::from(format!("nav-{i}")), path, label, active == Some(i))
                        .on_click(cx.listener(move |this, _, _, cx| this.navigate(page, cx)))
                },
            )))
            .child(div().flex_1())
            .child(
                row("nav-settings".into(), "icons/settings.svg", "Настройки", self.page == Page::Settings)
                    .when(self.page == Page::Settings, |el| el.bg(theme.sidebar_accent))
                    .on_click(cx.listener(|this, _, _, cx| this.navigate(Page::Settings, cx))),
            )
    }

    fn page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = *cx.theme();
        match self.page {
            // cached: the ambient field repaints every frame, the feed only when it changes
            Page::ForYou => AnyView::from(self.feed.clone())
                .cached(StyleRefinement::default().size_full())
                .into_any_element(),
            Page::Watching => match &self.watching {
                Some(w) => {
                    AnyView::from(w.clone()).cached(StyleRefinement::default().size_full()).into_any_element()
                }
                None => div().into_any_element(),
            },
            Page::Settings => self.settings(cx),
            Page::Favourites => self.favourites(cx),
            Page::Downloads => vacancy(
                "icons/download.svg",
                "Загрузки",
                format!("Видео, скачанные клавишей D, сохраняются в {}", downloads_dir().display()),
                Some((
                    "Открыть папку",
                    Box::new(|_: &mut Window, cx: &mut App| {
                        let dir = downloads_dir();
                        let _ = std::fs::create_dir_all(&dir);
                        cx.open_url(&format!("file://{}", dir.display()));
                    }),
                )),
                &theme,
            ),
            Page::Following => vacancy(
                "icons/users.svg",
                "Подписки",
                "Нужен вход в аккаунт — его пока нет.".into(),
                None,
                &theme,
            ),
            Page::Search => vacancy(
                "icons/search.svg",
                "Поиск",
                "Поиск видео появится в следующей версии.".into(),
                None,
                &theme,
            ),
            Page::Profile => vacancy(
                "icons/user-round.svg",
                "Профиль",
                "Нужен вход в аккаунт — его пока нет.".into(),
                None,
                &theme,
            ),
        }
    }

    fn favourites(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = *cx.theme();
        let items = Store::entity(cx).read(cx).library.favourites.clone();
        if items.is_empty() {
            return vacancy(
                "icons/bookmark.svg",
                "Избранное пусто",
                "Сохраняйте видео кнопкой с закладкой или клавишей S.".into(),
                None,
                &theme,
            );
        }
        let cards: Vec<AnyElement> = items
            .iter()
            .enumerate()
            .map(|(i, aweme)| {
                let cover = Images::get(&aweme.video.cover, 480, cx).or_else(|| {
                    aweme
                        .image_post_info
                        .images
                        .first()
                        .and_then(|im| Images::get(&im.display_image, 480, cx))
                });
                div()
                    .id(("fav", i))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .cursor_pointer()
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .h(px(240.))
                            .rounded(px(8.))
                            .overflow_hidden()
                            .bg(theme.secondary)
                            .border_1()
                            .border_color(theme.border)
                            .when_some(cover, |el, c| {
                                el.child(img(c).size_full().object_fit(ObjectFit::Cover))
                            })
                            .child(
                                div()
                                    .absolute()
                                    .left_2()
                                    .bottom_2()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .px_1p5()
                                    .py_0p5()
                                    .rounded(px(4.))
                                    .bg(theme.overlay)
                                    .text_size(theme.text(Text::Tiny))
                                    .text_color(gpui::white())
                                    .child(
                                        icon("icons/play-filled.svg").size(px(10.)).text_color(gpui::white()),
                                    )
                                    .child(crate::ui::compact(aweme.statistics.play_count)),
                            ),
                    )
                    .child(div().text_size(theme.text(Text::Small)).line_clamp(2).child(
                        if aweme.desc.is_empty() {
                            format!("@{}", aweme.author.handle())
                        } else {
                            aweme.desc.clone()
                        },
                    ))
                    .child(
                        div()
                            .text_size(theme.text(Text::Tiny))
                            .text_color(theme.muted_foreground)
                            .child(format!("@{}", aweme.author.handle())),
                    )
                    .hover(|s| s.opacity(0.85))
                    .on_click(cx.listener(move |this, _, _, cx| this.open_favourite(i, cx)))
                    .rising(SharedString::from(format!("fav-{}", aweme.aweme_id)))
                    .into_any_element()
            })
            .collect();
        scroll_area(
            "favourites",
            &self.favourites_scroll,
            &self.favourites_bar,
            div()
                .p_6()
                .flex()
                .flex_col()
                .gap_5()
                .child(heading("Избранное", format!("{} видео", items.len()), &theme))
                .child(div().grid().grid_cols(5).gap_4().children(cards)),
        )
    }

    fn settings(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = *cx.theme();
        let prefs = Store::prefs(cx).clone();
        let mode = prefs.comments_mode;
        let option = |id: &'static str, value: CommentsMode, title: &'static str, about: &'static str| {
            choice(id, mode == value, title, about, &theme)
                .on_click(move |_, _, cx| Store::update_prefs(cx, |p| p.comments_mode = value))
        };
        let glass = |id: &'static str, value: bool, title: &'static str, about: &'static str| {
            choice(id, prefs.liquid_glass == value, title, about, &theme)
                .on_click(move |_, _, cx| Store::update_prefs(cx, |p| p.liquid_glass = value))
        };
        let keys: [(&str, &str); 12] = [
            ("J / ↓", "следующее видео"),
            ("K / ↑", "предыдущее"),
            ("Пробел", "пауза"),
            ("← →", "перемотка ±5 с / фото"),
            ("L", "лайк"),
            ("C", "комментарии"),
            ("S", "в избранное"),
            ("M", "звук"),
            ("Y", "скопировать ссылку"),
            ("D", "скачать"),
            ("Ctrl+B", "сайдбар"),
            ("Esc", "закрыть"),
        ];
        scroll_area(
            "settings",
            &self.settings_scroll,
            &self.settings_bar,
            div()
                    .max_w(px(640.))
                    .p_6()
                    .flex()
                    .flex_col()
                    .gap_6()
                    .child(heading("Настройки", "Хранятся локально".into(), &theme))
                    .child(
                        section("Комментарии в узком окне", &theme)
                            .child(option(
                                "mode-sheet",
                                CommentsMode::Sheet,
                                "Шторкой снизу",
                                "Видео уменьшается и уезжает вверх, комментарии выезжают снизу.",
                            ))
                            .child(option(
                                "mode-expand",
                                CommentsMode::Expand,
                                "Расширять окно вправо",
                                "Окно растёт до широкого режима и возвращается обратно. В тайлинговых WM — шторка.",
                            )),
                    )

                    .child(
                        section("Кнопки у видео", &theme)
                            .child(glass(
                                "glass-liquid",
                                true,
                                "Жидкое стекло",
                                "Край кнопки преломляет видео под ней, по ободку бежит блик. Линза пока есть только на Linux.",
                            ))
                            .child(glass(
                                "glass-frosted",
                                false,
                                "Матовое стекло",
                                "Кнопки просто размывают то, что под ними.",
                            ))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .pt_1()
                                    .child(
                                        div()
                                            .flex()
                                            .justify_between()
                                            .child(div().font_weight(FontWeight::MEDIUM).child("Прозрачность"))
                                            .child(
                                                div()
                                                    .text_color(theme.muted_foreground)
                                                    .child(format!("{}%", (prefs.button_clarity * 100.).round())),
                                            ),
                                    )
                                    .child(slider(
                                        "button-clarity",
                                        prefs.button_clarity,
                                        |v, cx| Store::tweak_prefs(cx, |p| p.button_clarity = v),
                                        Store::save_prefs,
                                        cx,
                                    )),
                            ),
                    )
                    .child(
                        section("Подсветка вокруг видео", &theme)
                            .child(
                                choice(
                                    "ambilight",
                                    prefs.ambilight,
                                    "Свечение цветами видео",
                                    "Края видео подсвечивают фон вокруг него, как Ambilight у телевизоров. Чёрные полосы не светят.",
                                    &theme,
                                )
                                .on_click(|_, _, cx| Store::update_prefs(cx, |p| p.ambilight = !p.ambilight)),
                            )
                            .child(labelled_slider(
                                "ambilight-strength",
                                "Яркость",
                                prefs.ambilight_strength,
                                |p, v| p.ambilight_strength = v,
                                &theme,
                                cx,
                            ))
                            .child(labelled_slider(
                                "ambilight-spread",
                                "Размах",
                                prefs.ambilight_spread,
                                |p, v| p.ambilight_spread = v,
                                &theme,
                                cx,
                            )),
                    )
                    .child(
                        section("Клавиши", &theme).child(div().grid().grid_cols(2).gap_x_6().gap_y_2().children(keys.iter().map(
                            |(k, what)| {
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .child(
                                        div()
                                            .min_w(px(64.))
                                            .px_2()
                                            .py_0p5()
                                            .rounded(px(4.))
                                            .bg(theme.secondary)
                                            .border_1()
                                            .border_color(theme.border)
                                            .text_size(theme.text(Text::Small))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_center()
                                            .child(*k),
                                    )
                                    .child(div().text_color(theme.muted_foreground).child(*what))
                            },
                        ))),
                    )
                    .child(
                        section("О программе", &theme).child(
                            div()
                                .text_color(theme.muted_foreground)
                                .text_size(theme.text(Text::Small))
                                .child("tokers — нативный клиент TikTok на gpui. Стиль и анимации по мотивам Sonora."),
                        ),
                    ),
        )
    }
}

/// A vertically scrolling page with an overlay scrollbar.
fn scroll_area(
    id: &'static str,
    handle: &ScrollHandle,
    bar: &Entity<Scrollbar>,
    content: impl IntoElement,
) -> AnyElement {
    let (wheel, hover) = (bar.clone(), bar.clone());
    div()
        .id(id)
        .relative()
        .size_full()
        .on_hover(move |hovered, _, cx| hover.update(cx, |b, cx| b.set_hovered(*hovered, cx)))
        .child(
            div()
                .id(SharedString::from(format!("{id}-scroll")))
                .size_full()
                .overflow_y_scroll()
                .track_scroll(handle)
                .on_scroll_wheel(move |e, window, cx| wheel.update(cx, |b, cx| b.wheel(e, window, cx)))
                .child(content),
        )
        .child(bar.clone())
        .into_any_element()
}

fn heading(title: &str, sub: String, theme: &Theme) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div().text_size(theme.text(Text::Title)).font_weight(FontWeight::BOLD).child(title.to_string()),
        )
        .child(div().text_color(theme.muted_foreground).child(sub))
}

/// A setting's name, its value in percent and a slider for it.
fn labelled_slider(
    id: &'static str,
    label: &'static str,
    value: f32,
    set: fn(&mut crate::state::Prefs, f32),
    theme: &Theme,
    cx: &App,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .pt_1()
        .child(
            div().flex().justify_between().child(div().font_weight(FontWeight::MEDIUM).child(label)).child(
                div().text_color(theme.muted_foreground).child(format!("{}%", (value * 100.).round())),
            ),
        )
        .child(slider(id, value, move |v, cx| Store::tweak_prefs(cx, |p| set(p, v)), Store::save_prefs, cx))
}

/// A radio row: a dot, a title and a line about it.
fn choice(
    id: &'static str,
    on: bool,
    title: &'static str,
    about: &'static str,
    theme: &Theme,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .items_start()
        .gap_3()
        .p_3()
        .rounded(px(8.))
        .border_1()
        .border_color(if on { theme.primary.opacity(0.6) } else { theme.border })
        .bg(if on { theme.secondary } else { gpui::transparent_black() })
        .cursor_pointer()
        .hover(|s| s.bg(theme.secondary_hover))
        .child(
            div()
                .mt_0p5()
                .size(px(16.))
                .flex_none()
                .rounded_full()
                .border_1()
                .border_color(if on { theme.primary } else { theme.muted_foreground })
                .flex()
                .items_center()
                .justify_center()
                .when(on, |el| {
                    el.child(div().size(px(8.)).rounded_full().bg(theme.primary).motion(
                        SharedString::from(format!("radio-{id}")),
                        Motion::Quick,
                        |el, t| el.layer_scale(t),
                    ))
                }),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(div().font_weight(FontWeight::MEDIUM).child(title))
                .child(
                    div().text_size(theme.text(Text::Small)).text_color(theme.muted_foreground).child(about),
                ),
        )
}

/// Colour emoji fonts across systems, tried in order; the ones not installed are skipped.
const EMOJI_FONTS: [&str; 4] = ["Noto Color Emoji", "Apple Color Emoji", "Segoe UI Emoji", "Twemoji"];

fn section(label: &str, theme: &Theme) -> gpui::Div {
    div().flex().flex_col().gap_3().child(
        div()
            .text_size(theme.text(Text::Small))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme.muted_foreground)
            .child(label.to_uppercase()),
    )
}

type Act = Box<dyn Fn(&mut Window, &mut App) + 'static>;

fn vacancy(
    path: &'static str,
    title: &str,
    text: String,
    action: Option<(&'static str, Act)>,
    theme: &Theme,
) -> AnyElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap_3()
                .max_w(px(380.))
                .child(
                    div()
                        .size(px(56.))
                        .rounded_full()
                        .bg(theme.secondary)
                        .border_1()
                        .border_color(theme.border)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon(path).size(px(24.)).text_color(theme.muted_foreground)),
                )
                .child(
                    div()
                        .text_size(theme.text(Text::Large))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title.to_string()),
                )
                .child(div().text_center().text_color(theme.muted_foreground).child(text))
                .when_some(action, |el, (label, act)| {
                    el.child(
                        Button::new("vacancy-action")
                            .secondary()
                            .label(label)
                            .on_click(move |_, w, cx| act(w, cx)),
                    )
                })
                .rising(SharedString::from(format!("vacancy-{title}"))),
        )
        .into_any_element()
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        for bar in [&self.favourites_bar, &self.settings_bar] {
            bar.read(cx).sync();
        }
        let theme = *cx.theme();
        let viewport = window.viewport_size();
        let prefs = Store::prefs(cx).clone();
        let layout = Layout::compute(
            f32::from(viewport.width),
            f32::from(viewport.height),
            prefs.sidebar,
            false,
            crate::layout::Mode::Medium,
        );
        let sidebar_shown = layout.sidebar;
        if sidebar_shown && self.drawer_open {
            self.drawer_open = false;
            self.drawer.snap(0.);
        }
        for feed in [Some(&self.feed), self.watching.as_ref()].into_iter().flatten() {
            feed.update(cx, |f, cx| {
                if f.sidebar != prefs.sidebar {
                    f.sidebar = prefs.sidebar;
                    cx.notify(); // it is cached: tell it its room changed
                }
            });
        }

        // theme and backdrop follow the video on screen
        let feed = self.active_feed().cloned();
        let (tint, ambient) = match &feed {
            Some(feed) => {
                let current = feed.read(cx).current().cloned();
                match current {
                    Some(aweme) => {
                        let list = crate::feed::cover(&aweme);
                        let palette = Images::palette(list, crate::feed::COVER_PX, cx).unwrap_or(self.tint);
                        (palette, true)
                    }
                    None => (self.tint, true),
                }
            }
            None => (self.tint, false),
        };
        self.follow_tint(tint, cx);
        let ambient = ambient && crate::ambient::enabled();
        self.ambient_on = ambient;
        let feed_page = matches!(self.page, Page::ForYou | Page::Watching);
        let drawer_t = self.drawer.tick(window, cx).clamp(0., 1.);
        let radius = window_radius(window);
        let page = self.page(cx);
        let sidebar = sidebar_shown.then(|| self.sidebar(window, cx).into_any_element());
        let drawer =
            (!sidebar_shown && drawer_t > 0.001).then(|| self.sidebar(window, cx).into_any_element());

        div()
            .id("root")
            .track_focus(&self.focus)
            .key_context("Tokers")
            .relative()
            .size_full()
            .font_family("Inter")
            .map(|mut el| {
                // Emoji go to a colour emoji font first: left to the system's fallback,
                // some (😂) land in DejaVu Sans, which has them as plain outlines.
                el.text_style().font_fallbacks = Some(gpui::FontFallbacks::from_fonts(
                    EMOJI_FONTS.iter().map(|f| f.to_string()).collect(),
                ));
                el
            })
            .text_size(theme.font_size)
            .text_color(theme.foreground)
            .bg(theme.background)
            .when_some(radius, |el, r| el.rounded(r).overflow_hidden())
            .on_action(cx.listener(|this, _: &Next, _, cx| this.with_feed(cx, |f, cx| f.go(1, cx))))
            .on_action(cx.listener(|this, _: &Previous, _, cx| this.with_feed(cx, |f, cx| f.go(-1, cx))))
            .on_action(
                cx.listener(|this, _: &TogglePlay, _, cx| this.with_feed(cx, |f, cx| f.toggle_play(cx))),
            )
            .on_action(
                cx.listener(|this, _: &SeekBack, _, cx| this.with_feed(cx, |f, cx| f.seek_by(-5., cx))),
            )
            .on_action(
                cx.listener(|this, _: &SeekForward, _, cx| this.with_feed(cx, |f, cx| f.seek_by(5., cx))),
            )
            .on_action(
                cx.listener(|this, _: &ToggleLike, _, cx| this.with_feed(cx, |f, cx| f.toggle_like(cx))),
            )
            .on_action(cx.listener(|this, _: &ToggleFavourite, _, cx| {
                this.with_feed(cx, |f, cx| f.toggle_favourite(cx))
            }))
            .on_action(cx.listener(|this, _: &CopyLink, _, cx| this.with_feed(cx, |f, cx| f.copy_link(cx))))
            .on_action(cx.listener(|this, _: &Download, _, cx| this.with_feed(cx, |f, cx| f.download(cx))))
            .on_action(cx.listener(|this, _: &ToggleComments, window, cx| {
                if let Some(feed) = this.active_feed().cloned() {
                    feed.update(cx, |f, cx| f.toggle_comments(window, cx));
                }
            }))
            .on_action(
                cx.listener(|_, _: &ToggleMute, _, cx| Store::update_prefs(cx, |p| p.muted = !p.muted)),
            )
            .on_action(
                cx.listener(|_, _: &ToggleSidebar, _, cx| {
                    Store::update_prefs(cx, |p| p.sidebar = !p.sidebar)
                }),
            )
            .on_action(cx.listener(|this, _: &Close, window, cx| {
                if this.drawer_open {
                    this.drawer_open = false;
                    this.drawer.set(0.);
                    cx.notify();
                } else if let Some(feed) =
                    this.active_feed().cloned().filter(|f| f.read(cx).has_open_comments())
                {
                    feed.update(cx, |f, cx| f.close_comments_in(window, cx));
                } else if this.page == Page::Watching {
                    this.navigate(Page::Favourites, cx);
                }
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| this.focus.focus(window, cx)))
            // the cover, blurred and dim, behind everything
            // not cached: the renderer doesn't replay its filtered layer from a cache
            .when(ambient, |el| el.child(self.ambient.clone()))
            .child(
                div()
                    .relative()
                    .size_full()
                    .flex()
                    .flex_col()
                    .child(self.title_bar(sidebar_shown, window, cx))
                    .child(div().flex().flex_1().min_h_0().children(sidebar).child(
                        div().relative().flex_1().min_w_0().h_full().child(if feed_page {
                            // over a cached view only the compositor's filters may animate
                            div()
                                .size_full()
                                .child(page)
                                .veiling(("page", self.page_turns))
                                .into_any_element()
                        } else {
                            div().size_full().child(page).rising(("page", self.page_turns)).into_any_element()
                        }),
                    )),
            )
            .when_some(drawer, |el, drawer| {
                el.child(
                    div()
                        .id("drawer-scrim")
                        .absolute()
                        .top(px(TITLEBAR))
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .bg(theme.overlay.opacity(drawer_t))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.drawer_open = false;
                            this.drawer.set(0.);
                            cx.notify();
                        })),
                )
                .child(
                    div()
                        .absolute()
                        .top(px(TITLEBAR))
                        .bottom_0()
                        .left(px(-SIDEBAR * (1. - drawer_t)))
                        .shadow_lg()
                        .occlude()
                        .child(drawer),
                )
            })
            .when_some(self.toast.clone(), |el, (text, n)| {
                el.child(
                    div().absolute().bottom(px(28.)).left_0().right_0().flex().justify_center().child(
                        div()
                            .px_4()
                            .py_2()
                            .rounded(px(8.))
                            .bg(theme.popover.opacity(0.85))
                            .backdrop_blur(px(8.))
                            .border_1()
                            .border_color(theme.border)
                            .shadow_md()
                            .text_size(theme.text(Text::Label))
                            .child(text)
                            .rising(("toast", n)),
                    ),
                )
            })
            .children(window_frame(window))
    }
}
