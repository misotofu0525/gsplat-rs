//! Strict real-window Surface evidence for the M2b migration boundary.
//!
//! This host owns trace playback, receipt translation and final image I/O.
//! Renderer policy, ticket allocation, count readback and terminal publication
//! remain in `SurfaceRenderSession` and its Renderer-owned Exact runtime.

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use gsplat_render_wgpu::{
    SurfaceCurrentStatsCountSemantics, SurfaceCurrentStatsPlan, SurfaceCurrentStatsPoll,
    SurfaceCurrentStatsReceipt, SurfaceCurrentStatsRequest, SurfaceCurrentStatsSubmission,
    SurfaceCurrentStatsSubmissionReceipt, SurfaceCurrentStatsTerminal, SurfaceFrameCapture,
    SurfaceFrameOutput, SurfaceGpuOrderProducer, SurfaceOrderBackend, SurfaceOrderBackendUsed,
    SurfaceProjectedDrawExecution, SurfaceRasterExecutionPlan, SurfaceRenderSession,
};
use winit::{
    event::{Event, WindowEvent},
    event_loop::EventLoop,
};

use crate::{
    cli::{Args, SurfaceEvidencePlanArg},
    image_output::write_png,
    trace::CameraTracePlayback,
    viewer::{SurfaceResolutionReceipt, SurfaceTraceStep, surface_trace_steps},
};

const RECEIPT_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_INELIGIBLE_RETRIES: usize = 64;

pub(crate) fn configure(
    session: &mut SurfaceRenderSession,
    plan: SurfaceEvidencePlanArg,
) -> Result<(), String> {
    session
        .set_raster_execution_plan(SurfaceRasterExecutionPlan::ProjectedQuadsExact)
        .map_err(|error| error.to_string())?;
    match plan {
        SurfaceEvidencePlanArg::CpuPostSort => session
            .set_order_backend(SurfaceOrderBackend::Cpu)
            .map_err(|error| error.to_string()),
        SurfaceEvidencePlanArg::GpuPostSort => session
            .set_order_backend(SurfaceOrderBackend::Gpu)
            .map_err(|error| error.to_string()),
        SurfaceEvidencePlanArg::GpuPreproject => {
            pollster::block_on(
                session.prepare_gpu_order_producer(SurfaceGpuOrderProducer::Preproject),
            )
            .map_err(|error| error.to_string())?;
            session
                .set_order_backend(SurfaceOrderBackend::Gpu)
                .map_err(|error| error.to_string())?;
            session
                .set_gpu_order_producer(SurfaceGpuOrderProducer::Preproject)
                .map_err(|error| error.to_string())
        }
        SurfaceEvidencePlanArg::Adaptive => session
            .set_order_backend(SurfaceOrderBackend::Adaptive)
            .map_err(|error| error.to_string()),
    }
}

#[derive(Debug, Clone)]
struct EvidenceIdentity {
    requested_plan: SurfaceEvidencePlanArg,
    trace_id: String,
    trace_sha256: String,
    resolution: SurfaceResolutionReceipt,
    source_count: usize,
    resident_count: usize,
    sh_degree: u8,
}

#[derive(Debug)]
enum PendingKind {
    Frame(SurfaceTraceStep),
    Capture {
        step: SurfaceTraceStep,
        path: PathBuf,
        capture: SurfaceFrameCapture,
    },
}

#[derive(Debug)]
struct PendingReceipt {
    kind: PendingKind,
    output: SurfaceFrameOutput,
    submission: SurfaceCurrentStatsSubmissionReceipt,
    call_ms: f32,
    deadline: Instant,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum TerminalOutcome {
    #[default]
    Running,
    CaptureComplete,
    Committed,
}

impl TerminalOutcome {
    fn mark_capture_complete(&mut self) {
        debug_assert_eq!(*self, Self::Running);
        *self = Self::CaptureComplete;
    }

    fn commit_if_ready(&mut self) -> bool {
        if *self != Self::CaptureComplete {
            return false;
        }
        *self = Self::Committed;
        true
    }

