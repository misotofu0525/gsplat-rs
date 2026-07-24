use std::sync::Arc;

use gsplat_core::{Camera, SceneBuffers, Vec3f};

use super::{
    CurrentStatsPoll, CurrentStatsRequest, CurrentStatsSubmission, CurrentStatsTerminal,
    CurrentStatsUnsampledReason, GpuFrameEncodeRequest, GpuFrameSubmission, PlanId,
    PreparedRuntimeSlot, SubmittedGpuFrame, abandon_submitted_frame, encode_frame_gpu,
    submit_encoded_frame, submit_encoded_frame_unpublished,
};
use crate::evidence::PlanCountSemantics;
use crate::plans::TestGpuAdmissionMode;
use crate::renderer::controller::{ControllerConfig, SampleDisposition};
use crate::renderer::frame::Viewport;
use crate::renderer::gpu_prepare::CurrentStatsCapabilityTestFailure;
use crate::scene::{ResidentGpuBytePlan, ResidentSceneCpu};
use crate::{
    SurfaceCurrentStatsCountSemantics, SurfaceCurrentStatsPlan, SurfaceCurrentStatsTerminal,
};

const WIDTH: u32 = 64;
const HEIGHT: u32 = 64;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

fn exact_scene(depths: &[f32]) -> ResidentSceneCpu {
    exact_scene_at(
        &depths
            .iter()
            .enumerate()
            .map(|(index, &depth)| {
                Vec3f::new(
                    (index % 3) as f32 * 0.015 - 0.015,
                    (index / 3) as f32 * 0.015,
                    depth,
                )
            })
            .collect::<Vec<_>>(),
    )
}

fn exact_scene_at(positions: &[Vec3f]) -> ResidentSceneCpu {
    let count = positions.len();
    ResidentSceneCpu::encode_owned(SceneBuffers {
        positions: positions.to_vec(),
        opacity: vec![1.0; count],
        scale_xyz: vec![[-1.0; 3]; count],
        rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; count],
        color_dc: vec![[0.1, 0.2, 0.3]; count],
        sh_degree: 0,
        sh_rest: None,
    })
    .expect("Exact current-stats fixture")
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
        Err(error) => panic!("required current-stats Metal adapter unavailable: {error}"),
        #[cfg(not(target_os = "macos"))]
        Err(error) => {
            eprintln!("skipping optional current-stats GPU test: {error}");
            return None;
        }
    };
    #[cfg(target_os = "macos")]
    assert_eq!(adapter.get_info().backend, wgpu::Backend::Metal);
    let limits = portable_limits();
    if !limits.check_limits(&adapter.limits()) {
        #[cfg(target_os = "macos")]
        panic!("required current-stats Metal limits unavailable: {limits:?}");
        #[cfg(not(target_os = "macos"))]
        return None;
    }
    let descriptor = wgpu::DeviceDescriptor {
        label: Some("exact-current-stats-test-device"),
        required_features: wgpu::Features::empty(),
        required_limits: limits,
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    };
    match adapter.request_device(&descriptor).await {
        Ok((device, queue)) => Some((Arc::new(device), Arc::new(queue))),
        #[cfg(target_os = "macos")]
        Err(error) => panic!("required current-stats Metal device unavailable: {error}"),
        #[cfg(not(target_os = "macos"))]
        Err(error) => {
            eprintln!("skipping optional current-stats GPU test: {error}");
            None
        }
    }
}

async fn prepared_slot(
    device: &Arc<wgpu::Device>,
    queue: &Arc<wgpu::Queue>,
    scene: ResidentSceneCpu,
) -> PreparedRuntimeSlot {
    let mut slot = PreparedRuntimeSlot::prepare(scene).expect("CPU fallback");
    slot.set_test_gpu_admission_mode(TestGpuAdmissionMode::ConcreteAll);
    slot.prepare_gpu(device, queue, FORMAT)
        .await
        .expect("all Exact plans and canonical raster");
    slot
}

fn target(device: &wgpu::Device) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("exact-current-stats-target"),
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
    })
}

