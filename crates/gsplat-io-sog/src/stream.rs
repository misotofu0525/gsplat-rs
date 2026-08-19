//! Metadata-first Streamed SOG assembly with independent budgets.

use std::collections::{HashMap, HashSet, hash_map::Entry};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use gsplat_core::{Camera, SceneBuffers};

use crate::SogError;
use crate::decode::{append_scene, decode_sog_dir, decode_sog_range};
use crate::meta::{LodMeta, LodNode, LodRange, SogChunkMeta, load_chunk_meta, load_lod_meta};

const MIB: usize = 1024 * 1024;

/// Independent source, decoded-CPU, and gaussian budgets for Streamed SOG.
///
/// These budgets are applied while selecting leaves from `lod-meta.json`. They
/// are not a GPU page pool: the existing renderer still capacity-checks the
/// assembled subset before upload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamingBudgets {
    /// Maximum bytes of selected chunk files (metadata + images) to read.
    pub max_source_bytes: usize,
    /// Maximum estimated decoded `SceneBuffers` bytes for the assembled subset.
    pub max_decoded_bytes: usize,
    /// Maximum gaussians in the assembled subset, including environment.
    pub max_gaussians: usize,
}

impl Default for StreamingBudgets {
    fn default() -> Self {
        Self {
            max_source_bytes: 256 * MIB,
            max_decoded_bytes: 256 * MIB,
            max_gaussians: 1_000_000,
        }
    }
}

/// Result of assembling a budgeted Streamed SOG subset.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamAssembleResult {
    /// Selected subset, ready for the resident renderer.
    pub scene: SceneBuffers,
    /// Number of spatial leaves that contributed splats.
    pub selected_leaves: usize,
    /// Leaves skipped because they had no range or did not fit the budgets.
    pub dropped_leaves: usize,
    /// Gaussian count in `scene`, including environment when present.
    pub gaussians: usize,
    /// Bytes of selected chunk files (metadata + images).
    pub source_bytes: usize,
    /// Estimated decoded `SceneBuffers` bytes for `scene`.
    pub decoded_bytes: usize,
    /// Stable identity of the selected leaf ranges and environment.
    pub fingerprint: u64,
}

/// Holds Streamed SOG spatial metadata and a decoded-chunk cache.
///
/// The session never materializes the full scene. `assemble` reads `lod-meta.json`
/// first, selects leaves, then decodes only that subset.
pub struct StreamedSogSession {
    index: LodMeta,
    budgets: StreamingBudgets,
    decoded: HashMap<RangeKey, SceneBuffers>,
    environment: Option<SceneBuffers>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct RangeKey {
    file: usize,
    offset: usize,
    count: usize,
}

impl StreamedSogSession {
    /// Load `lod-meta.json` without decoding chunk images.
    pub fn open(lod_meta_path: &Path, budgets: StreamingBudgets) -> Result<Self, SogError> {
        Ok(Self {
            index: load_lod_meta(lod_meta_path)?,
            budgets,
            decoded: HashMap::new(),
            environment: None,
        })
    }

    pub fn budgets(&self) -> StreamingBudgets {
        self.budgets
    }

    pub fn index(&self) -> &LodMeta {
        &self.index
    }

    /// Select leaves for `camera` and decode only that subset.
    ///
    /// Without a camera, every leaf's LOD 0 range is required to fit. With a
    /// camera, farther leaves are dropped until the budgets fit. A single
    /// environment or nearest leaf that exceeds the full budget is a structured
    /// [`SogError::ResourceLimit`].
    pub fn assemble(&mut self, camera: Option<&Camera>) -> Result<StreamAssembleResult, SogError> {
        let plan = plan_selection(&self.index, camera, self.budgets)?;
        self.retain_plan(&plan);
        if let Some(environment) = &self.index.environment
            && self.environment.is_none()
        {
            self.environment = Some(decode_sog_dir(&self.index.root.join(environment))?);
        }

        self.decode_missing(&plan)?;

        let mut scene = SceneBuffers::default();
        if let Some(environment) = &self.environment {
            append_scene(&mut scene, environment.clone())?;
        }
        for range in &plan.ranges {
            append_scene(
                &mut scene,
                self.decoded
                    .get(&RangeKey::from(*range))
                    .expect("missing ranges are decoded before assembly")
                    .clone(),
            )?;
        }

        Ok(StreamAssembleResult {
            gaussians: scene.len(),
            selected_leaves: plan.ranges.len(),
            dropped_leaves: plan.dropped_leaves,
            source_bytes: plan.source_bytes,
            decoded_bytes: estimated_scene_bytes(&scene),
            fingerprint: selection_fingerprint(&plan, self.index.environment.is_some()),
            scene,
        })
    }

