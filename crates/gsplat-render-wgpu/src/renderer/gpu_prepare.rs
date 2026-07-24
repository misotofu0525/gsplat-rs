//! Transactional device preparation for the shadow Exact runtime.
//!
//! This module prepares only device-owned Resident, resolved-color, PostSort
//! order/rank-projection and dormant Preproject compute resources. It does not
//! admit Preproject as a plan, select policy, create a target/raster, acquire
//! an adapter, create a device, submit, poll, map, read back or present. The
//! later concrete GPU plans own orchestration and the renderer remains the
//! sole semantic-generation and result owner.

use std::sync::Arc;

use bytemuck::bytes_of;
use gsplat_core::Camera;
use thiserror::Error;

use crate::gpu::{ProjectedRankProjector, ProjectedRankSourceBindings};
use crate::plans::{FrameIdentity, GpuExecutionContext, GpuOwnerToken};
use crate::preproject_gpu::PreprojectedGpuCompute;
use crate::raster::QUAD_VERTEX_COUNT;
use crate::resident_gpu::{
    ResidentGpuResources, create_resident_color_bind_group_layout, create_resident_color_pipeline,
    create_resident_draw_bind_group_layout,
};
use crate::scene::{ResidentGpuBytePlan, ResidentSceneCpu};
use crate::{ResidentGpuError, make_surface_render_params};

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub(crate) enum GpuPreparationError {
    #[error("Exact GPU runtime resources are unavailable")]
    Unavailable,
    #[error("Exact GPU scene/project preparation failed: {0}")]
    Resource(#[from] ResidentGpuError),
    #[error("Exact GPU scene/project allocation failed: {0}")]
    OutOfMemory(String),
    #[error("Exact GPU scene/project validation failed: {0}")]
    Validation(String),
    #[error("Exact GPU scene/project device failure: {0}")]
    Internal(String),
    #[error("prepared GPU capacity/count/SH contract is inconsistent at {component}")]
    ExactContractMismatch { component: &'static str },
    #[error("GPU frame belongs to a stale runtime generation")]
    StaleRuntimeGeneration,
    #[error("GPU runtime candidate belongs to a different renderer-owned execution context")]
    ExecutionOwnerMismatch,
    #[error("renderer already has a bound GPU execution owner")]
    ExecutionOwnerAlreadyBound,
    #[error("GPU plan-set generation cannot move backward from {prepared} to {requested}")]
    PlanSetGenerationRegression { prepared: u64, requested: u64 },
    #[error("GPU frame camera is invalid")]
    InvalidCamera,
    #[error("GPU frame viewport is invalid: {width}x{height}")]
    InvalidViewport { width: u32, height: u32 },
    #[error("CPU PostSort order length {visible_count} exceeds prepared GPU capacity {capacity}")]
    CpuOrderCapacityExceeded { visible_count: usize, capacity: u32 },
    #[error(
        "CPU PostSort source ID {source_id} at rank {rank} exceeds addressable count {addressable_count}"
    )]
    CpuOrderSourceIdOutOfRange {
        rank: usize,
        source_id: u32,
        addressable_count: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SceneResourceGeneration {
    scene: u64,
    contract: u64,
}

impl SceneResourceGeneration {
    const fn from_frame(frame: FrameIdentity) -> Self {
        Self {
            scene: frame.scene_generation(),
            contract: frame.contract_generation(),
        }
    }

    const fn accepts(self, frame: FrameIdentity) -> bool {
        self.scene == frame.scene_generation() && self.contract == frame.contract_generation()
    }
}

/// Immutable proof that one complete CPU scene has one complete device-owned
/// counterpart. Every count is explicit so capacity cannot masquerade as
/// source, resident or addressable membership.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GpuPreparationReceipt {
    source_count: u32,
    capacity: u32,
    resident_count: u32,
    addressable_count: u32,
    sh_degree: u8,
    preproject_compute: bool,
    scene_resource_generation: SceneResourceGeneration,
    plan_set_generation: u64,
}

impl GpuPreparationReceipt {
    pub(crate) const fn source_count(self) -> u32 {
        self.source_count
    }

    pub(crate) const fn capacity(self) -> u32 {
        self.capacity
    }

    pub(crate) const fn resident_count(self) -> u32 {
        self.resident_count
    }

    pub(crate) const fn addressable_count(self) -> u32 {
        self.addressable_count
    }

    pub(crate) const fn sh_degree(self) -> u8 {
        self.sh_degree
    }

    pub(crate) const fn preproject_compute(self) -> bool {
        self.preproject_compute
    }

    pub(crate) const fn scene_generation(self) -> u64 {
        self.scene_resource_generation.scene
    }

    pub(crate) const fn contract_generation(self) -> u64 {
        self.scene_resource_generation.contract
    }

    pub(crate) const fn plan_set_generation(self) -> u64 {
        self.plan_set_generation
    }

    pub(crate) const fn accepts(self, frame: FrameIdentity) -> bool {
        self.scene_resource_generation.accepts(frame)
            && self.plan_set_generation == frame.plan_set_generation()
    }
}

/// Collision-free owner of one existing renderer device/queue pair.
pub(crate) struct GpuExecutionOwner {
    token: GpuOwnerToken,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
}

impl GpuExecutionOwner {
    pub(super) fn new(device: &Arc<wgpu::Device>, queue: &Arc<wgpu::Queue>) -> Self {
        Self {
            token: GpuOwnerToken::fresh(),
            device: Arc::clone(device),
            queue: Arc::clone(queue),
        }
    }

    fn device(&self) -> &wgpu::Device {
        &self.device
    }

    fn token(&self) -> &GpuOwnerToken {
        &self.token
    }

    pub(super) fn context<'a>(
        &'a self,
        queue: &'a Arc<wgpu::Queue>,
        encoder: &'a mut wgpu::CommandEncoder,
    ) -> Result<GpuExecutionContext<'a>, GpuPreparationError> {
        if !Arc::ptr_eq(&self.queue, queue) {
            return Err(GpuPreparationError::ExecutionOwnerMismatch);
        }
        Ok(GpuExecutionContext::new(&self.token, queue, encoder))
    }
}

/// The only complete device-owned scene/project candidate held by
/// `SceneRuntime`. PostSort and dormant Preproject are constructed within the
/// same fallible candidate. The PostSort projector borrows its sorter's final
/// IDs/indirect arguments; Preproject owns its source-order compaction counts
/// and stable-prefix sorter without any target-format raster state.
pub(crate) struct GpuScenePreparation {
    owner: GpuOwnerToken,
    resident: ResidentGpuResources,
    color_pipeline: wgpu::ComputePipeline,
    projector: ProjectedRankProjector,
    gpu_project_bind_group: wgpu::BindGroup,
    preproject: PreprojectedGpuCompute,
    receipt: GpuPreparationReceipt,
    max_workgroups_per_dimension: u32,
    #[cfg(test)]
    color_encode_count: u64,
    #[cfg(test)]
    preproject_encode_count: u64,
    #[cfg(test)]
    cpu_post_projection_encode_count: u64,
}

pub(crate) struct CpuPostProjectionRequest<'a> {
    ordered_ids: &'a [u32],
    camera: Camera,
    viewport: (u32, u32),
    frame: FrameIdentity,
    order_generation: u64,
}

