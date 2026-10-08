//! Remote images (covers, avatars, photo posts): fetched on the tokio runtime,
//! decoded and downscaled off the UI thread, kept as BGRA [`RenderImage`]s.
//!
//! Views read through [`Images::get`] and observe the [`Images`] entity, which
//! notifies whenever an image lands.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{App, AppContext, Context, Entity, Global, RenderImage};
use smallvec::smallvec;
use tokers::TikTok;
use tokers::models::UrlList;

use crate::theme::{Palette, palette};

const CAPACITY: usize = 240;
/// An evicted image stays in the atlas this long: a cached view still replays the
/// primitives it recorded, and a freed tile could already hold another picture
/// (Sonora's artwork cache does the same).
const REPRIEVE: Duration = Duration::from_millis(250);

enum Slot {
    Loading,
    Ready { image: Arc<RenderImage>, palette: Palette, used: Cell<Instant> },
    Failed,
}

pub struct Images {
    tiktok: TikTok,
    io: tokio::runtime::Handle,
    slots: HashMap<String, Slot>,
    condemned: Vec<Arc<RenderImage>>,
    /// Images asked for on behalf of an owner (a video: its cover, its comments' avatars and
    /// stickers), kept past the capacity while the owner is held.
    held: HashMap<String, HashSet<String>>,
}

struct ImagesGlobal(Entity<Images>);

impl Global for ImagesGlobal {}

impl Images {
    pub fn init(tiktok: TikTok, io: tokio::runtime::Handle, cx: &mut App) {
        let images = cx.new(|_| Images {
            tiktok,
            io,
            slots: HashMap::new(),
            condemned: Vec::new(),
            held: HashMap::new(),
        });
        cx.set_global(ImagesGlobal(images));
    }

    pub fn entity(cx: &App) -> Entity<Images> {
        cx.global::<ImagesGlobal>().0.clone()
    }

    /// The image, scaled to fit `max` px on its longer side; starts loading it if needed.
    pub fn get(list: &UrlList, max: u32, cx: &mut App) -> Option<Arc<RenderImage>> {
        Self::slot(list, max, cx).map(|(image, _)| image)
    }

    /// [`Self::get`], the image kept for `owner` (see [`Self::hold_only`]).
    pub fn get_for(owner: &str, list: &UrlList, max: u32, cx: &mut App) -> Option<Arc<RenderImage>> {
        let key = key(list, max)?;
        let entity = Self::entity(cx);
        if !entity.read(cx).held.get(owner).is_some_and(|keys| keys.contains(&key)) {
            let key = key.clone();
            entity.update(cx, |this, _| this.held.entry(owner.to_string()).or_default().insert(key));
        }
        Self::keyed(key, list, max, cx).map(|(image, _)| image)
    }

