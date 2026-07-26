//! Private Direct/Paged state owned by the standalone Surface session facade.
//!
//! `SurfacePresenter` retains Surface acquisition, configuration, submission,
//! presentation, capture, and host-resource coordination. This owner keeps the
//! scene candidate transaction, CPU/GPU order preparation, path-specific draw
//! state, and runtime telemetry out of that presenter responsibility.

use gsplat_core::{Camera, SceneBuffers};

use super::standalone_direct_runtime::{
    DirectGpuTelemetrySample, PreparedStandaloneDirectScene, StandaloneDirectRuntime,
};
use super::standalone_paged_runtime::{PreparedStandalonePagedScene, StandalonePagedRuntime};
use crate::gpu_telemetry::{
    CpuOrderCompletionTelemetry, CpuOrderTelemetryPoll, FrameInstanceCounts, GpuOrderTelemetryPoll,
};
use crate::surface_presenter::try_prepare_then_commit;
use crate::{GeometryPath, Renderer, SurfacePresenterError, TimerInstant};

#[derive(Clone, Copy)]
pub(crate) struct CpuCompletionSampleRequest {
    pub(crate) camera_revision: u64,
    pub(crate) started: TimerInstant,
    pub(crate) preprocess_ms: f32,
    pub(crate) sort_ms: f32,
}

enum StandaloneGeometry {
    Direct,
    Paged,
}

impl StandaloneGeometry {
    const fn path(&self) -> GeometryPath {
        match self {
            Self::Direct => GeometryPath::SortedIndexDirect,
            Self::Paged => GeometryPath::PagedActiveAtlas,
        }
    }
}

enum PreparedStandaloneGeometry {
    Direct(PreparedStandaloneDirectScene),
    Paged(PreparedStandalonePagedScene),
}

impl PreparedStandaloneGeometry {
    fn addressable_splat_count(&self) -> usize {
        match self {
            Self::Direct(direct) => direct.addressable_splat_count(),
            Self::Paged(paged) => paged.addressable_splat_count(),
        }
    }
}

struct GeometryResourceContext<'a> {
    device: &'a wgpu::Device,
    direct_runtime: &'a StandaloneDirectRuntime,
    paged_runtime: &'a StandalonePagedRuntime,
}

fn create_geometry_candidate(
    context: GeometryResourceContext<'_>,
    path: GeometryPath,
    renderer: &Renderer,
) -> Result<PreparedStandaloneGeometry, SurfacePresenterError> {
    let GeometryResourceContext {
        device,
        direct_runtime,
        paged_runtime,
    } = context;
    match path {
        GeometryPath::SortedIndexDirect => {
            let (scene, world_covariance_terms, alpha_values) =
                renderer.direct_scene_cpu_inputs().ok_or_else(|| {
                    if renderer.has_scene() {
                        SurfacePresenterError::GeometrySourceUnavailable { path }
                    } else {
                        SurfacePresenterError::SceneNotLoaded
                    }
                })?;
            Ok(PreparedStandaloneGeometry::Direct(
                direct_runtime.prepare_scene_candidate(
                    device,
                    scene,
                    world_covariance_terms,
                    alpha_values,
                )?,
            ))
        }
        GeometryPath::PackedAtlas => {
            Err(SurfacePresenterError::StandalonePackedPresenterUnsupported)
        }
        GeometryPath::PagedActiveAtlas => {
            let scene = renderer.scene().ok_or_else(|| {
                if renderer.has_scene() {
                    SurfacePresenterError::GeometrySourceUnavailable { path }
                } else {
                    SurfacePresenterError::SceneNotLoaded
                }
            })?;
            let pages = renderer
                .spatial_pages()
                .cloned()
                .ok_or(SurfacePresenterError::SceneNotLoaded)?;
            Ok(PreparedStandaloneGeometry::Paged(
                paged_runtime.prepare_scene_candidate(device, scene, pages)?,
            ))
        }
    }
}

pub(crate) struct StandaloneSessionRuntime {
    direct_runtime: StandaloneDirectRuntime,
    paged_runtime: StandalonePagedRuntime,
    geometry: StandaloneGeometry,
    addressable_splat_count: usize,
    cpu_order_completion_telemetry: CpuOrderCompletionTelemetry,
}

