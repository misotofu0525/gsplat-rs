//! Focused E11 whole-plan controller and terminal-sampler tests.

use super::controller::{
    ControllerConfig, FormalSampleKind, PlanDecision, SampleDisposition, WholePlanController,
};
use super::*;
use crate::evidence::{
    BoundedEvidenceRing, PlanComparisonKey, PlanCountSemantics, PlanSample, PlanSampleTicket,
};
use crate::plans::{FrameIdentity, OrderLane, PlanId};

const FRAME: FrameIdentity = FrameIdentity::new(3, 11, 5, 7, 9);

fn comparison(frame: FrameIdentity) -> PlanComparisonKey {
    PlanComparisonKey::new(frame, 129, 3)
}

fn controller(eligible: &[PlanId]) -> WholePlanController {
    let mut controller = WholePlanController::new(PlanId::CpuPostSort, eligible, comparison(FRAME));
    controller.set_config_for_test(ControllerConfig::accelerated());
    controller
}

fn sample_for(
    decision: PlanDecision,
    ticket: u64,
    frame: FrameIdentity,
    completion_ms: f32,
) -> PlanSample {
    let sample_ticket = PlanSampleTicket::new(
        ticket,
        decision.probe_generation(),
        decision.comparison(),
        decision.plan(),
    );
    match decision.plan() {
        PlanId::CpuPostSort => PlanSample::new(
            sample_ticket,
            frame,
            OrderLane::Cpu,
            ticket,
            Some(97),
            None,
            Some(97),
            PlanCountSemantics::DirectDrawEqualsVisible,
            completion_ms,
        ),
        PlanId::GpuPostSort => PlanSample::new(
            sample_ticket,
            frame,
            OrderLane::Gpu,
            ticket,
            None,
            None,
            None,
            PlanCountSemantics::IndirectDrawEqualsVisible,
            completion_ms,
        ),
        PlanId::GpuPreproject => PlanSample::new(
            sample_ticket,
            frame,
            OrderLane::Gpu,
            ticket,
            None,
            None,
            None,
            PlanCountSemantics::IndirectDrawEqualsContributor,
            completion_ms,
        ),
    }
}

fn complete(
    controller: &mut WholePlanController,
    decision: PlanDecision,
    ticket: &mut u64,
    completion_ms: f32,
) -> PlanSample {
    assert!(decision.formal_kind().is_some());
    let sample = sample_for(decision, *ticket, FRAME, completion_ms);
    assert!(controller.register_pending(decision, sample.sample_ticket()));
    assert_eq!(controller.observe(sample), SampleDisposition::Accepted);
    *ticket += 1;
    sample
}

fn drive_first_probe(controller: &mut WholePlanController, challenger_ms: f32) {
    let mut ticket = 1;
    let bootstrap = controller.choose_adaptive().expect("bootstrap decision");
    assert_eq!(bootstrap.plan(), PlanId::CpuPostSort);
    assert_eq!(bootstrap.formal_kind(), Some(FormalSampleKind::Bootstrap));
    complete(controller, bootstrap, &mut ticket, 10.0);

    let incumbent_a = controller.choose_adaptive().expect("ABBA A");
    assert_eq!(
        incumbent_a.formal_kind(),
        Some(FormalSampleKind::Probe { index: 0 })
    );
    complete(controller, incumbent_a, &mut ticket, 10.0);

    let warm_challenger = controller.choose_adaptive().expect("challenger warmup");
    assert_eq!(warm_challenger.plan(), PlanId::GpuPostSort);
    assert_eq!(
        warm_challenger.formal_kind(),
        Some(FormalSampleKind::TransitionWarmup)
    );
    complete(controller, warm_challenger, &mut ticket, challenger_ms);

    for index in [1, 2] {
        let challenger = controller.choose_adaptive().expect("challenger sample");
        assert_eq!(challenger.plan(), PlanId::GpuPostSort);
        assert_eq!(
            challenger.formal_kind(),
            Some(FormalSampleKind::Probe { index })
        );
        complete(controller, challenger, &mut ticket, challenger_ms);
    }

    let warm_incumbent = controller.choose_adaptive().expect("incumbent warmup");
    assert_eq!(warm_incumbent.plan(), PlanId::CpuPostSort);
    assert_eq!(
        warm_incumbent.formal_kind(),
        Some(FormalSampleKind::TransitionWarmup)
    );
    complete(controller, warm_incumbent, &mut ticket, 10.0);

    let incumbent_b = controller.choose_adaptive().expect("ABBA A close");
    assert_eq!(
        incumbent_b.formal_kind(),
        Some(FormalSampleKind::Probe { index: 3 })
    );
    complete(controller, incumbent_b, &mut ticket, 10.0);
}

