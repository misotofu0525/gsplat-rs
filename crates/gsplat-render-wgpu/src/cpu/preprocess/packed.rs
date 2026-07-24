//! Native direct-packed visibility/depth preprocessing.
//!
//! This leaf deliberately uses the Scalar depth oracle and optional Rayon
//! chunking only. Architecture-specific SIMD leaves remain independently
//! qualified and are not part of this experiment.

use gsplat_core::Camera;
use gsplat_sort::CpuSortBackend;
use rayon::prelude::*;

use crate::RendererError;
use crate::data::CpuPositionView;

use super::{
    MAX_PARALLEL_PREPROCESS_CHUNKS, PARALLEL_PREPROCESS_THRESHOLD, PreprocessContext, scalar,
};

#[derive(Default)]
pub(crate) struct PackedPreprocessChunkScratch {
    pairs: Vec<u64>,
}

fn reserve_output(pairs: &mut Vec<u64>, capacity: usize) {
    pairs.clear();
    if pairs.capacity() < capacity {
        pairs.reserve(capacity);
    }
}

#[inline]
fn pack_visible(depth: f32, source_id: u32) -> u64 {
    CpuSortBackend::pack_key_value(depth.max(0.0).to_bits(), source_id)
}

fn preprocess_into(
    positions: CpuPositionView<'_>,
    source_base: usize,
    context: PreprocessContext,
    pairs: &mut Vec<u64>,
) {
    for (local_index, position) in positions.as_slice().iter().copied().enumerate() {
        let depth = scalar::depth(position, context.camera_position, context.depth_row);
        if depth >= context.near_plane && depth <= context.far_plane {
            pairs.push(pack_visible(depth, (source_base + local_index) as u32));
        }
    }
}

pub(crate) fn positions_visible_into(
    positions: CpuPositionView<'_>,
    camera: &Camera,
    pairs: &mut Vec<u64>,
    chunks: &mut Vec<PackedPreprocessChunkScratch>,
) -> Result<(), RendererError> {
    let context = PreprocessContext::from_camera(camera)?;

    if positions.len() >= PARALLEL_PREPROCESS_THRESHOLD && rayon::current_num_threads() >= 2 {
        let chunk_count = rayon::current_num_threads()
            .min(MAX_PARALLEL_PREPROCESS_CHUNKS)
            .min(positions.len());
        let chunk_len = positions.len().div_ceil(chunk_count);
        if chunks.len() < chunk_count {
            chunks.resize_with(chunk_count, PackedPreprocessChunkScratch::default);
        }
        chunks[..chunk_count]
            .par_iter_mut()
            .enumerate()
            .for_each(|(chunk_index, scratch)| {
                let begin = chunk_index * chunk_len;
                let end = (begin + chunk_len).min(positions.len());
                reserve_output(&mut scratch.pairs, end - begin);
                preprocess_into(
                    positions.slice(begin..end),
                    begin,
                    context,
                    &mut scratch.pairs,
                );
            });

        reserve_output(pairs, positions.len());
        for scratch in &chunks[..chunk_count] {
            pairs.extend_from_slice(&scratch.pairs);
        }
        return Ok(());
    }

    reserve_output(pairs, positions.len());
    preprocess_into(positions, 0, context, pairs);
    Ok(())
}

#[cfg(test)]
mod tests {
    use gsplat_core::{Camera, Vec3f};

    use super::{PackedPreprocessChunkScratch, positions_visible_into, preprocess_into};
    use crate::cpu::preprocess::{PreprocessContext, scalar};
    use crate::data::CpuPositionView;

    fn camera(near_plane: f32, far_plane: f32) -> Camera {
        let mut camera = Camera::default();
        camera.intrinsics.near_plane = near_plane;
        camera.intrinsics.far_plane = far_plane;
        camera
    }

    fn unpack_pairs(pairs: &[u64]) -> (Vec<u32>, Vec<u32>) {
        pairs
            .iter()
            .copied()
            .map(|pair| ((pair >> 32) as u32, !(pair as u32)))
            .unzip()
    }

    #[test]
    fn scalar_packed_matches_split_oracle_for_adversarial_tail_and_source_base() {
        let near = 1.0_f32;
        let far = 3.0_f32;
        let below_near = f32::from_bits(near.to_bits() - 1);
        let above_near = f32::from_bits(near.to_bits() + 1);
        let below_far = f32::from_bits(far.to_bits() - 1);
        let above_far = f32::from_bits(far.to_bits() + 1);
        let mut positions = vec![Vec3f::new(0.0, 0.0, 2.0); 257];
        positions[..10].copy_from_slice(&[
            Vec3f::new(0.0, 0.0, near),
            Vec3f::new(0.0, 0.0, far),
            Vec3f::new(0.0, 0.0, below_near),
            Vec3f::new(0.0, 0.0, above_near),
            Vec3f::new(0.0, 0.0, below_far),
            Vec3f::new(0.0, 0.0, above_far),
            Vec3f::new(0.0, 0.0, f32::NAN),
            Vec3f::new(0.0, 0.0, f32::INFINITY),
            Vec3f::new(0.0, 0.0, 0.0),
            Vec3f::new(0.0, 0.0, -0.0),
        ]);
        let camera = camera(near, far);
        let context = PreprocessContext::from_camera(&camera).expect("camera");
        let source_base = 37;

        let mut expected_keys = Vec::new();
        let mut expected_ids = Vec::new();
        scalar::preprocess_into(
            CpuPositionView::new(&positions),
            source_base,
            context,
            &mut expected_keys,
            &mut expected_ids,
        );
        let mut packed = Vec::new();
        preprocess_into(
            CpuPositionView::new(&positions),
            source_base,
            context,
            &mut packed,
        );

        let (actual_keys, actual_ids) = unpack_pairs(&packed);
        assert_eq!(actual_keys, expected_keys);
        assert_eq!(actual_ids, expected_ids);
    }

    #[test]
    fn rayon_packed_matches_scalar_split_oracle_element_for_element() {
        let positions = (0..(super::PARALLEL_PREPROCESS_THRESHOLD + 257))
            .map(|index| {
                let depth = 1.0 + (index % 4096) as f32 * 0.000_1;
                Vec3f::new(index as f32 * 0.000_01, 0.0, depth)
            })
            .collect::<Vec<_>>();
        let camera = camera(0.5, 4.0);
        let context = PreprocessContext::from_camera(&camera).expect("camera");
        let mut expected_keys = Vec::new();
        let mut expected_ids = Vec::new();
        scalar::preprocess_into(
            CpuPositionView::new(&positions),
            0,
            context,
            &mut expected_keys,
            &mut expected_ids,
        );

        let mut packed = Vec::new();
        let mut chunks = Vec::<PackedPreprocessChunkScratch>::new();
        rayon::ThreadPoolBuilder::new()
            .num_threads(4)
            .build()
            .expect("rayon pool")
            .install(|| {
                positions_visible_into(
                    CpuPositionView::new(&positions),
                    &camera,
                    &mut packed,
                    &mut chunks,
                )
                .expect("packed preprocess");
            });

        let (actual_keys, actual_ids) = unpack_pairs(&packed);
        assert_eq!(actual_keys, expected_keys);
        assert_eq!(actual_ids, expected_ids);
    }
}
