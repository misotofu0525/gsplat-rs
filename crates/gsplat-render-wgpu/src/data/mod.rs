//! Strategy-free render data and kernel layout contracts.

mod layout;
mod view;

pub use layout::{
    GpuInstance, RESIDENT_CHUNK_META_BYTES, RESIDENT_CHUNK_SPLATS, RESIDENT_COLOR_AUX_WORDS,
    RESIDENT_COVARIANCE0_FLOATS, RESIDENT_COVARIANCE1_FLOATS, RESIDENT_SH_PLANES,
    RESIDENT_SH_WORDS_PER_PLANE, ResidentChunkMeta, ResidentColorAux, ResidentCovariance0,
    ResidentCovariance1, ResidentPositionAlpha, ResidentShPlane,
};
pub(crate) use layout::{GpuSortPair, GpuSurfaceRenderParams, GpuSurfaceSourceElem};
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use view::OwnedCpuOrderInput;
pub(crate) use view::{CameraCovarianceTerms, CpuPositionView, ShColorLayout, SplatSetView};
