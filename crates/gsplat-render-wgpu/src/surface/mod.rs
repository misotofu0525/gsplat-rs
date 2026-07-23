mod configuration;
mod lifecycle;

pub(crate) use configuration::SurfaceConfigurationOwner;
pub(crate) use lifecycle::{SurfaceLifecycle, create_surface_instance, select_present_mode};
