use gsplat_core::{Camera, Vec3f};
use thiserror::Error;

use crate::cpu_order::CpuOrderEngine;
use crate::renderer::gpu_prepare::{
    CpuPostProjectedHandles, CpuPostProjectionRequest, GpuPreparationError, GpuPreparationReceipt,
};
use crate::scene::SceneRuntime;
use crate::{CpuPositionView, RendererError};

use super::{
    DirectCountSemantics, FrameIdentity, GpuExecutionContext, GpuOwnerToken, PlanFrameInput,
    ProjectedWork,
};

#[derive(Debug, Error)]
pub(crate) enum CpuPostSortError {
    #[error("CPU PostSort allocation failed for {resource}")]
    AllocationFailed { resource: &'static str },
    #[error("CPU PostSort scene count mismatch: expected {expected}, got {actual}")]
    SceneCountMismatch { expected: u32, actual: usize },
    #[error("CPU PostSort order generation is exhausted")]
    OrderGenerationExhausted,
    #[error("CPU PostSort preprocessing or sorting failed: {0}")]
    Renderer(#[from] RendererError),
    #[error("CPU PostSort GPU projection failed: {0}")]
    GpuProjection(#[from] GpuPreparationError),
    #[error("CPU PostSort GPU projection contract is inconsistent at {component}")]
    ProjectionContractMismatch { component: &'static str },
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct CpuOrderGuard {
    scene_generation: u64,
    camera_revision: u64,
    contract_generation: u64,
    plan_set_generation: u64,
    source_count: u32,
    camera: Camera,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct CpuPostProjectionGuard {
    frame: FrameIdentity,
    source_count: u32,
    sh_degree: u8,
    camera: Camera,
    viewport_width: u32,
    viewport_height: u32,
    order_generation: u64,
    visible_count: u32,
    preparation: GpuPreparationReceipt,
}

/// Borrowed device completion of CpuPostSort. The CPU slice remains the
/// authoritative stable order; this view adds only the exact rank-indexed GPU
/// planes and the direct D=V identity needed by the later canonical raster.
pub(crate) struct CpuPostSortGpuWork<'a> {
    owner: GpuOwnerToken,
    handles: CpuPostProjectedHandles<'a>,
    guard: CpuPostProjectionGuard,
}

#[allow(dead_code)]
impl CpuPostSortGpuWork<'_> {
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

    pub(crate) const fn order_generation(&self) -> u64 {
        self.guard.order_generation
    }

    pub(crate) const fn direct_count(&self) -> u32 {
        self.guard.visible_count
    }

    pub(crate) const fn count_semantics(&self) -> DirectCountSemantics {
        DirectCountSemantics::DrawEqualsVisible
    }

    pub(crate) const fn receipt(&self) -> GpuPreparationReceipt {
        self.guard.preparation
    }

    pub(crate) fn same_owner(&self, owner: &GpuOwnerToken) -> bool {
        self.owner.same_owner(owner)
    }

    pub(crate) const fn ordered_source_ids(&self) -> &wgpu::Buffer {
        self.handles.ordered_source_ids()
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

    pub(crate) const fn projection_count_guard(&self) -> &wgpu::Buffer {
        self.handles.projection_count_guard()
    }
}

impl CpuOrderGuard {
    fn new(frame: FrameIdentity, source_count: u32, camera: Camera) -> Self {
        Self {
            scene_generation: frame.scene_generation(),
            camera_revision: frame.camera_revision(),
            contract_generation: frame.contract_generation(),
            plan_set_generation: frame.plan_set_generation(),
            source_count,
            camera,
        }
    }
}

/// Reusable workspace and authoritative order cache for Exact CPU PostSort.
pub(super) struct CpuPostSortPlan {
    engine: CpuOrderEngine,
    ordered_ids: Vec<u32>,
    guard: Option<CpuOrderGuard>,
    order_generation: u64,
}

impl CpuPostSortPlan {
    pub(super) fn prepare(source_count: usize) -> Result<Self, CpuPostSortError> {
        let engine = CpuOrderEngine::try_with_capacity(source_count).map_err(|error| {
            CpuPostSortError::AllocationFailed {
                resource: error.resource(),
            }
        })?;
        let mut ordered_ids = Vec::new();
        ordered_ids.try_reserve_exact(source_count).map_err(|_| {
            CpuPostSortError::AllocationFailed {
                resource: "authoritative source IDs",
            }
        })?;

        Ok(Self {
            engine,
            ordered_ids,
            guard: None,
            order_generation: 0,
        })
    }

    pub(super) fn execute<'a>(
        &'a mut self,
        scene: &'a mut SceneRuntime,
        input: PlanFrameInput<'_>,
        execution: Option<GpuExecutionContext<'_>>,
    ) -> Result<ProjectedWork<'a>, CpuPostSortError> {
        let camera = input.camera;
        let frame = input.frame;
        let source_count = input.source_count;
        let viewport_width = input.viewport_width;
        let viewport_height = input.viewport_height;
        if scene.source_count() != source_count as usize {
            return Err(CpuPostSortError::SceneCountMismatch {
                expected: source_count,
                actual: scene.source_count(),
            });
        }

        let gpu_preparation = execution
            .as_ref()
            .map(|context| {
                scene.validate_cpu_post_projection_context(
                    context.owner_token(),
                    camera,
                    viewport_width,
                    viewport_height,
                    frame,
                )
            })
            .transpose()?;
        let requested_guard = CpuOrderGuard::new(frame, source_count, *camera);
        if self.guard != Some(requested_guard) {
            self.refresh_order(scene.positions(), camera, requested_guard)?;
        }

        let cpu_post_gpu = match (execution, gpu_preparation) {
            (Some(execution), Some(preparation)) => {
                let visible_count = u32::try_from(self.ordered_ids.len()).map_err(|_| {
                    CpuPostSortError::ProjectionContractMismatch {
                        component: "CPU order length",
                    }
                })?;
                let guard = CpuPostProjectionGuard {
                    frame,
                    source_count,
                    sh_degree: scene.sh_degree(),
                    camera: *camera,
                    viewport_width,
                    viewport_height,
                    order_generation: self.order_generation,
                    visible_count,
                    preparation,
                };
                validate_projection_contract(scene, guard, &self.ordered_ids)?;
                let owner = execution.owner_token().clone();
                let handles = scene.encode_cpu_post_projection_frame(
                    execution,
                    CpuPostProjectionRequest::new(
                        &self.ordered_ids,
                        guard.camera,
                        (guard.viewport_width, guard.viewport_height),
                        guard.frame,
                        guard.order_generation,
                    ),
                )?;
                validate_encoded_handles(guard, &handles)?;
                Some(CpuPostSortGpuWork {
                    owner,
                    handles,
                    guard,
                })
            }
            (None, None) => None,
            _ => unreachable!("GPU preparation and execution are derived together"),
        };

        Ok(ProjectedWork::from_cpu_post_sort(
            frame,
            self.order_generation,
            source_count,
            &self.ordered_ids,
            cpu_post_gpu,
        ))
    }

    fn refresh_order(
        &mut self,
        positions: &[Vec3f],
        camera: &Camera,
        requested_guard: CpuOrderGuard,
    ) -> Result<(), CpuPostSortError> {
        let next_generation = self
            .order_generation
            .checked_add(1)
            .ok_or(CpuPostSortError::OrderGenerationExhausted)?;

        self.engine.order_positions(
            CpuPositionView::new(positions),
            camera,
            true,
            &mut self.ordered_ids,
        )?;
        self.order_generation = next_generation;
        self.guard = Some(requested_guard);
        Ok(())
    }

    pub(super) fn last_usable_order(&self) -> Option<&[u32]> {
        self.guard.as_ref().map(|_| self.ordered_ids.as_slice())
    }
}

fn validate_projection_contract(
    scene: &SceneRuntime,
    guard: CpuPostProjectionGuard,
    ordered_ids: &[u32],
) -> Result<(), CpuPostSortError> {
    guard
        .camera
        .validate()
        .map_err(|_| CpuPostSortError::ProjectionContractMismatch {
            component: "camera",
        })?;
    if guard.viewport_width == 0 || guard.viewport_height == 0 {
        return Err(CpuPostSortError::ProjectionContractMismatch {
            component: "viewport",
        });
    }
    let receipt = guard.preparation;
    if scene.source_count() != guard.source_count as usize
        || receipt.source_count() != guard.source_count
        || receipt.capacity() != guard.source_count
        || receipt.resident_count() != guard.source_count
        || receipt.addressable_count() != guard.source_count
    {
        return Err(CpuPostSortError::ProjectionContractMismatch {
            component: "source/capacity/resident/addressable count",
        });
    }
    if scene.sh_degree() != guard.sh_degree
        || receipt.sh_degree() != guard.sh_degree
        || guard.sh_degree > 3
    {
        return Err(CpuPostSortError::ProjectionContractMismatch {
            component: "SH degree",
        });
    }
    if guard.frame.scene_generation() != receipt.scene_generation()
        || guard.frame.contract_generation() != receipt.contract_generation()
        || guard.frame.plan_set_generation() != receipt.plan_set_generation()
    {
        return Err(CpuPostSortError::ProjectionContractMismatch {
            component: "scene/contract/plan-set generation",
        });
    }
    if ordered_ids.len() != guard.visible_count as usize
        || ordered_ids.len() > receipt.addressable_count() as usize
    {
        return Err(CpuPostSortError::ProjectionContractMismatch {
            component: "CPU order length/capacity",
        });
    }
    Ok(())
}

fn validate_encoded_handles(
    guard: CpuPostProjectionGuard,
    handles: &CpuPostProjectedHandles<'_>,
) -> Result<(), CpuPostSortError> {
    let receipt = handles.receipt();
    if receipt.frame_identity() != guard.frame
        || receipt.preparation() != guard.preparation
        || receipt.camera() != guard.camera
        || receipt.viewport() != (guard.viewport_width, guard.viewport_height)
        || receipt.order_generation() != guard.order_generation
        || receipt.visible_count() != guard.visible_count
    {
        return Err(CpuPostSortError::ProjectionContractMismatch {
            component: "encoded projection guard",
        });
    }
    if handles.projection_count_guard().size() < 16 {
        return Err(CpuPostSortError::ProjectionContractMismatch {
            component: "direct count guard",
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[cfg(not(target_arch = "wasm32"))]
    use std::sync::Arc;

    use gsplat_core::{Camera, RenderMode, SceneBuffers, Vec3f};

    use super::{
        CpuPostProjectionGuard, CpuPostSortError, CpuPostSortPlan, validate_projection_contract,
    };
    use crate::Renderer;
    #[cfg(not(target_arch = "wasm32"))]
    use crate::plans::{
        DirectCountSemantics, GpuOwnerToken, OrderLane, PlanId, PlanSetError, WorkUnavailable,
    };
    use crate::plans::{FrameIdentity, PlanFrameInput, ProjectedWork};
    #[cfg(not(target_arch = "wasm32"))]
    use crate::renderer::frame::Viewport;
    #[cfg(not(target_arch = "wasm32"))]
    use crate::renderer::gpu_prepare::GpuPreparationError;
    #[cfg(not(target_arch = "wasm32"))]
    use crate::renderer::{FrameExecutionError, PreparedRuntimeSlot, execute_frame_gpu};
    use crate::scene::{ResidentSceneCpu, SceneRuntime};

    fn scene_buffers(depths: &[f32]) -> SceneBuffers {
        scene_buffers_with_sh(depths, 0)
    }

    fn scene_buffers_with_sh(depths: &[f32], sh_degree: u8) -> SceneBuffers {
        let count = depths.len();
        let coefficient_count = match sh_degree {
            0 => 0,
            1 => 9,
            2 => 24,
            3 => 45,
            _ => unreachable!("Exact tests support only SH0-SH3"),
        };
        SceneBuffers {
            positions: depths
                .iter()
                .copied()
                .map(|z| Vec3f::new(0.0, 0.0, z))
                .collect(),
            opacity: vec![0.0; count],
            scale_xyz: vec![[0.0; 3]; count],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; count],
            color_dc: vec![[0.0; 3]; count],
            sh_degree,
            sh_rest: (coefficient_count != 0).then(|| vec![0.001; count * coefficient_count]),
        }
    }

    fn runtime(depths: &[f32]) -> SceneRuntime {
        let resident =
            ResidentSceneCpu::encode_owned(scene_buffers(depths)).expect("resident scene");
        SceneRuntime::prepare(resident).expect("scene runtime")
    }

    fn camera(near_plane: f32, far_plane: f32) -> Camera {
        let mut camera = Camera::default();
        camera.intrinsics.near_plane = near_plane;
        camera.intrinsics.far_plane = far_plane;
        camera
    }

    fn identity(camera_revision: u64) -> FrameIdentity {
        FrameIdentity::new(1, camera_revision, 1, 1, 1)
    }

    fn execute_cpu<'a>(
        plan: &'a mut CpuPostSortPlan,
        scene: &'a mut SceneRuntime,
        camera: &Camera,
        frame: FrameIdentity,
        source_count: u32,
    ) -> Result<ProjectedWork<'a>, CpuPostSortError> {
        plan.execute(
            scene,
            PlanFrameInput::new(camera, frame, source_count, 64, 64),
            None,
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn portable_limits() -> wgpu::Limits {
        let mut limits = wgpu::Limits::downlevel_defaults();
        limits.max_storage_buffers_per_shader_stage = 8;
        limits.max_storage_buffer_binding_size = 128 << 20;
        limits.max_buffer_size = 128 << 20;
        limits
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn request_device() -> Option<(
        wgpu::Instance,
        wgpu::AdapterInfo,
        Arc<wgpu::Device>,
        Arc<wgpu::Queue>,
    )> {
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
            #[cfg(target_os = "macos")]
            Err(error) => panic!("required CPU PostSort projection adapter unavailable: {error}"),
            #[cfg(not(target_os = "macos"))]
            Err(error) => {
                eprintln!("skipping CPU PostSort projection test; adapter unavailable: {error}");
                return None;
            }
        };
        let info = adapter.get_info();
        #[cfg(target_os = "macos")]
        assert_eq!(info.backend, wgpu::Backend::Metal, "Metal adapter required");
        let limits = portable_limits();
        if !limits.check_limits(&adapter.limits()) {
            #[cfg(target_os = "macos")]
            panic!("required CPU PostSort projection limits are unavailable: {limits:?}");
            #[cfg(not(target_os = "macos"))]
            {
                eprintln!("skipping CPU PostSort projection test; portable limits unavailable");
                return None;
            }
        }
        let descriptor = wgpu::DeviceDescriptor {
            label: Some("exact-cpu-post-projection-test-device"),
            required_features: wgpu::Features::empty(),
            required_limits: limits,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        };
        match adapter.request_device(&descriptor).await {
            Ok((device, queue)) => Some((instance, info, Arc::new(device), Arc::new(queue))),
            #[cfg(target_os = "macos")]
            Err(error) => panic!("required CPU PostSort projection device unavailable: {error}"),
            #[cfg(not(target_os = "macos"))]
            Err(error) => {
                eprintln!("skipping CPU PostSort projection test; device unavailable: {error}");
                None
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn prepared_cpu_post_projection_is_metal_and_discard_safe() {
        pollster::block_on(async {
            let Some((_instance, info, device, queue)) = request_device().await else {
                return;
            };
            eprintln!(
                "EXACT_CPU_POST_PROJECTION adapter={} backend={:?}",
                info.name, info.backend
            );
            let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
            for (count, sh_degree) in [(0, 0), (1, 1), (127, 2), (128, 3), (129, 0), (1_025, 3)] {
                let depths = (0..count)
                    .map(|index| match index % 4 {
                        0 => 1.0,
                        1 => 3.0,
                        _ => 2.0,
                    })
                    .collect::<Vec<_>>();
                let expected_order = [3.0, 2.0, 1.0]
                    .into_iter()
                    .flat_map(|depth| {
                        depths.iter().enumerate().filter_map(move |(index, value)| {
                            (*value == depth).then_some(index as u32)
                        })
                    })
                    .collect::<Vec<_>>();
                let resident =
                    ResidentSceneCpu::encode_owned(scene_buffers_with_sh(&depths, sh_degree))
                        .expect("scene");
                let mut slot = PreparedRuntimeSlot::prepare(resident).expect("CPU fallback");
                let preparation = slot
                    .prepare_gpu(&device, &queue, wgpu::TextureFormat::Rgba8Unorm)
                    .await
                    .expect("device-owned projection graph");
                assert_eq!(preparation.source_count(), count as u32);
                assert_eq!(preparation.capacity(), count as u32);
                assert_eq!(preparation.resident_count(), count as u32);
                assert_eq!(preparation.addressable_count(), count as u32);
                assert_eq!(preparation.sh_degree(), sh_degree);
                let viewport = Viewport::new(127, 65).expect("viewport");
                let camera = camera(1.0, 3.0);

                if count == 1 {
                    let base = CpuPostProjectionGuard {
                        frame: FrameIdentity::new(1, 1, 1, 1, 2),
                        source_count: 1,
                        sh_degree,
                        camera,
                        viewport_width: 127,
                        viewport_height: 65,
                        order_generation: 1,
                        visible_count: 1,
                        preparation,
                    };
                    assert!(validate_projection_contract(slot.scene(), base, &[0]).is_ok());
                    for frame in [
                        FrameIdentity::new(2, 1, 1, 1, 2),
                        FrameIdentity::new(1, 1, 1, 2, 2),
                        FrameIdentity::new(1, 1, 1, 1, 3),
                    ] {
                        assert!(matches!(
                            validate_projection_contract(
                                slot.scene(),
                                CpuPostProjectionGuard { frame, ..base },
                                &[0]
                            ),
                            Err(CpuPostSortError::ProjectionContractMismatch {
                                component: "scene/contract/plan-set generation"
                            })
                        ));
                    }
                    assert!(matches!(
                        validate_projection_contract(
                            slot.scene(),
                            CpuPostProjectionGuard {
                                source_count: 0,
                                ..base
                            },
                            &[0]
                        ),
                        Err(CpuPostSortError::ProjectionContractMismatch {
                            component: "source/capacity/resident/addressable count"
                        })
                    ));
                    assert!(matches!(
                        validate_projection_contract(
                            slot.scene(),
                            CpuPostProjectionGuard {
                                sh_degree: 0,
                                ..base
                            },
                            &[0]
                        ),
                        Err(CpuPostSortError::ProjectionContractMismatch {
                            component: "SH degree"
                        })
                    ));
                    let mut invalid_camera = camera;
                    invalid_camera.intrinsics.near_plane = 4.0;
                    invalid_camera.intrinsics.far_plane = 1.0;
                    assert!(matches!(
                        validate_projection_contract(
                            slot.scene(),
                            CpuPostProjectionGuard {
                                camera: invalid_camera,
                                ..base
                            },
                            &[0]
                        ),
                        Err(CpuPostSortError::ProjectionContractMismatch {
                            component: "camera"
                        })
                    ));
                    assert!(matches!(
                        validate_projection_contract(
                            slot.scene(),
                            CpuPostProjectionGuard {
                                viewport_width: 0,
                                ..base
                            },
                            &[0]
                        ),
                        Err(CpuPostSortError::ProjectionContractMismatch {
                            component: "viewport"
                        })
                    ));
                    assert!(matches!(
                        validate_projection_contract(
                            slot.scene(),
                            CpuPostProjectionGuard {
                                visible_count: 2,
                                ..base
                            },
                            &[0, 0]
                        ),
                        Err(CpuPostSortError::ProjectionContractMismatch {
                            component: "CPU order length/capacity"
                        })
                    ));
                }

                if count == 127 {
                    let before_frame = slot.frame_state();
                    let wrong_queue = Arc::new(queue.as_ref().clone());
                    let mut wrong_owner =
                        device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("wrong-owner-cpu-post-projection-test-encoder"),
                        });
                    assert!(matches!(
                        execute_frame_gpu(
                            &mut slot,
                            PlanId::CpuPostSort,
                            &camera,
                            viewport,
                            &wrong_queue,
                            &mut wrong_owner,
                        ),
                        Err(FrameExecutionError::GpuPreparation(
                            GpuPreparationError::ExecutionOwnerMismatch
                        ))
                    ));
                    drop(wrong_owner.finish());
                    assert_eq!(slot.frame_state(), before_frame);
                    assert!(slot.last_usable_cpu_order().is_none());
                    assert_eq!(slot.scene().gpu_cpu_post_projection_encode_count(), Some(0));
                }

                let mut discarded =
                    device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("discarded-cpu-post-projection-test-encoder"),
                    });
                let work = execute_frame_gpu(
                    &mut slot,
                    PlanId::CpuPostSort,
                    &camera,
                    viewport,
                    &queue,
                    &mut discarded,
                )
                .expect("CPU PostSort GPU projection");
                assert_eq!(work.order_lane(), OrderLane::Cpu);
                assert_eq!(work.cpu_order_ids().expect("CPU IDs"), expected_order);
                assert_eq!(work.visible_count(), Ok(count as u32));
                assert_eq!(work.draw_count(), Ok(count as u32));
                assert_eq!(
                    work.contributor_count(),
                    Err(WorkUnavailable::ContributorCount)
                );
                let order_generation = work.order_generation();
                let projected = work.cpu_post_gpu().expect("rank-indexed CPU projection");
                assert_eq!(projected.frame_identity(), work.frame_identity());
                assert_eq!(projected.order_generation(), order_generation);
                assert_eq!(projected.source_count(), count as u32);
                assert_eq!(projected.sh_degree(), sh_degree);
                assert_eq!(projected.camera(), camera);
                assert_eq!(projected.direct_count(), count as u32);
                assert_eq!(
                    projected.count_semantics(),
                    DirectCountSemantics::DrawEqualsVisible
                );
                assert_eq!(projected.receipt(), preparation);
                assert_eq!(projected.viewport(), (127, 65));
                let accepted_owner = projected.owner.clone();
                assert!(projected.same_owner(&accepted_owner));
                assert!(!projected.same_owner(&GpuOwnerToken::fresh()));
                assert!(projected.ordered_source_ids().size() >= (count.max(1) * 4) as u64);
                assert!(projected.projected_center_source().size() >= (count.max(1) * 16) as u64);
                assert!(projected.projected_axes().size() >= (count.max(1) * 16) as u64);
                assert!(projected.resolved_color().size() >= (count.max(1) * 4) as u64);
                assert_eq!(projected.projection_count_guard().size(), 16);
                drop(work);
                drop(discarded.finish());
                assert_eq!(slot.scene().gpu_cpu_post_projection_encode_count(), Some(1));

                let mut retry = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("retried-cpu-post-projection-test-encoder"),
                });
                let retried = execute_frame_gpu(
                    &mut slot,
                    PlanId::CpuPostSort,
                    &camera,
                    viewport,
                    &queue,
                    &mut retry,
                )
                .expect("discarded encoder requires complete CPU projection retry");
                assert_eq!(retried.order_generation(), order_generation);
                assert_eq!(retried.draw_count(), Ok(count as u32));
                drop(retried);
                drop(retry.finish());
                assert_eq!(slot.scene().gpu_cpu_post_projection_encode_count(), Some(2));

                if count == 1_025 {
                    let before_frame = slot.frame_state();
                    let before_order = slot.last_usable_cpu_order().expect("CPU order").to_vec();
                    let before_encodes = slot.scene().gpu_cpu_post_projection_encode_count();
                    let mut invalid_camera = camera;
                    invalid_camera.intrinsics.near_plane = 4.0;
                    invalid_camera.intrinsics.far_plane = 1.0;
                    let mut rejected =
                        device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("invalid-camera-cpu-post-projection-test-encoder"),
                        });
                    assert!(matches!(
                        execute_frame_gpu(
                            &mut slot,
                            PlanId::CpuPostSort,
                            &invalid_camera,
                            viewport,
                            &queue,
                            &mut rejected,
                        ),
                        Err(FrameExecutionError::PlanSet(PlanSetError::CpuPostSort(
                            CpuPostSortError::GpuProjection(GpuPreparationError::InvalidCamera)
                        )))
                    ));
                    drop(rejected.finish());
                    assert_eq!(slot.frame_state(), before_frame);
                    assert_eq!(slot.last_usable_cpu_order(), Some(before_order.as_slice()));
                    assert_eq!(
                        slot.scene().gpu_cpu_post_projection_encode_count(),
                        before_encodes
                    );
                }
            }
            assert!(
                validation.pop().await.is_none(),
                "CPU PostSort projection must be validation-clean"
            );
        });
    }