fn encode(
    slot: &mut PreparedRuntimeSlot,
    device: &wgpu::Device,
    plan: PlanId,
) -> super::PendingGpuFrame {
    let texture = target(device);
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    encode_frame_gpu(
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
    .expect("encode Exact frame")
}

fn encode_adaptive(
    slot: &mut PreparedRuntimeSlot,
    device: &wgpu::Device,
) -> super::PendingGpuFrame {
    let texture = target(device);
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    encode_frame_gpu(
        slot,
        GpuFrameEncodeRequest::adaptive(
            &Camera::default(),
            Viewport::new(WIDTH, HEIGHT).expect("viewport"),
            &view,
            FORMAT,
            wgpu::Color::BLACK,
        ),
    )
    .expect("encode adaptive Exact frame")
}

fn render(
    slot: &mut PreparedRuntimeSlot,
    device: &wgpu::Device,
    plan: PlanId,
) -> GpuFrameSubmission {
    let pending = encode(slot, device, plan);
    submit_encoded_frame(slot, pending).expect("submit and finalize Exact frame")
}

fn wait(device: &wgpu::Device, submission: &GpuFrameSubmission) {
    device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(submission.submission_index().clone()),
            timeout: None,
        })
        .expect("wait for current-stats submission");
}

fn issued(submission: &GpuFrameSubmission) -> super::CurrentStatsSubmissionReceipt {
    submission
        .current_stats_submission()
        .receipt()
        .expect("requested frame issues current-stats ticket")
}

fn one_terminal(poll: CurrentStatsPoll) -> CurrentStatsTerminal {
    poll.terminal()
        .unwrap_or_else(|| panic!("expected one current-stats terminal, got {poll:?}"))
}

#[test]
fn no_request_encodes_no_count_copy_and_publishes_not_requested() {
    pollster::block_on(async {
        let Some((device, queue)) = request_device().await else {
            return;
        };
        let mut slot = prepared_slot(&device, &queue, exact_scene(&[1.0])).await;
        let plan = ResidentGpuBytePlan::for_count(1, 0).expect("resource accounting");
        let resource_ledger = (
            Some(plan.projected_contributor_scan_sums + plan.projected_contributor_scan_params),
            4 * 2 * std::mem::size_of::<u32>() as u64,
        );
        assert_eq!(slot.current_stats_copy_count_for_test(), 0);
        assert_eq!(
            slot.current_stats_resource_bytes_for_test(),
            resource_ledger
        );
        assert_eq!(slot.current_stats_live_object_count_for_test(), Some(10));
        assert_eq!(
            slot.current_stats_observer_activity_for_test(),
            (0, 0, 0, 0)
        );
        assert!(!slot.current_stats_request_pending_for_test());

        let submission = render(&mut slot, &device, PlanId::CpuPostSort);
        assert_eq!(
            submission.current_stats_submission(),
            CurrentStatsSubmission::NotRequested
        );
        assert_eq!(slot.current_stats_copy_count_for_test(), 0);
        assert_eq!(
            slot.current_stats_resource_bytes_for_test(),
            resource_ledger
        );
        assert_eq!(slot.current_stats_live_object_count_for_test(), Some(10));
        assert!(!slot.current_stats_request_pending_for_test());
        assert!(slot.poll_current_stats().is_empty());
        assert_eq!(
            slot.current_stats_observer_activity_for_test(),
            (0, 0, 0, 0)
        );

        assert_eq!(slot.request_current_stats(), CurrentStatsRequest::Requested);
        assert_eq!(
            slot.current_stats_resource_bytes_for_test(),
            resource_ledger
        );
        assert_eq!(slot.current_stats_live_object_count_for_test(), Some(10));
        assert_eq!(
            slot.current_stats_observer_activity_for_test(),
            (0, 0, 0, 0)
        );
    });
}

