//! tokers UI. The API backend (request template + signers) is injected by the
//! binary that links it: see [`run`].

mod feed;

use gpui::{App, AppContext, Bounds, WindowBounds, WindowOptions, px, size};
use tokers::Backend;

use crate::feed::FeedView;

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

    gpui_platform::application().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(480.), px(820.)), cx);
        cx.open_window(
            WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() },
            |_, cx| cx.new(|cx| FeedView::new(tiktok, io, cx)),
        )
        .expect("open window");
        cx.on_window_closed(|cx, _| cx.quit()).detach();
        cx.activate(true);
    });
    drop(runtime);
}
