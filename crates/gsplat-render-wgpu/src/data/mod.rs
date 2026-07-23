//! Strategy-free render data and kernel layout contracts.

mod layout;
mod view;

pub use layout::GpuInstance;
pub(crate) use layout::{GpuSortPair, GpuSurfaceRenderParams, GpuSurfaceSourceElem};
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use view::OwnedCpuOrderInput;
pub(crate) use view::{CameraCovarianceTerms, ShColorLayout, SplatSetView};
