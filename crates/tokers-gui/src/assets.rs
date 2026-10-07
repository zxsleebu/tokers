//! Embedded fonts (Inter, OFL) and icons (Lucide, ISC; window controls from Sonora, CC0).

use std::borrow::Cow;

use gpui::{App, AssetSource, Result, SharedString};

macro_rules! embed {
    ($($path:literal),+ $(,)?) => {
        &[$(($path, include_bytes!(concat!("../assets/", $path)) as &[u8])),+]
    };
}

const FONTS: &[(&str, &[u8])] = embed![
    "fonts/Inter-Regular.ttf",
    "fonts/Inter-Medium.ttf",
    "fonts/Inter-SemiBold.ttf",
    "fonts/Inter-Bold.ttf",
];

const ICONS: &[(&str, &[u8])] = embed![
    "icons/bookmark.svg",
    "icons/bookmark-filled.svg",
    "icons/check.svg",
    "icons/chevron-down.svg",
    "icons/chevron-up.svg",
    "icons/circle-alert.svg",
    "icons/clapperboard.svg",
    "icons/compass.svg",
    "icons/disc-3.svg",
    "icons/download.svg",
    "icons/forward.svg",
    "icons/heart.svg",
    "icons/heart-filled.svg",
    "icons/house.svg",
    "icons/link.svg",
    "icons/menu.svg",
    "icons/message-circle.svg",
    "icons/message-circle-filled.svg",
    "icons/music-2.svg",
    "icons/panel-left-close.svg",
    "icons/panel-left-open.svg",
    "icons/pause.svg",
    "icons/pause-filled.svg",
    "icons/play.svg",
    "icons/play-filled.svg",
    "icons/plus.svg",
    "icons/refresh-cw.svg",
    "icons/search.svg",
    "icons/settings.svg",
    "icons/share-2.svg",
    "icons/user-round.svg",
    "icons/users.svg",
    "icons/volume-2.svg",
    "icons/volume-x.svg",
    "icons/window-close.svg",
    "icons/window-maximize.svg",
    "icons/window-minimize.svg",
    "icons/window-restore.svg",
    "icons/x.svg",
];

pub struct Assets;

impl Assets {
    pub fn load_fonts(cx: &App) -> Result<()> {
        cx.text_system().add_fonts(FONTS.iter().map(|(_, bytes)| Cow::Borrowed(*bytes)).collect())
    }
}

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(ICONS.iter().chain(FONTS).find(|(name, _)| *name == path).map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(ICONS
            .iter()
            .chain(FONTS)
            .filter(|(name, _)| name.starts_with(path))
            .map(|(n, _)| (*n).into())
            .collect())
    }
}
