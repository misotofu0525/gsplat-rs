//! Browser-facing WebAssembly bindings for the shared wgpu Surface renderer.

#[cfg(target_arch = "wasm32")]
mod wasm;

#[cfg(target_arch = "wasm32")]
pub use wasm::*;

#[cfg(not(target_arch = "wasm32"))]
pub const WEB_SDK_TARGET: &str = "wasm32-unknown-unknown";
