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

#[cfg(all(
    feature = "diagnostic-surface-capture-receipt",
    not(target_arch = "wasm32")
))]
use std::fs;

#[cfg(all(
    feature = "diagnostic-surface-capture-receipt",
    not(target_arch = "wasm32")
))]
use gsplat_render_wgpu::DiagnosticSurfaceCaptureReceipt;
use gsplat_render_wgpu::{
    SurfaceCurrentStatsCountSemantics, SurfaceCurrentStatsPlan, SurfaceCurrentStatsPoll,
    SurfaceCurrentStatsReceipt, SurfaceCurrentStatsRequest, SurfaceCurrentStatsSubmission,
    SurfaceCurrentStatsSubmissionReceipt, SurfaceCurrentStatsTerminal, SurfaceFrameCapture,
    SurfaceFrameOutput, SurfaceGpuOrderProducer, SurfaceOrderBackend, SurfaceOrderBackendUsed,
    SurfaceProjectedDrawExecution, SurfaceRasterExecutionPlan, SurfaceRenderSession,
};
#[cfg(all(
    feature = "diagnostic-surface-capture-receipt",
    not(target_arch = "wasm32")
))]
use sha2::{Digest, Sha256};
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
const DIAGNOSTIC_MULTI_CAPTURE_TRACE_FRAMES: [usize; 3] = [0, 1, 0];

#[derive(Debug, Default, PartialEq, Eq)]
struct MultiCaptureSchedule {
    next_capture: usize,
}

impl MultiCaptureSchedule {
    fn next_trace_frame(&self) -> Option<usize> {
        DIAGNOSTIC_MULTI_CAPTURE_TRACE_FRAMES
            .get(self.next_capture)
            .copied()
    }

    fn accept(&mut self, capture_index: usize, trace_frame: usize) -> Result<(), String> {
        let Some(expected_trace_frame) = self.next_trace_frame() else {
            return Err(format!(
                "duplicate diagnostic capture {capture_index}: terminal sequence is already complete"
            ));
        };
        if capture_index != self.next_capture || trace_frame != expected_trace_frame {
            return Err(format!(
                "diagnostic capture out of order: expected capture {} trace frame {}, got capture {capture_index} trace frame {trace_frame}",
                self.next_capture, expected_trace_frame,
            ));
        }
        self.next_capture += 1;
        Ok(())
    }

    fn require_complete(&self) -> Result<(), String> {
        if self.next_capture != DIAGNOSTIC_MULTI_CAPTURE_TRACE_FRAMES.len() {
            return Err(format!(
                "incomplete diagnostic capture terminal sequence: retained {} of {} captures",
                self.next_capture,
                DIAGNOSTIC_MULTI_CAPTURE_TRACE_FRAMES.len(),
            ));
        }
        Ok(())
    }
}

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
        capture_index: Option<usize>,
        step: SurfaceTraceStep,
        path: PathBuf,
        capture: PendingCapture,
    },
}

#[derive(Debug)]
enum PendingCapture {
    Ordinary(SurfaceFrameCapture),
    #[cfg(all(
        feature = "diagnostic-surface-capture-receipt",
        not(target_arch = "wasm32")
    ))]
    Diagnostic(DiagnosticSurfaceCaptureReceipt),
}

#[cfg(all(
    feature = "diagnostic-surface-capture-receipt",
    not(target_arch = "wasm32")
))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DiagnosticCaptureJoinIdentity {
    scene_generation: u64,
    camera_revision: u64,
    viewport_generation: u64,
    contract_generation: u64,
    plan_set_generation: u64,
    plan_id: &'static str,
    order_generation: u64,
    presentation_sequence: u64,
    width: u32,
    height: u32,
}

#[cfg(all(
    feature = "diagnostic-surface-capture-receipt",
    not(target_arch = "wasm32")
))]
impl DiagnosticCaptureJoinIdentity {
    fn from_capture(receipt: &DiagnosticSurfaceCaptureReceipt) -> Self {
        let frame = receipt.frame_identity();
        Self {
            scene_generation: frame.scene_generation(),
            camera_revision: frame.camera_revision(),
            viewport_generation: frame.viewport_generation(),
            contract_generation: frame.contract_generation(),
            plan_set_generation: frame.plan_set_generation(),
            plan_id: receipt.plan_id(),
            order_generation: receipt.order_generation(),
            presentation_sequence: receipt.presentation_sequence(),
            width: receipt.width(),
            height: receipt.height(),
        }
    }

