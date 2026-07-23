//! WGPU Surface presentation and geometry-resource ownership.

use gsplat_core::{Camera, SceneBuffers};
use gsplat_sort::CpuSortBackend;

use crate::SurfaceRasterExecutionPlan;
use crate::direct_gpu_order::GpuOrderTimestampRange;
use crate::draw_pass::{
    SplatDraw, SplatIndirectDraw, encode_splat_draw_into, encode_splat_indirect_draw_into,
};
use crate::gpu_producer_telemetry::{
    GpuProducerCountSource, GpuProducerSampleMetadata, GpuProducerTelemetry,
    GpuProducerTelemetryPoll, SurfaceGpuOrderProducer, SurfaceGpuProducerDrawScope,
};
use crate::gpu_telemetry::{
    CpuOrderCompletionTelemetry, CpuOrderTelemetryPoll, FrameInstanceCounts, GpuOrderTelemetry,
    GpuOrderTelemetryPoll, InstanceCountSource, TelemetrySubmission,
};
#[cfg(target_arch = "wasm32")]
use crate::gpu_telemetry::{GpuTelemetryTicket, SurfaceOrderMeasurementFailureReason};
use crate::packed_gpu;
use crate::paged_active_set::PagedActiveSet;
use crate::preproject_gpu::PreprojectedGpuOrder;
use crate::projected_draw_telemetry::{
    ProjectedDrawCountSource, ProjectedDrawSampleMetadata, ProjectedDrawTelemetry,
    ProjectedDrawTelemetryPoll,
};
use crate::projected_quads_gpu::{ProjectedDrawExecution, ProjectedQuadsGpu};
use crate::resident_gpu;
use crate::tiled_resident_gpu::{ResidentTiledFinish, ResidentTiledGpu};
use crate::{
    DEFAULT_PAGED_ATLAS_SLOTS, DirectGpuSceneOrder, DirectSceneError, DirectScenePath,
    DirectScenePreflight, DirectSceneResources, GeometryPath, PackedScenePath,
    PackedScenePreflight, PreparedRendererGeometryPath, Renderer, ResidentGpuBytePlan,
    ResidentSceneCpu, SpatialPageSet, SurfacePresenterError, TimerInstant,
    create_direct_bind_group_layout, create_direct_pipeline, create_surface_instance,
    direct_scene_preflight, packed_scene_preflight_with_limits, preprocess_paged_visible_into,
    refresh_paged_hot_colors, select_present_mode, surface_error_to_presenter, wgpu_label,
};
#[cfg(not(target_arch = "wasm32"))]
use crate::{timer_elapsed_ms, timer_now};

struct SurfaceAdapterContext {
    info: wgpu::AdapterInfo,
    limits: wgpu::Limits,
}

pub(crate) struct SurfacePagedRuntime {
    pub(crate) active_set: PagedActiveSet,
    sort_backend: CpuSortBackend,
    depth_keys: Vec<u32>,
    sorted_indices: Vec<u32>,
}

impl SurfacePagedRuntime {
    pub(crate) fn new(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        scene: &SceneBuffers,
        pages: SpatialPageSet,
    ) -> Result<Self, SurfacePresenterError> {
        let active_set = PagedActiveSet::new(device, layout, scene, pages)
            .map_err(|err| SurfacePresenterError::PagedAtlas(err.to_string()))?;
        Ok(Self {
            active_set,
            sort_backend: CpuSortBackend::default(),
            depth_keys: Vec::new(),
            sorted_indices: Vec::new(),
        })
    }

    pub(crate) fn prepare(
        &mut self,
        queue: &wgpu::Queue,
        scene: &SceneBuffers,
        camera: &Camera,
        width: u32,
        height: u32,
    ) -> Result<u32, SurfacePresenterError> {
        self.active_set
            .sync(queue, scene, camera)
            .map_err(|err| SurfacePresenterError::PagedAtlas(err.to_string()))?;
        let entries = self.active_set.atlas.active_entries();
        preprocess_paged_visible_into(
            scene,
            &entries,
            camera,
            &mut self.depth_keys,
            &mut self.sorted_indices,
        )
        .map_err(|err| SurfacePresenterError::PagedAtlas(err.to_string()))?;
        self.sort_backend
            .sort_values_by_keys(&self.depth_keys, &mut self.sorted_indices)
            .map_err(|err| SurfacePresenterError::PagedAtlas(err.to_string()))?;
        refresh_paged_hot_colors(queue, &mut self.active_set.atlas, scene, camera);
        self.active_set
            .atlas
            .resources
            .prepare(queue, &self.sorted_indices, camera, width, height, true)
            .map_err(SurfacePresenterError::from)
    }
}

pub struct SurfacePresenter {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    direct_pipeline: wgpu::RenderPipeline,
    direct_bind_group_layout: wgpu::BindGroupLayout,
    packed_pipeline: wgpu::RenderPipeline,
    packed_bind_group_layout: wgpu::BindGroupLayout,
    resident_draw_pipeline: Option<wgpu::RenderPipeline>,
    resident_draw_bind_group_layout: Option<wgpu::BindGroupLayout>,
    resident_color_pipeline: Option<wgpu::ComputePipeline>,
    resident_color_bind_group_layout: Option<wgpu::BindGroupLayout>,
    surface_config: wgpu::SurfaceConfiguration,
    surface_configuration_valid: bool,
    max_texture_dimension_2d: u32,
    adapter_max_storage_buffers_per_shader_stage: u32,
    adapter_max_storage_buffer_binding_size: u64,
    indirect_execution_supported: bool,
    addressable_splat_count: usize,
    instance_count: u32,
    geometry: SurfaceGeometry,
    gpu_order_telemetry: GpuOrderTelemetry,
    cpu_order_completion_telemetry: CpuOrderCompletionTelemetry,
    projected_draw_telemetry: ProjectedDrawTelemetry,
    gpu_producer_telemetry: GpuProducerTelemetry,
    gpu_order_producer: SurfaceGpuOrderProducer,
    gpu_producer_measurement_enabled: bool,
    last_gpu_producer_submission: TelemetrySubmission,
    last_actual_gpu_order_producer: Option<SurfaceGpuOrderProducer>,
    projected_draw_execution: ProjectedDrawExecution,
    projected_probe_generation: u64,
    projected_draw_sample_request: Option<ProjectedDrawSampleRequest>,
    last_projected_draw_submission: TelemetrySubmission,
    last_frame_presented: bool,
    last_presented_size: Option<(u32, u32)>,
}

#[derive(Clone, Copy)]
pub(crate) struct CpuCompletionSampleRequest {
    pub(crate) camera_revision: u64,
    pub(crate) started: TimerInstant,
    pub(crate) preprocess_ms: f32,
    pub(crate) sort_ms: f32,
}

#[derive(Clone, Copy)]
pub(crate) struct ProjectedDrawSampleRequest {
    pub(crate) camera_revision: u64,
    pub(crate) started: TimerInstant,
    pub(crate) order_backend: crate::SurfaceOrderBackendUsed,
    pub(crate) order_refreshed: bool,
}

enum SurfaceGeometry {
    Direct(Box<DirectSceneResources>),
    Packed(Box<SurfacePackedRuntime>),
    Paged(Box<SurfacePagedRuntime>),
}

/// Complete but unpublished GPU-side geometry graph for a runtime path
/// transition. This stays opaque to the session so publication remains an
/// infallible presenter operation.
#[cfg(target_arch = "wasm32")]
pub(crate) struct PreparedSurfaceGeometryPath {
    geometry: SurfaceGeometry,
}

enum PreparedSurfaceGpuOrder {
    AlreadyPrepared,
    Direct(DirectGpuSceneOrder),
    Packed {
        order: resident_gpu::ResidentGpuSceneOrder,
        projected_bind_group: wgpu::BindGroup,
    },
}

enum PreparedSurfaceGpuProducer {
    AlreadyPrepared,
    Preproject(Box<PreprojectedGpuOrder>),
}

struct SurfacePackedRuntime {
    resident: resident_gpu::ResidentGpuResources,
    projected: ProjectedQuadsGpu,
    projected_cache: ProjectedCacheState,
    preproject: Option<PreprojectedGpuOrder>,
    preproject_state: PreprojectProducerState,
    // The tiled software raster is an exact diagnostic oracle, not part of
    // the production allocation. Create it only for an explicit A/B request.
    tiled: Option<ResidentTiledGpu>,
    raster_plan: SurfaceRasterExecutionPlan,
    #[cfg(not(target_arch = "wasm32"))]
    phase_trace_emitted: u32,
    #[cfg(not(target_arch = "wasm32"))]
    last_count_resolve_prepare_ms: f32,
    #[cfg(target_arch = "wasm32")]
    pending: Option<WebPendingTiledFrame>,
    #[cfg(target_arch = "wasm32")]
    pending_gpu: Option<WebPendingTiledGpuFrame>,
    #[cfg(target_arch = "wasm32")]
    gpu_order_warmed: bool,
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
struct PreprojectProducerState {
    order_valid: bool,
    order_generation: u64,
    projection_generation: u64,
}

impl PreprojectProducerState {
    fn invalidate_order(&mut self) {
        self.order_valid = false;
        self.order_generation = self.order_generation.wrapping_add(1);
    }

    fn require_order(self, refresh_order: bool) -> Result<(), SurfacePresenterError> {
        if refresh_order || self.order_valid {
            Ok(())
        } else {
            Err(SurfacePresenterError::PreprojectOrderUnavailable)
        }
    }

