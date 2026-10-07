//! Clips on disk: the current video's file, the next ones' and the last few watched,
//! so the player opens them from disk (going back, or paging on to a clip fetched
//! ahead) instead of streaming them again. Cleared at start: it is a cache, not a
//! library.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::state::cache_dir;

enum Clip {
    Fetching,
    Ready(PathBuf),
    Failed,
}

pub struct Clips {
    dir: PathBuf,
    files: HashMap<String, Clip>,
}

impl Clips {
    pub fn new() -> Self {
        let dir = cache_dir().join("clips");
        let _ = std::fs::remove_dir_all(&dir);
        Clips { dir, files: HashMap::new() }
    }

    /// Where a clip goes; `None` if it is already there, on its way or failed. Marks it
    /// as on its way.
    pub fn begin(&mut self, id: &str) -> Option<PathBuf> {
        if self.files.contains_key(id) {
            return None;
        }
        self.files.insert(id.to_string(), Clip::Fetching);
        Some(self.dir.join(format!("{id}.mp4")))
    }

    /// The fetch is over: the file, or nothing if it failed. A clip let go meanwhile
    /// loses its file at once.
    pub fn finish(&mut self, id: &str, path: Option<PathBuf>) {
        match (self.files.get_mut(id), path) {
            (Some(clip), Some(path)) => *clip = Clip::Ready(path),
            (Some(clip), None) => *clip = Clip::Failed,
            (None, Some(path)) => {
                let _ = std::fs::remove_file(path);
            }
            (None, None) => {}
        }
    }

    /// `file://` URI of the clip, once it is on disk.
    pub fn uri(&self, id: &str) -> Option<String> {
        match self.files.get(id)? {
            Clip::Ready(path) => Some(format!("file://{}", path.display())),
            _ => None,
        }
    }

    pub fn fetching(&self, id: &str) -> bool {
        matches!(self.files.get(id), Some(Clip::Fetching))
    }

    /// Only these clips stay; the others' files are deleted.
    pub fn retain(&mut self, keep: &HashSet<String>) {
        self.files.retain(|id, clip| {
            let kept = keep.contains(id);
            if !kept && let Clip::Ready(path) = clip {
                let _ = std::fs::remove_file(path);
            }
            kept
        });
    }
}
