//! Video playback: a GStreamer `playbin3` per clip, decoded frames handed to
//! gpui as BGRA [`RenderImage`]s.
//!
//! Frames arrive on a GStreamer streaming thread, already paced to the pipeline
//! clock (the appsink syncs), so the UI just shows whatever frame is newest.
//! Loops are gapless: playback runs as a segment and every `SegmentDone` seeks
//! back to 0 without a flush.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Once};
use std::time::Duration;

use futures::StreamExt;
use futures::channel::mpsc;
use gpui::{
    App, Bounds, Context, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, ObjectFit, ParentElement,
    Pixels, Render, RenderImage, Styled, Window, canvas, div, img, prelude::*, px,
};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_video as gst_video;
use gstreamer_video::prelude::*;
use smallvec::smallvec;

use crate::motion::{Motion, Motioned as _, Rising as _};
use crate::theme::ActiveTheme as _;
use crate::ui::icon;

static INIT: Once = Once::new();
/// Video repaints are capped at 60 per second.
const FRAME: Duration = Duration::from_micros(16_667);

fn init() {
    INIT.call_once(|| {
        if let Err(e) = gst::init() {
            eprintln!("gstreamer init failed: {e}");
        }
    });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Loading,
    Playing,
    Paused,
    Failed,
}

pub struct VideoPlayer {
    pipeline: Option<gst::Element>,
    latest: Arc<Mutex<Option<Arc<RenderImage>>>>,
    /// Frame painted last; dropped from the atlas once a newer one replaces it.
    shown: Option<Arc<RenderImage>>,
    status: Status,
    /// Whether the user wants it playing (it may still be prerolling).
    want_play: bool,
    /// Muted regardless of the user's setting (a clip sliding out of view).
    silent: bool,
    muted: bool,
    duration: Option<Duration>,
    error: Option<String>,
    segment_started: bool,
    /// Where the seek bar was laid out, for turning a click into a position.
    bar: Rc<Cell<Option<Bounds<Pixels>>>>,
    scrubbing: bool,
    /// Bumped on every user play/pause, so the glyph animation restarts.
    toggles: usize,
    /// Show the seek bar and pause glyph (off for audio-only photo posts).
    pub chrome: bool,
}

impl VideoPlayer {
    /// `uri`: an `https://` stream or a `file://` path.
    /// `max_height`: physical pixels the picture is shown at; frames are scaled to it
    /// in the pipeline, so conversion, copies and uploads cost what is on screen.
    pub fn new(
        uri: &str,
        max_height: u32,
        play: bool,
        volume: f64,
        muted: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        init();
        let mut this = VideoPlayer {
            pipeline: None,
            latest: Arc::default(),
            shown: None,
            status: Status::Loading,
            want_play: play,
            silent: false,
            muted,
            duration: None,
            error: None,
            segment_started: false,
            bar: Rc::default(),
            scrubbing: false,
            toggles: 0,
            chrome: true,
        };
        if let Err(e) = this.build(uri, max_height, volume, muted, cx) {
            this.fail(e.to_string());
        }
        this
    }