    #[test]
    fn near_and_far_are_inclusive() {
        let below_near = f32::from_bits(1.0_f32.to_bits() - 1);
        let above_far = f32::from_bits(3.0_f32.to_bits() + 1);
        let mut scene = runtime(&[1.0, below_near, 3.0, above_far]);
        let mut plan = CpuPostSortPlan::prepare(scene.source_count()).expect("plan");

        let work = execute_cpu(&mut plan, &mut scene, &camera(1.0, 3.0), identity(1), 4)
            .expect("CPU order");

        assert_eq!(work.cpu_order_ids().expect("CPU IDs"), [2, 0]);
        assert_eq!(work.visible_count(), Ok(2));
    }

    #[test]
    fn depth_is_descending_and_equal_depth_ties_keep_source_ids() {
        let mut scene = runtime(&[2.0, 3.0, 2.0, 1.0]);
        let mut plan = CpuPostSortPlan::prepare(scene.source_count()).expect("plan");

        let work = execute_cpu(&mut plan, &mut scene, &camera(0.5, 4.0), identity(1), 4)
            .expect("CPU order");

        assert_eq!(work.cpu_order_ids().expect("CPU IDs"), [1, 0, 2, 3]);
    }

    #[test]
    fn empty_and_single_point_orders_are_exact() {
        let mut empty = runtime(&[]);
        let mut empty_plan = CpuPostSortPlan::prepare(0).expect("empty plan");
        let empty_work = execute_cpu(
            &mut empty_plan,
            &mut empty,
            &camera(0.5, 4.0),
            identity(1),
            0,
        )
        .expect("empty order");
        assert!(empty_work.cpu_order_ids().expect("CPU IDs").is_empty());
        assert_eq!(
            empty_work.draw_count(),
            Err(crate::plans::WorkUnavailable::DrawCount)
        );
        assert!(matches!(
            empty_work.cpu_post_gpu(),
            Err(crate::plans::WorkUnavailable::GpuProjectedWork)
        ));

        let mut single = runtime(&[2.0]);
        let mut single_plan = CpuPostSortPlan::prepare(1).expect("single plan");
        let single_work = execute_cpu(
            &mut single_plan,
            &mut single,
            &camera(0.5, 4.0),
            identity(1),
            1,
        )
        .expect("single order");
        assert_eq!(single_work.cpu_order_ids().expect("CPU IDs"), [0]);
    }

