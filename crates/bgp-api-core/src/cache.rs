use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::Mutex;

use bytes::Bytes;

#[derive(Debug, Default)]
struct CacheInner {
    enabled: bool,
    max_bytes: usize,
    used_bytes: usize,
    entries: HashMap<String, Bytes>,
    order: VecDeque<String>,
}

#[derive(Debug)]
pub struct ResponseCache {
    inner: Mutex<CacheInner>,
}

impl ResponseCache {
    #[must_use]
    pub fn new(max_bytes: usize) -> Self {
        Self {
            inner: Mutex::new(CacheInner {
                enabled: max_bytes > 0,
                max_bytes,
                used_bytes: 0,
                entries: HashMap::new(),
                order: VecDeque::new(),
            }),
        }
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<Bytes> {
        let guard = self.inner.lock().ok()?;
        if !guard.enabled {
            return None;
        }
        guard.entries.get(key).cloned()
    }

    pub fn insert(&self, key: String, body: Bytes) {
        let Ok(mut guard) = self.inner.lock() else {
            return;
        };
        if !guard.enabled || guard.max_bytes == 0 || body.len() > guard.max_bytes {
            return;
        }
        if let Some(prev) = guard.entries.remove(&key) {
            guard.used_bytes = guard.used_bytes.saturating_sub(prev.len());
        }
        while guard.used_bytes + body.len() > guard.max_bytes {
            let Some(oldest) = guard.order.pop_front() else {
                break;
            };
            if let Some(evicted) = guard.entries.remove(&oldest) {
                guard.used_bytes = guard.used_bytes.saturating_sub(evicted.len());
            }
        }
        guard.used_bytes += body.len();
        guard.order.push_back(key.clone());
        guard.entries.insert(key, body);
    }

    pub fn enable(&self) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.enabled = guard.max_bytes > 0;
        }
    }

    #[must_use]
    pub fn disable_and_clear(&self) -> (usize, usize) {
        let Ok(mut guard) = self.inner.lock() else {
            return (0, 0);
        };
        let count = guard.entries.len();
        let bytes = guard.used_bytes;
        guard.enabled = false;
        guard.used_bytes = 0;
        guard.entries.clear();
        guard.order.clear();
        (count, bytes)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RuntimeCacheRelease {
    pub compact_entries: usize,
    pub compact_bytes: usize,
    pub resource_entries: usize,
    pub resource_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct RuntimeCacheController {
    pub(crate) compact: Arc<ResponseCache>,
    pub(crate) resource: Arc<ResponseCache>,
}

impl RuntimeCacheController {
    #[must_use]
    pub fn new(compact_bytes: usize, resource_bytes: usize) -> Self {
        Self {
            compact: Arc::new(ResponseCache::new(compact_bytes)),
            resource: Arc::new(ResponseCache::new(resource_bytes)),
        }
    }

    pub fn enable(&self) {
        self.compact.enable();
        self.resource.enable();
    }

    #[must_use]
    pub fn disable_and_clear(&self) -> RuntimeCacheRelease {
        let (ce, cb) = self.compact.disable_and_clear();
        let (re, rb) = self.resource.disable_and_clear();
        RuntimeCacheRelease {
            compact_entries: ce,
            compact_bytes: cb,
            resource_entries: re,
            resource_bytes: rb,
        }
    }
}
