//! Motion presets and helpers, after Sonora (`crates/ui/src/motion.rs`).

use std::time::{Duration, Instant};

use gpui::{
    Animation, AnimationElement, AnimationExt as _, App, ElementId, Hsla, IntoElement, Pixels, Rgba,
    SpringConfig, SpringState, Styled, Window, ease_in_out, ease_out_quint, px,
};

const ENTRANCE_BLUR: Pixels = px(1.5);
const ENTRANCE_ZOOM: f32 = 0.01;

pub enum Springs {}

impl Springs {
    /// Fast, nearly critically damped feedback for direct UI transitions.
    pub const RESPONSIVE: SpringConfig = SpringConfig::new(360., 38., 1.);
    /// Feed paging: a touch of overshoot, settles in ~0.25 s.
    pub const PAGE: SpringConfig = SpringConfig::new(560., 44., 1.);
    /// Panels and sheets sliding in.
    pub const PANEL: SpringConfig = SpringConfig::new(300., 34., 1.);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    Base,
    Slow,
}

impl Motion {
    pub fn span(self) -> Duration {
        Duration::from_millis(match self {
            Motion::Base => 200,
            Motion::Slow => 320,
        })
    }

    pub fn animation(self) -> Animation {
        let animation = Animation::new(self.span());
        match self {
            Motion::Base => animation.with_easing(ease_in_out),
            Motion::Slow => animation.with_easing(ease_out_quint()),
        }
    }
}

fn entrance() -> Animation {
    Animation::new(Motion::Base.span() + Duration::from_millis(50)).with_easing(ease_out_expo)
}

/// Scale and blur of an element `hidden` of the way out (0 = fully in).
pub fn veiled<E: Styled>(element: E, hidden: f32) -> E {
    let hidden = hidden.clamp(0., 1.);
    if hidden <= 0. {
        // at rest: no filter layer at all
        return element;
    }
    element.layer_scale(1. - ENTRANCE_ZOOM * hidden).blur(ENTRANCE_BLUR * hidden)
}

/// The whole entrance: the veil plus the fade that goes with it.
pub fn entering<E: Styled>(element: E, hidden: f32) -> E {
    if hidden <= 0. {
        return element;
    }
    veiled(element, hidden).opacity(1. - hidden.clamp(0., 1.))
}

/// The entrance without the fade, safe over a cached view (opacity would freeze there).
pub trait Veiling: Sized {
    fn veiling(self, id: impl Into<ElementId>) -> AnimationElement<Self>;
}

impl<E: Styled + IntoElement + 'static> Veiling for E {
    fn veiling(self, id: impl Into<ElementId>) -> AnimationElement<Self> {
        self.with_animation(id, entrance(), |element, delta| veiled(element, 1. - delta))
    }
}

pub trait Rising: Sized {
    fn rising(self, id: impl Into<ElementId>) -> AnimationElement<Self>;
}

impl<E: Styled + IntoElement + 'static> Rising for E {
    fn rising(self, id: impl Into<ElementId>) -> AnimationElement<Self> {
        self.with_animation(id, entrance(), |element, delta| entering(element, 1. - delta))
    }
}

pub trait Motioned: Sized {
    fn motion(
        self,
        id: impl Into<ElementId>,
        motion: Motion,
        animator: impl Fn(Self, f32) -> Self + 'static,
    ) -> AnimationElement<Self>;
}

impl<E: IntoElement + 'static> Motioned for E {
    fn motion(
        self,
        id: impl Into<ElementId>,
        motion: Motion,
        animator: impl Fn(Self, f32) -> Self + 'static,
    ) -> AnimationElement<Self> {
        self.with_animation(id, motion.animation(), animator)
    }
}

/// A spring stepped by its owner on every render, for motion that a view drives
/// from its own state (paging, sheets, indicators). Keeps velocity across retargets.
#[derive(Clone, Copy, Debug)]
pub struct Spring {
    config: SpringConfig,
    state: SpringState,
    target: f32,
    stepped: Instant,
    epsilon: f32,
}

