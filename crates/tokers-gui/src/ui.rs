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
    Secondary,
    Primary,
}

#[derive(IntoElement)]
pub struct Button {
    base: Stateful<Div>,
    label: Option<SharedString>,
    icon: Option<SharedString>,
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
        let Button { mut base, label, icon, variant, small, selected, tint, on_click } = self;
        let theme = *cx.theme();
        let (background, hover, active, foreground) = match variant {
            Variant::Ghost => (None, theme.secondary_hover, theme.secondary_active, theme.foreground),
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
            .when(variant == Variant::Secondary, |this| this.border_1().border_color(theme.border))
            .when(selected, |this| this.bg(theme.secondary_active))
            .cursor_pointer()
            .hover(move |style| style.bg(hover))
            .active(move |style| style.bg(active))
            .when_some(icon, |this, path| this.child(icon_el(path, px(16.), foreground)))
            .when_some(label, |this, label| this.child(div().min_w_0().truncate().child(label)))
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