    #[test]
    fn invalid_camera_preserves_the_last_usable_order() {
        let mut scene = runtime(&[1.0, 3.0, 2.0]);
        let mut plan = CpuPostSortPlan::prepare(scene.source_count()).expect("plan");
        let first = execute_cpu(&mut plan, &mut scene, &camera(0.5, 4.0), identity(1), 3)
            .expect("CPU order")
            .cpu_order_ids()
            .expect("CPU IDs")
            .to_vec();

        let error = execute_cpu(&mut plan, &mut scene, &camera(4.0, 1.0), identity(2), 3);

        assert!(error.is_err());
        assert_eq!(plan.last_usable_order(), Some(first.as_slice()));
    }

    #[test]
    fn identical_complete_guard_reuses_the_order_generation() {
        let mut scene = runtime(&[1.0, 3.0, 2.0]);
        let mut plan = CpuPostSortPlan::prepare(scene.source_count()).expect("plan");
        let first_generation =
            execute_cpu(&mut plan, &mut scene, &camera(0.5, 4.0), identity(1), 3)
                .expect("first order")
                .order_generation();
        let second_generation =
            execute_cpu(&mut plan, &mut scene, &camera(0.5, 4.0), identity(1), 3)
                .expect("reused order")
                .order_generation();

        assert_eq!(first_generation, second_generation);
    }