    const fn is_committed(self) -> bool {
        matches!(self, Self::Committed)
    }
}

#[allow(deprecated)] // Kept aligned with the existing winit Surface loop.
pub(crate) fn run(
    args: &Args,
    event_loop: EventLoop<()>,
    window: Arc<winit::window::Window>,
    mut session: SurfaceRenderSession,
    playback: &CameraTracePlayback,
    adapter_info: wgpu::AdapterInfo,
) -> Result<(), String> {
    let requested_plan = args
        .surface_evidence_plan
        .ok_or_else(|| "surface evidence plan is missing".to_owned())?;
    let capture_path = args
        .png_out
        .clone()
        .ok_or_else(|| "surface evidence capture path is missing".to_owned())?;
    let steps = surface_trace_steps(playback)?;
    let capture_step = steps
        .last()
        .copied()
        .ok_or_else(|| "surface evidence trace schedule is empty".to_owned())?;
    let trace = playback.trace();
    let source_count = session
        .renderer()
        .scene_len()
        .ok_or_else(|| "surface evidence scene is not loaded".to_owned())?;
    let resident_count = session
        .renderer()
        .resident_scene()
        .map_or(source_count, |scene| scene.len());
    if source_count != resident_count {
        return Err(format!(
            "surface evidence membership mismatch: source={source_count}, resident={resident_count}"
        ));
    }
    let requested_size = (trace.display.width, trace.display.height);
    let resolution = SurfaceResolutionReceipt::validated(
        requested_size,
        session.surface_size(),
        session.internal_render_size(),
    )?;
    let identity = EvidenceIdentity {
        requested_plan,
        trace_id: trace.trace_id.clone(),
        trace_sha256: trace.content_sha256.clone(),
        resolution,
        source_count,
        resident_count,
        sh_degree: session.renderer().scene_sh_degree().unwrap_or(0),
    };
    if identity.sh_degree != 3 {
        return Err(format!(
            "formal surface evidence requires source SH3, got SH{}",
            identity.sh_degree
        ));
    }
    if session.raster_execution_plan() != SurfaceRasterExecutionPlan::ProjectedQuadsExact {
        return Err("surface evidence did not select the canonical Exact raster".to_owned());
    }

    println!(
        "SURFACE_EXACT_EVIDENCE_BEGIN trace_id={} trace_sha256={} exact_plan_requested={} geometry_path=packed_atlas raster_execution_plan=projected_quads_exact blend_mode=sorted_alpha source_membership=all sampling=disabled lod=disabled adapter_backend={} adapter_name={:?} adapter_device_type={} adapter_driver={:?} adapter_driver_info={:?} source_count={} decoded_count={} encoded_count={} resident_count={} addressable_count={} sh_degree={} requested_width={} requested_height={} surface_width={} surface_height={} internal_render_width={} internal_render_height={} dynamic_resolution=disabled upscaling=disabled full_resolution={} trace_frames={}",
        identity.trace_id,
        identity.trace_sha256,
        identity.requested_plan.label(),
        adapter_backend_label(adapter_info.backend),
        adapter_info.name,
        adapter_device_type_label(adapter_info.device_type),
        nonempty_adapter_field(&adapter_info.driver),
        nonempty_adapter_field(&adapter_info.driver_info),
        identity.source_count,
        identity.source_count,
        identity.source_count,
        identity.resident_count,
        identity.resident_count,
        identity.sh_degree,
        identity.resolution.requested.0,
        identity.resolution.requested.1,
        identity.resolution.surface.0,
        identity.resolution.surface.1,
        identity.resolution.internal_render.0,
        identity.resolution.internal_render.1,
        identity.resolution.full_resolution(),
        steps.len(),
    );

    let window_id = window.id();
    let error = Arc::new(Mutex::new(None::<String>));
    let shared_error = Arc::clone(&error);
    let completed = Arc::new(AtomicBool::new(false));
    let shared_completed = Arc::clone(&completed);
    let started = Instant::now();
    let mut next_step = 0_usize;
    let mut pending = None::<PendingReceipt>;
    let mut request_pending = false;
    let mut ineligible_retries = 0_usize;
    let mut terminal_outcome = TerminalOutcome::default();
    let mut actual_plans = BTreeSet::new();
    let mut measured_frames = 0_usize;
    let mut capture_retries = 0_usize;

    event_loop
        .run(move |event, target| match event {
            _ if terminal_outcome.is_committed() => {}
            Event::AboutToWait => window.request_redraw(),
            Event::WindowEvent {
                window_id: id,
                event: WindowEvent::CloseRequested,
            } if id == window_id => {
                store_error(
                    &shared_error,
                    "surface evidence window closed before canonical publication".to_owned(),
                );
                target.exit();
            }
            Event::WindowEvent {
                window_id: id,
                event: WindowEvent::Resized(size),
            } if id == window_id
                && (size.width, size.height) != identity.resolution.requested =>
            {
                store_error(
                    &shared_error,
                    format!(
                        "surface evidence resize {}x{} violates {}x{} trace",
                        size.width,
                        size.height,
                        identity.resolution.requested.0,
                        identity.resolution.requested.1
                    ),
                );
                target.exit();
            }
            Event::WindowEvent {
                window_id: id,
                event: WindowEvent::RedrawRequested,
            } if id == window_id => {
                if let Some(waiting) = pending.take() {
                    match session.poll_current_stats() {
                        SurfaceCurrentStatsPoll::Empty => {
                            if Instant::now() >= waiting.deadline {
                                store_error(
                                    &shared_error,
                                    format!(
                                        "current-stats ticket {} did not resolve within {:?}",
                                        waiting.submission.ticket(),
                                        RECEIPT_TIMEOUT
                                    ),
                                );
                                target.exit();
                            } else {
                                pending = Some(waiting);
                                std::thread::sleep(Duration::from_millis(1));
                            }
                            return;
                        }
                        SurfaceCurrentStatsPoll::Unsampled(reason) => {
                            store_error(
                                &shared_error,
                                format!(
                                    "issued current-stats ticket {} resolved as unsampled: {reason:?}",
                                    waiting.submission.ticket()
                                ),
                            );
                            target.exit();
                            return;
                        }
                        SurfaceCurrentStatsPoll::Terminal(terminal) => {
                            let receipt = match validate_terminal(
                                &identity,
                                &waiting,
                                terminal,
                                session.last_presented_size(),
                            ) {
                                Ok(receipt) => receipt,
                                Err(message) => {
                                    store_error(&shared_error, message);
                                    target.exit();
                                    return;
                                }
                            };
                            actual_plans.insert(plan_label(receipt.submission().join().executed_plan()));
                            match waiting.kind {
                                PendingKind::Frame(step) => {
                                    print_frame(
                                        &identity,
                                        step,
                                        waiting.output,
                                        waiting.call_ms,
                                        started.elapsed(),
                                        receipt,
                                    );
                                    measured_frames += usize::from(step.measured());
                                    next_step += 1;
                                }
                                PendingKind::Capture {
                                    step,
                                    path,
                                    capture,
                                } => {
                                    if let Err(message) = write_png(
                                        &path,
                                        capture.width,
                                        capture.height,
                                        &capture.rgba8,
                                    ) {
                                        store_error(&shared_error, message);
                                        target.exit();
                                        return;
                                    }
                                    print_capture(
                                        &identity,
                                        step,
                                        waiting.output,
                                        &path,
                                        &capture,
                                        receipt,
                                    );
                                    terminal_outcome.mark_capture_complete();
                                }
                            }
                        }
                    }
                }

                if terminal_outcome.commit_if_ready() {
                    println!(
                        "SURFACE_EXACT_EVIDENCE_SUMMARY status=ok exact_plan_requested={} actual_plan_set={} trace_frames={} measured_frames={} eligibility_retries={} capture_retries={} terminal_receipts={} final_capture=available",
                        identity.requested_plan.label(),
                        actual_plans.iter().copied().collect::<Vec<_>>().join(","),
                        steps.len(),
                        measured_frames,
                        ineligible_retries,
                        capture_retries,
                        steps.len() + 1,
                    );
                    shared_completed.store(true, Ordering::Release);
                    target.exit();
                    return;
                }

                let (step, capture_attempt) = if next_step < steps.len() {
                    (steps[next_step], false)
                } else {
                    (capture_step, true)
                };
                if !request_pending {
                    match session.request_current_stats() {
                        SurfaceCurrentStatsRequest::Requested => request_pending = true,
                        SurfaceCurrentStatsRequest::Unsampled(reason) => {
                            store_error(
                                &shared_error,
                                format!("current-stats request unavailable: {reason:?}"),
                            );
                            target.exit();
                            return;
                        }
                    }
                }
                if let Err(message) = session
                    .set_camera(step.camera)
                    .map_err(|error| format!("surface evidence camera update failed: {error}"))
                {
                    store_error(&shared_error, message);
                    target.exit();
                    return;
                }
                session.force_sort_refresh();
                if capture_attempt
                    && let Err(error) = session.request_surface_capture()
                {
                    store_error(
                        &shared_error,
                        format!("surface evidence capture request failed: {error}"),
                    );
                    target.exit();
                    return;
                }
                let call_started = Instant::now();
                let output = match session.render_frame() {
                    Ok(output) => output,
                    Err(error) => {
                        store_error(
                            &shared_error,
                            format!("surface evidence render failed: {error}"),
                        );
                        target.exit();
                        return;
                    }
                };
                let call_ms = call_started.elapsed().as_secs_f32() * 1_000.0;
                if !output.frame_presented
                    || output.tiled_preparation_pending
                    || session.last_presented_size() != Some(identity.resolution.requested)
                {
                    store_error(
                        &shared_error,
                        format!(
                            "surface evidence frame was not a complete full-resolution presentation: presented={} preparation_pending={} presented_size={:?}",
                            output.frame_presented,
                            output.tiled_preparation_pending,
                            session.last_presented_size()
                        ),
                    );
                    target.exit();
                    return;
                }
                let capture = if capture_attempt {
                    match session.take_surface_capture() {
                        Ok(capture) => Some(capture),
                        Err(error) => {
                            store_error(
                                &shared_error,
                                format!("surface evidence capture readback failed: {error}"),
                            );
                            target.exit();
                            return;
                        }
                    }
                } else {
                    None
                };
                match session.current_stats_submission() {
                    SurfaceCurrentStatsSubmission::Issued(submission) => {
                        request_pending = false;
                        let kind = match capture {
                            Some(capture) => PendingKind::Capture {
                                step,
                                path: capture_path.clone(),
                                capture,
                            },
                            None => PendingKind::Frame(step),
                        };
                        pending = Some(PendingReceipt {
                            kind,
                            output,
                            submission,
                            call_ms,
                            deadline: Instant::now() + RECEIPT_TIMEOUT,
                        });
                    }
                    SurfaceCurrentStatsSubmission::NotRequested => {
                        ineligible_retries += 1;
                        capture_retries += usize::from(capture_attempt);
                        if ineligible_retries > MAX_INELIGIBLE_RETRIES {
                            store_error(
                                &shared_error,
                                format!(
                                    "current-stats request remained ineligible for more than {MAX_INELIGIBLE_RETRIES} presented frames"
                                ),
                            );
                            target.exit();
                            return;
                        }
                        match session.poll_current_stats() {
                            SurfaceCurrentStatsPoll::Empty => {}
                            SurfaceCurrentStatsPoll::Unsampled(reason) => {
                                request_pending = false;
                                store_error(
                                    &shared_error,
                                    format!("current-stats request failed before issue: {reason:?}"),
                                );
                                target.exit();
                            }
                            SurfaceCurrentStatsPoll::Terminal(terminal) => {
                                store_error(
                                    &shared_error,
                                    format!(
                                        "current-stats returned an unjoined terminal before issue: {terminal:?}"
                                    ),
                                );
                                target.exit();
                            }
                        }
                    }
                }
            }
            _ => {}
        })
        .map_err(|error| format!("surface evidence event loop failed: {error}"))?;

    if let Some(message) = error
        .lock()
        .map_err(|_| "surface evidence error lock poisoned".to_owned())?
        .take()
    {
        return Err(message);
    }
    if !completed.load(Ordering::Acquire) {
        return Err("surface evidence ended without a validated final capture".to_owned());
    }
    Ok(())
}

fn validate_terminal(
    identity: &EvidenceIdentity,
    waiting: &PendingReceipt,
    terminal: SurfaceCurrentStatsTerminal,
    presented_size: Option<(u32, u32)>,
) -> Result<SurfaceCurrentStatsReceipt, String> {
    let receipt = match terminal {
        SurfaceCurrentStatsTerminal::Ready(receipt) => receipt,
        SurfaceCurrentStatsTerminal::MapFailure(failure) => {
            return Err(format!(
                "current-stats ticket {} map failed",
                failure.submission().ticket()
            ));
        }
        SurfaceCurrentStatsTerminal::GenerationInvalidated(failure) => {
            return Err(format!(
                "current-stats ticket {} generation was invalidated",
                failure.submission().ticket()
            ));
        }
        SurfaceCurrentStatsTerminal::Expired(failure) => {
            return Err(format!(
                "current-stats ticket {} expired",
                failure.submission().ticket()
            ));
        }
        SurfaceCurrentStatsTerminal::Dropped(failure) => {
            return Err(format!(
                "current-stats ticket {} was dropped",
                failure.submission().ticket()
            ));
        }
    };
    if receipt.submission() != waiting.submission {
        return Err(format!(
            "current-stats terminal identity mismatch: expected ticket {}, got {}",
            waiting.submission.ticket(),
            receipt.submission().ticket()
        ));
    }
    if presented_size != Some(identity.resolution.requested) {
        return Err("current-stats terminal lost the matching presentation size".to_owned());
    }
    let join = receipt.submission().join();
    if join.frame_identity().camera_revision() != waiting.output.camera_revision {
        return Err("current-stats terminal camera revision does not match its frame".to_owned());
    }
    validate_plan(
        identity.requested_plan,
        waiting.output,
        join.executed_plan(),
    )?;
    validate_counts(identity.source_count, receipt)?;
    Ok(receipt)
}

fn validate_plan(
    requested: SurfaceEvidencePlanArg,
    output: SurfaceFrameOutput,
    actual: SurfaceCurrentStatsPlan,
) -> Result<(), String> {
    let forced = match requested {
        SurfaceEvidencePlanArg::CpuPostSort => Some(SurfaceCurrentStatsPlan::CpuPostSort),
        SurfaceEvidencePlanArg::GpuPostSort => Some(SurfaceCurrentStatsPlan::GpuPostSort),
        SurfaceEvidencePlanArg::GpuPreproject => Some(SurfaceCurrentStatsPlan::GpuPreproject),
        SurfaceEvidencePlanArg::Adaptive => None,
    };
    if forced.is_some_and(|forced| forced != actual) {
        return Err(format!(
            "forced plan mismatch: requested={}, actual={}",
            requested.label(),
            plan_label(actual)
        ));
    }
    let expected = match actual {
        SurfaceCurrentStatsPlan::CpuPostSort => (
            SurfaceOrderBackendUsed::Cpu,
            SurfaceProjectedDrawExecution::Candidate,
            None,
        ),
        SurfaceCurrentStatsPlan::GpuPostSort => (
            SurfaceOrderBackendUsed::Gpu,
            SurfaceProjectedDrawExecution::Candidate,
            Some(SurfaceGpuOrderProducer::PostSort),
        ),
        SurfaceCurrentStatsPlan::GpuPreproject => (
            SurfaceOrderBackendUsed::Gpu,
            SurfaceProjectedDrawExecution::Compact,
            Some(SurfaceGpuOrderProducer::Preproject),
        ),
    };
    if (
        output.order_backend,
        output.projected_draw_execution,
        output.gpu_order_producer,
    ) != expected
    {
        return Err(format!(
            "frame output contradicts executed plan {}",
            plan_label(actual)
        ));
    }
    Ok(())
}

fn validate_counts(source_count: usize, receipt: SurfaceCurrentStatsReceipt) -> Result<(), String> {
    let counts = receipt.counts();
    if counts.source() as usize != source_count
        || counts.contributor() > counts.visible()
        || counts.visible() > counts.source()
    {
        return Err(format!(
            "current-stats ticket {} violates C<=V<=S: S={} V={} C={} D={}",
            receipt.submission().ticket(),
            counts.source(),
            counts.visible(),
            counts.contributor(),
            counts.drawn()
        ));
    }
    let drawn_is_valid = match receipt.count_semantics() {
        SurfaceCurrentStatsCountSemantics::DirectDrawEqualsVisible
        | SurfaceCurrentStatsCountSemantics::IndirectDrawEqualsVisible => {
            counts.drawn() == counts.visible()
        }
        SurfaceCurrentStatsCountSemantics::IndirectDrawEqualsContributor => {
            counts.drawn() == counts.contributor()
        }
    };
    if !drawn_is_valid {
        return Err(format!(
            "current-stats ticket {} violates its count semantics",
            receipt.submission().ticket()
        ));
    }
    Ok(())
}

fn print_frame(
    identity: &EvidenceIdentity,
    step: SurfaceTraceStep,
    output: SurfaceFrameOutput,
    call_ms: f32,
    elapsed: Duration,
    receipt: SurfaceCurrentStatsReceipt,
) {
    let submission = receipt.submission();
    let join = submission.join();
    let frame = join.frame_identity();
    let counts = receipt.counts();
    println!(
        "SURFACE_EXACT_EVIDENCE_FRAME trace_id={} trace_sha256={} playback_index={} phase={} measured_sample={} trace_frame={} trace_timestamp_ns={} elapsed_ns={} exact_plan_requested={} exact_plan_actual={} current_stats_ticket={} scene_generation={} camera_revision={} viewport_generation={} contract_generation={} plan_set_generation={} order_generation={} raster_generation={} encode_attempt={} presentation_sequence={} count_semantics={} source_count={} visible_count={} contributor_count={} drawn_count={} exact_contributor_compaction={} sort_refreshed={} actual_backend={} call_ms={:.6} frame_wall_ms={:.6} requested_width={} requested_height={} presented_width={} presented_height={} frame_presented=true terminal_receipt=ready",
        identity.trace_id,
        identity.trace_sha256,
        step.playback_index,
        step.phase.as_str(),
        option_usize(step.measured_sample_index),
        step.trace_frame_index,
        step.timestamp_ns,
        u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX),
        identity.requested_plan.label(),
        plan_label(join.executed_plan()),
        submission.ticket(),
        frame.scene_generation(),
        frame.camera_revision(),
        frame.viewport_generation(),
        frame.contract_generation(),
        frame.plan_set_generation(),
        join.order_generation(),
        join.raster_generation(),
        join.encode_attempt(),
        join.presentation_sequence(),
        count_semantics_label(receipt.count_semantics()),
        counts.source(),
        counts.visible(),
        counts.contributor(),
        counts.drawn(),
        receipt.count_semantics()
            == SurfaceCurrentStatsCountSemantics::IndirectDrawEqualsContributor,
        output.sort_refreshed,
        backend_label(output.order_backend),
        call_ms,
        output.timings.frame_wall_ms,
        identity.resolution.requested.0,
        identity.resolution.requested.1,
        identity.resolution.requested.0,
        identity.resolution.requested.1,
    );
}