    fn build(
        &mut self,
        uri: &str,
        max_height: u32,
        volume: f64,
        muted: bool,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        let sink = gst_app::AppSink::builder().max_buffers(2).drop(true).build();
        // convert + downscale (aspect kept: only the height is bounded) on two threads
        let convert = gst::ElementFactory::make("videoconvertscale").property("n-threads", 2u32).build()?;
        let caps = gst::Caps::builder("video/x-raw")
            .field("format", "BGRA")
            .field("height", gst::IntRange::new(2, max_height.max(2) as i32))
            .field("pixel-aspect-ratio", gst::Fraction::new(1, 1))
            .build();
        let filter = gst::ElementFactory::make("capsfilter").property("caps", &caps).build()?;
        let bin = gst::Bin::new();
        bin.add_many([&convert, &filter, sink.upcast_ref()])?;
        gst::Element::link_many([&convert, &filter, sink.upcast_ref()])?;
        let pad = convert.static_pad("sink").ok_or_else(|| anyhow::anyhow!("no sink pad"))?;
        bin.add_pad(&gst::GhostPad::with_target(&pad)?)?;

        let (tx, mut rx) = mpsc::unbounded::<()>();
        let pending = Arc::new(AtomicBool::new(false));
        let latest = self.latest.clone();
        let pending_cb = pending.clone();
        let deliver = Arc::new(move |sample: &gst::Sample| {
            if let Some(image) = to_render_image(sample) {
                *latest.lock().unwrap() = Some(image);
                if !pending_cb.swap(true, Ordering::AcqRel) {
                    let _ = tx.unbounded_send(());
                }
            }
        });
        let deliver_preroll = deliver.clone();
        sink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                    deliver(&sample);
                    Ok(gst::FlowSuccess::Ok)
                })
                // a paused (prerolled) clip shows its first frame, not the cover
                .new_preroll(move |sink| {
                    let sample = sink.pull_preroll().map_err(|_| gst::FlowError::Eos)?;
                    deliver_preroll(&sample);
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );

        let pipeline = gst::ElementFactory::make("playbin3")
            .property("uri", uri)
            .property("video-sink", &bin)
            .property("volume", volume)
            .property("mute", muted)
            .build()?;
        let bus = pipeline.bus().ok_or_else(|| anyhow::anyhow!("pipeline has no bus"))?;
        pipeline.set_state(gst::State::Paused)?;
        self.pipeline = Some(pipeline);

        // new frame -> repaint, at most one wakeup in flight
        cx.spawn(async move |this, cx| {
            // at most one repaint per FRAME: a 60+ fps clip doesn't drive the window faster
            while rx.next().await.is_some() {
                let shown = std::time::Instant::now();
                pending.store(false, Ordering::Release);
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
                let rest = FRAME.saturating_sub(shown.elapsed());
                if !rest.is_zero() {
                    cx.background_executor().timer(rest).await;
                }
            }
        })
        .detach();

        let mut messages = bus.stream();
        cx.spawn(async move |this, cx| {
            while let Some(msg) = messages.next().await {
                let alive = this.update(cx, |this, cx| this.on_message(&msg, cx)).is_ok();
                if !alive {
                    break;
                }
            }
        })
        .detach();
        Ok(())
    }

    fn on_message(&mut self, msg: &gst::Message, cx: &mut Context<Self>) {
        use gst::MessageView;
        let Some(pipeline) = self.pipeline.clone() else { return };
        match msg.view() {
            MessageView::AsyncDone(_) if !self.segment_started => {
                // Prerolled: switch to segment playback so the loop has no gap.
                self.segment_started = true;
                let _ = pipeline
                    .seek_simple(gst::SeekFlags::FLUSH | gst::SeekFlags::SEGMENT, gst::ClockTime::ZERO);
                self.duration = pipeline.query_duration::<gst::ClockTime>().map(|d| d.into());
                self.apply_state();
                cx.notify();
            }
            MessageView::SegmentDone(_) => {
                let _ = pipeline.seek_simple(gst::SeekFlags::SEGMENT, gst::ClockTime::ZERO);
            }
            MessageView::Eos(_) => {
                let _ = pipeline
                    .seek_simple(gst::SeekFlags::FLUSH | gst::SeekFlags::SEGMENT, gst::ClockTime::ZERO);
            }
            MessageView::DurationChanged(_) => {
                self.duration = pipeline.query_duration::<gst::ClockTime>().map(|d| d.into());
                cx.notify();
            }
            MessageView::Error(e) => {
                self.fail(format!("{}", e.error()));
                cx.notify();
            }
            _ => {}
        }
    }

    fn fail(&mut self, error: String) {
        self.status = Status::Failed;
        self.error = Some(error);
        if let Some(p) = self.pipeline.take() {
            let _ = p.set_state(gst::State::Null);
        }
    }

    fn apply_state(&mut self) {
        let Some(p) = &self.pipeline else { return };
        if !self.segment_started {
            return;
        }
        let target = if self.want_play { gst::State::Playing } else { gst::State::Paused };
        let _ = p.set_state(target);
        self.status = if self.want_play { Status::Playing } else { Status::Paused };
    }

    pub fn set_playing(&mut self, play: bool, cx: &mut Context<Self>) {
        if self.want_play != play {
            self.want_play = play;
            self.apply_state();
            cx.notify();
        }
    }

    /// User play/pause: also flashes the glyph.
    pub fn toggle(&mut self, cx: &mut Context<Self>) {
        self.toggles += 1;
        self.set_playing(!self.want_play, cx);
    }

    pub fn is_playing(&self) -> bool {
        self.want_play
    }

    pub fn has_frame(&self) -> bool {
        self.shown.is_some() || self.latest.lock().unwrap().is_some()
    }

    pub fn set_volume(&mut self, volume: f64, muted: bool) {
        self.muted = muted;
        if let Some(p) = &self.pipeline {
            p.set_property("volume", volume.clamp(0., 1.));
            p.set_property("mute", muted || self.silent);
        }
    }

    /// Mute without touching the user's setting.
    pub fn set_silent(&mut self, silent: bool) {
        self.silent = silent;
        if let Some(p) = &self.pipeline {
            p.set_property("mute", self.muted || silent);
        }
    }

    pub fn position(&self) -> Option<Duration> {
        self.pipeline.as_ref()?.query_position::<gst::ClockTime>().map(Into::into)
    }

    /// 0..1 through the clip.
    pub fn progress(&self) -> f32 {
        match (self.position(), self.duration) {
            (Some(p), Some(d)) if !d.is_zero() => (p.as_secs_f32() / d.as_secs_f32()).clamp(0., 1.),
            _ => 0.,
        }
    }

    pub fn seek(&mut self, fraction: f32) {
        let (Some(p), Some(d)) = (&self.pipeline, self.duration) else { return };
        let at = gst::ClockTime::from_nseconds((d.as_nanos() as f64 * fraction.clamp(0., 1.) as f64) as u64);
        let _ = p.seek_simple(gst::SeekFlags::FLUSH | gst::SeekFlags::SEGMENT | gst::SeekFlags::ACCURATE, at);
    }

    pub fn seek_by(&mut self, seconds: f64) {
        let (Some(pos), Some(d)) = (self.position(), self.duration) else { return };
        if d.is_zero() {
            return;
        }
        let target = (pos.as_secs_f64() + seconds).clamp(0., d.as_secs_f64());
        self.seek((target / d.as_secs_f64()) as f32);
    }

    /// Stop decoding and free the frames held in the atlas.
    pub fn shutdown(&mut self, window: Option<&mut Window>, cx: &mut App) {
        if let Some(p) = self.pipeline.take() {
            let _ = p.set_state(gst::State::Null);
        }
        if let Some(frame) = self.shown.take() {
            cx.drop_image(frame, window);
        }
        self.latest.lock().unwrap().take();
    }
}

