//! Where everything goes for a given window size (docs/design/layouts3.svg).
//!
//! The video column is 9:16, never covered by chrome or cropped. The buttons are
//! either wholly beside the video or wholly on it, never half over its edge:
//!
//! * wide: video anchored left at full height, buttons beside it, comments panel;
//! * medium: the video with its buttons beside it, centred as one group;
//! * overlay, once that group no longer fits: the video centred alone, the buttons
//!   inside its right edge like the phone app; narrower than the column itself,
//!   the column scales down (bars above and below).
//!
//! Within a mode everything follows the window continuously. A change of mode is
//! one animated move (the view blends [`Layout::arrange`] for both), with a little
//! hysteresis at each threshold so a resize hovering on it doesn't flicker.
//! The buttons never move vertically: always bottom of the column.
//!
//! The sidebar folds into a titlebar menu once it would push the buttons onto the video.
//! Every threshold derives from the window height, since that sets the column width.

pub const TITLEBAR: f32 = 40.;
pub const SIDEBAR: f32 = 208.;
pub const ACTIONS: f32 = 88.;
pub const COMMENTS_MIN: f32 = 360.;
/// Share of the content height the comment sheet takes when open.
pub const SHEET: f32 = 0.58;
/// Once open, the panel stays until the window is this much narrower than its threshold.
pub const WIDE_HOLD: f32 = 24.;
/// Once beside, the buttons stay there until the group overflows; back from the
/// overlay they need this much room to spare.
pub const BESIDE_HOLD: f32 = 16.;
/// Space under the button stack.
const LIFT: f32 = 24.;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Rect { x, y, w, h }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Video, buttons beside it, comments panel.
    Wide,
    /// Buttons beside the video; comments open on request.
    Medium,
    /// Buttons over the video, like the phone app.
    Overlay,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub mode: Mode,
    /// `false`: the sidebar is folded into the titlebar menu button.
    pub sidebar: bool,
    /// Everything right of the sidebar and below the titlebar.
    pub content: Rect,
    /// The 9:16 column the video is fitted into.
    pub video: Rect,
    /// The column the action buttons are stacked in (bottom-aligned), `None` under the sheet.
    pub actions: Option<Rect>,
    /// Space under the button stack, from the bottom of the content.
    pub actions_lift: f32,
    /// 1 = buttons beside the video, 0 = on it; in between only while the view animates.
    pub beside: f32,
    pub comments_panel: Option<Rect>,
    pub comments_sheet: Option<Rect>,
}

impl Layout {
    /// `sidebar`: the user wants it (it still folds when the video would not fit).
    /// `sheet_open`: the comment sheet is up (ignored in wide mode, which always shows comments).
    /// `was`: the previous mode (thresholds hold a little toward it).
    pub fn compute(width: f32, height: f32, sidebar: bool, sheet_open: bool, was: Mode) -> Layout {
        let mode = Self::mode(width, height, sidebar, was);
        Self::arrange(width, height, sidebar, sheet_open, mode)
    }

    /// The arrangement this window gets (`was`: the one it had a moment ago).
    pub fn mode(width: f32, height: f32, sidebar: bool, was: Mode) -> Mode {
        let full_w = column_width((height - TITLEBAR).max(0.));
        let free =
            width - if shows_sidebar(width, full_w, sidebar) { SIDEBAR } else { 0. } - full_w - ACTIONS;
        let hold = |held: bool, margin: f32| if held { margin } else { 0. };
        if free >= COMMENTS_MIN - hold(was == Mode::Wide, WIDE_HOLD) {
            Mode::Wide
        } else if free >= hold(was == Mode::Overlay, BESIDE_HOLD) {
            Mode::Medium
        } else {
            Mode::Overlay
        }
    }

