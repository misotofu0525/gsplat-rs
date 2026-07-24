use std::fmt;

use gsplat_core::Vec3f;

use super::{ResidentSceneCpu, ResidentSceneError};
use crate::plans::{FrameIdentity, GpuExecutionContext, GpuOwnerToken};
use crate::renderer::gpu_prepare::{
    CpuPostProjectedHandles, CpuPostProjectionRequest, GpuExecutionOwner, GpuPreparationError,
    GpuPreparationReceipt, GpuPreprojectHandles, GpuProjectedHandles, GpuScenePreparation,
};

/// Private Exact, all-resident scene owner used by the shadow renderer core.
///
/// The resident value is consumed at construction. Callers receive only
/// borrowed views, so the shadow core cannot create a second positions array
/// or a second scene owner.
pub(crate) struct SceneRuntime {
    resident: ResidentSceneCpu,
    gpu: Option<GpuScenePreparation>,
}

impl fmt::Debug for SceneRuntime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SceneRuntime")
            .field("source_count", &self.source_count())
            .field("sh_degree", &self.sh_degree())
            .field("gpu_prepared", &self.gpu.is_some())
            .finish()
    }
}

impl SceneRuntime {
    pub(crate) fn prepare(resident: ResidentSceneCpu) -> Result<Self, ResidentSceneError> {
        resident.validate_complete()?;
        Ok(Self {
            resident,
            gpu: None,
        })
    }

    /// Builds a complete device-owned candidate without publishing it. The
    /// Renderer stages PlanSet admission before this value can be committed.
    pub(crate) async fn stage_gpu(
        &self,
        owner: &GpuExecutionOwner,
        generation: FrameIdentity,
    ) -> Result<GpuScenePreparation, GpuPreparationError> {
        if self.gpu.is_some() {
            return Err(GpuPreparationError::ExecutionOwnerAlreadyBound);
        }
        GpuScenePreparation::prepare(owner, &self.resident, generation).await
    }

    /// Infallibly publishes one fully staged GPU scene candidate.
    pub(crate) fn commit_gpu(&mut self, candidate: GpuScenePreparation) {
        debug_assert!(self.gpu.is_none());
        self.gpu = Some(candidate);
    }

    pub(crate) fn gpu_preparation(&self) -> Option<GpuPreparationReceipt> {
        self.gpu.as_ref().map(GpuScenePreparation::receipt)
    }

    #[cfg(test)]
    pub(crate) fn current_stats_resource_bytes(&self) -> Option<u64> {
        self.gpu
            .as_ref()
            .and_then(GpuScenePreparation::current_stats_resource_bytes)
    }

    #[cfg(test)]
    pub(crate) fn current_stats_live_object_count(&self) -> Option<usize> {
        self.gpu
            .as_ref()
            .and_then(GpuScenePreparation::current_stats_live_object_count)
    }

    #[cfg(test)]
    pub(crate) fn gpu_color_encode_count(&self) -> Option<u64> {
        self.gpu
            .as_ref()
            .map(GpuScenePreparation::color_encode_count)
    }

    #[cfg(test)]
    pub(crate) fn gpu_preproject_encode_count(&self) -> Option<u64> {
        self.gpu
            .as_ref()
            .map(GpuScenePreparation::preproject_encode_count)
    }

    #[cfg(test)]
    pub(crate) fn gpu_cpu_post_projection_encode_count(&self) -> Option<u64> {
        self.gpu
            .as_ref()
            .map(GpuScenePreparation::cpu_post_projection_encode_count)
    }

    #[cfg(test)]
    pub(crate) fn rebind_gpu(
        &mut self,
        owner: &GpuExecutionOwner,
        generation: FrameIdentity,
    ) -> Result<GpuPreparationReceipt, GpuPreparationError> {
        self.gpu
            .as_mut()
            .ok_or(GpuPreparationError::Unavailable)?
            .rebind_existing(owner, generation)
    }