#[test]
fn every_plan_uses_actual_count_sources_for_boundary_and_distinct_v_c_scenes() {
    pollster::block_on(async {
        let Some((device, queue)) = request_device().await else {
            return;
        };
        for (scene, source_visible, contributor) in [
            (exact_scene(&[]), 0, 0),
            (exact_scene(&[1.0]), 1, 1),
            (exact_scene(&[1.0, 1.0, 1.0]), 3, 3),
            (exact_scene_at(&[Vec3f::new(100.0, 0.0, 1.0)]), 1, 0),
        ] {
            let mut slot = prepared_slot(&device, &queue, scene).await;
            for plan in [
                PlanId::CpuPostSort,
                PlanId::GpuPostSort,
                PlanId::GpuPreproject,
            ] {
                let copies_before = slot.current_stats_copy_count_for_test();
                assert_eq!(slot.request_current_stats(), CurrentStatsRequest::Requested);
                let submission = render(&mut slot, &device, plan);
                assert_eq!(
                    slot.current_stats_copy_count_for_test() - copies_before,
                    if plan == PlanId::CpuPostSort { 1 } else { 2 }
                );
                let issued = issued(&submission);
                wait(&device, &submission);
                let terminal = one_terminal(slot.poll_current_stats());
                let CurrentStatsTerminal::Ready(receipt) = terminal else {
                    panic!("expected Ready for {plan:?}, got {terminal:?}");
                };
                let SurfaceCurrentStatsTerminal::Ready(surface_receipt) = terminal.into() else {
                    panic!("public Surface DTO did not preserve Ready terminal");
                };
                assert_eq!(receipt.submission(), issued);
                assert!(receipt.submission().ticket().get() > 0);
                assert_eq!(receipt.submission().join().plan_id(), plan);
                assert_eq!(
                    receipt.submission().join().frame_identity(),
                    submission.frame_identity()
                );
                assert_eq!(
                    receipt.submission().join().raster_generation(),
                    submission.frame_identity().plan_set_generation()
                );
                assert_eq!(
                    receipt.submission().join().order_generation(),
                    submission.order_generation()
                );
                assert_eq!(
                    receipt.submission().join().encode_attempt(),
                    submission.encode_attempt()
                );
                assert_eq!(
                    receipt.submission().join().presentation_sequence(),
                    submission.presentation_sequence()
                );
                let counts = receipt.counts();
                assert_eq!(counts.source(), source_visible);
                assert_eq!(counts.visible(), source_visible);
                assert_eq!(counts.contributor(), contributor);
                assert_eq!(
                    counts.drawn(),
                    if plan == PlanId::GpuPreproject {
                        contributor
                    } else {
                        source_visible
                    }
                );
                assert_eq!(
                    receipt.count_semantics(),
                    match plan {
                        PlanId::CpuPostSort => PlanCountSemantics::DirectDrawEqualsVisible,
                        PlanId::GpuPostSort => PlanCountSemantics::IndirectDrawEqualsVisible,
                        PlanId::GpuPreproject => {
                            PlanCountSemantics::IndirectDrawEqualsContributor
                        }
                    }
                );
                let surface_submission = surface_receipt.submission();
                let surface_join = surface_submission.join();
                let surface_frame = surface_join.frame_identity();
                assert_eq!(surface_submission.ticket(), issued.ticket().get());
                assert_eq!(
                    surface_frame.scene_generation(),
                    submission.frame_identity().scene_generation()
                );
                assert_eq!(
                    surface_frame.camera_revision(),
                    submission.frame_identity().camera_revision()
                );
                assert_eq!(
                    surface_frame.viewport_generation(),
                    submission.frame_identity().viewport_generation()
                );
                assert_eq!(
                    surface_frame.contract_generation(),
                    submission.frame_identity().contract_generation()
                );
                assert_eq!(
                    surface_frame.plan_set_generation(),
                    submission.frame_identity().plan_set_generation()
                );
                assert_eq!(
                    surface_join.executed_plan(),
                    match plan {
                        PlanId::CpuPostSort => SurfaceCurrentStatsPlan::CpuPostSort,
                        PlanId::GpuPostSort => SurfaceCurrentStatsPlan::GpuPostSort,
                        PlanId::GpuPreproject => SurfaceCurrentStatsPlan::GpuPreproject,
                    }
                );
                assert_eq!(
                    surface_join.order_generation(),
                    submission.order_generation()
                );
                assert_eq!(
                    surface_join.raster_generation(),
                    submission.frame_identity().plan_set_generation()
                );
                assert_eq!(surface_join.encode_attempt(), submission.encode_attempt());
                assert_eq!(
                    surface_join.presentation_sequence(),
                    submission.presentation_sequence()
                );
                assert_eq!(surface_receipt.counts().source(), counts.source());
                assert_eq!(surface_receipt.counts().visible(), counts.visible());
                assert_eq!(surface_receipt.counts().contributor(), counts.contributor());
                assert_eq!(surface_receipt.counts().drawn(), counts.drawn());
                assert_eq!(
                    surface_receipt.count_semantics(),
                    match receipt.count_semantics() {
                        PlanCountSemantics::DirectDrawEqualsVisible => {
                            SurfaceCurrentStatsCountSemantics::DirectDrawEqualsVisible
                        }
                        PlanCountSemantics::IndirectDrawEqualsVisible => {
                            SurfaceCurrentStatsCountSemantics::IndirectDrawEqualsVisible
                        }
                        PlanCountSemantics::IndirectDrawEqualsContributor => {
                            SurfaceCurrentStatsCountSemantics::IndirectDrawEqualsContributor
                        }
                    }
                );
            }
        }
    });
}

