//! x86_64 preprocess leaf reserved for the separately qualified AVX2/FMA task.

use crate::data::CpuPositionView;

use super::{DepthKeyPrecision, PreprocessContext, scalar};

pub(super) fn preprocess_into(
    positions: CpuPositionView<'_>,
    source_base: usize,
    context: PreprocessContext,
    precision: DepthKeyPrecision,
    depth_keys: &mut Vec<u32>,
    source_ids: &mut Vec<u32>,
) {
    scalar::preprocess_into_with_precision(
        positions,
        source_base,
        context,
        precision,
        depth_keys,
        source_ids,
    );
}
