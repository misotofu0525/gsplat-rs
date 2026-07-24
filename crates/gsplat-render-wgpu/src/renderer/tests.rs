use gsplat_core::{Camera, SceneBuffers, Vec3f};
use thiserror::Error;

use super::{PlanId, PreparedRuntimeSlot, execute_frame};
use crate::plans::{FrameIdentity, OrderLane, WorkUnavailable};
use crate::renderer::frame::Viewport;
use crate::scene::ResidentSceneCpu;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShadowCurrentnessError {
    FrameIdentity,
    CameraOrViewport,
    SceneContract,
    AuthoritativeOrder,
    WorkCounts,
    UnsupportedWork,
    GenerationExhausted,
}

#[derive(Debug, Error)]
pub(crate) enum ShadowFrameError {
    #[error("shadow frame execution failed: {0}")]
    Frame(#[from] super::FrameExecutionError),
    #[error("shadow work accessor failed: {0}")]
    Work(#[from] WorkUnavailable),
}

#[derive(Debug)]
pub(crate) struct ShadowFrame {
    plan_id: PlanId,
    order_lane: OrderLane,
    frame_identity: FrameIdentity,
    order_generation: u64,
    source_count: u32,
    sh_degree: u8,
    visible_count: Result<u32, WorkUnavailable>,
    contributor_count: Result<u32, WorkUnavailable>,
    draw_count: Result<u32, WorkUnavailable>,
    cpu_order_ids: Vec<u32>,
}

impl ShadowFrame {
    pub(crate) const fn plan_id(&self) -> PlanId {
        self.plan_id
    }

    pub(crate) const fn order_lane(&self) -> OrderLane {
        self.order_lane
    }

    pub(crate) const fn frame_identity(&self) -> FrameIdentity {
        self.frame_identity
    }

    pub(crate) const fn order_generation(&self) -> u64 {
        self.order_generation
    }

    pub(crate) const fn source_count(&self) -> u32 {
        self.source_count
    }

    pub(crate) const fn sh_degree(&self) -> u8 {
        self.sh_degree
    }

    pub(crate) const fn visible_count(&self) -> Result<u32, WorkUnavailable> {
        self.visible_count
    }

    pub(crate) const fn contributor_count(&self) -> Result<u32, WorkUnavailable> {
        self.contributor_count
    }

    pub(crate) const fn draw_count(&self) -> Result<u32, WorkUnavailable> {
        self.draw_count
    }

    pub(crate) fn cpu_order_ids(&self) -> &[u32] {
        &self.cpu_order_ids
    }
}

pub(crate) fn capture_shadow_frame(
    slot: &mut PreparedRuntimeSlot,
    requested: PlanId,
    camera: &Camera,
    viewport: Viewport,
) -> Result<ShadowFrame, ShadowFrameError> {
    let sh_degree = slot.scene().sh_degree();
    let frame = {
        let work = execute_frame(slot, requested, camera, viewport)?;
        ShadowFrame {
            plan_id: work.plan_id(),
            order_lane: work.order_lane(),
            frame_identity: work.frame_identity(),
            order_generation: work.order_generation(),
            source_count: work.source_count(),
            sh_degree,
            visible_count: work.visible_count(),
            contributor_count: work.contributor_count(),
            draw_count: work.draw_count(),
            cpu_order_ids: work.cpu_order_ids()?.to_vec(),
        }
    };
    Ok(frame)
}

pub(crate) fn validate_current_shadow_frame(
    slot: &PreparedRuntimeSlot,
    frame: &ShadowFrame,
    camera: &Camera,
    viewport: Viewport,
) -> Result<(), ShadowCurrentnessError> {
    if slot.frame_state().identity() != frame.frame_identity {
        return Err(ShadowCurrentnessError::FrameIdentity);
    }
    let candidate = slot
        .frame_state()
        .candidate_for_frame(*camera, viewport)
        .map_err(|_| ShadowCurrentnessError::GenerationExhausted)?
        .identity();
    if candidate != frame.frame_identity {
        return Err(ShadowCurrentnessError::CameraOrViewport);
    }

    let source_count = u32::try_from(slot.scene().source_count())
        .map_err(|_| ShadowCurrentnessError::SceneContract)?;
    if source_count != frame.source_count || slot.scene().sh_degree() != frame.sh_degree {
        return Err(ShadowCurrentnessError::SceneContract);
    }
    if frame.plan_id != PlanId::CpuPostSort || frame.order_lane != OrderLane::Cpu {
        return Err(ShadowCurrentnessError::UnsupportedWork);
    }
    if slot.last_usable_cpu_order() != Some(frame.cpu_order_ids()) {
        return Err(ShadowCurrentnessError::AuthoritativeOrder);
    }
    if frame.visible_count != Ok(frame.cpu_order_ids.len() as u32)
        || frame.contributor_count != Err(WorkUnavailable::ContributorCount)
        || frame.draw_count != Err(WorkUnavailable::DrawCount)
    {
        return Err(ShadowCurrentnessError::WorkCounts);
    }
    Ok(())
}

fn resident(depths: &[f32]) -> ResidentSceneCpu {
    let count = depths.len();
    ResidentSceneCpu::encode_owned(SceneBuffers {
        positions: depths
            .iter()
            .copied()
            .map(|z| Vec3f::new(0.0, 0.0, z))
            .collect(),
        opacity: vec![0.0; count],
        scale_xyz: vec![[-3.0; 3]; count],
        rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; count],
        color_dc: vec![[0.0; 3]; count],
        sh_degree: 0,
        sh_rest: None,
    })
    .expect("resident scene")
}

fn frame_receipt(
    slot: &mut PreparedRuntimeSlot,
    camera: &Camera,
    viewport: Viewport,
) -> (crate::plans::FrameIdentity, u64, Vec<u32>) {
    let work = execute_frame(slot, PlanId::CpuPostSort, camera, viewport).expect("CPU frame");
    (
        work.frame_identity(),
        work.order_generation(),
        work.cpu_order_ids().expect("CPU IDs").to_vec(),
    )
}

#[test]
fn failed_replacement_preserves_runtime_generations_fallback_and_order() {
    let mut slot =
        PreparedRuntimeSlot::prepare(resident(&[1.0, 3.0, 2.0])).expect("prepared runtime");
    let (_, _, old_order) = frame_receipt(
        &mut slot,
        &Camera::default(),
        Viewport::new(640, 480).expect("viewport"),
    );
    let old_frame = slot.frame_state();
    let old_fallback = slot.fallback();
    let old_scene_positions = slot.scene().positions().as_ptr();

    let mut invalid = resident(&[9.0]);
    invalid.sh_degree = 4;
    let result = slot.replace(invalid);

    assert!(result.is_err());
    assert_eq!(slot.frame_state(), old_frame);
    assert_eq!(slot.fallback(), old_fallback);
    assert_eq!(slot.eligible(), [PlanId::CpuPostSort]);
    assert_eq!(slot.scene().positions().as_ptr(), old_scene_positions);
    assert_eq!(slot.last_usable_cpu_order(), Some(old_order.as_slice()));
}

#[test]
fn successful_replacement_publishes_one_fresh_runtime() {
    let mut slot = PreparedRuntimeSlot::prepare(resident(&[1.0])).expect("prepared runtime");
    let old_frame = slot.frame_state().identity();

    slot.replace(resident(&[2.0, 3.0]))
        .expect("runtime replacement");

    let new_frame = slot.frame_state().identity();
    assert_eq!(
        new_frame.scene_generation(),
        old_frame.scene_generation() + 1
    );
    assert_eq!(
        new_frame.contract_generation(),
        old_frame.contract_generation() + 1
    );
    assert_eq!(
        new_frame.plan_set_generation(),
        old_frame.plan_set_generation() + 1
    );
    assert_eq!(slot.scene().source_count(), 2);
    assert_eq!(slot.last_usable_cpu_order(), None);
}

#[test]
fn complete_camera_and_viewport_identity_reuses_or_invalidates_exactly() {
    let mut slot =
        PreparedRuntimeSlot::prepare(resident(&[1.0, 3.0, 2.0])).expect("prepared runtime");
    let viewport = Viewport::new(640, 480).expect("viewport");
    let (first, first_order_generation, _) = frame_receipt(&mut slot, &Camera::default(), viewport);
    let (unchanged, unchanged_order_generation, _) =
        frame_receipt(&mut slot, &Camera::default(), viewport);

    let mut positioned = Camera::default();
    positioned.pose.position.z = -1.0;
    let (position_identity, position_order_generation, _) =
        frame_receipt(&mut slot, &positioned, viewport);

    let half_angle = 0.125_f32;
    let mut rotated = positioned;
    rotated.pose.rotation_xyzw = [0.0, half_angle.sin(), 0.0, half_angle.cos()];
    let (rotation_identity, rotation_order_generation, _) =
        frame_receipt(&mut slot, &rotated, viewport);

    let mut changed_intrinsics = rotated;
    changed_intrinsics.intrinsics.vertical_fov_radians *= 0.9;
    let (intrinsics_identity, intrinsics_order_generation, _) =
        frame_receipt(&mut slot, &changed_intrinsics, viewport);

    let (resized_identity, resized_order_generation, _) = frame_receipt(
        &mut slot,
        &changed_intrinsics,
        Viewport::new(800, 600).expect("resized viewport"),
    );

    assert_eq!(unchanged, first);
    assert_eq!(unchanged_order_generation, first_order_generation);
    assert_eq!(
        position_identity.camera_revision(),
        first.camera_revision() + 1
    );
    assert_eq!(position_order_generation, first_order_generation + 1);
    assert_eq!(
        rotation_identity.camera_revision(),
        position_identity.camera_revision() + 1
    );
    assert_eq!(rotation_order_generation, position_order_generation + 1);
    assert_eq!(
        intrinsics_identity.camera_revision(),
        rotation_identity.camera_revision() + 1
    );
    assert_eq!(intrinsics_order_generation, rotation_order_generation + 1);
    assert_eq!(
        resized_identity.viewport_generation(),
        intrinsics_identity.viewport_generation() + 1
    );
    assert_eq!(
        resized_identity.camera_revision(),
        intrinsics_identity.camera_revision()
    );
    assert_eq!(resized_order_generation, intrinsics_order_generation);
}

#[test]
fn failed_frame_keeps_identity_fallback_and_last_usable_order() {
    let mut slot =
        PreparedRuntimeSlot::prepare(resident(&[1.0, 3.0, 2.0])).expect("prepared runtime");
    let viewport = Viewport::new(640, 480).expect("viewport");
    let (_, old_order_generation, old_order) =
        frame_receipt(&mut slot, &Camera::default(), viewport);
    let old_frame = slot.frame_state();
    let old_fallback = slot.fallback();

    let mut invalid_camera = Camera::default();
    invalid_camera.intrinsics.near_plane = 2.0;
    invalid_camera.intrinsics.far_plane = 1.0;
    assert!(
        execute_frame(
            &mut slot,
            PlanId::CpuPostSort,
            &invalid_camera,
            Viewport::new(800, 600).expect("candidate viewport"),
        )
        .is_err()
    );
    assert!(execute_frame(&mut slot, PlanId::GpuPostSort, &Camera::default(), viewport,).is_err());
    assert_eq!(slot.frame_state(), old_frame);
    assert_eq!(slot.fallback(), old_fallback);
    assert_eq!(slot.last_usable_cpu_order(), Some(old_order.as_slice()));
    let (_, reused_order_generation, reused_order) =
        frame_receipt(&mut slot, &Camera::default(), viewport);
    assert_eq!(reused_order_generation, old_order_generation);
    assert_eq!(reused_order, old_order);
}

#[cfg(not(target_arch = "wasm32"))]
mod canonical_submission {
    use std::sync::Arc;