fn print_capture(
    identity: &EvidenceIdentity,
    step: SurfaceTraceStep,
    output: SurfaceFrameOutput,
    path: &Path,
    capture: &SurfaceFrameCapture,
    receipt: SurfaceCurrentStatsReceipt,
) {
    let submission = receipt.submission();
    let join = submission.join();
    let counts = receipt.counts();
    println!(
        "SURFACE_EXACT_EVIDENCE_CAPTURE status=ok path={:?} trace_frame={} exact_plan_requested={} exact_plan_actual={} current_stats_ticket={} camera_revision={} presentation_sequence={} count_semantics={} source_count={} visible_count={} contributor_count={} drawn_count={} exact_contributor_compaction={} actual_backend={} requested_width={} requested_height={} captured_width={} captured_height={} frame_presented={} terminal_receipt=ready",
        path.to_string_lossy(),
        step.trace_frame_index,
        identity.requested_plan.label(),
        plan_label(join.executed_plan()),
        submission.ticket(),
        join.frame_identity().camera_revision(),
        join.presentation_sequence(),
        count_semantics_label(receipt.count_semantics()),
        counts.source(),
        counts.visible(),
        counts.contributor(),
        counts.drawn(),
        receipt.count_semantics()
            == SurfaceCurrentStatsCountSemantics::IndirectDrawEqualsContributor,
        backend_label(output.order_backend),
        identity.resolution.requested.0,
        identity.resolution.requested.1,
        capture.width,
        capture.height,
        output.frame_presented,
    );
}

