//! E12 two-phase target-publication tests.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;

use gsplat_core::{Camera, SceneBuffers, Vec3f};

use super::controller::{ControllerConfig, SampleDisposition};
use super::sampler::PlanSampleDescriptor;
use super::*;
use crate::plans::TestGpuAdmissionMode;

const WIDTH: u32 = 64;
const HEIGHT: u32 = 64;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

fn exact_scene(count: usize) -> ResidentSceneCpu {
    let buffers = SceneBuffers {
        positions: (0..count)
            .map(|index| {
                Vec3f::new(
                    (index % 17) as f32 * 0.01 - 0.08,
                    ((index / 17) % 9) as f32 * 0.01 - 0.04,
                    1.0 + (index % 5) as f32 * 0.01,
                )
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
        sh_degree: 0,
        sh_rest: None,
    };
    ResidentSceneCpu::encode_owned(buffers).expect("Exact E12 fixture")
}

fn portable_limits() -> wgpu::Limits {
    let mut limits = wgpu::Limits::downlevel_defaults();
    limits.max_storage_buffers_per_shader_stage =
        crate::resident_gpu::RESIDENT_COLOR_STORAGE_BINDINGS;
    limits.max_storage_buffer_binding_size = 128 << 20;
    limits.max_buffer_size = 128 << 20;
    limits
}

async fn request_device() -> Option<(Arc<wgpu::Device>, Arc<wgpu::Queue>)> {
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
        Err(error) => panic!("required E12 Metal adapter unavailable: {error}"),
        #[cfg(not(target_os = "macos"))]
        Err(error) => {
            eprintln!("skipping optional E12 GPU test: {error}");
            return None;
        }
    };
    #[cfg(target_os = "macos")]
    assert_eq!(adapter.get_info().backend, wgpu::Backend::Metal);
    let limits = portable_limits();
    if !limits.check_limits(&adapter.limits()) {
        #[cfg(target_os = "macos")]
        panic!("required E12 Metal limits unavailable: {limits:?}");
        #[cfg(not(target_os = "macos"))]
        return None;
    }
    let descriptor = wgpu::DeviceDescriptor {
        label: Some("exact-e12-target-transaction-device"),
        required_features: wgpu::Features::empty(),
        required_limits: limits,
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    };
    match adapter.request_device(&descriptor).await {
        Ok((device, queue)) => Some((Arc::new(device), Arc::new(queue))),
        #[cfg(target_os = "macos")]
        Err(error) => panic!("required E12 Metal device unavailable: {error}"),
        #[cfg(not(target_os = "macos"))]
        Err(error) => {
            eprintln!("skipping optional E12 GPU test: {error}");
            None
        }
    }
}

fn target(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("exact-e12-target"),
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
    })
}

async fn prepared_slot(
    device: &Arc<wgpu::Device>,
    queue: &Arc<wgpu::Queue>,
) -> PreparedRuntimeSlot {
    let mut slot = PreparedRuntimeSlot::prepare(exact_scene(129)).expect("CPU runtime");
    slot.set_test_gpu_admission_mode(TestGpuAdmissionMode::ConcreteAll);
    slot.prepare_gpu(device, queue, FORMAT)
        .await
        .expect("three Exact plans and canonical raster");
    slot.set_test_controller_config(ControllerConfig::accelerated());
    slot.enable_test_plan_evidence();
    slot
}

fn submit_adaptive(
    slot: &mut PreparedRuntimeSlot,
    device: &wgpu::Device,
    camera: &Camera,
    viewport: Viewport,
) -> (wgpu::Texture, SubmittedGpuFrame) {
    let texture = target(device, viewport.width(), viewport.height());
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let pending = encode_frame_gpu(
        slot,
        GpuFrameEncodeRequest::adaptive(camera, viewport, &view, FORMAT, wgpu::Color::BLACK),
    )
    .expect("adaptive Exact encode");
    let submitted =
        submit_encoded_frame_unpublished(slot, pending).expect("unpublished queue submit");
    (texture, submitted)
}

