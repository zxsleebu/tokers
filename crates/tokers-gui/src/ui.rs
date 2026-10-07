//! Small widgets, after Sonora's `crates/ui` (button, skeleton, window controls/frame).

use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    Animation, AnimationExt as _, AnyElement, App, ClickEvent, CursorStyle, Decorations, Div, ElementId,
    Hsla, Interactivity, MouseButton, Pixels, ResizeEdge, SharedString, Stateful, StyleRefinement, Svg,
    Window, WindowControlArea, div, ease_in_out, px, svg,
};

use crate::theme::{ActiveTheme as _, Text};

pub fn icon(path: impl Into<SharedString>) -> Svg {
    svg().path(path.into()).flex_none()
}

type Click = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Variant {
    Ghost,
    Outline,
    Secondary,
    Primary,
}

#[derive(IntoElement)]
pub struct Button {
    base: Stateful<Div>,
    label: Option<SharedString>,
    icon: Option<SharedString>,
    trailing: Option<SharedString>,
    variant: Variant,
    small: bool,
    selected: bool,
    tint: Option<Hsla>,
    on_click: Option<Click>,
}

impl Button {
    #[track_caller]
    pub fn new(id: impl Into<ElementId>) -> Self {
        Button {
            base: div().id(id),
            label: None,
            icon: None,
            trailing: None,
            variant: Variant::Ghost,
            small: false,
            selected: false,
            tint: None,
            on_click: None,
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn icon(mut self, path: impl Into<SharedString>) -> Self {
        self.icon = Some(path.into());
        self
    }

    /// An icon after the label (a picker's chevron).
    pub fn trailing(mut self, path: impl Into<SharedString>) -> Self {
        self.trailing = Some(path.into());
        self
    }

    /// No fill, a hairline border: a picker's face.
    pub fn outline(mut self) -> Self {
        self.variant = Variant::Outline;
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn secondary(mut self) -> Self {
        self.variant = Variant::Secondary;
        self
    }

    pub fn primary(mut self) -> Self {
        self.variant = Variant::Primary;
        self
    }

    pub fn small(mut self) -> Self {
        self.small = true;
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl Styled for Button {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for Button {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for Button {}

impl RenderOnce for Button {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Button { mut base, label, icon, trailing, variant, small, selected, tint, on_click } = self;
        let theme = *cx.theme();
        let (background, hover, active, foreground) = match variant {
            Variant::Ghost | Variant::Outline => {
                (None, theme.secondary_hover, theme.secondary_active, theme.foreground)
            }
            Variant::Secondary => {
                (Some(theme.secondary), theme.secondary_hover, theme.secondary_active, theme.foreground)
            }
            Variant::Primary => {
                (Some(theme.primary), theme.primary_hover, theme.primary_hover, theme.primary_foreground)
            }
        };
        let foreground = tint.unwrap_or(foreground);
        let (height, padding, gap) =
            if small { (px(26.), px(8.), px(4.)) } else { (px(32.), px(12.), px(6.)) };
        let overrides = std::mem::take(base.style());

        let mut button = base
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(gap)
            .h(height)
            .px(padding)
            .rounded(theme.radius)
            .text_color(foreground)
            .when(small, |this| this.text_size(theme.text(Text::Label)))
            .when_some(background, |this, bg| this.bg(bg))
            .when(matches!(variant, Variant::Secondary | Variant::Outline), |this| {
                this.border_1().border_color(theme.border)
            })
            .when(selected, |this| this.bg(theme.secondary_active))
            .cursor_pointer()
            .hover(move |style| style.bg(hover))
            .active(move |style| style.bg(active))
            .when_some(icon, |this, path| this.child(icon_el(path, px(16.), foreground)))
            .when_some(label, |this, label| {
                this.child(div().min_w_0().truncate().when(trailing.is_some(), |l| l.flex_1()).child(label))
            })
            .when_some(trailing, |this, path| this.child(icon_el(path, px(16.), foreground)))
            .when_some(on_click, |this, handler| {
                this.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(move |event, window, cx| handler(event, window, cx))
            });
        button.style().refine(&overrides);
        button
    }
}

fn icon_el(path: SharedString, size: Pixels, color: Hsla) -> Svg {
    icon(path).size(size).text_color(color)
}

/// A pulsing placeholder (Sonora `skeleton.rs`).
pub fn skeleton(
    id: impl Into<ElementId>,
    shape: impl FnOnce(Div) -> Div,
    cx: &App,
) -> gpui::AnimationElement<Div> {
    let theme = cx.theme();
    shape(div().bg(theme.muted).rounded(theme.radius)).with_animation(
        id,
        Animation::new(Duration::from_millis(1400)).repeat().with_easing(ease_in_out),
        |this, delta| {
            let fade = 1. - (delta * std::f32::consts::TAU).cos().abs() * 0.5;
            this.opacity(0.4 + fade * 0.3)
        },
    )
}

/// A round spinner (rotating arc).
pub fn spinner(id: impl Into<ElementId>, size: Pixels, color: Hsla) -> impl IntoElement {
    icon("icons/refresh-cw.svg").size(size).text_color(color).with_animation(
        id,
        Animation::new(Duration::from_millis(900)).repeat(),
        |this, delta| this.with_transformation(gpui::Transformation::rotate(gpui::percentage(delta))),
    )
}

/// Minimise / maximise / close, drawn by the app (client-side decorations).
pub fn window_controls(window: &Window, cx: &App) -> impl IntoElement {
    let theme = *cx.theme();
    let supported = window.window_controls();
    let maximized = window.is_maximized();
    let controls = [
        supported.minimize.then_some((
            "window-minimize",
            "icons/window-minimize.svg",
            WindowControlArea::Min,
        )),
        supported.maximize.then_some((
            "window-maximize",
            if maximized { "icons/window-restore.svg" } else { "icons/window-maximize.svg" },
            WindowControlArea::Max,
        )),
        Some(("window-close", "icons/window-close.svg", WindowControlArea::Close)),
    ];
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .children(controls.into_iter().flatten().map(move |(id, path, area)| {
            let danger = area == WindowControlArea::Close;
            div()
                .id(id)
                .group(id)
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(20.))
                .rounded(theme.radius)
                .cursor_pointer()
                .occlude()
                .window_control_area(area)
                .hover(move |s| s.bg(if danger { theme.danger } else { theme.secondary_active }))
                .child(
                    icon(path).size(px(16.)).text_color(theme.muted_foreground).group_hover(id, move |s| {
                        s.text_color(if danger { theme.danger_foreground } else { theme.foreground })
                    }),
                )
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    match area {
                        WindowControlArea::Min => window.minimize_window(),
                        WindowControlArea::Max => window.zoom_window(),
                        _ => window.remove_window(),
                    }
                })
        }))
}

/// Resize handles along the free edges of a client-decorated window.
pub fn window_frame(window: &Window) -> Option<AnyElement> {
    let tiling = match window.window_decorations() {
        Decorations::Client { tiling } if !window.is_maximized() => tiling,
        _ => return None,
    };
    let edges = [
        (!tiling.top, ResizeEdge::Top),
        (!tiling.bottom, ResizeEdge::Bottom),
        (!tiling.left, ResizeEdge::Left),
        (!tiling.right, ResizeEdge::Right),
        (!tiling.top && !tiling.left, ResizeEdge::TopLeft),
        (!tiling.top && !tiling.right, ResizeEdge::TopRight),
        (!tiling.bottom && !tiling.left, ResizeEdge::BottomLeft),
        (!tiling.bottom && !tiling.right, ResizeEdge::BottomRight),
    ];
    Some(
        div()
            .absolute()
            .inset_0()
            .children(edges.into_iter().filter(|(free, _)| *free).map(|(_, edge)| resize_handle(edge)))
            .into_any_element(),
    )
}

/// Corner radius of the window, when the app draws its own frame.
pub fn window_radius(window: &Window) -> Option<Pixels> {
    match window.window_decorations() {
        Decorations::Client { tiling } if !window.is_maximized() && !tiling.is_tiled() => Some(px(10.)),
        _ => None,
    }
}

fn resize_handle(edge: ResizeEdge) -> Div {
    const EDGE: Pixels = px(5.);
    const CORNER: Pixels = px(10.);
    let handle = div()
        .absolute()
        .cursor(match edge {
            ResizeEdge::Top | ResizeEdge::Bottom => CursorStyle::ResizeUpDown,
            ResizeEdge::Left | ResizeEdge::Right => CursorStyle::ResizeLeftRight,
            ResizeEdge::TopLeft | ResizeEdge::BottomRight => CursorStyle::ResizeUpLeftDownRight,
            ResizeEdge::TopRight | ResizeEdge::BottomLeft => CursorStyle::ResizeUpRightDownLeft,
        })
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            cx.stop_propagation();
            window.start_window_resize(edge);
        });
    match edge {
        ResizeEdge::Top => handle.top_0().left_0().right_0().h(EDGE),
        ResizeEdge::Bottom => handle.bottom_0().left_0().right_0().h(EDGE),
        ResizeEdge::Left => handle.top_0().bottom_0().left_0().w(EDGE),
        ResizeEdge::Right => handle.top_0().bottom_0().right_0().w(EDGE),
        ResizeEdge::TopLeft => handle.top_0().left_0().size(CORNER),
        ResizeEdge::TopRight => handle.top_0().right_0().size(CORNER),
        ResizeEdge::BottomLeft => handle.bottom_0().left_0().size(CORNER),
        ResizeEdge::BottomRight => handle.bottom_0().right_0().size(CORNER),
    }
}

/// `1234567` → `1.2M`.
pub fn compact(n: u64) -> String {
    match n {
        0..1_000 => n.to_string(),
        1_000..1_000_000 => trim(n as f64 / 1e3, "K"),
        1_000_000..1_000_000_000 => trim(n as f64 / 1e6, "M"),
        _ => trim(n as f64 / 1e9, "B"),
    }
}

fn trim(v: f64, unit: &str) -> String {
    if v >= 100. { format!("{v:.0}{unit}") } else { format!("{:.1}{unit}", v).replace(".0", "") }
}

#[cfg(test)]
mod tests {
    #[test]
    fn compact_numbers() {
        assert_eq!(super::compact(999), "999");
        assert_eq!(super::compact(1_000), "1K");
        assert_eq!(super::compact(1_250), "1.2K");
        assert_eq!(super::compact(128_400), "128K");
        assert_eq!(super::compact(3_400_000), "3.4M");
    }
}

// ── slider (after Sonora's scrubber) ──

/// What a slider drag carries, to tell its own drags from other sliders'.
#[derive(Clone)]
struct Grab(SharedString);

impl gpui::Render for Grab {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

type Slide = std::rc::Rc<dyn Fn(f32, &mut App)>;
type Done = std::rc::Rc<dyn Fn(&mut App)>;

/// Sonora's scrubber: a 4 px track, the part up to the value filled, a 12 px thumb inset so
/// it never overhangs the ends, and a 24 px tall strip to grab.
const TRACK: Pixels = px(4.);
const THUMB: Pixels = px(12.);
const GRIP: Pixels = px(24.);

/// A horizontal slider over 0..1: press anywhere on it or drag the thumb. `on_change` gets the
/// value as it moves; `on_release` runs when the button comes up (a place to save it).
pub fn slider(
    id: impl Into<SharedString>,
    value: f32,
    on_change: impl Fn(f32, &mut App) + 'static,
    on_release: impl Fn(&mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let id: SharedString = id.into();
    let value = value.clamp(0., 1.);
    let bounds = std::rc::Rc::new(std::cell::Cell::new(gpui::Bounds::<Pixels>::default()));
    let on_change: Slide = std::rc::Rc::new(on_change);
    let on_release: Done = std::rc::Rc::new(on_release);
    // the thumb's centre runs from one half-thumb in to the other
    let at = |x: Pixels, bounds: gpui::Bounds<Pixels>| {
        let travel = (bounds.size.width - THUMB).max(px(1.));
        ((x - bounds.origin.x - THUMB / 2.) / travel).clamp(0., 1.)
    };
    let (pressed, dragged) = (on_change.clone(), on_change);
    let (up, up_out) = (on_release.clone(), on_release);
    let (press_bounds, mine) = (bounds.clone(), id.clone());
    let inset = THUMB / 2.;
    div()
        .id(ElementId::Name(id.clone()))
        .relative()
        .h(GRIP)
        .w_full()
        .flex()
        .items_center()
        .cursor_pointer()
        .child(gpui::canvas(move |b, _, _| bounds.set(b), |_, _, _, _| {}).absolute().inset_0())
        .child(
            div().relative().w_full().h(TRACK).rounded_full().bg(theme.muted).child(
                div().absolute().top_0().bottom_0().left(inset).right(inset).child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(-inset)
                        .right(gpui::relative(1. - value))
                        .rounded_full()
                        .bg(theme.foreground),
                )
                .child(
                    div()
                        .absolute()
                        .top((TRACK - THUMB) / 2.)
                        .left(gpui::relative(value))
                        .ml(-THUMB / 2.)
                        .size(THUMB)
                        .rounded_full()
                        .bg(theme.foreground),
                ),
            ),
        )
        .on_mouse_down(MouseButton::Left, move |e, _, cx| {
            cx.stop_propagation();
            pressed(at(e.position.x, press_bounds.get()), cx);
        })
        .on_drag(Grab(id), |grab, _, _, cx| cx.new(|_| grab.clone()))
        .on_drag_move(move |e: &gpui::DragMoveEvent<Grab>, _, cx| {
            if e.drag(cx).0 == mine {
                dragged(at(e.event.position.x, e.bounds), cx);
            }
        })
        .on_mouse_up(MouseButton::Left, move |_, _, cx| up(cx))
        .on_mouse_up_out(MouseButton::Left, move |_, _, cx| up_out(cx))
}

// ── settings controls, after Sonora's `crates/ui` (switch, separator, label, tabs, menu) ──

/// How long a switch takes to turn (Sonora's `Motion::Control`).
const TURN: Duration = Duration::from_millis(110);
/// The switch is this share of a small control's height (26 px)...
const SWITCH_SCALE: f32 = 0.85;
/// ...this many times as wide as it is tall...
const SWITCH_WIDTH: f32 = 1.75;
/// ...with the knob this far inside its border.
const SWITCH_INSET: f32 = 2.;

/// Whether a switch was drawn on or off, and when it last turned: it animates only then.
struct Turning {
    drawn: bool,
    turned: Option<std::time::Instant>,
}

/// A sliding on/off switch.
#[derive(IntoElement)]
pub struct Switch {
    id: ElementId,
    checked: bool,
    on_click: Option<Click>,
}

impl Switch {
    pub fn new(id: impl Into<ElementId>, checked: bool) -> Self {
        Switch { id: id.into(), checked, on_click: None }
    }

    pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Switch {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Switch { id, checked, on_click } = self;
        let theme = *cx.theme();
        let height = px((26. * SWITCH_SCALE).round());
        let width = px((f32::from(height) * SWITCH_WIDTH).round());
        let thumb = height - px((SWITCH_INSET + 1.) * 2.);
        let travel = width - height;
        let (from, to) = if checked { (0., 1.) } else { (1., 0.) };
        let pick = |on: Hsla, off: Hsla| if checked { (off, on) } else { (on, off) };
        let (track_was, track_is) = pick(theme.primary, theme.muted);
        let (edge_was, edge_is) = pick(theme.primary, theme.border);
        let (hover_was, hover_is) = pick(theme.primary_hover, theme.secondary_hover);
        let (knob_was, knob_is) = pick(theme.primary_foreground, theme.muted_foreground);

        let turning = window
            .use_keyed_state((id.clone(), "turning"), cx, |_, _| Turning { drawn: checked, turned: None });
        let animates = turning.update(cx, |t, _| {
            if t.drawn != checked {
                t.drawn = checked;
                t.turned = Some(std::time::Instant::now());
            }
            t.turned.is_some_and(|at| at.elapsed() < TURN)
        }) && !cx.reduce_motion();
        let turn = || Animation::new(TURN).with_easing(ease_in_out);
        let knob = div().size(thumb).flex_none().rounded(thumb / 2.);

        let switch = div()
            .id(id)
            .flex()
            .flex_none()
            .items_center()
            .w(width)
            .h(height)
            .p(px(SWITCH_INSET))
            .rounded(height / 2.)
            .bg(track_is)
            .border_1()
            .border_color(edge_is)
            .cursor_pointer()
            .child(if animates {
                knob.with_animation(("thumb", usize::from(checked)), turn(), move |knob, t| {
                    knob.ml(travel * (from + (to - from) * t)).bg(crate::motion::mix(knob_was, knob_is, t))
                })
                .into_any_element()
            } else {
                knob.ml(travel * to).bg(knob_is).into_any_element()
            })
            .when_some(on_click, |this, handler| {
                this.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(move |event, window, cx| handler(event, window, cx))
            });
        if animates {
            switch
                .with_animation(("track", usize::from(checked)), turn(), move |track, t| {
                    let hover = crate::motion::mix(hover_was, hover_is, t);
                    track
                        .bg(crate::motion::mix(track_was, track_is, t))
                        .border_color(crate::motion::mix(edge_was, edge_is, t))
                        .hover(move |style| style.bg(hover))
                })
                .into_any_element()
        } else {
            switch.hover(move |style| style.bg(hover_is)).into_any_element()
        }
    }
}

/// A hairline across its parent.
pub fn separator(theme: &crate::theme::Theme) -> Div {
    div().flex_none().w_full().h(px(1.)).bg(theme.border)
}

/// A group's title: small, semibold, muted, upper case.
pub fn eyebrow(label: &str, theme: &crate::theme::Theme) -> Div {
    div()
        .flex_none()
        .text_size(theme.text(Text::Small))
        .text_color(theme.muted_foreground)
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .child(label.to_uppercase())
}

/// Segment buttons in one pill (Sonora's `TabBar`), floating over the page under it.
pub fn tab_bar(items: impl IntoIterator<Item = Button>, theme: &crate::theme::Theme) -> Div {
    div()
        .flex()
        .gap_1()
        .p_1()
        .rounded(theme.radius)
        .bg(theme.secondary)
        .border_1()
        .border_color(theme.border)
        .shadow_sm()
        .children(items.into_iter().map(|item| item.flex_shrink_0().rounded(theme.radius - px(2.))))
}

/// A dropdown's panel (Sonora's `Menu`).
pub fn menu_panel(width: Pixels, theme: &crate::theme::Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .p_1()
        .w(width)
        .rounded(theme.radius)
        .border_1()
        .border_color(theme.border)
        .bg(theme.popover)
        .shadow_md()
        .text_color(theme.foreground)
}

/// A row of a dropdown: its label, a tick on the chosen one.
pub fn menu_item(id: impl Into<ElementId>, label: impl Into<SharedString>, selected: bool, theme: &crate::theme::Theme) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .w_full()
        .min_w_0()
        .items_center()
        .justify_between()
        .gap_3()
        .px_3()
        .py_1()
        .rounded(theme.radius - px(2.))
        .cursor_pointer()
        .when(selected, |this| this.bg(theme.secondary_active))
        .hover(move |this| this.bg(theme.secondary_hover))
        .child(div().truncate().child(label.into()))
        .when(selected, |this| this.child(div().flex_none().child("✓")))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}
