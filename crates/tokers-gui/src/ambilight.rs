//! Ambient light around the video, as an Ambilight TV throws the picture's own colours
//! on the wall behind it. The approach follows "Ambient light for YouTube" by Wessel
//! Kroos (github.com/WesselKroos/youtube-ambilight, MIT): the frame is "projected"
//! outward, so a point left of the video takes the colour of the video's left edge,
//! above it the top edge, the corners blending diagonally; black letterbox bars are
//! skipped; the glow fades out on an eased curve and follows the video smoothly in time.
//!
//! Here it runs on the CPU over the player's [`Glimpse`] (the frame as a ~36×64 grid):
//! a grid of glow a little larger than the picture, blurred, uploaded as a small
//! image and stretched (and softened once more) by the GPU behind the video.

use std::sync::Arc;
use std::time::Instant;

use gpui::{RenderImage, Window};
use image::{Frame, RgbaImage};
use smallvec::smallvec;

use crate::player::Glimpse;

/// Cells along the picture's height in the glow grid.
const DOWN: usize = 48;
/// How far inside its edge the projection samples the picture (share of the half-size):
/// the outermost rim alone is often a compression artefact or a border line.
const DEPTH: f32 = 0.12;
/// The glow stays at full strength out to this share of its reach, then fades.
const FADE_START: f32 = 0.12;
/// Exponent of the fade: above 1 drops quickly near the video and trails off softly.
const FADE_CURVE: f32 = 1.7;
/// Time constant of the blend toward each new frame (s): no flicker, still follows cuts.
const SMOOTH: f32 = 0.12;
/// A row or column of the glimpse darker than this (mean luma) is a candidate bar...
const BAR_DARK: f32 = 0.06;
/// ...if no cell in it is brighter than this.
const BAR_PEAK: f32 = 0.14;
/// Bars never take more than this share of a side.
const BAR_MAX: f32 = 0.35;

/// Where the glow image goes, relative to the picture it surrounds: its origin and size
/// in multiples of the picture's size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reach {
    pub left: f32,
    pub top: f32,
    pub wide: f32,
    pub tall: f32,
    /// The picture's aspect (width / height), to fit it in the video column.
    pub aspect: f32,
}

pub struct Ambilight {
    /// Premultiplied RGBA per cell.
    glow: Vec<[f32; 4]>,
    /// Grid size, the picture's part of it and the margin around it, in cells.
    w: usize,
    h: usize,
    vw: usize,
    vh: usize,
    edge: usize,
    /// Letterbox crop (top, bottom, left, right), shares of the frame, eased.
    crop: [f32; 4],
    seen: u64,
    stepped: Instant,
    image: Option<Arc<RenderImage>>,
    /// The last frame's glimpse, kept while the glow fades out after it.
    last: Option<Arc<Glimpse>>,
}

impl Ambilight {
    pub fn new() -> Self {
        Ambilight {
            glow: Vec::new(),
            w: 0,
            h: 0,
            vw: 0,
            vh: 0,
            edge: 0,
            crop: [0.; 4],
            seen: 0,
            stepped: Instant::now(),
            image: None,
            last: None,
        }
    }