impl<'a> CpuPostProjectionRequest<'a> {
    pub(crate) const fn new(
        ordered_ids: &'a [u32],
        camera: Camera,
        viewport: (u32, u32),
        frame: FrameIdentity,
        order_generation: u64,
    ) -> Self {
        Self {
            ordered_ids,
            camera,
            viewport,
            frame,
            order_generation,
        }
    }
}

impl GpuScenePreparation {
    pub(crate) async fn prepare(
        owner: &GpuExecutionOwner,
        scene: &ResidentSceneCpu,
        generation: FrameIdentity,
    ) -> Result<Self, GpuPreparationError> {
        let device = owner.device();
        validate_adapter_capacity(
            scene.len(),
            scene.sh_degree,
            scene.sh_plane_count(),
            &device.limits(),
        )?;

        let (validation_scope, oom_scope, internal_scope) = (
            device.push_error_scope(wgpu::ErrorFilter::Validation),
            device.push_error_scope(wgpu::ErrorFilter::OutOfMemory),
            device.push_error_scope(wgpu::ErrorFilter::Internal),
        );
        let candidate = Self::create_candidate(owner, scene, generation);
        let internal = internal_scope.pop().await.map(|error| error.to_string());
        let out_of_memory = oom_scope.pop().await.map(|error| error.to_string());
        let validation = validation_scope.pop().await.map(|error| error.to_string());
        if let Some(error) = classify_scope_errors(internal, out_of_memory, validation) {
            return Err(error);
        }
        candidate
    }

    fn create_candidate(
        owner: &GpuExecutionOwner,
        scene: &ResidentSceneCpu,
        generation: FrameIdentity,
    ) -> Result<Self, GpuPreparationError> {
        let device = owner.device();
        let source_count = u32::try_from(scene.len())
            .map_err(|_| GpuPreparationError::Resource(ResidentGpuError::AddressSpaceExceeded))?;
        let draw_layout = create_resident_draw_bind_group_layout(device);
        let color_layout = create_resident_color_bind_group_layout(device);
        let color_pipeline = create_resident_color_pipeline(device, &color_layout);
        let mut resident = ResidentGpuResources::new(device, &draw_layout, &color_layout, scene)?;
        let order = resident.create_gpu_order_candidate(device, &draw_layout)?;
        let projector = ProjectedRankProjector::new(
            device,
            source_count,
            QUAD_VERTEX_COUNT,
            &resident.order_buffer,
            project_source_bindings(&resident),
        )?;
        let gpu_project_bind_group = projector.create_external_bind_group(
            device,
            project_source_bindings(&resident),
            order.sorter.final_ids(),
            order.sorter.indirect_args(),
        );
        resident.publish_gpu_order(order);
        let preproject = PreprojectedGpuCompute::new(device, &resident, QUAD_VERTEX_COUNT)?;

        let receipt =
            validate_complete_receipt(scene, &resident, &projector, &preproject, generation)?;
        Ok(Self {
            owner: owner.token().clone(),
            resident,
            color_pipeline,
            projector,
            gpu_project_bind_group,
            preproject,
            receipt,
            max_workgroups_per_dimension: device.limits().max_compute_workgroups_per_dimension,
            #[cfg(test)]
            color_encode_count: 0,
            #[cfg(test)]
            preproject_encode_count: 0,
            #[cfg(test)]
            cpu_post_projection_encode_count: 0,
        })
    }

    pub(crate) const fn receipt(&self) -> GpuPreparationReceipt {
        self.receipt
    }

    /// Existing device resources may be rebound only to a newer PlanSet from
    /// the same renderer-owned scene/contract generation and the same stable
    /// owner capability. Scene and contract generations always require a
    /// fresh whole-runtime candidate.
    #[cfg(test)]
    pub(crate) fn rebind_existing(
        &mut self,
        owner: &GpuExecutionOwner,
        generation: FrameIdentity,
    ) -> Result<GpuPreparationReceipt, GpuPreparationError> {
        if !self.owner.same_owner(owner.token()) {
            return Err(GpuPreparationError::ExecutionOwnerMismatch);
        }
        if !self.receipt.scene_resource_generation.accepts(generation) {
            return Err(GpuPreparationError::StaleRuntimeGeneration);
        }
        let requested = generation.plan_set_generation();
        let prepared = self.receipt.plan_set_generation;
        if requested < prepared {
            return Err(GpuPreparationError::PlanSetGenerationRegression {
                prepared,
                requested,
            });
        }
        self.receipt.plan_set_generation = requested;
        Ok(self.receipt)
    }

