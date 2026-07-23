mod capture;
mod configuration;
mod lifecycle;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use capture::SurfaceCapture;
pub use capture::SurfaceFrameCapture;
pub(crate) use configuration::SurfaceConfigurationOwner;
pub(crate) use lifecycle::{SurfaceLifecycle, create_surface_instance, select_present_mode};
