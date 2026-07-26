//! Private exact CPU-order implementation leaves.

#[cfg(not(target_arch = "wasm32"))]
pub(super) mod calibration;
pub(super) mod preprocess;
pub(super) mod reference;
pub(super) mod workspace;