impl Spring {
    pub fn new(config: SpringConfig, at: f32) -> Self {
        Spring {
            config,
            state: SpringState { position: at, velocity: 0. },
            target: at,
            stepped: Instant::now(),
            epsilon: 0.001,
        }
    }

    /// Counts as at rest this close to the target (in its own units: pixels need less care).
    pub fn resting_within(mut self, epsilon: f32) -> Self {
        self.epsilon = epsilon;
        self
    }

    pub fn set(&mut self, target: f32) {
        if self.settled() {
            self.stepped = Instant::now();
        }
        self.target = target;
    }

    /// Jump there with no motion.
    pub fn snap(&mut self, to: f32) {
        self.target = to;
        self.state = SpringState { position: to, velocity: 0. };
    }

    pub fn settled(&self) -> bool {
        self.config.is_settled(self.state, self.target, self.epsilon)
    }

    /// Where it is now (as of the last tick).
    pub fn value(&self) -> f32 {
        self.state.position
    }

    /// Units per second, signed.
    pub fn velocity(&self) -> f32 {
        self.state.velocity
    }

    /// Adds an impulse: the spring swings away and back (a press, a landing).
    pub fn kick(&mut self, velocity: f32) {
        if self.settled() {
            self.stepped = Instant::now();
        }
        self.state.velocity += velocity;
    }

    /// Advance to now and return the value; asks for another frame until it rests.
    pub fn tick(&mut self, window: &mut Window, cx: &App) -> f32 {
        let now = Instant::now();
        let dt = now.duration_since(self.stepped).as_secs_f32().min(0.064);
        self.stepped = now;
        if cx.reduce_motion() {
            self.state = SpringState { position: self.target, velocity: 0. };
        } else {
            self.state = self.config.step(self.state, self.target, dt);
            if self.settled() {
                self.state = SpringState { position: self.target, velocity: 0. };
            } else {
                window.request_animation_frame();
            }
        }
        self.state.position
    }
}

pub fn mix(from: Hsla, to: Hsla, t: f32) -> Hsla {
    let (from, to) = (Rgba::from(from), Rgba::from(to));
    let step = t.clamp(0., 1.);
    let blend = |a: f32, b: f32| a + (b - a) * step;
    Rgba { r: blend(from.r, to.r), g: blend(from.g, to.g), b: blend(from.b, to.b), a: blend(from.a, to.a) }
        .into()
}

pub fn ease_out_expo(progress: f32) -> f32 {
    cubic_bezier(progress.clamp(0., 1.), 0.16, 1., 0.3, 1.)
}

pub fn ease_out_cubic(progress: f32) -> f32 {
    cubic_bezier(progress.clamp(0., 1.), 0.33, 1., 0.68, 1.)
}

fn cubic_bezier(progress: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    if progress == 0. || progress == 1. {
        return progress;
    }
    let axis = |t: f32, a: f32, b: f32| {
        let r = 1. - t;
        3. * r * r * t * a + 3. * r * t * t * b + t * t * t
    };
    let slope = |t: f32| {
        let r = 1. - t;
        3. * r * r * x1 + 6. * r * t * (x2 - x1) + 3. * t * t * (1. - x2)
    };
    let mut parameter = progress;
    for _ in 0..6 {
        let gradient = slope(parameter);
        if gradient.abs() <= f32::EPSILON {
            break;
        }
        parameter = (parameter - (axis(parameter, x1, x2) - progress) / gradient).clamp(0., 1.);
    }
    axis(parameter, y1, y2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expo_easing_has_css_endpoints_and_shape() {
        assert_eq!(ease_out_expo(0.), 0.);
        assert_eq!(ease_out_expo(1.), 1.);
        assert!(ease_out_expo(0.25) > 0.8);
    }
}