#[test]
fn observer_ring_busy_does_not_block_formal_sampler_or_adaptive_progress() {
    pollster::block_on(async {
        let Some((device, queue)) = request_device().await else {
            return;
        };
        let mut slot = prepared_slot(&device, &queue, exact_scene(&[1.0])).await;
        let mut submissions = Vec::new();
        for _ in 0..4 {
            assert_eq!(slot.request_current_stats(), CurrentStatsRequest::Requested);
            let submission = render(&mut slot, &device, PlanId::GpuPostSort);
            assert_eq!(submission.plan_id(), PlanId::GpuPostSort);
            assert!(!slot.sampler_pending_for_test());
            submissions.push(submission);
        }
        wait(&device, submissions.last().expect("submitted samples"));
        assert_eq!(
            slot.request_current_stats(),
            CurrentStatsRequest::Unsampled(CurrentStatsUnsampledReason::Busy)
        );
        assert!(!slot.sampler_pending_for_test());
        assert!(!slot.current_stats_request_pending_for_test());

        // The observer callbacks are now ready but deliberately unpolled. A
        // real Adaptive controller must still issue and consume its formal
        // tickets, then advance from bootstrap through the first challenger
        // transition while observer capacity remains Busy.
        slot.set_test_controller_config(ControllerConfig::accelerated());
        let bootstrap_pending = encode_adaptive(&mut slot, &device);
        let bootstrap =
            submit_encoded_frame(&mut slot, bootstrap_pending).expect("submit adaptive bootstrap");
        let bootstrap_ticket = bootstrap
            .plan_sample_ticket()
            .expect("formal bootstrap ticket under observer pressure");
        assert_eq!(bootstrap.plan_id(), PlanId::CpuPostSort);
        assert!(slot.sampler_pending_for_test());
        assert_eq!(
            bootstrap.current_stats_submission(),
            CurrentStatsSubmission::NotRequested
        );
        wait(&device, &bootstrap);
        let (sample, disposition) = slot
            .poll_test_plan_sampler()
            .expect("formal bootstrap completion");
        assert_eq!(sample.sample_ticket(), bootstrap_ticket);
        assert_eq!(disposition, SampleDisposition::Accepted);
        assert_eq!(
            slot.request_current_stats(),
            CurrentStatsRequest::Unsampled(CurrentStatsUnsampledReason::Busy)
        );

        let incumbent_pending = encode_adaptive(&mut slot, &device);
        let incumbent = submit_encoded_frame(&mut slot, incumbent_pending)
            .expect("submit first incumbent probe");
        let incumbent_ticket = incumbent
            .plan_sample_ticket()
            .expect("formal incumbent ticket under observer pressure");
        assert_eq!(incumbent.plan_id(), PlanId::CpuPostSort);
        assert!(incumbent_ticket.probe_generation() > bootstrap_ticket.probe_generation());
        wait(&device, &incumbent);
        assert_eq!(
            slot.poll_test_plan_sampler()
                .expect("formal incumbent completion")
                .1,
            SampleDisposition::Accepted
        );

        let challenger_pending = encode_adaptive(&mut slot, &device);
        let challenger = submit_encoded_frame(&mut slot, challenger_pending)
            .expect("submit challenger transition");
        assert_eq!(challenger.plan_id(), PlanId::GpuPostSort);
        assert!(challenger.plan_sample_ticket().is_some());
        wait(&device, &challenger);
        assert_eq!(
            slot.poll_test_plan_sampler()
                .expect("formal challenger completion")
                .1,
            SampleDisposition::Accepted
        );

        let expected_tickets = submissions
            .iter()
            .map(|submission| issued(submission).ticket())
            .collect::<Vec<_>>();
        let terminals = (0..4)
            .map(|_| one_terminal(slot.poll_current_stats()))
            .collect::<Vec<_>>();
        assert_eq!(
            terminals
                .iter()
                .map(|terminal| terminal.ticket())
                .collect::<Vec<_>>(),
            expected_tickets
        );
        assert!(
            terminals
                .windows(2)
                .all(|pair| pair[0].ticket() < pair[1].ticket())
        );
        assert!(
            terminals
                .windows(2)
                .all(|pair| { pair[0].submission().join() != pair[1].submission().join() })
        );
        assert!(slot.poll_current_stats().is_empty());
    });
}

