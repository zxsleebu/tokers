//! Smooth wheel scrolling, after Sonora (`crates/ui/src/glide.rs`): the scroll
//! container applies a wheel notch at once, the glide takes the jump back and eases
//! the view toward where it landed, a frame at a time. Further notches extend the
//! target, so a spin of the wheel reads as one continuous move. Touchpads already
//! scroll in fine steps and are left alone.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{App, EntityId, ListState, Pixels, Point, ScrollHandle, Window, point, px};

/// A scroll position the glide can drive.
pub trait ScrollPosition: Clone + 'static {
    fn offset(&self) -> Point<Pixels>;
    fn set_offset(&self, offset: Point<Pixels>);
    fn max_offset(&self) -> Point<Pixels>;
}

impl ScrollPosition for ScrollHandle {
    fn offset(&self) -> Point<Pixels> {
        ScrollHandle::offset(self)
    }
    fn set_offset(&self, offset: Point<Pixels>) {
        ScrollHandle::set_offset(self, offset);
    }
    fn max_offset(&self) -> Point<Pixels> {
        ScrollHandle::max_offset(self)
    }
}

impl ScrollPosition for ListState {
    fn offset(&self) -> Point<Pixels> {
        self.scroll_px_offset_for_scrollbar()
    }
    fn set_offset(&self, offset: Point<Pixels>) {
        self.set_offset_from_scrollbar(offset);
    }
    fn max_offset(&self) -> Point<Pixels> {
        self.max_offset_for_scrollbar()
    }
}

/// Share of the remaining distance covered per `HERTZ` tick.
const EASE: f32 = 0.12;
const HERTZ: f32 = 180.;
/// A frame later than this (a stall, a hidden window) is not caught up in one leap.
const STALL: Duration = Duration::from_millis(64);
const REST: Pixels = px(0.5);

#[derive(Default)]
struct Drift {
    shown: Point<Pixels>,
    target: Point<Pixels>,
    gliding: bool,
    armed: bool,
    beat: Option<Instant>,
}

#[derive(Clone, Default)]
pub struct Glide {
    drift: Rc<RefCell<Drift>>,
    /// The view that draws the scrolled content, repainted each step (else the window).
    watched: Option<EntityId>,
}

impl Glide {
    pub fn watch(&mut self, view: EntityId) {
        self.watched = Some(view);
    }

    /// Picks up wherever something else moved the view, unless a glide is under way.
    pub fn sync(&self, scroll: &impl ScrollPosition) {
        let mut drift = self.drift.borrow_mut();
        if !drift.gliding {
            drift.shown = scroll.offset();
        }
    }

    /// A wheel notch just moved `scroll`: glide there from where the view was shown.
    pub fn nudge(&self, scroll: &impl ScrollPosition, window: &mut Window) {
        {
            let mut drift = self.drift.borrow_mut();
            let step = scroll.offset() - drift.shown;
            let from = if drift.gliding { drift.target } else { drift.shown };
            drift.target = held(from + step, scroll);
            drift.gliding = true;
            scroll.set_offset(drift.shown);
        }
        self.schedule_frame(scroll, window);
    }

    /// Lands at `to` at once, ending any glide.
    pub fn jump(&self, scroll: &impl ScrollPosition, to: Point<Pixels>) {
        let landed = {
            let mut drift = self.drift.borrow_mut();
            drift.target = held(to, scroll);
            drift.shown = drift.target;
            drift.gliding = false;
            drift.beat = None;
            drift.shown
        };
        scroll.set_offset(landed);
    }

    fn schedule_frame(&self, scroll: &impl ScrollPosition, window: &mut Window) {
        {
            let mut drift = self.drift.borrow_mut();
            if drift.armed {
                return;
            }
            drift.armed = true;
        }
        let glide = self.clone();
        let scroll = scroll.clone();
        window.on_next_frame(move |window, cx| glide.step(&scroll, window, cx));
    }

    fn step(&self, scroll: &impl ScrollPosition, window: &mut Window, cx: &mut App) {
        let landed = {
            let mut drift = self.drift.borrow_mut();
            drift.armed = false;
            if !drift.gliding {
                return;
            }
            let now = Instant::now();
            let elapsed = drift
                .beat
                .replace(now)
                .map_or(Duration::from_secs_f32(1. / HERTZ), |beat| now.duration_since(beat).min(STALL));
            let target = held(drift.target, scroll);
            let ease = 1. - (1. - EASE).powf(elapsed.as_secs_f32() * HERTZ);
            let step = target - drift.shown;
            drift.shown += point(step.x * ease, step.y * ease);
            if step.x.abs() < REST && step.y.abs() < REST {
                drift.shown = target;
                drift.gliding = false;
                drift.beat = None;
                target
            } else {
                held(drift.shown, scroll)
            }
        };
        scroll.set_offset(landed);
        match self.watched {
            Some(view) => cx.notify(view),
            None => window.refresh(),
        }
        self.schedule_frame(scroll, window);
    }
}

/// Clamped to the scrollable range (offsets run from 0 down to -max).
fn held(at: Point<Pixels>, scroll: &impl ScrollPosition) -> Point<Pixels> {
    let reach = scroll.max_offset();
    point(
        at.x.clamp(-reach.x.max(Pixels::ZERO), Pixels::ZERO),
        at.y.clamp(-reach.y.max(Pixels::ZERO), Pixels::ZERO),
    )
}