    #[test]
    fn sync_surface_legacy_offscreen_and_cpu_post_plan_share_exact_order() {
        let depths = (0..257)
            .map(|index| match index % 7 {
                0 => 1.0,
                1 | 2 => 2.0,
                3 => 3.0,
                4 => f32::from_bits(1.0_f32.to_bits() - 1),
                5 => f32::from_bits(3.0_f32.to_bits() + 1),
                _ => 2.5,
            })
            .collect::<Vec<_>>();
        let camera = camera(1.0, 3.0);

        let mut renderer = Renderer::new_for_surface(RenderMode::SortedAlpha).expect("renderer");
        renderer
            .load_scene(scene_buffers(&depths))
            .expect("legacy scene");
        let (legacy_order, _) = renderer
            .build_sorted_indices(&camera)
            .expect("legacy/offscreen order");
        renderer
            .build_surface_sorted_indices_with_sort_refresh(&camera, true)
            .expect("sync Surface order");
        let surface_order = renderer.current_sorted_indices().to_vec();

        let mut scene = runtime(&depths);
        let mut plan = CpuPostSortPlan::prepare(scene.source_count()).expect("plan");
        let source_count = scene.source_count() as u32;
        let plan_order = execute_cpu(&mut plan, &mut scene, &camera, identity(1), source_count)
            .expect("plan order")
            .cpu_order_ids()
            .expect("CPU IDs")
            .to_vec();

        assert_eq!(surface_order, legacy_order);
        assert_eq!(plan_order, legacy_order);
    }
}