    /// Encodes the fixed color/order/project sequence using the strictly
    /// owner-bound borrowed queue and caller encoder, then returns only opaque
    /// projected handles. It owns no submit, polling, mapping, readback,
    /// presentation, target or raster lifecycle.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn encode_gpu_frame<'scene>(
        &'scene mut self,
        context: GpuExecutionContext<'_>,
        camera: gsplat_core::Camera,
        width: u32,
        height: u32,
        frame: FrameIdentity,
    ) -> Result<GpuProjectedHandles<'scene>, GpuPreparationError> {
        let gpu = self.gpu.as_mut().ok_or(GpuPreparationError::Unavailable)?;
        gpu.encode_frame(context, camera, width, height, frame)
    }

    pub(crate) fn validate_cpu_post_projection_context(
        &self,
        owner: &GpuOwnerToken,
        camera: &gsplat_core::Camera,
        width: u32,
        height: u32,
        frame: FrameIdentity,
    ) -> Result<GpuPreparationReceipt, GpuPreparationError> {
        let gpu = self.gpu.as_ref().ok_or(GpuPreparationError::Unavailable)?;
        gpu.validate_cpu_post_projection_context(owner, camera, width, height, frame)
    }

    pub(crate) fn encode_cpu_post_projection_frame<'scene>(
        &'scene mut self,
        context: GpuExecutionContext<'_>,
        request: CpuPostProjectionRequest<'_>,
    ) -> Result<CpuPostProjectedHandles<'scene>, GpuPreparationError> {
        let gpu = self.gpu.as_mut().ok_or(GpuPreparationError::Unavailable)?;
        gpu.encode_cpu_post_projection_frame(context, request)
    }

    /// Encodes one complete current-frame Exact Preproject graph through the
    /// atomically published device owner. The returned seam contains only
    /// borrowed GPU handles/count sources and immutable currentness receipts;
    /// it owns no raster, submission, readback or cache publication.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn encode_gpu_preproject_frame<'scene>(
        &'scene mut self,
        context: GpuExecutionContext<'_>,
        camera: gsplat_core::Camera,
        width: u32,
        height: u32,
        frame: FrameIdentity,
    ) -> Result<GpuPreprojectHandles<'scene>, GpuPreparationError> {
        let gpu = self.gpu.as_mut().ok_or(GpuPreparationError::Unavailable)?;
        gpu.encode_preproject_frame(context, camera, width, height, frame)
    }

    pub(crate) fn source_count(&self) -> usize {
        self.resident.len()
    }

    pub(crate) fn sh_degree(&self) -> u8 {
        self.resident.sh_degree
    }

    pub(crate) fn positions(&self) -> &[Vec3f] {
        &self.resident.positions
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn resident(&self) -> &ResidentSceneCpu {
        &self.resident
    }
}

#[cfg(test)]
mod tests {
    use gsplat_core::{SceneBuffers, Vec3f};

    use super::SceneRuntime;
    use crate::scene::ResidentSceneCpu;

    fn scene() -> SceneBuffers {
        SceneBuffers {
            positions: vec![Vec3f::new(1.0, 2.0, 3.0)],
            opacity: vec![0.0],
            scale_xyz: vec![[0.0; 3]],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]],
            color_dc: vec![[0.0; 3]],
            sh_degree: 0,
            sh_rest: None,
        }
    }

    #[test]
    fn owns_one_resident_scene_and_borrows_its_positions() {
        let resident = ResidentSceneCpu::encode_owned(scene()).expect("resident scene");
        let positions_ptr = resident.positions.as_ptr();

        let runtime = SceneRuntime::prepare(resident).expect("scene runtime");

        assert_eq!(runtime.positions().as_ptr(), positions_ptr);
        assert_eq!(runtime.source_count(), 1);
        assert_eq!(runtime.sh_degree(), 0);
        assert_eq!(runtime.resident().len(), 1);
        assert!(runtime.gpu_preparation().is_none());
    }

    #[test]
    fn rejects_an_incomplete_resident_scene() {
        let mut resident = ResidentSceneCpu::encode_owned(scene()).expect("resident scene");
        resident.sh_degree = 4;

        assert!(SceneRuntime::prepare(resident).is_err());
    }
}
