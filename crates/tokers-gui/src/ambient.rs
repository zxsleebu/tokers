//! The ambient background, ported from Sonora (`crates/views/src/shared/ambient.rs`):
//! five soft blobs in the cover's colours drifting under a light blur, drawn at a
//! fraction of the window and scaled up by the compositor, darkened so the UI on
//! top stays readable. A new cover washes in rather than cuts.

use std::f32::consts::TAU;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{Context, Div, Hsla, Render, RenderImage, Rgba, WeakEntity, Window, div, img, point, px};
use image::{Frame, RgbaImage};

use crate::theme::{ActiveTheme as _, Theme};

const BLOBS: usize = 5;
/// The widest blur, in device pixels, the renderer still runs at full resolution.
const BLUR_FULL: f32 = 4.;
/// The field is drawn this many times smaller than it is shown.
const DOWNSCALE: f32 = 4.;
/// How much of itself the field keeps under the dark overlay folded into its colours.
const SHADE: f32 = 0.36;
/// Base centre (fractions), diameter (fraction of the smaller side), period (s),
/// phase, drift amplitude x / y (fractions).
const SPECS: [(f32, f32, f32, f32, f32, f32, f32); BLOBS] = [
    (0.22, 0.30, 1.25, 22., 0.0, 0.17, 0.13),
    (0.80, 0.24, 1.15, 18., 1.7, 0.15, 0.17),
    (0.52, 0.68, 1.35, 27., 3.4, 0.18, 0.12),
    (0.12, 0.78, 1.05, 16., 5.1, 0.14, 0.16),
    (0.85, 0.72, 0.90, 20., 2.5, 0.16, 0.14),
];
/// Concentric discs faking a radial falloff under the narrow blur.
const DISCS: usize = 16;
const DISC_FAINT: f32 = 0.055;
const DISC_STRONG: f32 = 0.16;
/// Time constant of the ease onto a new palette, in seconds.
const WASH: f32 = 0.8;
/// The drift moves well under a pixel a frame and sits under a blur: 60 updates a
/// second are as smooth as the display's 165, at a third of the cost.
const FRAME: Duration = Duration::from_micros(16_667);

/// `TOKERS_AMBIENT=0` turns the field off (for slow GPUs and for measuring).
pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("TOKERS_AMBIENT").map_or(true, |v| v != "0"))
}

pub struct Ambient {
    started: Instant,
    stepped: Instant,
    painted: Option<[Hsla; BLOBS]>,
}

impl Ambient {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        let now = Instant::now();
        Ambient { started: now, stepped: now, painted: None }
    }

    /// Eases the painted colours one frame toward `target`.
    fn wash(&mut self, target: [Hsla; BLOBS], animates: bool) -> [Hsla; BLOBS] {
        let now = Instant::now();
        let step = now.duration_since(self.stepped).as_secs_f32();
        self.stepped = now;
        let painted = match self.painted {
            Some(painted) if animates => {
                let delta = 1. - (-step / WASH).exp();
                std::array::from_fn(|i| blend(painted[i], target[i], delta))
            }
            _ => target,
        };
        self.painted = Some(painted);
        painted
    }

    /// The cover's leading hue from a light highlight to a dark shadow, plus its
    /// runner-up; quiet neutrals for art without colour.
    fn colors(theme: &Theme) -> [Hsla; BLOBS] {
        let neutral = |l: f32| Hsla { h: 0.06, s: 0.12, l, a: 1. };
        let floor = (0.10 + theme.background.l * 0.8).clamp(0.10, 0.24);
        let Some(tint) = theme.tint else {
            return [
                neutral(floor),
                neutral(floor + 0.025),
                neutral(floor + 0.05),
                neutral(floor + 0.015),
                neutral(floor + 0.04),
            ];
        };
        let accent = Hsla { h: tint.h, s: tint.s.clamp(0.6, 0.85), l: 0.44, a: 1. };
        let base = Hsla {
            h: accent.h,
            s: (accent.s * 0.95 + 0.08).clamp(0.55, 0.85),
            l: (0.42 + (accent.l - 0.45) * 0.5).clamp(0.32, 0.55),
            a: 1.,
        };
        let second = match theme.tint_secondary {
            Some(second) => Hsla {
                h: second.h,
                s: (second.s * 0.9).clamp(0.4, 0.72),
                l: (0.38 + (second.l - 0.45) * 0.5).clamp(0.28, 0.5),
                a: 1.,
            },
            None => neutral(floor + 0.03),
        };
        [
            base,
            Hsla { l: (base.l + 0.14).clamp(0.3, 0.62), s: (base.s - 0.08).clamp(0.4, 0.75), ..base },
            second,
            Hsla { l: (base.l - 0.13).clamp(0.18, 0.45), s: (base.s + 0.04).clamp(0.45, 0.8), ..base },
            Hsla {
                h: (base.h + 0.97).rem_euclid(1.),
                l: (base.l - 0.16).clamp(0.16, 0.4),
                s: (base.s - 0.02).clamp(0.4, 0.78),
                ..base
            },
        ]
    }
}