impl Drop for VideoPlayer {
    fn drop(&mut self) {
        if let Some(p) = self.pipeline.take() {
            let _ = p.set_state(gst::State::Null);
        }
    }
}

impl Render for VideoPlayer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let latest = self.latest.lock().unwrap().clone();
        if let Some(latest) = latest {
            let changed = self.shown.as_ref().is_none_or(|s| s.id != latest.id);
            if changed && let Some(old) = self.shown.replace(latest) {
                let _ = window.drop_image(old);
            }
        }
        let theme = *cx.theme();
        let playing = self.want_play;
        let progress = self.progress();
        let bar = self.bar.clone();
        let toggles = self.toggles;

        div()
            .size_full()
            .relative()
            .when_some(self.shown.clone(), |el, frame| {
                el.child(img(frame).absolute().inset_0().size_full().object_fit(ObjectFit::Contain).motion(
                    "frame-in",
                    Motion::Slow,
                    |el, t| el.opacity(t),
                ))
            })
            .when(self.chrome && !playing && toggles > 0, |el| {
                el.child(
                    div().absolute().inset_0().flex().items_center().justify_center().child(
                        div()
                            .size(px(72.))
                            .rounded_full()
                            .bg(theme.overlay)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("icons/play-filled.svg").size(px(30.)).text_color(gpui::white()))
                            .rising(("paused", toggles)),
                    ),
                )
            })
            .when(self.chrome && self.status != Status::Failed, |el| {
                el.child(
                    // seek bar: a hairline that thickens under the pointer
                    div()
                        .id("seek")
                        .group("seek")
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .h(px(14.))
                        .flex()
                        .items_end()
                        .cursor_pointer()
                        .child(
                            div()
                                .relative()
                                .w_full()
                                .h(px(3.))
                                .group_hover("seek", |s| s.h(px(6.)))
                                .bg(gpui::white().opacity(0.22))
                                .child(
                                    div()
                                        .absolute()
                                        .left_0()
                                        .top_0()
                                        .bottom_0()
                                        .w(gpui::relative(progress))
                                        .bg(theme.primary),
                                )
                                .child(
                                    canvas(move |bounds, _, _| bar.set(Some(bounds)), |_, _, _, _| {})
                                        .absolute()
                                        .inset_0(),
                                ),
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, e: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                this.scrubbing = true;
                                this.seek_to_x(e.position.x);
                                cx.notify();
                            }),
                        )
                        .on_click(|_, _, cx| cx.stop_propagation()),
                )
            })
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                if this.scrubbing {
                    if e.pressed_button == Some(MouseButton::Left) {
                        this.seek_to_x(e.position.x);
                        cx.notify();
                    } else {
                        this.scrubbing = false;
                    }
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, _| this.scrubbing = false))
    }
}

impl VideoPlayer {
    fn seek_to_x(&mut self, x: Pixels) {
        if let Some(b) = self.bar.get() {
            let f = ((x - b.origin.x) / b.size.width).clamp(0., 1.);
            self.seek(f);
        }
    }
}

fn to_render_image(sample: &gst::Sample) -> Option<Arc<RenderImage>> {
    let caps = sample.caps()?;
    let info = gst_video::VideoInfo::from_caps(caps).ok()?;
    let buffer = sample.buffer()?;
    let frame = gst_video::VideoFrameRef::from_buffer_ref_readable(buffer, &info).ok()?;
    let (w, h) = (frame.width() as usize, frame.height() as usize);
    let stride = frame.plane_stride()[0] as usize;
    let data = frame.plane_data(0).ok()?;
    let mut bgra = Vec::with_capacity(w * h * 4);
    for row in 0..h {
        bgra.extend_from_slice(&data[row * stride..row * stride + w * 4]);
    }
    // gpui keeps image data as BGRA; the buffer type just says "4 bytes per pixel".
    let buf = image::RgbaImage::from_raw(w as u32, h as u32, bgra)?;
    Some(Arc::new(RenderImage::new(smallvec![image::Frame::new(buf)])))
}