    fn from_current_stats(
        receipt: &SurfaceCurrentStatsReceipt,
        requested_size: (u32, u32),
    ) -> Self {
        let join = receipt.submission().join();
        let frame = join.frame_identity();
        Self {
            scene_generation: frame.scene_generation(),
            camera_revision: frame.camera_revision(),
            viewport_generation: frame.viewport_generation(),
            contract_generation: frame.contract_generation(),
            plan_set_generation: frame.plan_set_generation(),
            plan_id: diagnostic_plan_id(join.executed_plan()),
            order_generation: join.order_generation(),
            presentation_sequence: join.presentation_sequence(),
            width: requested_size.0,
            height: requested_size.1,
        }
    }
}

#[cfg(all(
    feature = "diagnostic-surface-capture-receipt",
    not(target_arch = "wasm32")
))]
#[derive(Debug, PartialEq, Eq)]
struct DiagnosticCaptureReceiptRecord {
    profile: &'static str,
    scene_generation: u64,
    camera_revision: u64,
    viewport_generation: u64,
    contract_generation: u64,
    plan_set_generation: u64,
    plan_id: &'static str,
    order_generation: u64,
    presentation_sequence: u64,
    width: u32,
    height: u32,
    rgba8_sha256: String,
}

#[cfg(all(
    feature = "diagnostic-surface-capture-receipt",
    not(target_arch = "wasm32")
))]
impl DiagnosticCaptureReceiptRecord {
    fn from_receipt(receipt: &DiagnosticSurfaceCaptureReceipt) -> Self {
        let frame = receipt.frame_identity();
        let rgba8_sha256 = rgba8_sha256(receipt.rgba8());
        Self {
            profile: receipt.depth_precision_profile(),
            scene_generation: frame.scene_generation(),
            camera_revision: frame.camera_revision(),
            viewport_generation: frame.viewport_generation(),
            contract_generation: frame.contract_generation(),
            plan_set_generation: frame.plan_set_generation(),
            plan_id: receipt.plan_id(),
            order_generation: receipt.order_generation(),
            presentation_sequence: receipt.presentation_sequence(),
            width: receipt.width(),
            height: receipt.height(),
            rgba8_sha256,
        }
    }

    fn line(&self) -> String {
        format!(
            "SURFACE_DIAGNOSTIC_CAPTURE_RECEIPT profile={} scene_generation={} camera_revision={} viewport_generation={} contract_generation={} plan_set_generation={} plan_id={} order_generation={} presentation_sequence={} width={} height={} rgba8_sha256={}",
            self.profile,
            self.scene_generation,
            self.camera_revision,
            self.viewport_generation,
            self.contract_generation,
            self.plan_set_generation,
            self.plan_id,
            self.order_generation,
            self.presentation_sequence,
            self.width,
            self.height,
            self.rgba8_sha256,
        )
    }
}

#[cfg(all(
    feature = "diagnostic-surface-capture-receipt",
    not(target_arch = "wasm32")
))]
fn rgba8_sha256(rgba8: &[u8]) -> String {
    format!("{:x}", Sha256::digest(rgba8))
}

#[derive(Debug)]
struct PendingReceipt {
    kind: PendingKind,
    output: SurfaceFrameOutput,
    submission: SurfaceCurrentStatsSubmissionReceipt,
    call_ms: f32,
    deadline: Instant,
}

#[cfg(all(
    feature = "diagnostic-surface-capture-receipt",
    not(target_arch = "wasm32")
))]
#[derive(Debug)]
struct ValidatedDiagnosticCapture {
    capture_index: usize,
    step: SurfaceTraceStep,
    path: PathBuf,
    capture: DiagnosticSurfaceCaptureReceipt,
    output: SurfaceFrameOutput,
    receipt: SurfaceCurrentStatsReceipt,
    call_ms: f32,
    elapsed_ns: u64,
}