    fn record_projection(&mut self, refresh_order: bool) {
        self.projection_generation = self.projection_generation.wrapping_add(1);
        if refresh_order {
            self.order_generation = self.order_generation.wrapping_add(1);
            self.order_valid = true;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProjectedOrderSource {
    Cpu,
    Gpu,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ProjectedCacheKey {
    order_source: ProjectedOrderSource,
    order_generation: u64,
    camera: Camera,
    width: u32,
    height: u32,
    draw_count_guard: u32,
    draw_execution: ProjectedDrawExecution,
    probe_generation: u64,
}

#[derive(Default)]
struct ProjectedCacheState {
    key: Option<ProjectedCacheKey>,
    order_generation: u64,
    projection_generation: u64,
}

impl ProjectedCacheState {
    fn invalidate_order_if(&mut self, order_refreshed: bool) {
        if order_refreshed {
            self.key = None;
            self.order_generation = self.order_generation.wrapping_add(1);
        }
    }

    const fn order_generation(&self) -> u64 {
        self.order_generation
    }

    fn needs_projection(&self, key: ProjectedCacheKey) -> bool {
        self.key != Some(key)
    }

    fn publish(&mut self, key: ProjectedCacheKey) {
        self.key = Some(key);
        self.projection_generation = self.projection_generation.wrapping_add(1);
    }

    const fn projection_generation(&self) -> u64 {
        self.projection_generation
    }
}

/// Browser WebGPU may lazily initialize the first large packed GPU-order
/// pipeline. The preparation submission is deliberately not a frame: it
/// acquires no surface and reserves no benchmark ticket. Once that submission
/// has been queued, the same camera revision is retried as one ordinary,
/// presented, measured frame. Native backends and direct geometry never need
/// this extra turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GpuOrderPreparationPlan {
    pending: bool,
    acquire_surface: bool,
    reserve_measurement: bool,
}

const fn gpu_order_preparation_plan(
    web: bool,
    packed_geometry: bool,
    refresh_order: bool,
    already_prepared: bool,
) -> GpuOrderPreparationPlan {
    let pending = web && packed_geometry && refresh_order && !already_prepared;
    GpuOrderPreparationPlan {
        pending,
        acquire_surface: !pending,
        reserve_measurement: refresh_order && !pending,
    }
}

/// Stops an ordinary GPU frame before telemetry reservation when the Surface
/// supplies no drawable. A Web preparation turn intentionally has no drawable
/// and therefore continues. In particular, a timed-out frame can never issue
/// a ticket whose C/D readback came from an older projected cache.
const fn unavailable_gpu_surface_submission(
    preparation: GpuOrderPreparationPlan,
    frame_available: bool,
) -> Option<TelemetrySubmission> {
    if preparation.acquire_surface && !frame_available {
        return Some(if preparation.reserve_measurement {
            TelemetrySubmission::SurfaceUnavailable
        } else {
            TelemetrySubmission::NotRequested
        });
    }
    None
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy)]
struct WebPendingTiledFrame {
    camera: Camera,
    work_count: u32,
    completion: Option<CpuCompletionSampleRequest>,
    count_ready: bool,
}

#[cfg(target_arch = "wasm32")]
struct WebPendingTiledGpuFrame {
    camera: Camera,
    work_count: u32,
    camera_revision: u64,
    completion_started: TimerInstant,
    count_ready: bool,
    refresh_order: bool,
    warmup_only: bool,
    telemetry_ticket: Option<GpuTelemetryTicket>,
}

impl SurfaceGeometry {
    const fn path(&self) -> GeometryPath {
        match self {
            Self::Direct(_) => GeometryPath::SortedIndexDirect,
            Self::Packed(_) => GeometryPath::PackedAtlas,
            Self::Paged(_) => GeometryPath::PagedActiveAtlas,
        }
    }

    fn addressable_splat_count(&self) -> usize {
        match self {
            Self::Direct(direct) => direct.capacity,
            Self::Packed(packed) => packed.resident.capacity,
            Self::Paged(paged) => paged.active_set.atlas.resources.capacity,
        }
    }
}

fn resident_pipelines_unavailable(storage_bindings: u32) -> SurfacePresenterError {
    resident_gpu::ResidentGpuError::StorageBindingCountUnsupported(storage_bindings).into()
}

fn supports_resident_pipeline_layout(limits: &wgpu::Limits) -> bool {
    limits.max_storage_buffers_per_shader_stage >= resident_gpu::RESIDENT_COLOR_STORAGE_BINDINGS
}

fn supports_direct_gpu_order(downlevel: &wgpu::DownlevelCapabilities) -> bool {
    downlevel
        .flags
        .contains(wgpu::DownlevelFlags::INDIRECT_EXECUTION)
}

fn classify_surface_gpu_order_scope_errors(
    path: GeometryPath,
    internal: Option<String>,
    out_of_memory: Option<String>,
    validation: Option<String>,
) -> Option<SurfacePresenterError> {
    match path {
        GeometryPath::PackedAtlas => out_of_memory
            .map(resident_gpu::ResidentGpuError::GpuOrderOutOfMemory)
            .or_else(|| internal.map(resident_gpu::ResidentGpuError::GpuOrderInternal))
            .or_else(|| validation.map(resident_gpu::ResidentGpuError::GpuOrderValidation))
            .map(SurfacePresenterError::from),
        GeometryPath::SortedIndexDirect => out_of_memory
            .map(|error| format!("out of memory: {error}"))
            .or_else(|| internal.map(|error| format!("internal: {error}")))
            .or_else(|| validation.map(|error| format!("validation: {error}")))
            .map(DirectSceneError::GpuOrderInitialization)
            .map(SurfacePresenterError::from),
        GeometryPath::PagedActiveAtlas => Some(SurfacePresenterError::GpuOrderUnsupported),
    }
}

fn classify_surface_geometry_scope_errors(
    path: GeometryPath,
    internal: Option<String>,
    out_of_memory: Option<String>,
    validation: Option<String>,
) -> Option<SurfacePresenterError> {
    out_of_memory
        .map(|message| SurfacePresenterError::SurfaceGeometryOutOfMemory { path, message })
        .or_else(|| {
            internal.map(|message| SurfacePresenterError::SurfaceGeometryInternal { path, message })
        })
        .or_else(|| {
            validation
                .map(|message| SurfacePresenterError::SurfaceGeometryValidation { path, message })
        })
}

#[cfg(any(target_arch = "wasm32", test))]
fn classify_surface_configure_scope_errors(
    internal: Option<String>,
    out_of_memory: Option<String>,
    validation: Option<String>,
) -> Option<SurfacePresenterError> {
    out_of_memory
        .map(|_| SurfacePresenterError::SurfaceOutOfMemory)
        .or_else(|| {
            internal
                .map(|error| SurfacePresenterError::SurfaceConfigure(format!("internal: {error}")))
        })
        .or_else(|| {
            validation.map(|error| {
                SurfacePresenterError::SurfaceConfigure(format!("validation: {error}"))
            })
        })
}

/// Resolves the independently scoped Compact candidate. Initial construction
/// and Candidate/Adaptive geometry switches use best-effort admission;
/// forced Compact switches make the exact same graph a required part of the
/// unpublished transaction.
fn resolve_projected_compaction_candidate<T>(
    candidate: Result<Option<T>, resident_gpu::ResidentGpuError>,
    internal: Option<String>,
    out_of_memory: Option<String>,
    validation: Option<String>,
    required: bool,
) -> Result<Option<T>, SurfacePresenterError> {
    if let Some(error) = classify_surface_geometry_scope_errors(
        GeometryPath::PackedAtlas,
        internal,
        out_of_memory,
        validation,
    ) {
        return if required { Err(error) } else { Ok(None) };
    }
    match candidate {
        Ok(Some(candidate)) => Ok(Some(candidate)),
        Ok(None) if required => Err(SurfacePresenterError::ProjectedCompactionUnsupported),
        Ok(None) => Ok(None),
        Err(error) if required => Err(error.into()),
        Err(_) => Ok(None),
    }
}

async fn prepare_optional_projected_compaction(
    device: &wgpu::Device,
    surface_format: wgpu::TextureFormat,
    indirect_execution_supported: bool,
    geometry: &mut SurfaceGeometry,
    required: bool,
) -> Result<(), SurfacePresenterError> {
    if !matches!(geometry, SurfaceGeometry::Packed(_)) {
        return Ok(());
    }
    if !indirect_execution_supported {
        return if required {
            Err(SurfacePresenterError::ProjectedCompactionUnsupported)
        } else {
            Ok(())
        };
    }

    let (validation_scope, oom_scope, internal_scope) = (
        device.push_error_scope(wgpu::ErrorFilter::Validation),
        device.push_error_scope(wgpu::ErrorFilter::OutOfMemory),
        device.push_error_scope(wgpu::ErrorFilter::Internal),
    );
    let candidate = match geometry {
        SurfaceGeometry::Packed(packed) => packed
            .projected
            .create_contributor_compaction_candidate(device, surface_format, &packed.resident),
        SurfaceGeometry::Direct(_) | SurfaceGeometry::Paged(_) => unreachable!("checked above"),
    };
    let internal_error = internal_scope.pop().await.map(|error| error.to_string());
    let oom_error = oom_scope.pop().await.map(|error| error.to_string());
    let validation_error = validation_scope.pop().await.map(|error| error.to_string());
    if let Some(candidate) = resolve_projected_compaction_candidate(
        candidate,
        internal_error,
        oom_error,
        validation_error,
        required,
    )? {
        let SurfaceGeometry::Packed(packed) = geometry else {
            unreachable!("geometry cannot change while optional resources are validated")
        };
        packed.projected.publish_contributor_compaction(candidate);
    }
    Ok(())
}

/// Device-local dependencies shared by every geometry-path constructor.
/// Keeping them together makes initial creation and transactional path
/// switching use the same resource factory contract.
struct GeometryResourceContext<'a> {
    device: &'a wgpu::Device,
    direct_bind_group_layout: &'a wgpu::BindGroupLayout,
    packed_bind_group_layout: &'a wgpu::BindGroupLayout,
    resident_draw_bind_group_layout: Option<&'a wgpu::BindGroupLayout>,
    resident_color_bind_group_layout: Option<&'a wgpu::BindGroupLayout>,
    surface_format: wgpu::TextureFormat,
    width: u32,
    height: u32,
}

fn create_geometry_resources(
    context: GeometryResourceContext<'_>,
    path: GeometryPath,
    renderer: &Renderer,
    prepared_renderer: Option<&PreparedRendererGeometryPath>,
) -> Result<SurfaceGeometry, SurfacePresenterError> {
    let GeometryResourceContext {
        device,
        direct_bind_group_layout,
        packed_bind_group_layout,
        resident_draw_bind_group_layout,
        resident_color_bind_group_layout,
        surface_format,
        width: _width,
        height: _height,
    } = context;
    debug_assert!(prepared_renderer.is_none_or(|prepared| prepared.path() == path));
    match path {
        GeometryPath::SortedIndexDirect => {
            let scene = renderer.scene().ok_or_else(|| {
                if renderer.has_scene() {
                    SurfacePresenterError::GeometrySourceUnavailable { path }
                } else {
                    SurfacePresenterError::SceneNotLoaded
                }
            })?;
            let world_covariance_terms = prepared_renderer
                .and_then(PreparedRendererGeometryPath::world_covariance_terms)
                .or(renderer.world_covariance_terms.as_deref())
                .ok_or(SurfacePresenterError::SceneNotLoaded)?;
            let alpha_values = prepared_renderer
                .and_then(PreparedRendererGeometryPath::alpha_values)
                .or(renderer.alpha_values.as_deref())
                .ok_or(SurfacePresenterError::SceneNotLoaded)?;
            let direct_scene = DirectSceneResources::new(
                device,
                direct_bind_group_layout,
                scene,
                world_covariance_terms,
                alpha_values,
            )?;
            Ok(SurfaceGeometry::Direct(Box::new(direct_scene)))
        }
        GeometryPath::PackedAtlas => {
            let resident_draw_bind_group_layout =
                resident_draw_bind_group_layout.ok_or_else(|| {
                    resident_gpu::ResidentGpuError::StorageBindingCountUnsupported(
                        device.limits().max_storage_buffers_per_shader_stage,
                    )
                })?;
            let resident_color_bind_group_layout =
                resident_color_bind_group_layout.ok_or_else(|| {
                    resident_gpu::ResidentGpuError::StorageBindingCountUnsupported(
                        device.limits().max_storage_buffers_per_shader_stage,
                    )
                })?;
            // Production Packed loads already own the compact source. The
            // fallback only serves explicit Direct -> Packed A/B switching.
            let fallback_scene;
            let resident_scene = if let Some(scene) = renderer.resident_scene() {
                scene
            } else {
                let scene = renderer
                    .scene()
                    .ok_or(SurfacePresenterError::SceneNotLoaded)?;
                fallback_scene = ResidentSceneCpu::encode(scene)?;
                &fallback_scene
            };
            let resident = resident_gpu::ResidentGpuResources::new(
                device,
                resident_draw_bind_group_layout,
                resident_color_bind_group_layout,
                resident_scene,
            )?;
            let projected = ProjectedQuadsGpu::new(device, surface_format, &resident)?;
            let raster_plan = SurfaceRasterExecutionPlan::ProjectedQuadsExact;
            Ok(SurfaceGeometry::Packed(Box::new(SurfacePackedRuntime {
                resident,
                projected,
                projected_cache: ProjectedCacheState::default(),
                preproject: None,
                preproject_state: PreprojectProducerState::default(),
                tiled: None,
                raster_plan,
                #[cfg(not(target_arch = "wasm32"))]
                phase_trace_emitted: 0,
                #[cfg(not(target_arch = "wasm32"))]
                last_count_resolve_prepare_ms: 0.0,
                #[cfg(target_arch = "wasm32")]
                pending: None,
                #[cfg(target_arch = "wasm32")]
                pending_gpu: None,
                #[cfg(target_arch = "wasm32")]
                gpu_order_warmed: false,
            })))
        }
        GeometryPath::PagedActiveAtlas => {
            let scene = renderer.scene().ok_or_else(|| {
                if renderer.has_scene() {
                    SurfacePresenterError::GeometrySourceUnavailable { path }
                } else {
                    SurfacePresenterError::SceneNotLoaded
                }
            })?;
            let pages = prepared_renderer
                .and_then(PreparedRendererGeometryPath::spatial_pages)
                .cloned()
                .or_else(|| renderer.spatial_pages.clone())
                .ok_or(SurfacePresenterError::SceneNotLoaded)?;
            let paged_scene =
                SurfacePagedRuntime::new(device, packed_bind_group_layout, scene, pages)?;
            Ok(SurfaceGeometry::Paged(Box::new(paged_scene)))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SurfaceResourcePlan {
    pub(crate) geometry_path: GeometryPath,
    pub(crate) direct_preflight: DirectScenePreflight,
    pub(crate) packed_preflight: PackedScenePreflight,
    pub(crate) resident_plan: ResidentGpuBytePlan,
    pub(crate) paged_plan: packed_gpu::PagedCompactBytePlan,
    pub(crate) required_texture_dimension: u32,
}

impl SurfaceResourcePlan {
    pub(crate) fn validate_selected_path(
        self,
        limits: &wgpu::Limits,
    ) -> Result<(), SurfacePresenterError> {
        match self.geometry_path {
            GeometryPath::SortedIndexDirect
                if self.direct_preflight.path == DirectScenePath::ActiveAtlasRequired =>
            {
                Err(DirectSceneError::ResourceLimitExceeded(Box::new(self.direct_preflight)).into())
            }
            GeometryPath::PackedAtlas => {
                if self.packed_preflight.path == PackedScenePath::PagingRequired {
                    return Err(DirectSceneError::PackedResourceLimitExceeded(Box::new(
                        self.packed_preflight,
                    ))
                    .into());
                }
                self.resident_plan.validate_limits(limits)?;
                Ok(())
            }
            GeometryPath::PagedActiveAtlas => {
                self.paged_plan.validate()?;
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn surface_resource_plan(
    geometry_path: GeometryPath,
    scene_splats: usize,
    sh_degree: u8,
    page_count: usize,
    page_capacity: usize,
    width: u32,
    height: u32,
    limits: &wgpu::Limits,
) -> Result<SurfaceResourcePlan, DirectSceneError> {
    let direct_preflight = direct_scene_preflight(scene_splats, sh_degree, limits)?;
    let paged_resident_capacity = match geometry_path {
        GeometryPath::PagedActiveAtlas => {
            let slot_count = page_count.clamp(1, DEFAULT_PAGED_ATLAS_SLOTS);
            slot_count
                .checked_mul(page_capacity.max(1))
                .ok_or(DirectSceneError::ResourceSizeOverflow)?
        }
        GeometryPath::SortedIndexDirect | GeometryPath::PackedAtlas => scene_splats,
    };
    let packed_preflight = packed_scene_preflight_with_limits(scene_splats, sh_degree, limits)?;
    let resident_plan = packed_preflight.resident_gpu;
    let paged_plan = packed_gpu::PagedCompactBytePlan::for_count(
        paged_resident_capacity,
        u64::from(limits.max_storage_buffer_binding_size).min(limits.max_buffer_size),
    )?;
    let required_texture_dimension = width.max(height);

    Ok(SurfaceResourcePlan {
        geometry_path,
        direct_preflight,
        packed_preflight,
        resident_plan,
        paged_plan,
        required_texture_dimension,
    })
}

pub(crate) fn try_prepare_then_commit<State, Prepared, Error>(
    state: &mut State,
    prepare: impl FnOnce(&State) -> Result<Prepared, Error>,
    commit: impl FnOnce(&mut State, Prepared),
) -> Result<(), Error> {
    let prepared = prepare(state)?;
    commit(state, prepared);
    Ok(())
}

fn surface_required_device_limits(
    adapter_limits: &wgpu::Limits,
    resource_plan: &SurfaceResourcePlan,
) -> Result<wgpu::Limits, SurfacePresenterError> {
    resource_plan.validate_selected_path(adapter_limits)?;
    if resource_plan.required_texture_dimension > adapter_limits.max_texture_dimension_2d {
        return Err(SurfacePresenterError::DeviceCreation(format!(
            "required texture dimension {} exceeds adapter limit {}",
            resource_plan.required_texture_dimension, adapter_limits.max_texture_dimension_2d
        )));
    }

    let required_storage_bytes = match resource_plan.geometry_path {
        GeometryPath::SortedIndexDirect => resource_plan
            .direct_preflight
            .requirements
            .iter()
            .map(|requirement| requirement.required_bytes)
            .max()
            .unwrap_or(0),
        GeometryPath::PackedAtlas => resource_plan.resident_plan.largest_storage_binding_bytes(),
        GeometryPath::PagedActiveAtlas => resource_plan
            .paged_plan
            .sorted_indices_bytes
            .max(resource_plan.paged_plan.hot_record_storage_bytes),
    };
    let required_storage_binding_size = u32::try_from(required_storage_bytes).map_err(|_| {
        SurfacePresenterError::DeviceCreation(format!(
            "selected geometry path requires a {required_storage_bytes}-byte storage binding, exceeding the wgpu limit representation"
        ))
    })?;

    let mut required_limits = wgpu::Limits::downlevel_defaults();
    // Preserve the adapter's full resize headroom. SurfacePresenter may be
    // created for a small window and later resized to a larger display; only
    // storage/buffer limits are intentionally requested scene-by-scene.
    required_limits.max_texture_dimension_2d = adapter_limits.max_texture_dimension_2d;
    required_limits.max_storage_buffer_binding_size = required_limits
        .max_storage_buffer_binding_size
        .max(required_storage_binding_size);
    required_limits.max_buffer_size = required_limits.max_buffer_size.max(required_storage_bytes);
    if resource_plan.geometry_path == GeometryPath::PackedAtlas
        || (resource_plan.geometry_path == GeometryPath::SortedIndexDirect
            && adapter_limits.max_storage_buffers_per_shader_stage
                >= resident_gpu::RESIDENT_COLOR_STORAGE_BINDINGS)
    {
        // A Direct presenter may transactionally move to full-resident Packed
        // later, but WebGPU device limits cannot be raised after creation.
        // Reserve the binding-count capability only when the adapter already
        // exposes it; lower-capability Direct devices still construct and the
        // later Packed request fails explicitly.
        required_limits.max_storage_buffers_per_shader_stage = required_limits
            .max_storage_buffers_per_shader_stage
            .max(resident_gpu::RESIDENT_COLOR_STORAGE_BINDINGS);
    }

    if !required_limits.check_limits(adapter_limits) {
        return Err(SurfacePresenterError::DeviceCreation(format!(
            "surface requirements exceed adapter capabilities; requested={required_limits:?}; adapter={adapter_limits:?}"
        )));
    }
    Ok(required_limits)
}

impl SurfacePresenter {
    /// Creates a presenter for an owned native window target.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn from_window<T>(
        target: T,
        width: u32,
        height: u32,
        renderer: &Renderer,
    ) -> Result<Self, SurfacePresenterError>
    where
        T: Into<wgpu::SurfaceTarget<'static>>,
    {
        Self::from_window_selected(target, width, height, renderer).await
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn from_window_selected<T>(
        target: T,
        width: u32,
        height: u32,
        renderer: &Renderer,
    ) -> Result<Self, SurfacePresenterError>
    where
        T: Into<wgpu::SurfaceTarget<'static>>,
    {
        let instance = create_surface_instance();
        let surface = instance
            .create_surface(target)
            .map_err(|_| SurfacePresenterError::SurfaceCreation)?;
        Self::from_surface_async(instance, surface, width, height, renderer).await
    }

    /// Creates a presenter from raw handles supplied by an embedding platform.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that the raw display and window handles remain valid until
    /// after the returned presenter is dropped.
    pub unsafe fn from_raw_handles(
        raw_display_handle: wgpu::rwh::RawDisplayHandle,
        raw_window_handle: wgpu::rwh::RawWindowHandle,
        width: u32,
        height: u32,
        renderer: &Renderer,
    ) -> Result<Self, SurfacePresenterError> {
        pollster::block_on(Self::from_raw_handles_selected(
            raw_display_handle,
            raw_window_handle,
            width,
            height,
            renderer,
        ))
    }

    async fn from_raw_handles_selected(
        raw_display_handle: wgpu::rwh::RawDisplayHandle,
        raw_window_handle: wgpu::rwh::RawWindowHandle,
        width: u32,
        height: u32,
        renderer: &Renderer,
    ) -> Result<Self, SurfacePresenterError> {
        let instance = create_surface_instance();
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle,
                raw_window_handle,
            })
        }
        .map_err(|_| SurfacePresenterError::SurfaceCreation)?;

        Self::from_surface_async(instance, surface, width, height, renderer).await
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn from_canvas(
        canvas: web_sys::HtmlCanvasElement,
        width: u32,
        height: u32,
        renderer: &Renderer,
    ) -> Result<Self, SurfacePresenterError> {
        Self::from_canvas_selected(canvas, width, height, renderer).await
    }

    #[cfg(target_arch = "wasm32")]
    async fn from_canvas_selected(
        canvas: web_sys::HtmlCanvasElement,
        width: u32,
        height: u32,
        renderer: &Renderer,
    ) -> Result<Self, SurfacePresenterError> {
        let instance = create_surface_instance();
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(|_| SurfacePresenterError::SurfaceCreation)?;
        Self::from_surface_async(instance, surface, width, height, renderer).await
    }

    async fn from_surface_async(
        instance: wgpu::Instance,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
        renderer: &Renderer,
    ) -> Result<Self, SurfacePresenterError> {
        if width == 0 || height == 0 {
            return Err(SurfacePresenterError::InvalidSurfaceSize);
        }

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|_| SurfacePresenterError::NoAdapter)?;

        let adapter_limits = adapter.limits();
        let adapter_context = SurfaceAdapterContext {
            info: adapter.get_info(),
            limits: adapter_limits,
        };
        Self::from_surface_with_adapter_async(
            adapter,
            surface,
            width,
            height,
            renderer,
            adapter_context,
        )
        .await
    }

    async fn from_surface_with_adapter_async(
        adapter: wgpu::Adapter,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
        renderer: &Renderer,
        adapter_context: SurfaceAdapterContext,
    ) -> Result<Self, SurfacePresenterError> {
        let SurfaceAdapterContext {
            info: adapter_info,
            limits: adapter_limits,
        } = adapter_context;
        let adapter_features = adapter.features();
        let downlevel = adapter.get_downlevel_capabilities();
        let indirect_execution_supported = supports_direct_gpu_order(&downlevel);
        let timestamp_queries_enabled = adapter_features.contains(wgpu::Features::TIMESTAMP_QUERY)
            && downlevel
                .flags
                .contains(wgpu::DownlevelFlags::NONBLOCKING_QUERY_RESOLVE);
        // Plan against the adapter's physical limits first. Device creation
        // then requests only the selected path's exact increase above portable
        // defaults rather than copying the adapter maximum wholesale.
        let scene_splats = renderer
            .scene_len()
            .ok_or(SurfacePresenterError::SceneNotLoaded)?;
        let sh_degree = renderer
            .scene_sh_degree()
            .ok_or(SurfacePresenterError::SceneNotLoaded)?;
        let geometry_path = renderer.geometry_path();
        let (page_count, page_capacity) = renderer
            .spatial_pages
            .as_ref()
            .map(|pages| (pages.page_count(), pages.page_capacity))
            .unwrap_or_default();
        if geometry_path == GeometryPath::PagedActiveAtlas && page_count == 0 {
            return Err(SurfacePresenterError::SceneNotLoaded);
        }
        let resource_plan = surface_resource_plan(
            geometry_path,
            scene_splats,
            sh_degree,
            page_count,
            page_capacity,
            width,
            height,
            &adapter_limits,
        )?;
        let required_limits = surface_required_device_limits(&adapter_limits, &resource_plan)?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: wgpu_label("gsplat-surface-device"),
                required_features: if timestamp_queries_enabled {
                    wgpu::Features::TIMESTAMP_QUERY
                        | if cfg!(any(target_os = "macos", target_os = "ios")) {
                            wgpu::Features::empty()
                        } else {
                            adapter_features & wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS
                        }
                } else {
                    wgpu::Features::empty()
                },
                required_limits,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|err| {
                SurfacePresenterError::DeviceCreation(format!(
                    "{err}; adapter={adapter_info:?}; limits={adapter_limits:?}"
                ))
            })?;

        let caps = surface.get_capabilities(&adapter);
        let Some(format) = caps.formats.first().copied() else {
            return Err(SurfacePresenterError::NoSurfaceFormat);
        };
        let present_mode = select_present_mode(&caps);
        let alpha_mode = caps
            .alpha_modes
            .first()
            .copied()
            .unwrap_or(wgpu::CompositeAlphaMode::Opaque);

        let max_texture_dimension_2d = device.limits().max_texture_dimension_2d.max(1);
        let (surface_width, surface_height) = (width, height);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: surface_width,
            height: surface_height,
            present_mode,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        surface.configure(&device, &surface_config);
        if let Some(err) = error_scope.pop().await {
            return Err(SurfacePresenterError::SurfaceConfigure(err.to_string()));
        }

        // Every shared pipeline/layout and the selected geometry is one
        // unpublished candidate. WebGPU reports constructor failures only
        // when these async scopes are popped, so do not build or expose any
        // part of the presenter outside this transaction.
        let (validation_scope, oom_scope, internal_scope) = (
            device.push_error_scope(wgpu::ErrorFilter::Validation),
            device.push_error_scope(wgpu::ErrorFilter::OutOfMemory),
            device.push_error_scope(wgpu::ErrorFilter::Internal),
        );
        let direct_bind_group_layout = create_direct_bind_group_layout(&device);
        let direct_pipeline = create_direct_pipeline(&device, &direct_bind_group_layout, format);
        let packed_bind_group_layout = packed_gpu::create_packed_bind_group_layout(&device);
        let packed_pipeline =
            packed_gpu::create_packed_pipeline(&device, &packed_bind_group_layout, format);
        let (
            resident_draw_bind_group_layout,
            resident_draw_pipeline,
            resident_color_bind_group_layout,
            resident_color_pipeline,
        ) = if supports_resident_pipeline_layout(&device.limits()) {
            let draw_layout = resident_gpu::create_resident_draw_bind_group_layout(&device);
            let draw_pipeline =
                resident_gpu::create_resident_draw_pipeline(&device, &draw_layout, format);
            let color_layout = resident_gpu::create_resident_color_bind_group_layout(&device);
            let color_pipeline =
                resident_gpu::create_resident_color_pipeline(&device, &color_layout);
            (
                Some(draw_layout),
                Some(draw_pipeline),
                Some(color_layout),
                Some(color_pipeline),
            )
        } else {
            (None, None, None, None)
        };
        let geometry_result = create_geometry_resources(
            GeometryResourceContext {
                device: &device,
                direct_bind_group_layout: &direct_bind_group_layout,
                packed_bind_group_layout: &packed_bind_group_layout,
                resident_draw_bind_group_layout: resident_draw_bind_group_layout.as_ref(),
                resident_color_bind_group_layout: resident_color_bind_group_layout.as_ref(),
                surface_format: format,
                width: surface_width,
                height: surface_height,
            },
            geometry_path,
            renderer,
            None,
        );
        let gpu_order_telemetry =
            GpuOrderTelemetry::new(&device, &queue, timestamp_queries_enabled);
        let cpu_order_completion_telemetry = CpuOrderCompletionTelemetry::new(&device);
        let projected_draw_telemetry = ProjectedDrawTelemetry::new(&device);
        let gpu_producer_telemetry = GpuProducerTelemetry::new(&device);
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
        let mut geometry = geometry_result?;
        // Compact is an optional performance graph. Validate it only after the
        // mandatory Candidate geometry is known-good, and publish it only on
        // complete success so an OOM/validation failure cannot reject Packed.
        prepare_optional_projected_compaction(
            &device,
            format,
            indirect_execution_supported,
            &mut geometry,
            false,
        )
        .await?;
        let addressable_splat_count = geometry.addressable_splat_count();

        Ok(Self {
            surface,
            device,
            queue,
            direct_pipeline,
            direct_bind_group_layout,
            packed_pipeline,
            packed_bind_group_layout,
            resident_draw_pipeline,
            resident_draw_bind_group_layout,
            resident_color_pipeline,
            resident_color_bind_group_layout,
            surface_config,
            surface_configuration_valid: true,
            max_texture_dimension_2d,
            adapter_max_storage_buffers_per_shader_stage: adapter_limits
                .max_storage_buffers_per_shader_stage,
            adapter_max_storage_buffer_binding_size: u64::from(
                adapter_limits.max_storage_buffer_binding_size,
            )
            .min(adapter_limits.max_buffer_size),
            indirect_execution_supported,
            addressable_splat_count,
            instance_count: 0,
            geometry,
            gpu_order_telemetry,
            cpu_order_completion_telemetry,
            projected_draw_telemetry,
            gpu_producer_telemetry,
            gpu_order_producer: SurfaceGpuOrderProducer::PostSort,
            gpu_producer_measurement_enabled: false,
            last_gpu_producer_submission: TelemetrySubmission::NotRequested,
            last_actual_gpu_order_producer: None,
            projected_draw_execution: ProjectedDrawExecution::Compact,
            projected_probe_generation: 0,
            projected_draw_sample_request: None,
            last_projected_draw_submission: TelemetrySubmission::NotRequested,
            last_frame_presented: false,
            last_presented_size: None,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), SurfacePresenterError> {
        if width == 0 || height == 0 {
            return Err(SurfacePresenterError::InvalidSurfaceSize);
        }
        if width > self.max_texture_dimension_2d || height > self.max_texture_dimension_2d {
            return Err(SurfacePresenterError::GpuDimensionsUnsupported {
                width,
                height,
                max_dimension: self.max_texture_dimension_2d,
            });
        }
        if self.surface_size() == (width, height) && self.surface_configuration_valid {
            return Ok(());
        }
        #[cfg(target_arch = "wasm32")]
        {
            Err(SurfacePresenterError::SurfaceResizePreparationRequired)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let SurfaceGeometry::Packed(packed) = &mut self.geometry {
                if let Some(tiled) = packed.tiled.as_mut() {
                    tiled.resize(&self.device, width, height)?;
                }
                packed.phase_trace_emitted = 0;
            }
            self.surface_config.width = width;
            self.surface_config.height = height;
            self.surface.configure(&self.device, &self.surface_config);
            self.surface_configuration_valid = true;
            if let SurfaceGeometry::Packed(packed) = &mut self.geometry {
                packed.preproject_state.invalidate_order();
            }
            self.gpu_order_telemetry.invalidate_generation();
            self.cpu_order_completion_telemetry.invalidate_generation();
            self.projected_draw_telemetry.invalidate_generation();
            self.gpu_producer_telemetry.invalidate_generation();
            Ok(())
        }
    }

    #[cfg(target_arch = "wasm32")]
    async fn configure_surface_scoped(
        &self,
        config: &wgpu::SurfaceConfiguration,
    ) -> Option<SurfacePresenterError> {
        let (validation_scope, oom_scope, internal_scope) = (
            self.device.push_error_scope(wgpu::ErrorFilter::Validation),
            self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory),
            self.device.push_error_scope(wgpu::ErrorFilter::Internal),
        );
        self.surface.configure(&self.device, config);
        let internal_error = internal_scope.pop().await;
        let oom_error = oom_scope.pop().await;
        let validation_error = validation_scope.pop().await;
        classify_surface_configure_scope_errors(
            internal_error.map(|error| error.to_string()),
            oom_error.map(|error| error.to_string()),
            validation_error.map(|error| error.to_string()),
        )
    }

    /// Transactionally reconfigures the browser Surface for the production
    /// Packed + Projected path. The published size remains unchanged until all
    /// WebGPU error scopes complete. A failed attempt reconfigures the old
    /// descriptor; if that rollback also fails, presentation becomes
    /// fail-closed until a later successful transactional resize.
    #[cfg(target_arch = "wasm32")]
    pub async fn resize_async(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<(), SurfacePresenterError> {
        if width == 0 || height == 0 {
            return Err(SurfacePresenterError::InvalidSurfaceSize);
        }
        if width > self.max_texture_dimension_2d || height > self.max_texture_dimension_2d {
            return Err(SurfacePresenterError::GpuDimensionsUnsupported {
                width,
                height,
                max_dimension: self.max_texture_dimension_2d,
            });
        }
        if !matches!(
            &self.geometry,
            SurfaceGeometry::Packed(packed)
                if packed.raster_plan == SurfaceRasterExecutionPlan::ProjectedQuadsExact
        ) {
            return Err(SurfacePresenterError::SurfaceResizeUnsupported);
        }
        if self.surface_size() == (width, height) && self.surface_configuration_valid {
            return Ok(());
        }

        let previous = self.surface_config.clone();
        let mut candidate = previous.clone();
        candidate.width = width;
        candidate.height = height;
        if let Some(resize_error) = self.configure_surface_scoped(&candidate).await {
            let resize_message = resize_error.to_string();
            if let Some(rollback_error) = self.configure_surface_scoped(&previous).await {
                self.surface_configuration_valid = false;
                return Err(SurfacePresenterError::SurfaceResizeRollbackFailed {
                    resize_error: resize_message,
                    rollback_error: rollback_error.to_string(),
                });
            }
            self.surface_configuration_valid = true;
            return Err(resize_error);
        }

        self.surface_config = candidate;
        self.surface_configuration_valid = true;
        self.last_frame_presented = false;
        self.last_presented_size = None;
        if let SurfaceGeometry::Packed(packed) = &mut self.geometry {
            packed.projected_cache.key = None;
            packed.preproject_state.invalidate_order();
            packed.pending = None;
            packed.pending_gpu = None;
        }
        self.gpu_order_telemetry.invalidate_generation();
        self.cpu_order_completion_telemetry.invalidate_generation();
        self.projected_draw_telemetry.invalidate_generation();
        self.gpu_producer_telemetry.invalidate_generation();
        Ok(())
    }

    pub const fn surface_size(&self) -> (u32, u32) {
        (self.surface_config.width, self.surface_config.height)
    }

    /// Actual dimensions of the raster target before presentation. Packed
    /// tiled rendering owns an intermediate image; the other plans rasterize
    /// directly into the Surface at its configured size.
    pub fn internal_render_size(&self) -> (u32, u32) {
        match &self.geometry {
            SurfaceGeometry::Packed(packed)
                if packed.raster_plan == SurfaceRasterExecutionPlan::TiledExact =>
            {
                packed
                    .tiled
                    .as_ref()
                    .expect("TiledExact plan owns its diagnostic raster")
                    .size()
            }
            SurfaceGeometry::Direct(_) | SurfaceGeometry::Packed(_) | SurfaceGeometry::Paged(_) => {
                self.surface_size()
            }
        }
    }

    /// Number of splat records allocated by the selected Surface geometry.
    pub const fn addressable_splat_count(&self) -> usize {
        self.addressable_splat_count
    }

    /// Physical adapter storage-binding count used by exactness admission.
    pub const fn adapter_max_storage_buffers_per_shader_stage(&self) -> u32 {
        self.adapter_max_storage_buffers_per_shader_stage
    }

    /// Effective physical adapter size for one storage-buffer binding.
    pub const fn adapter_max_storage_buffer_binding_size(&self) -> u64 {
        self.adapter_max_storage_buffer_binding_size
    }

    pub fn set_frame_latency(&mut self, latency: u32) {
        let latency = latency.clamp(1, 4);
        if self.surface_config.desired_maximum_frame_latency == latency {
            return;
        }

        self.surface_config.desired_maximum_frame_latency = latency;
        self.surface.configure(&self.device, &self.surface_config);
        self.gpu_order_telemetry.invalidate_generation();
        self.cpu_order_completion_telemetry.invalidate_generation();
        self.projected_draw_telemetry.invalidate_generation();
        self.gpu_producer_telemetry.invalidate_generation();
    }

    pub const fn geometry_path(&self) -> GeometryPath {
        self.geometry.path()
    }

    /// Whether the most recent top-level render call actually presented a
    /// drawable. Exact-count preparation, timeouts, and errors leave this
    /// false.
    pub(crate) const fn last_frame_presented(&self) -> bool {
        self.last_frame_presented
    }

    pub(crate) const fn last_presented_size(&self) -> Option<(u32, u32)> {
        self.last_presented_size
    }

    /// Current raster execution plan. Packed defaults to exact preprojected
    /// hardware quads; Direct and Paged retain their global-quad implementation.
    pub const fn raster_execution_plan(&self) -> SurfaceRasterExecutionPlan {
        match &self.geometry {
            SurfaceGeometry::Packed(packed) => packed.raster_plan,
            SurfaceGeometry::Direct(_) | SurfaceGeometry::Paged(_) => {
                SurfaceRasterExecutionPlan::GlobalQuads
            }
        }
    }

    pub(crate) fn projected_contributor_indirect_draw_enabled(&self) -> bool {
        matches!(
            &self.geometry,
            SurfaceGeometry::Packed(packed)
                if packed.raster_plan == SurfaceRasterExecutionPlan::ProjectedQuadsExact
                    && packed.projected.resolve_draw_execution(ProjectedDrawExecution::Compact)
                        == ProjectedDrawExecution::Compact
        )
    }

    pub(crate) fn set_projected_draw_execution(
        &mut self,
        execution: ProjectedDrawExecution,
        force_projection: bool,
        sample_request: Option<ProjectedDrawSampleRequest>,
    ) {
        self.projected_draw_execution = execution;
        self.projected_draw_sample_request = sample_request;
        if force_projection {
            self.projected_probe_generation = self.projected_probe_generation.wrapping_add(1);
        }
    }

    pub(crate) fn resolved_projected_draw_execution(&self) -> ProjectedDrawExecution {
        match &self.geometry {
            SurfaceGeometry::Packed(packed)
                if packed.raster_plan == SurfaceRasterExecutionPlan::ProjectedQuadsExact =>
            {
                packed
                    .projected
                    .resolve_draw_execution(self.projected_draw_execution)
            }
            SurfaceGeometry::Direct(_) | SurfaceGeometry::Packed(_) | SurfaceGeometry::Paged(_) => {
                ProjectedDrawExecution::Candidate
            }
        }
    }

    pub(crate) const fn gpu_order_producer(&self) -> SurfaceGpuOrderProducer {
        self.gpu_order_producer
    }

    pub(crate) fn invalidate_gpu_order_producer_prefix(&mut self) {
        if let SurfaceGeometry::Packed(packed) = &mut self.geometry {
            packed.preproject_state.invalidate_order();
        }
    }

    pub(crate) fn set_gpu_producer_measurement_enabled(&mut self, enabled: bool) {
        if self.gpu_producer_measurement_enabled == enabled {
            return;
        }
        self.gpu_producer_measurement_enabled = enabled;
        self.gpu_producer_telemetry.invalidate_generation();
        self.last_gpu_producer_submission = TelemetrySubmission::NotRequested;
    }

    fn preproject_graph_is_prepared(&self) -> bool {
        matches!(
            &self.geometry,
            SurfaceGeometry::Packed(packed) if packed.preproject.is_some()
        )
    }

    fn validate_preproject_producer_context(&self) -> Result<(), SurfacePresenterError> {
        if !self.indirect_execution_supported
            || !matches!(
                &self.geometry,
                SurfaceGeometry::Packed(packed)
                    if packed.raster_plan == SurfaceRasterExecutionPlan::ProjectedQuadsExact
                        && packed.projected.resolve_draw_execution(ProjectedDrawExecution::Compact)
                            == ProjectedDrawExecution::Compact
            )
        {
            return Err(SurfacePresenterError::PreprojectProducerIncompatible);
        }
        Ok(())
    }

    fn create_preproject_candidate(
        &self,
    ) -> Result<PreparedSurfaceGpuProducer, SurfacePresenterError> {
        self.validate_preproject_producer_context()?;
        let SurfaceGeometry::Packed(packed) = &self.geometry else {
            unreachable!("preproject context validation requires Packed")
        };
        if packed.preproject.is_some() {
            return Ok(PreparedSurfaceGpuProducer::AlreadyPrepared);
        }
        Ok(PreparedSurfaceGpuProducer::Preproject(Box::new(
            PreprojectedGpuOrder::new(&self.device, self.surface_config.format, &packed.resident)?,
        )))
    }

    fn publish_preproject_candidate(&mut self, prepared: PreparedSurfaceGpuProducer) {
        match prepared {
            PreparedSurfaceGpuProducer::AlreadyPrepared => {}
            PreparedSurfaceGpuProducer::Preproject(candidate) => {
                let SurfaceGeometry::Packed(packed) = &mut self.geometry else {
                    unreachable!("preproject candidate must publish into Packed")
                };
                debug_assert!(packed.preproject.is_none());
                packed.preproject = Some(*candidate);
                packed.preproject_state.invalidate_order();
            }
        }
    }

    /// Builds and publishes the complete dormant preproject graph only after
    /// validation/OOM/internal scopes have completed. It does not change the
    /// selected producer or frame state.
    pub async fn prepare_gpu_order_producer(
        &mut self,
        producer: SurfaceGpuOrderProducer,
    ) -> Result<(), SurfacePresenterError> {
        if producer == SurfaceGpuOrderProducer::PostSort {
            return self.prepare_post_sort_gpu_order().await;
        }
        self.validate_preproject_producer_context()?;
        if self.preproject_graph_is_prepared() {
            return Ok(());
        }

        let (validation_scope, oom_scope, internal_scope) = (
            self.device.push_error_scope(wgpu::ErrorFilter::Validation),
            self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory),
            self.device.push_error_scope(wgpu::ErrorFilter::Internal),
        );
        let prepared = self.create_preproject_candidate();
        let internal_error = internal_scope.pop().await;
        let oom_error = oom_scope.pop().await;
        let validation_error = validation_scope.pop().await;
        if let Some(error) = classify_surface_gpu_order_scope_errors(
            GeometryPath::PackedAtlas,
            internal_error.map(|error| error.to_string()),
            oom_error.map(|error| error.to_string()),
            validation_error.map(|error| error.to_string()),
        ) {
            return Err(error);
        }
        self.publish_preproject_candidate(prepared?);
        Ok(())
    }

    /// Selects a completely prepared Packed GPU producer. Native callers may
    /// synchronously prepare the dormant graph; browser callers must await
    /// [`Self::prepare_gpu_order_producer`] first.
    pub(crate) fn set_gpu_order_producer(
        &mut self,
        producer: SurfaceGpuOrderProducer,
    ) -> Result<(), SurfacePresenterError> {
        if self.gpu_order_producer == producer {
            return Ok(());
        }
        if producer == SurfaceGpuOrderProducer::Preproject {
            self.validate_preproject_producer_context()?;
            if !self.preproject_graph_is_prepared() {
                #[cfg(target_arch = "wasm32")]
                return Err(SurfacePresenterError::GpuProducerPreparationRequired);
                #[cfg(not(target_arch = "wasm32"))]
                pollster::block_on(self.prepare_gpu_order_producer(producer))?;
            }
        } else if !self.post_sort_gpu_order_is_prepared() {
            #[cfg(target_arch = "wasm32")]
            return Err(SurfacePresenterError::GpuOrderPreparationRequired);
            #[cfg(not(target_arch = "wasm32"))]
            pollster::block_on(self.prepare_post_sort_gpu_order())?;
        }
        self.gpu_order_producer = producer;
        if let SurfaceGeometry::Packed(packed) = &mut self.geometry {
            packed.preproject_state.invalidate_order();
            packed.projected_cache.key = None;
        }
        // Producer workloads and count buffers are different generations.
        // Old projected tickets become terminal invalidations and can never
        // be mistaken for a preproject completion.
        self.gpu_order_telemetry.invalidate_generation();
        self.projected_draw_telemetry.invalidate_generation();
        self.gpu_producer_telemetry.invalidate_generation();
        self.last_projected_draw_submission = TelemetrySubmission::NotRequested;
        self.last_gpu_producer_submission = TelemetrySubmission::NotRequested;
        self.last_actual_gpu_order_producer = None;
        Ok(())
    }

    /// Force an A/B raster plan without changing CPU/GPU ordering policy.
    /// There is deliberately no point-count heuristic here; strategy code can
    /// evaluate measured telemetry and call this explicit knob later.
    pub fn set_raster_execution_plan(
        &mut self,
        plan: SurfaceRasterExecutionPlan,
    ) -> Result<(), SurfacePresenterError> {
        if self.gpu_order_producer == SurfaceGpuOrderProducer::Preproject
            && plan != SurfaceRasterExecutionPlan::ProjectedQuadsExact
        {
            return Err(SurfacePresenterError::PreprojectProducerIncompatible);
        }
        match &mut self.geometry {
            SurfaceGeometry::Packed(packed) => {
                if packed.raster_plan == plan {
                    return Ok(());
                }
                if plan == SurfaceRasterExecutionPlan::TiledExact {
                    let tiled = ResidentTiledGpu::new(
                        &self.device,
                        self.surface_config.format,
                        &packed.resident,
                        self.surface_config.width,
                        self.surface_config.height,
                    )?;
                    // Publish only after complete construction succeeds.
                    packed.tiled = Some(tiled);
                } else {
                    // The diagnostic owner includes three 16-byte source
                    // planes plus viewport/work buffers. It must not remain in
                    // the production large-scene residency after the A/B run.
                    packed.tiled = None;
                }
                packed.raster_plan = plan;
                #[cfg(not(target_arch = "wasm32"))]
                {
                    packed.phase_trace_emitted = 0;
                }
                #[cfg(target_arch = "wasm32")]
                {
                    packed.pending = None;
                    packed.pending_gpu = None;
                }
                self.gpu_order_telemetry.invalidate_generation();
                self.cpu_order_completion_telemetry.invalidate_generation();
                self.projected_draw_telemetry.invalidate_generation();
                self.gpu_producer_telemetry.invalidate_generation();
                Ok(())
            }
            SurfaceGeometry::Direct(_) | SurfaceGeometry::Paged(_)
                if plan == SurfaceRasterExecutionPlan::GlobalQuads =>
            {
                Ok(())
            }
            SurfaceGeometry::Direct(_) | SurfaceGeometry::Paged(_) => {
                Err(SurfacePresenterError::GpuOrderUnsupported)
            }
        }
    }

    pub const fn tiled_entry_count(&self) -> Option<u32> {
        match &self.geometry {
            SurfaceGeometry::Packed(packed) => match &packed.tiled {
                Some(tiled) => Some(tiled.active_entry_count()),
                None => Some(0),
            },
            SurfaceGeometry::Direct(_) | SurfaceGeometry::Paged(_) => None,
        }
    }

    pub const fn tiled_entry_capacity(&self) -> Option<u32> {
        match &self.geometry {
            SurfaceGeometry::Packed(packed) => match &packed.tiled {
                Some(tiled) => Some(tiled.entry_capacity()),
                None => Some(0),
            },
            SurfaceGeometry::Direct(_) | SurfaceGeometry::Paged(_) => None,
        }
    }

    /// Switches the Surface geometry path, clearing and rebuilding the GPU
    /// scene resources for the new path from the renderer's loaded scene.
    ///
    /// This is an experimental A/B benchmark knob: callers must keep
    /// `renderer`'s loaded scene in sync with the presenter that was created
    /// from it. The device was sized for the initially selected path, so a
    /// target path needing larger bindings can return an error; preparation is
    /// transactional and leaves the current path intact in that case.
    pub fn set_geometry_path(
        &mut self,
        path: GeometryPath,
        renderer: &Renderer,
    ) -> Result<(), SurfacePresenterError> {
        if self.gpu_order_producer == SurfaceGpuOrderProducer::Preproject
            && path != GeometryPath::PackedAtlas
        {
            return Err(SurfacePresenterError::PreprojectProducerIncompatible);
        }
        if self.geometry.path() == path {
            return Ok(());
        }

        try_prepare_then_commit(
            self,
            |presenter| presenter.prepare_geometry_resources(path, renderer),
            |presenter, geometry| {
                presenter.addressable_splat_count = geometry.addressable_splat_count();
                presenter.geometry = geometry;
                presenter.instance_count = 0;
                presenter.gpu_order_telemetry.invalidate_generation();
                presenter.projected_draw_telemetry.invalidate_generation();
                presenter.gpu_producer_telemetry.invalidate_generation();
                presenter
                    .cpu_order_completion_telemetry
                    .invalidate_generation();
            },
        )
    }

    /// Builds a complete browser geometry candidate while the active
    /// presenter remains untouched. Packed construction includes resident
    /// buffers, exact projected-contributor pipelines/buffers, and—when the
    /// selected order policy needs it—the sorter and projected order binding.
    /// The candidate becomes publishable only after every WebGPU error scope
    /// has completed successfully.
    #[cfg(target_arch = "wasm32")]
    pub(crate) async fn prepare_geometry_path_async(
        &self,
        path: GeometryPath,
        renderer: &Renderer,
        prepared_renderer: &PreparedRendererGeometryPath,
        prepare_gpu_order: bool,
        require_projected_compaction: bool,
    ) -> Result<PreparedSurfaceGeometryPath, SurfacePresenterError> {
        debug_assert_eq!(prepared_renderer.path(), path);

        let (validation_scope, oom_scope, internal_scope) = (
            self.device.push_error_scope(wgpu::ErrorFilter::Validation),
            self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory),
            self.device.push_error_scope(wgpu::ErrorFilter::Internal),
        );
        let geometry_result = (|| {
            let mut geometry = create_geometry_resources(
                GeometryResourceContext {
                    device: &self.device,
                    direct_bind_group_layout: &self.direct_bind_group_layout,
                    packed_bind_group_layout: &self.packed_bind_group_layout,
                    resident_draw_bind_group_layout: self.resident_draw_bind_group_layout.as_ref(),
                    resident_color_bind_group_layout: self
                        .resident_color_bind_group_layout
                        .as_ref(),
                    surface_format: self.surface_config.format,
                    width: self.surface_config.width,
                    height: self.surface_config.height,
                },
                path,
                renderer,
                Some(prepared_renderer),
            )?;
            if prepare_gpu_order {
                if !self.indirect_execution_supported {
                    return Err(SurfacePresenterError::GpuOrderUnsupported);
                }
                let order = self.create_gpu_order_candidate_for_geometry(&geometry)?;
                Self::publish_gpu_order_candidate_to_geometry(&mut geometry, order);
            }
            Ok(geometry)
        })();
        let internal_error = internal_scope.pop().await;
        let oom_error = oom_scope.pop().await;
        let validation_error = validation_scope.pop().await;
        if let Some(error) = classify_surface_geometry_scope_errors(
            path,
            internal_error.map(|error| error.to_string()),
            oom_error.map(|error| error.to_string()),
            validation_error.map(|error| error.to_string()),
        ) {
            return Err(error);
        }

        let mut geometry = geometry_result?;
        prepare_optional_projected_compaction(
            &self.device,
            self.surface_config.format,
            self.indirect_execution_supported,
            &mut geometry,
            require_projected_compaction,
        )
        .await?;
        Ok(PreparedSurfaceGeometryPath { geometry })
    }

    /// Atomically publishes an already scoped browser geometry graph. This
    /// commit path performs no allocation and cannot report a late GPU error.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn publish_geometry_path_candidate(
        &mut self,
        prepared: PreparedSurfaceGeometryPath,
    ) {
        debug_assert_ne!(self.geometry.path(), prepared.geometry.path());
        self.addressable_splat_count = prepared.geometry.addressable_splat_count();
        self.geometry = prepared.geometry;
        self.instance_count = 0;
        self.gpu_order_telemetry.invalidate_generation();
        self.cpu_order_completion_telemetry.invalidate_generation();
        self.projected_draw_telemetry.invalidate_generation();
        self.gpu_producer_telemetry.invalidate_generation();
    }

    fn prepare_geometry_resources(
        &self,
        path: GeometryPath,
        renderer: &Renderer,
    ) -> Result<SurfaceGeometry, SurfacePresenterError> {
        let mut geometry = create_geometry_resources(
            GeometryResourceContext {
                device: &self.device,
                direct_bind_group_layout: &self.direct_bind_group_layout,
                packed_bind_group_layout: &self.packed_bind_group_layout,
                resident_draw_bind_group_layout: self.resident_draw_bind_group_layout.as_ref(),
                resident_color_bind_group_layout: self.resident_color_bind_group_layout.as_ref(),
                surface_format: self.surface_config.format,
                width: self.surface_config.width,
                height: self.surface_config.height,
            },
            path,
            renderer,
            None,
        )?;
        // Native event loops expose this historical synchronous A/B switch.
        // Its capability and size gates run before construction; the browser
        // path above uses the fully isolated asynchronous transaction.
        if self.indirect_execution_supported
            && let SurfaceGeometry::Packed(packed) = &mut geometry
            && let Some(candidate) = packed.projected.create_contributor_compaction_candidate(
                &self.device,
                self.surface_config.format,
                &packed.resident,
            )?
        {
            packed.projected.publish_contributor_compaction(candidate);
        }
        Ok(geometry)
    }

    pub fn render_sorted_indices(
        &mut self,
        scene: &SceneBuffers,
        sorted_indices: &[u32],
        camera: &Camera,
        refresh_indices: bool,
    ) -> Result<(), SurfacePresenterError> {
        self.last_frame_presented = false;
        self.last_presented_size = None;
        if !matches!(self.geometry, SurfaceGeometry::Paged(_)) {
            return self.render_cpu_sorted_indices(sorted_indices, camera, refresh_indices);
        }
        self.instance_count = match &mut self.geometry {
            SurfaceGeometry::Paged(paged) => paged.prepare(
                &self.queue,
                scene,
                camera,
                self.surface_config.width,
                self.surface_config.height,
            )?,
            SurfaceGeometry::Direct(_) | SurfaceGeometry::Packed(_) => unreachable!(),
        };
        self.present_geometry(camera)
    }

    /// Draw Direct or Packed CPU order without requiring the wide source
    /// buffers that production Packed loading deliberately releases.
    pub(crate) fn render_cpu_sorted_indices(
        &mut self,
        sorted_indices: &[u32],
        camera: &Camera,
        refresh_indices: bool,
    ) -> Result<(), SurfacePresenterError> {
        self.render_cpu_sorted_indices_tracked(sorted_indices, camera, refresh_indices, None)
            .map(|_| ())
    }

    pub(crate) fn render_cpu_sorted_indices_tracked(
        &mut self,
        sorted_indices: &[u32],
        camera: &Camera,
        refresh_indices: bool,
        completion: Option<CpuCompletionSampleRequest>,
    ) -> Result<TelemetrySubmission, SurfacePresenterError> {
        self.last_frame_presented = false;
        self.last_presented_size = None;
        #[cfg(target_arch = "wasm32")]
        if matches!(
            self.geometry,
            SurfaceGeometry::Packed(ref packed)
                if packed.raster_plan == SurfaceRasterExecutionPlan::TiledExact
        ) {
            return self.render_packed_tiled_cpu_web(
                sorted_indices,
                camera,
                refresh_indices,
                completion,
            );
        }
        self.instance_count = match &mut self.geometry {
            SurfaceGeometry::Direct(direct) => direct.prepare_cpu(
                &self.queue,
                sorted_indices,
                camera,
                self.surface_config.width,
                self.surface_config.height,
                refresh_indices,
            )?,
            SurfaceGeometry::Packed(packed) => {
                let instance_count = packed.resident.prepare_cpu_order(
                    &self.queue,
                    sorted_indices,
                    camera,
                    self.surface_config.width,
                    self.surface_config.height,
                    refresh_indices,
                )?;
                // The rank-indexed projection cache is valid only for the
                // exact order that populated it. Camera/viewport/backend/count
                // changes are covered by the cache key at draw time.
                packed.projected_cache.invalidate_order_if(refresh_indices);
                instance_count
            }
            SurfaceGeometry::Paged(_) => {
                return Err(SurfacePresenterError::PagedAtlasUnsupported);
            }
        };
        #[cfg(not(target_arch = "wasm32"))]
        if matches!(
            self.geometry,
            SurfaceGeometry::Packed(ref packed)
                if packed.raster_plan == SurfaceRasterExecutionPlan::TiledExact
        ) {
            return self.present_packed_tiled_native(camera, self.instance_count, completion);
        }
        self.present_geometry_tracked(camera, completion)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn present_packed_tiled_native(
        &mut self,
        camera: &Camera,
        work_count: u32,
        completion: Option<CpuCompletionSampleRequest>,
    ) -> Result<TelemetrySubmission, SurfacePresenterError> {
        let storage_bindings = self.device.limits().max_storage_buffers_per_shader_stage;
        let color_pipeline = self
            .resident_color_pipeline
            .as_ref()
            .ok_or_else(|| resident_pipelines_unavailable(storage_bindings))?;
        let count_started = timer_now();
        let mut count_encoder =
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: wgpu_label("gsplat-surface-resident-tiled-count-encoder"),
                });
        {
            let SurfaceGeometry::Packed(packed) = &mut self.geometry else {
                unreachable!();
            };
            packed.resident.encode_color_resolve_if_needed(
                &self.queue,
                color_pipeline,
                &mut count_encoder,
                camera,
                self.device.limits().max_compute_workgroups_per_dimension,
            )?;
            packed
                .tiled
                .as_mut()
                .expect("TiledExact plan owns its diagnostic raster")
                .encode_count_prepass(
                    &self.device,
                    &self.queue,
                    &packed.resident,
                    &packed.resident.order_buffer,
                    work_count,
                    &mut count_encoder,
                )?;
        }
        self.queue.submit(Some(count_encoder.finish()));
        {
            let SurfaceGeometry::Packed(packed) = &mut self.geometry else {
                unreachable!();
            };
            packed
                .tiled
                .as_mut()
                .expect("TiledExact plan owns its diagnostic raster")
                .resolve_count_and_prepare(&self.device, &self.queue)?;
            packed.last_count_resolve_prepare_ms = timer_elapsed_ms(count_started);
        }

        let Some(frame) = self.acquire_surface_texture()? else {
            return Ok(if completion.is_some() {
                TelemetrySubmission::SurfaceUnavailable
            } else {
                TelemetrySubmission::NotRequested
            });
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: wgpu_label("gsplat-surface-resident-tiled-finish-encoder"),
            });
        {
            let SurfaceGeometry::Packed(packed) = &mut self.geometry else {
                unreachable!();
            };
            packed
                .tiled
                .as_mut()
                .expect("TiledExact plan owns its diagnostic raster")
                .encode_finish(
                    &self.device,
                    &self.queue,
                    &packed.resident,
                    ResidentTiledFinish {
                        source_order_btf: &packed.resident.order_buffer,
                        work_count,
                        target: &view,
                    },
                    &mut encoder,
                )?;
        }
        let command_buffer = encoder.finish();
        let mut completion_ticket = completion.and_then(|request| {
            self.cpu_order_completion_telemetry.begin_sample(
                request.camera_revision,
                request.preprocess_ms,
                request.sort_ms,
            )
        });
        let submitted_ticket = completion_ticket.as_ref().map(|ticket| ticket.ticket);
        if let (Some(ticket), Some(request)) = (completion_ticket.take(), completion) {
            self.cpu_order_completion_telemetry
                .arm(&command_buffer, ticket, request.started);
        }
        self.queue.submit(Some(command_buffer));
        self.maybe_emit_tiled_phase_trace()?;
        self.present_frame(frame);
        Ok(match (completion, submitted_ticket) {
            (None, _) => TelemetrySubmission::NotRequested,
            (Some(_), Some(ticket)) => TelemetrySubmission::Issued(ticket),
            (Some(_), None) => TelemetrySubmission::RingBusy,
        })
    }

    #[cfg(target_arch = "wasm32")]
    fn render_packed_tiled_cpu_web(
        &mut self,
        sorted_indices: &[u32],
        camera: &Camera,
        refresh_indices: bool,
        completion: Option<CpuCompletionSampleRequest>,
    ) -> Result<TelemetrySubmission, SurfacePresenterError> {
        let gpu_pending_ready = {
            let SurfaceGeometry::Packed(packed) = &mut self.geometry else {
                unreachable!();
            };
            match packed.pending_gpu.as_mut() {
                None => true,
                Some(pending) if pending.count_ready => true,
                Some(_) => {
                    let ready = packed
                        .tiled
                        .as_mut()
                        .expect("TiledExact plan owns its diagnostic raster")
                        .try_resolve_count_and_prepare(&self.device, &self.queue)?
                        .is_some();
                    if ready && let Some(pending) = packed.pending_gpu.as_mut() {
                        pending.count_ready = true;
                    }
                    ready
                }
            }
        };
        if !gpu_pending_ready {
            return Ok(TelemetrySubmission::GpuOrderPreparationPending);
        }
        let stale_gpu_ticket = {
            let SurfaceGeometry::Packed(packed) = &mut self.geometry else {
                unreachable!();
            };
            packed
                .pending_gpu
                .take()
                .and_then(|pending| pending.telemetry_ticket)
        };
        if let Some(ticket) = stale_gpu_ticket {
            self.gpu_order_telemetry.fail_encoding(
                ticket,
                SurfaceOrderMeasurementFailureReason::GenerationInvalidated,
            );
        }

        let resolved_pending = {
            let SurfaceGeometry::Packed(packed) = &mut self.geometry else {
                unreachable!();
            };
            match packed.pending {
                None => None,
                Some(pending) if pending.count_ready => Some(pending),
                Some(mut pending) => {
                    let ready = packed
                        .tiled
                        .as_mut()
                        .expect("TiledExact plan owns its diagnostic raster")
                        .try_resolve_count_and_prepare(&self.device, &self.queue)?
                        .is_some();
                    if ready {
                        pending.count_ready = true;
                        packed.pending = Some(pending);
                        Some(pending)
                    } else {
                        None
                    }
                }
            }
        };
        if let SurfaceGeometry::Packed(packed) = &self.geometry
            && packed.pending.is_some()
            && resolved_pending.is_none()
        {
            return Ok(TelemetrySubmission::GpuOrderPreparationPending);
        }
        if let Some(pending) = resolved_pending {
            let revision_matches = completion.is_none_or(|current| {
                pending
                    .completion
                    .is_some_and(|previous| previous.camera_revision == current.camera_revision)
            });
            if pending.camera == *camera && revision_matches {
                let Some(frame) = self.acquire_surface_texture()? else {
                    return Ok(if pending.completion.is_some() {
                        TelemetrySubmission::SurfaceUnavailable
                    } else {
                        TelemetrySubmission::NotRequested
                    });
                };
                let view = frame
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                let mut encoder =
                    self.device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: wgpu_label("gsplat-surface-resident-tiled-web-finish-encoder"),
                        });
                {
                    let SurfaceGeometry::Packed(packed) = &mut self.geometry else {
                        unreachable!();
                    };
                    packed
                        .tiled
                        .as_mut()
                        .expect("TiledExact plan owns its diagnostic raster")
                        .encode_finish(
                            &self.device,
                            &self.queue,
                            &packed.resident,
                            ResidentTiledFinish {
                                source_order_btf: &packed.resident.order_buffer,
                                work_count: pending.work_count,
                                target: &view,
                            },
                            &mut encoder,
                        )?;
                    packed.pending = None;
                }
                let command_buffer = encoder.finish();
                let mut completion_ticket = pending.completion.and_then(|request| {
                    self.cpu_order_completion_telemetry.begin_sample(
                        request.camera_revision,
                        request.preprocess_ms,
                        request.sort_ms,
                    )
                });
                let submitted_ticket = completion_ticket.as_ref().map(|ticket| ticket.ticket);
                if let (Some(ticket), Some(request)) =
                    (completion_ticket.take(), pending.completion)
                {
                    self.cpu_order_completion_telemetry.arm(
                        &command_buffer,
                        ticket,
                        request.started,
                    );
                }
                self.queue.submit(Some(command_buffer));
                self.present_frame(frame);
                return Ok(match (pending.completion, submitted_ticket) {
                    (None, _) => TelemetrySubmission::NotRequested,
                    (Some(_), Some(ticket)) => TelemetrySubmission::Issued(ticket),
                    (Some(_), None) => TelemetrySubmission::RingBusy,
                });
            }
            // The callback completed for an obsolete camera revision. Its
            // exact buffers are valid but must never be presented as the new
            // camera; release the pending marker and start the current frame.
            let SurfaceGeometry::Packed(packed) = &mut self.geometry else {
                unreachable!();
            };
            packed.pending = None;
        }

