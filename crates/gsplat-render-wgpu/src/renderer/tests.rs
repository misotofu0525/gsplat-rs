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
    use std::{sync::Arc, time::Instant};

    use super::*;
    use crate::plans::TestGpuAdmissionMode;
    use crate::renderer::{
        FrameExecutionError, GpuFrameEncodeRequest, RasterCountSemantics, encode_frame_gpu,
        submit_encoded_frame, submit_encoded_frame_with_followup_for_test,
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
        target_and_readback_size(device, label, WIDTH, HEIGHT)
    }

    fn target_and_readback_size(
        device: &wgpu::Device,
        label: &'static str,
        width: u32,
        height: u32,
    ) -> (wgpu::Texture, wgpu::TextureView, wgpu::Buffer) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width,
                height,
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
            size: u64::from(width * height * 4),
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
        append_readback_size(encoder, texture, readback, WIDTH, HEIGHT);
    }

    fn append_readback_size(
        encoder: &mut wgpu::CommandEncoder,
        texture: &wgpu::Texture,
        readback: &wgpu::Buffer,
        width: u32,
        height: u32,
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
                    bytes_per_row: Some(width * 4),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
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

    async fn render_plan_split_single_submit(
        slot: &mut PreparedRuntimeSlot,
        device: &wgpu::Device,
        plan: PlanId,
    ) -> (crate::renderer::GpuFrameSubmission, Vec<u8>) {
        let (texture, view, readback) = target_and_readback(device, "exact-renderer-split-target");
        let validation_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let pending = encode_frame_gpu(
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
        .expect("complete render command buffer");
        let mut copy_encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("exact-renderer-split-copy-encoder"),
        });
        append_readback(&mut copy_encoder, &texture, &readback);
        let submission =
            submit_encoded_frame_with_followup_for_test(slot, pending, copy_encoder.finish())
                .expect("one submit with ordered render and copy command buffers");

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
            .expect("wait for split-path exact submission");
        receiver.recv().expect("map callback").expect("map result");
        assert!(validation_scope.pop().await.is_none());
        let bytes = slice.get_mapped_range().to_vec();
        readback.unmap();
        (submission, bytes)
    }

    fn assert_equivalent_submission(
        batched: &crate::renderer::GpuFrameSubmission,
        split: &crate::renderer::GpuFrameSubmission,
    ) {
        assert_eq!(batched.frame_identity(), split.frame_identity());
        assert_eq!(batched.plan_id(), split.plan_id());
        assert_eq!(batched.order_lane(), split.order_lane());
        assert_eq!(batched.order_generation(), split.order_generation());
        assert_eq!(batched.source_count(), split.source_count());
        assert_eq!(batched.visible_count(), split.visible_count());
        assert_eq!(batched.contributor_count(), split.contributor_count());
        assert_eq!(batched.draw_count(), split.draw_count());
        assert_eq!(batched.count_semantics(), split.count_semantics());
    }

    #[derive(Clone, Copy)]
    enum CommandLayout {
        Batched,
        SplitSingleSubmit,
    }

    #[derive(Clone, Copy)]
    struct FrameTiming {
        encode_submit_ms: f64,
        completion_ms: f64,
        terminal_ms: f64,
    }

    #[derive(Clone, Copy)]
    struct TimedFrameTarget<'a> {
        texture: &'a wgpu::Texture,
        view: &'a wgpu::TextureView,
        readback: &'a wgpu::Buffer,
        width: u32,
        height: u32,
    }

    async fn timed_frame(
        slot: &mut PreparedRuntimeSlot,
        device: &wgpu::Device,
        camera: &Camera,
        target: TimedFrameTarget<'_>,
        layout: CommandLayout,
    ) -> FrameTiming {
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let started = Instant::now();
        let mut pending = encode_frame_gpu(
            slot,
            GpuFrameEncodeRequest::new(
                PlanId::GpuPostSort,
                camera,
                Viewport::new(target.width, target.height).expect("benchmark viewport"),
                target.view,
                FORMAT,
                wgpu::Color::BLACK,
            ),
        )
        .expect("finite E10 plan/raster encode");

        let submission = match layout {
            CommandLayout::Batched => {
                append_readback_size(
                    pending.encoder_mut(),
                    target.texture,
                    target.readback,
                    target.width,
                    target.height,
                );
                submit_encoded_frame(slot, pending).expect("batched submission")
            }
            CommandLayout::SplitSingleSubmit => {
                let mut copy_encoder =
                    device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("e10-finite-split-copy-encoder"),
                    });
                append_readback_size(
                    &mut copy_encoder,
                    target.texture,
                    target.readback,
                    target.width,
                    target.height,
                );
                submit_encoded_frame_with_followup_for_test(slot, pending, copy_encoder.finish())
                    .expect("split single submission")
            }
        };
        let submitted = Instant::now();
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission.submission_index().clone()),
                timeout: None,
            })
            .expect("wait for exact finite E10 submission");
        let completed = Instant::now();
        assert!(scope.pop().await.is_none());
        FrameTiming {
            encode_submit_ms: (submitted - started).as_secs_f64() * 1_000.0,
            completion_ms: (completed - submitted).as_secs_f64() * 1_000.0,
            terminal_ms: (completed - started).as_secs_f64() * 1_000.0,
        }
    }

    async fn timed_pair(
        batched: &mut PreparedRuntimeSlot,
        split: &mut PreparedRuntimeSlot,
        device: &wgpu::Device,
        camera: &Camera,
        batched_target: TimedFrameTarget<'_>,
        split_target: TimedFrameTarget<'_>,
        batched_first: bool,
    ) -> (FrameTiming, FrameTiming) {
        if batched_first {
            let a = timed_frame(
                batched,
                device,
                camera,
                batched_target,
                CommandLayout::Batched,
            )
            .await;
            let b = timed_frame(
                split,
                device,
                camera,
                split_target,
                CommandLayout::SplitSingleSubmit,
            )
            .await;
            (a, b)
        } else {
            let b = timed_frame(
                split,
                device,
                camera,
                split_target,
                CommandLayout::SplitSingleSubmit,
            )
            .await;
            let a = timed_frame(
                batched,
                device,
                camera,
                batched_target,
                CommandLayout::Batched,
            )
            .await;
            (a, b)
        }
    }

    fn median(values: &[f64]) -> f64 {
        let mut sorted = values.to_vec();
        sorted.sort_by(f64::total_cmp);
        sorted[sorted.len() / 2]
    }

    fn p95(values: &[f64]) -> f64 {
        let mut sorted = values.to_vec();
        sorted.sort_by(f64::total_cmp);
        sorted[(sorted.len() * 95).div_ceil(100).saturating_sub(1)]
    }

    #[test]
    fn single_encoder_matches_split_single_submit() {
        pollster::block_on(async {
            let Some((info, device, queue)) = request_device().await else {
                return;
            };
            #[cfg(target_os = "macos")]
            assert_eq!(info.backend, wgpu::Backend::Metal);

            for plan in [
                PlanId::CpuPostSort,
                PlanId::GpuPostSort,
                PlanId::GpuPreproject,
            ] {
                let scene = exact_scene(129, 3);
                let mut batched =
                    PreparedRuntimeSlot::prepare(scene.clone()).expect("batched runtime");
                let mut split = PreparedRuntimeSlot::prepare(scene).expect("split runtime");
                for slot in [&mut batched, &mut split] {
                    slot.set_test_gpu_admission_mode(TestGpuAdmissionMode::ConcreteAll);
                    slot.prepare_gpu(&device, &queue, FORMAT)
                        .await
                        .expect("complete exact runtime");
                }

                let before_batched = batched.frame_state();
                let before_split = split.frame_state();
                assert_eq!(before_batched, before_split);
                let (batched_submission, batched_image) =
                    render_plan(&mut batched, &device, plan).await;
                let (split_submission, split_image) =
                    render_plan_split_single_submit(&mut split, &device, plan).await;
                assert_equivalent_submission(&batched_submission, &split_submission);
                assert_eq!(batched.frame_state(), split.frame_state());
                assert_ne!(batched.frame_state(), before_batched);
                assert_eq!(batched_image, split_image);
            }
        });
    }

    #[test]
    #[ignore = "finite E10 release observation; run exactly once on required Metal"]
    fn finite_single_encoder_batching_experiment() {
        pollster::block_on(async {
            const BENCH_WIDTH: u32 = 640;
            const BENCH_HEIGHT: u32 = 480;
            const WARMUP_PAIRS: usize = 10;
            const MEASURED_PAIRS: usize = 100;
            const BLOCKS: usize = 5;

            let Some((info, device, queue)) = request_device().await else {
                return;
            };
            #[cfg(target_os = "macos")]
            assert_eq!(info.backend, wgpu::Backend::Metal);
            let scene = exact_scene(65_537, 3);
            let mut batched =
                PreparedRuntimeSlot::prepare(scene.clone()).expect("batched benchmark runtime");
            let mut split = PreparedRuntimeSlot::prepare(scene).expect("split benchmark runtime");
            for slot in [&mut batched, &mut split] {
                slot.set_test_gpu_admission_mode(TestGpuAdmissionMode::ConcreteAll);
                slot.prepare_gpu(&device, &queue, FORMAT)
                    .await
                    .expect("complete exact benchmark runtime");
            }
            let (batched_texture, batched_view, batched_readback) = target_and_readback_size(
                &device,
                "e10-finite-batched-target",
                BENCH_WIDTH,
                BENCH_HEIGHT,
            );
            let (split_texture, split_view, split_readback) = target_and_readback_size(
                &device,
                "e10-finite-split-target",
                BENCH_WIDTH,
                BENCH_HEIGHT,
            );

            let batched_target = TimedFrameTarget {
                texture: &batched_texture,
                view: &batched_view,
                readback: &batched_readback,
                width: BENCH_WIDTH,
                height: BENCH_HEIGHT,
            };
            let split_target = TimedFrameTarget {
                texture: &split_texture,
                view: &split_view,
                readback: &split_readback,
                width: BENCH_WIDTH,
                height: BENCH_HEIGHT,
            };
            let camera_for_pair = |pair_index: usize| {
                let mut camera = Camera::default();
                camera.pose.position.x = if pair_index.is_multiple_of(2) {
                    0.0
                } else {
                    0.03
                };
                camera
            };

            for pair in 0..WARMUP_PAIRS {
                let camera = camera_for_pair(pair);
                let _ = timed_pair(
                    &mut batched,
                    &mut split,
                    &device,
                    &camera,
                    batched_target,
                    split_target,
                    pair.is_multiple_of(2),
                )
                .await;
            }
            let mut batched_samples = Vec::with_capacity(MEASURED_PAIRS);
            let mut split_samples = Vec::with_capacity(MEASURED_PAIRS);
            for pair in 0..MEASURED_PAIRS {
                let pair_index = pair + WARMUP_PAIRS;
                let camera = camera_for_pair(pair_index);
                let (a, b) = timed_pair(
                    &mut batched,
                    &mut split,
                    &device,
                    &camera,
                    batched_target,
                    split_target,
                    pair_index.is_multiple_of(2),
                )
                .await;
                batched_samples.push(a);
                split_samples.push(b);
            }

            let block_size = MEASURED_PAIRS / BLOCKS;
            let block_wins = (0..BLOCKS)
                .filter(|block| {
                    let range = block * block_size..(block + 1) * block_size;
                    median(
                        &batched_samples[range.clone()]
                            .iter()
                            .map(|sample| sample.terminal_ms)
                            .collect::<Vec<_>>(),
                    ) < median(
                        &split_samples[range]
                            .iter()
                            .map(|sample| sample.terminal_ms)
                            .collect::<Vec<_>>(),
                    )
                })
                .count();
            let metric = |samples: &[FrameTiming], field: fn(FrameTiming) -> f64| {
                samples.iter().copied().map(field).collect::<Vec<_>>()
            };
            let a_terminal = metric(&batched_samples, |sample| sample.terminal_ms);
            let b_terminal = metric(&split_samples, |sample| sample.terminal_ms);
            let a_median = median(&a_terminal);
            let b_median = median(&b_terminal);
            let decision = if a_median < b_median && block_wins >= 4 {
                "Accept"
            } else {
                "Reject"
            };
            eprintln!(
                "E10_BATCHING decision={decision} adapter={} backend={:?} pairs={} block_wins={}/{} batched_encode_submit_median_ms={:.6} split_encode_submit_median_ms={:.6} batched_completion_median_ms={:.6} split_completion_median_ms={:.6} batched_terminal_median_ms={:.6} split_terminal_median_ms={:.6} batched_terminal_p95_ms={:.6} split_terminal_p95_ms={:.6}",
                info.name,
                info.backend,
                MEASURED_PAIRS,
                block_wins,
                BLOCKS,
                median(&metric(&batched_samples, |sample| sample.encode_submit_ms)),
                median(&metric(&split_samples, |sample| sample.encode_submit_ms)),
                median(&metric(&batched_samples, |sample| sample.completion_ms)),
                median(&metric(&split_samples, |sample| sample.completion_ms)),
                a_median,
                b_median,
                p95(&a_terminal),
                p95(&b_terminal),
            );
        });
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