#[test]
fn pending_observer_gets_one_turn_then_formal_waits_for_a_known_safe_queue_entry() {
    pollster::block_on(async {
        let Some((device, queue)) = request_device().await else {
            return;
        };
        let mut slot = prepared_slot(&device, &queue, exact_scene(&[1.0])).await;
        slot.set_test_controller_config(ControllerConfig::accelerated());
        assert_eq!(slot.request_current_stats(), CurrentStatsRequest::Requested);
        let copies_before = slot.current_stats_copy_count_for_test();

        // The controller asked for a formal bootstrap, but the pending
        // observer receives exactly one bounded turn first.
        let observer_pending = encode_adaptive(&mut slot, &device);
        let observer = submit_encoded_frame(&mut slot, observer_pending)
            .expect("observer fairness submission");
        assert!(observer.plan_sample_ticket().is_none());
        assert!(matches!(
            observer.current_stats_submission(),
            CurrentStatsSubmission::Issued(_)
        ));
        assert_eq!(slot.current_stats_copy_count_for_test() - copies_before, 1);
        assert!(!slot.current_stats_request_pending_for_test());
        assert_eq!(slot.request_current_stats(), CurrentStatsRequest::Requested);

        // The next frame began with observer work still queued. Its
        // non-blocking progress poll may complete that work, but the formal
        // sample is still suppressed for this frame.
        let boundary_pending = encode_adaptive(&mut slot, &device);
        let boundary =
            submit_encoded_frame(&mut slot, boundary_pending).expect("queue-boundary submission");
        assert!(boundary.plan_sample_ticket().is_none());
        assert_eq!(
            boundary.current_stats_submission(),
            CurrentStatsSubmission::NotRequested
        );
        assert!(slot.current_stats_request_pending_for_test());
        wait(&device, &boundary);

        // Only a later frame that was already queue-safe at entry may start
        // the controller's completion interval.
        let formal_pending = encode_adaptive(&mut slot, &device);
        let formal = submit_encoded_frame(&mut slot, formal_pending).expect("formal submission");
        assert!(formal.plan_sample_ticket().is_some());
        assert_eq!(
            formal.current_stats_submission(),
            CurrentStatsSubmission::NotRequested
        );
        assert!(slot.current_stats_request_pending_for_test());
        wait(&device, &formal);
        assert_eq!(
            slot.poll_test_plan_sampler()
                .expect("formal controller completion")
                .1,
            SampleDisposition::Accepted
        );

        // The queued second request did not starve the formal sample, and now
        // receives the next bounded turn without contributing values to it.
        let second_observer_pending = encode_adaptive(&mut slot, &device);
        let second_observer = submit_encoded_frame(&mut slot, second_observer_pending)
            .expect("second observer fairness submission");
        assert!(second_observer.plan_sample_ticket().is_none());
        assert!(matches!(
            second_observer.current_stats_submission(),
            CurrentStatsSubmission::Issued(_)
        ));
        assert!(!slot.current_stats_request_pending_for_test());
        wait(&device, &second_observer);
        let first = one_terminal(slot.poll_current_stats());
        let second = one_terminal(slot.poll_current_stats());
        assert!(first.ticket() < second.ticket());
        assert_ne!(first.submission().join(), second.submission().join());
        assert!(slot.poll_current_stats().is_empty());
    });
}