fn diagnostic_multi_capture_steps(
    playback: &CameraTracePlayback,
    first_playback_index: usize,
    template: SurfaceTraceStep,
) -> Result<Vec<SurfaceTraceStep>, String> {
    DIAGNOSTIC_MULTI_CAPTURE_TRACE_FRAMES
        .into_iter()
        .enumerate()
        .map(|(capture_index, trace_frame_index)| {
            let frame = playback.trace().frame(trace_frame_index).map_err(|error| {
                format!(
                    "diagnostic multi-capture requires trace frame {trace_frame_index}: {error}"
                )
            })?;
            Ok(SurfaceTraceStep {
                playback_index: first_playback_index + capture_index,
                phase_frame_index: capture_index,
                measured_sample_index: None,
                trace_frame_index,
                timestamp_ns: frame.timestamp_ns,
                camera: frame.camera().map_err(|error| error.to_string())?,
                ..template
            })
        })
        .collect()
}

fn diagnostic_multi_capture_path(
    base: &Path,
    capture_index: usize,
    trace_frame: usize,
) -> Result<PathBuf, String> {
    let stem = base
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "diagnostic multi-capture PNG path requires a UTF-8 file stem".to_owned())?;
    let extension = base
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("png");
    Ok(base
        .with_file_name(format!("{stem}.captures"))
        .join(format!(
            "capture-{capture_index}-trace-{trace_frame}.{extension}"
        )))
}

fn diagnostic_multi_capture_directory(base: &Path) -> Result<PathBuf, String> {
    let stem = base
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "diagnostic multi-capture PNG path requires a UTF-8 file stem".to_owned())?;
    Ok(base.with_file_name(format!("{stem}.captures")))
}