    pub(crate) fn encode_frame<'scene>(
        &'scene mut self,
        context: GpuExecutionContext<'_>,
        camera: Camera,
        width: u32,
        height: u32,
        frame: FrameIdentity,
    ) -> Result<GpuProjectedHandles<'scene>, GpuPreparationError> {
        let (owner, queue, encoder) = context.into_parts();
        if !self.owner.same_owner(owner) {
            return Err(GpuPreparationError::ExecutionOwnerMismatch);
        }
        if !self.receipt.accepts(frame) {
            return Err(GpuPreparationError::StaleRuntimeGeneration);
        }
        validate_frame_input(&camera, width, height)?;
        validate_runtime_counts(
            self.receipt,
            &self.resident,
            &self.projector,
            &self.preproject,
        )?;

        let mut params = make_surface_render_params(
            &camera,
            width,
            height,
            self.receipt.source_count,
            u32::from(self.receipt.sh_degree),
        );
        params.order_stride_words = 1;
        params.order_id_offset_words = 0;
        params.source_position_stride_words = 4;
        queue.write_buffer(&self.resident.draw_params_buffer, 0, bytes_of(&params));
        self.order()?
            .sorter
            .set_indirect_vertex_count(queue, QUAD_VERTEX_COUNT);

        self.resident.encode_color_resolve_uncached(
            queue,
            &self.color_pipeline,
            encoder,
            &camera,
            self.max_workgroups_per_dimension,
        )?;
        #[cfg(test)]
        {
            self.color_encode_count += 1;
        }
        let order =
            self.resident
                .gpu_order()
                .ok_or(GpuPreparationError::ExactContractMismatch {
                    component: "sorter publication",
                })?;
        order.sorter.encode(encoder);
        self.projector.encode_external(
            encoder,
            &self.gpu_project_bind_group,
            self.receipt.source_count,
        )?;
        Ok(GpuProjectedHandles {
            frame,
            receipt: self.receipt,
            ordered_source_ids: order.sorter.final_ids(),
            indirect_args: order.sorter.indirect_args(),
            projected_center_source: self.projector.projected_center_source(),
            projected_axes: self.projector.projected_axes(),
            resolved_color: &self.resident.resolved_color_buffer,
        })
    }

    /// Validates the complete CPU PostSort projection context without writing
    /// queue state or encoding commands. Plans use this before refreshing the
    /// authoritative CPU order so a wrong owner or stale frame cannot publish
    /// a new order as a side effect of a rejected GPU projection request.
    pub(crate) fn validate_cpu_post_projection_context(
        &self,
        owner: &GpuOwnerToken,
        camera: &Camera,
        width: u32,
        height: u32,
        frame: FrameIdentity,
    ) -> Result<GpuPreparationReceipt, GpuPreparationError> {
        if !self.owner.same_owner(owner) {
            return Err(GpuPreparationError::ExecutionOwnerMismatch);
        }
        if !self.receipt.accepts(frame) {
            return Err(GpuPreparationError::StaleRuntimeGeneration);
        }
        validate_frame_input(camera, width, height)?;
        validate_runtime_counts(
            self.receipt,
            &self.resident,
            &self.projector,
            &self.preproject,
        )?;
        Ok(self.receipt)
    }

    /// Uploads the authoritative visible CPU IDs and encodes the existing
    /// Resident color plus CPU-bound rank projection into the caller's
    /// encoder. Every call rewrites the direct D=V count and re-encodes all
    /// GPU work; successful encoding publishes no cache or generation.
    pub(crate) fn encode_cpu_post_projection_frame<'scene>(
        &'scene mut self,
        context: GpuExecutionContext<'_>,
        request: CpuPostProjectionRequest<'_>,
    ) -> Result<CpuPostProjectedHandles<'scene>, GpuPreparationError> {
        let (owner, queue, encoder) = context.into_parts();
        let (width, height) = request.viewport;
        self.validate_cpu_post_projection_context(
            owner,
            &request.camera,
            width,
            height,
            request.frame,
        )?;
        validate_cpu_order(self.receipt, request.ordered_ids)?;

        let visible_count = self.resident.prepare_cpu_order(
            queue,
            request.ordered_ids,
            &request.camera,
            width,
            height,
            true,
        )?;
        self.projector
            .write_cpu_draw_args(queue, QUAD_VERTEX_COUNT, visible_count, false);
        self.resident.encode_color_resolve_uncached(
            queue,
            &self.color_pipeline,
            encoder,
            &request.camera,
            self.max_workgroups_per_dimension,
        )?;
        #[cfg(test)]
        {
            self.color_encode_count += 1;
        }
        self.projector.encode_cpu(encoder, visible_count)?;
        #[cfg(test)]
        {
            self.cpu_post_projection_encode_count += 1;
        }

        Ok(CpuPostProjectedHandles {
            receipt: CpuPostProjectionFrameReceipt {
                frame: request.frame,
                preparation: self.receipt,
                camera: request.camera,
                viewport_width: width,
                viewport_height: height,
                order_generation: request.order_generation,
                visible_count,
            },
            ordered_source_ids: &self.resident.order_buffer,
            projected_center_source: self.projector.projected_center_source(),
            projected_axes: self.projector.projected_axes(),
            resolved_color: &self.resident.resolved_color_buffer,
            projection_count_guard: self.projector.cpu_draw_args(),
        })
    }

    fn order(&self) -> Result<&crate::resident_gpu::ResidentGpuSceneOrder, GpuPreparationError> {
        self.resident
            .gpu_order()
            .ok_or(GpuPreparationError::ExactContractMismatch {
                component: "sorter publication",
            })
    }

    #[cfg(test)]
    pub(crate) const fn color_encode_count(&self) -> u64 {
        self.color_encode_count
    }

    /// Encodes the complete current-frame Exact Preproject graph. Every call
    /// resolves color and recomputes all source projections, V/C scans,
    /// stable compaction and full32 order; encode success publishes no cache.
    pub(crate) fn encode_preproject_frame<'scene>(
        &'scene mut self,
        context: GpuExecutionContext<'_>,
        camera: Camera,
        width: u32,
        height: u32,
        frame: FrameIdentity,
    ) -> Result<GpuPreprojectHandles<'scene>, GpuPreparationError> {
        let (owner, queue, encoder) = context.into_parts();
        if !self.owner.same_owner(owner) {
            return Err(GpuPreparationError::ExecutionOwnerMismatch);
        }
        if !self.receipt.accepts(frame) {
            return Err(GpuPreparationError::StaleRuntimeGeneration);
        }
        validate_frame_input(&camera, width, height)?;
        validate_runtime_counts(
            self.receipt,
            &self.resident,
            &self.projector,
            &self.preproject,
        )?;

        self.resident.encode_color_resolve_uncached(
            queue,
            &self.color_pipeline,
            encoder,
            &camera,
            self.max_workgroups_per_dimension,
        )?;
        self.preproject
            .encode(queue, encoder, &self.resident, &camera, width, height);
        #[cfg(test)]
        {
            self.color_encode_count += 1;
            self.preproject_encode_count += 1;
        }

        let (candidate_count, candidate_count_offset) =
            self.preproject.candidate_count_buffer_and_offset();
        let (contributor_count, contributor_count_offset) =
            self.preproject.contributor_count_buffer_and_offset();
        Ok(GpuPreprojectHandles {
            receipt: GpuPreprojectFrameReceipt {
                frame,
                preparation: self.receipt,
                camera,
                viewport_width: width,
                viewport_height: height,
            },
            ordered_source_ids: self.preproject.final_source_ids(),
            indirect_args: self.preproject.draw_args(),
            projected_center_alpha_key: self.preproject.source_center_alpha_key(),
            projected_axes: self.preproject.source_axes(),
            resolved_color: &self.resident.resolved_color_buffer,
            candidate_count: GpuCountSource {
                buffer: candidate_count,
                offset: candidate_count_offset,
            },
            contributor_count: GpuCountSource {
                buffer: contributor_count,
                offset: contributor_count_offset,
            },
        })
    }

    #[cfg(test)]
    pub(crate) const fn preproject_encode_count(&self) -> u64 {
        self.preproject_encode_count
    }

    #[cfg(test)]
    pub(crate) const fn cpu_post_projection_encode_count(&self) -> u64 {
        self.cpu_post_projection_encode_count
    }
}

/// Immutable currentness and direct-count proof for one CPU PostSort
/// projection encode. The direct count is authoritative D=V; no contributor
/// count is inferred from the projection shader's private workspace.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CpuPostProjectionFrameReceipt {
    frame: FrameIdentity,
    preparation: GpuPreparationReceipt,
    camera: Camera,
    viewport_width: u32,
    viewport_height: u32,
    order_generation: u64,
    visible_count: u32,
}

impl CpuPostProjectionFrameReceipt {
    pub(crate) const fn frame_identity(self) -> FrameIdentity {
        self.frame
    }

    pub(crate) const fn preparation(self) -> GpuPreparationReceipt {
        self.preparation
    }

    pub(crate) const fn camera(self) -> Camera {
        self.camera
    }

    pub(crate) const fn viewport(self) -> (u32, u32) {
        (self.viewport_width, self.viewport_height)
    }

    pub(crate) const fn order_generation(self) -> u64 {
        self.order_generation
    }

    pub(crate) const fn visible_count(self) -> u32 {
        self.visible_count
    }
}

/// Borrowed GPU completion of the same CPU PostSort plan. Buffers remain
/// owned by the atomically published SceneRuntime graph, while the explicit
/// direct count is the later canonical raster's D=V source.
pub(crate) struct CpuPostProjectedHandles<'a> {
    receipt: CpuPostProjectionFrameReceipt,
    ordered_source_ids: &'a wgpu::Buffer,
    projected_center_source: &'a wgpu::Buffer,
    projected_axes: &'a wgpu::Buffer,
    resolved_color: &'a wgpu::Buffer,
    projection_count_guard: &'a wgpu::Buffer,
}