#[test]
fn controller_interleaves_complete_plans_and_requires_hysteresis() {
    let eligible = [
        PlanId::CpuPostSort,
        PlanId::GpuPostSort,
        PlanId::GpuPreproject,
    ];
    let mut winning = controller(&eligible);
    drive_first_probe(&mut winning, 5.0);
    assert_eq!(winning.incumbent_for_test(), PlanId::GpuPostSort);

    // Promotion creates a real minimum residency. Only after the submitted
    // incumbent frame does the next fixed-order challenger become eligible.
    let resident = winning.choose_adaptive().expect("minimum residency");
    assert_eq!(resident.plan(), PlanId::GpuPostSort);
    assert_eq!(resident.formal_kind(), None);
    winning.submitted_without_sample(resident);
    let next_probe = winning.choose_adaptive().expect("next challenger probe");
    assert_eq!(next_probe.plan(), PlanId::GpuPostSort);
    assert_eq!(
        next_probe.formal_kind(),
        Some(FormalSampleKind::Probe { index: 0 })
    );

    let mut close = controller(&eligible);
    drive_first_probe(&mut close, 9.5);
    assert_eq!(close.incumbent_for_test(), PlanId::CpuPostSort);
}

#[test]
fn failed_challenger_cools_down_then_is_reprobed() {
    let eligible = [PlanId::CpuPostSort, PlanId::GpuPostSort];
    let mut controller = controller(&eligible);
    let mut ticket = 1;
    let bootstrap = controller.choose_adaptive().expect("bootstrap");
    complete(&mut controller, bootstrap, &mut ticket, 10.0);
    let incumbent = controller
        .choose_adaptive()
        .expect("first incumbent sample");
    complete(&mut controller, incumbent, &mut ticket, 10.0);
    let challenger = controller.choose_adaptive().expect("challenger warmup");
    assert_eq!(challenger.plan(), PlanId::GpuPostSort);
    controller.execution_failed(challenger);

    for _ in 0..2 {
        let held = controller.choose_adaptive().expect("cooldown incumbent");
        assert_eq!(held.plan(), PlanId::CpuPostSort);
        assert_eq!(held.formal_kind(), None);
        controller.submitted_without_sample(held);
    }
    let reprobe = controller.choose_adaptive().expect("bounded reprobe");
    assert_eq!(reprobe.plan(), PlanId::CpuPostSort);
    assert_eq!(
        reprobe.formal_kind(),
        Some(FormalSampleKind::Probe { index: 0 })
    );
}

#[test]
fn stale_duplicate_wrong_plan_and_wrong_generation_samples_fail_closed() {
    let eligible = [PlanId::CpuPostSort, PlanId::GpuPostSort];
    let mut controller = controller(&eligible);
    let decision = controller.choose_adaptive().expect("bootstrap");
    let valid = sample_for(decision, 7, FRAME, 3.0);
    assert!(controller.register_pending(decision, valid.sample_ticket()));

    let wrong_plan_ticket = PlanSampleTicket::new(
        7,
        decision.probe_generation(),
        decision.comparison(),
        PlanId::GpuPostSort,
    );
    let wrong_plan = PlanSample::new(
        wrong_plan_ticket,
        FRAME,
        OrderLane::Gpu,
        7,
        None,
        None,
        None,
        PlanCountSemantics::IndirectDrawEqualsVisible,
        3.0,
    );
    assert_eq!(controller.observe(wrong_plan), SampleDisposition::Rejected);
    assert_eq!(controller.pending_for_test(), Some(valid.sample_ticket()));

    let wrong_generation_ticket = PlanSampleTicket::new(
        7,
        decision.probe_generation() + 1,
        decision.comparison(),
        decision.plan(),
    );
    let wrong_generation = PlanSample::new(
        wrong_generation_ticket,
        FRAME,
        OrderLane::Cpu,
        7,
        Some(97),
        None,
        Some(97),
        PlanCountSemantics::DirectDrawEqualsVisible,
        3.0,
    );
    assert_eq!(
        controller.observe(wrong_generation),
        SampleDisposition::Rejected
    );
    assert_eq!(controller.observe(valid), SampleDisposition::Accepted);
    assert_eq!(controller.observe(valid), SampleDisposition::Rejected);

    let next = controller.choose_adaptive().expect("next bootstrap");
    let old = sample_for(next, 8, FRAME, 3.0);
    assert!(controller.register_pending(next, old.sample_ticket()));
    let replacement_frame = FrameIdentity::new(4, 12, 6, 8, 10);
    assert!(controller.synchronize(
        PlanId::CpuPostSort,
        &eligible,
        comparison(replacement_frame),
    ));
    assert_eq!(controller.observe(old), SampleDisposition::Rejected);
}