fn store_error(slot: &Mutex<Option<String>>, message: String) {
    if let Ok(mut slot) = slot.lock() {
        *slot = Some(message);
    }
}

const fn plan_label(plan: SurfaceCurrentStatsPlan) -> &'static str {
    match plan {
        SurfaceCurrentStatsPlan::CpuPostSort => "cpu_post_sort",
        SurfaceCurrentStatsPlan::GpuPostSort => "gpu_post_sort",
        SurfaceCurrentStatsPlan::GpuPreproject => "gpu_preproject",
    }
}

const fn count_semantics_label(semantics: SurfaceCurrentStatsCountSemantics) -> &'static str {
    match semantics {
        SurfaceCurrentStatsCountSemantics::DirectDrawEqualsVisible => "direct_draw_equals_visible",
        SurfaceCurrentStatsCountSemantics::IndirectDrawEqualsVisible => {
            "indirect_draw_equals_visible"
        }
        SurfaceCurrentStatsCountSemantics::IndirectDrawEqualsContributor => {
            "indirect_draw_equals_contributor"
        }
    }
}

const fn backend_label(backend: SurfaceOrderBackendUsed) -> &'static str {
    match backend {
        SurfaceOrderBackendUsed::Cpu => "cpu",
        SurfaceOrderBackendUsed::Gpu => "gpu",
    }
}

