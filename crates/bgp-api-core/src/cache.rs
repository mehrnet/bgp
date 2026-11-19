use std::borrow::Borrow;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::net::IpAddr;
use std::sync::{Arc, RwLock};

use bytes::Bytes;

const CACHE_SHARDS: usize = 16;

#[derive(Debug, Default)]
struct CacheInner<K> {
    enabled: bool,
    max_bytes: usize,
    used_bytes: usize,
    entries: HashMap<K, Bytes>,
    order: VecDeque<K>,
}

#[derive(Debug)]
pub struct ResponseCache<K> {
    shards: [RwLock<CacheInner<K>>; CACHE_SHARDS],
}

impl<K: Eq + Hash + Clone> ResponseCache<K> {
    #[must_use]
    pub fn new(max_bytes: usize) -> Self {
        let per_shard = if max_bytes == 0 {
            0
        } else {
            max_bytes.div_ceil(CACHE_SHARDS)
        };
        let shards = std::array::from_fn(|_| {
            RwLock::new(CacheInner {
                enabled: per_shard > 0,
                max_bytes: per_shard,
                used_bytes: 0,
                entries: HashMap::new(),
                order: VecDeque::new(),
            })
        });
        Self { shards }
    }

    fn shard_for<Q: Hash + ?Sized>(&self, key: &Q) -> &RwLock<CacheInner<K>> {
        let mut h = FnvHasher(0xcbf2_9ce4_8422_2325);
        key.hash(&mut h);
        let idx = (h.finish() as usize) & (CACHE_SHARDS - 1);
        // Safe: CACHE_SHARDS is 16 and idx is masked with 15.
        self.shards.get(idx).unwrap_or(&self.shards[0])
    }

    #[must_use]
    pub fn get<Q>(&self, key: &Q) -> Option<Bytes>
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        let guard = self.shard_for(key).read().ok()?;
        if !guard.enabled {
            return None;
        }
        guard.entries.get(key).cloned()
    }

    pub fn insert(&self, key: K, body: Bytes) {
        let Ok(mut guard) = self.shard_for(&key).write() else {
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
        for shard in &self.shards {
            if let Ok(mut guard) = shard.write() {
                guard.enabled = guard.max_bytes > 0;
            }
        }
    }

    #[must_use]
    pub fn disable_and_clear(&self) -> (usize, usize) {
        let (mut count, mut bytes) = (0usize, 0usize);
        for shard in &self.shards {
            let Ok(mut guard) = shard.write() else {
                continue;
            };
            count += guard.entries.len();
            bytes += guard.used_bytes;
            guard.enabled = false;
            guard.used_bytes = 0;
            guard.entries.clear();
            guard.order.clear();
        }
        (count, bytes)
    }
}

struct FnvHasher(u64);

impl Hasher for FnvHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
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
    pub(crate) compact: Arc<ResponseCache<IpAddr>>,
    pub(crate) resource: Arc<ResponseCache<String>>,
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