    use super::*;
    use crate::plans::TestGpuAdmissionMode;
    use crate::renderer::{
        FrameExecutionError, GpuFrameEncodeRequest, RasterCountSemantics, encode_frame_gpu,
        submit_encoded_frame,
    };

    const WIDTH: u32 = 64;
    const HEIGHT: u32 = 64;
    const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

    fn exact_scene(count: usize, sh_degree: u8) -> ResidentSceneCpu {
        let coefficients = match sh_degree {
            0 => 0,
            1 => 9,
            2 => 24,
            3 => 45,
            _ => unreachable!("Exact fixture supports SH0-SH3"),
        };
        let buffers = SceneBuffers {
            positions: (0..count)
                .map(|index| {
                    let x = (index % 17) as f32 * 0.01 - 0.08;
                    let y = ((index / 17) % 9) as f32 * 0.01 - 0.04;
                    Vec3f::new(x, y, 1.0 + (index % 5) as f32 * 0.01)
                })
                .collect(),
            opacity: vec![0.0; count],
            scale_xyz: vec![[-3.0; 3]; count],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; count],
            color_dc: (0..count)
                .map(|index| {
                    let value = index as f32 / count.max(1) as f32;
                    [value * 0.25, value * 0.1, value * 0.05]
                })
                .collect(),
            sh_degree,
            sh_rest: (coefficients != 0).then(|| vec![0.001; count * coefficients]),
        };
        ResidentSceneCpu::encode_owned(buffers).expect("Exact resident fixture")
    }