    /// Steps the glow toward `frame` (none: it fades out) and returns the image to draw
    /// and where. `spread`: reach of the glow beyond the picture, as a share of its height;
    /// `strength`: 0..1.
    pub fn update(
        &mut self,
        frame: Option<Arc<Glimpse>>,
        spread: f32,
        strength: f32,
        window: &mut Window,
    ) -> Option<(Arc<RenderImage>, Reach)> {
        let now = Instant::now();
        let dt = now.duration_since(self.stepped).as_secs_f32().min(0.25);
        let fresh = frame.as_ref().is_some_and(|f| f.id != self.seen);
        if let Some(f) = &frame {
            self.last = Some(f.clone());
        }
        let source = self.last.clone()?;
        let edge = ((DOWN as f32 * spread).round() as usize).max(2);
        let vw = ((DOWN * source.w) as f32 / source.h as f32).round().clamp(4., 4. * DOWN as f32) as usize;
        if (vw, DOWN, edge) != (self.vw, self.vh, self.edge) {
            // a new shape: start from dark, the blend fades it in
            (self.vw, self.vh, self.edge) = (vw, DOWN, edge);
            (self.w, self.h) = (vw + 2 * edge, DOWN + 2 * edge);
            self.glow = vec![[0.; 4]; self.w * self.h];
            self.crop = bars(&source);
        }
        let fading = frame.is_none();
        if !fresh && !fading {
            return self.image.clone().map(|image| (image, self.reach(&source)));
        }
        self.stepped = now;
        if let Some(f) = &frame {
            self.seen = f.id;
            let target = bars(f);
            for (crop, target) in self.crop.iter_mut().zip(target) {
                // bars come and go with scenes: ease the crop rather than jump
                *crop += (target - *crop) * 0.25;
            }
        }

        let k = 1. - (-dt / SMOOTH).exp();
        let lit = (strength * 1.6).clamp(0., 2.);
        let mut alive = false;
        for oy in 0..self.h {
            for ox in 0..self.w {
                let target = if fading { [0.; 4] } else { self.cell(&source, ox, oy, lit) };
                let cell = &mut self.glow[oy * self.w + ox];
                for c in 0..4 {
                    cell[c] += (target[c] - cell[c]) * k;
                }
                alive |= cell[3] > 0.004;
            }
        }
        if fading && !alive {
            self.last = None;
            self.drop_image(window);
            return None;
        }
        if fading {
            window.request_animation_frame();
        }
        let blurred = blur(&self.glow, self.w, self.h);
        let image = to_image(&blurred, self.w, self.h);
        self.drop_image(window);
        self.image = Some(image.clone());
        Some((image, self.reach(&source)))
    }

    fn drop_image(&mut self, window: &mut Window) {
        if let Some(old) = self.image.take() {
            let _ = window.drop_image(old);
        }
    }

    fn reach(&self, source: &Glimpse) -> Reach {
        Reach {
            left: -(self.edge as f32) / self.vw as f32,
            top: -(self.edge as f32) / self.vh as f32,
            wide: self.w as f32 / self.vw as f32,
            tall: self.h as f32 / self.vh as f32,
            aspect: source.w as f32 / source.h as f32,
        }
    }

    /// The glow a cell of the grid wants: the picture projected out to it, faded by distance.
    fn cell(&self, source: &Glimpse, ox: usize, oy: usize, lit: f32) -> [f32; 4] {
        let (vw, vh, edge) = (self.vw as f32, self.vh as f32, self.edge as f32);
        // centred on the picture, its edges at ±0.5
        let qx = (ox as f32 + 0.5 - edge) / vw - 0.5;
        let qy = (oy as f32 + 0.5 - edge) / vh - 0.5;
        // beyond the edge, in cells; the larger of the two, so the sides meet on the diagonals
        let out = ((qx.abs() - 0.5) * vw).max((qy.abs() - 0.5) * vh).max(0.) / edge;
        let fade = ((out - FADE_START) / (1. - FADE_START)).clamp(0., 1.);
        let alpha = (1. - fade).powf(FADE_CURVE);
        if alpha <= 0. {
            return [0.; 4];
        }
        // pulled in toward the centre until it lands just inside the edge it faces
        let reach = (qx.abs() * 2.).max(qy.abs() * 2.);
        let pull = if reach > 1. - DEPTH { (1. - DEPTH) / reach } else { 1. };
        let (px, py) = (qx * pull + 0.5, qy * pull + 0.5);
        // into the frame past its letterbox bars
        let [top, bottom, left, right] = self.crop;
        let u = left + px * (1. - left - right);
        let v = top + py * (1. - top - bottom);
        let [r, g, b] = vivid(sample(source, u, v), lit);
        [r * alpha, g * alpha, b * alpha, alpha]
    }
}