impl CpuPostProjectedHandles<'_> {
    pub(crate) const fn receipt(&self) -> CpuPostProjectionFrameReceipt {
        self.receipt
    }

    pub(crate) const fn ordered_source_ids(&self) -> &wgpu::Buffer {
        self.ordered_source_ids
    }

    pub(crate) const fn projected_center_source(&self) -> &wgpu::Buffer {
        self.projected_center_source
    }

    pub(crate) const fn projected_axes(&self) -> &wgpu::Buffer {
        self.projected_axes
    }

    pub(crate) const fn resolved_color(&self) -> &wgpu::Buffer {
        self.resolved_color
    }

    pub(crate) const fn projection_count_guard(&self) -> &wgpu::Buffer {
        self.projection_count_guard
    }
}

/// GPU-owned count source. The host receives the buffer identity and byte
/// offset only; no numeric V/C/D value is fabricated or read back here.
#[derive(Clone, Copy)]
pub(crate) struct GpuCountSource<'a> {
    buffer: &'a wgpu::Buffer,
    offset: u64,
}

impl GpuCountSource<'_> {
    pub(crate) const fn buffer(&self) -> &wgpu::Buffer {
        self.buffer
    }

    pub(crate) const fn offset(&self) -> u64 {
        self.offset
    }
}

/// Immutable currentness proof bound to one complete Preproject encode.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GpuPreprojectFrameReceipt {
    frame: FrameIdentity,
    preparation: GpuPreparationReceipt,
    camera: Camera,
    viewport_width: u32,
    viewport_height: u32,
}

impl GpuPreprojectFrameReceipt {
    pub(crate) const fn frame_identity(self) -> FrameIdentity {
        self.frame
    }

    pub(crate) const fn preparation(self) -> GpuPreparationReceipt {
        self.preparation
    }

    pub(crate) const fn camera(self) -> Camera {
        self.camera
    }

    pub(crate) const fn viewport(self) -> (u32, u32) {
        (self.viewport_width, self.viewport_height)
    }
}

/// Accessor-only handoff for the later plans-only E9 writer. Every buffer is
/// borrowed from the atomically published SceneRuntime GPU graph.
pub(crate) struct GpuPreprojectHandles<'a> {
    receipt: GpuPreprojectFrameReceipt,
    ordered_source_ids: &'a wgpu::Buffer,
    indirect_args: &'a wgpu::Buffer,
    projected_center_alpha_key: &'a wgpu::Buffer,
    projected_axes: &'a wgpu::Buffer,
    resolved_color: &'a wgpu::Buffer,
    candidate_count: GpuCountSource<'a>,
    contributor_count: GpuCountSource<'a>,
}

impl GpuPreprojectHandles<'_> {
    pub(crate) const fn receipt(&self) -> GpuPreprojectFrameReceipt {
        self.receipt
    }

    pub(crate) const fn ordered_source_ids(&self) -> &wgpu::Buffer {
        self.ordered_source_ids
    }

    pub(crate) const fn indirect_args(&self) -> &wgpu::Buffer {
        self.indirect_args
    }

    pub(crate) const fn projected_center_alpha_key(&self) -> &wgpu::Buffer {
        self.projected_center_alpha_key
    }

    pub(crate) const fn projected_axes(&self) -> &wgpu::Buffer {
        self.projected_axes
    }

    pub(crate) const fn resolved_color(&self) -> &wgpu::Buffer {
        self.resolved_color
    }

    pub(crate) const fn candidate_count(&self) -> GpuCountSource<'_> {
        self.candidate_count
    }

    pub(crate) const fn contributor_count(&self) -> GpuCountSource<'_> {
        self.contributor_count
    }
}

/// Accessor-only opaque handoff for the later GPU PostSort plan. The indirect
/// arguments remain sorter-owned and no host-visible count is fabricated.
pub(crate) struct GpuProjectedHandles<'a> {
    frame: FrameIdentity,
    receipt: GpuPreparationReceipt,
    ordered_source_ids: &'a wgpu::Buffer,
    indirect_args: &'a wgpu::Buffer,
    projected_center_source: &'a wgpu::Buffer,
    projected_axes: &'a wgpu::Buffer,
    resolved_color: &'a wgpu::Buffer,
}

impl GpuProjectedHandles<'_> {
    pub(crate) const fn frame_identity(&self) -> FrameIdentity {
        self.frame
    }

    pub(crate) const fn receipt(&self) -> GpuPreparationReceipt {
        self.receipt
    }

    pub(crate) const fn ordered_source_ids(&self) -> &wgpu::Buffer {
        self.ordered_source_ids
    }

    pub(crate) const fn indirect_args(&self) -> &wgpu::Buffer {
        self.indirect_args
    }

    pub(crate) const fn projected_center_source(&self) -> &wgpu::Buffer {
        self.projected_center_source
    }

    pub(crate) const fn projected_axes(&self) -> &wgpu::Buffer {
        self.projected_axes
    }

    pub(crate) const fn resolved_color(&self) -> &wgpu::Buffer {
        self.resolved_color
    }
}

pub(crate) fn commit_complete_candidate<T, E>(
    published: &mut Option<T>,
    candidate: Result<T, E>,
) -> Result<(), E> {
    let candidate = candidate?;
    *published = Some(candidate);
    Ok(())
}

fn validate_adapter_capacity(
    source_count: usize,
    sh_degree: u8,
    sh_plane_count: u32,
    limits: &wgpu::Limits,
) -> Result<(), GpuPreparationError> {
    if sh_degree > 3 {
        return Err(GpuPreparationError::ExactContractMismatch {
            component: "SH degree",
        });
    }
    let _ = u32::try_from(source_count)
        .map_err(|_| GpuPreparationError::Resource(ResidentGpuError::AddressSpaceExceeded))?;
    ResidentGpuBytePlan::for_count(source_count, sh_plane_count)?.validate_limits(limits)?;
    Ok(())
}

fn validate_complete_receipt(
    scene: &ResidentSceneCpu,
    resident: &ResidentGpuResources,
    projector: &ProjectedRankProjector,
    preproject: &PreprojectedGpuCompute,
    generation: FrameIdentity,
) -> Result<GpuPreparationReceipt, GpuPreparationError> {
    let source_count = u32::try_from(scene.len())
        .map_err(|_| GpuPreparationError::Resource(ResidentGpuError::AddressSpaceExceeded))?;
    let capacity = u32::try_from(resident.capacity)
        .map_err(|_| GpuPreparationError::Resource(ResidentGpuError::AddressSpaceExceeded))?;
    let receipt = GpuPreparationReceipt {
        source_count,
        capacity,
        resident_count: capacity,
        addressable_count: projector.capacity(),
        sh_degree: scene.sh_degree,
        preproject_compute: true,
        scene_resource_generation: SceneResourceGeneration::from_frame(generation),
        plan_set_generation: generation.plan_set_generation(),
    };
    validate_runtime_counts(receipt, resident, projector, preproject)?;
    Ok(receipt)
}

fn validate_runtime_counts(
    receipt: GpuPreparationReceipt,
    resident: &ResidentGpuResources,
    projector: &ProjectedRankProjector,
    preproject: &PreprojectedGpuCompute,
) -> Result<(), GpuPreparationError> {
    if receipt.source_count != receipt.capacity
        || receipt.source_count != receipt.resident_count
        || receipt.source_count != receipt.addressable_count
        || usize::try_from(receipt.source_count).ok() != Some(resident.capacity)
        || receipt.addressable_count != projector.capacity()
        || receipt.source_count != preproject.capacity()
    {
        return Err(GpuPreparationError::ExactContractMismatch {
            component: "source/capacity/resident/addressable count",
        });
    }
    if u32::from(receipt.sh_degree) != resident.sh_degree || receipt.sh_degree > 3 {
        return Err(GpuPreparationError::ExactContractMismatch {
            component: "SH degree",
        });
    }
    if !receipt.preproject_compute {
        return Err(GpuPreparationError::ExactContractMismatch {
            component: "Preproject compute graph",
        });
    }
    Ok(())
}