#[test]
fn abandoned_submission_keeps_request_for_next_presented_frame_without_publishing_ticket() {
    pollster::block_on(async {
        let Some((device, queue)) = request_device().await else {
            return;
        };
        let mut slot = prepared_slot(&device, &queue, exact_scene(&[1.0])).await;
        assert_eq!(slot.request_current_stats(), CurrentStatsRequest::Requested);
        let pending = encode(&mut slot, &device, PlanId::CpuPostSort);
        let mut submitted = submit_encoded_frame_unpublished(&mut slot, pending)
            .expect("submit unpublished target");
        assert!(slot.current_stats_request_pending_for_test());
        assert!(abandon_submitted_frame(&mut submitted));
        assert!(!abandon_submitted_frame(&mut submitted));
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        assert!(slot.poll_current_stats().is_empty());
        assert!(slot.current_stats_request_pending_for_test());

        let retry = render(&mut slot, &device, PlanId::CpuPostSort);
        assert!(matches!(
            retry.current_stats_submission(),
            CurrentStatsSubmission::Issued(_)
        ));
        assert!(!slot.current_stats_request_pending_for_test());
    });
}

#[derive(Clone, Copy)]
enum PendingReplacementPoint {
    Reserved,
    Encoded,
    Submitted,
}

#[test]
fn pending_request_intent_transfers_across_runtime_replacement_at_every_unpublished_stage() {
    pollster::block_on(async {
        let Some((device, queue)) = request_device().await else {
            return;
        };
        for point in [
            PendingReplacementPoint::Reserved,
            PendingReplacementPoint::Encoded,
            PendingReplacementPoint::Submitted,
        ] {
            let mut slot = prepared_slot(&device, &queue, exact_scene(&[1.0])).await;
            assert_eq!(slot.request_current_stats(), CurrentStatsRequest::Requested);
            let mut submitted: Option<SubmittedGpuFrame> = None;
            let pending = match point {
                PendingReplacementPoint::Reserved => None,
                PendingReplacementPoint::Encoded | PendingReplacementPoint::Submitted => {
                    Some(encode(&mut slot, &device, PlanId::CpuPostSort))
                }
            };
            if matches!(point, PendingReplacementPoint::Submitted) {
                submitted = Some(
                    submit_encoded_frame_unpublished(
                        &mut slot,
                        pending.expect("encoded pending frame"),
                    )
                    .expect("submit unpublished target"),
                );
            } else {
                drop(pending);
            }

            slot.replace(exact_scene(&[1.0, 1.0]))
                .expect("replace runtime with pending observer request");
            if let Some(mut submitted) = submitted {
                assert!(abandon_submitted_frame(&mut submitted));
            }
            assert!(slot.current_stats_request_pending_for_test());
            assert_eq!(slot.current_stats_resource_bytes_for_test(), (None, 0));
            slot.set_test_gpu_admission_mode(TestGpuAdmissionMode::ConcreteAll);
            slot.prepare_gpu(&device, &queue, FORMAT)
                .await
                .expect("prepare replacement GPU runtime");
            let plan = ResidentGpuBytePlan::for_count(2, 0).expect("replacement accounting");
            assert_eq!(
                slot.current_stats_resource_bytes_for_test(),
                (
                    Some(
                        plan.projected_contributor_scan_sums
                            + plan.projected_contributor_scan_params
                    ),
                    4 * 2 * std::mem::size_of::<u32>() as u64,
                )
            );
            let replacement = render(&mut slot, &device, PlanId::CpuPostSort);
            assert!(matches!(
                replacement.current_stats_submission(),
                CurrentStatsSubmission::Issued(_)
            ));
            assert!(!slot.current_stats_request_pending_for_test());
            assert_eq!(slot.current_stats_live_object_count_for_test(), Some(10));
        }
    });
}

