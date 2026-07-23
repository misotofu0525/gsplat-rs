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
pub(crate) use budget::PROJECTED_CACHE_BYTES_PER_SPLAT;
pub(crate) use budget::{
    PROJECT_WORKGROUP_SIZE, PROJECTED_CACHE_PLANE_BYTES_PER_SPLAT, RESIDENT_COLOR_STORAGE_BINDINGS,
    SCAN_ITEMS_PER_GROUP, SCAN_WORKGROUP_SIZE,
};
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