fn validate_cpu_order(
    receipt: GpuPreparationReceipt,
    ordered_ids: &[u32],
) -> Result<(), GpuPreparationError> {
    if ordered_ids.len() > receipt.addressable_count as usize {
        return Err(GpuPreparationError::CpuOrderCapacityExceeded {
            visible_count: ordered_ids.len(),
            capacity: receipt.addressable_count,
        });
    }
    if let Some((rank, source_id)) = ordered_ids
        .iter()
        .copied()
        .enumerate()
        .find(|(_, source_id)| *source_id >= receipt.addressable_count)
    {
        return Err(GpuPreparationError::CpuOrderSourceIdOutOfRange {
            rank,
            source_id,
            addressable_count: receipt.addressable_count,
        });
    }
    Ok(())
}

fn validate_frame_input(
    camera: &Camera,
    width: u32,
    height: u32,
) -> Result<(), GpuPreparationError> {
    camera
        .validate()
        .map_err(|_| GpuPreparationError::InvalidCamera)?;
    if width == 0 || height == 0 {
        return Err(GpuPreparationError::InvalidViewport { width, height });
    }
    Ok(())
}

fn classify_scope_errors(
    internal: Option<String>,
    out_of_memory: Option<String>,
    validation: Option<String>,
) -> Option<GpuPreparationError> {
    out_of_memory
        .map(GpuPreparationError::OutOfMemory)
        .or_else(|| internal.map(GpuPreparationError::Internal))
        .or_else(|| validation.map(GpuPreparationError::Validation))
}

fn project_source_bindings(resident: &ResidentGpuResources) -> ProjectedRankSourceBindings<'_> {
    ProjectedRankSourceBindings {
        position_alpha: &resident.position_alpha_buffer,
        covariance0: &resident.covariance0_buffer,
        covariance1: &resident.covariance1_buffer,
        draw_params: &resident.draw_params_buffer,
    }
}

#[cfg(test)]
mod tests {
    use gsplat_core::{SceneBuffers, Vec3f};

    use super::*;
    use crate::plans::{PlanId, PlanSetError, TestGpuAdmissionMode};
    use crate::renderer::frame::Viewport;
    use crate::renderer::{
        FrameExecutionError, GpuPreparationStatus, GpuRuntimePreparationError, PreparedRuntimeSlot,
        execute_frame, execute_frame_gpu,
    };

    const TAIL_COUNT: usize = 129;

    fn frame(scene: u64, camera: u64, viewport: u64) -> FrameIdentity {
        frame_with_plan_set(scene, camera, viewport, 1)
    }

    fn frame_with_plan_set(scene: u64, camera: u64, viewport: u64, plan_set: u64) -> FrameIdentity {
        FrameIdentity::new(scene, camera, viewport, 1, plan_set)
    }