#[test]
fn zero_nonfinite_and_negative_terminal_durations_fail_closed() {
    let eligible = [PlanId::CpuPostSort, PlanId::GpuPostSort];
    for duration in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        let mut controller = controller(&eligible);
        let decision = controller.choose_adaptive().expect("bootstrap");
        let sample = sample_for(decision, 17, FRAME, duration);
        assert!(controller.register_pending(decision, sample.sample_ticket()));
        assert_eq!(controller.observe(sample), SampleDisposition::Rejected);
        assert_eq!(controller.pending_for_test(), None);
        assert_eq!(controller.incumbent_for_test(), PlanId::CpuPostSort);
    }
}

#[test]
fn cpu_visible_count_above_source_count_fails_closed_and_releases_pending() {
    let eligible = [PlanId::CpuPostSort, PlanId::GpuPostSort];
    let mut controller = controller(&eligible);
    let decision = controller.choose_adaptive().expect("bootstrap");
    let ticket = PlanSampleTicket::new(
        19,
        decision.probe_generation(),
        decision.comparison(),
        decision.plan(),
    );
    let impossible = PlanSample::new(
        ticket,
        FRAME,
        OrderLane::Cpu,
        19,
        Some(130),
        None,
        Some(130),
        PlanCountSemantics::DirectDrawEqualsVisible,
        1.0,
    );
    assert!(controller.register_pending(decision, ticket));
    assert_eq!(controller.observe(impossible), SampleDisposition::Rejected);
    assert_eq!(controller.pending_for_test(), None);
    assert_eq!(controller.incumbent_for_test(), PlanId::CpuPostSort);
}

#[test]
fn optional_ring_pressure_cannot_change_controller_decisions() {
    let eligible = [PlanId::CpuPostSort, PlanId::GpuPostSort];
    let mut observed = controller(&eligible);
    let mut control = controller(&eligible);
    let mut ring = BoundedEvidenceRing::new();
    let mut ticket = 1;

    for _ in 0..80 {
        let left = observed.choose_adaptive().expect("observed choice");
        let right = control.choose_adaptive().expect("control choice");
        assert_eq!(left.plan(), right.plan());
        assert_eq!(left.formal_kind(), right.formal_kind());
        if left.formal_kind().is_some() {
            let ms = if left.plan() == PlanId::CpuPostSort {
                10.0
            } else {
                5.0
            };
            let sample = sample_for(left, ticket, FRAME, ms);
            assert!(observed.register_pending(left, sample.sample_ticket()));
            assert!(control.register_pending(right, sample.sample_ticket()));
            assert_eq!(observed.observe(sample), SampleDisposition::Accepted);
            assert_eq!(control.observe(sample), SampleDisposition::Accepted);
            // Optional observation happens strictly after mandatory policy.
            ring.push(sample);
            ticket += 1;
        } else {
            observed.submitted_without_sample(left);
            control.submitted_without_sample(right);
        }
    }
    assert_eq!(observed.incumbent_for_test(), control.incumbent_for_test());
    assert_eq!(ring.drain().count(), 64);
}

