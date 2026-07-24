use gsplat_core::Camera;
use thiserror::Error;

use crate::renderer::gpu_prepare::{
    GpuCountSource, GpuPreparationError, GpuPreparationReceipt, GpuPreprojectHandles,
};
use crate::scene::SceneRuntime;

use super::{
    FrameIdentity, GpuCapabilityReceipt, GpuExecutionContext, GpuOwnerToken,
    IndirectCountSemantics, PlanFrameInput, ProjectedWork,
};

#[derive(Debug, Error, PartialEq, Eq)]
pub(crate) enum GpuPreprojectError {
    #[error("GPU Preproject admission contract is inconsistent at {component}")]
    AdmissionContractMismatch { component: &'static str },
    #[error("GPU Preproject frame contract is inconsistent at {component}")]
    FrameContractMismatch { component: &'static str },
    #[error("GPU Preproject encoding failed: {0}")]
    Encoding(#[from] GpuPreparationError),
    #[error("GPU Preproject order generation is exhausted")]
    OrderGenerationExhausted,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct GpuPreprojectGuard {
    frame: FrameIdentity,
    source_count: u32,
    sh_degree: u8,
    camera: Camera,
    viewport_width: u32,
    viewport_height: u32,
}

/// Canonical GPU Preproject handoff borrowed from the atomically published
/// scene graph. Numeric V/C/D remain GPU-owned; the two count sources plus the
/// indirect arguments prove the exact `D=C<=V<=S` relationship without a host
/// placeholder or readback.
pub(crate) struct GpuPreprojectWork<'a> {
    owner: GpuOwnerToken,
    handles: GpuPreprojectHandles<'a>,
    guard: GpuPreprojectGuard,
}

#[allow(dead_code)]
impl GpuPreprojectWork<'_> {
    pub(crate) const fn frame_identity(&self) -> FrameIdentity {
        self.guard.frame
    }

    pub(crate) const fn camera(&self) -> Camera {
        self.guard.camera
    }

    pub(crate) const fn viewport(&self) -> (u32, u32) {
        (self.guard.viewport_width, self.guard.viewport_height)
    }

    pub(crate) const fn source_count(&self) -> u32 {
        self.guard.source_count
    }

    pub(crate) const fn sh_degree(&self) -> u8 {
        self.guard.sh_degree
    }

    pub(crate) const fn receipt(&self) -> GpuPreparationReceipt {
        self.handles.receipt().preparation()
    }

    pub(crate) fn same_owner(&self, owner: &GpuOwnerToken) -> bool {
        self.owner.same_owner(owner)
    }

    pub(crate) const fn count_semantics(&self) -> IndirectCountSemantics {
        IndirectCountSemantics::DrawEqualsContributor
    }

    pub(crate) const fn ordered_source_ids(&self) -> &wgpu::Buffer {
        self.handles.ordered_source_ids()
    }

    pub(crate) const fn indirect_args(&self) -> &wgpu::Buffer {
        self.handles.indirect_args()
    }

    pub(crate) const fn projected_center_alpha_key(&self) -> &wgpu::Buffer {
        self.handles.projected_center_alpha_key()
    }

    pub(crate) const fn projected_axes(&self) -> &wgpu::Buffer {
        self.handles.projected_axes()
    }

    pub(crate) const fn resolved_color(&self) -> &wgpu::Buffer {
        self.handles.resolved_color()
    }

    pub(crate) const fn candidate_count(&self) -> GpuCountSource<'_> {
        self.handles.candidate_count()
    }

    pub(crate) const fn contributor_count(&self) -> GpuCountSource<'_> {
        self.handles.contributor_count()
    }
}

/// One complete prepared GPU Preproject plan. Primitive compute resources are
/// created in the accepted atomic GPU scene transaction; this plan freezes the
/// admitted Exact capability, validates complete currentness, invokes only the
/// SceneRuntime Preproject seam, and returns canonical ProjectedWork.
pub(super) struct GpuPreprojectPlan {
    capability: GpuCapabilityReceipt,
    order_generation: u64,
}

impl GpuPreprojectPlan {
    pub(super) fn prepare(capability: GpuCapabilityReceipt) -> Result<Self, GpuPreprojectError> {
        validate_complete_capability(capability)?;
        Ok(Self {
            capability,
            order_generation: 0,
        })
    }

    pub(super) const fn capability(&self) -> GpuCapabilityReceipt {
        self.capability
    }

