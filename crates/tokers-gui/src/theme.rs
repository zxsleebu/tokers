//! Colours and sizes, after Sonora's dark theme (github.com/sonorahq/sonora,
//! `crates/ui/src/theme.rs`), tinted by the cover of the video on screen.

use std::f32::consts::TAU;

use gpui::{App, Global, Hsla, Pixels, Rgba, px, rgb, rgba};

const SURFACE_TINT: f32 = 0.5;
const BORDER_TINT: f32 = 0.4;
const TEXT_TINT: f32 = 0.12;
const MAX_WASH_SATURATION: f32 = 0.7;
const MIN_ACCENT_SATURATION: f32 = 0.6;
const MAX_ACCENT_SATURATION: f32 = 0.85;

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub background: Hsla,
    pub foreground: Hsla,
    pub border: Hsla,
    pub muted: Hsla,
    pub muted_foreground: Hsla,
    pub overlay: Hsla,
    pub secondary: Hsla,
    pub secondary_hover: Hsla,
    pub secondary_active: Hsla,
    pub primary: Hsla,
    pub primary_foreground: Hsla,
    pub primary_hover: Hsla,
    pub danger: Hsla,
    pub danger_foreground: Hsla,
    pub popover: Hsla,
    pub sidebar: Hsla,
    pub sidebar_accent: Hsla,
    pub sidebar_border: Hsla,
    pub title_bar_border: Hsla,
    /// The hue the theme wears (cover of the current video), if any.
    pub tint: Option<Hsla>,
    /// The cover's second colour, for the ambient background.
    pub tint_secondary: Option<Hsla>,
    pub radius: Pixels,
    pub font_size: Pixels,
}

impl Global for Theme {}

impl Theme {
    pub fn dark() -> Self {
        Theme {
            background: rgb(0x0a0a0a).into(),
            foreground: rgb(0xfafafa).into(),
            border: rgba(0x50505066).into(),
            muted: rgb(0x262626).into(),
            muted_foreground: rgb(0x909090).into(),
            overlay: rgba(0x0000008c).into(),
            secondary: rgb(0x171717).into(),
            secondary_hover: rgba(0x36363666).into(),
            secondary_active: rgba(0x4d4d4d66).into(),
            primary: rgb(0xfafafa).into(),
            primary_foreground: rgb(0x171717).into(),
            primary_hover: rgba(0xe5e5e5b3).into(),
            danger: rgb(0x7f1d1d).into(),
            danger_foreground: rgb(0xfef2f2).into(),
            popover: rgb(0x141414).into(),
            sidebar: rgb(0x0a0a0a).into(),
            sidebar_accent: rgba(0x50505066).into(),
            sidebar_border: rgb(0x262626).into(),
            title_bar_border: rgb(0x262626).into(),
            tint: None,
            tint_secondary: None,
            radius: px(6.),
            font_size: px(14.),
        }
    }

    /// The theme for a cover's palette (plain dark for greyscale art).
    pub fn for_palette(palette: Palette) -> Self {
        match palette.primary {
            Some(tint) => Theme { tint_secondary: palette.secondary, ..Theme::dark().tinted(tint) },
            None => Theme::dark(),
        }
    }

    /// The theme washed toward `tint`: surfaces pick up its hue, the accent becomes it.
    pub fn tinted(mut self, tint: Hsla) -> Self {
        for field in [
            &mut self.background,
            &mut self.secondary,
            &mut self.secondary_hover,
            &mut self.secondary_active,
            &mut self.muted,
            &mut self.popover,
            &mut self.sidebar,
            &mut self.sidebar_accent,
        ] {
            *field = wash(*field, tint, SURFACE_TINT);
        }
        for field in [&mut self.border, &mut self.sidebar_border, &mut self.title_bar_border] {
            *field = wash(*field, tint, BORDER_TINT);
        }
        for field in [&mut self.foreground, &mut self.muted_foreground] {
            *field = wash(*field, tint, TEXT_TINT);
        }
        let accent =
            |l| Hsla { h: tint.h, s: tint.s.clamp(MIN_ACCENT_SATURATION, MAX_ACCENT_SATURATION), l, a: 1. };
        self.primary = accent(0.72);
        self.primary_hover = Hsla { a: 0.7, ..accent(0.82) };
        self.primary_foreground = Hsla { s: tint.s.min(0.25), l: 0.08, ..self.primary };
        self.tint = Some(tint);
        self
    }

    /// Each colour of `self` moved `t` of the way toward `to` (theme crossfades).
    pub fn mix(&self, to: &Theme, t: f32) -> Theme {
        let m = |a: Hsla, b: Hsla| crate::motion::mix(a, b, t);
        Theme {
            background: m(self.background, to.background),
            foreground: m(self.foreground, to.foreground),
            border: m(self.border, to.border),
            muted: m(self.muted, to.muted),
            muted_foreground: m(self.muted_foreground, to.muted_foreground),
            overlay: m(self.overlay, to.overlay),
            secondary: m(self.secondary, to.secondary),
            secondary_hover: m(self.secondary_hover, to.secondary_hover),
            secondary_active: m(self.secondary_active, to.secondary_active),
            primary: m(self.primary, to.primary),
            primary_foreground: m(self.primary_foreground, to.primary_foreground),
            primary_hover: m(self.primary_hover, to.primary_hover),
            danger: m(self.danger, to.danger),
            danger_foreground: m(self.danger_foreground, to.danger_foreground),
            popover: m(self.popover, to.popover),
            sidebar: m(self.sidebar, to.sidebar),
            sidebar_accent: m(self.sidebar_accent, to.sidebar_accent),
            sidebar_border: m(self.sidebar_border, to.sidebar_border),
            title_bar_border: m(self.title_bar_border, to.title_bar_border),
            tint: to.tint,
            tint_secondary: to.tint_secondary,
            radius: to.radius,
            font_size: to.font_size,
        }
    }