async fn assert_optional_capability_failure_does_not_gate_product(
    device: &Arc<wgpu::Device>,
    queue: &Arc<wgpu::Queue>,
    failure: CurrentStatsCapabilityTestFailure,
) {
    let mut slot = prepared_slot(device, queue, exact_scene(&[1.0])).await;
    assert_eq!(slot.request_current_stats(), CurrentStatsRequest::Requested);
    slot.replace(exact_scene(&[1.0, 1.0]))
        .expect("replace with transferred request");
    let frame_before = slot.frame_state();
    let fallback_before = slot.fallback();

    slot.set_test_gpu_admission_mode(TestGpuAdmissionMode::ConcreteAll);
    slot.set_current_stats_capability_failure_for_test(failure);
    slot.prepare_gpu(device, queue, FORMAT)
        .await
        .expect("observer failure cannot reject product GPU admission");
    assert_ne!(slot.frame_state(), frame_before);
    assert_eq!(slot.fallback(), fallback_before);
    assert!(slot.gpu_preparation().is_some());
    assert!(!slot.current_stats_request_pending_for_test());
    assert_eq!(slot.current_stats_resource_bytes_for_test(), (None, 0));
    assert_eq!(slot.current_stats_live_object_count_for_test(), None);
    if failure == CurrentStatsCapabilityTestFailure::ScopedInvalidBuffer {
        assert!(device.limits().max_buffer_size.checked_add(1).is_some());
        assert!(matches!(
            slot.current_stats_capability_last_error_for_test(),
            Some(
                super::GpuPreparationError::Validation(_)
                    | super::GpuPreparationError::OutOfMemory(_)
                    | super::GpuPreparationError::Internal(_)
            )
        ));
    }
    let poll = slot.poll_current_stats();
    assert_eq!(
        poll.unsampled(),
        Some(CurrentStatsUnsampledReason::ResourceUnavailable)
    );
    assert!(poll.terminal().is_none());
    assert!(slot.poll_current_stats().is_empty());
    assert_eq!(
        slot.request_current_stats(),
        CurrentStatsRequest::Unsampled(CurrentStatsUnsampledReason::ResourceUnavailable)
    );

    let submission = render(&mut slot, device, PlanId::CpuPostSort);
    assert_eq!(
        submission.current_stats_submission(),
        CurrentStatsSubmission::NotRequested
    );
    assert_eq!(submission.plan_id(), PlanId::CpuPostSort);
}

#[test]
fn scan_candidate_failure_is_optional_and_resolves_transferred_request() {
    pollster::block_on(async {
        let Some((device, queue)) = request_device().await else {
            return;
        };
        assert_optional_capability_failure_does_not_gate_product(
            &device,
            &queue,
            CurrentStatsCapabilityTestFailure::AfterScanCandidate,
        )
        .await;
    });
}

#[test]
fn readback_candidate_failure_is_optional_and_resolves_transferred_request() {
    pollster::block_on(async {
        let Some((device, queue)) = request_device().await else {
            return;
        };
        assert_optional_capability_failure_does_not_gate_product(
            &device,
            &queue,
            CurrentStatsCapabilityTestFailure::AfterReadbackCandidate,
        )
        .await;
    });
}

#[test]
fn real_scoped_wgpu_failure_is_structured_optional_and_non_panicking() {
    pollster::block_on(async {
        let Some((device, queue)) = request_device().await else {
            return;
        };
        assert_optional_capability_failure_does_not_gate_product(
            &device,
            &queue,
            CurrentStatsCapabilityTestFailure::ScopedInvalidBuffer,
        )
        .await;
    });
}

#[test]
fn later_product_failure_cannot_half_publish_observer_capability() {
    pollster::block_on(async {
        let Some((device, queue)) = request_device().await else {
            return;
        };
        let mut slot = PreparedRuntimeSlot::prepare(exact_scene(&[1.0]))
            .expect("CPU fallback before product admission");
        slot.set_test_gpu_admission_mode(TestGpuAdmissionMode::Fail);
        assert!(slot.prepare_gpu(&device, &queue, FORMAT).await.is_err());
        assert!(slot.gpu_preparation().is_none());
        assert_eq!(slot.current_stats_resource_bytes_for_test(), (None, 0));
        assert_eq!(slot.current_stats_live_object_count_for_test(), None);
        assert_eq!(
            slot.request_current_stats(),
            CurrentStatsRequest::Unsampled(CurrentStatsUnsampledReason::GpuUnavailable)
        );

        slot.set_test_gpu_admission_mode(TestGpuAdmissionMode::ConcreteAll);
        slot.prepare_gpu(&device, &queue, FORMAT)
            .await
            .expect("clean retry publishes product plus complete observer capability");
        assert!(slot.gpu_preparation().is_some());
        assert_eq!(slot.current_stats_live_object_count_for_test(), Some(10));
        assert_eq!(slot.request_current_stats(), CurrentStatsRequest::Requested);
    });
}