impl Render for Ambient {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let animates = !cx.reduce_motion();
        // Frames come off the display's clock: a timer is never in phase with the
        // refresh, and a slow drift on one reads as a twitch.
        let elapsed = if animates {
            arm(window, cx.entity().downgrade(), Instant::now());
            self.started.elapsed().as_secs_f32()
        } else {
            0.
        };
        let colors = self.wash(Self::colors(&theme), animates);
        let viewport = window.viewport_size();
        let wide = f32::from(viewport.width).max(1.) / DOWNSCALE;
        let high = f32::from(viewport.height).max(1.) / DOWNSCALE;

        div()
            .absolute()
            .inset_0()
            .overflow_hidden()
            .when_some(crate::ui::window_radius(window), |el, r| el.rounded(r))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .when_some(crate::ui::window_radius(window), |el, r| el.rounded(r))
                    .bg(shaded(Hsla { a: 1., ..theme.background })),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .w(px(wide))
                    .h(px(high))
                    .layer_scale(DOWNSCALE)
                    .layer_scale_origin(point(0., 0.))
                    .blur(px(BLUR_FULL / window.scale_factor()))
                    .children(SPECS.iter().enumerate().map(|(index, spec)| {
                        let (base_x, base_y, size, period, phase, amp_x, amp_y) = *spec;
                        let spin = TAU * elapsed / period;
                        let x = base_x + amp_x * (spin + phase).sin();
                        let y = base_y + amp_y * (spin * 0.83 + phase * 1.7).cos();
                        let grown = wide.min(high) * size * (1. + 0.12 * (spin * 0.6 + phase * 2.3).sin());
                        let color = colors[index];
                        div()
                            .absolute()
                            .left(px(x * wide - grown / 2.))
                            .top(px(y * high - grown / 2.))
                            .size(px(grown))
                            .children((0..DISCS).map(move |step| {
                                let fraction = 1. - step as f32 / DISCS as f32 * (1. - 1. / DISCS as f32);
                                let opacity = DISC_FAINT
                                    + step as f32 / (DISCS as f32 - 1.) * (DISC_STRONG - DISC_FAINT);
                                let stepped = grown * fraction;
                                div()
                                    .absolute()
                                    .left(px((grown - stepped) / 2.))
                                    .top(px((grown - stepped) / 2.))
                                    .size(px(stepped))
                                    .rounded_full()
                                    .bg(shaded(color).opacity(opacity))
                            }))
                    })),
            )
            .child(grain(window))
    }
}

/// Repaint on the first vsync at least `FRAME` after `since`: frames stay in phase
/// with the display, at no more than 60 a second.
fn arm(window: &mut Window, ambient: WeakEntity<Ambient>, since: Instant) {
    window.on_next_frame(move |window, cx| {
        if since.elapsed() + Duration::from_millis(2) >= FRAME {
            ambient.update(cx, |_, cx| cx.notify()).ok();
        } else {
            arm(window, ambient, since);
        }
    });
}

/// A colour under the dark overlay, folded in rather than painted as a sheet.
fn shaded(color: Hsla) -> Hsla {
    let rgba = Rgba::from(color);
    let kept = 1. - SHADE;
    Hsla::from(Rgba { r: rgba.r * kept, g: rgba.g * kept, b: rgba.b * kept, a: rgba.a })
}

/// Straight-line blend through RGB, so unrelated hues pass through grey.
fn blend(from: Hsla, to: Hsla, delta: f32) -> Hsla {
    let (from, to) = (Rgba::from(from), Rgba::from(to));
    let c = |a: f32, b: f32| a + (b - a) * delta;
    Hsla::from(Rgba { r: c(from.r, to.r), g: c(from.g, to.g), b: c(from.b, to.b), a: c(from.a, to.a) })
}

// ── dither (Sonora `crates/ui/src/grain.rs`) ──

const TILE: u32 = 512;
const PUSH: u8 = 2;

/// A dither tile: each pixel nudged toward white or black by up to `PUSH`, so a wide
/// dark gradient doesn't band.
fn tile() -> Arc<RenderImage> {
    static TILED: OnceLock<Arc<RenderImage>> = OnceLock::new();
    TILED
        .get_or_init(|| {
            let mut seed = 0x2545_f491_4f6c_dd1d_u64;
            let mut roll = move || {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                seed
            };
            let mut pixels = RgbaImage::new(TILE, TILE);
            for pixel in pixels.pixels_mut() {
                let draw = roll();
                let level = if draw & 1 == 0 { 0 } else { 255 };
                let alpha = (draw >> 8) as u8 % (PUSH + 1);
                *pixel = image::Rgba([level, level, level, alpha]);
            }
            Arc::new(RenderImage::new(vec![Frame::new(pixels)]))
        })
        .clone()
}

fn grain(window: &Window) -> Div {
    let scale = window.scale_factor().max(1.);
    let side = TILE as f32 / scale;
    let viewport = window.viewport_size();
    let across = (f32::from(viewport.width) / side).ceil().max(1.) as usize;
    let down = (f32::from(viewport.height) / side).ceil().max(1.) as usize;
    div().absolute().inset_0().overflow_hidden().children((0..down).flat_map(move |row| {
        (0..across).map(move |column| {
            img(tile())
                .absolute()
                .left(px(column as f32 * side))
                .top(px(row as f32 * side))
                .w(px(side))
                .h(px(side))
        })
    }))
}
