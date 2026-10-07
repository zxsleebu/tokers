//! Overlay scrollbar, after Sonora (`crates/ui/src/scrollbar.rs`): a thin thumb at
//! the right edge that shows while scrolling or hovered and can be dragged.
//! Works over a scrolling div ([`ScrollHandle`]) or a virtual [`ListState`].

use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{
    Context, DispatchPhase, Entity, EntityId, ListState, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Render, ScrollHandle, Window, canvas, div, point, px,
};

use crate::motion::{Spring, Springs};
use crate::theme::ActiveTheme as _;

const BAR: Pixels = px(6.);
const INSET: Pixels = px(3.);
const MIN_THUMB: Pixels = px(28.);
const LINGER: Duration = Duration::from_millis(1400);
const RESTING: f32 = 0.35;
const ACTIVE: f32 = 0.6;

#[derive(Clone)]
pub enum Target {
    Area(ScrollHandle),
    List(ListState),
}

impl Target {
    fn viewport(&self) -> Pixels {
        match self {
            Target::Area(s) => s.bounds().size.height,
            Target::List(s) => s.viewport_bounds().size.height,
        }
    }

    fn hidden(&self) -> Pixels {
        match self {
            Target::Area(s) => s.max_offset().y,
            Target::List(s) => s.max_offset_for_scrollbar().y,
        }
    }

    fn offset(&self) -> Pixels {
        let raw = match self {
            Target::Area(s) => -s.offset().y,
            Target::List(s) => -s.scroll_px_offset_for_scrollbar().y,
        };
        raw.clamp(Pixels::ZERO, self.hidden())
    }

    fn set_offset(&self, offset: Pixels) {
        let p = point(Pixels::ZERO, -offset);
        match self {
            Target::Area(s) => s.set_offset(p),
            Target::List(s) => s.set_offset_from_scrollbar(p),
        }
    }
}

pub struct Scrollbar {
    target: Target,
    awake_until: Instant,
    hovered: bool,
    /// Pointer y and scroll offset when the drag began.
    drag: Option<(Pixels, Pixels)>,
    opacity: Spring,
    /// The view that draws the scrolled content (repainted while dragging).
    owner: Option<EntityId>,
}

impl Scrollbar {
    pub fn new(target: Target) -> Self {
        Scrollbar {
            target,
            awake_until: Instant::now(),
            hovered: false,
            drag: None,
            opacity: Spring::new(Springs::RESPONSIVE, 0.),
            owner: None,
        }
    }

    pub fn area(handle: &ScrollHandle, owner: EntityId, cx: &mut gpui::App) -> Entity<Self> {
        cx.new(|_| Scrollbar { owner: Some(owner), ..Scrollbar::new(Target::Area(handle.clone())) })
    }

    pub fn list(state: &ListState, owner: EntityId, cx: &mut gpui::App) -> Entity<Self> {
        cx.new(|_| Scrollbar { owner: Some(owner), ..Scrollbar::new(Target::List(state.clone())) })
    }

    /// Something scrolled: show the bar for a moment.
    pub fn wake(&mut self, cx: &mut Context<Self>) {
        self.awake_until = Instant::now() + LINGER;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(LINGER + Duration::from_millis(20)).await;
            this.update(cx, |_, cx| cx.notify()).ok();
        })
        .detach();
    }

    /// The pointer is over the scrolled area.
    pub fn set_hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if self.hovered != hovered {
            self.hovered = hovered;
            cx.notify();
        }
    }
}

impl Render for Scrollbar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = self.target.viewport();
        let hidden = self.target.hidden();
        if hidden <= px(0.5) || viewport <= px(0.) {
            return div().into_any_element();
        }
        let track = viewport - INSET * 2.;
        let thumb = (track * (viewport / (viewport + hidden))).max(MIN_THUMB).min(track);
        let top = INSET + (track - thumb) * (self.target.offset() / hidden);

        let target = if self.drag.is_some() {
            ACTIVE
        } else if self.hovered || Instant::now() < self.awake_until {
            RESTING
        } else {
            0.
        };
        self.opacity.set(target);
        let opacity = self.opacity.tick(window, cx);
        let color = cx.theme().foreground;
        let dragging = self.drag.is_some();
        let this = cx.entity().downgrade();

        div()
            .absolute()
            .top_0()
            .right_0()
            .bottom_0()
            .w(BAR + INSET * 2.)
            .child(
                div()
                    .id("thumb")
                    .absolute()
                    .top(top)
                    .right(INSET)
                    .w(BAR)
                    .h(thumb)
                    .rounded_full()
                    .bg(color.opacity(opacity))
                    .hover(|s| s.bg(color.opacity(ACTIVE)))
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, e: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.drag = Some((e.position.y, this.target.offset()));
                            if let Target::List(s) = &this.target {
                                s.scrollbar_drag_started();
                            }
                            cx.notify();
                        }),
                    ),
            )
            .when(dragging, |el| {
                // follow the pointer anywhere in the window until the button comes up
                el.child(canvas(
                    |_, _, _| {},
                    move |_, _, window, _| {
                        let moving = this.clone();
                        window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                            if phase != DispatchPhase::Bubble {
                                return;
                            }
                            moving
                                .update(cx, |bar, cx| {
                                    if let Some((y0, offset0)) = bar.drag {
                                        let viewport = bar.target.viewport();
                                        let hidden = bar.target.hidden();
                                        let track = viewport - INSET * 2.;
                                        let thumb = (track * (viewport / (viewport + hidden)))
                                            .max(MIN_THUMB)
                                            .min(track);
                                        let room = (track - thumb).max(px(1.));
                                        let offset = offset0 + (e.position.y - y0) * (hidden / room);
                                        bar.target.set_offset(offset.clamp(Pixels::ZERO, hidden));
                                        if let Some(owner) = bar.owner {
                                            gpui::App::notify(cx, owner);
                                        }
                                        cx.notify();
                                    }
                                })
                                .ok();
                        });
                        let releasing = this.clone();
                        window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                            if phase != DispatchPhase::Bubble {
                                return;
                            }
                            releasing
                                .update(cx, |bar, cx| {
                                    bar.drag = None;
                                    if let Target::List(s) = &bar.target {
                                        s.scrollbar_drag_ended();
                                    }
                                    bar.wake(cx);
                                })
                                .ok();
                        });
                    },
                ))
            })
            .into_any_element()
    }
}
