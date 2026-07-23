//! x86_64 preprocess leaf reserved for the separately qualified AVX2/FMA task.

use crate::data::CpuPositionView;

use super::{PreprocessContext, scalar};

pub(super) fn preprocess_into(
    positions: CpuPositionView<'_>,
    source_base: usize,
    context: PreprocessContext,
    depth_keys: &mut Vec<u32>,
    source_ids: &mut Vec<u32>,
) {
    scalar::preprocess_into(positions, source_base, context, depth_keys, source_ids);
}