    fn portable_limits() -> wgpu::Limits {
        let mut limits = wgpu::Limits::downlevel_defaults();
        limits.max_storage_buffers_per_shader_stage =
            crate::resident_gpu::RESIDENT_COLOR_STORAGE_BINDINGS;
        limits.max_storage_buffer_binding_size = 128 << 20;
        limits.max_buffer_size = 128 << 20;
        limits
    }

    async fn request_device() -> Option<(wgpu::AdapterInfo, Arc<wgpu::Device>, Arc<wgpu::Queue>)> {
        #[cfg(target_os = "macos")]
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            ..Default::default()
        });
        #[cfg(not(target_os = "macos"))]
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
            Err(error) => panic!("required Exact renderer Metal adapter unavailable: {error}"),
            #[cfg(not(target_os = "macos"))]
            Err(error) => {
                eprintln!("skipping optional Exact renderer GPU test: {error}");
                return None;
            }
        };
        let info = adapter.get_info();
        #[cfg(target_os = "macos")]
        assert_eq!(info.backend, wgpu::Backend::Metal, "Metal adapter required");
        let limits = portable_limits();
        if !limits.check_limits(&adapter.limits()) {
            #[cfg(target_os = "macos")]
            panic!("required Exact renderer Metal limits unavailable: {limits:?}");
            #[cfg(not(target_os = "macos"))]
            {
                eprintln!("skipping optional Exact renderer GPU test; limits unavailable");
                return None;
            }
        }
        let descriptor = wgpu::DeviceDescriptor {
            label: Some("exact-renderer-canonical-submission-test-device"),
            required_features: wgpu::Features::empty(),
            required_limits: limits,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        };
        match adapter.request_device(&descriptor).await {
            Ok((device, queue)) => Some((info, Arc::new(device), Arc::new(queue))),
            #[cfg(target_os = "macos")]
            Err(error) => panic!("required Exact renderer Metal device unavailable: {error}"),
            #[cfg(not(target_os = "macos"))]
            Err(error) => {
                eprintln!("skipping optional Exact renderer GPU test: {error}");
                None
            }
        }
    }

    fn target_and_readback(
        device: &wgpu::Device,
        label: &'static str,
    ) -> (wgpu::Texture, wgpu::TextureView, wgpu::Buffer) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: WIDTH,
                height: HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("exact-renderer-canonical-readback"),
            size: u64::from(WIDTH * HEIGHT * 4),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        (texture, view, readback)
    }

    fn append_readback(
        encoder: &mut wgpu::CommandEncoder,
        texture: &wgpu::Texture,
        readback: &wgpu::Buffer,
    ) {
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(WIDTH * 4),
                    rows_per_image: Some(HEIGHT),
                },
            },
            wgpu::Extent3d {
                width: WIDTH,
                height: HEIGHT,
                depth_or_array_layers: 1,
            },
        );
    }

    async fn render_plan(
        slot: &mut PreparedRuntimeSlot,
        device: &wgpu::Device,
        plan: PlanId,
    ) -> (crate::renderer::GpuFrameSubmission, Vec<u8>) {
        let (texture, view, readback) = target_and_readback(device, "exact-renderer-target");
        let validation_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut pending = encode_frame_gpu(
            slot,
            GpuFrameEncodeRequest::new(
                plan,
                &Camera::default(),
                Viewport::new(WIDTH, HEIGHT).expect("viewport"),
                &view,
                FORMAT,
                wgpu::Color::BLACK,
            ),
        )
        .expect("complete plan plus canonical raster encode");
        append_readback(pending.encoder_mut(), &texture, &readback);
        let submission =
            submit_encoded_frame(slot, pending).expect("single renderer-owned submission");

        let slice = readback.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission.submission_index().clone()),
                timeout: None,
            })
            .expect("wait for the exact submission");
        receiver.recv().expect("map callback").expect("map result");
        assert!(validation_scope.pop().await.is_none());
        let bytes = slice.get_mapped_range().to_vec();
        readback.unmap();
        (submission, bytes)
    }

    #[test]
    fn all_exact_plans_share_one_raster_and_one_submission() {
        pollster::block_on(async {
            let Some((info, device, queue)) = request_device().await else {
                return;
            };
            eprintln!(
                "EXACT_CANONICAL_SUBMISSION adapter={} backend={:?}",
                info.name, info.backend
            );

            for (count, sh_degree) in [(0, 0), (129, 3)] {
                let mut slot = PreparedRuntimeSlot::prepare(exact_scene(count, sh_degree))
                    .expect("prepared runtime");
                slot.set_test_gpu_admission_mode(TestGpuAdmissionMode::ConcreteAll);
                slot.prepare_gpu(&device, &queue, FORMAT)
                    .await
                    .expect("scene, plans and raster admitted atomically");

                let before = slot.frame_state();
                let (_stale_texture, stale_view, _stale_readback) =
                    target_and_readback(&device, "exact-renderer-stale-target");
                let stale = encode_frame_gpu(
                    &mut slot,
                    GpuFrameEncodeRequest::new(
                        PlanId::CpuPostSort,
                        &Camera::default(),
                        Viewport::new(WIDTH, HEIGHT).expect("viewport"),
                        &stale_view,
                        FORMAT,
                        wgpu::Color::BLACK,
                    ),
                )
                .expect("first pending frame");
                let (_newer_texture, newer_view, _newer_readback) =
                    target_and_readback(&device, "exact-renderer-newer-target");
                let newer = encode_frame_gpu(
                    &mut slot,
                    GpuFrameEncodeRequest::new(
                        PlanId::CpuPostSort,
                        &Camera::default(),
                        Viewport::new(WIDTH, HEIGHT).expect("viewport"),
                        &newer_view,
                        FORMAT,
                        wgpu::Color::BLACK,
                    ),
                )
                .expect("newer pending frame");
                assert_eq!(slot.frame_state(), before);
                assert!(matches!(
                    submit_encoded_frame(&mut slot, stale),
                    Err(FrameExecutionError::PendingFrameMismatch {
                        component: "latest encode attempt"
                    })
                ));
                drop(newer);
                assert_eq!(slot.frame_state(), before);

                let (_older_texture, older_view, _older_readback) =
                    target_and_readback(&device, "exact-renderer-before-failed-attempt-target");
                let older = encode_frame_gpu(
                    &mut slot,
                    GpuFrameEncodeRequest::new(
                        PlanId::CpuPostSort,
                        &Camera::default(),
                        Viewport::new(WIDTH, HEIGHT).expect("viewport"),
                        &older_view,
                        FORMAT,
                        wgpu::Color::BLACK,
                    ),
                )
                .expect("pending frame before a later failed attempt");
                let mut moved_camera = Camera::default();
                moved_camera.pose.position.x = 0.25;
                assert!(matches!(
                    encode_frame_gpu(
                        &mut slot,
                        GpuFrameEncodeRequest::new(
                            PlanId::CpuPostSort,
                            &moved_camera,
                            Viewport::new(WIDTH, HEIGHT).expect("viewport"),
                            &older_view,
                            wgpu::TextureFormat::Bgra8Unorm,
                            wgpu::Color::BLACK,
                        ),
                    ),
                    Err(FrameExecutionError::Raster(
                        crate::raster::CanonicalRasterError::TargetFormatMismatch { .. }
                    ))
                ));
                assert!(matches!(
                    submit_encoded_frame(&mut slot, older),
                    Err(FrameExecutionError::PendingFrameMismatch {
                        component: "latest encode attempt"
                    })
                ));
                assert_eq!(slot.frame_state(), before);

                let (cpu, cpu_image) = render_plan(&mut slot, &device, PlanId::CpuPostSort).await;
                let (gpu_post, gpu_post_image) =
                    render_plan(&mut slot, &device, PlanId::GpuPostSort).await;
                let (gpu_pre, gpu_pre_image) =
                    render_plan(&mut slot, &device, PlanId::GpuPreproject).await;

                assert_eq!(
                    cpu.count_semantics(),
                    RasterCountSemantics::DirectDrawEqualsVisible
                );
                assert_eq!(cpu.visible_count(), Some(count as u32));
                assert_eq!(cpu.draw_count(), Some(count as u32));
                assert_eq!(cpu.contributor_count(), None);
                assert_eq!(
                    gpu_post.count_semantics(),
                    RasterCountSemantics::IndirectDrawEqualsVisible
                );
                assert_eq!(
                    gpu_pre.count_semantics(),
                    RasterCountSemantics::IndirectDrawEqualsContributor
                );
                assert!(cpu.encode_attempt() < gpu_post.encode_attempt());
                assert!(gpu_post.encode_attempt() < gpu_pre.encode_attempt());
                assert_eq!(cpu_image, gpu_post_image);
                assert_eq!(cpu_image, gpu_pre_image);
                if count != 0 {
                    assert!(cpu_image.iter().any(|byte| *byte != 0));
                }
            }
        });
    }

    #[test]
    fn pending_frame_rejects_a_different_renderer_owner() {
        pollster::block_on(async {
            let Some((_info, device, queue)) = request_device().await else {
                return;
            };
            let mut first = PreparedRuntimeSlot::prepare(exact_scene(1, 0)).expect("first runtime");
            let mut second =
                PreparedRuntimeSlot::prepare(exact_scene(1, 0)).expect("second runtime");
            first.set_test_gpu_admission_mode(TestGpuAdmissionMode::ConcreteAll);
            second.set_test_gpu_admission_mode(TestGpuAdmissionMode::ConcreteAll);
            first
                .prepare_gpu(&device, &queue, FORMAT)
                .await
                .expect("first GPU runtime");
            second
                .prepare_gpu(&device, &queue, FORMAT)
                .await
                .expect("second GPU runtime");

            let (_texture, view, _readback) =
                target_and_readback(&device, "exact-renderer-owner-target");
            let pending = encode_frame_gpu(
                &mut first,
                GpuFrameEncodeRequest::new(
                    PlanId::CpuPostSort,
                    &Camera::default(),
                    Viewport::new(WIDTH, HEIGHT).expect("viewport"),
                    &view,
                    FORMAT,
                    wgpu::Color::BLACK,
                ),
            )
            .expect("pending first-owner frame");
            assert!(matches!(
                submit_encoded_frame(&mut second, pending),
                Err(FrameExecutionError::PendingFrameMismatch {
                    component: "GPU execution owner"
                })
            ));
        });
    }
}