        let storage_bindings = self.device.limits().max_storage_buffers_per_shader_stage;
        let color_pipeline = self
            .resident_color_pipeline
            .as_ref()
            .ok_or_else(|| resident_pipelines_unavailable(storage_bindings))?;
        self.instance_count = {
            let SurfaceGeometry::Packed(packed) = &mut self.geometry else {
                unreachable!();
            };
            packed.resident.prepare_cpu_order(
                &self.queue,
                sorted_indices,
                camera,
                self.surface_config.width,
                self.surface_config.height,
                refresh_indices,
            )?
        };
        let mut count_encoder =
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: wgpu_label("gsplat-surface-resident-tiled-web-count-encoder"),
                });
        {
            let SurfaceGeometry::Packed(packed) = &mut self.geometry else {
                unreachable!();
            };
            packed.resident.encode_color_resolve_if_needed(
                &self.queue,
                color_pipeline,
                &mut count_encoder,
                camera,
                self.device.limits().max_compute_workgroups_per_dimension,
            )?;
            packed
                .tiled
                .as_mut()
                .expect("TiledExact plan owns its diagnostic raster")
                .encode_count_prepass(
                    &self.device,
                    &self.queue,
                    &packed.resident,
                    &packed.resident.order_buffer,
                    self.instance_count,
                    &mut count_encoder,
                )?;
        }
        self.queue.submit(Some(count_encoder.finish()));
        {
            let SurfaceGeometry::Packed(packed) = &mut self.geometry else {
                unreachable!();
            };
            packed.pending = Some(WebPendingTiledFrame {
                camera: *camera,
                work_count: self.instance_count,
                completion,
                count_ready: false,
            });
            let ready = packed
                .tiled
                .as_mut()
                .expect("TiledExact plan owns its diagnostic raster")
                .try_resolve_count_and_prepare(&self.device, &self.queue)?
                .is_some();
            if ready && let Some(pending) = packed.pending.as_mut() {
                pending.count_ready = true;
            }
        }
        Ok(TelemetrySubmission::GpuOrderPreparationPending)
    }

    fn post_sort_gpu_order_is_prepared(&self) -> bool {
        Self::geometry_gpu_order_is_prepared(&self.geometry)
    }

    fn gpu_order_is_prepared(&self) -> bool {
        match self.gpu_order_producer {
            SurfaceGpuOrderProducer::PostSort => self.post_sort_gpu_order_is_prepared(),
            SurfaceGpuOrderProducer::Preproject => self.preproject_graph_is_prepared(),
        }
    }

    fn geometry_gpu_order_is_prepared(geometry: &SurfaceGeometry) -> bool {
        match geometry {
            SurfaceGeometry::Direct(direct) => direct.gpu_order().is_some(),
            SurfaceGeometry::Packed(packed) => {
                packed.resident.gpu_order().is_some()
                    && packed.projected.gpu_order_bind_group_is_prepared()
            }
            SurfaceGeometry::Paged(_) => false,
        }
    }

    fn create_gpu_order_candidate(&self) -> Result<PreparedSurfaceGpuOrder, SurfacePresenterError> {
        self.create_gpu_order_candidate_for_geometry(&self.geometry)
    }

    fn create_gpu_order_candidate_for_geometry(
        &self,
        geometry: &SurfaceGeometry,
    ) -> Result<PreparedSurfaceGpuOrder, SurfacePresenterError> {
        let resident_draw_layout = self.resident_draw_bind_group_layout.as_ref();
        let storage_bindings = self.device.limits().max_storage_buffers_per_shader_stage;
        match geometry {
            SurfaceGeometry::Direct(direct) => {
                if direct.gpu_order().is_some() {
                    return Ok(PreparedSurfaceGpuOrder::AlreadyPrepared);
                }
                Ok(PreparedSurfaceGpuOrder::Direct(
                    direct
                        .create_gpu_order_candidate(&self.device, &self.direct_bind_group_layout)?,
                ))
            }
            SurfaceGeometry::Packed(packed) => {
                let order_prepared = packed.resident.gpu_order().is_some();
                let projected_prepared = packed.projected.gpu_order_bind_group_is_prepared();
                if order_prepared && projected_prepared {
                    return Ok(PreparedSurfaceGpuOrder::AlreadyPrepared);
                }
                if order_prepared != projected_prepared {
                    return Err(resident_gpu::ResidentGpuError::GpuOrderInternal(
                        "GPU-order resources were only partially published".into(),
                    )
                    .into());
                }
                let layout = resident_draw_layout
                    .ok_or_else(|| resident_pipelines_unavailable(storage_bindings))?;
                let order = packed
                    .resident
                    .create_gpu_order_candidate(&self.device, layout)?;
                let projected_bind_group = packed.projected.create_gpu_order_bind_group_candidate(
                    &self.device,
                    &packed.resident,
                    order.sorter.final_ids(),
                    order.sorter.indirect_args(),
                );
                Ok(PreparedSurfaceGpuOrder::Packed {
                    order,
                    projected_bind_group,
                })
            }
            SurfaceGeometry::Paged(_) => Err(SurfacePresenterError::GpuOrderUnsupported),
        }
    }

    fn publish_gpu_order_candidate(&mut self, prepared: PreparedSurfaceGpuOrder) {
        Self::publish_gpu_order_candidate_to_geometry(&mut self.geometry, prepared);
    }

    fn publish_gpu_order_candidate_to_geometry(
        geometry: &mut SurfaceGeometry,
        prepared: PreparedSurfaceGpuOrder,
    ) {
        match (geometry, prepared) {
            (_, PreparedSurfaceGpuOrder::AlreadyPrepared) => {}
            (SurfaceGeometry::Direct(direct), PreparedSurfaceGpuOrder::Direct(order)) => {
                direct.publish_gpu_order(order);
            }
            (
                SurfaceGeometry::Packed(packed),
                PreparedSurfaceGpuOrder::Packed {
                    order,
                    projected_bind_group,
                },
            ) => {
                // Both assignments are infallible. Until this point neither
                // candidate is reachable from the live presenter.
                packed.resident.publish_gpu_order(order);
                packed
                    .projected
                    .publish_gpu_order_bind_group(projected_bind_group);
            }
            _ => unreachable!("GPU-order candidate must match the live geometry path"),
        }
    }

    async fn prepare_post_sort_gpu_order(&mut self) -> Result<(), SurfacePresenterError> {
        if !self.indirect_execution_supported {
            return Err(SurfacePresenterError::GpuOrderUnsupported);
        }
        if self.post_sort_gpu_order_is_prepared() {
            return Ok(());
        }

        let path = self.geometry.path();
        let (validation_scope, oom_scope, internal_scope) = (
            self.device.push_error_scope(wgpu::ErrorFilter::Validation),
            self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory),
            self.device.push_error_scope(wgpu::ErrorFilter::Internal),
        );
        let prepared = self.create_gpu_order_candidate();
        let internal_error = internal_scope.pop().await;
        let oom_error = oom_scope.pop().await;
        let validation_error = validation_scope.pop().await;
        if let Some(error) = classify_surface_gpu_order_scope_errors(
            path,
            internal_error.map(|error| error.to_string()),
            oom_error.map(|error| error.to_string()),
            validation_error.map(|error| error.to_string()),
        ) {
            return Err(error);
        }
        self.publish_gpu_order_candidate(prepared?);
        Ok(())
    }

    /// Pre-creates the selected GPU producer graph outside a measured or
    /// presented frame. Browser callers must await this before selecting GPU
    /// or Adaptive ordering.
    pub async fn prepare_gpu_order(&mut self) -> Result<(), SurfacePresenterError> {
        match self.gpu_order_producer {
            SurfaceGpuOrderProducer::PostSort => self.prepare_post_sort_gpu_order().await,
            SurfaceGpuOrderProducer::Preproject => {
                self.prepare_gpu_order_producer(SurfaceGpuOrderProducer::Preproject)
                    .await
            }
        }
    }

    /// Native selection remains synchronous. Web cannot block the browser
    /// event loop waiting for `pop_error_scope`, so it accepts only a
    /// candidate already published by [`Self::prepare_gpu_order`].
    pub(crate) fn prepare_direct_gpu_order(&mut self) -> Result<(), SurfacePresenterError> {
        if !self.indirect_execution_supported {
            return Err(SurfacePresenterError::GpuOrderUnsupported);
        }
        if self.gpu_order_is_prepared() {
            return Ok(());
        }
        #[cfg(target_arch = "wasm32")]
        {
            Err(SurfacePresenterError::GpuOrderPreparationRequired)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            pollster::block_on(self.prepare_gpu_order())
        }
    }

    /// Generates and stably sorts Direct depth pairs on this presenter's GPU,
    /// then draws from the resident pair buffer in the same submission.
    pub(crate) fn render_direct_gpu_order(
        &mut self,
        camera: &Camera,
        refresh_order: bool,
        camera_revision: u64,
        completion_started: TimerInstant,
    ) -> Result<TelemetrySubmission, SurfacePresenterError> {
        self.last_frame_presented = false;
        self.last_presented_size = None;
        let projected_sample_request = self.projected_draw_sample_request.take();
        self.last_projected_draw_submission = TelemetrySubmission::NotRequested;
        self.last_gpu_producer_submission = TelemetrySubmission::NotRequested;
        self.last_actual_gpu_order_producer = None;
        if self.gpu_order_producer == SurfaceGpuOrderProducer::Preproject {
            if projected_sample_request.is_some() {
                return Err(SurfacePresenterError::PreprojectProducerIncompatible);
            }
            return self.render_preproject_gpu_order(
                camera,
                refresh_order,
                camera_revision,
                completion_started,
            );
        }
        let resident_draw_layout = self.resident_draw_bind_group_layout.as_ref();
        let storage_bindings = self.device.limits().max_storage_buffers_per_shader_stage;
        self.instance_count = match &mut self.geometry {
            SurfaceGeometry::Direct(direct) => direct.prepare_gpu(
                &self.device,
                &self.direct_bind_group_layout,
                &self.queue,
                camera,
                self.surface_config.width,
                self.surface_config.height,
            )?,
            SurfaceGeometry::Packed(packed) => {
                let count = u32::try_from(packed.resident.capacity)
                    .map_err(|_| resident_gpu::ResidentGpuError::AddressSpaceExceeded)?;
                packed.resident.prepare_gpu_order_draw(
                    &self.device,
                    resident_draw_layout
                        .ok_or_else(|| resident_pipelines_unavailable(storage_bindings))?,
                    &self.queue,
                    resident_gpu::ResidentGpuOrderDraw {
                        camera,
                        width: self.surface_config.width,
                        height: self.surface_config.height,
                        instance_count: count,
                        order_stride_words: 1,
                        order_id_offset_words: 0,
                    },
                )?;
                let order = packed
                    .resident
                    .gpu_order()
                    .ok_or(SurfacePresenterError::GpuOrderUnsupported)?;
                packed.projected.ensure_gpu_order_bind_group(
                    &self.device,
                    &packed.resident,
                    order.sorter.final_ids(),
                    order.sorter.indirect_args(),
                )?;
                packed.projected_cache.invalidate_order_if(refresh_order);
                count
            }
            SurfaceGeometry::Paged(_) => {
                return Err(SurfacePresenterError::GpuOrderUnsupported);
            }
        };

        if matches!(
            self.geometry,
            SurfaceGeometry::Packed(ref packed)
                if packed.raster_plan == SurfaceRasterExecutionPlan::TiledExact
        ) {
            #[cfg(not(target_arch = "wasm32"))]
            {
                return self.render_packed_tiled_gpu_native(
                    camera,
                    refresh_order,
                    camera_revision,
                    completion_started,
                );
            }
            #[cfg(target_arch = "wasm32")]
            {
                return self.render_packed_tiled_gpu_web(
                    camera,
                    refresh_order,
                    camera_revision,
                    completion_started,
                );
            }
        }

        // Browser WebGPU can lazily compile the first large Resident radix
        // pipeline. As with the exact tiled path below, do not publish that
        // initialization turn as a drawable or issue a formal ticket: submit
        // the complete order compute once, then retry the same frame plan.
        // This prevents a transient zero-count/black first GPU frame while
        // preserving the exact camera and full source membership.
        let packed_geometry = matches!(&self.geometry, SurfaceGeometry::Packed(_));
        #[cfg(target_arch = "wasm32")]
        let gpu_order_prepared = match &self.geometry {
            SurfaceGeometry::Packed(packed) => packed.gpu_order_warmed,
            SurfaceGeometry::Direct(_) | SurfaceGeometry::Paged(_) => true,
        };
        #[cfg(not(target_arch = "wasm32"))]
        let gpu_order_prepared = true;
        let preparation = gpu_order_preparation_plan(
            cfg!(target_arch = "wasm32"),
            packed_geometry,
            refresh_order,
            gpu_order_prepared,
        );

        // Acquire before reserving a telemetry slot so a surface error cannot
        // strand the slot in Encoding. The Web warmup intentionally has no
        // surface. An ordinary timeout cannot issue a formal count receipt for
        // an unpresented frame, nor read C/D from an older projected cache.
        let frame = if preparation.acquire_surface {
            self.acquire_surface_texture()?
        } else {
            None
        };
        if let Some(submission) = unavailable_gpu_surface_submission(preparation, frame.is_some()) {
            if projected_sample_request.is_some() {
                self.last_projected_draw_submission = TelemetrySubmission::SurfaceUnavailable;
            }
            if self.gpu_producer_measurement_enabled
                && matches!(
                    &self.geometry,
                    SurfaceGeometry::Packed(packed)
                        if packed.raster_plan == SurfaceRasterExecutionPlan::ProjectedQuadsExact
                )
            {
                self.last_gpu_producer_submission = TelemetrySubmission::SurfaceUnavailable;
            }
            return Ok(submission);
        }
        let resident_draw_pipeline = self.resident_draw_pipeline.as_ref();
        let resident_color_pipeline = self.resident_color_pipeline.as_ref();
        let allow_timestamps = match &self.geometry {
            SurfaceGeometry::Direct(direct) => !direct
                .gpu_order()
                .ok_or(SurfacePresenterError::GpuOrderUnsupported)?
                .sorter
                .is_empty(),
            SurfaceGeometry::Packed(packed) => !packed
                .resident
                .gpu_order()
                .ok_or(SurfacePresenterError::GpuOrderUnsupported)?
                .sorter
                .is_empty(),
            SurfaceGeometry::Paged(_) => false,
        };
        let mut telemetry_ticket = preparation
            .reserve_measurement
            .then(|| {
                self.gpu_order_telemetry
                    .begin_sample(camera_revision, allow_timestamps)
            })
            .flatten();
        let timestamp_range = telemetry_ticket.as_ref().and_then(|ticket| {
            ticket
                .query_set
                .as_ref()
                .map(|query_set| GpuOrderTimestampRange {
                    query_set,
                    keygen_begin_index: 0,
                    keygen_end_index: 1,
                    radix_begin_index: 2,
                    radix_end_index: 3,
                })
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: wgpu_label("gsplat-surface-direct-gpu-order-encoder"),
            });
        let projected_order_generation = match &self.geometry {
            SurfaceGeometry::Packed(packed) => packed.projected_cache.order_generation(),
            SurfaceGeometry::Direct(_) | SurfaceGeometry::Paged(_) => 0,
        };
        let projected_draw_execution = self.resolved_projected_draw_execution();
        let projected_cache_key = ProjectedCacheKey {
            order_source: ProjectedOrderSource::Gpu,
            order_generation: projected_order_generation,
            camera: *camera,
            width: self.surface_config.width,
            height: self.surface_config.height,
            // GPU projection dispatches resident capacity and guards ranks
            // with the authoritative indirect visible count. Any change to
            // that count comes from an order refresh, which invalidates the
            // cache before this key is considered.
            draw_count_guard: self.instance_count,
            draw_execution: projected_draw_execution,
            probe_generation: self.projected_probe_generation,
        };
        let mut projection_rebuilt = false;
        let color_result = match &mut self.geometry {
            SurfaceGeometry::Direct(direct) => {
                let gpu_order = direct
                    .gpu_order()
                    .ok_or(SurfacePresenterError::GpuOrderUnsupported)?;
                if refresh_order {
                    gpu_order
                        .sorter
                        .encode_with_timestamps(&mut encoder, timestamp_range);
                }
                Ok(false)
            }
            SurfaceGeometry::Packed(packed) => {
                if refresh_order {
                    packed
                        .resident
                        .gpu_order()
                        .ok_or(SurfacePresenterError::GpuOrderUnsupported)?
                        .sorter
                        .encode_with_timestamps(&mut encoder, timestamp_range);
                }
                let color_resolved = packed
                    .resident
                    .encode_color_resolve_if_needed(
                        &self.queue,
                        resident_color_pipeline
                            .ok_or_else(|| resident_pipelines_unavailable(storage_bindings))?,
                        &mut encoder,
                        camera,
                        self.device.limits().max_compute_workgroups_per_dimension,
                    )
                    .map_err(SurfacePresenterError::from)?;
                if frame.is_some()
                    && packed.raster_plan == SurfaceRasterExecutionPlan::ProjectedQuadsExact
                    && packed.projected_cache.needs_projection(projected_cache_key)
                {
                    packed
                        .projected
                        .encode_gpu_projection_for_draw(&mut encoder, projected_draw_execution)?;
                    packed.projected_cache.publish(projected_cache_key);
                    projection_rebuilt = true;
                }
                Ok(color_resolved)
            }
            SurfaceGeometry::Paged(_) => unreachable!(),
        };
        let color_resolved = match color_result {
            Ok(value) => value,
            Err(error) => {
                if let Some(ticket) = telemetry_ticket.take() {
                    self.gpu_order_telemetry.cancel(ticket);
                }
                return Err(error);
            }
        };

        if let Some(frame) = frame.as_ref() {
            let view = frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            match &self.geometry {
                SurfaceGeometry::Direct(direct) => {
                    let order = direct
                        .gpu_order()
                        .ok_or(SurfacePresenterError::GpuOrderUnsupported)?;
                    order.sorter.set_indirect_vertex_count(
                        &self.queue,
                        resident_gpu::RESIDENT_QUAD_VERTEX_COUNT,
                    );
                    encode_splat_indirect_draw_into(
                        &mut encoder,
                        &SplatIndirectDraw {
                            pass_label: "gsplat-surface-direct-gpu-order-draw-pass",
                            view: &view,
                            pipeline: &self.direct_pipeline,
                            bind_group: &order.bind_group,
                            clear: wgpu::Color::BLACK,
                            indirect_args: order.sorter.indirect_args(),
                        },
                    );
                }
                SurfaceGeometry::Packed(packed) => {
                    let order = packed
                        .resident
                        .gpu_order()
                        .ok_or(SurfacePresenterError::GpuOrderUnsupported)?;
                    order.sorter.set_indirect_vertex_count(
                        &self.queue,
                        resident_gpu::RESIDENT_QUAD_VERTEX_COUNT,
                    );
                    match packed.raster_plan {
                        SurfaceRasterExecutionPlan::ProjectedQuadsExact
                            if projected_draw_execution == ProjectedDrawExecution::Compact =>
                        {
                            encode_splat_indirect_draw_into(
                                &mut encoder,
                                &SplatIndirectDraw {
                                    pass_label: "gsplat-surface-projected-contributors-gpu-order-draw-pass",
                                    view: &view,
                                    pipeline: packed
                                        .projected
                                        .contributor_draw_pipeline()
                                        .expect("available compaction owns its draw pipeline"),
                                    bind_group: packed
                                        .projected
                                        .contributor_draw_bind_group()
                                        .expect("available compaction owns its draw binding"),
                                    clear: wgpu::Color::BLACK,
                                    indirect_args: packed
                                        .projected
                                        .contributor_indirect_args()
                                        .expect("available compaction owns its draw args"),
                                },
                            );
                        }
                        SurfaceRasterExecutionPlan::ProjectedQuadsExact => {
                            // This fallback is retained only for exact
                            // downlevel symmetry. GPU ordering itself requires
                            // indirect execution, so production GPU-order
                            // presenters take the compact branch above.
                            encode_splat_indirect_draw_into(
                                &mut encoder,
                                &SplatIndirectDraw {
                                    pass_label: "gsplat-surface-projected-quads-gpu-order-draw-pass",
                                    view: &view,
                                    pipeline: packed.projected.draw_pipeline(),
                                    bind_group: packed.projected.draw_bind_group(),
                                    clear: wgpu::Color::BLACK,
                                    indirect_args: order.sorter.indirect_args(),
                                },
                            );
                        }
                        SurfaceRasterExecutionPlan::GlobalQuads => {
                            encode_splat_indirect_draw_into(
                                &mut encoder,
                                &SplatIndirectDraw {
                                    pass_label: "gsplat-surface-resident-gpu-order-draw-pass",
                                    view: &view,
                                    pipeline: resident_draw_pipeline.ok_or_else(|| {
                                        resident_pipelines_unavailable(storage_bindings)
                                    })?,
                                    bind_group: &order.draw_bind_group,
                                    clear: wgpu::Color::BLACK,
                                    indirect_args: order.sorter.indirect_args(),
                                },
                            );
                        }
                        SurfaceRasterExecutionPlan::TiledExact => unreachable!(),
                    }
                }
                SurfaceGeometry::Paged(_) => unreachable!(),
            }
        }

        let projected_metadata = projected_sample_request.and_then(|request| {
            frame.as_ref()?;
            match &self.geometry {
                SurfaceGeometry::Packed(packed)
                    if packed.raster_plan == SurfaceRasterExecutionPlan::ProjectedQuadsExact =>
                {
                    Some(ProjectedDrawSampleMetadata {
                        camera_revision: request.camera_revision,
                        execution: projected_draw_execution,
                        order_backend: request.order_backend,
                        projection_generation: packed.projected_cache.projection_generation(),
                        probe_generation: self.projected_probe_generation,
                        projection_rebuilt,
                        order_refreshed: request.order_refreshed,
                    })
                }
                SurfaceGeometry::Direct(_)
                | SurfaceGeometry::Packed(_)
                | SurfaceGeometry::Paged(_) => None,
            }
        });
        let mut projected_reservation = projected_metadata
            .and_then(|metadata| self.projected_draw_telemetry.begin_sample(metadata));
        let producer_metadata = (self.gpu_producer_measurement_enabled
            && frame.is_some()
            && projected_draw_execution == ProjectedDrawExecution::Compact)
            .then(|| match &self.geometry {
                SurfaceGeometry::Packed(packed)
                    if packed.raster_plan == SurfaceRasterExecutionPlan::ProjectedQuadsExact =>
                {
                    u32::try_from(packed.resident.capacity)
                        .ok()
                        .map(|source_count| GpuProducerSampleMetadata {
                            camera_revision,
                            producer: SurfaceGpuOrderProducer::PostSort,
                            order_generation: packed.projected_cache.order_generation(),
                            projection_generation: packed.projected_cache.projection_generation(),
                            source_count,
                            order_refreshed: refresh_order,
                            draw_scope: if refresh_order {
                                SurfaceGpuProducerDrawScope::ExactCurrentContributors
                            } else {
                                SurfaceGpuProducerDrawScope::StaleOrderCandidates
                            },
                        })
                }
                SurfaceGeometry::Direct(_)
                | SurfaceGeometry::Packed(_)
                | SurfaceGeometry::Paged(_) => None,
            })
            .flatten();
        let mut producer_reservation = producer_metadata
            .and_then(|metadata| self.gpu_producer_telemetry.begin_sample(metadata));
        if let Some(reservation) = projected_reservation.as_ref()
            && let SurfaceGeometry::Packed(packed) = &self.geometry
        {
            let candidate_args = packed
                .resident
                .gpu_order()
                .ok_or(SurfacePresenterError::GpuOrderUnsupported)?
                .sorter
                .indirect_args();
            let candidate = ProjectedDrawCountSource::indirect_args(candidate_args);
            let (contributor_buffer, contributor_offset) =
                packed.projected.contributor_count_buffer_and_offset();
            let contributor = ProjectedDrawCountSource::raw(contributor_buffer, contributor_offset);
            let drawn = packed
                .projected
                .contributor_indirect_args()
                .filter(|_| projected_draw_execution == ProjectedDrawExecution::Compact)
                .map(ProjectedDrawCountSource::indirect_args)
                .unwrap_or(candidate);
            debug_assert!(self.projected_draw_telemetry.encode_count_readback(
                &mut encoder,
                reservation,
                candidate,
                contributor,
                drawn,
            ));
        }
        if let Some(reservation) = producer_reservation.as_ref()
            && let SurfaceGeometry::Packed(packed) = &self.geometry
        {
            let (contributor_buffer, contributor_offset) =
                packed.projected.contributor_count_buffer_and_offset();
            let contributor = GpuProducerCountSource::raw(contributor_buffer, contributor_offset);
            let drawn = GpuProducerCountSource::indirect_args(
                packed
                    .projected
                    .contributor_indirect_args()
                    .expect("producer telemetry requires forced Compact"),
            );
            debug_assert!(self.gpu_producer_telemetry.encode_count_readback(
                &mut encoder,
                reservation,
                contributor,
                drawn,
            ));
        }

        if let Some(ticket) = telemetry_ticket.as_ref() {
            match &self.geometry {
                SurfaceGeometry::Direct(direct) => {
                    let indirect_args = direct
                        .gpu_order()
                        .ok_or(SurfacePresenterError::GpuOrderUnsupported)?
                        .sorter
                        .indirect_args();
                    self.gpu_order_telemetry
                        .encode_readback(&mut encoder, ticket, indirect_args);
                }
                SurfaceGeometry::Packed(packed)
                    if packed.raster_plan == SurfaceRasterExecutionPlan::ProjectedQuadsExact =>
                {
                    let candidate_args = packed
                        .resident
                        .gpu_order()
                        .ok_or(SurfacePresenterError::GpuOrderUnsupported)?
                        .sorter
                        .indirect_args();
                    let candidate = InstanceCountSource::indirect_args(candidate_args);
                    let (contributor_buffer, contributor_offset) =
                        packed.projected.contributor_count_buffer_and_offset();
                    let contributor =
                        InstanceCountSource::raw(contributor_buffer, contributor_offset);
                    let drawn = packed
                        .projected
                        .contributor_indirect_args()
                        .filter(|_| projected_draw_execution == ProjectedDrawExecution::Compact)
                        .map(InstanceCountSource::indirect_args)
                        .unwrap_or(candidate);
                    self.gpu_order_telemetry.encode_instance_count_readback(
                        &mut encoder,
                        ticket,
                        candidate,
                        contributor,
                        drawn,
                        projected_draw_execution == ProjectedDrawExecution::Compact,
                    );
                }
                SurfaceGeometry::Packed(packed) => {
                    let indirect_args = packed
                        .resident
                        .gpu_order()
                        .ok_or(SurfacePresenterError::GpuOrderUnsupported)?
                        .sorter
                        .indirect_args();
                    self.gpu_order_telemetry
                        .encode_readback(&mut encoder, ticket, indirect_args);
                }
                SurfaceGeometry::Paged(_) => unreachable!(),
            }
        }

        if frame.is_none() && !refresh_order && !color_resolved {
            return Ok(TelemetrySubmission::NotRequested);
        }
        let command_buffer = encoder.finish();
        let submitted_ticket = telemetry_ticket.as_ref().map(|ticket| ticket.ticket);
        let projected_started = projected_sample_request.map(|request| request.started);
        let projected_submitted_ticket = match (projected_reservation.take(), projected_started) {
            (Some(reservation), Some(started)) => self
                .projected_draw_telemetry
                .arm(&command_buffer, reservation, started)
                .map(|ticket| ticket.ticket),
            (None, _) | (_, None) => None,
        };
        let producer_submitted_ticket = producer_reservation.take().and_then(|reservation| {
            self.gpu_producer_telemetry
                .arm(&command_buffer, reservation, completion_started)
                .map(|ticket| ticket.ticket)
        });
        if let Some(ticket) = telemetry_ticket.take() {
            self.gpu_order_telemetry
                .arm(&command_buffer, ticket, completion_started);
        }
        self.queue.submit(Some(command_buffer));
        #[cfg(target_arch = "wasm32")]
        if preparation.pending
            && let SurfaceGeometry::Packed(packed) = &mut self.geometry
        {
            packed.gpu_order_warmed = true;
        }
        if let Some(frame) = frame {
            self.present_frame(frame);
            if matches!(self.geometry, SurfaceGeometry::Packed(_)) {
                self.last_actual_gpu_order_producer = Some(SurfaceGpuOrderProducer::PostSort);
            }
        }
        self.last_projected_draw_submission = match (projected_metadata, projected_submitted_ticket)
        {
            (None, _) => TelemetrySubmission::NotRequested,
            (Some(_), Some(ticket)) => TelemetrySubmission::Issued(ticket),
            (Some(_), None) => TelemetrySubmission::RingBusy,
        };
        self.last_gpu_producer_submission = match (producer_metadata, producer_submitted_ticket) {
            (None, _) => TelemetrySubmission::NotRequested,
            (Some(_), Some(ticket)) => TelemetrySubmission::Issued(ticket),
            (Some(_), None) => TelemetrySubmission::RingBusy,
        };
        if preparation.pending {
            return Ok(TelemetrySubmission::GpuOrderPreparationPending);
        }
        Ok(match (refresh_order, submitted_ticket) {
            (false, _) => TelemetrySubmission::NotRequested,
            (true, Some(ticket)) => TelemetrySubmission::Issued(ticket),
            (true, None) => TelemetrySubmission::RingBusy,
        })
    }

    fn render_preproject_gpu_order(
        &mut self,
        camera: &Camera,
        refresh_order: bool,
        camera_revision: u64,
        completion_started: TimerInstant,
    ) -> Result<TelemetrySubmission, SurfacePresenterError> {
        self.validate_preproject_producer_context()?;
        {
            let SurfaceGeometry::Packed(packed) = &self.geometry else {
                return Err(SurfacePresenterError::PreprojectProducerIncompatible);
            };
            if packed.preproject.is_none() {
                return Err(SurfacePresenterError::GpuProducerPreparationRequired);
            }
            // A non-refresh frame must never draw uninitialized or invalidated
            // ID/indirect state. Session scheduling normally forces this; the
            // presenter guard keeps direct callers fail-closed as well.
            packed.preproject_state.require_order(refresh_order)?;
        }

        let Some(frame) = self.acquire_surface_texture()? else {
            self.last_gpu_producer_submission = if self.gpu_producer_measurement_enabled {
                TelemetrySubmission::SurfaceUnavailable
            } else {
                TelemetrySubmission::NotRequested
            };
            return Ok(if refresh_order {
                TelemetrySubmission::SurfaceUnavailable
            } else {
                TelemetrySubmission::NotRequested
            });
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let storage_bindings = self.device.limits().max_storage_buffers_per_shader_stage;
        let resident_color_pipeline = self
            .resident_color_pipeline
            .as_ref()
            .ok_or_else(|| resident_pipelines_unavailable(storage_bindings))?;
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: wgpu_label("gsplat-surface-preproject-gpu-order-encoder"),
            });

        let (source_count, order_generation, projection_generation) = {
            let SurfaceGeometry::Packed(packed) = &mut self.geometry else {
                unreachable!("validated preproject geometry")
            };
            let source_count = u32::try_from(packed.resident.capacity)
                .map_err(|_| resident_gpu::ResidentGpuError::AddressSpaceExceeded)?;
            packed.resident.encode_color_resolve_if_needed(
                &self.queue,
                resident_color_pipeline,
                &mut encoder,
                camera,
                self.device.limits().max_compute_workgroups_per_dimension,
            )?;
            let preproject = packed
                .preproject
                .as_ref()
                .expect("validated preproject graph");
            if refresh_order {
                preproject.encode(
                    &self.queue,
                    &mut encoder,
                    &packed.resident,
                    camera,
                    self.surface_config.width,
                    self.surface_config.height,
                );
            } else {
                // Projection and the complete-S count scan are current even
                // when sort cadence deliberately retains the old order.
                preproject.encode_projection_and_count(
                    &self.queue,
                    &mut encoder,
                    &packed.resident,
                    camera,
                    self.surface_config.width,
                    self.surface_config.height,
                );
            }
            packed.preproject_state.record_projection(refresh_order);
            (
                source_count,
                packed.preproject_state.order_generation,
                packed.preproject_state.projection_generation,
            )
        };
        self.instance_count = source_count;
        let mut order_ticket = refresh_order
            .then(|| {
                self.gpu_order_telemetry
                    .begin_sample(camera_revision, false)
            })
            .flatten();

        {
            let SurfaceGeometry::Packed(packed) = &self.geometry else {
                unreachable!("validated preproject geometry")
            };
            let preproject = packed
                .preproject
                .as_ref()
                .expect("validated preproject graph");
            encode_splat_indirect_draw_into(
                &mut encoder,
                &SplatIndirectDraw {
                    pass_label: "gsplat-surface-preproject-gpu-order-draw-pass",
                    view: &view,
                    pipeline: preproject.draw_pipeline(),
                    bind_group: preproject.draw_bind_group(),
                    clear: wgpu::Color::BLACK,
                    indirect_args: preproject.draw_args(),
                },
            );
        }

        let draw_scope = if refresh_order {
            SurfaceGpuProducerDrawScope::ExactCurrentContributors
        } else {
            SurfaceGpuProducerDrawScope::StaleOrderCandidates
        };
        let mut producer_reservation = self
            .gpu_producer_measurement_enabled
            .then(|| {
                self.gpu_producer_telemetry
                    .begin_sample(GpuProducerSampleMetadata {
                        camera_revision,
                        producer: SurfaceGpuOrderProducer::Preproject,
                        order_generation,
                        projection_generation,
                        source_count,
                        order_refreshed: refresh_order,
                        draw_scope,
                    })
            })
            .flatten();
        {
            let SurfaceGeometry::Packed(packed) = &self.geometry else {
                unreachable!("validated preproject geometry")
            };
            let preproject = packed
                .preproject
                .as_ref()
                .expect("validated preproject graph");
            let current_contributor = if refresh_order {
                GpuProducerCountSource::raw(preproject.order_control(), 0)
            } else {
                let (buffer, offset) = preproject.contributor_count_buffer_and_offset();
                GpuProducerCountSource::raw(buffer, offset)
            };
            let drawn = GpuProducerCountSource::indirect_args(preproject.draw_args());
            if let Some(reservation) = producer_reservation.as_ref() {
                debug_assert!(self.gpu_producer_telemetry.encode_count_readback(
                    &mut encoder,
                    reservation,
                    current_contributor,
                    drawn,
                ));
            }
            if let Some(ticket) = order_ticket.as_ref() {
                let (candidate_buffer, candidate_offset) =
                    preproject.candidate_count_buffer_and_offset();
                let current_candidate =
                    InstanceCountSource::raw(candidate_buffer, candidate_offset);
                let current_contributor = if refresh_order {
                    InstanceCountSource::raw(preproject.order_control(), 0)
                } else {
                    let (buffer, offset) = preproject.contributor_count_buffer_and_offset();
                    InstanceCountSource::raw(buffer, offset)
                };
                self.gpu_order_telemetry.encode_instance_count_readback(
                    &mut encoder,
                    ticket,
                    current_candidate,
                    current_contributor,
                    InstanceCountSource::indirect_args(preproject.draw_args()),
                    refresh_order,
                );
            }
        }

        let command_buffer = encoder.finish();
        let submitted_order_ticket = order_ticket.as_ref().map(|ticket| ticket.ticket);
        let submitted_producer_ticket = producer_reservation.take().and_then(|reservation| {
            self.gpu_producer_telemetry
                .arm(&command_buffer, reservation, completion_started)
                .map(|ticket| ticket.ticket)
        });
        if let Some(ticket) = order_ticket.take() {
            self.gpu_order_telemetry
                .arm(&command_buffer, ticket, completion_started);
        }
        self.queue.submit(Some(command_buffer));
        self.present_frame(frame);
        self.last_actual_gpu_order_producer = Some(SurfaceGpuOrderProducer::Preproject);
        self.last_gpu_producer_submission = if !self.gpu_producer_measurement_enabled {
            TelemetrySubmission::NotRequested
        } else if let Some(ticket) = submitted_producer_ticket {
            TelemetrySubmission::Issued(ticket)
        } else {
            TelemetrySubmission::RingBusy
        };
        Ok(match (refresh_order, submitted_order_ticket) {
            (false, _) => TelemetrySubmission::NotRequested,
            (true, Some(ticket)) => TelemetrySubmission::Issued(ticket),
            (true, None) => TelemetrySubmission::RingBusy,
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn render_packed_tiled_gpu_native(
        &mut self,
        camera: &Camera,
        refresh_order: bool,
        camera_revision: u64,
        completion_started: TimerInstant,
    ) -> Result<TelemetrySubmission, SurfacePresenterError> {
        let storage_bindings = self.device.limits().max_storage_buffers_per_shader_stage;
        let color_pipeline = self
            .resident_color_pipeline
            .as_ref()
            .ok_or_else(|| resident_pipelines_unavailable(storage_bindings))?;
        let allow_timestamps = {
            let SurfaceGeometry::Packed(packed) = &self.geometry else {
                unreachable!();
            };
            !packed
                .resident
                .gpu_order()
                .ok_or(SurfacePresenterError::GpuOrderUnsupported)?
                .sorter
                .is_empty()
        };
        let mut telemetry_ticket = refresh_order
            .then(|| {
                self.gpu_order_telemetry
                    .begin_sample(camera_revision, allow_timestamps)
            })
            .flatten();
        let timestamp_range = telemetry_ticket.as_ref().and_then(|ticket| {
            ticket
                .query_set
                .as_ref()
                .map(|query_set| GpuOrderTimestampRange {
                    query_set,
                    keygen_begin_index: 0,
                    keygen_end_index: 1,
                    radix_begin_index: 2,
                    radix_end_index: 3,
                })
        });

        let count_started = timer_now();
        let mut count_encoder =
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: wgpu_label("gsplat-surface-resident-tiled-gpu-count-encoder"),
                });
        let prepass_result = (|| {
            let SurfaceGeometry::Packed(packed) = &mut self.geometry else {
                unreachable!();
            };
            let order = packed
                .resident
                .gpu_order()
                .ok_or(SurfacePresenterError::GpuOrderUnsupported)?;
            if refresh_order {
                order
                    .sorter
                    .encode_with_timestamps(&mut count_encoder, timestamp_range);
            }
            packed.resident.encode_color_resolve_if_needed(
                &self.queue,
                color_pipeline,
                &mut count_encoder,
                camera,
                self.device.limits().max_compute_workgroups_per_dimension,
            )?;
            let order = packed
                .resident
                .gpu_order()
                .ok_or(SurfacePresenterError::GpuOrderUnsupported)?;
            packed
                .tiled
                .as_mut()
                .expect("TiledExact plan owns its diagnostic raster")
                .encode_count_prepass(
                    &self.device,
                    &self.queue,
                    &packed.resident,
                    order.sorter.final_ids(),
                    self.instance_count,
                    &mut count_encoder,
                )?;
            if let Some(ticket) = telemetry_ticket.as_ref() {
                self.gpu_order_telemetry.encode_readback(
                    &mut count_encoder,
                    ticket,
                    order.sorter.indirect_args(),
                );
            }
            Ok::<_, SurfacePresenterError>(())
        })();
        if let Err(error) = prepass_result {
            if let Some(ticket) = telemetry_ticket.take() {
                self.gpu_order_telemetry.cancel(ticket);
            }
            return Err(error);
        }
        self.queue.submit(Some(count_encoder.finish()));
        let count_result = {
            let SurfaceGeometry::Packed(packed) = &mut self.geometry else {
                unreachable!();
            };
            packed
                .tiled
                .as_mut()
                .expect("TiledExact plan owns its diagnostic raster")
                .resolve_count_and_prepare(&self.device, &self.queue)
        };
        if let Err(error) = count_result {
            if let Some(ticket) = telemetry_ticket.take() {
                self.gpu_order_telemetry.cancel(ticket);
            }
            return Err(error.into());
        }
        if let SurfaceGeometry::Packed(packed) = &mut self.geometry {
            packed.last_count_resolve_prepare_ms = timer_elapsed_ms(count_started);
        }

        let Some(frame) = self.acquire_surface_texture()? else {
            if let Some(ticket) = telemetry_ticket.take() {
                self.gpu_order_telemetry.cancel(ticket);
            }
            return Ok(if refresh_order {
                TelemetrySubmission::SurfaceUnavailable
            } else {
                TelemetrySubmission::NotRequested
            });
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: wgpu_label("gsplat-surface-resident-tiled-gpu-finish-encoder"),
            });
        {
            let SurfaceGeometry::Packed(packed) = &mut self.geometry else {
                unreachable!();
            };
            let order = packed
                .resident
                .gpu_order()
                .ok_or(SurfacePresenterError::GpuOrderUnsupported)?;
            packed
                .tiled
                .as_mut()
                .expect("TiledExact plan owns its diagnostic raster")
                .encode_finish(
                    &self.device,
                    &self.queue,
                    &packed.resident,
                    ResidentTiledFinish {
                        source_order_btf: order.sorter.final_ids(),
                        work_count: self.instance_count,
                        target: &view,
                    },
                    &mut encoder,
                )?;
        }
        let command_buffer = encoder.finish();
        let submitted_ticket = telemetry_ticket.as_ref().map(|ticket| ticket.ticket);
        if let Some(ticket) = telemetry_ticket.take() {
            self.gpu_order_telemetry
                .arm(&command_buffer, ticket, completion_started);
        }
        self.queue.submit(Some(command_buffer));
        self.maybe_emit_tiled_phase_trace()?;
        self.present_frame(frame);
        Ok(match (refresh_order, submitted_ticket) {
            (false, _) => TelemetrySubmission::NotRequested,
            (true, Some(ticket)) => TelemetrySubmission::Issued(ticket),
            (true, None) => TelemetrySubmission::RingBusy,
        })
    }

    #[cfg(target_arch = "wasm32")]
    fn render_packed_tiled_gpu_web(
        &mut self,
        camera: &Camera,
        refresh_order: bool,
        camera_revision: u64,
        completion_started: TimerInstant,
    ) -> Result<TelemetrySubmission, SurfacePresenterError> {
        // A CPU-count callback owns the same mapped status buffer. Finish it
        // fail-closed before allowing GPU ordering to replace the source-ID
        // buffer or draw parameters.
        let cpu_pending_ready = {
            let SurfaceGeometry::Packed(packed) = &mut self.geometry else {
                unreachable!();
            };
            if packed.pending.is_some() {
                packed
                    .tiled
                    .as_mut()
                    .expect("TiledExact plan owns its diagnostic raster")
                    .try_resolve_count_and_prepare(&self.device, &self.queue)?
                    .is_some()
            } else {
                true
            }
        };
        if !cpu_pending_ready {
            return Ok(TelemetrySubmission::GpuOrderPreparationPending);
        }
        if let SurfaceGeometry::Packed(packed) = &mut self.geometry {
            packed.pending = None;
        }

        let gpu_count_ready = {
            let SurfaceGeometry::Packed(packed) = &mut self.geometry else {
                unreachable!();
            };
            match packed.pending_gpu.as_mut() {
                None => None,
                Some(pending) if pending.count_ready => Some(true),
                Some(_) => {
                    let ready = packed
                        .tiled
                        .as_mut()
                        .expect("TiledExact plan owns its diagnostic raster")
                        .try_resolve_count_and_prepare(&self.device, &self.queue)?
                        .is_some();
                    if ready && let Some(pending) = packed.pending_gpu.as_mut() {
                        pending.count_ready = true;
                    }
                    Some(ready)
                }
            }
        };
        if gpu_count_ready == Some(false) {
            return Ok(TelemetrySubmission::GpuOrderPreparationPending);
        }

        if gpu_count_ready == Some(true) {
            let mut pending = {
                let SurfaceGeometry::Packed(packed) = &mut self.geometry else {
                    unreachable!();
                };
                packed.pending_gpu.take().expect("ready GPU tiled frame")
            };
            if pending.warmup_only {
                let SurfaceGeometry::Packed(packed) = &mut self.geometry else {
                    unreachable!();
                };
                packed.gpu_order_warmed = true;
            } else if pending.camera != *camera || pending.camera_revision != camera_revision {
                if let Some(ticket) = pending.telemetry_ticket.take() {
                    self.gpu_order_telemetry.fail_encoding(
                        ticket,
                        SurfaceOrderMeasurementFailureReason::GenerationInvalidated,
                    );
                }
            } else {
                let Some(frame) = self.acquire_surface_texture()? else {
                    let SurfaceGeometry::Packed(packed) = &mut self.geometry else {
                        unreachable!();
                    };
                    packed.pending_gpu = Some(pending);
                    return Ok(TelemetrySubmission::NotRequested);
                };
                let view = frame
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                let mut encoder =
                    self.device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: wgpu_label(
                                "gsplat-surface-resident-tiled-web-gpu-finish-encoder",
                            ),
                        });
                {
                    let SurfaceGeometry::Packed(packed) = &mut self.geometry else {
                        unreachable!();
                    };
                    let order = packed
                        .resident
                        .gpu_order()
                        .ok_or(SurfacePresenterError::GpuOrderUnsupported)?;
                    packed
                        .tiled
                        .as_mut()
                        .expect("TiledExact plan owns its diagnostic raster")
                        .encode_finish(
                            &self.device,
                            &self.queue,
                            &packed.resident,
                            ResidentTiledFinish {
                                source_order_btf: order.sorter.final_ids(),
                                work_count: pending.work_count,
                                target: &view,
                            },
                            &mut encoder,
                        )?;
                }
                let command_buffer = encoder.finish();
                let submitted_ticket = pending
                    .telemetry_ticket
                    .as_ref()
                    .map(|ticket| ticket.ticket);
                if let Some(ticket) = pending.telemetry_ticket.take() {
                    self.gpu_order_telemetry.arm(
                        &command_buffer,
                        ticket,
                        pending.completion_started,
                    );
                }
                self.queue.submit(Some(command_buffer));
                self.present_frame(frame);
                return Ok(match (pending.refresh_order, submitted_ticket) {
                    (false, _) => TelemetrySubmission::NotRequested,
                    (true, Some(ticket)) => TelemetrySubmission::Issued(ticket),
                    (true, None) => TelemetrySubmission::RingBusy,
                });
            }
        }

        let storage_bindings = self.device.limits().max_storage_buffers_per_shader_stage;
        let color_pipeline = self
            .resident_color_pipeline
            .as_ref()
            .ok_or_else(|| resident_pipelines_unavailable(storage_bindings))?;
        let allow_timestamps = {
            let SurfaceGeometry::Packed(packed) = &self.geometry else {
                unreachable!();
            };
            !packed
                .resident
                .gpu_order()
                .ok_or(SurfacePresenterError::GpuOrderUnsupported)?
                .sorter
                .is_empty()
        };
        let warmup_only = {
            let SurfaceGeometry::Packed(packed) = &self.geometry else {
                unreachable!();
            };
            refresh_order && !packed.gpu_order_warmed
        };
        let mut telemetry_ticket = (refresh_order && !warmup_only)
            .then(|| {
                self.gpu_order_telemetry
                    .begin_sample(camera_revision, allow_timestamps)
            })
            .flatten();
        let timestamp_range = telemetry_ticket.as_ref().and_then(|ticket| {
            ticket
                .query_set
                .as_ref()
                .map(|query_set| GpuOrderTimestampRange {
                    query_set,
                    keygen_begin_index: 0,
                    keygen_end_index: 1,
                    radix_begin_index: 2,
                    radix_end_index: 3,
                })
        });
        let mut count_encoder =
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: wgpu_label("gsplat-surface-resident-tiled-web-gpu-count-encoder"),
                });
        let prepass_result = (|| {
            let SurfaceGeometry::Packed(packed) = &mut self.geometry else {
                unreachable!();
            };
            let order = packed
                .resident
                .gpu_order()
                .ok_or(SurfacePresenterError::GpuOrderUnsupported)?;
            if refresh_order {
                order
                    .sorter
                    .encode_with_timestamps(&mut count_encoder, timestamp_range);
            }
            packed.resident.encode_color_resolve_if_needed(
                &self.queue,
                color_pipeline,
                &mut count_encoder,
                camera,
                self.device.limits().max_compute_workgroups_per_dimension,
            )?;
            let order = packed
                .resident
                .gpu_order()
                .ok_or(SurfacePresenterError::GpuOrderUnsupported)?;
            packed
                .tiled
                .as_mut()
                .expect("TiledExact plan owns its diagnostic raster")
                .encode_count_prepass(
                    &self.device,
                    &self.queue,
                    &packed.resident,
                    order.sorter.final_ids(),
                    self.instance_count,
                    &mut count_encoder,
                )?;
            if let Some(ticket) = telemetry_ticket.as_ref() {
                self.gpu_order_telemetry.encode_readback(
                    &mut count_encoder,
                    ticket,
                    order.sorter.indirect_args(),
                );
            }
            Ok::<_, SurfacePresenterError>(())
        })();
        if let Err(error) = prepass_result {
            if let Some(ticket) = telemetry_ticket.take() {
                self.gpu_order_telemetry.cancel(ticket);
            }
            return Err(error);
        }
        self.queue.submit(Some(count_encoder.finish()));
        {
            let SurfaceGeometry::Packed(packed) = &mut self.geometry else {
                unreachable!();
            };
            packed.pending_gpu = Some(WebPendingTiledGpuFrame {
                camera: *camera,
                work_count: self.instance_count,
                camera_revision,
                completion_started,
                count_ready: false,
                refresh_order,
                warmup_only,
                telemetry_ticket,
            });
            let ready = packed
                .tiled
                .as_mut()
                .expect("TiledExact plan owns its diagnostic raster")
                .try_resolve_count_and_prepare(&self.device, &self.queue)?
                .is_some();
            if ready && let Some(pending) = packed.pending_gpu.as_mut() {
                pending.count_ready = true;
            }
        }
        Ok(TelemetrySubmission::GpuOrderPreparationPending)
    }

    pub(crate) fn poll_gpu_order_telemetry(&mut self) -> GpuOrderTelemetryPoll {
        self.gpu_order_telemetry.poll(&self.device)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn maybe_emit_tiled_phase_trace(&mut self) -> Result<(), SurfacePresenterError> {
        let Some(requested) = std::env::var_os("GSPLAT_TILED_PHASE_TRACE") else {
            return Ok(());
        };
        let trace_limit = requested
            .to_string_lossy()
            .parse::<u32>()
            .unwrap_or(1)
            .max(1);
        let SurfaceGeometry::Packed(packed) = &mut self.geometry else {
            return Ok(());
        };
        if packed.phase_trace_emitted >= trace_limit {
            return Ok(());
        }
        let timings = packed
            .tiled
            .as_ref()
            .expect("TiledExact plan owns its diagnostic raster")
            .read_phase_timings_blocking(&self.device, &self.queue)?;
        eprintln!(
            "gsplat_tiled_phase sample={} plan={:?} source_count={} entry_count={} entry_capacity={} count_resolve_prepare_ms={:.3} gpu={:?}",
            packed.phase_trace_emitted,
            packed.raster_plan,
            packed.resident.capacity,
            packed
                .tiled
                .as_ref()
                .expect("TiledExact plan owns its diagnostic raster")
                .active_entry_count(),
            packed
                .tiled
                .as_ref()
                .expect("TiledExact plan owns its diagnostic raster")
                .entry_capacity(),
            packed.last_count_resolve_prepare_ms,
            timings,
        );
        packed.phase_trace_emitted += 1;
        Ok(())
    }

    pub(crate) fn poll_cpu_order_completion_telemetry(&mut self) -> CpuOrderTelemetryPoll {
        let _ = self.device.poll(wgpu::PollType::Poll);
        self.cpu_order_completion_telemetry.poll()
    }

    pub(crate) fn poll_projected_draw_telemetry(&mut self) -> ProjectedDrawTelemetryPoll {
        self.projected_draw_telemetry.poll(&self.device)
    }

    pub(crate) fn poll_gpu_producer_telemetry(&mut self) -> GpuProducerTelemetryPoll {
        self.gpu_producer_telemetry.poll(&self.device)
    }

    pub(crate) fn take_projected_draw_submission(&mut self) -> TelemetrySubmission {
        std::mem::replace(
            &mut self.last_projected_draw_submission,
            TelemetrySubmission::NotRequested,
        )
    }

    pub(crate) fn take_gpu_producer_submission(&mut self) -> TelemetrySubmission {
        std::mem::replace(
            &mut self.last_gpu_producer_submission,
            TelemetrySubmission::NotRequested,
        )
    }

    pub(crate) const fn take_actual_gpu_order_producer(
        &mut self,
    ) -> Option<SurfaceGpuOrderProducer> {
        let producer = self.last_actual_gpu_order_producer;
        self.last_actual_gpu_order_producer = None;
        producer
    }

    pub(crate) fn gpu_order_timestamps_enabled(&self) -> bool {
        self.gpu_order_telemetry.timestamps_enabled()
    }

    fn present_geometry(&mut self, camera: &Camera) -> Result<(), SurfacePresenterError> {
        self.present_geometry_tracked(camera, None).map(|_| ())
    }

    fn present_geometry_tracked(
        &mut self,
        camera: &Camera,
        completion: Option<CpuCompletionSampleRequest>,
    ) -> Result<TelemetrySubmission, SurfacePresenterError> {
        let projected_sample_request = self.projected_draw_sample_request.take();
        self.last_projected_draw_submission = TelemetrySubmission::NotRequested;
        let Some(frame) = self.acquire_surface_texture()? else {
            if projected_sample_request.is_some() {
                self.last_projected_draw_submission = TelemetrySubmission::SurfaceUnavailable;
            }
            return Ok(if completion.is_some() {
                TelemetrySubmission::SurfaceUnavailable
            } else {
                TelemetrySubmission::NotRequested
            });
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: wgpu_label("gsplat-surface-encoder"),
            });
        let storage_bindings = self.device.limits().max_storage_buffers_per_shader_stage;
        let resident_color_pipeline = self.resident_color_pipeline.as_ref();
        let resident_draw_pipeline = self.resident_draw_pipeline.as_ref();
        let projected_order_generation = match &self.geometry {
            SurfaceGeometry::Packed(packed) => packed.projected_cache.order_generation(),
            SurfaceGeometry::Direct(_) | SurfaceGeometry::Paged(_) => 0,
        };
        let projected_draw_execution = self.resolved_projected_draw_execution();
        let projected_cache_key = ProjectedCacheKey {
            order_source: ProjectedOrderSource::Cpu,
            order_generation: projected_order_generation,
            camera: *camera,
            width: self.surface_config.width,
            height: self.surface_config.height,
            draw_count_guard: self.instance_count,
            draw_execution: projected_draw_execution,
            probe_generation: self.projected_probe_generation,
        };
        let mut projection_rebuilt = false;
        if let SurfaceGeometry::Packed(packed) = &mut self.geometry {
            packed.resident.encode_color_resolve_if_needed(
                &self.queue,
                resident_color_pipeline
                    .ok_or_else(|| resident_pipelines_unavailable(storage_bindings))?,
                &mut encoder,
                camera,
                self.device.limits().max_compute_workgroups_per_dimension,
            )?;
            if packed.raster_plan == SurfaceRasterExecutionPlan::ProjectedQuadsExact
                && packed.projected_cache.needs_projection(projected_cache_key)
            {
                packed.projected.encode_cpu_projection_for_draw(
                    &self.queue,
                    &mut encoder,
                    self.instance_count,
                    projected_draw_execution,
                )?;
                packed.projected_cache.publish(projected_cache_key);
                projection_rebuilt = true;
            }
        }
        match &self.geometry {
            SurfaceGeometry::Direct(direct) => encode_splat_draw_into(
                &mut encoder,
                &SplatDraw {
                    pass_label: "gsplat-surface-direct-pass",
                    view: &view,
                    pipeline: &self.direct_pipeline,
                    bind_group: &direct.cpu_bind_group,
                    clear: wgpu::Color::BLACK,
                    vertex_count: resident_gpu::RESIDENT_QUAD_VERTEX_COUNT,
                    instance_count: self.instance_count,
                },
            ),
            SurfaceGeometry::Packed(packed)
                if packed.raster_plan == SurfaceRasterExecutionPlan::ProjectedQuadsExact
                    && projected_draw_execution == ProjectedDrawExecution::Compact =>
            {
                encode_splat_indirect_draw_into(
                    &mut encoder,
                    &SplatIndirectDraw {
                        pass_label: "gsplat-surface-projected-contributors-pass",
                        view: &view,
                        pipeline: packed
                            .projected
                            .contributor_draw_pipeline()
                            .expect("available compaction owns its draw pipeline"),
                        bind_group: packed
                            .projected
                            .contributor_draw_bind_group()
                            .expect("available compaction owns its draw binding"),
                        clear: wgpu::Color::BLACK,
                        indirect_args: packed
                            .projected
                            .contributor_indirect_args()
                            .expect("available compaction owns its draw args"),
                    },
                );
            }
            SurfaceGeometry::Packed(packed)
                if packed.raster_plan == SurfaceRasterExecutionPlan::ProjectedQuadsExact =>
            {
                // Downlevel adapters keep the exact direct V-instance path;
                // invalid projected ranks remain guarded by zero alpha.
                encode_splat_draw_into(
                    &mut encoder,
                    &SplatDraw {
                        pass_label: "gsplat-surface-projected-quads-pass",
                        view: &view,
                        pipeline: packed.projected.draw_pipeline(),
                        bind_group: packed.projected.draw_bind_group(),
                        clear: wgpu::Color::BLACK,
                        vertex_count: resident_gpu::RESIDENT_QUAD_VERTEX_COUNT,
                        instance_count: self.instance_count,
                    },
                );
            }
            SurfaceGeometry::Packed(packed) => encode_splat_draw_into(
                &mut encoder,
                &SplatDraw {
                    pass_label: "gsplat-surface-resident-pass",
                    view: &view,
                    pipeline: resident_draw_pipeline
                        .ok_or_else(|| resident_pipelines_unavailable(storage_bindings))?,
                    bind_group: &packed.resident.draw_bind_group,
                    clear: wgpu::Color::BLACK,
                    vertex_count: resident_gpu::RESIDENT_QUAD_VERTEX_COUNT,
                    instance_count: self.instance_count,
                },
            ),
            SurfaceGeometry::Paged(paged) => encode_splat_draw_into(
                &mut encoder,
                &SplatDraw {
                    pass_label: "gsplat-surface-paged-pass",
                    view: &view,
                    pipeline: &self.packed_pipeline,
                    bind_group: &paged.active_set.atlas.resources.bind_group,
                    clear: wgpu::Color::BLACK,
                    vertex_count: packed_gpu::PACKED_QUAD_VERTEX_COUNT,
                    instance_count: self.instance_count,
                },
            ),
        }
        let projected_metadata =
            projected_sample_request.and_then(|request| match &self.geometry {
                SurfaceGeometry::Packed(packed)
                    if packed.raster_plan == SurfaceRasterExecutionPlan::ProjectedQuadsExact =>
                {
                    Some(ProjectedDrawSampleMetadata {
                        camera_revision: request.camera_revision,
                        execution: projected_draw_execution,
                        order_backend: request.order_backend,
                        projection_generation: packed.projected_cache.projection_generation(),
                        probe_generation: self.projected_probe_generation,
                        projection_rebuilt,
                        order_refreshed: request.order_refreshed,
                    })
                }
                SurfaceGeometry::Direct(_)
                | SurfaceGeometry::Packed(_)
                | SurfaceGeometry::Paged(_) => None,
            });
        let mut projected_reservation = projected_metadata
            .and_then(|metadata| self.projected_draw_telemetry.begin_sample(metadata));
        if let Some(reservation) = projected_reservation.as_ref()
            && let SurfaceGeometry::Packed(packed) = &self.geometry
        {
            let candidate =
                ProjectedDrawCountSource::indirect_args(packed.projected.cpu_candidate_args());
            let (contributor_buffer, contributor_offset) =
                packed.projected.contributor_count_buffer_and_offset();
            let contributor = ProjectedDrawCountSource::raw(contributor_buffer, contributor_offset);
            let drawn = packed
                .projected
                .contributor_indirect_args()
                .filter(|_| projected_draw_execution == ProjectedDrawExecution::Compact)
                .map(ProjectedDrawCountSource::indirect_args)
                .unwrap_or(candidate);
            debug_assert!(self.projected_draw_telemetry.encode_count_readback(
                &mut encoder,
                reservation,
                candidate,
                contributor,
                drawn,
            ));
        }
        let mut completion_ticket = completion.and_then(|request| {
            self.cpu_order_completion_telemetry
                .begin_sample_with_counts(
                    request.camera_revision,
                    request.preprocess_ms,
                    request.sort_ms,
                    FrameInstanceCounts {
                        candidate_visible: self.instance_count,
                        contributor: self.instance_count,
                        drawn: self.instance_count,
                        exact_contributor_compaction: false,
                    },
                )
        });
        if let Some(ticket) = completion_ticket.as_ref()
            && let SurfaceGeometry::Packed(packed) = &self.geometry
            && packed.raster_plan == SurfaceRasterExecutionPlan::ProjectedQuadsExact
        {
            let candidate =
                InstanceCountSource::indirect_args(packed.projected.cpu_candidate_args());
            let (contributor_buffer, contributor_offset) =
                packed.projected.contributor_count_buffer_and_offset();
            let contributor = InstanceCountSource::raw(contributor_buffer, contributor_offset);
            let drawn = packed
                .projected
                .contributor_indirect_args()
                .filter(|_| projected_draw_execution == ProjectedDrawExecution::Compact)
                .map(InstanceCountSource::indirect_args)
                .unwrap_or(candidate);
            self.cpu_order_completion_telemetry.encode_count_readback(
                &mut encoder,
                ticket,
                candidate,
                contributor,
                drawn,
                projected_draw_execution == ProjectedDrawExecution::Compact,
            );
        }
        let command_buffer = encoder.finish();
        let submitted_ticket = completion_ticket.as_ref().map(|ticket| ticket.ticket);
        let projected_started = projected_sample_request.map(|request| request.started);
        let projected_submitted_ticket = match (projected_reservation.take(), projected_started) {
            (Some(reservation), Some(started)) => self
                .projected_draw_telemetry
                .arm(&command_buffer, reservation, started)
                .map(|ticket| ticket.ticket),
            (None, _) | (_, None) => None,
        };
        if let (Some(ticket), Some(request)) = (completion_ticket.take(), completion) {
            self.cpu_order_completion_telemetry
                .arm(&command_buffer, ticket, request.started);
        }
        self.queue.submit(Some(command_buffer));
        self.present_frame(frame);
        self.last_projected_draw_submission = match (projected_metadata, projected_submitted_ticket)
        {
            (None, _) => TelemetrySubmission::NotRequested,
            (Some(_), Some(ticket)) => TelemetrySubmission::Issued(ticket),
            (Some(_), None) => TelemetrySubmission::RingBusy,
        };
        Ok(match (completion, submitted_ticket) {
            (None, _) => TelemetrySubmission::NotRequested,
            (Some(_), Some(ticket)) => TelemetrySubmission::Issued(ticket),
            (Some(_), None) => TelemetrySubmission::RingBusy,
        })
    }

    fn acquire_surface_texture(
        &mut self,
    ) -> Result<Option<wgpu::SurfaceTexture>, SurfacePresenterError> {
        if !self.surface_configuration_valid {
            return Err(SurfacePresenterError::SurfaceConfigure(
                "surface is fail-closed after a resize rollback failure".into(),
            ));
        }
        match self.surface.get_current_texture() {
            Ok(frame) => Ok(Some(frame)),
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.surface_config);
                match self.surface.get_current_texture() {
                    Ok(frame) => Ok(Some(frame)),
                    Err(wgpu::SurfaceError::Timeout) => Ok(None),
                    Err(err) => Err(surface_error_to_presenter(err)),
                }
            }
            Err(wgpu::SurfaceError::Timeout) => Ok(None),
            Err(err) => Err(surface_error_to_presenter(err)),
        }
    }

    fn present_frame(&mut self, frame: wgpu::SurfaceTexture) {
        self.last_presented_size = Some((frame.texture.width(), frame.texture.height()));
        frame.present();
        self.last_frame_presented = true;
    }

    pub const fn instance_count(&self) -> u32 {
        self.instance_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preproject_first_frame_and_invalidation_require_a_refresh() {
        let mut state = PreprojectProducerState::default();
        assert!(matches!(
            state.require_order(false),
            Err(SurfacePresenterError::PreprojectOrderUnavailable)
        ));
        assert!(state.require_order(true).is_ok());
        state.record_projection(true);
        assert!(state.order_valid);
        assert_eq!(state.order_generation, 1);
        assert_eq!(state.projection_generation, 1);

        // A current-camera non-refresh projection advances only the projected
        // geometry generation and deliberately retains the order identity.
        state.require_order(false).expect("refreshed prefix");
        state.record_projection(false);
        assert_eq!(state.order_generation, 1);
        assert_eq!(state.projection_generation, 2);

        // Scene replacement, resize, backend transitions, and producer
        // transitions invalidate through this fail-closed primitive.
        state.invalidate_order();
        assert!(!state.order_valid);
        assert_eq!(state.order_generation, 2);
        assert!(matches!(
            state.require_order(false),
            Err(SurfacePresenterError::PreprojectOrderUnavailable)
        ));
    }

    #[test]
    fn preproject_web_scope_failure_cannot_publish_a_partial_graph() {
        let mut published = false;
        let result = try_prepare_then_commit(
            &mut published,
            |_| {
                classify_surface_gpu_order_scope_errors(
                    GeometryPath::PackedAtlas,
                    None,
                    Some("synthetic preproject OOM".into()),
                    None,
                )
                .map_or(Ok(PreparedSurfaceGpuProducer::AlreadyPrepared), Err)
            },
            |published, _| *published = true,
        );
        assert!(matches!(
            result,
            Err(SurfacePresenterError::ResidentGpu(
                resident_gpu::ResidentGpuError::GpuOrderOutOfMemory(_)
            ))
        ));
        assert!(!published);
    }

    #[test]
    fn projected_draw_modes_remain_independent_from_order_backend() {
        assert_ne!(
            ProjectedDrawExecution::Candidate,
            ProjectedDrawExecution::Compact,
        );
    }

    #[test]
    fn optional_compaction_failure_is_discarded_without_rejecting_candidate() {
        assert_eq!(
            resolve_projected_compaction_candidate(Ok(Some(7_u32)), None, None, None, false)
                .expect("optional success"),
            Some(7)
        );
        for accepted in [
            resolve_projected_compaction_candidate::<u32>(
                Ok(Some(7_u32)),
                Some("internal".into()),
                None,
                None,
                false,
            ),
            resolve_projected_compaction_candidate::<u32>(
                Ok(Some(7_u32)),
                None,
                Some("oom".into()),
                None,
                false,
            ),
            resolve_projected_compaction_candidate::<u32>(
                Ok(Some(7_u32)),
                None,
                None,
                Some("validation".into()),
                false,
            ),
            resolve_projected_compaction_candidate::<u32>(
                Err(resident_gpu::ResidentGpuError::GpuOrderInitialization(
                    "typed construction failure".into(),
                )),
                None,
                None,
                None,
                false,
            ),
        ] {
            assert_eq!(accepted.expect("best-effort admission"), None);
        }
    }

    #[test]
    fn forced_compact_makes_the_optional_graph_transactional() {
        assert!(matches!(
            resolve_projected_compaction_candidate::<u32>(Ok(None), None, None, None, true),
            Err(SurfacePresenterError::ProjectedCompactionUnsupported),
        ));
        assert!(matches!(
            resolve_projected_compaction_candidate::<u32>(
                Ok(Some(7)),
                None,
                Some("oom".into()),
                None,
                true,
            ),
            Err(SurfacePresenterError::SurfaceGeometryOutOfMemory {
                path: GeometryPath::PackedAtlas,
                ..
            }),
        ));
    }

    #[test]
    fn unavailable_gpu_surface_never_issues_a_stale_count_ticket() {
        let ordinary_refresh = gpu_order_preparation_plan(false, true, true, true);
        assert_eq!(
            unavailable_gpu_surface_submission(ordinary_refresh, false),
            Some(TelemetrySubmission::SurfaceUnavailable),
        );

        let cached_frame = gpu_order_preparation_plan(false, true, false, true);
        assert_eq!(
            unavailable_gpu_surface_submission(cached_frame, false),
            Some(TelemetrySubmission::NotRequested),
        );

        let web_warmup = gpu_order_preparation_plan(true, true, true, false);
        assert_eq!(unavailable_gpu_surface_submission(web_warmup, false), None,);
        assert_eq!(
            unavailable_gpu_surface_submission(ordinary_refresh, true),
            None,
        );
    }

    #[test]
    fn first_web_packed_gpu_order_is_hidden_and_unmeasured() {
        let plan = gpu_order_preparation_plan(true, true, true, false);
        assert_eq!(
            plan,
            GpuOrderPreparationPlan {
                pending: true,
                acquire_surface: false,
                reserve_measurement: false,
            }
        );
    }

    #[test]
    fn same_camera_retry_is_presented_with_exactly_one_measurement_reservation() {
        let plan = gpu_order_preparation_plan(true, true, true, true);
        assert_eq!(
            plan,
            GpuOrderPreparationPlan {
                pending: false,
                acquire_surface: true,
                reserve_measurement: true,
            }
        );
    }

    #[test]
    fn later_web_camera_refresh_does_not_repeat_one_time_preparation() {
        let plan = gpu_order_preparation_plan(true, true, true, true);
        assert!(!plan.pending);
        assert!(plan.acquire_surface);
        assert!(plan.reserve_measurement);
    }

    #[test]
    fn native_and_direct_gpu_order_do_not_hide_the_first_frame() {
        for plan in [
            gpu_order_preparation_plan(false, true, true, false),
            gpu_order_preparation_plan(true, false, true, false),
        ] {
            assert!(!plan.pending);
            assert!(plan.acquire_surface);
            assert!(plan.reserve_measurement);
        }
    }

    #[test]
    fn cached_gpu_order_is_presented_without_a_new_measurement() {
        let plan = gpu_order_preparation_plan(true, true, false, true);
        assert_eq!(
            plan,
            GpuOrderPreparationPlan {
                pending: false,
                acquire_surface: true,
                reserve_measurement: false,
            }
        );
    }

    #[test]
    fn projected_cache_reuses_only_the_same_camera_viewport_order_and_count() {
        let base = ProjectedCacheKey {
            order_source: ProjectedOrderSource::Cpu,
            order_generation: 0,
            camera: Camera::default(),
            width: 1_920,
            height: 1_080,
            draw_count_guard: 1_886_298,
            draw_execution: ProjectedDrawExecution::Candidate,
            probe_generation: 0,
        };
        let mut cache = ProjectedCacheState::default();
        assert!(cache.needs_projection(base));
        cache.publish(base);
        assert!(!cache.needs_projection(base));

        let mut changed_camera = base;
        changed_camera.camera.pose.position.x = 0.25;
        assert!(cache.needs_projection(changed_camera));
        assert!(cache.needs_projection(ProjectedCacheKey {
            width: 1_280,
            ..base
        }));
        assert!(cache.needs_projection(ProjectedCacheKey {
            order_source: ProjectedOrderSource::Gpu,
            ..base
        }));
        assert!(cache.needs_projection(ProjectedCacheKey {
            draw_count_guard: base.draw_count_guard - 1,
            ..base
        }));
        assert!(cache.needs_projection(ProjectedCacheKey {
            draw_execution: ProjectedDrawExecution::Compact,
            ..base
        }));
        assert!(cache.needs_projection(ProjectedCacheKey {
            probe_generation: 1,
            ..base
        }));

        cache.invalidate_order_if(false);
        assert!(!cache.needs_projection(base));
        cache.invalidate_order_if(true);
        assert!(cache.needs_projection(base));
        assert_eq!(cache.order_generation(), 1);
        let next_generation = ProjectedCacheKey {
            order_generation: cache.order_generation(),
            ..base
        };
        cache.publish(next_generation);
        assert!(!cache.needs_projection(next_generation));
        assert!(cache.needs_projection(base));
    }

    #[test]
    fn four_binding_direct_devices_skip_eager_resident_pipeline_creation() {
        let mut direct_limits = wgpu::Limits::downlevel_defaults();
        direct_limits.max_storage_buffers_per_shader_stage = 4;
        assert!(!supports_resident_pipeline_layout(&direct_limits));

        direct_limits.max_storage_buffers_per_shader_stage = 7;
        assert!(!supports_resident_pipeline_layout(&direct_limits));

        direct_limits.max_storage_buffers_per_shader_stage = 8;
        assert!(supports_resident_pipeline_layout(&direct_limits));
    }

    #[test]
    fn gpu_order_requires_indirect_execution_before_resource_creation() {
        let supported = wgpu::DownlevelCapabilities::default();
        assert!(supports_direct_gpu_order(&supported));

        let mut unsupported = supported;
        unsupported
            .flags
            .remove(wgpu::DownlevelFlags::INDIRECT_EXECUTION);
        assert!(!supports_direct_gpu_order(&unsupported));
    }

    #[test]
    fn packed_gpu_order_scope_errors_are_structured_and_prioritize_oom() {
        let error = classify_surface_gpu_order_scope_errors(
            GeometryPath::PackedAtlas,
            Some("internal".into()),
            Some("oom".into()),
            Some("validation".into()),
        )
        .expect("scope error");
        assert!(matches!(
            error,
            SurfacePresenterError::ResidentGpu(
                resident_gpu::ResidentGpuError::GpuOrderOutOfMemory(ref message)
            ) if message == "oom"
        ));

        let error = classify_surface_gpu_order_scope_errors(
            GeometryPath::PackedAtlas,
            None,
            None,
            Some("validation".into()),
        )
        .expect("scope error");
        assert!(matches!(
            error,
            SurfacePresenterError::ResidentGpu(
                resident_gpu::ResidentGpuError::GpuOrderValidation(ref message)
            ) if message == "validation"
        ));
    }

    #[test]
    fn gpu_order_scope_success_has_no_error_and_paged_is_unsupported() {
        assert!(
            classify_surface_gpu_order_scope_errors(GeometryPath::PackedAtlas, None, None, None,)
                .is_none()
        );
        assert!(matches!(
            classify_surface_gpu_order_scope_errors(
                GeometryPath::PagedActiveAtlas,
                None,
                None,
                None,
            ),
            Some(SurfacePresenterError::GpuOrderUnsupported)
        ));
    }

    #[test]
    fn geometry_scope_errors_prioritize_oom_and_preserve_target_path() {
        let error = classify_surface_geometry_scope_errors(
            GeometryPath::PackedAtlas,
            Some("internal".into()),
            Some("oom".into()),
            Some("validation".into()),
        )
        .expect("scope error");
        assert!(matches!(
            error,
            SurfacePresenterError::SurfaceGeometryOutOfMemory {
                path: GeometryPath::PackedAtlas,
                ref message,
            } if message == "oom"
        ));

        let error = classify_surface_geometry_scope_errors(
            GeometryPath::SortedIndexDirect,
            None,
            None,
            Some("bad binding".into()),
        )
        .expect("scope error");
        assert!(matches!(
            error,
            SurfacePresenterError::SurfaceGeometryValidation {
                path: GeometryPath::SortedIndexDirect,
                ref message,
            } if message == "bad binding"
        ));
        assert!(
            classify_surface_geometry_scope_errors(GeometryPath::PackedAtlas, None, None, None,)
                .is_none()
        );
    }

    #[test]
    fn surface_resize_scope_errors_prioritize_oom_and_keep_validation_structured() {
        assert!(matches!(
            classify_surface_configure_scope_errors(
                Some("internal".into()),
                Some("oom".into()),
                Some("validation".into()),
            ),
            Some(SurfacePresenterError::SurfaceOutOfMemory)
        ));
        assert!(matches!(
            classify_surface_configure_scope_errors(
                None,
                None,
                Some("invalid resize".into()),
            ),
            Some(SurfacePresenterError::SurfaceConfigure(message))
                if message == "validation: invalid resize"
        ));
        assert!(
            classify_surface_configure_scope_errors(None, None, None).is_none(),
            "successful configure must not fabricate a resize failure"
        );
    }

    fn adapter_limits(storage_bytes: u32, buffer_bytes: u64) -> wgpu::Limits {
        let mut adapter_limits = wgpu::Limits::downlevel_defaults();
        adapter_limits.max_storage_buffer_binding_size = storage_bytes;
        adapter_limits.max_buffer_size = buffer_bytes;
        adapter_limits.max_storage_buffers_per_shader_stage = 8;
        adapter_limits
    }

    fn resource_plan(
        path: GeometryPath,
        scene_splats: usize,
        sh_degree: u8,
        page_count: usize,
        page_capacity: usize,
        limits: &wgpu::Limits,
    ) -> SurfaceResourcePlan {
        surface_resource_plan(
            path,
            scene_splats,
            sh_degree,
            page_count,
            page_capacity,
            716,
            1_600,
            limits,
        )
        .expect("resource plan")
    }

    #[test]
    fn small_direct_scene_keeps_portable_limits_on_larger_adapter() {
        let mut adapter = adapter_limits(256 << 20, 512 << 20);
        adapter.max_texture_dimension_2d = 16_384;
        let plan = resource_plan(GeometryPath::SortedIndexDirect, 279_199, 3, 0, 0, &adapter);

        let requested = surface_required_device_limits(&adapter, &plan).expect("limits");

        assert_eq!(requested.max_texture_dimension_2d, 16_384);
        assert_eq!(
            requested.max_storage_buffer_binding_size,
            wgpu::Limits::downlevel_defaults().max_storage_buffer_binding_size
        );
        assert_eq!(
            requested.max_buffer_size,
            wgpu::Limits::downlevel_defaults().max_buffer_size
        );
        assert_eq!(
            requested.max_storage_buffers_per_shader_stage,
            resident_gpu::RESIDENT_COLOR_STORAGE_BINDINGS,
            "a capable Direct device must retain runtime Packed headroom"
        );
    }

    #[test]
    fn direct_device_without_resident_bindings_still_constructs_direct_limits() {
        let mut adapter = adapter_limits(256 << 20, 512 << 20);
        adapter.max_storage_buffers_per_shader_stage =
            resident_gpu::RESIDENT_COLOR_STORAGE_BINDINGS - 1;
        let plan = resource_plan(GeometryPath::SortedIndexDirect, 279_199, 3, 0, 0, &adapter);

        let requested = surface_required_device_limits(&adapter, &plan).expect("direct limits");

        assert_eq!(
            requested.max_storage_buffers_per_shader_stage,
            wgpu::Limits::downlevel_defaults().max_storage_buffers_per_shader_stage
        );
    }

    #[test]
    fn larger_adapter_requests_exact_750k_degree_three_binding() {
        let adapter = adapter_limits(256 << 20, 512 << 20);
        let plan = resource_plan(GeometryPath::SortedIndexDirect, 750_000, 3, 0, 0, &adapter);

        let requested = surface_required_device_limits(&adapter, &plan).expect("limits");

        assert_eq!(requested.max_storage_buffer_binding_size, 135_000_000);
        assert_eq!(requested.max_buffer_size, 256 << 20);
        assert_eq!(
            crate::direct_scene_preflight(750_000, 3, &requested)
                .expect("preflight")
                .path,
            crate::DirectScenePath::Direct
        );
    }

    #[test]
    fn surface_larger_than_adapter_texture_limit_is_rejected() {
        let adapter = adapter_limits(256 << 20, 512 << 20);
        let plan = surface_resource_plan(
            GeometryPath::SortedIndexDirect,
            279_199,
            3,
            0,
            0,
            adapter.max_texture_dimension_2d + 1,
            1_600,
            &adapter,
        )
        .expect("resource plan");

        let error = surface_required_device_limits(&adapter, &plan).unwrap_err();

        assert!(matches!(error, SurfacePresenterError::DeviceCreation(_)));
        assert!(error.to_string().contains("required texture dimension"));
    }

    #[test]
    fn physical_128_mib_adapter_rejects_750k_degree_three_scene() {
        let adapter = wgpu::Limits::downlevel_defaults();
        let plan = resource_plan(GeometryPath::SortedIndexDirect, 750_000, 3, 0, 0, &adapter);

        let error = surface_required_device_limits(&adapter, &plan).unwrap_err();
        let SurfacePresenterError::DirectScene(DirectSceneError::ResourceLimitExceeded(report)) =
            error
        else {
            panic!("unexpected error: {error:?}");
        };
        assert_eq!(report.effective_storage_binding_limit, 128 << 20);
        assert_eq!(report.max_direct_splats, 745_654);
        assert_eq!(report.requirements[2].required_bytes, 135_000_000);
    }

    #[test]
    fn direct_scene_above_256_mib_raises_binding_and_buffer_exactly() {
        let adapter = adapter_limits(512 << 20, 512 << 20);
        let plan = resource_plan(
            GeometryPath::SortedIndexDirect,
            1_500_000,
            3,
            0,
            0,
            &adapter,
        );

        let requested = surface_required_device_limits(&adapter, &plan).expect("limits");

        assert_eq!(requested.max_storage_buffer_binding_size, 270_000_000);
        assert_eq!(requested.max_buffer_size, 270_000_000);
    }

    #[test]
    fn packed_selection_does_not_request_oversized_direct_binding() {
        let adapter = adapter_limits(512 << 20, 512 << 20);
        let plan = resource_plan(GeometryPath::PackedAtlas, 3_000_000, 3, 0, 0, &adapter);

        let requested = surface_required_device_limits(&adapter, &plan).expect("limits");

        assert_eq!(
            plan.direct_preflight.path,
            DirectScenePath::ActiveAtlasRequired
        );
        assert_eq!(plan.packed_preflight.path, PackedScenePath::PackedAtlas);
        assert_eq!(
            plan.packed_preflight.resident_gpu.position_alpha,
            48_000_000
        );
        assert_eq!(plan.packed_preflight.resident_gpu.sh_plane_count, 4);
        assert_eq!(
            plan.packed_preflight.largest_storage_binding_bytes,
            48_000_000
        );
        assert_eq!(
            requested.max_storage_buffer_binding_size,
            wgpu::Limits::downlevel_defaults().max_storage_buffer_binding_size
        );
        assert_eq!(
            requested.max_buffer_size,
            wgpu::Limits::downlevel_defaults().max_buffer_size
        );
    }

    #[test]
    fn paged_selection_requests_only_fixed_resident_capacity() {
        let adapter = wgpu::Limits::downlevel_defaults();
        let page_capacity = 65_536;
        let scene_splats = 10_000_000;
        let plan = resource_plan(
            GeometryPath::PagedActiveAtlas,
            scene_splats,
            3,
            scene_splats.div_ceil(page_capacity),
            page_capacity,
            &adapter,
        );

        let requested = surface_required_device_limits(&adapter, &plan).expect("limits");

        assert_eq!(plan.paged_plan.hot_record_storage_bytes, 4 * 65_536 * 20);
        assert_eq!(plan.packed_preflight.path, PackedScenePath::PagingRequired);
        assert_eq!(
            requested.max_storage_buffer_binding_size,
            wgpu::Limits::downlevel_defaults().max_storage_buffer_binding_size
        );
        assert_eq!(
            requested.max_buffer_size,
            wgpu::Limits::downlevel_defaults().max_buffer_size
        );
    }

    #[test]
    fn packed_surface_accepts_exact_portable_binding_boundary() {
        let adapter = adapter_limits(128 << 20, 256 << 20);
        let plan = resource_plan(GeometryPath::PackedAtlas, 8_388_608, 3, 0, 0, &adapter);

        let requested = surface_required_device_limits(&adapter, &plan).expect("limits");

        assert_eq!(plan.packed_preflight.path, PackedScenePath::PackedAtlas);
        assert_eq!(
            plan.resident_plan.largest_storage_binding_bytes(),
            128 << 20
        );
        assert_eq!(requested.max_storage_buffer_binding_size, 128 << 20);
        assert_eq!(requested.max_buffer_size, 256 << 20);
        assert_eq!(requested.max_storage_buffers_per_shader_stage, 8);
    }

    #[test]
    fn packed_surface_rejects_one_splat_above_portable_binding_boundary() {
        let adapter = adapter_limits(128 << 20, 256 << 20);
        let plan = resource_plan(GeometryPath::PackedAtlas, 8_388_609, 3, 0, 0, &adapter);

        let error = surface_required_device_limits(&adapter, &plan).unwrap_err();
        let SurfacePresenterError::DirectScene(DirectSceneError::PackedResourceLimitExceeded(
            report,
        )) = error
        else {
            panic!("unexpected error: {error:?}");
        };
        assert_eq!(report.largest_storage_binding_bytes, (128 << 20) + 16);
        assert_eq!(
            report.failure,
            Some(crate::PackedScenePreflightFailure::StorageBindingSize {
                required_bytes: (128 << 20) + 16,
                limit_bytes: 128 << 20,
            })
        );
    }

    #[test]
    fn packed_surface_rejects_fewer_than_eight_storage_bindings() {
        let mut adapter = adapter_limits(128 << 20, 256 << 20);
        adapter.max_storage_buffers_per_shader_stage = 7;
        let plan = resource_plan(GeometryPath::PackedAtlas, 1, 3, 0, 0, &adapter);

        let error = surface_required_device_limits(&adapter, &plan).unwrap_err();
        let SurfacePresenterError::DirectScene(DirectSceneError::PackedResourceLimitExceeded(
            report,
        )) = error
        else {
            panic!("unexpected error: {error:?}");
        };
        assert_eq!(
            report.failure,
            Some(crate::PackedScenePreflightFailure::StorageBindingCount {
                required: 8,
                available: 7,
            })
        );
    }
}