fn validate_capture_timing(
    call_ms: f32,
    frame_wall_ms: f32,
    elapsed_ns: u64,
    previous_elapsed_ns: Option<u64>,
) -> Result<(), String> {
    if !call_ms.is_finite() || call_ms < 0.0 {
        return Err(format!("diagnostic capture has invalid call_ms {call_ms}"));
    }
    if !frame_wall_ms.is_finite() || frame_wall_ms < 0.0 {
        return Err(format!(
            "diagnostic capture has invalid frame_wall_ms {frame_wall_ms}"
        ));
    }
    if previous_elapsed_ns.is_some_and(|previous| elapsed_ns <= previous) {
        return Err(format!(
            "diagnostic capture elapsed_ns is not strictly ordered: previous={}, current={elapsed_ns}",
            previous_elapsed_ns.unwrap_or_default(),
        ));
    }
    Ok(())
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
    let diagnostic_capture_receipt = args.surface_diagnostic_capture_receipt;
    let diagnostic_multi_capture = args.surface_diagnostic_multi_capture;
    let steps = surface_trace_steps(playback)?;
    let capture_step = steps
        .last()
        .copied()
        .ok_or_else(|| "surface evidence trace schedule is empty".to_owned())?;
    let multi_capture_steps = if diagnostic_multi_capture {
        diagnostic_multi_capture_steps(playback, steps.len(), capture_step)?
    } else {
        Vec::new()
    };
    let multi_capture_directory = if diagnostic_multi_capture {
        let directory = diagnostic_multi_capture_directory(&capture_path)?;
        if directory.exists() {
            return Err(format!(
                "diagnostic multi-capture destination already exists: {}",
                directory.display()
            ));
        }
        Some(directory)
    } else {
        None
    };
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
    let mut multi_capture_schedule = MultiCaptureSchedule::default();
    #[cfg(all(
        feature = "diagnostic-surface-capture-receipt",
        not(target_arch = "wasm32")
    ))]
    let mut validated_multi_captures = Vec::<ValidatedDiagnosticCapture>::new();

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
                            #[cfg(all(
                                feature = "diagnostic-surface-capture-receipt",
                                not(target_arch = "wasm32")
                            ))]
                            if let PendingKind::Capture {
                                capture: PendingCapture::Diagnostic(capture_receipt),
                                ..
                            } = &waiting.kind
                                && let Err(message) = validate_diagnostic_capture_join(
                                    capture_receipt,
                                    &receipt,
                                    identity.resolution.requested,
                                )
                            {
                                store_error(&shared_error, message);
                                target.exit();
                                return;
                            }
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
                                    capture_index,
                                    step,
                                    path,
                                    capture,
                                } => {
                                    #[cfg(all(
                                        feature = "diagnostic-surface-capture-receipt",
                                        not(target_arch = "wasm32")
                                    ))]
                                    if diagnostic_multi_capture {
                                        let Some(capture_index) = capture_index else {
                                            store_error(
                                                &shared_error,
                                                "diagnostic multi-capture lost its capture index"
                                                    .to_owned(),
                                            );
                                            target.exit();
                                            return;
                                        };
                                        let PendingCapture::Diagnostic(capture) = capture else {
                                            store_error(
                                                &shared_error,
                                                "diagnostic multi-capture received an ordinary capture"
                                                    .to_owned(),
                                            );
                                            target.exit();
                                            return;
                                        };
                                        if let Err(message) = multi_capture_schedule
                                            .accept(capture_index, step.trace_frame_index)
                                        {
                                            store_error(&shared_error, message);
                                            target.exit();
                                            return;
                                        }
                                        let elapsed_ns = u64::try_from(started.elapsed().as_nanos())
                                            .unwrap_or(u64::MAX);
                                        if let Err(message) = validate_capture_timing(
                                            waiting.call_ms,
                                            waiting.output.timings.frame_wall_ms,
                                            elapsed_ns,
                                            validated_multi_captures
                                                .last()
                                                .map(|capture| capture.elapsed_ns),
                                        ) {
                                            store_error(&shared_error, message);
                                            target.exit();
                                            return;
                                        }
                                        validated_multi_captures.push(
                                            ValidatedDiagnosticCapture {
                                                capture_index,
                                                step,
                                                path,
                                                capture,
                                                output: waiting.output,
                                                receipt,
                                                call_ms: waiting.call_ms,
                                                elapsed_ns,
                                            },
                                        );
                                        if multi_capture_schedule.next_trace_frame().is_some() {
                                            return;
                                        }
                                        if let Err(message) = multi_capture_schedule
                                            .require_complete()
                                            .and_then(|()| {
                                                publish_diagnostic_multi_captures(
                                                    &identity,
                                                    multi_capture_directory.as_deref().ok_or_else(
                                                        || {
                                                            "diagnostic multi-capture destination was not admitted"
                                                                .to_owned()
                                                        },
                                                    )?,
                                                    &validated_multi_captures,
                                                )
                                            })
                                        {
                                            store_error(&shared_error, message);
                                            target.exit();
                                            return;
                                        }
                                        terminal_outcome.mark_capture_complete();
                                        return;
                                    }
                                    match capture {
                                        PendingCapture::Ordinary(capture) => {
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
                                                capture.width,
                                                capture.height,
                                                receipt,
                                            );
                                        }
                                        #[cfg(all(
                                            feature = "diagnostic-surface-capture-receipt",
                                            not(target_arch = "wasm32")
                                        ))]
                                        PendingCapture::Diagnostic(capture_receipt) => {
                                            if let Err(message) = write_png(
                                                &path,
                                                capture_receipt.width(),
                                                capture_receipt.height(),
                                                capture_receipt.rgba8(),
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
                                                capture_receipt.width(),
                                                capture_receipt.height(),
                                                receipt,
                                            );
                                            println!(
                                                "{}",
                                                DiagnosticCaptureReceiptRecord::from_receipt(
                                                    &capture_receipt
                                                )
                                                .line()
                                            );
                                        }
                                    }
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
                        steps.len()
                            + if diagnostic_multi_capture {
                                DIAGNOSTIC_MULTI_CAPTURE_TRACE_FRAMES.len()
                            } else {
                                1
                            },
                    );
                    shared_completed.store(true, Ordering::Release);
                    target.exit();
                    return;
                }

                let (step, capture_attempt) = if next_step < steps.len() {
                    (steps[next_step], false)
                } else if diagnostic_multi_capture {
                    let capture_index = multi_capture_schedule.next_capture;
                    let Some(step) = multi_capture_steps.get(capture_index).copied() else {
                        store_error(
                            &shared_error,
                            "diagnostic multi-capture schedule completed without terminal commit"
                                .to_owned(),
                        );
                        target.exit();
                        return;
                    };
                    (step, true)
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
                    || output.gpu_order_preparation_pending
                    || session.last_presented_size() != Some(identity.resolution.requested)
                {
                    store_error(
                        &shared_error,
                        format!(
                            "surface evidence frame was not a complete full-resolution presentation: presented={} preparation_pending={} presented_size={:?}",
                            output.frame_presented,
                            output.gpu_order_preparation_pending,
                            session.last_presented_size()
                        ),
                    );
                    target.exit();
                    return;
                }
                let capture = if capture_attempt {
                    match take_pending_capture(
                        &mut session,
                        diagnostic_capture_receipt,
                    ) {
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
                                capture_index: diagnostic_multi_capture
                                    .then_some(multi_capture_schedule.next_capture),
                                step,
                                path: if diagnostic_multi_capture {
                                    match diagnostic_multi_capture_path(
                                        &capture_path,
                                        multi_capture_schedule.next_capture,
                                        step.trace_frame_index,
                                    ) {
                                        Ok(path) => path,
                                        Err(message) => {
                                            store_error(&shared_error, message);
                                            target.exit();
                                            return;
                                        }
                                    }
                                } else {
                                    capture_path.clone()
                                },
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
        return Err(if diagnostic_multi_capture {
            "surface evidence ended with an incomplete diagnostic capture terminal sequence"
                .to_owned()
        } else {
            "surface evidence ended without a validated final capture".to_owned()
        });
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

#[cfg(all(
    feature = "diagnostic-surface-capture-receipt",
    not(target_arch = "wasm32")
))]
fn publish_diagnostic_multi_captures(
    identity: &EvidenceIdentity,
    directory: &Path,
    captures: &[ValidatedDiagnosticCapture],
) -> Result<(), String> {
    if captures.len() != DIAGNOSTIC_MULTI_CAPTURE_TRACE_FRAMES.len() {
        return Err(format!(
            "incomplete diagnostic capture terminal sequence: retained {} of {} captures",
            captures.len(),
            DIAGNOSTIC_MULTI_CAPTURE_TRACE_FRAMES.len(),
        ));
    }
    for (capture_index, (capture, expected_trace_frame)) in captures
        .iter()
        .zip(DIAGNOSTIC_MULTI_CAPTURE_TRACE_FRAMES)
        .enumerate()
    {
        if capture.capture_index != capture_index
            || capture.step.trace_frame_index != expected_trace_frame
        {
            return Err(format!(
                "diagnostic capture publication order mismatch at capture {capture_index}"
            ));
        }
        validate_diagnostic_capture_join(
            &capture.capture,
            &capture.receipt,
            identity.resolution.requested,
        )?;
        validate_capture_timing(
            capture.call_ms,
            capture.output.timings.frame_wall_ms,
            capture.elapsed_ns,
            capture_index
                .checked_sub(1)
                .map(|previous| captures[previous].elapsed_ns),
        )?;
        if capture.path.parent() != Some(directory) {
            return Err(format!(
                "diagnostic capture {} escaped admitted destination {}",
                capture.path.display(),
                directory.display(),
            ));
        }
    }

    fs::create_dir(directory).map_err(|error| {
        format!(
            "cannot create fresh diagnostic multi-capture destination {}: {error}",
            directory.display()
        )
    })?;
    for capture in captures {
        write_png(
            &capture.path,
            capture.capture.width(),
            capture.capture.height(),
            capture.capture.rgba8(),
        )?;
    }
    for capture in captures {
        print_diagnostic_multi_capture_terminal(capture);
    }
    Ok(())
}