    fn source(count: usize, sh_degree: u8) -> SceneBuffers {
        let coefficients = match sh_degree {
            0 => 0,
            1 => 9,
            2 => 24,
            3 => 45,
            _ => unreachable!("test supports Exact SH0-SH3 only"),
        };
        SceneBuffers {
            positions: (0..count)
                .map(|index| Vec3f::new(index as f32 * 0.01, 0.0, 1.0 + index as f32 * 0.001))
                .collect(),
            opacity: vec![0.0; count],
            scale_xyz: vec![[-3.0; 3]; count],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; count],
            color_dc: vec![[0.0; 3]; count],
            sh_degree,
            sh_rest: (coefficients != 0).then(|| vec![0.0; count * coefficients]),
        }
    }

    fn portable_limits(storage_bindings: u32) -> wgpu::Limits {
        let mut limits = wgpu::Limits::downlevel_defaults();
        limits.max_storage_buffers_per_shader_stage = storage_bindings;
        limits.max_storage_buffer_binding_size = 128 << 20;
        limits.max_buffer_size = 128 << 20;
        limits
    }

    async fn request_device(
        storage_bindings: u32,
    ) -> Option<(
        wgpu::Instance,
        wgpu::Adapter,
        wgpu::AdapterInfo,
        Arc<wgpu::Device>,
        Arc<wgpu::Queue>,
    )> {
        let required = std::env::var_os("GSPLAT_REQUIRE_GPU_PREPARATION").is_some();
        let require_metal = std::env::var_os("GSPLAT_REQUIRE_METAL_GPU_PREPARATION").is_some();
        let instance = wgpu::Instance::default();
        let adapter = match instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
        {
            Ok(adapter) => adapter,
            Err(error) if required || require_metal => {
                panic!("required GPU preparation adapter unavailable: {error}")
            }
            Err(error) => {
                eprintln!("skipping GPU preparation test; adapter unavailable: {error}");
                return None;
            }
        };
        let info = adapter.get_info();
        if require_metal {
            assert_eq!(info.backend, wgpu::Backend::Metal, "Metal adapter required");
        }
        let limits = portable_limits(storage_bindings);
        if !limits.check_limits(&adapter.limits()) {
            if required || require_metal {
                panic!("required GPU preparation limits are unavailable: {limits:?}");
            }
            eprintln!("skipping GPU preparation test; requested limits unavailable");
            return None;
        }
        match adapter
            .request_device(&test_device_descriptor(limits))
            .await
        {
            Ok((device, queue)) => {
                Some((instance, adapter, info, Arc::new(device), Arc::new(queue)))
            }
            Err(error) if required || require_metal => {
                panic!("required GPU preparation device unavailable: {error}")
            }
            Err(error) => {
                eprintln!("skipping GPU preparation test; device unavailable: {error}");
                None
            }
        }
    }

    fn test_device_descriptor(limits: wgpu::Limits) -> wgpu::DeviceDescriptor<'static> {
        wgpu::DeviceDescriptor {
            label: Some("exact-gpu-preparation-test-device"),
            required_features: wgpu::Features::empty(),
            required_limits: limits,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }
    }

    async fn stage_and_commit_scene_gpu(
        runtime: &mut crate::scene::SceneRuntime,
        owner: &GpuExecutionOwner,
        generation: FrameIdentity,
    ) -> Result<GpuPreparationReceipt, GpuPreparationError> {
        let candidate = runtime.stage_gpu(owner, generation).await?;
        let receipt = candidate.receipt();
        runtime.commit_gpu(candidate);
        Ok(receipt)
    }

    #[test]
    fn portable_binding_boundary_is_exact() {
        let limits = portable_limits(8);
        let at_boundary = (128_usize << 20) / 16;
        assert!(validate_adapter_capacity(at_boundary, 3, 4, &limits).is_ok());
        assert!(matches!(
            validate_adapter_capacity(at_boundary + 1, 3, 4, &limits),
            Err(GpuPreparationError::Resource(
                ResidentGpuError::BindingLimitExceeded { .. }
            ))
        ));
        assert!(matches!(
            validate_adapter_capacity(1, 0, 0, &portable_limits(7)),
            Err(GpuPreparationError::Resource(
                ResidentGpuError::StorageBindingCountUnsupported(7)
            ))
        ));
    }

    #[test]
    fn cpu_post_order_upload_rejects_capacity_and_source_id_mismatch() {
        let receipt = GpuPreparationReceipt {
            source_count: 1,
            capacity: 1,
            resident_count: 1,
            addressable_count: 1,
            sh_degree: 0,
            preproject_compute: true,
            scene_resource_generation: SceneResourceGeneration {
                scene: 1,
                contract: 1,
            },
            plan_set_generation: 2,
        };

        assert!(validate_cpu_order(receipt, &[]).is_ok());
        assert!(validate_cpu_order(receipt, &[0]).is_ok());
        assert_eq!(
            validate_cpu_order(receipt, &[0, 0]),
            Err(GpuPreparationError::CpuOrderCapacityExceeded {
                visible_count: 2,
                capacity: 1,
            })
        );
        assert_eq!(
            validate_cpu_order(receipt, &[1]),
            Err(GpuPreparationError::CpuOrderSourceIdOutOfRange {
                rank: 0,
                source_id: 1,
                addressable_count: 1,
            })
        );
    }

    #[test]
    fn candidate_commit_and_scope_failure_are_transactional() {
        let mut published = Some("old");
        let failure: Result<&str, &str> = Err("resource failure");
        assert_eq!(
            commit_complete_candidate(&mut published, failure),
            Err("resource failure")
        );
        assert_eq!(published, Some("old"));
        let complete: Result<&str, &str> = Ok("complete");
        assert!(commit_complete_candidate(&mut published, complete).is_ok());
        assert_eq!(published, Some("complete"));

        assert_eq!(
            classify_scope_errors(
                Some("internal".into()),
                Some("oom".into()),
                Some("validation".into()),
            ),
            Some(GpuPreparationError::OutOfMemory("oom".into()))
        );
        assert_eq!(
            classify_scope_errors(Some("internal".into()), None, Some("validation".into())),
            Some(GpuPreparationError::Internal("internal".into()))
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn optional_resource_failure_preserves_exact_cpu_fallback() {
        pollster::block_on(async {
            let Some((_instance, _adapter, _info, device, queue)) = request_device(7).await else {
                return;
            };
            let resident = ResidentSceneCpu::encode_owned(source(1, 0)).expect("resident scene");
            let mut slot = PreparedRuntimeSlot::prepare(resident).expect("CPU fallback");
            let before = slot.frame_state();

            let status = slot.prepare_gpu_optional(&device, &queue).await;

            assert!(matches!(status, GpuPreparationStatus::Omitted(_)));
            assert!(status.receipt().is_none());
            assert!(status.omission().is_some());
            assert_eq!(slot.fallback(), PlanId::CpuPostSort);
            assert_eq!(slot.eligible(), &[PlanId::CpuPostSort]);
            assert_eq!(slot.frame_state(), before);
            assert_eq!(slot.scene().source_count(), 1);
            assert!(slot.gpu_preparation().is_none());
            assert!(slot.gpu_capability().is_none());
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn staged_plan_failure_publishes_no_scene_plan_generation_or_owner_half() {
        pollster::block_on(async {
            let Some((_instance, _adapter, _info, device, queue)) = request_device(8).await else {
                return;
            };
            let resident = ResidentSceneCpu::encode_owned(source(1, 3)).expect("resident scene");
            let mut slot = PreparedRuntimeSlot::prepare(resident).expect("CPU fallback");
            slot.set_test_gpu_admission_mode(TestGpuAdmissionMode::Fail);
            let before_frame = slot.frame_state();
            let before_eligible = slot.eligible().to_vec();

            assert!(matches!(
                slot.prepare_gpu(&device, &queue).await,
                Err(GpuRuntimePreparationError::PlanSet(
                    PlanSetError::GpuAdmissionRejected {
                        reason: "injected staged-plan failure"
                    }
                ))
            ));
            assert_eq!(slot.frame_state(), before_frame);
            assert_eq!(slot.eligible(), before_eligible);
            assert_eq!(slot.fallback(), PlanId::CpuPostSort);
            assert!(slot.gpu_preparation().is_none());
            assert!(slot.gpu_capability().is_none());
            assert_eq!(slot.scene().gpu_preproject_encode_count(), None);

            let viewport = Viewport::new(64, 64).expect("viewport");
            let work = execute_frame(&mut slot, PlanId::CpuPostSort, &Camera::default(), viewport)
                .expect("CPU fallback remains executable");
            assert_eq!(work.plan_id(), PlanId::CpuPostSort);
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn same_staged_hook_can_atomically_admit_a_test_concrete_gpu_plan() {
        pollster::block_on(async {
            let Some((_instance, _adapter, _info, device, queue)) = request_device(8).await else {
                return;
            };
            let resident = ResidentSceneCpu::encode_owned(source(1, 3)).expect("resident scene");
            let mut slot = PreparedRuntimeSlot::prepare(resident).expect("CPU fallback");
            slot.set_test_gpu_admission_mode(TestGpuAdmissionMode::Concrete);

            let receipt = slot
                .prepare_gpu(&device, &queue)
                .await
                .expect("atomic test concrete admission");
            let capability = slot.gpu_capability().expect("PlanSet capability");

            assert_eq!(receipt.plan_set_generation(), 2);
            assert!(receipt.preproject_compute());
            assert_eq!(capability.plan_set_generation(), 2);
            assert_eq!(slot.frame_state().identity().plan_set_generation(), 2);
            assert_eq!(slot.test_gpu_post_capability(), Some(capability));
            assert_eq!(slot.eligible(), &[PlanId::CpuPostSort, PlanId::GpuPostSort]);
            assert_eq!(slot.fallback(), PlanId::CpuPostSort);
            assert_eq!(slot.gpu_preparation(), Some(receipt));
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn independent_instance_owners_never_alias_and_rebind_only_newer_plan_sets() {
        pollster::block_on(async {
            let Some((_instance_a, _adapter_a, _info_a, device_a, queue_a)) =
                request_device(8).await
            else {
                return;
            };
            let Some((_instance_b, _adapter_b, _info_b, device_b, queue_b)) =
                request_device(8).await
            else {
                return;
            };
            eprintln!(
                "independent wgpu Instance first-device proxy equality: {}",
                device_a == device_b
            );
            let owner_a = GpuExecutionOwner::new(&device_a, &queue_a);
            let owner_b = GpuExecutionOwner::new(&device_b, &queue_b);
            assert!(!owner_a.token().same_owner(owner_b.token()));

            let resident = ResidentSceneCpu::encode_owned(source(1, 3)).expect("resident scene");
            let mut runtime =
                crate::scene::SceneRuntime::prepare(resident).expect("Exact CPU runtime");
            let original =
                stage_and_commit_scene_gpu(&mut runtime, &owner_a, frame_with_plan_set(1, 0, 0, 1))
                    .await
                    .expect("initial GPU candidate");

            assert!(matches!(
                runtime
                    .stage_gpu(&owner_b, frame_with_plan_set(1, 0, 0, 1))
                    .await,
                Err(GpuPreparationError::ExecutionOwnerAlreadyBound)
            ));
            assert_eq!(
                runtime.rebind_gpu(&owner_b, frame_with_plan_set(1, 0, 0, 1)),
                Err(GpuPreparationError::ExecutionOwnerMismatch)
            );
            assert_eq!(runtime.gpu_preparation(), Some(original));
            assert_eq!(
                runtime.rebind_gpu(&owner_a, FrameIdentity::new(2, 0, 0, 1, 1)),
                Err(GpuPreparationError::StaleRuntimeGeneration)
            );
            assert_eq!(runtime.gpu_preparation(), Some(original));
            assert_eq!(
                runtime.rebind_gpu(&owner_a, FrameIdentity::new(1, 0, 0, 2, 1)),
                Err(GpuPreparationError::StaleRuntimeGeneration)
            );
            assert_eq!(runtime.gpu_preparation(), Some(original));

            let rebound_frame = frame_with_plan_set(1, 0, 0, 2);
            let rebound = runtime
                .rebind_gpu(&owner_a, rebound_frame)
                .expect("same-resource plan-set rebind");
            assert_ne!(rebound, original);
            assert!(rebound.accepts(rebound_frame));
            assert!(!rebound.accepts(frame_with_plan_set(1, 0, 0, 1)));
            assert_eq!(
                runtime.rebind_gpu(&owner_a, frame_with_plan_set(1, 0, 0, 1)),
                Err(GpuPreparationError::PlanSetGenerationRegression {
                    prepared: 2,
                    requested: 1,
                })
            );
            assert_eq!(runtime.gpu_preparation(), Some(rebound));

            let mut foreign_encoder =
                device_b.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("foreign-owner-test-encoder"),
                });
            assert!(matches!(
                runtime.encode_gpu_frame(
                    owner_b
                        .context(&queue_b, &mut foreign_encoder)
                        .expect("foreign owner context"),
                    Camera::default(),
                    64,
                    64,
                    frame_with_plan_set(1, 1, 1, 2),
                ),
                Err(GpuPreparationError::ExecutionOwnerMismatch)
            ));
            assert!(matches!(
                runtime.encode_gpu_preproject_frame(
                    owner_b
                        .context(&queue_b, &mut foreign_encoder)
                        .expect("foreign owner context"),
                    Camera::default(),
                    64,
                    64,
                    frame_with_plan_set(1, 1, 1, 2),
                ),
                Err(GpuPreparationError::ExecutionOwnerMismatch)
            ));

            let mut encoder = device_a.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("rebound-plan-set-test-encoder"),
            });
            assert!(matches!(
                owner_a.context(&queue_b, &mut encoder),
                Err(GpuPreparationError::ExecutionOwnerMismatch)
            ));
            assert!(matches!(
                runtime.encode_gpu_frame(
                    owner_a
                        .context(&queue_a, &mut encoder)
                        .expect("bound owner context"),
                    Camera::default(),
                    64,
                    64,
                    frame_with_plan_set(1, 1, 1, 1),
                ),
                Err(GpuPreparationError::StaleRuntimeGeneration)
            ));
            let handles = runtime
                .encode_gpu_frame(
                    owner_a
                        .context(&queue_a, &mut encoder)
                        .expect("bound owner context"),
                    Camera::default(),
                    64,
                    64,
                    frame_with_plan_set(1, 1, 1, 2),
                )
                .expect("rebound candidate is immediately usable");
            assert_eq!(handles.receipt(), rebound);
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn preproject_seam_is_current_owner_bound_and_discard_safe() {
        pollster::block_on(async {
            let Some((_instance, _adapter, info, device, queue)) = request_device(8).await else {
                return;
            };
            eprintln!(
                "EXACT_GPU_PREPROJECT_ADAPTER adapter={} backend={:?}",
                info.name, info.backend
            );

            for (count, sh_degree) in [(0, 0), (1, 1), (127, 2), (128, 3), (129, 0), (1_025, 3)] {
                let owner = GpuExecutionOwner::new(&device, &queue);
                let resident =
                    ResidentSceneCpu::encode_owned(source(count, sh_degree)).expect("scene");
                let mut runtime =
                    crate::scene::SceneRuntime::prepare(resident).expect("Exact CPU runtime");
                let prepared = stage_and_commit_scene_gpu(&mut runtime, &owner, frame(1, 0, 0))
                    .await
                    .expect("PostSort plus dormant Preproject candidate");
                assert!(prepared.preproject_compute());
                assert_eq!(prepared.source_count(), count as u32);
                assert_eq!(prepared.sh_degree(), sh_degree);

                let mut invalid_camera = Camera::default();
                invalid_camera.intrinsics.near_plane = 2.0;
                invalid_camera.intrinsics.far_plane = 1.0;
                let mut rejected_encoder =
                    device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("rejected-preproject-frame-test-encoder"),
                    });
                assert!(matches!(
                    runtime.encode_gpu_preproject_frame(
                        owner
                            .context(&queue, &mut rejected_encoder)
                            .expect("bound owner context"),
                        invalid_camera,
                        64,
                        64,
                        frame(1, 1, 1),
                    ),
                    Err(GpuPreparationError::InvalidCamera)
                ));
                assert!(matches!(
                    runtime.encode_gpu_preproject_frame(
                        owner
                            .context(&queue, &mut rejected_encoder)
                            .expect("bound owner context"),
                        Camera::default(),
                        0,
                        64,
                        frame(1, 1, 1),
                    ),
                    Err(GpuPreparationError::InvalidViewport {
                        width: 0,
                        height: 64
                    })
                ));
                assert!(matches!(
                    runtime.encode_gpu_preproject_frame(
                        owner
                            .context(&queue, &mut rejected_encoder)
                            .expect("bound owner context"),
                        Camera::default(),
                        64,
                        64,
                        frame_with_plan_set(2, 1, 1, 1),
                    ),
                    Err(GpuPreparationError::StaleRuntimeGeneration)
                ));
                assert_eq!(runtime.gpu_preproject_encode_count(), Some(0));
                drop(rejected_encoder);

                let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
                let current = frame(1, 7, 9);
                let camera = Camera::default();
                let mut discarded_encoder =
                    device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("discarded-preproject-frame-test-encoder"),
                    });
                {
                    let handles = runtime
                        .encode_gpu_preproject_frame(
                            owner
                                .context(&queue, &mut discarded_encoder)
                                .expect("bound owner context"),
                            camera,
                            127,
                            65,
                            current,
                        )
                        .expect("complete Preproject encode");
                    let receipt = handles.receipt();
                    assert_eq!(receipt.frame_identity(), current);
                    assert_eq!(receipt.preparation(), prepared);
                    assert_eq!(receipt.camera(), camera);
                    assert_eq!(receipt.viewport(), (127, 65));
                    assert!(receipt.preparation().preproject_compute());
                    let _ = (
                        handles.ordered_source_ids(),
                        handles.indirect_args(),
                        handles.projected_center_alpha_key(),
                        handles.projected_axes(),
                        handles.resolved_color(),
                        handles.candidate_count().buffer(),
                        handles.contributor_count().buffer(),
                    );
                    assert_eq!(handles.candidate_count().offset() % 4, 0);
                    assert_eq!(handles.contributor_count().offset() % 4, 0);
                }
                assert_eq!(runtime.gpu_preproject_encode_count(), Some(1));
                drop(discarded_encoder);

                let mut retry_encoder =
                    device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("retried-preproject-frame-test-encoder"),
                    });
                runtime
                    .encode_gpu_preproject_frame(
                        owner
                            .context(&queue, &mut retry_encoder)
                            .expect("bound owner context"),
                        camera,
                        127,
                        65,
                        current,
                    )
                    .expect("discarded encoder requires a complete retry");
                assert_eq!(runtime.gpu_preproject_encode_count(), Some(2));
                let _command_buffer = retry_encoder.finish();
                assert!(
                    validation.pop().await.is_none(),
                    "Preproject adapter encode must be validation-clean"
                );
            }
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn preproject_count_sh_and_graph_receipt_mismatches_fail_closed() {
        pollster::block_on(async {
            let Some((_instance, _adapter, _info, device, queue)) = request_device(8).await else {
                return;
            };
            let owner = GpuExecutionOwner::new(&device, &queue);
            let resident = ResidentSceneCpu::encode_owned(source(129, 3)).expect("scene");
            let candidate = GpuScenePreparation::prepare(&owner, &resident, frame(1, 0, 0))
                .await
                .expect("complete candidate");

            let mut count_mismatch = candidate.receipt;
            count_mismatch.resident_count -= 1;
            assert!(matches!(
                validate_runtime_counts(
                    count_mismatch,
                    &candidate.resident,
                    &candidate.projector,
                    &candidate.preproject,
                ),
                Err(GpuPreparationError::ExactContractMismatch {
                    component: "source/capacity/resident/addressable count"
                })
            ));

            let mut sh_mismatch = candidate.receipt;
            sh_mismatch.sh_degree = 2;
            assert!(matches!(
                validate_runtime_counts(
                    sh_mismatch,
                    &candidate.resident,
                    &candidate.projector,
                    &candidate.preproject,
                ),
                Err(GpuPreparationError::ExactContractMismatch {
                    component: "SH degree"
                })
            ));

            let mut graph_mismatch = candidate.receipt;
            graph_mismatch.preproject_compute = false;
            assert!(matches!(
                validate_runtime_counts(
                    graph_mismatch,
                    &candidate.resident,
                    &candidate.projector,
                    &candidate.preproject,
                ),
                Err(GpuPreparationError::ExactContractMismatch {
                    component: "Preproject compute graph"
                })
            ));
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn complete_sh_counts_and_encode_seam_are_device_owned_and_current() {
        pollster::block_on(async {
            let Some((_instance, _adapter, info, device, queue)) = request_device(8).await else {
                return;
            };
            eprintln!(
                "EXACT_GPU_PREPARATION adapter={} backend={:?}",
                info.name, info.backend
            );

            for sh_degree in 0..=3 {
                for count in [0, 1, TAIL_COUNT] {
                    let owner = GpuExecutionOwner::new(&device, &queue);
                    let resident =
                        ResidentSceneCpu::encode_owned(source(count, sh_degree)).expect("scene");
                    let mut runtime =
                        crate::scene::SceneRuntime::prepare(resident).expect("Exact CPU runtime");
                    let receipt = stage_and_commit_scene_gpu(&mut runtime, &owner, frame(1, 0, 0))
                        .await
                        .expect("complete GPU candidate");
                    assert_eq!(receipt.source_count(), count as u32);
                    assert_eq!(receipt.capacity(), count as u32);
                    assert_eq!(receipt.resident_count(), count as u32);
                    assert_eq!(receipt.addressable_count(), count as u32);
                    assert_eq!(receipt.sh_degree(), sh_degree);

                    let mut stale_encoder =
                        device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("stale-exact-gpu-frame-test-encoder"),
                        });
                    assert!(matches!(
                        runtime.encode_gpu_frame(
                            owner
                                .context(&queue, &mut stale_encoder)
                                .expect("bound owner context"),
                            Camera::default(),
                            64,
                            64,
                            FrameIdentity::new(2, 1, 1, 1, 1),
                        ),
                        Err(GpuPreparationError::StaleRuntimeGeneration)
                    ));

                    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
                    let mut discarded_encoder =
                        device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("discarded-exact-gpu-frame-test-encoder"),
                        });
                    {
                        let current = frame(1, 1, 1);
                        let handles = runtime
                            .encode_gpu_frame(
                                owner
                                    .context(&queue, &mut discarded_encoder)
                                    .expect("bound owner context"),
                                Camera::default(),
                                64,
                                64,
                                current,
                            )
                            .expect("single-method exact color/order/project encode");
                        discarded_encoder.insert_debug_marker(
                            "caller encoder remains usable while projected handles are borrowed",
                        );
                        assert_eq!(handles.frame_identity(), current);
                        assert_eq!(handles.receipt(), receipt);
                        let _ = (
                            handles.ordered_source_ids(),
                            handles.indirect_args(),
                            handles.projected_center_source(),
                            handles.projected_axes(),
                            handles.resolved_color(),
                        );
                    }
                    assert_eq!(runtime.gpu_color_encode_count(), Some(1));
                    drop(discarded_encoder);

                    let mut retry_encoder =
                        device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("retried-exact-gpu-frame-test-encoder"),
                        });
                    runtime
                        .encode_gpu_frame(
                            owner
                                .context(&queue, &mut retry_encoder)
                                .expect("bound owner context"),
                            Camera::default(),
                            64,
                            64,
                            frame(1, 1, 1),
                        )
                        .expect("discarded encoder must retry uncached color and full sequence");
                    assert_eq!(runtime.gpu_color_encode_count(), Some(2));
                    let _command_buffer = retry_encoder.finish();
                    assert!(
                        validation.pop().await.is_none(),
                        "GPU preparation encode must be validation-clean"
                    );
                }
            }
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn renderer_execute_routes_gpu_context_through_plan_set() {
        pollster::block_on(async {
            let Some((_instance, _adapter, _info, device, queue)) = request_device(8).await else {
                return;
            };
            let resident = ResidentSceneCpu::encode_owned(source(1, 0)).expect("resident scene");
            let mut slot = PreparedRuntimeSlot::prepare(resident).expect("runtime slot");
            let receipt = slot
                .prepare_gpu(&device, &queue)
                .await
                .expect("renderer-owned GPU context");
            assert_eq!(receipt.plan_set_generation(), 2);
            assert_eq!(slot.frame_state().identity().plan_set_generation(), 2);
            assert_eq!(
                slot.gpu_capability()
                    .expect("PlanSet capability receipt")
                    .plan_set_generation(),
                2
            );
            let current_frame = slot.frame_state();
            assert!(matches!(
                slot.prepare_gpu(&device, &queue).await,
                Err(GpuRuntimePreparationError::Resources(
                    GpuPreparationError::ExecutionOwnerAlreadyBound
                ))
            ));
            assert_eq!(slot.frame_state(), current_frame);
            let viewport = Viewport::new(64, 64).expect("viewport");

            assert!(matches!(
                execute_frame(&mut slot, PlanId::GpuPostSort, &Camera::default(), viewport,),
                Err(FrameExecutionError::PlanSet(
                    PlanSetError::GpuExecutionUnavailable {
                        requested: PlanId::GpuPostSort
                    }
                ))
            ));

            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("renderer-plan-set-gpu-seam-test-encoder"),
            });
            assert!(matches!(
                execute_frame_gpu(
                    &mut slot,
                    PlanId::GpuPostSort,
                    &Camera::default(),
                    viewport,
                    &queue,
                    &mut encoder,
                ),
                Err(FrameExecutionError::PlanSet(
                    PlanSetError::RequestedPlanUnprepared {
                        requested: PlanId::GpuPostSort
                    }
                ))
            ));
        });
    }
}