    pub(super) fn execute<'scene>(
        &mut self,
        scene: &'scene mut SceneRuntime,
        input: PlanFrameInput<'_>,
        execution: GpuExecutionContext<'_>,
    ) -> Result<ProjectedWork<'scene>, GpuPreprojectError> {
        let guard = GpuPreprojectGuard {
            frame: input.frame,
            source_count: input.source_count,
            sh_degree: scene.sh_degree(),
            camera: *input.camera,
            viewport_width: input.viewport_width,
            viewport_height: input.viewport_height,
        };
        self.validate_frame(scene, guard)?;
        let next_generation = self
            .order_generation
            .checked_add(1)
            .ok_or(GpuPreprojectError::OrderGenerationExhausted)?;
        let owner = execution.owner_token().clone();

        // Encode every leaf on every call. The caller may discard its encoder,
        // so encode success is deliberately not a cache-publication boundary.
        let handles = scene.encode_gpu_preproject_frame(
            execution,
            guard.camera,
            guard.viewport_width,
            guard.viewport_height,
            guard.frame,
        )?;
        validate_encoded_handles(self.capability, guard, &handles)?;

        self.order_generation = next_generation;
        Ok(ProjectedWork::from_gpu_preproject(
            guard.frame,
            next_generation,
            guard.source_count,
            GpuPreprojectWork {
                owner,
                handles,
                guard,
            },
        ))
    }

    fn validate_frame(
        &self,
        scene: &SceneRuntime,
        guard: GpuPreprojectGuard,
    ) -> Result<(), GpuPreprojectError> {
        validate_complete_capability(self.capability)?;
        guard
            .camera
            .validate()
            .map_err(|_| GpuPreprojectError::FrameContractMismatch {
                component: "camera",
            })?;
        if scene.source_count() != guard.source_count as usize
            || self.capability.source_count() != guard.source_count
        {
            return Err(GpuPreprojectError::FrameContractMismatch {
                component: "source count",
            });
        }
        if scene.sh_degree() != self.capability.sh_degree() {
            return Err(GpuPreprojectError::FrameContractMismatch {
                component: "SH degree",
            });
        }
        if guard.frame.scene_generation() != self.capability.scene_generation()
            || guard.frame.contract_generation() != self.capability.contract_generation()
            || guard.frame.plan_set_generation() != self.capability.plan_set_generation()
        {
            return Err(GpuPreprojectError::FrameContractMismatch {
                component: "scene/contract/plan-set generation",
            });
        }
        if guard.viewport_width == 0 || guard.viewport_height == 0 {
            return Err(GpuPreprojectError::FrameContractMismatch {
                component: "viewport",
            });
        }
        let prepared =
            scene
                .gpu_preparation()
                .ok_or(GpuPreprojectError::FrameContractMismatch {
                    component: "complete Preproject graph",
                })?;
        validate_preparation_receipt(self.capability, prepared)
    }
}

fn validate_complete_capability(
    capability: GpuCapabilityReceipt,
) -> Result<(), GpuPreprojectError> {
    if capability.source_count() != capability.capacity()
        || capability.source_count() != capability.resident_count()
        || capability.source_count() != capability.addressable_count()
    {
        return Err(GpuPreprojectError::AdmissionContractMismatch {
            component: "source/capacity/resident/addressable count",
        });
    }
    if capability.sh_degree() > 3 {
        return Err(GpuPreprojectError::AdmissionContractMismatch {
            component: "SH degree",
        });
    }
    Ok(())
}

fn validate_preparation_receipt(
    capability: GpuCapabilityReceipt,
    receipt: GpuPreparationReceipt,
) -> Result<(), GpuPreprojectError> {
    if receipt.source_count() != capability.source_count()
        || receipt.capacity() != capability.capacity()
        || receipt.resident_count() != capability.resident_count()
        || receipt.addressable_count() != capability.addressable_count()
        || receipt.sh_degree() != capability.sh_degree()
        || receipt.scene_generation() != capability.scene_generation()
        || receipt.contract_generation() != capability.contract_generation()
        || receipt.plan_set_generation() != capability.plan_set_generation()
    {
        return Err(GpuPreprojectError::FrameContractMismatch {
            component: "prepared scene receipt",
        });
    }
    if !receipt.preproject_compute() {
        return Err(GpuPreprojectError::FrameContractMismatch {
            component: "complete Preproject graph",
        });
    }
    Ok(())
}

fn validate_encoded_handles(
    capability: GpuCapabilityReceipt,
    guard: GpuPreprojectGuard,
    handles: &GpuPreprojectHandles<'_>,
) -> Result<(), GpuPreprojectError> {
    let receipt = handles.receipt();
    if receipt.frame_identity() != guard.frame
        || receipt.camera() != guard.camera
        || receipt.viewport() != (guard.viewport_width, guard.viewport_height)
    {
        return Err(GpuPreprojectError::FrameContractMismatch {
            component: "encoded work guard",
        });
    }
    validate_preparation_receipt(capability, receipt.preparation())?;
    for count in [handles.candidate_count(), handles.contributor_count()] {
        if count.offset() % 4 != 0
            || count.offset().checked_add(4).is_none()
            || count.offset() + 4 > count.buffer().size()
        {
            return Err(GpuPreprojectError::FrameContractMismatch {
                component: "GPU count source",
            });
        }
    }
    if handles.indirect_args().size() < 16 {
        return Err(GpuPreprojectError::FrameContractMismatch {
            component: "exact indirect draw arguments",
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests;