#[cfg(all(
    feature = "diagnostic-surface-capture-receipt",
    not(target_arch = "wasm32")
))]
fn print_diagnostic_multi_capture_terminal(capture: &ValidatedDiagnosticCapture) {
    let submission = capture.receipt.submission();
    let join = submission.join();
    let frame = join.frame_identity();
    let counts = capture.receipt.counts();
    let atomic = DiagnosticCaptureReceiptRecord::from_receipt(&capture.capture);
    println!(
        "SURFACE_DIAGNOSTIC_MULTI_CAPTURE_TERMINAL status=ok capture_index={} path={:?} trace_frame={} trace_timestamp_ns={} elapsed_ns={} call_ms={:.6} frame_wall_ms={:.6} current_stats_ticket={} current_stats_scene_generation={} current_stats_camera_revision={} current_stats_viewport_generation={} current_stats_contract_generation={} current_stats_plan_set_generation={} current_stats_plan_id={} current_stats_order_generation={} current_stats_raster_generation={} current_stats_encode_attempt={} current_stats_presentation_sequence={} count_semantics={} source_count={} visible_count={} contributor_count={} drawn_count={} exact_contributor_compaction={} capture_receipt_profile={} capture_receipt_scene_generation={} capture_receipt_camera_revision={} capture_receipt_viewport_generation={} capture_receipt_contract_generation={} capture_receipt_plan_set_generation={} capture_receipt_plan_id={} capture_receipt_order_generation={} capture_receipt_presentation_sequence={} capture_receipt_width={} capture_receipt_height={} capture_receipt_rgba8_sha256={} frame_presented={} terminal_receipt=ready",
        capture.capture_index,
        capture.path.to_string_lossy(),
        capture.step.trace_frame_index,
        capture.step.timestamp_ns,
        capture.elapsed_ns,
        capture.call_ms,
        capture.output.timings.frame_wall_ms,
        submission.ticket(),
        frame.scene_generation(),
        frame.camera_revision(),
        frame.viewport_generation(),
        frame.contract_generation(),
        frame.plan_set_generation(),
        plan_label(join.executed_plan()),
        join.order_generation(),
        join.raster_generation(),
        join.encode_attempt(),
        join.presentation_sequence(),
        count_semantics_label(capture.receipt.count_semantics()),
        counts.source(),
        counts.visible(),
        counts.contributor(),
        counts.drawn(),
        capture.receipt.count_semantics()
            == SurfaceCurrentStatsCountSemantics::IndirectDrawEqualsContributor,
        atomic.profile,
        atomic.scene_generation,
        atomic.camera_revision,
        atomic.viewport_generation,
        atomic.contract_generation,
        atomic.plan_set_generation,
        atomic.plan_id,
        atomic.order_generation,
        atomic.presentation_sequence,
        atomic.width,
        atomic.height,
        atomic.rgba8_sha256,
        capture.output.frame_presented,
    );
}

