//! Exact-count compact CPU scene ownership and encoding.

mod budget;
mod builder;
mod codec;
mod preflight;
mod resident;
mod runtime;

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use budget::PROJECT_WORKGROUP_SIZE;
pub(crate) use budget::RESIDENT_COLOR_STORAGE_BINDINGS;
pub use budget::{ResidentCpuByteAccounting, ResidentGpuBytePlan};
pub use builder::ResidentSceneBuilder;
pub use codec::resident_sh_plane_count;
pub use preflight::{
    DirectSceneError, DirectScenePath, DirectScenePreflight, DirectSceneRemediation,
    DirectSceneResource, DirectSceneResourceRequirement, PackedScenePath, PackedScenePreflight,
    PackedScenePreflightFailure, PackedScenePreflightLimits, direct_scene_preflight,
    packed_scene_preflight, packed_scene_preflight_with_limits,
};
pub use resident::{
    ResidentEncodingReport, ResidentSceneCpu, ResidentSceneError, ResidentSourceSplat,
};
pub(crate) use runtime::SceneRuntime;