    /// The layout in a given mode, decided by the caller. Any mode can be arranged
    /// at any size, so the view can blend two of them.
    pub fn arrange(width: f32, height: f32, sidebar: bool, sheet_open: bool, mode: Mode) -> Layout {
        let content_h = (height - TITLEBAR).max(0.);
        let full_w = column_width(content_h);
        let sidebar = shows_sidebar(width, full_w, sidebar);
        let left = if sidebar { SIDEBAR } else { 0. };
        let content = Rect::new(left, TITLEBAR, (width - left).max(0.), content_h);
        let avail = content.w;

        if mode == Mode::Wide {
            let beside = left + full_w;
            return Layout {
                mode: Mode::Wide,
                sidebar,
                content,
                video: Rect::new(left, TITLEBAR, full_w, content_h),
                actions: Some(Rect::new(beside, TITLEBAR, ACTIONS, content_h)),
                actions_lift: LIFT,
                beside: 1.,
                comments_panel: Some(Rect::new(
                    beside + ACTIONS,
                    TITLEBAR,
                    width - beside - ACTIONS,
                    content_h,
                )),
                comments_sheet: None,
            };
        }

        let w = full_w.min(avail);
        let h = w * 16. / 9.;
        let (x, actions_x, beside) = if mode == Mode::Medium {
            // the video and its buttons, centred as a group (should the group not
            // fit while the view animates away from here, the buttons stop at the edge)
            let x = left + ((avail - ACTIONS - w) / 2.).max(0.);
            (x, (x + w).min(width - ACTIONS), 1.)
        } else {
            let x = left + (avail - w) / 2.;
            (x, x + w - ACTIONS, 0.)
        };

        if sheet_open {
            let sheet_h = (content_h * SHEET).round();
            let top_h = content_h - sheet_h;
            let w = column_width(top_h).min(avail);
            let h = w * 16. / 9.;
            return Layout {
                mode,
                sidebar,
                content,
                video: Rect::new(left + (avail - w) / 2., TITLEBAR + (top_h - h) / 2., w, h),
                actions: None,
                actions_lift: LIFT,
                beside,
                comments_panel: None,
                comments_sheet: Some(Rect::new(left, TITLEBAR + top_h, avail, sheet_h)),
            };
        }

        Layout {
            mode,
            sidebar,
            content,
            video: Rect::new(x, TITLEBAR + (content_h - h) / 2., w, h),
            actions: Some(Rect::new(actions_x, TITLEBAR, ACTIONS, content_h)),
            actions_lift: LIFT,
            beside,
            comments_panel: None,
            comments_sheet: None,
        }
    }

    /// Window width at which comments get their own panel, for this height.
    pub fn wide_width(height: f32, sidebar: bool) -> f32 {
        (if sidebar { SIDEBAR } else { 0. })
            + column_width((height - TITLEBAR).max(0.))
            + ACTIONS
            + COMMENTS_MIN
    }
}

/// The sidebar stays while the video and its buttons still fit beside it.
fn shows_sidebar(width: f32, full_w: f32, wanted: bool) -> bool {
    wanted && width - SIDEBAR >= full_w + ACTIONS
}