    /// Only these owners' images stay past the capacity; the others' go back to taking
    /// their chances with the rest.
    pub fn hold_only<'a>(owners: impl IntoIterator<Item = &'a str>, cx: &mut App) {
        let owners: HashSet<&str> = owners.into_iter().collect();
        Self::entity(cx).update(cx, |this, _| this.held.retain(|owner, _| owners.contains(owner.as_str())));
    }

    /// The image's colours, once it is loaded.
    pub fn palette(list: &UrlList, max: u32, cx: &mut App) -> Option<Palette> {
        Self::slot(list, max, cx).map(|(_, palette)| palette)
    }

    fn slot(list: &UrlList, max: u32, cx: &mut App) -> Option<(Arc<RenderImage>, Palette)> {
        Self::keyed(key(list, max)?, list, max, cx)
    }

    /// Asked on every frame for every image on screen: the URLs are only gone through
    /// when the image is not there yet.
    fn keyed(key: String, list: &UrlList, max: u32, cx: &mut App) -> Option<(Arc<RenderImage>, Palette)> {
        let entity = Self::entity(cx);
        match entity.read(cx).slots.get(&key) {
            Some(Slot::Ready { image, palette, used }) => {
                used.set(Instant::now());
                return Some((image.clone(), *palette));
            }
            Some(_) => return None,
            None => {}
        }
        entity.update(cx, |this, cx| this.load(key, candidates(list), max, cx));
        None
    }

    fn load(&mut self, key: String, urls: Vec<String>, max: u32, cx: &mut Context<Self>) {
        self.slots.insert(key.clone(), Slot::Loading);
        if self.slots.len() > CAPACITY {
            self.trim(cx);
        }

        let transport = self.tiktok.transport().clone();
        let job = self.io.spawn(async move {
            for url in urls {
                let Ok(resp) = transport.get(&url, &[], None, false).await else { continue };
                if resp.status != 200 || resp.body.is_empty() {
                    continue;
                }
                let decoded =
                    tokio::task::spawn_blocking(move || decode(&resp.body, max)).await.ok().flatten();
                if decoded.is_some() {
                    return decoded;
                }
            }
            None
        });
        cx.spawn(async move |this, cx| {
            let result = job.await.ok().flatten();
            this.update(cx, |this, cx| {
                let slot = match result {
                    Some((image, palette)) => Slot::Ready { image, palette, used: Cell::new(Instant::now()) },
                    None => Slot::Failed,
                };
                if let Some(entry) = this.slots.get_mut(&key) {
                    *entry = slot;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }
}

impl Images {
    /// Evict the least recently used quarter (of what no held owner keeps); free their
    /// atlas tiles a moment later.
    fn trim(&mut self, cx: &mut Context<Self>) {
        let kept: HashSet<&String> = self.held.values().flatten().collect();
        let mut ready: Vec<(Instant, String)> = self
            .slots
            .iter()
            .filter(|(k, _)| !kept.contains(k))
            .filter_map(|(k, s)| match s {
                Slot::Ready { used, .. } => Some((used.get(), k.clone())),
                _ => None,
            })
            .collect();
        ready.sort();
        let fresh = self.condemned.is_empty();
        for (_, key) in ready.into_iter().take(CAPACITY / 4) {
            if let Some(Slot::Ready { image, .. }) = self.slots.remove(&key) {
                self.condemned.push(image);
            }
        }
        // failed lookups are retried after a trim
        self.slots.retain(|_, s| !matches!(s, Slot::Failed));
        if fresh && !self.condemned.is_empty() {
            cx.refresh_windows(); // cached views rebuild and ask again
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(REPRIEVE).await;
                this.update(cx, |this, cx| {
                    for image in std::mem::take(&mut this.condemned) {
                        cx.drop_image(image, None);
                    }
                })
                .ok();
            })
            .detach();
        }
    }
}

/// The cache key of an image at a size: its URI, else its first URL.
fn key(list: &UrlList, max: u32) -> Option<String> {
    let id = if list.uri.is_empty() { list.url_list.first()?.clone() } else { list.uri.clone() };
    Some(format!("{id}@{max}"))
}

/// HEIC/HEIF through libheif: avatars come only in that format, and the signed
/// URLs can't be rewritten to another extension.
fn decode_heif(bytes: &[u8]) -> Option<image::RgbaImage> {
    use libheif_rs::{ColorSpace, HeifContext, LibHeif, RgbChroma};
    let context = HeifContext::read_from_bytes(bytes).ok()?;
    let handle = context.primary_image_handle().ok()?;
    let decoded = LibHeif::new().decode(&handle, ColorSpace::Rgb(RgbChroma::Rgba), None).ok()?;
    let plane = decoded.planes().interleaved?;
    let (w, h, stride) = (plane.width as usize, plane.height as usize, plane.stride);
    let mut rgba = Vec::with_capacity(w * h * 4);
    for row in 0..h {
        rgba.extend_from_slice(&plane.data[row * stride..row * stride + w * 4]);
    }
    image::RgbaImage::from_raw(w as u32, h as u32, rgba)
}

/// Decodable URLs first: the CDN offers `.heic` next to `.jpeg`/`.webp`.
fn candidates(list: &UrlList) -> Vec<String> {
    let mut urls: Vec<String> = list.url_list.clone();
    urls.sort_by_key(|u| u.split('?').next().unwrap_or(u).ends_with(".heic"));
    urls
}

fn decode(bytes: &[u8], max: u32) -> Option<(Arc<RenderImage>, Palette)> {
    let image = image::load_from_memory(bytes)
        .ok()
        .or_else(|| decode_heif(bytes).map(image::DynamicImage::ImageRgba8))?;
    let image = if image.width().max(image.height()) > max { image.thumbnail(max, max) } else { image };
    let mut rgba = image.into_rgba8();
    for px in rgba.as_chunks_mut::<4>().0 {
        px.swap(0, 2); // gpui keeps BGRA
    }
    let palette = palette(&rgba);
    Some((Arc::new(RenderImage::new(smallvec![image::Frame::new(rgba)])), palette))
}