#[cfg(not(target_arch = "wasm32"))]
mod metal {
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };

    use gsplat_core::{Camera, SceneBuffers, Vec3f};

    use super::*;
    use crate::TimerInstant;
    use crate::plans::TestGpuAdmissionMode;
    use crate::renderer::sampler::{PlanSampleDescriptor, PlanSampler};
    use crate::scene::ResidentSceneCpu;

    const WIDTH: u32 = 64;
    const HEIGHT: u32 = 64;
    const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

    fn exact_scene(count: usize) -> ResidentSceneCpu {
        ResidentSceneCpu::encode_owned(SceneBuffers {
            positions: (0..count)
                .map(|index| {
                    Vec3f::new(
                        (index % 11) as f32 * 0.01 - 0.05,
                        ((index / 11) % 7) as f32 * 0.01 - 0.03,
                        1.0 + (index % 3) as f32 * 0.01,
                    )
                })
                .collect(),
            opacity: vec![0.0; count],
            scale_xyz: vec![[-3.0; 3]; count],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; count],
            color_dc: vec![[0.1, 0.05, 0.02]; count],
            sh_degree: 3,
            sh_rest: Some(vec![0.001; count * 45]),
        })
        .expect("Exact Resident scene")
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
            Err(error) => panic!("required E11 Metal adapter unavailable: {error}"),
            #[cfg(not(target_os = "macos"))]
            Err(error) => {
                eprintln!("skipping optional E11 GPU test: {error}");
                return None;
            }
        };
        let info = adapter.get_info();
        #[cfg(target_os = "macos")]
        assert_eq!(info.backend, wgpu::Backend::Metal, "Metal adapter required");
        let limits = portable_limits();
        if !limits.check_limits(&adapter.limits()) {
            #[cfg(target_os = "macos")]
            panic!("required E11 Metal limits unavailable: {limits:?}");
            #[cfg(not(target_os = "macos"))]
            return None;
        }
        let descriptor = wgpu::DeviceDescriptor {
            label: Some("exact-e11-controller-test-device"),
            required_features: wgpu::Features::empty(),
            required_limits: limits,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        };
        match adapter.request_device(&descriptor).await {
            Ok((device, queue)) => Some((info, Arc::new(device), Arc::new(queue))),
            #[cfg(target_os = "macos")]
            Err(error) => panic!("required E11 Metal device unavailable: {error}"),
            #[cfg(not(target_os = "macos"))]
            Err(error) => {
                eprintln!("skipping optional E11 GPU test: {error}");
                None
            }
        }
    }

    fn target(device: &wgpu::Device) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("exact-e11-target"),
            size: wgpu::Extent3d {
                width: WIDTH,
                height: HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        (texture, view)
    }

    #[test]
    fn poll_before_command_buffer_submission_is_nonblocking_and_incomplete() {
        pollster::block_on(async {
            let Some((_info, device, queue)) = request_device().await else {
                return;
            };
            let encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("exact-e11-unsubmitted-terminal-sample"),
            });
            let command_buffer = encoder.finish();
            let mut sampler = PlanSampler::new();
            let descriptor = PlanSampleDescriptor {
                probe_generation: 1,
                comparison: comparison(FRAME),
                frame: FRAME,
                plan: PlanId::CpuPostSort,
                order_lane: OrderLane::Cpu,
                order_generation: 1,
                visible_count: Some(129),
                contributor_count: None,
                draw_count: Some(129),
                count_semantics: PlanCountSemantics::DirectDrawEqualsVisible,
            };
            sampler
                .arm(&command_buffer, descriptor, TimerInstant::now())
                .expect("one pending sample");

            let poll_started = Instant::now();
            assert!(sampler.poll(&device).is_none());
            assert!(
                poll_started.elapsed() < Duration::from_millis(100),
                "PollType::Poll unexpectedly blocked before submission"
            );
            assert!(sampler.has_pending());

            let submission = queue.submit([command_buffer]);
            device
                .poll(wgpu::PollType::Wait {
                    submission_index: Some(submission),
                    timeout: None,
                })
                .expect("wait submitted empty command buffer");
            let sample = sampler.poll(&device).expect("terminal sample");
            assert!(sample.frame_complete_ms() > 0.0);
            assert!(!sampler.has_pending());
        });
    }

    #[test]
    fn runtime_replacement_expires_published_ticket_before_late_callback() {
        pollster::block_on(async {
            let Some((_info, device, queue)) = request_device().await else {
                return;
            };
            let mut slot =
                PreparedRuntimeSlot::prepare(exact_scene(129)).expect("first prepared runtime");
            slot.set_test_gpu_admission_mode(TestGpuAdmissionMode::ConcreteAll);
            slot.prepare_gpu(&device, &queue, FORMAT)
                .await
                .expect("first GPU plan set");
            slot.set_test_controller_config(ControllerConfig::accelerated());

            let (_first_texture, first_view) = target(&device);
            let first_pending = encode_frame_gpu(
                &mut slot,
                GpuFrameEncodeRequest::adaptive(
                    &Camera::default(),
                    Viewport::new(WIDTH, HEIGHT).expect("viewport"),
                    &first_view,
                    FORMAT,
                    wgpu::Color::BLACK,
                ),
            )
            .expect("first adaptive encode");
            let first_submission =
                submit_encoded_frame(&mut slot, first_pending).expect("first submit");
            let expired = first_submission
                .plan_sample_ticket()
                .expect("published first ticket");
            assert!(slot.sampler_pending_for_test());

            slot.replace(exact_scene(131))
                .expect("transactional runtime replacement");
            assert!(!slot.sampler_pending_for_test());
            device
                .poll(wgpu::PollType::Wait {
                    submission_index: Some(first_submission.submission_index().clone()),
                    timeout: None,
                })
                .expect("late old submission completion");
            assert!(slot.poll_test_plan_sampler().is_none());

            slot.set_test_gpu_admission_mode(TestGpuAdmissionMode::ConcreteAll);
            slot.prepare_gpu(&device, &queue, FORMAT)
                .await
                .expect("replacement GPU plan set");
            slot.set_test_controller_config(ControllerConfig::accelerated());
            let (_next_texture, next_view) = target(&device);
            let next_pending = encode_frame_gpu(
                &mut slot,
                GpuFrameEncodeRequest::adaptive(
                    &Camera::default(),
                    Viewport::new(WIDTH, HEIGHT).expect("viewport"),
                    &next_view,
                    FORMAT,
                    wgpu::Color::BLACK,
                ),
            )
            .expect("replacement adaptive encode");
            let next_submission =
                submit_encoded_frame(&mut slot, next_pending).expect("replacement submit");
            let current = next_submission
                .plan_sample_ticket()
                .expect("published replacement ticket");
            assert_ne!(expired.comparison(), current.comparison());
            device
                .poll(wgpu::PollType::Wait {
                    submission_index: Some(next_submission.submission_index().clone()),
                    timeout: None,
                })
                .expect("replacement completion");
            let (sample, disposition) = slot
                .poll_test_plan_sampler()
                .expect("replacement terminal sample");
            assert_eq!(sample.sample_ticket(), current);
            assert_eq!(disposition, SampleDisposition::Accepted);
            assert!(!slot.sampler_pending_for_test());
        });
    }

    #[test]
    fn mandatory_sampler_covers_all_complete_plans_on_metal() {
        pollster::block_on(async {
            let Some((info, device, queue)) = request_device().await else {
                return;
            };
            eprintln!(
                "EXACT_E11_CONTROLLER adapter={} backend={:?}",
                info.name, info.backend
            );
            let mut slot =
                PreparedRuntimeSlot::prepare(exact_scene(129)).expect("prepared CPU runtime");
            slot.set_test_gpu_admission_mode(TestGpuAdmissionMode::ConcreteAll);
            slot.prepare_gpu(&device, &queue, FORMAT)
                .await
                .expect("three Exact plans and raster");
            slot.set_test_controller_config(ControllerConfig::accelerated());
            slot.enable_test_plan_evidence();

            let mut sampled_plans = Vec::new();
            let mut delayed_first_sample = false;
            for frame_index in 0..40 {
                let (_texture, view) = target(&device);
                let mut camera = Camera::default();
                camera.pose.position.x = frame_index as f32 * 0.0001;
                let pending = encode_frame_gpu(
                    &mut slot,
                    GpuFrameEncodeRequest::adaptive(
                        &camera,
                        Viewport::new(WIDTH, HEIGHT).expect("viewport"),
                        &view,
                        FORMAT,
                        wgpu::Color::BLACK,
                    ),
                )
                .expect("adaptive complete-plan encode");
                if frame_index == 0 {
                    std::thread::sleep(Duration::from_millis(5));
                    delayed_first_sample = true;
                }
                let submission =
                    submit_encoded_frame(&mut slot, pending).expect("one exact submission");
                if submission.plan_sample_ticket().is_some() {
                    assert!(slot.sampler_pending_for_test());
                }
                device
                    .poll(wgpu::PollType::Wait {
                        submission_index: Some(submission.submission_index().clone()),
                        timeout: None,
                    })
                    .expect("wait exact E11 submission");
                if let Some((sample, disposition)) = slot.poll_test_plan_sampler() {
                    assert_eq!(disposition, SampleDisposition::Accepted);
                    assert!(sample.is_comparable());
                    if sampled_plans.is_empty() && delayed_first_sample {
                        assert!(sample.frame_complete_ms() >= 4.0);
                    }
                    sampled_plans.push(sample.plan_id());
                }
                if [
                    PlanId::CpuPostSort,
                    PlanId::GpuPostSort,
                    PlanId::GpuPreproject,
                ]
                .iter()
                .all(|plan| sampled_plans.contains(plan))
                {
                    break;
                }
            }

            assert!(sampled_plans.contains(&PlanId::CpuPostSort));
            assert!(sampled_plans.contains(&PlanId::GpuPostSort));
            assert!(sampled_plans.contains(&PlanId::GpuPreproject));
            let evidence = slot.drain_test_plan_evidence();
            assert_eq!(evidence.len(), sampled_plans.len());
            assert!(evidence.iter().all(|sample| sample.is_comparable()));
        });
    }
}
