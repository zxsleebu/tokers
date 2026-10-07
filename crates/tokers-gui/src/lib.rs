//! tokers UI. The API backend (request template + signers) is injected by the
//! binary that links it: see [`run`].

mod ambient;
mod assets;
mod comments;
mod feed;
mod glide;
mod layout;
mod media;
mod motion;
mod player;
mod root;
mod scrollbar;
mod state;
mod theme;
mod ui;

use gpui::{
    App, AppContext, Bounds, KeyBinding, TitlebarOptions, WindowBackgroundAppearance, WindowBounds,
    WindowDecorations, WindowOptions, actions, px, size,
};
use tokers::Backend;

use crate::assets::Assets;
use crate::media::Images;
use crate::root::Root;
use crate::state::Store;
use crate::theme::Theme;

actions!(
    tokers,
    [
        Next,
        Previous,
        TogglePlay,
        SeekBack,
        SeekForward,
        ToggleLike,
        ToggleComments,
        ToggleFavourite,
        ToggleMute,
        ToggleSidebar,
        CopyLink,
        Download,
        Close,
    ]
);

/// Open the main window and run until it is closed.
pub fn run(backend: Backend) {
    // The API client is tokio-based; gpui has its own executor. Requests run on
    // this runtime, and their JoinHandles are awaited from gpui tasks.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("tokers-io")
        .enable_all()
        .build()
        .expect("tokio runtime");
    let io = runtime.handle().clone();
    let tiktok = backend.tiktok().build();

    gpui_platform::application().with_assets(Assets).run(move |cx: &mut App| {
        if let Err(e) = Assets::load_fonts(cx) {
            eprintln!("tokers: fonts: {e}");
        }
        cx.set_global(Theme::dark());
        Store::init(cx);
        Images::init(tiktok.clone(), io.clone(), cx);
        bind_keys(cx);

        let bounds = Bounds::centered(None, size(px(1180.), px(820.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("tokers".into()),
                    appears_transparent: true,
                    ..Default::default()
                }),
                window_background: WindowBackgroundAppearance::Transparent,
                window_decorations: Some(WindowDecorations::Client),
                window_min_size: Some(size(px(320.), px(480.))),
                app_id: Some("tokers".into()),
                // full frame rate in the background too: paging and video stay smooth
                inactive_frame_interval: None,
                ..Default::default()
            },
            |window, cx| {
                window.set_rem_size(cx.global::<Theme>().font_size);
                cx.new(|cx| Root::new(tiktok, io, window, cx))
            },
        )
        .expect("open window");
        cx.on_window_closed(|cx, _| cx.quit()).detach();
        cx.activate(true);
    });
    drop(runtime);
}

fn bind_keys(cx: &mut App) {
    let ctx = Some("Tokers");
    cx.bind_keys([
        KeyBinding::new("j", Next, ctx),
        KeyBinding::new("down", Next, ctx),
        KeyBinding::new("pagedown", Next, ctx),
        KeyBinding::new("k", Previous, ctx),
        KeyBinding::new("up", Previous, ctx),
        KeyBinding::new("pageup", Previous, ctx),
        KeyBinding::new("space", TogglePlay, ctx),
        KeyBinding::new("left", SeekBack, ctx),
        KeyBinding::new("right", SeekForward, ctx),
        KeyBinding::new("l", ToggleLike, ctx),
        KeyBinding::new("c", ToggleComments, ctx),
        KeyBinding::new("s", ToggleFavourite, ctx),
        KeyBinding::new("m", ToggleMute, ctx),
        KeyBinding::new("y", CopyLink, ctx),
        KeyBinding::new("d", Download, ctx),
        KeyBinding::new("ctrl-b", ToggleSidebar, ctx),
        KeyBinding::new("escape", Close, ctx),
    ]);
}
