//! Bounded CPU residency caches for compressed sources and decoded scenes.
//!
//! Phase C keeps whole-scene SPZ residency separate from the future page
//! scheduler. These caches enforce independent byte budgets so compression
//! does not merely move a memory spike from GPU to CPU.

use std::collections::{HashMap, VecDeque};
use std::mem::size_of;

use gsplat_core::{SceneBuffers, Vec3f};

/// Independent byte budgets for compressed bytes and decoded `SceneBuffers`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceCacheBudgets {
    pub max_compressed_bytes: usize,
    pub max_decoded_bytes: usize,
}

impl Default for SourceCacheBudgets {
    fn default() -> Self {
        Self {
            max_compressed_bytes: 256 * 1024 * 1024,
            max_decoded_bytes: 512 * 1024 * 1024,
        }
    }
}

/// Estimate the logical residency bytes accounted for a decoded scene.
pub fn estimated_scene_bytes(scene: &SceneBuffers) -> usize {
    let count = scene.len();
    let base = count
        .saturating_mul(size_of::<Vec3f>())
        .saturating_add(count.saturating_mul(size_of::<f32>()))
        .saturating_add(count.saturating_mul(size_of::<[f32; 3]>() * 2))
        .saturating_add(count.saturating_mul(size_of::<[f32; 4]>()));
    let rest = scene
        .sh_rest
        .as_ref()
        .map(|coeffs| coeffs.len().saturating_mul(size_of::<f32>()))
        .unwrap_or(0);
    base.saturating_add(rest)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceCacheError {
    /// A single insert exceeds the cache budget even after eviction.
    EntryExceedsBudget {
        requested: usize,
        limit: usize,
    },
}

#[derive(Debug)]
struct CacheEntry<V> {
    bytes: usize,
    value: V,
}

/// LRU cache that tracks an explicit byte cost per entry.
#[derive(Debug)]
pub struct BoundedByteCache<V> {
    max_bytes: usize,
    current_bytes: usize,
    order: VecDeque<String>,
    entries: HashMap<String, CacheEntry<V>>,
}

impl<V> BoundedByteCache<V> {
    pub fn new(max_bytes: usize) -> Self {
        Self {
            max_bytes,
            current_bytes: 0,
            order: VecDeque::new(),
            entries: HashMap::new(),
        }
    }

    pub fn max_bytes(&self) -> usize {
        self.max_bytes
    }

    pub fn current_bytes(&self) -> usize {
        self.current_bytes
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }

    pub fn get(&mut self, key: &str) -> Option<&V> {
        if !self.entries.contains_key(key) {
            return None;
        }
        self.touch(key);
        self.entries.get(key).map(|entry| &entry.value)
    }

    pub fn insert(
        &mut self,
        key: impl Into<String>,
        value: V,
        bytes: usize,
    ) -> Result<(), SourceCacheError> {
        let key = key.into();
        if bytes > self.max_bytes {
            return Err(SourceCacheError::EntryExceedsBudget {
                requested: bytes,
                limit: self.max_bytes,
            });
        }
        if let Some(previous) = self.entries.remove(&key) {
            self.current_bytes = self.current_bytes.saturating_sub(previous.bytes);
            self.order.retain(|existing| existing != &key);
        }
        while self.current_bytes.saturating_add(bytes) > self.max_bytes {
            let Some(evicted_key) = self.order.pop_front() else {
                break;
            };
            if let Some(evicted) = self.entries.remove(&evicted_key) {
                self.current_bytes = self.current_bytes.saturating_sub(evicted.bytes);
            }
        }
        if self.current_bytes.saturating_add(bytes) > self.max_bytes {
            return Err(SourceCacheError::EntryExceedsBudget {
                requested: bytes,
                limit: self.max_bytes,
            });
        }
        self.current_bytes = self.current_bytes.saturating_add(bytes);
        self.order.push_back(key.clone());
        self.entries.insert(key, CacheEntry { bytes, value });
        Ok(())
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
        self.current_bytes = 0;
    }

    fn touch(&mut self, key: &str) {
        if let Some(position) = self.order.iter().position(|existing| existing == key) {
            let key = self.order.remove(position).expect("position checked");
            self.order.push_back(key);
        }
    }
}

/// Paired compressed-source and decoded-scene CPU caches.
#[derive(Debug)]
pub struct SourceResidencyCaches {
    pub compressed: BoundedByteCache<Vec<u8>>,
    pub decoded: BoundedByteCache<SceneBuffers>,
}

impl SourceResidencyCaches {
    pub fn new(budgets: SourceCacheBudgets) -> Self {
        Self {
            compressed: BoundedByteCache::new(budgets.max_compressed_bytes),
            decoded: BoundedByteCache::new(budgets.max_decoded_bytes),
        }
    }

    pub fn insert_compressed(
        &mut self,
        key: impl Into<String>,
        bytes: Vec<u8>,
    ) -> Result<(), SourceCacheError> {
        let cost = bytes.len();
        self.compressed.insert(key, bytes, cost)
    }

    pub fn insert_decoded(
        &mut self,
        key: impl Into<String>,
        scene: SceneBuffers,
    ) -> Result<(), SourceCacheError> {
        let cost = estimated_scene_bytes(&scene);
        self.decoded.insert(key, scene, cost)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gsplat_core::Vec3f;

    fn tiny_scene(count: usize) -> SceneBuffers {
        SceneBuffers {
            positions: vec![Vec3f::new(0.0, 0.0, 1.0); count],
            opacity: vec![0.0; count],
            scale_xyz: vec![[-4.0, -4.0, -4.0]; count],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; count],
            color_dc: vec![[0.0, 0.0, 0.0]; count],
            sh_degree: 0,
            sh_rest: None,
        }
    }

    #[test]
    fn compressed_cache_evicts_oldest_until_budget_fits() {
        let mut cache = BoundedByteCache::new(10);
        cache.insert("a", vec![0_u8; 4], 4).unwrap();
        cache.insert("b", vec![0_u8; 4], 4).unwrap();
        assert_eq!(cache.current_bytes(), 8);
        cache.insert("c", vec![0_u8; 6], 6).unwrap();
        // Evict oldest `a` so `b`(4)+`c`(6) fit the 10-byte budget.
        assert!(!cache.contains_key("a"));
        assert!(cache.contains_key("b"));
        assert!(cache.contains_key("c"));
        assert_eq!(cache.current_bytes(), 10);
        assert_eq!(cache.get("c").map(Vec::len), Some(6));
    }

    #[test]
    fn compressed_cache_rejects_entry_larger_than_budget() {
        let mut cache = BoundedByteCache::new(8);
        let err = cache
            .insert("too-big", vec![0_u8; 9], 9)
            .expect_err("must reject oversized entry");
        assert_eq!(
            err,
            SourceCacheError::EntryExceedsBudget {
                requested: 9,
                limit: 8,
            }
        );
        assert!(cache.is_empty());
    }

    #[test]
    fn lru_touch_protects_recently_used_entry() {
        let mut cache = BoundedByteCache::new(8);
        cache.insert("a", vec![0_u8; 4], 4).unwrap();
        cache.insert("b", vec![0_u8; 4], 4).unwrap();
        assert!(cache.get("a").is_some());
        cache.insert("c", vec![0_u8; 4], 4).unwrap();
        assert!(cache.contains_key("a"));
        assert!(!cache.contains_key("b"));
        assert!(cache.contains_key("c"));
    }

    #[test]
    fn decoded_cache_uses_scene_byte_estimate() {
        let one = estimated_scene_bytes(&tiny_scene(1));
        let mut caches = SourceResidencyCaches::new(SourceCacheBudgets {
            max_compressed_bytes: 1024,
            max_decoded_bytes: one,
        });
        caches.insert_decoded("one", tiny_scene(1)).unwrap();
        caches.insert_decoded("two", tiny_scene(1)).unwrap();
        // Budget fits exactly one splat scene; second insert must evict first.
        assert_eq!(caches.decoded.len(), 1);
        assert!(caches.decoded.contains_key("two"));
        assert!(!caches.decoded.contains_key("one"));
        assert_eq!(caches.decoded.current_bytes(), one);
    }

    #[test]
    fn paired_caches_track_independent_budgets() {
        let mut caches = SourceResidencyCaches::new(SourceCacheBudgets {
            max_compressed_bytes: 16,
            max_decoded_bytes: estimated_scene_bytes(&tiny_scene(1)) * 2,
        });
        caches
            .insert_compressed("src", vec![1_u8; 12])
            .unwrap();
        caches.insert_decoded("src", tiny_scene(1)).unwrap();
        assert_eq!(caches.compressed.current_bytes(), 12);
        assert_eq!(
            caches.decoded.current_bytes(),
            estimated_scene_bytes(&tiny_scene(1))
        );
        caches
            .insert_compressed("other", vec![2_u8; 8])
            .unwrap();
        assert!(!caches.compressed.contains_key("src"));
        assert!(caches.decoded.contains_key("src"));
    }
}