/// Bilinear sample of the glimpse at (u, v) in 0..1.
fn sample(g: &Glimpse, u: f32, v: f32) -> [f32; 3] {
    let x = (u * g.w as f32 - 0.5).clamp(0., (g.w - 1) as f32);
    let y = (v * g.h as f32 - 0.5).clamp(0., (g.h - 1) as f32);
    let (x0, y0) = (x as usize, y as usize);
    let (x1, y1) = ((x0 + 1).min(g.w - 1), (y0 + 1).min(g.h - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let at = |x: usize, y: usize| g.rgb[y * g.w + x];
    let mut out = [0.; 3];
    for (c, out) in out.iter_mut().enumerate() {
        let top = at(x0, y0)[c] + (at(x1, y0)[c] - at(x0, y0)[c]) * fx;
        let bottom = at(x0, y1)[c] + (at(x1, y1)[c] - at(x0, y1)[c]) * fx;
        *out = top + (bottom - top) * fy;
    }
    out
}

/// A touch more saturated, scaled by `lit`: light thrown on a wall reads paler than the
/// screen it comes from.
fn vivid([r, g, b]: [f32; 3], lit: f32) -> [f32; 3] {
    let grey = 0.299 * r + 0.587 * g + 0.114 * b;
    let push = |c: f32| ((grey + (c - grey) * 1.3) * lit).clamp(0., 1.);
    [push(r), push(g), push(b)]
}

/// Letterbox bars of a frame (top, bottom, left, right) as shares of it: runs of dark,
/// flat rows or columns from each side.
fn bars(g: &Glimpse) -> [f32; 4] {
    let luma = |[r, g, b]: [f32; 3]| 0.299 * r + 0.587 * g + 0.114 * b;
    let dark = |cells: &mut dyn Iterator<Item = [f32; 3]>| {
        let (mut sum, mut peak, mut n) = (0., 0f32, 0.);
        for c in cells {
            let l = luma(c);
            sum += l;
            peak = peak.max(l);
            n += 1.;
        }
        sum / n < BAR_DARK && peak < BAR_PEAK
    };
    let row = |y: usize| dark(&mut (0..g.w).map(|x| g.rgb[y * g.w + x]));
    let column = |x: usize| dark(&mut (0..g.h).map(|y| g.rgb[y * g.w + x]));
    let run = |n: usize, is_bar: &dyn Fn(usize) -> bool| {
        let limit = (n as f32 * BAR_MAX) as usize;
        let count = (0..limit).take_while(|&i| is_bar(i)).count();
        // an all-dark frame (a fade to black) is not letterboxed
        if count >= limit { 0. } else { count as f32 / n as f32 }
    };
    [
        run(g.h, &|i| row(i)),
        run(g.h, &|i| row(g.h - 1 - i)),
        run(g.w, &|i| column(i)),
        run(g.w, &|i| column(g.w - 1 - i)),
    ]
}

/// Two passes of a 5-wide box blur each way: smooth enough that the GPU's stretch shows
/// no cells.
fn blur(src: &[[f32; 4]], w: usize, h: usize) -> Vec<[f32; 4]> {
    let mut a = src.to_vec();
    let mut b = vec![[0.; 4]; a.len()];
    for _ in 0..2 {
        box_pass(&a, &mut b, w, h, true);
        box_pass(&b, &mut a, w, h, false);
    }
    a
}

fn box_pass(src: &[[f32; 4]], dst: &mut [[f32; 4]], w: usize, h: usize, horizontal: bool) {
    const R: isize = 2;
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0.; 4];
            for d in -R..=R {
                let (sx, sy) = if horizontal {
                    ((x as isize + d).clamp(0, w as isize - 1) as usize, y)
                } else {
                    (x, (y as isize + d).clamp(0, h as isize - 1) as usize)
                };
                let c = src[sy * w + sx];
                for i in 0..4 {
                    sum[i] += c[i];
                }
            }
            dst[y * w + x] = sum.map(|v| v / (2 * R + 1) as f32);
        }
    }
}

/// Premultiplied cells to the straight-alpha BGRA gpui samples, with a little noise in
/// the low bits so the wide dark fade doesn't band.
fn to_image(cells: &[[f32; 4]], w: usize, h: usize) -> Arc<RenderImage> {
    let mut bytes = Vec::with_capacity(w * h * 4);
    let mut seed = 0x9e37_79b9_u32;
    for c in cells {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let noise = (seed & 0xff) as f32 / 255. - 0.5;
        let a = c[3].clamp(0., 1.);
        let straight = |v: f32| if a > 1e-4 { (v / a).clamp(0., 1.) } else { 0. };
        let byte = |v: f32| (v * 255. + noise).round().clamp(0., 255.) as u8;
        bytes.extend_from_slice(&[byte(straight(c[2])), byte(straight(c[1])), byte(straight(c[0])), byte(a)]);
    }
    let buf = RgbaImage::from_raw(w as u32, h as u32, bytes).expect("glow buffer matches its size");
    Arc::new(RenderImage::new(smallvec![Frame::new(buf)]))
}