fn column_width(height: f32) -> f32 {
    (height * 9. / 16.).floor()
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: f32 = 720. + TITLEBAR;
    const COL: f32 = 405.; // 720 * 9/16

    fn at(width: f32) -> Layout {
        Layout::compute(width, H, false, false, Mode::Medium)
    }

    #[test]
    fn wide_has_the_panel_and_a_left_anchored_video() {
        let l = at(COL + ACTIONS + COMMENTS_MIN);
        assert_eq!(l.mode, Mode::Wide);
        assert_eq!(l.video, Rect::new(0., TITLEBAR, COL, 720.));
        assert_eq!(l.comments_panel.unwrap().w, COMMENTS_MIN);
    }

    #[test]
    fn medium_centres_the_video_with_its_buttons() {
        let l = at(COL + ACTIONS + 200.);
        assert_eq!(l.mode, Mode::Medium);
        assert_eq!(l.beside, 1.);
        assert_eq!(l.video.x, 100.);
        assert_eq!(l.actions.unwrap().x, 100. + COL);
        assert_eq!(l.video.h, 720.);
    }

    #[test]
    fn buttons_are_beside_or_inside_never_half_over() {
        for was in [Mode::Medium, Mode::Overlay] {
            let mut w = 320.;
            while w < 1400. {
                let l = Layout::compute(w, H, false, false, was);
                let (a, v) = (l.actions.unwrap(), l.video);
                let right = v.x + v.w;
                assert!(
                    (a.x - right).abs() < 0.01 || (a.x + a.w - right).abs() < 0.01,
                    "buttons half over the video at {w}: {a:?} {v:?}"
                );
                assert!(a.x + a.w <= w + 0.01, "buttons off the window at {w}");
                w += 1.;
            }
        }
    }

    #[test]
    fn overlay_centres_the_video_with_the_buttons_inside() {
        let l = at(COL + ACTIONS / 2.);
        assert_eq!(l.mode, Mode::Overlay);
        assert_eq!(l.beside, 0.);
        assert_eq!((l.video.x, l.video.w, l.video.h), (ACTIONS / 4., COL, 720.));
        assert_eq!(l.actions.unwrap().x, ACTIONS / 4. + COL - ACTIONS);
        // the buttons stay at the same height in every mode
        assert_eq!(l.actions_lift, at(320.).actions_lift);
        assert_eq!(l.actions_lift, at(1200.).actions_lift);
    }

    #[test]
    fn narrow_is_overlay_and_never_crops() {
        let l = at(320.);
        assert_eq!(l.mode, Mode::Overlay);
        assert_eq!(l.beside, 0.);
        assert_eq!(l.video.w, 320.);
        assert!((l.video.h - 320. * 16. / 9.).abs() < 0.01);
    }

    #[test]
    fn nothing_jumps_within_a_mode() {
        // a change of mode is animated by the view
        for mode in [Mode::Wide, Mode::Medium, Mode::Overlay] {
            let mut prev = Layout::arrange(300., H, false, false, mode);
            let mut w = 301.;
            while w < 1400. {
                let l = Layout::arrange(w, H, false, false, mode);
                let jump = |a: Rect, b: Rect| (a.x - b.x).abs().max((a.w - b.w).abs()).max((a.y - b.y).abs());
                assert!(
                    jump(prev.video, l.video) <= 3.5,
                    "video jumps at {w}: {:?} -> {:?}",
                    prev.video,
                    l.video
                );
                let (pa, la) = (prev.actions.unwrap(), l.actions.unwrap());
                assert!(jump(pa, la) <= 3.5, "buttons jump at {w}: {pa:?} -> {la:?}");
                assert!((prev.actions_lift - l.actions_lift).abs() <= 3.5, "lift jumps at {w}");
                assert!(la.x >= l.video.x + l.video.w - ACTIONS - 0.01, "buttons detached at {w}");
                if mode != Mode::Wide {
                    // the buttons are never left floating away from the video
                    assert!(la.x <= l.video.x + l.video.w + 0.01, "buttons float at {w}");
                }
                prev = l;
                w += 1.;
            }
        }
    }

    #[test]
    fn thresholds_hold_toward_the_previous_mode() {
        let wide = COL + ACTIONS + COMMENTS_MIN;
        assert_eq!(Layout::mode(wide - 10., H, false, Mode::Wide), Mode::Wide);
        assert_eq!(Layout::mode(wide - 10., H, false, Mode::Medium), Mode::Medium);
        let beside = COL + ACTIONS;
        assert_eq!(Layout::mode(beside + 5., H, false, Mode::Medium), Mode::Medium);
        assert_eq!(Layout::mode(beside + 5., H, false, Mode::Overlay), Mode::Overlay);
        assert_eq!(Layout::mode(beside - 1., H, false, Mode::Medium), Mode::Overlay);
    }

    #[test]
    fn sheet_shrinks_the_video_above_it() {
        let l = Layout::compute(560., H, true, true, Mode::Medium);
        let sheet = l.comments_sheet.unwrap();
        assert!(l.video.y + l.video.h <= sheet.y + 0.01);
        assert!(l.actions.is_none());
        assert!(Layout::compute(1400., H, true, true, Mode::Medium).comments_sheet.is_none());
    }

    #[test]
    fn sidebar_folds_before_the_buttons_go_onto_the_video() {
        let fits = SIDEBAR + COL + ACTIONS;
        assert!(Layout::compute(fits, H, true, false, Mode::Medium).sidebar);
        let folded = Layout::compute(fits - 1., H, true, false, Mode::Medium);
        assert!(!folded.sidebar);
        assert_eq!(folded.mode, Mode::Medium);
    }
}