fn print_capture(
    identity: &EvidenceIdentity,
    step: SurfaceTraceStep,
    output: SurfaceFrameOutput,
    path: &Path,
    captured_width: u32,
    captured_height: u32,
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
        captured_width,
        captured_height,
        output.frame_presented,
    );
}

fn take_pending_capture(
    session: &mut SurfaceRenderSession,
    diagnostic: bool,
) -> Result<PendingCapture, String> {
    #[cfg(all(
        feature = "diagnostic-surface-capture-receipt",
        not(target_arch = "wasm32")
    ))]
    if diagnostic {
        return session
            .take_diagnostic_surface_capture_receipt()
            .map(PendingCapture::Diagnostic)
            .map_err(|error| error.to_string());
    }

    if diagnostic {
        return Err("diagnostic Surface capture receipt support is unavailable".to_owned());
    }
    session
        .take_surface_capture()
        .map(PendingCapture::Ordinary)
        .map_err(|error| error.to_string())
}

#[cfg(all(
    feature = "diagnostic-surface-capture-receipt",
    not(target_arch = "wasm32")
))]
fn validate_diagnostic_capture_join(
    capture: &DiagnosticSurfaceCaptureReceipt,
    current_stats: &SurfaceCurrentStatsReceipt,
    requested_size: (u32, u32),
) -> Result<(), String> {
    validate_diagnostic_capture_join_identity(
        DiagnosticCaptureJoinIdentity::from_capture(capture),
        DiagnosticCaptureJoinIdentity::from_current_stats(current_stats, requested_size),
    )
}