fn wait(device: &wgpu::Device, submitted: &SubmittedGpuFrame) {
    device
        .poll(wgpu::PollType::Wait {
            submission_index: submitted.submission_index().cloned(),
            timeout: None,
        })
        .expect("wait E12 submission");
}

fn arm_old_presented_sample(
    slot: &mut PreparedRuntimeSlot,
    device: &wgpu::Device,
) -> (wgpu::CommandBuffer, PlanSampleTicket) {
    let decision = slot
        .controller
        .choose_adaptive()
        .expect("bootstrap decision");
    assert!(decision.formal_kind().is_some());
    let encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("exact-e12-old-presented-sample"),
    });
    let command_buffer = encoder.finish();
    let frame = slot.frame.identity();
    let source_count = slot.runtime.contract.source_count;
    let ticket = slot
        .sampler
        .arm(
            &command_buffer,
            PlanSampleDescriptor {
                probe_generation: decision.probe_generation(),
                comparison: decision.comparison(),
                frame,
                plan: decision.plan(),
                order_lane: OrderLane::Cpu,
                order_generation: 1,
                visible_count: Some(source_count),
                contributor_count: None,
                draw_count: Some(source_count),
                count_semantics: RasterCountSemantics::DirectDrawEqualsVisible,
            },
            crate::timer_now(),
        )
        .expect("old presented sample");
    assert!(
        slot.controller
            .register_pending(decision, ticket, OrderLane::Cpu)
    );
    (command_buffer, ticket)
}

#[test]
fn submitted_but_unpresented_completion_is_never_published_or_observed() {
    pollster::block_on(async {
        let Some((device, queue)) = request_device().await else {
            return;
        };
        let mut slot = prepared_slot(&device, &queue).await;
        let before = slot.frame_state();
        let (_texture, mut submitted) = submit_adaptive(
            &mut slot,
            &device,
            &Camera::default(),
            Viewport::new(WIDTH, HEIGHT).expect("viewport"),
        );
        assert_eq!(slot.frame_state(), before);
        assert!(!slot.sampler_pending_for_test());
        assert!(slot.drain_test_plan_evidence().is_empty());

        wait(&device, &submitted);
        assert!(slot.poll_test_plan_sampler().is_none());
        assert!(abandon_submitted_frame(&mut submitted));
        assert!(!abandon_submitted_frame(&mut submitted));
        assert_eq!(slot.frame_state(), before);

        let (_retry_texture, mut retry) = submit_adaptive(
            &mut slot,
            &device,
            &Camera::default(),
            Viewport::new(WIDTH, HEIGHT).expect("viewport"),
        );
        let published = finalize_submitted_frame(&mut slot, &mut retry).expect("presented retry");
        assert!(published.plan_sample_ticket().is_some());
        assert_ne!(slot.frame_state(), before);
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(published.submission_index().clone()),
                timeout: None,
            })
            .expect("wait presented retry submission");
        let (sample, disposition) = slot
            .poll_test_plan_sampler()
            .expect("only presented retry sample");
        assert_eq!(disposition, SampleDisposition::Accepted);
        assert_eq!(Some(sample.sample_ticket()), published.plan_sample_ticket());
        assert_eq!(slot.drain_test_plan_evidence(), vec![sample]);
    });
}

