//! Canonical SortedAlpha raster pipeline and draw encoding.

mod encode;
mod pipeline;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use encode::encode_splat_draw;
pub(crate) use encode::{
    SplatDraw, SplatIndirectDraw, encode_splat_draw_into, encode_splat_indirect_draw_into,
};
pub(crate) use pipeline::{SplatPipeline, create_splat_bind_group_layout, create_splat_pipeline};

pub(crate) const QUAD_VERTEX_COUNT: u32 = 4;