fn option_usize(value: Option<usize>) -> String {
    value.map_or_else(|| "none".to_owned(), |value| value.to_string())
}

const fn adapter_backend_label(backend: wgpu::Backend) -> &'static str {
    match backend {
        wgpu::Backend::Noop => "noop",
        wgpu::Backend::Vulkan => "vulkan",
        wgpu::Backend::Metal => "metal",
        wgpu::Backend::Dx12 => "dx12",
        wgpu::Backend::Gl => "gl",
        wgpu::Backend::BrowserWebGpu => "browser_webgpu",
    }
}

const fn adapter_device_type_label(device_type: wgpu::DeviceType) -> &'static str {
    match device_type {
        wgpu::DeviceType::Other => "other",
        wgpu::DeviceType::IntegratedGpu => "integrated_gpu",
        wgpu::DeviceType::DiscreteGpu => "discrete_gpu",
        wgpu::DeviceType::VirtualGpu => "virtual_gpu",
        wgpu::DeviceType::Cpu => "cpu",
    }
}

fn nonempty_adapter_field(value: &str) -> &str {
    if value.is_empty() {
        "unavailable"
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requested_plan_labels_are_closed_and_stable() {
        assert_eq!(SurfaceEvidencePlanArg::CpuPostSort.label(), "cpu_post_sort");
        assert_eq!(SurfaceEvidencePlanArg::GpuPostSort.label(), "gpu_post_sort");
        assert_eq!(
            SurfaceEvidencePlanArg::GpuPreproject.label(),
            "gpu_preproject"
        );
        assert_eq!(SurfaceEvidencePlanArg::Adaptive.label(), "adaptive");
    }

    #[test]
    fn count_semantics_labels_do_not_conflate_candidate_and_contributor_draws() {
        assert_eq!(
            count_semantics_label(SurfaceCurrentStatsCountSemantics::IndirectDrawEqualsVisible),
            "indirect_draw_equals_visible"
        );
        assert_eq!(
            count_semantics_label(SurfaceCurrentStatsCountSemantics::IndirectDrawEqualsContributor),
            "indirect_draw_equals_contributor"
        );
    }

    #[test]
    fn terminal_outcome_commits_once_and_suppresses_repeated_delivery() {
        let mut outcome = TerminalOutcome::default();
        assert!(!outcome.is_committed());
        assert!(!outcome.commit_if_ready());

        outcome.mark_capture_complete();
        assert!(outcome.commit_if_ready());
        assert!(outcome.is_committed());

        for _ in 0..162 {
            assert!(!outcome.commit_if_ready());
            assert!(outcome.is_committed());
        }
    }
}