#[test]
fn stale_and_duplicate_target_finalize_fail_closed() {
    pollster::block_on(async {
        let Some((device, queue)) = request_device().await else {
            return;
        };
        let mut slot = prepared_slot(&device, &queue).await;
        let before = slot.frame_state();
        let (_first_texture, mut first) = submit_adaptive(
            &mut slot,
            &device,
            &Camera::default(),
            Viewport::new(WIDTH, HEIGHT).expect("viewport"),
        );
        let second_texture = target(&device, WIDTH, HEIGHT);
        let second_view = second_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let _newer_pending = encode_frame_gpu(
            &mut slot,
            GpuFrameEncodeRequest::new(
                PlanId::CpuPostSort,
                &Camera::default(),
                Viewport::new(WIDTH, HEIGHT).expect("viewport"),
                &second_view,
                FORMAT,
                wgpu::Color::BLACK,
            ),
        )
        .expect("newer encode invalidates older target token");
        assert!(matches!(
            finalize_submitted_frame(&mut slot, &mut first),
            Err(FrameExecutionError::PendingFrameMismatch {
                component: "latest encode attempt"
            })
        ));
        assert_eq!(slot.frame_state(), before);
        assert!(abandon_submitted_frame(&mut first));

        let (_third_texture, mut third) = submit_adaptive(
            &mut slot,
            &device,
            &Camera::default(),
            Viewport::new(WIDTH, HEIGHT).expect("viewport"),
        );
        finalize_submitted_frame(&mut slot, &mut third).expect("first finalize");
        let published = slot.frame_state();
        assert!(matches!(
            finalize_submitted_frame(&mut slot, &mut third),
            Err(FrameExecutionError::PendingFrameMismatch {
                component: "submitted target token"
            })
        ));
        assert_eq!(slot.frame_state(), published);
    });
}

#[test]
fn key_change_abort_preserves_old_sample_while_present_replaces_it() {
    pollster::block_on(async {
        let Some((device, queue)) = request_device().await else {
            return;
        };

        let mut aborted = prepared_slot(&device, &queue).await;
        let (old_buffer, old_ticket) = arm_old_presented_sample(&mut aborted, &device);
        let (_texture, mut changed) = submit_adaptive(
            &mut aborted,
            &device,
            &Camera::default(),
            Viewport::new(WIDTH + 1, HEIGHT).expect("changed viewport"),
        );
        assert_eq!(aborted.sampler.pending_ticket(), Some(old_ticket));
        assert!(abandon_submitted_frame(&mut changed));
        assert_eq!(aborted.sampler.pending_ticket(), Some(old_ticket));
        let old_submission = queue.submit([old_buffer]);
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(old_submission),
                timeout: None,
            })
            .expect("old presented sample completion");
        let (old_sample, disposition) = aborted
            .poll_test_plan_sampler()
            .expect("old sample survived aborted key change");
        assert_eq!(old_sample.sample_ticket(), old_ticket);
        assert_eq!(disposition, SampleDisposition::Accepted);

        let mut presented = prepared_slot(&device, &queue).await;
        let (old_buffer, old_ticket) = arm_old_presented_sample(&mut presented, &device);
        let (_texture, mut changed) = submit_adaptive(
            &mut presented,
            &device,
            &Camera::default(),
            Viewport::new(WIDTH + 1, HEIGHT).expect("changed viewport"),
        );
        let changed_index = changed
            .submission_index()
            .cloned()
            .expect("unpublished index");
        let changed_result =
            finalize_submitted_frame(&mut presented, &mut changed).expect("present key change");
        let changed_ticket = changed_result
            .plan_sample_ticket()
            .expect("new comparison sample");
        assert_ne!(old_ticket, changed_ticket);
        assert_eq!(presented.sampler.pending_ticket(), Some(changed_ticket));

        let old_submission = queue.submit([old_buffer]);
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(changed_index),
                timeout: None,
            })
            .expect("new key sample completion");
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(old_submission),
                timeout: None,
            })
            .expect("orphan old sample completion");
        let (sample, disposition) = presented
            .poll_test_plan_sampler()
            .expect("only new sample remains observable");
        assert_eq!(sample.sample_ticket(), changed_ticket);
        assert_eq!(disposition, SampleDisposition::Accepted);
        assert!(presented.poll_test_plan_sampler().is_none());
    });
}
