use gsplat_core::Camera;
use thiserror::Error;

use crate::renderer::gpu_prepare::{
    GpuPreparationError, GpuPreparationReceipt, GpuProjectedHandles,
};
use crate::scene::SceneRuntime;

use super::{
    FrameIdentity, GpuCapabilityReceipt, GpuExecutionContext, GpuOwnerToken,
    IndirectCountSemantics, PlanFrameInput, ProjectedWork,
};

#[derive(Debug, Error, PartialEq, Eq)]
pub(crate) enum GpuPostSortError {
    #[error("GPU PostSort admission contract is inconsistent at {component}")]
    AdmissionContractMismatch { component: &'static str },
    #[error("GPU PostSort frame contract is inconsistent at {component}")]
    FrameContractMismatch { component: &'static str },
    #[error("GPU PostSort encoding failed: {0}")]
    Encoding(#[from] GpuPreparationError),
    #[error("GPU PostSort order generation is exhausted")]
    OrderGenerationExhausted,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct GpuPostSortGuard {
    frame: FrameIdentity,
    source_count: u32,
    sh_degree: u8,
    camera: Camera,
    viewport_width: u32,
    viewport_height: u32,
}

/// Canonical GPU PostSort handoff. The buffers remain owned by the staged
/// scene graph, and the cloned token records which renderer-owned execution
/// capability SceneRuntime accepted before returning these handles.
pub(crate) struct GpuPostSortWork<'a> {
    owner: GpuOwnerToken,
    handles: GpuProjectedHandles<'a>,
    guard: GpuPostSortGuard,
}

#[allow(dead_code)]
impl GpuPostSortWork<'_> {
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
        self.handles.receipt()
    }

    pub(crate) fn same_owner(&self, owner: &GpuOwnerToken) -> bool {
        self.owner.same_owner(owner)
    }

    pub(crate) const fn count_semantics(&self) -> IndirectCountSemantics {
        IndirectCountSemantics::DrawEqualsVisible
    }

    pub(crate) const fn ordered_source_ids(&self) -> &wgpu::Buffer {
        self.handles.ordered_source_ids()
    }

    pub(crate) const fn indirect_args(&self) -> &wgpu::Buffer {
        self.handles.indirect_args()
    }

    pub(crate) const fn projected_center_source(&self) -> &wgpu::Buffer {
        self.handles.projected_center_source()
    }

    pub(crate) const fn projected_axes(&self) -> &wgpu::Buffer {
        self.handles.projected_axes()
    }

    pub(crate) const fn resolved_color(&self) -> &wgpu::Buffer {
        self.handles.resolved_color()
    }
}

/// Complete prepared GPU PostSort plan. Resource creation stays in E8a's
/// transactional scene candidate; this owner freezes the admitted capability,
/// validates every execution against it, sequences the existing exact GPU
/// leaves, and creates canonical projected work.
pub(super) struct GpuPostSortPlan {
    capability: GpuCapabilityReceipt,
    order_generation: u64,
}

impl GpuPostSortPlan {
    pub(super) fn prepare(capability: GpuCapabilityReceipt) -> Result<Self, GpuPostSortError> {
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
    ) -> Result<ProjectedWork<'scene>, GpuPostSortError> {
        let guard = GpuPostSortGuard {
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
            .ok_or(GpuPostSortError::OrderGenerationExhausted)?;
        let owner = execution.owner_token().clone();

        // Deliberately encode the complete sequence every time. The caller
        // owns submission and may discard this encoder, so a successful encode
        // is not a cache-publication boundary. SceneRuntime validates this
        // token against the atomically staged GPU graph before encoding.
        let handles = scene.encode_gpu_frame(
            execution,
            guard.camera,
            guard.viewport_width,
            guard.viewport_height,
            guard.frame,
        )?;
        validate_encoded_handles(self.capability, guard, &handles)?;

        self.order_generation = next_generation;
        Ok(ProjectedWork::from_gpu_post_sort(
            guard.frame,
            next_generation,
            guard.source_count,
            GpuPostSortWork {
                owner,
                handles,
                guard,
            },
        ))
    }

    fn validate_frame(
        &self,
        scene: &SceneRuntime,
        guard: GpuPostSortGuard,
    ) -> Result<(), GpuPostSortError> {
        validate_complete_capability(self.capability)?;
        guard
            .camera
            .validate()
            .map_err(|_| GpuPostSortError::FrameContractMismatch {
                component: "camera",
            })?;
        if scene.source_count() != guard.source_count as usize
            || self.capability.source_count() != guard.source_count
        {
            return Err(GpuPostSortError::FrameContractMismatch {
                component: "source count",
            });
        }
        if scene.sh_degree() != self.capability.sh_degree() {
            return Err(GpuPostSortError::FrameContractMismatch {
                component: "SH degree",
            });
        }
        if guard.frame.scene_generation() != self.capability.scene_generation()
            || guard.frame.contract_generation() != self.capability.contract_generation()
            || guard.frame.plan_set_generation() != self.capability.plan_set_generation()
        {
            return Err(GpuPostSortError::FrameContractMismatch {
                component: "scene/contract/plan-set generation",
            });
        }
        if guard.viewport_width == 0 || guard.viewport_height == 0 {
            return Err(GpuPostSortError::FrameContractMismatch {
                component: "viewport",
            });
        }
        let prepared = scene
            .gpu_preparation()
            .ok_or(GpuPostSortError::FrameContractMismatch {
                component: "device-owned scene graph",
            })?;
        validate_preparation_receipt(self.capability, prepared)
    }
}

fn validate_complete_capability(capability: GpuCapabilityReceipt) -> Result<(), GpuPostSortError> {
    if capability.source_count() != capability.capacity()
        || capability.source_count() != capability.resident_count()
        || capability.source_count() != capability.addressable_count()
    {
        return Err(GpuPostSortError::AdmissionContractMismatch {
            component: "source/capacity/resident/addressable count",
        });
    }
    if capability.sh_degree() > 3 {
        return Err(GpuPostSortError::AdmissionContractMismatch {
            component: "SH degree",
        });
    }
    Ok(())
}

fn validate_preparation_receipt(
    capability: GpuCapabilityReceipt,
    receipt: GpuPreparationReceipt,
) -> Result<(), GpuPostSortError> {
    if receipt.source_count() != capability.source_count()
        || receipt.capacity() != capability.capacity()
        || receipt.resident_count() != capability.resident_count()
        || receipt.addressable_count() != capability.addressable_count()
        || receipt.sh_degree() != capability.sh_degree()
        || receipt.scene_generation() != capability.scene_generation()
        || receipt.contract_generation() != capability.contract_generation()
        || receipt.plan_set_generation() != capability.plan_set_generation()
    {
        return Err(GpuPostSortError::FrameContractMismatch {
            component: "prepared scene receipt",
        });
    }
    Ok(())
}

fn validate_encoded_handles(
    capability: GpuCapabilityReceipt,
    guard: GpuPostSortGuard,
    handles: &GpuProjectedHandles<'_>,
) -> Result<(), GpuPostSortError> {
    if handles.frame_identity() != guard.frame {
        return Err(GpuPostSortError::FrameContractMismatch {
            component: "encoded work generation",
        });
    }
    validate_preparation_receipt(capability, handles.receipt())
}

#[cfg(test)]
mod tests;