#[cfg(all(
    feature = "diagnostic-surface-capture-receipt",
    not(target_arch = "wasm32")
))]
fn validate_diagnostic_capture_join_identity(
    diagnostic: DiagnosticCaptureJoinIdentity,
    current_stats: DiagnosticCaptureJoinIdentity,
) -> Result<(), String> {
    macro_rules! require_match {
        ($field:ident) => {
            if diagnostic.$field != current_stats.$field {
                return Err(format!(
                    "diagnostic capture/current-stats {} mismatch: diagnostic={:?}, current_stats={:?}",
                    stringify!($field),
                    diagnostic.$field,
                    current_stats.$field,
                ));
            }
        };
    }

    require_match!(scene_generation);
    require_match!(camera_revision);
    require_match!(viewport_generation);
    require_match!(contract_generation);
    require_match!(plan_set_generation);
    require_match!(plan_id);
    require_match!(order_generation);
    require_match!(presentation_sequence);
    require_match!(width);
    require_match!(height);
    Ok(())
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

#[cfg(all(
    feature = "diagnostic-surface-capture-receipt",
    not(target_arch = "wasm32")
))]
const fn diagnostic_plan_id(plan: SurfaceCurrentStatsPlan) -> &'static str {
    match plan {
        SurfaceCurrentStatsPlan::CpuPostSort => "CpuPostSort",
        SurfaceCurrentStatsPlan::GpuPostSort => "GpuPostSort",
        SurfaceCurrentStatsPlan::GpuPreproject => "GpuPreproject",
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
    fn diagnostic_multi_capture_schedule_is_exact_and_complete_only_after_010() {
        let mut schedule = MultiCaptureSchedule::default();
        assert_eq!(schedule.next_trace_frame(), Some(0));
        assert!(schedule.require_complete().is_err());

        for (capture_index, trace_frame) in [0, 1, 0].into_iter().enumerate() {
            schedule.accept(capture_index, trace_frame).unwrap();
        }

        assert_eq!(schedule.next_trace_frame(), None);
        assert_eq!(schedule.require_complete(), Ok(()));
        assert!(schedule.accept(3, 0).unwrap_err().contains("duplicate"));
    }

    #[test]
    fn diagnostic_multi_capture_schedule_rejects_missing_and_out_of_order_terminals() {
        let mut schedule = MultiCaptureSchedule::default();
        assert!(schedule.accept(1, 1).unwrap_err().contains("out of order"));
        assert!(schedule.accept(0, 1).unwrap_err().contains("out of order"));
        assert_eq!(schedule.next_trace_frame(), Some(0));

        schedule.accept(0, 0).unwrap();
        schedule.accept(1, 1).unwrap();
        assert!(
            schedule
                .require_complete()
                .unwrap_err()
                .contains("retained 2 of 3")
        );
        assert!(schedule.accept(1, 0).unwrap_err().contains("out of order"));
    }

    #[test]
    fn diagnostic_multi_capture_steps_resolve_actual_trace_frames_010() {
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/perf/trace/fixtures/camera-trace-v1.json"
        ))
        .unwrap();
        let trace = gsplat_core::camera_trace::CameraTrace::from_json_slice(&bytes).unwrap();
        let playback = CameraTracePlayback::Fixed {
            trace,
            frame_index: 0,
            warmup_frames: 1,
            measured_frames: 1,
        };
        let ordinary = surface_trace_steps(&playback).unwrap();
        let captures =
            diagnostic_multi_capture_steps(&playback, ordinary.len(), *ordinary.last().unwrap())
                .unwrap();

        assert_eq!(
            captures
                .iter()
                .map(|step| step.trace_frame_index)
                .collect::<Vec<_>>(),
            vec![0, 1, 0]
        );
        assert_eq!(
            captures
                .iter()
                .map(|step| step.playback_index)
                .collect::<Vec<_>>(),
            vec![ordinary.len(), ordinary.len() + 1, ordinary.len() + 2]
        );
        assert!(
            captures
                .iter()
                .all(|step| step.measured_sample_index.is_none())
        );

        let mut one_frame_trace = playback.trace().clone();
        one_frame_trace.frames.truncate(1);
        let one_frame_playback = CameraTracePlayback::Fixed {
            trace: one_frame_trace,
            frame_index: 0,
            warmup_frames: 0,
            measured_frames: 1,
        };
        assert!(
            diagnostic_multi_capture_steps(&one_frame_playback, ordinary.len(), ordinary[0],)
                .unwrap_err()
                .contains("requires trace frame 1")
        );
    }

    #[test]
    fn diagnostic_multi_capture_paths_are_distinct_and_directory_scoped() {
        let base = Path::new("target/final-frame.png");
        let directory = diagnostic_multi_capture_directory(base).unwrap();
        assert_eq!(directory, Path::new("target/final-frame.captures"));
        assert_eq!(
            DIAGNOSTIC_MULTI_CAPTURE_TRACE_FRAMES
                .into_iter()
                .enumerate()
                .map(|(index, trace_frame)| {
                    diagnostic_multi_capture_path(base, index, trace_frame).unwrap()
                })
                .collect::<Vec<_>>(),
            vec![
                PathBuf::from("target/final-frame.captures/capture-0-trace-0.png"),
                PathBuf::from("target/final-frame.captures/capture-1-trace-1.png"),
                PathBuf::from("target/final-frame.captures/capture-2-trace-0.png"),
            ]
        );
    }

    #[test]
    fn diagnostic_capture_timing_fails_closed_on_invalid_or_unordered_values() {
        assert_eq!(validate_capture_timing(1.0, 2.0, 3, None), Ok(()));
        assert_eq!(validate_capture_timing(1.0, 2.0, 4, Some(3)), Ok(()));
        assert!(validate_capture_timing(f32::NAN, 2.0, 3, None).is_err());
        assert!(validate_capture_timing(1.0, f32::INFINITY, 3, None).is_err());
        assert!(validate_capture_timing(1.0, 2.0, 3, Some(3)).is_err());
        assert!(validate_capture_timing(1.0, 2.0, 2, Some(3)).is_err());
    }

    #[cfg(all(
        feature = "diagnostic-surface-capture-receipt",
        not(target_arch = "wasm32")
    ))]
    #[test]
    fn diagnostic_capture_receipt_record_is_stable_and_hashes_rgba8_bytes() {
        assert_eq!(
            rgba8_sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );

        let record = DiagnosticCaptureReceiptRecord {
            profile: "CandidateStable24",
            scene_generation: 1,
            camera_revision: 2,
            viewport_generation: 3,
            contract_generation: 4,
            plan_set_generation: 5,
            plan_id: "GpuPostSort",
            order_generation: 6,
            presentation_sequence: 7,
            width: 1920,
            height: 1080,
            rgba8_sha256: "feed".to_owned(),
        };
        assert_eq!(
            record.line(),
            "SURFACE_DIAGNOSTIC_CAPTURE_RECEIPT profile=CandidateStable24 scene_generation=1 camera_revision=2 viewport_generation=3 contract_generation=4 plan_set_generation=5 plan_id=GpuPostSort order_generation=6 presentation_sequence=7 width=1920 height=1080 rgba8_sha256=feed"
        );
    }

    #[cfg(all(
        feature = "diagnostic-surface-capture-receipt",
        not(target_arch = "wasm32")
    ))]
    #[test]
    fn diagnostic_capture_join_identity_matches_all_keys_fail_closed() {
        let expected = DiagnosticCaptureJoinIdentity {
            scene_generation: 1,
            camera_revision: 2,
            viewport_generation: 3,
            contract_generation: 4,
            plan_set_generation: 5,
            plan_id: "GpuPostSort",
            order_generation: 6,
            presentation_sequence: 7,
            width: 1920,
            height: 1080,
        };
        assert!(validate_diagnostic_capture_join_identity(expected, expected).is_ok());

        for (actual, key) in [
            (
                DiagnosticCaptureJoinIdentity {
                    scene_generation: 8,
                    ..expected
                },
                "scene_generation",
            ),
            (
                DiagnosticCaptureJoinIdentity {
                    camera_revision: 8,
                    ..expected
                },
                "camera_revision",
            ),
            (
                DiagnosticCaptureJoinIdentity {
                    viewport_generation: 8,
                    ..expected
                },
                "viewport_generation",
            ),
            (
                DiagnosticCaptureJoinIdentity {
                    contract_generation: 8,
                    ..expected
                },
                "contract_generation",
            ),
            (
                DiagnosticCaptureJoinIdentity {
                    plan_set_generation: 8,
                    ..expected
                },
                "plan_set_generation",
            ),
            (
                DiagnosticCaptureJoinIdentity {
                    plan_id: "GpuPreproject",
                    ..expected
                },
                "plan_id",
            ),
            (
                DiagnosticCaptureJoinIdentity {
                    order_generation: 8,
                    ..expected
                },
                "order_generation",
            ),
            (
                DiagnosticCaptureJoinIdentity {
                    presentation_sequence: 8,
                    ..expected
                },
                "presentation_sequence",
            ),
            (
                DiagnosticCaptureJoinIdentity {
                    width: 1280,
                    ..expected
                },
                "width",
            ),
            (
                DiagnosticCaptureJoinIdentity {
                    height: 720,
                    ..expected
                },
                "height",
            ),
        ] {
            assert!(
                validate_diagnostic_capture_join_identity(actual, expected)
                    .unwrap_err()
                    .contains(key)
            );
        }
    }

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