impl StandaloneSessionRuntime {
    pub(crate) async fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        timestamp_queries_enabled: bool,
        renderer: &Renderer,
    ) -> Result<(Self, usize), SurfacePresenterError> {
        let geometry_path = renderer.geometry_path();
        let (validation_scope, oom_scope, internal_scope) = (
            device.push_error_scope(wgpu::ErrorFilter::Validation),
            device.push_error_scope(wgpu::ErrorFilter::OutOfMemory),
            device.push_error_scope(wgpu::ErrorFilter::Internal),
        );
        let mut direct_runtime =
            StandaloneDirectRuntime::new(device, queue, format, timestamp_queries_enabled);
        let mut paged_runtime = StandalonePagedRuntime::new(device, format);
        let geometry_result = create_geometry_candidate(
            GeometryResourceContext {
                device,
                direct_runtime: &direct_runtime,
                paged_runtime: &paged_runtime,
            },
            geometry_path,
            renderer,
        );
        let internal_error = internal_scope.pop().await;
        let oom_error = oom_scope.pop().await;
        let validation_error = validation_scope.pop().await;
        if oom_error.is_some() {
            return Err(SurfacePresenterError::SurfaceOutOfMemory);
        }
        if let Some(error) = internal_error.or(validation_error) {
            return Err(SurfacePresenterError::DeviceCreation(format!(
                "surface geometry resource creation failed: {error}"
            )));
        }
        let prepared = geometry_result?;
        let addressable_splat_count = prepared.addressable_splat_count();
        let geometry = match prepared {
            PreparedStandaloneGeometry::Direct(scene) => {
                direct_runtime.publish_scene(scene);
                StandaloneGeometry::Direct
            }
            PreparedStandaloneGeometry::Paged(scene) => {
                paged_runtime.publish_scene(scene);
                StandaloneGeometry::Paged
            }
        };
        Ok((
            Self {
                direct_runtime,
                paged_runtime,
                geometry,
                addressable_splat_count,
                cpu_order_completion_telemetry: CpuOrderCompletionTelemetry::default(),
            },
            addressable_splat_count,
        ))
    }

    pub(crate) const fn geometry_path(&self) -> GeometryPath {
        self.geometry.path()
    }

    pub(crate) fn set_geometry_path(
        &mut self,
        device: &wgpu::Device,
        path: GeometryPath,
        renderer: &Renderer,
    ) -> Result<usize, SurfacePresenterError> {
        if self.geometry.path() == path {
            return Ok(self.addressable_splat_count());
        }
        if cfg!(target_arch = "wasm32") || path == GeometryPath::PackedAtlas {
            return Err(SurfacePresenterError::SurfaceGeometrySwitchUnsupported);
        }

        try_prepare_then_commit(
            self,
            |runtime| {
                create_geometry_candidate(
                    GeometryResourceContext {
                        device,
                        direct_runtime: &runtime.direct_runtime,
                        paged_runtime: &runtime.paged_runtime,
                    },
                    path,
                    renderer,
                )
            },
            |runtime, prepared| {
                runtime.addressable_splat_count = prepared.addressable_splat_count();
                runtime.geometry = match prepared {
                    PreparedStandaloneGeometry::Direct(scene) => {
                        runtime.direct_runtime.publish_scene(scene);
                        runtime.paged_runtime.clear_scene();
                        StandaloneGeometry::Direct
                    }
                    PreparedStandaloneGeometry::Paged(scene) => {
                        runtime.paged_runtime.publish_scene(scene);
                        runtime.direct_runtime.clear_scene();
                        StandaloneGeometry::Paged
                    }
                };
                runtime.direct_runtime.invalidate_gpu_order_telemetry();
                runtime
                    .cpu_order_completion_telemetry
                    .invalidate_generation();
            },
        )?;
        Ok(self.addressable_splat_count())
    }

    pub(crate) fn invalidate_telemetry(&mut self) {
        self.direct_runtime.invalidate_gpu_order_telemetry();
        self.cpu_order_completion_telemetry.invalidate_generation();
    }

    pub(crate) fn prepare_paged_frame(
        &mut self,
        queue: &wgpu::Queue,
        scene: &SceneBuffers,
        camera: &Camera,
        width: u32,
        height: u32,
    ) -> Result<(), SurfacePresenterError> {
        if !matches!(self.geometry, StandaloneGeometry::Paged) {
            return Err(SurfacePresenterError::PagedAtlasUnsupported);
        }
        self.paged_runtime
            .prepare_frame(queue, scene, camera, width, height)
    }

    pub(crate) fn prepare_cpu_order(
        &mut self,
        queue: &wgpu::Queue,
        sorted_indices: &[u32],
        camera: &Camera,
        width: u32,
        height: u32,
        refresh_indices: bool,
    ) -> Result<(), SurfacePresenterError> {
        if !matches!(self.geometry, StandaloneGeometry::Direct) {
            return Err(SurfacePresenterError::PagedAtlasUnsupported);
        }
        self.direct_runtime.prepare_cpu_order(
            queue,
            sorted_indices,
            camera,
            width,
            height,
            refresh_indices,
        )
    }

    pub(crate) async fn prepare_gpu_order(
        &mut self,
        device: &wgpu::Device,
        indirect_execution_supported: bool,
    ) -> Result<(), SurfacePresenterError> {
        if !indirect_execution_supported || !matches!(self.geometry, StandaloneGeometry::Direct) {
            return Err(SurfacePresenterError::GpuOrderUnsupported);
        }
        if self.direct_runtime.gpu_order_is_prepared() {
            return Ok(());
        }
        self.direct_runtime.prepare_gpu_order(device).await
    }

    pub(crate) fn gpu_order_is_prepared(&self) -> bool {
        matches!(self.geometry, StandaloneGeometry::Direct)
            && self.direct_runtime.gpu_order_is_prepared()
    }

    pub(crate) fn prepare_gpu_frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        camera: &Camera,
        width: u32,
        height: u32,
    ) -> Result<(), SurfacePresenterError> {
        if !matches!(self.geometry, StandaloneGeometry::Direct) {
            return Err(SurfacePresenterError::GpuOrderUnsupported);
        }
        self.direct_runtime
            .prepare_gpu_frame(device, queue, camera, width, height)
    }

    pub(crate) fn begin_gpu_order_sample(
        &mut self,
        camera_revision: u64,
        refresh_order: bool,
    ) -> Result<Option<DirectGpuTelemetrySample>, SurfacePresenterError> {
        self.direct_runtime
            .begin_gpu_order_sample(camera_revision, refresh_order)
    }

    pub(crate) fn encode_gpu_order_draw(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        queue: &wgpu::Queue,
        refresh_order: bool,
        sample: Option<&DirectGpuTelemetrySample>,
    ) -> Result<(), SurfacePresenterError> {
        self.direct_runtime
            .encode_gpu_order_draw(encoder, view, queue, refresh_order, sample)
    }

    pub(crate) fn arm_gpu_order_sample(
        &mut self,
        command_buffer: &wgpu::CommandBuffer,
        sample: DirectGpuTelemetrySample,
        started: TimerInstant,
    ) {
        self.direct_runtime
            .arm_gpu_order_sample(command_buffer, sample, started);
    }

    pub(crate) fn encode_draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) -> Result<(), SurfacePresenterError> {
        match self.geometry {
            StandaloneGeometry::Direct => self.direct_runtime.encode_cpu_draw(encoder, view),
            StandaloneGeometry::Paged => self.paged_runtime.encode_draw(encoder, view),
        }
    }

    pub(crate) fn begin_cpu_completion_sample(
        &mut self,
        request: CpuCompletionSampleRequest,
    ) -> Option<crate::gpu_telemetry::CpuCompletionTicket> {
        let count = self.instance_count();
        self.cpu_order_completion_telemetry
            .begin_sample_with_counts(
                request.camera_revision,
                request.preprocess_ms,
                request.sort_ms,
                FrameInstanceCounts {
                    candidate_visible: count,
                    contributor: count,
                    drawn: count,
                    exact_contributor_compaction: false,
                },
            )
    }

    pub(crate) fn arm_cpu_completion_sample(
        &mut self,
        command_buffer: &wgpu::CommandBuffer,
        ticket: crate::gpu_telemetry::CpuCompletionTicket,
        started: TimerInstant,
    ) {
        self.cpu_order_completion_telemetry
            .arm(command_buffer, ticket, started);
    }

    pub(crate) fn poll_gpu_order_telemetry(
        &mut self,
        device: &wgpu::Device,
    ) -> GpuOrderTelemetryPoll {
        self.direct_runtime.poll_gpu_order_telemetry(device)
    }

    pub(crate) fn poll_cpu_order_completion_telemetry(&mut self) -> CpuOrderTelemetryPoll {
        self.cpu_order_completion_telemetry.poll()
    }

    pub(crate) fn gpu_order_timestamps_enabled(&self) -> bool {
        self.direct_runtime.gpu_order_timestamps_enabled()
    }

    pub(crate) const fn instance_count(&self) -> u32 {
        match self.geometry {
            StandaloneGeometry::Direct => self.direct_runtime.instance_count(),
            StandaloneGeometry::Paged => self.paged_runtime.instance_count(),
        }
    }

    fn addressable_splat_count(&self) -> usize {
        self.addressable_splat_count
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn owner_contains_the_standalone_candidate_order_and_telemetry_closure() {
        let source = include_str!("standalone_session_runtime.rs");
        for responsibility in [
            "create_geometry_candidate",
            "prepare_scene_candidate",
            "publish_scene",
            "prepare_cpu_order",
            "prepare_gpu_order",
            "CpuOrderCompletionTelemetry",
            "poll_gpu_order_telemetry",
        ] {
            assert!(source.contains(responsibility), "missing {responsibility}");
        }
    }
}
