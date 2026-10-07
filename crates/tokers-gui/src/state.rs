//! What survives a restart: preferences (`$XDG_CONFIG_HOME/tokers/settings.json`)
//! and the local library — likes and favourites (`$XDG_DATA_HOME/tokers/library.json`).
//! There is no account, so both live on this machine only.

use std::collections::HashSet;
use std::path::PathBuf;

use gpui::{App, AppContext, Context, Entity, Global};
use serde::{Deserialize, Serialize};
use tokers::models::Aweme;

/// How comments open when the window is too narrow for the side panel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommentsMode {
    /// A sheet slides up from the bottom; the video shrinks above it.
    #[default]
    Sheet,
    /// The window grows to the right until the panel fits, and shrinks back on close.
    Expand,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub comments_mode: CommentsMode,
    pub volume: f32,
    pub muted: bool,
    pub sidebar: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { comments_mode: CommentsMode::Sheet, volume: 0.8, muted: false, sidebar: true }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Library {
    pub liked: HashSet<String>,
    /// Newest first.
    pub favourites: Vec<Aweme>,
}

pub struct Store {
    pub prefs: Prefs,
    pub library: Library,
}

struct StoreGlobal(Entity<Store>);

impl Global for StoreGlobal {}

impl Store {
    pub fn init(cx: &mut App) {
        let store = cx.new(|_| Store { prefs: load(&prefs_path()), library: load(&library_path()) });
        cx.set_global(StoreGlobal(store));
    }

    pub fn entity(cx: &App) -> Entity<Store> {
        cx.global::<StoreGlobal>().0.clone()
    }

    pub fn prefs(cx: &App) -> &Prefs {
        &Self::entity(cx).read(cx).prefs
    }

    pub fn update_prefs(cx: &mut App, f: impl FnOnce(&mut Prefs)) {
        Self::entity(cx).update(cx, |store, cx| {
            f(&mut store.prefs);
            save(&prefs_path(), &store.prefs);
            cx.notify();
        });
    }

    pub fn is_liked(id: &str, cx: &App) -> bool {
        Self::entity(cx).read(cx).library.liked.contains(id)
    }

    pub fn is_favourite(id: &str, cx: &App) -> bool {
        Self::entity(cx).read(cx).library.favourites.iter().any(|a| a.aweme_id == id)
    }

    pub fn toggle_like(id: &str, cx: &mut App) -> bool {
        Self::update_library(cx, |lib| {
            if lib.liked.remove(id) {
                false
            } else {
                lib.liked.insert(id.to_string());
                true
            }
        })
    }

    pub fn toggle_favourite(aweme: &Aweme, cx: &mut App) -> bool {
        Self::update_library(cx, |lib| {
            if let Some(i) = lib.favourites.iter().position(|a| a.aweme_id == aweme.aweme_id) {
                lib.favourites.remove(i);
                false
            } else {
                lib.favourites.insert(0, aweme.clone());
                true
            }
        })
    }

    fn update_library<R>(cx: &mut App, f: impl FnOnce(&mut Library) -> R) -> R {
        Self::entity(cx).update(cx, |store: &mut Store, cx: &mut Context<Store>| {
            let r = f(&mut store.library);
            save(&library_path(), &store.library);
            cx.notify();
            r
        })
    }
}

fn dir(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(fallback)))
        .unwrap_or_else(std::env::temp_dir)
        .join("tokers")
}

fn prefs_path() -> PathBuf {
    dir("XDG_CONFIG_HOME", ".config").join("settings.json")
}

fn library_path() -> PathBuf {
    dir("XDG_DATA_HOME", ".local/share").join("library.json")
}

/// Where downloaded videos go.
pub fn downloads_dir() -> PathBuf {
    std::env::var_os("XDG_DOWNLOAD_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Downloads")))
        .unwrap_or_else(std::env::temp_dir)
        .join("tokers")
}

fn load<T: Default + for<'de> Deserialize<'de>>(path: &PathBuf) -> T {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save<T: Serialize>(path: &PathBuf, value: &T) {
    let write = || -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
        std::fs::rename(tmp, path)
    };
    if let Err(e) = write() {
        eprintln!("tokers: saving {}: {e}", path.display());
    }
}