    pub fn text(&self, size: Text) -> Pixels {
        self.font_size * size.ratio()
    }
}

fn wash(base: Hsla, tint: Hsla, strength: f32) -> Hsla {
    Hsla { h: tint.h, s: (base.s + tint.s * strength).min(MAX_WASH_SATURATION), l: base.l, a: base.a }
}

/// Sonora's type scale, as ratios of the base font size.
#[derive(Clone, Copy, Debug)]
pub enum Text {
    Tiny,
    Small,
    Label,
    Body,
    Large,
    Title,
}

impl Text {
    fn ratio(self) -> f32 {
        match self {
            Text::Tiny => 0.77,
            Text::Small => 0.85,
            Text::Label => 0.92,
            Text::Body => 1.,
            Text::Large => 1.38,
            Text::Title => 1.69,
        }
    }
}

pub trait ActiveTheme {
    fn theme(&self) -> &Theme;
}

impl ActiveTheme for App {
    fn theme(&self) -> &Theme {
        self.global::<Theme>()
    }
}

// ── cover palette (Sonora `crates/ui/src/palette.rs`) ──

const BINS: usize = 24;
const SAMPLES: usize = 6000;
const MIN_SATURATION: f32 = 0.14;
const MIN_LIGHTNESS: f32 = 0.10;
const MAX_LIGHTNESS: f32 = 0.94;
const MIN_SHARE: f32 = 0.01;
const MIN_WEIGHT: f32 = 25.;

#[derive(Clone, Copy, Default)]
struct Bin {
    weight: f32,
    x: f32,
    y: f32,
    saturation: f32,
    lightness: f32,
}

impl Bin {
    fn add(&mut self, color: Hsla, weight: f32) {
        let angle = color.h * TAU;
        self.weight += weight;
        self.x += angle.cos() * weight;
        self.y += angle.sin() * weight;
        self.saturation += color.s * weight;
        self.lightness += color.l * weight;
    }

    fn merge(&mut self, other: &Self) {
        self.weight += other.weight;
        self.x += other.x;
        self.y += other.y;
        self.saturation += other.saturation;
        self.lightness += other.lightness;
    }

    fn colour(&self) -> Option<Hsla> {
        (self.weight > 0.).then(|| Hsla {
            h: self.y.atan2(self.x).rem_euclid(TAU) / TAU,
            s: (self.saturation / self.weight).clamp(0., 1.),
            l: (self.lightness / self.weight).clamp(0., 1.),
            a: 1.,
        })
    }
}

const SECONDARY_SHARE: f32 = 0.02;
const SECONDARY_WEIGHT: f32 = 10.;
const SECONDARY_GAP: usize = 3;
const SECONDARY_HUE: f32 = 0.05;

/// What an image names: its leading hue and, when it has a real second colour
/// family, the runner-up (Sonora `palette.rs`). Both `None` for greyscale art.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Palette {
    pub primary: Option<Hsla>,
    pub secondary: Option<Hsla>,
}

/// The palette of BGRA pixels.
pub fn palette(bgra: &[u8]) -> Palette {
    let stride = (bgra.len() / 4 / SAMPLES).max(1);
    let mut bins = [Bin::default(); BINS];
    let mut sampled = 0.;
    for px in bgra.as_chunks::<4>().0.iter().step_by(stride) {
        sampled += 1.;
        let colour = Hsla::from(Rgba {
            r: px[2] as f32 / 255.,
            g: px[1] as f32 / 255.,
            b: px[0] as f32 / 255.,
            a: 1.,
        });
        if colour.s < MIN_SATURATION || colour.l < MIN_LIGHTNESS || colour.l > MAX_LIGHTNESS {
            continue;
        }
        let index = ((colour.h * BINS as f32) as usize).min(BINS - 1);
        // On a dark image the little light colour is the accent, not the black around it.
        bins[index].add(colour, colour.s * (0.3 + 0.7 * colour.l));
    }
    let score =
        |i: usize| bins[i].weight + (bins[(i + BINS - 1) % BINS].weight + bins[(i + 1) % BINS].weight) * 0.5;
    let cluster = |i: usize| {
        let mut c = bins[i];
        c.merge(&bins[(i + BINS - 1) % BINS]);
        c.merge(&bins[(i + 1) % BINS]);
        c.colour()
    };
    let Some(peak) = (0..BINS)
        .max_by(|&a, &b| score(a).total_cmp(&score(b)))
        .filter(|&p| score(p) >= (sampled * MIN_SHARE).max(MIN_WEIGHT))
    else {
        return Palette::default();
    };
    let primary = cluster(peak);
    let secondary = (0..BINS)
        .filter(|&i| {
            let gap = i.abs_diff(peak).min(BINS - i.abs_diff(peak));
            gap >= SECONDARY_GAP && score(i) >= (sampled * SECONDARY_SHARE).max(SECONDARY_WEIGHT)
        })
        .max_by(|&a, &b| score(a).total_cmp(&score(b)))
        .and_then(cluster)
        .filter(|c| primary.is_some_and(|p| (c.h - p.h).abs().min(1. - (c.h - p.h).abs()) >= SECONDARY_HUE));
    Palette { primary, secondary }
}