    fn decode_missing(&mut self, plan: &SelectionPlan) -> Result<(), SogError> {
        let missing: Vec<LodRange> = plan
            .ranges
            .iter()
            .copied()
            .filter(|range| !self.decoded.contains_key(&RangeKey::from(*range)))
            .collect();
        #[cfg(not(target_arch = "wasm32"))]
        if missing.len() > 1 {
            return self.decode_missing_parallel(&missing);
        }
        for range in missing {
            self.decode_one(range)?;
        }
        Ok(())
    }

    fn decode_one(&mut self, range: LodRange) -> Result<(), SogError> {
        let meta_path = self.index.root.join(&self.index.filenames[range.file]);
        let meta = load_chunk_meta(&meta_path)?;
        let slice = decode_sog_range(&meta_path, &meta, range.offset, range.count)?;
        self.decoded.insert(RangeKey::from(range), slice);
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn decode_missing_parallel(&mut self, missing: &[LodRange]) -> Result<(), SogError> {
        let decoded = std::thread::scope(|scope| {
            let index = &self.index;
            let handles: Vec<_> = missing
                .iter()
                .copied()
                .map(|range| {
                    scope.spawn(move || {
                        let meta_path = index.root.join(&index.filenames[range.file]);
                        let meta = load_chunk_meta(&meta_path)?;
                        decode_sog_range(&meta_path, &meta, range.offset, range.count)
                            .map(|scene| (range, scene))
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap_or(Err(SogError::DecodeJoin)))
                .collect::<Result<Vec<_>, _>>()
        })?;
        for (range, scene) in decoded {
            self.decoded.insert(RangeKey::from(range), scene);
        }
        Ok(())
    }

    fn retain_plan(&mut self, plan: &SelectionPlan) {
        let live: HashSet<RangeKey> = plan.ranges.iter().copied().map(RangeKey::from).collect();
        self.decoded.retain(|key, _| live.contains(key));
        if self.index.environment.is_none() {
            self.environment = None;
        }
    }
}

/// Assemble a Streamed SOG subset in one shot.
pub fn assemble_streamed_sog(
    lod_meta_path: &Path,
    camera: Option<&Camera>,
    budgets: StreamingBudgets,
) -> Result<StreamAssembleResult, SogError> {
    StreamedSogSession::open(lod_meta_path, budgets)?.assemble(camera)
}

struct SelectionPlan {
    ranges: Vec<LodRange>,
    dropped_leaves: usize,
    source_bytes: usize,
}

fn plan_selection(
    index: &LodMeta,
    camera: Option<&Camera>,
    budgets: StreamingBudgets,
) -> Result<SelectionPlan, SogError> {
    let mut leaves = Vec::new();
    index.tree.walk_leaves(&mut leaves);
    let mut ranked: Vec<RankedLeaf> = leaves
        .into_iter()
        .filter_map(|leaf| rank_leaf(leaf, camera, index.lod_levels))
        .collect();
    ranked.sort_by(|left, right| left.distance.total_cmp(&right.distance));

    let mut meta_cache: HashMap<usize, CachedChunk> = HashMap::new();
    let mut env_cost = ChunkCost::default();
    if let Some(environment) = &index.environment {
        let env_path = index.root.join(environment);
        let meta = load_chunk_meta(&env_path)?;
        env_cost = chunk_cost(&env_path, &meta, meta.count)?;
        ensure_single_fits(
            "environment gaussians",
            env_cost.gaussians,
            budgets.max_gaussians,
        )?;
        ensure_single_fits(
            "environment source bytes",
            env_cost.source_bytes,
            budgets.max_source_bytes,
        )?;
        ensure_single_fits(
            "environment decoded bytes",
            env_cost.decoded_bytes,
            budgets.max_decoded_bytes,
        )?;
    }

    if camera.is_none() {
        return plan_without_camera(index, &ranked, env_cost, budgets, &mut meta_cache);
    }
    plan_with_camera(index, &ranked, env_cost, budgets, &mut meta_cache)
}

fn plan_without_camera(
    index: &LodMeta,
    ranked: &[RankedLeaf],
    env_cost: ChunkCost,
    budgets: StreamingBudgets,
    meta_cache: &mut HashMap<usize, CachedChunk>,
) -> Result<SelectionPlan, SogError> {
    let mut ranges = Vec::new();
    let mut gaussians = env_cost.gaussians;
    let mut decoded_bytes = env_cost.decoded_bytes;
    let mut unique_files: HashSet<usize> = HashSet::new();
    let mut dropped_leaves = 0_usize;
    for leaf in ranked {
        let Some(range) = leaf.range else {
            dropped_leaves += 1;
            continue;
        };
        gaussians = gaussians.saturating_add(range.count);
        decoded_bytes =
            decoded_bytes.saturating_add(range_decoded_bytes(index, range, meta_cache)?);
        unique_files.insert(range.file);
        ranges.push(range);
    }
    let source_bytes = env_cost.source_bytes.saturating_add(unique_source_bytes(
        index,
        &unique_files,
        meta_cache,
    )?);
    ensure_total_fits("gaussians", gaussians, budgets.max_gaussians)?;
    ensure_total_fits("source bytes", source_bytes, budgets.max_source_bytes)?;
    ensure_total_fits("decoded bytes", decoded_bytes, budgets.max_decoded_bytes)?;
    if ranges.is_empty() && index.environment.is_none() {
        return Err(SogError::ResourceLimit {
            resource: "gaussians",
            requested: 0,
            limit: budgets.max_gaussians,
        });
    }
    Ok(SelectionPlan {
        ranges,
        dropped_leaves,
        source_bytes,
    })
}

fn plan_with_camera(
    index: &LodMeta,
    ranked: &[RankedLeaf],
    env_cost: ChunkCost,
    budgets: StreamingBudgets,
    meta_cache: &mut HashMap<usize, CachedChunk>,
) -> Result<SelectionPlan, SogError> {
    let mut ranges = Vec::new();
    let mut gaussians = env_cost.gaussians;
    let mut decoded_bytes = env_cost.decoded_bytes;
    let mut unique_files: HashSet<usize> = HashSet::new();
    let mut dropped_leaves = 0_usize;
    for leaf in ranked {
        let Some(range) = leaf.range else {
            dropped_leaves += 1;
            continue;
        };
        let adding_file = !unique_files.contains(&range.file);
        let added_source = if adding_file {
            file_source_bytes(index, range.file, meta_cache)?
        } else {
            0
        };
        let added_decoded = range_decoded_bytes(index, range, meta_cache)?;
        let next_gaussians = gaussians.saturating_add(range.count);
        let next_source = env_cost
            .source_bytes
            .saturating_add(unique_source_bytes(index, &unique_files, meta_cache)?)
            .saturating_add(added_source);
        let next_decoded = decoded_bytes.saturating_add(added_decoded);
        let fits = next_gaussians <= budgets.max_gaussians
            && next_source <= budgets.max_source_bytes
            && next_decoded <= budgets.max_decoded_bytes;
        if !fits {
            if ranges.is_empty() {
                if next_gaussians > budgets.max_gaussians {
                    ensure_single_fits("gaussians", range.count, budgets.max_gaussians)?;
                    return Err(SogError::ResourceLimit {
                        resource: "gaussians",
                        requested: next_gaussians,
                        limit: budgets.max_gaussians,
                    });
                }
                if next_source > budgets.max_source_bytes {
                    ensure_single_fits("source bytes", added_source, budgets.max_source_bytes)?;
                    return Err(SogError::ResourceLimit {
                        resource: "source bytes",
                        requested: next_source,
                        limit: budgets.max_source_bytes,
                    });
                }
                ensure_single_fits("decoded bytes", added_decoded, budgets.max_decoded_bytes)?;
                return Err(SogError::ResourceLimit {
                    resource: "decoded bytes",
                    requested: next_decoded,
                    limit: budgets.max_decoded_bytes,
                });
            }
            dropped_leaves += 1;
            continue;
        }
        gaussians = next_gaussians;
        decoded_bytes = next_decoded;
        unique_files.insert(range.file);
        ranges.push(range);
    }
    if ranges.is_empty() && index.environment.is_none() {
        return Err(SogError::ResourceLimit {
            resource: "gaussians",
            requested: 0,
            limit: budgets.max_gaussians,
        });
    }
    Ok(SelectionPlan {
        source_bytes: env_cost.source_bytes.saturating_add(unique_source_bytes(
            index,
            &unique_files,
            meta_cache,
        )?),
        ranges,
        dropped_leaves,
    })
}

struct RankedLeaf {
    distance: f32,
    range: Option<LodRange>,
}

struct CachedChunk {
    meta: SogChunkMeta,
    source_bytes: usize,
}

#[derive(Default, Clone, Copy)]
struct ChunkCost {
    gaussians: usize,
    source_bytes: usize,
    decoded_bytes: usize,
}

fn rank_leaf(leaf: &LodNode, camera: Option<&Camera>, lod_levels: u32) -> Option<RankedLeaf> {
    let center = rub_to_ruf(leaf.center());
    let (distance, lod) = match camera {
        Some(camera) => {
            let dx = center[0] - camera.pose.position.x;
            let dy = center[1] - camera.pose.position.y;
            let dz = center[2] - camera.pose.position.z;
            let distance = (dx * dx + dy * dy + dz * dz).sqrt();
            (distance, pick_lod(distance, lod_levels))
        }
        None => (0.0, 0),
    };
    Some(RankedLeaf {
        distance,
        range: pick_range(leaf, lod),
    })
}

fn pick_range(leaf: &LodNode, lod: u32) -> Option<LodRange> {
    if let Some(range) = leaf.lods.get(&lod) {
        return Some(*range);
    }
    leaf.lods
        .iter()
        .filter(|(candidate, _)| **candidate >= lod)
        .min_by_key(|(candidate, _)| **candidate)
        .map(|(_, range)| *range)
        .or_else(|| {
            leaf.lods
                .iter()
                .max_by_key(|(candidate, _)| *candidate)
                .map(|(_, range)| *range)
        })
}

fn pick_lod(distance: f32, lod_levels: u32) -> u32 {
    if lod_levels == 0 {
        return 0;
    }
    ((distance / 8.0).floor() as u32).min(lod_levels.saturating_sub(1))
}

fn rub_to_ruf(point: [f32; 3]) -> [f32; 3] {
    [point[0], point[1], -point[2]]
}

fn cached_chunk<'a>(
    index: &LodMeta,
    file: usize,
    meta_cache: &'a mut HashMap<usize, CachedChunk>,
) -> Result<&'a CachedChunk, SogError> {
    match meta_cache.entry(file) {
        Entry::Occupied(entry) => Ok(entry.into_mut()),
        Entry::Vacant(entry) => {
            let meta_path = index.root.join(&index.filenames[file]);
            let meta = load_chunk_meta(&meta_path)?;
            let source_bytes = sum_paths(&chunk_payload_paths(&meta_path, &meta))?;
            Ok(entry.insert(CachedChunk { meta, source_bytes }))
        }
    }
}

fn file_source_bytes(
    index: &LodMeta,
    file: usize,
    meta_cache: &mut HashMap<usize, CachedChunk>,
) -> Result<usize, SogError> {
    Ok(cached_chunk(index, file, meta_cache)?.source_bytes)
}

fn unique_source_bytes(
    index: &LodMeta,
    files: &HashSet<usize>,
    meta_cache: &mut HashMap<usize, CachedChunk>,
) -> Result<usize, SogError> {
    let mut total = 0_usize;
    for file in files {
        total = total.saturating_add(file_source_bytes(index, *file, meta_cache)?);
    }
    Ok(total)
}

fn range_decoded_bytes(
    index: &LodMeta,
    range: LodRange,
    meta_cache: &mut HashMap<usize, CachedChunk>,
) -> Result<usize, SogError> {
    let chunk = cached_chunk(index, range.file, meta_cache)?;
    let sh_degree = chunk.meta.shn.as_ref().map(|shn| shn.bands).unwrap_or(0);
    Ok(estimated_decoded_bytes(range.count, sh_degree))
}

fn chunk_cost(meta_path: &Path, meta: &SogChunkMeta, count: usize) -> Result<ChunkCost, SogError> {
    let sh_degree = meta.shn.as_ref().map(|shn| shn.bands).unwrap_or(0);
    Ok(ChunkCost {
        gaussians: count,
        source_bytes: sum_paths(&chunk_payload_paths(meta_path, meta))?,
        decoded_bytes: estimated_decoded_bytes(count, sh_degree),
    })
}

fn chunk_payload_paths(meta_path: &Path, meta: &SogChunkMeta) -> Vec<PathBuf> {
    let dir = meta_path.parent().unwrap_or(meta_path);
    let mut paths = vec![
        meta_path.to_path_buf(),
        dir.join(&meta.means_files[0]),
        dir.join(&meta.means_files[1]),
        dir.join(&meta.scales_file),
        dir.join(&meta.quats_file),
        dir.join(&meta.sh0_file),
    ];
    if let Some(shn) = &meta.shn {
        paths.push(dir.join(&shn.centroids_file));
        paths.push(dir.join(&shn.labels_file));
    }
    paths
}

fn sum_paths(paths: &[PathBuf]) -> Result<usize, SogError> {
    let mut total = 0_usize;
    for path in paths {
        total = total.saturating_add(file_len(path)?);
    }
    Ok(total)
}

fn file_len(path: &Path) -> Result<usize, SogError> {
    Ok(usize::try_from(path.metadata()?.len()).unwrap_or(usize::MAX))
}

fn ensure_single_fits(
    resource: &'static str,
    requested: usize,
    limit: usize,
) -> Result<(), SogError> {
    if requested > limit {
        Err(SogError::ResourceLimit {
            resource,
            requested,
            limit,
        })
    } else {
        Ok(())
    }
}

fn ensure_total_fits(
    resource: &'static str,
    requested: usize,
    limit: usize,
) -> Result<(), SogError> {
    ensure_single_fits(resource, requested, limit)
}

pub(crate) fn estimated_scene_bytes(scene: &SceneBuffers) -> usize {
    estimated_decoded_bytes(scene.len(), scene.sh_degree)
}

fn selection_fingerprint(plan: &SelectionPlan, has_environment: bool) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    has_environment.hash(&mut hasher);
    for range in &plan.ranges {
        range.hash(&mut hasher);
    }
    hasher.finish()
}

pub(crate) fn estimated_decoded_bytes(count: usize, sh_degree: u8) -> usize {
    let coeff_total = (usize::from(sh_degree) + 1).saturating_pow(2);
    let rest = coeff_total
        .saturating_sub(1)
        .saturating_mul(3)
        .saturating_mul(count)
        .saturating_mul(std::mem::size_of::<f32>());
    count
        .saturating_mul(std::mem::size_of::<gsplat_core::Vec3f>())
        .saturating_add(count.saturating_mul(std::mem::size_of::<f32>()))
        .saturating_add(count.saturating_mul(std::mem::size_of::<[f32; 3]>() * 2))
        .saturating_add(count.saturating_mul(std::mem::size_of::<[f32; 4]>()))
        .saturating_add(rest)
}

impl From<LodRange> for RangeKey {
    fn from(range: LodRange) -> Self {
        Self {
            file: range.file,
            offset: range.offset,
            count: range.count,
        }
    }
}
