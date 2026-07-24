//! Canonical SortedAlpha raster pipeline and draw encoding.

mod canonical;
mod encode;
mod pipeline;

// E10b prepares the dormant leaf before the Exact shadow-core integration
// task consumes these crate-private types.
#[allow(unused_imports)]
pub(crate) use canonical::{
    CanonicalRaster, CanonicalRasterError, CanonicalRasterInput, CanonicalRasterResources,
    RankIndexedRasterResources, SourceIndexedRasterResources,
};
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use encode::encode_splat_draw;
pub(crate) use encode::{
    SplatDraw, SplatIndirectDraw, encode_splat_draw_into, encode_splat_indirect_draw_into,
};
pub(crate) use pipeline::{SplatPipeline, create_splat_bind_group_layout, create_splat_pipeline};

pub(crate) const QUAD_VERTEX_COUNT: u32 = 4;
