//! In-memory cache of game media (covers, artwork, screenshots) of games that are not installed.
//!
//! Installed games bring their media in the install folder (`igdb/`). Everything else is only
//! shown while browsing the server's catalog, which can hold thousands of games, so it is not
//! written to disk: decoded images stay in memory up to a size limit and the least recently shown
//! ones are dropped (and downloaded again when they are shown again).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Decoded size kept in memory (BGRA). A cover is about 0.4 MB, a 1080p artwork about 8 MB.
pub const LIMIT: usize = 256 * 1024 * 1024;

/// Images shown this recently are never dropped, even above the limit: they are on screen, and
/// dropping them would download them again on the next frame.
const KEEP: Duration = Duration::from_secs(5);

/// Server URL, game id, file name.
pub type MediaKey = (String, i64, String);

struct Entry<T> {
    value: Arc<T>,
    size: usize,
    used: Instant,
}

pub struct MediaCache<T> {
    entries: HashMap<MediaKey, Entry<T>>,
    size: usize,
    limit: usize,
    /// Dropped images; the UI removes them from the GPU texture atlas.
    evicted: Vec<Arc<T>>,
}

impl<T> MediaCache<T> {
    pub fn new(limit: usize) -> Self {
        MediaCache { entries: HashMap::new(), size: 0, limit, evicted: Vec::new() }
    }

    /// The image, marked as just used.
    pub fn get(&mut self, key: &MediaKey, now: Instant) -> Option<Arc<T>> {
        let e = self.entries.get_mut(key)?;
        e.used = now;
        Some(e.value.clone())
    }

    pub fn insert(&mut self, key: MediaKey, value: Arc<T>, size: usize, now: Instant) {
        if let Some(old) = self.entries.insert(key, Entry { value, size, used: now }) {
            self.size -= old.size;
            self.evicted.push(old.value);
        }
        self.size += size;
        while self.size > self.limit {
            let oldest = self.entries.iter().filter(|(_, e)| now.duration_since(e.used) >= KEEP).min_by_key(|(_, e)| e.used).map(|(k, _)| k.clone());
            let Some(key) = oldest else { break };
            if let Some(e) = self.entries.remove(&key) {
                self.size -= e.size;
                self.evicted.push(e.value);
            }
        }
    }

    pub fn take_evicted(&mut self) -> Vec<Arc<T>> {
        std::mem::take(&mut self.evicted)
    }

    #[cfg(test)]
    fn size(&self) -> usize {
        self.size
    }
}

#[cfg(test)]
mod tests {
    use super::{KEEP, MediaCache, MediaKey};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    fn key(id: i64) -> MediaKey {
        ("http://server".into(), id, "cover.jpg".into())
    }

    #[test]
    fn drops_the_least_recently_used_images_above_the_limit() {
        let mut c = MediaCache::new(300);
        let t0 = Instant::now();
        for id in 0..3 {
            c.insert(key(id), Arc::new(id), 100, t0 + Duration::from_secs(id as u64));
        }
        let later = t0 + KEEP * 3;
        // Shown again: no longer the oldest.
        assert!(c.get(&key(0), later).is_some());
        c.insert(key(3), Arc::new(3), 100, later);
        assert!(c.get(&key(1), later).is_none());
        assert!(c.get(&key(0), later).is_some());
        assert_eq!(c.size(), 300);
        assert_eq!(c.take_evicted().iter().map(|v| **v).collect::<Vec<_>>(), vec![1]);
        assert!(c.take_evicted().is_empty());
    }

    #[test]
    fn keeps_images_on_screen_even_above_the_limit() {
        let mut c = MediaCache::new(100);
        let now = Instant::now();
        c.insert(key(1), Arc::new(1), 100, now);
        c.insert(key(2), Arc::new(2), 100, now);
        assert_eq!(c.size(), 200);
        assert!(c.take_evicted().is_empty());
        // Once they are off screen the limit applies again.
        c.insert(key(3), Arc::new(3), 50, now + KEEP);
        assert!(c.size() <= 100);
    }

    #[test]
    fn replacing_an_image_releases_the_old_one() {
        let mut c = MediaCache::new(1000);
        let now = Instant::now();
        c.insert(key(1), Arc::new(1), 100, now);
        c.insert(key(1), Arc::new(2), 60, now);
        assert_eq!(c.size(), 60);
        assert_eq!(c.take_evicted().len(), 1);
    }
}