#[test]
fn issued_ticket_is_generation_invalidated_once_and_late_callback_cannot_enter_replacement() {
    pollster::block_on(async {
        let Some((device, queue)) = request_device().await else {
            return;
        };
        let mut slot = prepared_slot(&device, &queue, exact_scene(&[1.0])).await;
        assert_eq!(slot.request_current_stats(), CurrentStatsRequest::Requested);
        let submission = render(&mut slot, &device, PlanId::GpuPostSort);
        let issued = issued(&submission);
        slot.replace(exact_scene(&[1.0, 1.0]))
            .expect("replace issued-ticket runtime");

        let terminal = one_terminal(slot.poll_current_stats());
        let CurrentStatsTerminal::GenerationInvalidated(failure) = terminal else {
            panic!("expected generation invalidation, got {terminal:?}");
        };
        assert_eq!(failure.submission(), issued);
        assert!(slot.poll_current_stats().is_empty());
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        assert!(slot.poll_current_stats().is_empty());
    });
}

#[test]
fn map_failure_expiry_and_terminal_uniqueness_are_ticket_bound() {
    pollster::block_on(async {
        let Some((device, queue)) = request_device().await else {
            return;
        };
        let mut slot = prepared_slot(&device, &queue, exact_scene(&[1.0])).await;

        assert_eq!(slot.request_current_stats(), CurrentStatsRequest::Requested);
        let failed = render(&mut slot, &device, PlanId::GpuPreproject);
        let failed_ticket = issued(&failed).ticket();
        wait(&device, &failed);
        assert!(slot.force_current_stats_map_failure_for_test(failed_ticket));
        let terminal = one_terminal(slot.poll_current_stats());
        assert!(matches!(terminal, CurrentStatsTerminal::MapFailure(_)));
        assert_eq!(terminal.ticket(), failed_ticket);
        assert!(slot.poll_current_stats().is_empty());
        assert!(!slot.expire_current_stats(failed_ticket));

        assert_eq!(slot.request_current_stats(), CurrentStatsRequest::Requested);
        let expiring = render(&mut slot, &device, PlanId::CpuPostSort);
        let expiring_ticket = issued(&expiring).ticket();
        assert_ne!(failed_ticket, expiring_ticket);
        assert!(slot.expire_current_stats(expiring_ticket));
        let terminal = one_terminal(slot.poll_current_stats());
        assert!(matches!(terminal, CurrentStatsTerminal::Expired(_)));
        assert_eq!(terminal.ticket(), expiring_ticket);
        assert!(slot.poll_current_stats().is_empty());
        assert!(!slot.expire_current_stats(expiring_ticket));
    });
}

#[test]
fn out_of_order_terminal_and_late_callback_cannot_duplicate_a_ticket() {
    pollster::block_on(async {
        let Some((device, queue)) = request_device().await else {
            return;
        };
        let mut slot = prepared_slot(&device, &queue, exact_scene(&[1.0])).await;

        assert_eq!(slot.request_current_stats(), CurrentStatsRequest::Requested);
        let first = render(&mut slot, &device, PlanId::CpuPostSort);
        let first_ticket = issued(&first).ticket();
        assert_eq!(slot.request_current_stats(), CurrentStatsRequest::Requested);
        let second = render(&mut slot, &device, PlanId::GpuPostSort);
        let second_ticket = issued(&second).ticket();
        assert!(second_ticket > first_ticket);

        // Callback state is only an atomic terminal flag. Inject the later
        // ticket's MapFailure first and consume it without device polling.
        assert!(slot.hold_current_stats_callback_for_test(first_ticket));
        assert!(slot.force_current_stats_map_failure_for_test(second_ticket));
        let terminal = one_terminal(slot.poll_current_stats_without_device_for_test());
        assert!(matches!(terminal, CurrentStatsTerminal::MapFailure(_)));
        assert_eq!(terminal.ticket(), second_ticket);

        assert!(slot.force_current_stats_map_failure_for_test(first_ticket));
        let terminal = one_terminal(slot.poll_current_stats_without_device_for_test());
        assert!(matches!(terminal, CurrentStatsTerminal::MapFailure(_)));
        assert_eq!(terminal.ticket(), first_ticket);

        // Waiting either submission may deliver its real cancelled-map
        // callback after the synthetic ordering, but those old callbacks
        // cannot publish a second terminal for either ticket.
        wait(&device, &first);
        wait(&device, &second);
        assert!(slot.poll_current_stats().is_empty());
    });
}
