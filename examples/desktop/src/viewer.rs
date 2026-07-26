#[cfg(feature = "interactive-viewer")]
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use gsplat_core::Camera;
#[cfg(feature = "interactive-viewer")]
use gsplat_core::Vec3f;
#[cfg(feature = "interactive-viewer")]
use gsplat_core::camera_trace::CameraTraceSequencePhase;
#[cfg(feature = "interactive-viewer")]
use gsplat_core::{CameraIntrinsics, CameraPose};
use gsplat_render_wgpu::Renderer;
#[cfg(feature = "interactive-viewer")]
use gsplat_render_wgpu::{GeometryPath, SurfaceGpuOrderProducer, SurfaceOrderBackend};
#[cfg(feature = "interactive-viewer")]
use gsplat_render_wgpu::{
    SurfaceAdaptivePendingSample, SurfaceAdaptiveState, SurfaceCpuOrderMeasurement,
    SurfaceFrameOutput, SurfaceGpuProducerDrawScope, SurfaceGpuProducerMeasurement,
    SurfaceGpuProducerMeasurementFailure, SurfaceGpuProducerMeasurementSubmission,
    SurfaceGpuProducerMeasurementUnsampledReason, SurfaceOrderBackendUsed, SurfaceOrderMeasurement,
    SurfaceOrderMeasurementFailure, SurfaceOrderMeasurementSubmission,
    SurfaceOrderMeasurementUnsampledReason, SurfaceProjectedDrawPolicy, SurfaceRasterExecutionPlan,
    SurfaceRenderSession, SurfaceTimingSource,
};
#[cfg(feature = "interactive-viewer")]
use winit::{
    dpi::PhysicalSize,
    event::{ElementState, Event, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::EventLoop,
    keyboard::{KeyCode, PhysicalKey},
    window::WindowAttributes,
};

use crate::cli::Args;
#[cfg(feature = "interactive-viewer")]
use crate::cli::{SurfaceBenchmarkMode, SurfaceSortPolicyArg, geometry_path_label};
#[cfg(feature = "interactive-viewer")]
use crate::image_output::write_png;
#[cfg(feature = "interactive-viewer")]
use crate::scene::positions_center;
use crate::scene::{initial_camera, load_ply_path_into_renderer};
use crate::trace::CameraTracePlayback;

pub(crate) fn run(args: &Args, trace_playback: Option<&CameraTracePlayback>) -> Result<(), String> {
    let mut renderer =
        Renderer::with_config_for_surface(args.config).map_err(|error| error.to_string())?;
    renderer.set_geometry_path(args.geometry_path);
    load_ply_path_into_renderer(&args.dataset_path, &mut renderer)?;
    let camera = initial_camera(args, &renderer, trace_playback)?;
    run_interactive(args, renderer, camera, trace_playback)
}

#[cfg(feature = "interactive-viewer")]
#[allow(deprecated)] // winit 0.30 compatibility; migrate both loops to ApplicationHandler together.
fn run_interactive(
    args: &Args,
    renderer: Renderer,
    camera: Camera,
    trace_playback: Option<&CameraTracePlayback>,
) -> Result<(), String> {
    let event_loop =
        EventLoop::new().map_err(|err| format!("event loop creation failed: {err}"))?;
    let window = Arc::new(
        event_loop
            .create_window(
                WindowAttributes::default()
                    .with_title("gsplat-rs viewer")
                    .with_visible(trace_playback.is_none())
                    .with_inner_size(PhysicalSize::new(args.config.width, args.config.height)),
            )
            .map_err(|err| format!("window creation failed: {err}"))?,
    );
    let orbit_target = renderer
        .positions()
        .and_then(positions_center)
        .unwrap_or(Vec3f::new(0.0, 0.0, 0.0));
    let mut session = pollster::block_on(SurfaceRenderSession::from_window(
        renderer,
        window.clone(),
        args.config.width,
        args.config.height,
        camera,
    ))
    .map_err(|err| err.to_string())?;
    let surface_adapter_info = session.adapter_info().clone();
    session
        .set_sort_interval(1)
        .map_err(|err| err.to_string())?;
    if let Some(plan) = args.surface_evidence_plan {
        crate::surface_evidence::configure(&mut session, plan)?;
    } else if let Some(producer) = args.surface_gpu_producer {
        // The diagnostic producer comparison must construct only the selected
        // GPU graph before the backend becomes active. Both arms use the same
        // forced-Compact projected draw contract and independent receipts.
        session
            .set_raster_execution_plan(SurfaceRasterExecutionPlan::ProjectedQuadsExact)
            .map_err(|err| err.to_string())?;
        session
            .set_projected_draw_policy(SurfaceProjectedDrawPolicy::Compact)
            .map_err(|err| err.to_string())?;
        pollster::block_on(session.prepare_gpu_order_producer(producer))
            .map_err(|err| err.to_string())?;
        session
            .set_gpu_order_producer(producer)
            .map_err(|err| err.to_string())?;
        session
            .set_gpu_producer_measurement_enabled(true)
            .map_err(|err| err.to_string())?;
    }
    if args.surface_evidence_plan.is_none() {
        session
            .set_order_backend(args.order_backend)
            .map_err(|err| err.to_string())?;
    }

    if let Some(playback) = trace_playback {
        if args.surface_evidence_plan.is_some() {
            return crate::surface_evidence::run(
                args,
                event_loop,
                window,
                session,
                playback,
                surface_adapter_info,
            );
        }
        return run_surface_trace_benchmark(args, event_loop, window, session, playback);
    }

    println!("interactive viewer controls:");
    println!("  mouse-left drag / arrow keys: orbit around scene");
    println!("  W/S or wheel: dolly in/out, A/D/Q/E: pan");
    println!("  Q/E: down/up, Shift: faster, Ctrl: slower");
    println!("  Esc: exit");

    let mut controller = CameraController::new(camera, orbit_target);
    let mut input = InputState::default();
    let mut last_frame = Instant::now();
    let mut title_timer = Instant::now();
    let mut title_frames = 0_u32;
    let mut orbit_phase = 0.0_f32;
    let window_id = window.id();
    let render_error = Arc::new(Mutex::new(None::<String>));
    let render_error_shared = render_error.clone();

    event_loop
        .run(move |event, target| match event {
            Event::AboutToWait => {
                window.request_redraw();
            }
            Event::WindowEvent {
                window_id: id,
                event,
            } if id == window_id => match event {
                WindowEvent::CloseRequested => target.exit(),
                WindowEvent::Resized(size) => {
                    if let Err(err) = session.resize(size.width, size.height) {
                        if let Ok(mut slot) = render_error_shared.lock() {
                            *slot = Some(format!("interactive resize failed: {err}"));
                        }
                        target.exit();
                    }
                }
                WindowEvent::KeyboardInput { event, .. } => {
                    if let PhysicalKey::Code(code) = event.physical_key {
                        if code == KeyCode::Escape && event.state == ElementState::Pressed {
                            target.exit();
                            return;
                        }
                        match event.state {
                            ElementState::Pressed => {
                                input.keys_down.insert(code);
                            }
                            ElementState::Released => {
                                input.keys_down.remove(&code);
                            }
                        }
                    }
                }
                WindowEvent::MouseInput { state, button, .. } => {
                    if button == MouseButton::Left {
                        input.mouse_left_down = state == ElementState::Pressed;
                        if !input.mouse_left_down {
                            input.last_cursor = None;
                        }
                    }
                }
                WindowEvent::CursorMoved { position, .. } => {
                    let current = (position.x as f32, position.y as f32);
                    if input.mouse_left_down {
                        if let Some((lx, ly)) = input.last_cursor {
                            input.mouse_delta.0 += current.0 - lx;
                            input.mouse_delta.1 += current.1 - ly;
                        }
                        input.last_cursor = Some(current);
                    } else {
                        input.last_cursor = Some(current);
                    }
                }
                WindowEvent::MouseWheel { delta, .. } => match delta {
                    MouseScrollDelta::LineDelta(_, y) => input.scroll_y += y,
                    MouseScrollDelta::PixelDelta(p) => input.scroll_y += (p.y as f32) / 100.0,
                },
                WindowEvent::RedrawRequested => {
                    let now = Instant::now();
                    let dt = (now - last_frame).as_secs_f32().clamp(0.0, 0.1);
                    last_frame = now;

                    if args.orbit {
                        orbit_phase += dt * 0.8;
                        controller.set_yaw(orbit_phase);
                    } else {
                        controller.update(&input, dt);
                    }

                    let camera_now = controller.camera();
                    if let Err(err) = session.set_camera(camera_now) {
                        if let Ok(mut slot) = render_error_shared.lock() {
                            *slot = Some(format!("interactive camera update failed: {err}"));
                        }
                        target.exit();
                        return;
                    }
                    let output = match session.render_frame() {
                        Ok(output) => output,
                        Err(err) => {
                            if let Ok(mut slot) = render_error_shared.lock() {
                                *slot = Some(format!("interactive render failed: {err}"));
                            }
                            target.exit();
                            return;
                        }
                    };
                    let stats = output.stats;

                    title_frames = title_frames.saturating_add(1);
                    let elapsed = title_timer.elapsed();
                    if elapsed >= Duration::from_millis(500) {
                        let fps = (title_frames as f32) / elapsed.as_secs_f32().max(1e-6);
                        window.set_title(&format!(
                            "gsplat-rs viewer | fps={fps:.1} frame={:.2}ms visible={} drawn={}",
                            stats.frame_ms, stats.visible_count, stats.drawn_count
                        ));
                        title_timer = Instant::now();
                        title_frames = 0;
                    }

                    input.end_frame();
                }
                _ => {}
            },
            _ => {}
        })
        .map_err(|err| format!("interactive event loop failed: {err}"))?;

    if let Some(err) = render_error
        .lock()
        .map_err(|_| "interactive error state lock poisoned".to_owned())?
        .take()
    {
        return Err(err);
    }
    println!("interactive-viewer exited");
    Ok(())
}

#[cfg(feature = "interactive-viewer")]
const SURFACE_TELEMETRY_DRAIN_TIMEOUT: Duration = Duration::from_secs(30);

#[cfg(feature = "interactive-viewer")]
#[derive(Debug, Clone, Copy)]
pub(crate) struct SurfaceTraceStep {
    pub(crate) playback_index: usize,
    pub(crate) phase: CameraTraceSequencePhase,
    pub(crate) loop_index: usize,
    pub(crate) phase_frame_index: usize,
    pub(crate) measured_sample_index: Option<usize>,
    pub(crate) trace_frame_index: usize,
    pub(crate) timestamp_ns: u64,
    pub(crate) camera: Camera,
}

#[cfg(feature = "interactive-viewer")]
impl SurfaceTraceStep {
    pub(crate) const fn measured(self) -> bool {
        matches!(self.phase, CameraTraceSequencePhase::Measure)
    }
}

#[cfg(feature = "interactive-viewer")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SurfaceResolutionReceipt {
    pub(crate) requested: (u32, u32),
    pub(crate) surface: (u32, u32),
    pub(crate) internal_render: (u32, u32),
}

#[cfg(feature = "interactive-viewer")]
impl SurfaceResolutionReceipt {
    pub(crate) fn validated(
        requested: (u32, u32),
        surface: (u32, u32),
        internal_render: (u32, u32),
    ) -> Result<Self, String> {
        if requested != surface || requested != internal_render {
            return Err(format!(
                "full-resolution Surface mismatch: requested={}x{}, surface={}x{}, internal_render={}x{}",
                requested.0,
                requested.1,
                surface.0,
                surface.1,
                internal_render.0,
                internal_render.1,
            ));
        }
        Ok(Self {
            requested,
            surface,
            internal_render,
        })
    }

    pub(crate) const fn full_resolution(self) -> bool {
        self.requested.0 == self.surface.0
            && self.requested.1 == self.surface.1
            && self.requested.0 == self.internal_render.0
            && self.requested.1 == self.internal_render.1
    }
}

#[cfg(feature = "interactive-viewer")]
#[derive(Debug, Clone)]
struct SurfaceBenchmarkIdentity {
    trace_id: String,
    trace_sha256: String,
    mode: SurfaceBenchmarkMode,
    sort_policy: SurfaceSortPolicyArg,
    requested_backend: SurfaceOrderBackend,
    geometry_path: GeometryPath,
    raster_execution_plan: SurfaceRasterExecutionPlan,
    gpu_order_producer: Option<SurfaceGpuOrderProducer>,
    projected_draw_policy: SurfaceProjectedDrawPolicy,
    resolution: SurfaceResolutionReceipt,
    source_count: usize,
    resident_count: usize,
    sh_degree: u8,
}

#[cfg(feature = "interactive-viewer")]
pub(crate) fn surface_trace_steps(
    playback: &CameraTracePlayback,
) -> Result<Vec<SurfaceTraceStep>, String> {
    let total = match playback {
        CameraTracePlayback::Fixed {
            warmup_frames,
            measured_frames,
            ..
        } => warmup_frames
            .checked_add(*measured_frames)
            .ok_or_else(|| "fixed camera trace schedule length overflowed usize".to_owned())?,
        CameraTracePlayback::Sequence {
            trace,
            frame_indices,
            warmup_frames,
            measured_frames,
            loops,
        } => trace
            .sequence(
                frame_indices.clone(),
                *warmup_frames,
                *measured_frames,
                *loops,
            )
            .map_err(|error| error.to_string())?
            .len(),
    };
    let mut steps = Vec::new();
    steps
        .try_reserve_exact(total)
        .map_err(|_| format!("cannot allocate {total} camera trace schedule entries"))?;

    match playback {
        CameraTracePlayback::Fixed {
            trace,
            frame_index,
            warmup_frames,
            measured_frames,
        } => {
            let frame = trace
                .frame(*frame_index)
                .map_err(|error| error.to_string())?;
            let camera = frame.camera().map_err(|error| error.to_string())?;
            for playback_index in 0..*warmup_frames {
                steps.push(SurfaceTraceStep {
                    playback_index,
                    phase: CameraTraceSequencePhase::Warmup,
                    loop_index: 0,
                    phase_frame_index: playback_index,
                    measured_sample_index: None,
                    trace_frame_index: *frame_index,
                    timestamp_ns: frame.timestamp_ns,
                    camera,
                });
            }
            for measured_sample_index in 0..*measured_frames {
                steps.push(SurfaceTraceStep {
                    playback_index: *warmup_frames + measured_sample_index,
                    phase: CameraTraceSequencePhase::Measure,
                    loop_index: 0,
                    phase_frame_index: measured_sample_index,
                    measured_sample_index: Some(measured_sample_index),
                    trace_frame_index: *frame_index,
                    timestamp_ns: frame.timestamp_ns,
                    camera,
                });
            }
        }
        CameraTracePlayback::Sequence {
            trace,
            frame_indices,
            warmup_frames,
            measured_frames,
            loops,
        } => {
            let sequence = trace
                .sequence(
                    frame_indices.clone(),
                    *warmup_frames,
                    *measured_frames,
                    *loops,
                )
                .map_err(|error| error.to_string())?;
            for (playback_index, step) in sequence.steps().enumerate() {
                steps.push(SurfaceTraceStep {
                    playback_index,
                    phase: step.phase,
                    loop_index: step.loop_index,
                    phase_frame_index: step.phase_frame_index,
                    measured_sample_index: step.measured_sample_index,
                    trace_frame_index: step.trace_frame_index,
                    timestamp_ns: step.frame.timestamp_ns,
                    camera: step.frame.camera().map_err(|error| error.to_string())?,
                });
            }
        }
    }
    Ok(steps)
}

#[cfg(feature = "interactive-viewer")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SurfaceBenchmarkTicket {
    pub(crate) backend: SurfaceOrderBackendUsed,
    pub(crate) camera_revision: u64,
    pub(crate) measured: bool,
}

#[cfg(feature = "interactive-viewer")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SurfaceBenchmarkProducerTicket {
    pub(crate) producer: SurfaceGpuOrderProducer,
    pub(crate) camera_revision: u64,
    pub(crate) measured: bool,
}

#[cfg(feature = "interactive-viewer")]
#[derive(Debug, Default)]
pub(crate) struct SurfaceBenchmarkSummary {
    trace_frames: usize,
    measured_frames: usize,
    presented_frames: usize,
    /// Size reported by the presenter after the latest successful native
    /// Surface presentation. This is an execution receipt, not an inference
    /// from the requested window size.
    presented_size: Option<(u32, u32)>,
    /// Kept in the artifact for compatibility; strict receipt draining never
    /// renders an extra frame, so this remains zero.
    drain_frames: usize,
    receipt_polls: usize,
    measured_cpu_frames: usize,
    measured_gpu_frames: usize,
    sort_refreshes: usize,
    gpu_fallback_frames: usize,
    cpu_preprocess_ms: Vec<f32>,
    cpu_sort_ms: Vec<f32>,
    pub(crate) cpu_completion_ms: Vec<f32>,
    frame_wall_ms: Vec<f32>,
    gpu_order_ms: Vec<f32>,
    gpu_completion_ms: Vec<f32>,
    gpu_timestamp_measurements: usize,
    gpu_completion_only_measurements: usize,
    gpu_incomplete_timestamp_measurements: usize,
    gpu_refreshes_without_ticket: usize,
    cpu_requests_without_ticket: usize,
    surface_unavailable_measurements: usize,
    pub(crate) outstanding_order_tickets: HashMap<u64, SurfaceBenchmarkTicket>,
    terminal_order_tickets: HashSet<u64>,
    pub(crate) outstanding_producer_tickets: HashMap<u64, SurfaceBenchmarkProducerTicket>,
    terminal_producer_tickets: HashSet<u64>,
    pub(crate) producer_completion_ms: Vec<f32>,
    pub(crate) producer_exact_frames: usize,
    producer_stale_frames: usize,
    producer_ring_busy: usize,
    producer_surface_unavailable: usize,
    throughput_first_measured_start: Option<Instant>,
    throughput_last_measured_start: Option<Instant>,
    throughput_measured_starts: usize,
    final_adaptive_state: SurfaceAdaptiveState,
    final_actual_backend: Option<SurfaceOrderBackendUsed>,
}

#[cfg(feature = "interactive-viewer")]
impl SurfaceBenchmarkSummary {
    fn record_output_state(&mut self, output: &SurfaceFrameOutput) {
        self.final_adaptive_state = output.adaptive_state;
        self.final_actual_backend = Some(output.order_backend);
    }

    fn record_trace_frame(
        &mut self,
        step: SurfaceTraceStep,
        output: &SurfaceFrameOutput,
        presented_size: Option<(u32, u32)>,
    ) {
        self.trace_frames += 1;
        self.presented_frames += usize::from(output.frame_presented);
        if output.frame_presented {
            self.presented_size = presented_size;
        }
        self.sort_refreshes += usize::from(output.sort_refreshed);
        self.gpu_fallback_frames += usize::from(output.gpu_sort_fallback);
        if !step.measured() {
            return;
        }
        self.measured_frames += 1;
        self.frame_wall_ms.push(output.timings.frame_wall_ms);
        match output.order_backend {
            SurfaceOrderBackendUsed::Cpu => {
                self.measured_cpu_frames += 1;
                self.cpu_preprocess_ms.push(output.stats.preprocess_ms);
                self.cpu_sort_ms.push(output.stats.sort_ms);
            }
            SurfaceOrderBackendUsed::Gpu => self.measured_gpu_frames += 1,
        }
    }

    fn record_throughput_start(&mut self, step: SurfaceTraceStep, started: Instant) {
        if !step.measured() {
            return;
        }
        self.throughput_first_measured_start.get_or_insert(started);
        self.throughput_last_measured_start = Some(started);
        self.throughput_measured_starts += 1;
    }

    fn surface_throughput_fps(&self) -> Option<f32> {
        let intervals = self.throughput_measured_starts.checked_sub(1)?;
        if intervals == 0 {
            return None;
        }
        let elapsed = self
            .throughput_last_measured_start?
            .duration_since(self.throughput_first_measured_start?)
            .as_secs_f32();
        (elapsed > 0.0).then_some(intervals as f32 / elapsed)
    }

    fn record_submission(
        &mut self,
        mode: SurfaceBenchmarkMode,
        measured: bool,
        output: &SurfaceFrameOutput,
    ) -> Result<(), String> {
        if output.submitted_measurement_ticket != output.order_measurement_submission.ticket() {
            return Err("Surface measurement submission compatibility fields disagree".to_owned());
        }
        match output.order_measurement_submission {
            SurfaceOrderMeasurementSubmission::NotRequested => Ok(()),
            SurfaceOrderMeasurementSubmission::Issued { backend, ticket } => {
                if backend != output.order_backend {
                    return Err(format!(
                        "Surface measurement ticket {ticket} says backend {backend:?}, but frame used {:?}",
                        output.order_backend
                    ));
                }
                if self.terminal_order_tickets.contains(&ticket)
                    || self
                        .outstanding_order_tickets
                        .insert(
                            ticket,
                            SurfaceBenchmarkTicket {
                                backend,
                                camera_revision: output.camera_revision,
                                measured,
                            },
                        )
                        .is_some()
                {
                    return Err(format!(
                        "Surface measurement ticket {ticket} was issued more than once"
                    ));
                }
                Ok(())
            }
            SurfaceOrderMeasurementSubmission::Unsampled { backend, reason } => {
                if backend != output.order_backend {
                    return Err(format!(
                        "unsampled Surface measurement says backend {backend:?}, but frame used {:?}",
                        output.order_backend
                    ));
                }
                match (backend, reason) {
                    (
                        SurfaceOrderBackendUsed::Gpu,
                        SurfaceOrderMeasurementUnsampledReason::RingBusy,
                    ) => self.gpu_refreshes_without_ticket += 1,
                    (
                        SurfaceOrderBackendUsed::Cpu,
                        SurfaceOrderMeasurementUnsampledReason::RingBusy,
                    ) => self.cpu_requests_without_ticket += 1,
                    (_, SurfaceOrderMeasurementUnsampledReason::SurfaceUnavailable) => {
                        self.surface_unavailable_measurements += 1;
                    }
                }
                match (mode, reason) {
                    (
                        SurfaceBenchmarkMode::Throughput,
                        SurfaceOrderMeasurementUnsampledReason::RingBusy,
                    ) => Ok(()),
                    _ => Err(format!(
                        "Surface {} benchmark could not issue a {backend:?} measurement ticket: {reason:?}",
                        mode.label()
                    )),
                }
            }
        }
    }

    fn record_producer_submission(
        &mut self,
        mode: SurfaceBenchmarkMode,
        expected_producer: Option<SurfaceGpuOrderProducer>,
        measured: bool,
        output: &SurfaceFrameOutput,
    ) -> Result<(), String> {
        if expected_producer.is_some()
            && output.order_backend == SurfaceOrderBackendUsed::Gpu
            && output.frame_presented
            && output.gpu_order_producer != expected_producer
        {
            return Err(format!(
                "GPU producer mismatch: expected {expected_producer:?}, actual {:?}",
                output.gpu_order_producer
            ));
        }
        if output.order_backend == SurfaceOrderBackendUsed::Cpu
            && output.gpu_order_producer.is_some()
        {
            return Err("CPU frame reported an actual GPU order producer".to_owned());
        }

        match output.gpu_producer_measurement_submission {
            SurfaceGpuProducerMeasurementSubmission::NotRequested => {
                if expected_producer.is_some()
                    && output.order_backend == SurfaceOrderBackendUsed::Gpu
                    && output.frame_presented
                {
                    return Err(
                        "presented GPU producer frame omitted its measurement submission"
                            .to_owned(),
                    );
                }
                Ok(())
            }
            SurfaceGpuProducerMeasurementSubmission::Issued { producer, ticket } => {
                if expected_producer != Some(producer)
                    || output.order_backend != SurfaceOrderBackendUsed::Gpu
                    || output.gpu_order_producer != Some(producer)
                {
                    return Err(format!(
                        "GPU producer ticket {ticket} is inconsistent with configured/actual producer"
                    ));
                }
                if self.terminal_producer_tickets.contains(&ticket)
                    || self
                        .outstanding_producer_tickets
                        .insert(
                            ticket,
                            SurfaceBenchmarkProducerTicket {
                                producer,
                                camera_revision: output.camera_revision,
                                measured,
                            },
                        )
                        .is_some()
                {
                    return Err(format!(
                        "GPU producer ticket {ticket} was issued more than once"
                    ));
                }
                Ok(())
            }
            SurfaceGpuProducerMeasurementSubmission::Unsampled { producer, reason } => {
                if expected_producer != Some(producer)
                    || output.order_backend != SurfaceOrderBackendUsed::Gpu
                    || output.gpu_order_producer != Some(producer)
                {
                    return Err(
                        "unsampled GPU producer receipt has inconsistent identity".to_owned()
                    );
                }
                match reason {
                    SurfaceGpuProducerMeasurementUnsampledReason::RingBusy => {
                        self.producer_ring_busy += 1;
                    }
                    SurfaceGpuProducerMeasurementUnsampledReason::SurfaceUnavailable => {
                        self.producer_surface_unavailable += 1;
                    }
                }
                match (mode, reason) {
                    (
                        SurfaceBenchmarkMode::Throughput,
                        SurfaceGpuProducerMeasurementUnsampledReason::RingBusy,
                    ) => Ok(()),
                    _ => Err(format!(
                        "Surface {} benchmark could not issue a {producer:?} producer ticket: {reason:?}",
                        mode.label()
                    )),
                }
            }
        }
    }

    fn take_producer_ticket(
        &mut self,
        ticket: u64,
        camera_revision: u64,
        producer: SurfaceGpuOrderProducer,
    ) -> Result<SurfaceBenchmarkProducerTicket, String> {
        if self.terminal_producer_tickets.contains(&ticket) {
            return Err(format!(
                "GPU producer ticket {ticket} produced more than one terminal receipt"
            ));
        }
        let context = self
            .outstanding_producer_tickets
            .remove(&ticket)
            .ok_or_else(|| format!("unknown GPU producer terminal ticket {ticket}"))?;
        if context.camera_revision != camera_revision || context.producer != producer {
            return Err(format!(
                "GPU producer ticket {ticket} identity mismatch: issued={context:?}, terminal=({producer:?}, revision {camera_revision})"
            ));
        }
        self.terminal_producer_tickets.insert(ticket);
        Ok(context)
    }

    pub(crate) fn record_producer_measurement(
        &mut self,
        measurement: SurfaceGpuProducerMeasurement,
        expected_source_count: usize,
    ) -> Result<bool, String> {
        let context = self.take_producer_ticket(
            measurement.ticket,
            measurement.camera_revision,
            measurement.producer,
        )?;
        if measurement.source_count as usize != expected_source_count
            || measurement.contributor_count > measurement.source_count
            || measurement.drawn_count > measurement.source_count
        {
            return Err(format!(
                "GPU producer ticket {} violates S/C/D bounds: expected S={}, got S={} C={} D={}",
                measurement.ticket,
                expected_source_count,
                measurement.source_count,
                measurement.contributor_count,
                measurement.drawn_count,
            ));
        }
        match measurement.draw_scope {
            SurfaceGpuProducerDrawScope::ExactCurrentContributors => {
                if !measurement.order_refreshed
                    || measurement.drawn_count != measurement.contributor_count
                {
                    return Err(format!(
                        "GPU producer ticket {} exact-current scope requires refreshed D=C",
                        measurement.ticket
                    ));
                }
                self.producer_exact_frames += 1;
            }
            SurfaceGpuProducerDrawScope::StaleOrderCandidates => {
                if measurement.order_refreshed {
                    return Err(format!(
                        "GPU producer ticket {} stale scope cannot report an order refresh",
                        measurement.ticket
                    ));
                }
                self.producer_stale_frames += 1;
            }
        }
        if !measurement.frame_complete_ms.is_finite() || measurement.frame_complete_ms < 0.0 {
            return Err(format!(
                "GPU producer ticket {} contains invalid completion timing",
                measurement.ticket
            ));
        }
        if context.measured {
            self.producer_completion_ms
                .push(measurement.frame_complete_ms);
        }
        Ok(context.measured)
    }

    fn record_producer_failure(
        &mut self,
        failure: SurfaceGpuProducerMeasurementFailure,
    ) -> Result<SurfaceBenchmarkProducerTicket, String> {
        self.take_producer_ticket(failure.ticket, failure.camera_revision, failure.producer)
    }

    fn take_terminal_ticket(
        &mut self,
        ticket: u64,
        camera_revision: u64,
        expected_backend: Option<SurfaceOrderBackendUsed>,
    ) -> Result<SurfaceBenchmarkTicket, String> {
        if self.terminal_order_tickets.contains(&ticket) {
            return Err(format!(
                "Surface measurement ticket {ticket} produced more than one terminal receipt"
            ));
        }
        let context = self
            .outstanding_order_tickets
            .remove(&ticket)
            .ok_or_else(|| format!("unknown Surface measurement terminal ticket {ticket}"))?;
        if context.camera_revision != camera_revision {
            return Err(format!(
                "Surface measurement ticket {ticket} revision mismatch: issued {}, terminal {camera_revision}",
                context.camera_revision
            ));
        }
        if expected_backend.is_some_and(|backend| backend != context.backend) {
            return Err(format!(
                "Surface measurement ticket {ticket} backend mismatch: issued {:?}, terminal {:?}",
                context.backend, expected_backend
            ));
        }
        self.terminal_order_tickets.insert(ticket);
        Ok(context)
    }

    pub(crate) fn record_cpu_measurement(
        &mut self,
        measurement: SurfaceCpuOrderMeasurement,
    ) -> Result<bool, String> {
        validate_surface_measurement_counts(
            measurement.visible_count,
            measurement.contributor_count,
            measurement.drawn_count,
            measurement.exact_contributor_compaction,
            &format!("CPU Surface measurement ticket {}", measurement.ticket),
        )?;
        if [
            measurement.preprocess_ms,
            measurement.sort_ms,
            measurement.frame_complete_ms,
        ]
        .into_iter()
        .any(|value| !value.is_finite() || value < 0.0)
        {
            return Err(format!(
                "CPU Surface measurement ticket {} contains invalid timing evidence",
                measurement.ticket
            ));
        }
        let context = self.take_terminal_ticket(
            measurement.ticket,
            measurement.camera_revision,
            Some(SurfaceOrderBackendUsed::Cpu),
        )?;
        if context.measured {
            self.cpu_completion_ms.push(measurement.frame_complete_ms);
        }
        Ok(context.measured)
    }

    fn record_gpu_measurement(
        &mut self,
        measurement: SurfaceOrderMeasurement,
    ) -> Result<bool, String> {
        validate_surface_measurement_counts(
            measurement.visible_count,
            measurement.contributor_count,
            measurement.drawn_count,
            measurement.exact_contributor_compaction,
            &format!("GPU Surface measurement ticket {}", measurement.ticket),
        )?;
        if !measurement.gpu_complete_ms.is_finite() || measurement.gpu_complete_ms < 0.0 {
            return Err(format!(
                "GPU Surface measurement ticket {} contains invalid completion timing",
                measurement.ticket
            ));
        }
        let context = self.take_terminal_ticket(
            measurement.ticket,
            measurement.camera_revision,
            Some(SurfaceOrderBackendUsed::Gpu),
        )?;
        match measurement.timing_source {
            SurfaceTimingSource::TimestampQuery => {
                self.gpu_timestamp_measurements += 1;
                self.gpu_incomplete_timestamp_measurements +=
                    usize::from(measurement.gpu_order_ms.is_none());
            }
            SurfaceTimingSource::CompletionOnly => self.gpu_completion_only_measurements += 1,
        }
        if context.measured {
            if let Some(order_ms) = measurement.gpu_order_ms {
                self.gpu_order_ms.push(order_ms);
            }
            self.gpu_completion_ms.push(measurement.gpu_complete_ms);
        }
        Ok(context.measured)
    }

    fn record_failure(
        &mut self,
        failure: SurfaceOrderMeasurementFailure,
    ) -> Result<SurfaceBenchmarkTicket, String> {
        self.take_terminal_ticket(failure.ticket, failure.camera_revision, None)
    }

    pub(crate) fn has_outstanding_tickets(&self) -> bool {
        !self.outstanding_order_tickets.is_empty() || !self.outstanding_producer_tickets.is_empty()
    }

    fn outstanding_ticket_count(&self, backend: SurfaceOrderBackendUsed) -> usize {
        self.outstanding_order_tickets
            .values()
            .filter(|ticket| ticket.backend == backend)
            .count()
    }

    fn outstanding_ticket_snapshot(&self) -> Vec<(u64, SurfaceOrderBackendUsed, u64)> {
        let mut tickets: Vec<_> = self
            .outstanding_order_tickets
            .iter()
            .map(|(&ticket, context)| (ticket, context.backend, context.camera_revision))
            .collect();
        tickets.sort_by_key(|entry| entry.0);
        tickets
    }

    fn outstanding_producer_ticket_snapshot(&self) -> Vec<(u64, SurfaceGpuOrderProducer, u64)> {
        let mut tickets: Vec<_> = self
            .outstanding_producer_tickets
            .iter()
            .map(|(&ticket, context)| (ticket, context.producer, context.camera_revision))
            .collect();
        tickets.sort_by_key(|entry| entry.0);
        tickets
    }
}

#[cfg(feature = "interactive-viewer")]
fn validate_surface_measurement_counts(
    visible_count: u32,
    contributor_count: u32,
    drawn_count: u32,
    exact_contributor_compaction: bool,
    context: &str,
) -> Result<(), String> {
    if contributor_count > visible_count {
        return Err(format!(
            "{context} violates C <= V: C={contributor_count}, V={visible_count}"
        ));
    }
    if exact_contributor_compaction {
        if drawn_count != contributor_count {
            return Err(format!(
                "{context} exact contributor execution requires D=C: D={drawn_count}, C={contributor_count}"
            ));
        }
    } else if drawn_count != visible_count {
        return Err(format!(
            "{context} direct/downlevel execution requires D=V: D={drawn_count}, V={visible_count}"
        ));
    }
    Ok(())
}

#[cfg(feature = "interactive-viewer")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SurfaceBenchmarkAction {
    RenderTraceStep,
    PollReceipts,
    Complete,
}

#[cfg(feature = "interactive-viewer")]
pub(crate) fn surface_benchmark_action(
    mode: SurfaceBenchmarkMode,
    next_step: usize,
    total_steps: usize,
    outstanding_tickets: bool,
    adaptive_pending: Option<SurfaceAdaptivePendingSample>,
) -> SurfaceBenchmarkAction {
    if next_step < total_steps && mode == SurfaceBenchmarkMode::Throughput {
        SurfaceBenchmarkAction::RenderTraceStep
    } else if outstanding_tickets || adaptive_pending.is_some() {
        SurfaceBenchmarkAction::PollReceipts
    } else if next_step < total_steps {
        SurfaceBenchmarkAction::RenderTraceStep
    } else {
        SurfaceBenchmarkAction::Complete
    }
}

#[cfg(feature = "interactive-viewer")]
#[allow(deprecated)] // See run_interactive; this remains a real winit Surface loop.
fn run_surface_trace_benchmark(
    args: &Args,
    event_loop: EventLoop<()>,
    window: Arc<winit::window::Window>,
    mut session: SurfaceRenderSession,
    playback: &CameraTracePlayback,
) -> Result<(), String> {
    if args.geometry_path == GeometryPath::PagedActiveAtlas {
        return Err(
            "paged geometry is excluded from the full-quality Surface benchmark because it does not keep the complete source resident"
                .to_owned(),
        );
    }
    let steps = surface_trace_steps(playback)?;
    let trace = playback.trace();
    let requested_backend = args.order_backend;
    let source_count = session
        .renderer()
        .scene_len()
        .ok_or_else(|| "surface benchmark scene is not loaded".to_owned())?;
    // Both accepted Surface paths allocate one addressable GPU record per
    // source. Packed exposes its independently built compact count here;
    // Direct's addressable set is the wide source itself.
    let resident_count = session
        .renderer()
        .resident_scene()
        .map_or(source_count, |scene| scene.len());
    if resident_count != source_count {
        return Err(format!(
            "full-quality Surface count mismatch: source={source_count}, resident={resident_count}"
        ));
    }
    let sh_degree = session.renderer().scene_sh_degree().unwrap_or(0);
    let expected_size = (trace.display.width, trace.display.height);
    let resolution = SurfaceResolutionReceipt::validated(
        expected_size,
        session.surface_size(),
        session.internal_render_size(),
    )?;
    let raster_execution_plan = session.raster_execution_plan();
    if args.geometry_path == GeometryPath::PackedAtlas
        && raster_execution_plan != SurfaceRasterExecutionPlan::ProjectedQuadsExact
    {
        return Err(format!(
            "full-quality Packed Surface benchmark requires ProjectedQuadsExact, got {raster_execution_plan:?}",
        ));
    }
    if let Some(requested_producer) = args.surface_gpu_producer {
        if session.gpu_order_producer() != requested_producer {
            return Err(format!(
                "GPU producer A/B requested {requested_producer:?}, got {:?}",
                session.gpu_order_producer()
            ));
        }
        if session.projected_draw_policy() != SurfaceProjectedDrawPolicy::Compact {
            return Err(format!(
                "GPU producer A/B requires forced Compact projected drawing, got {:?}",
                session.projected_draw_policy()
            ));
        }
    }
    let identity = SurfaceBenchmarkIdentity {
        trace_id: trace.trace_id.clone(),
        trace_sha256: trace.content_sha256.clone(),
        mode: args.surface_benchmark_mode,
        sort_policy: args.surface_sort_policy,
        requested_backend,
        geometry_path: args.geometry_path,
        raster_execution_plan,
        gpu_order_producer: args.surface_gpu_producer,
        projected_draw_policy: session.projected_draw_policy(),
        resolution,
        source_count,
        resident_count,
        sh_degree,
    };

    println!(
        "SURFACE_BENCHMARK_BEGIN trace_id={} trace_sha256={} benchmark_mode={} sort_policy={} requested_backend={} geometry_path={} raster_execution_plan={} gpu_order_producer={} projected_draw_policy={} producer_measurement_enabled={} sort_interval=1 requested_width={} requested_height={} surface_width={} surface_height={} internal_render_width={} internal_render_height={} dynamic_resolution=disabled upscaling=disabled full_resolution={} source_count={} resident_count={} sh_degree={} trace_frames={}",
        identity.trace_id,
        identity.trace_sha256,
        identity.mode.label(),
        identity.sort_policy.label(),
        order_backend_label(identity.requested_backend),
        geometry_path_label(identity.geometry_path),
        raster_execution_plan_label(identity.raster_execution_plan),
        identity
            .gpu_order_producer
            .map_or("product-default", gpu_order_producer_label),
        projected_draw_policy_label(identity.projected_draw_policy),
        identity.gpu_order_producer.is_some(),
        identity.resolution.requested.0,
        identity.resolution.requested.1,
        identity.resolution.surface.0,
        identity.resolution.surface.1,
        identity.resolution.internal_render.0,
        identity.resolution.internal_render.1,
        identity.resolution.full_resolution(),
        identity.source_count,
        identity.resident_count,
        identity.sh_degree,
        steps.len(),
    );

    let window_id = window.id();
    let render_error = Arc::new(Mutex::new(None::<String>));
    let render_error_shared = Arc::clone(&render_error);
    let mut next_step = 0_usize;
    let mut drain_started = None::<Instant>;
    let mut summary = SurfaceBenchmarkSummary::default();
    let mut benchmark_completed = false;

    event_loop
        .run(move |event, target| match event {
            Event::AboutToWait if !benchmark_completed => window.request_redraw(),
            Event::WindowEvent {
                window_id: id,
                event,
            } if id == window_id => match event {
                WindowEvent::CloseRequested => {
                    if surface_benchmark_action(
                        args.surface_benchmark_mode,
                        next_step,
                        steps.len(),
                        summary.has_outstanding_tickets(),
                        session.adaptive_pending_sample(),
                    ) != SurfaceBenchmarkAction::Complete
                    {
                        store_surface_benchmark_error(
                            &render_error_shared,
                            "surface benchmark window closed before completion".to_owned(),
                        );
                    }
                    target.exit();
                }
                WindowEvent::Resized(size)
                    if size.width != expected_size.0 || size.height != expected_size.1 =>
                {
                    store_surface_benchmark_error(
                        &render_error_shared,
                        format!(
                            "surface benchmark resize {}x{} violates trace display {}x{}",
                            size.width, size.height, expected_size.0, expected_size.1
                        ),
                    );
                    target.exit();
                }
                WindowEvent::RedrawRequested => {
                    if benchmark_completed {
                        return;
                    }
                    let adaptive_pending = session.adaptive_pending_sample();
                    let action = surface_benchmark_action(
                        args.surface_benchmark_mode,
                        next_step,
                        steps.len(),
                        summary.has_outstanding_tickets(),
                        adaptive_pending,
                    );
                    let trace_step = (action == SurfaceBenchmarkAction::RenderTraceStep)
                        .then(|| steps[next_step]);
                    let expected_sort_refresh = trace_step.is_some_and(|step| {
                        identity.sort_policy == SurfaceSortPolicyArg::EveryFrame
                            || step.playback_index == 0
                            || steps[step.playback_index - 1].camera != step.camera
                    });
                    if let Some(step) = trace_step {
                        if let Err(error) = session.set_camera(step.camera) {
                            store_surface_benchmark_error(
                                &render_error_shared,
                                format!("surface trace camera update failed: {error}"),
                            );
                            target.exit();
                            return;
                        }
                        if identity.sort_policy == SurfaceSortPolicyArg::EveryFrame {
                            // Dynamic-order experiments deliberately measure a
                            // fresh exact order even for a repeated trace pose.
                            session.force_sort_refresh();
                        }
                    } else if action == SurfaceBenchmarkAction::Complete {
                        if let Some(png_path) = args.png_out.as_deref() {
                            let requested_capture_producer =
                                match identity.gpu_order_producer {
                                    Some(producer) => producer,
                                    None => {
                                        store_surface_benchmark_error(
                                            &render_error_shared,
                                            "surface capture requires an explicit GPU producer identity"
                                                .to_owned(),
                                        );
                                        target.exit();
                                        return;
                                    }
                                };
                            let capture_step = steps.first().copied().ok_or_else(|| {
                                "surface capture requires a non-empty trace schedule".to_owned()
                            });
                            let capture_step = match capture_step {
                                Ok(step) => step,
                                Err(error) => {
                                    store_surface_benchmark_error(&render_error_shared, error);
                                    target.exit();
                                    return;
                                }
                            };
                            if let Err(error) = session.set_gpu_producer_measurement_enabled(false)
                            {
                                store_surface_benchmark_error(
                                    &render_error_shared,
                                    format!(
                                        "surface capture could not disable producer measurement: {error}"
                                    ),
                                );
                                target.exit();
                                return;
                            }
                            if let Err(error) =
                                session.set_order_backend(SurfaceOrderBackend::Gpu)
                            {
                                store_surface_benchmark_error(
                                    &render_error_shared,
                                    format!(
                                        "surface capture could not force the selected GPU producer: {error}"
                                    ),
                                );
                                target.exit();
                                return;
                            }
                            if let Err(error) = session.set_camera(capture_step.camera) {
                                store_surface_benchmark_error(
                                    &render_error_shared,
                                    format!("surface capture camera update failed: {error}"),
                                );
                                target.exit();
                                return;
                            }
                            session.force_sort_refresh();
                            if let Err(error) = session.request_surface_capture() {
                                store_surface_benchmark_error(
                                    &render_error_shared,
                                    format!("surface capture request failed: {error}"),
                                );
                                target.exit();
                                return;
                            }
                            let output = match session.render_frame() {
                                Ok(output) => output,
                                Err(error) => {
                                    store_surface_benchmark_error(
                                        &render_error_shared,
                                        format!("surface capture render failed: {error}"),
                                    );
                                    target.exit();
                                    return;
                                }
                            };
                            let actual_capture_producer =
                                match output.gpu_order_producer {
                                    Some(actual) if actual == requested_capture_producer => actual,
                                    actual => {
                                        store_surface_benchmark_error(
                                            &render_error_shared,
                                            format!(
                                                "surface capture GPU producer mismatch: requested={requested_capture_producer:?}, actual={actual:?}",
                                            ),
                                        );
                                        target.exit();
                                        return;
                                    }
                                };
                            if !output.frame_presented
                                || output.tiled_preparation_pending
                                || output.gpu_producer_measurement_submission
                                    != SurfaceGpuProducerMeasurementSubmission::NotRequested
                            {
                                store_surface_benchmark_error(
                                    &render_error_shared,
                                    format!(
                                        "surface capture frame was not an unmeasured complete presentation: presented={} preparation_pending={} producer_submission={:?}",
                                        output.frame_presented,
                                        output.tiled_preparation_pending,
                                        output.gpu_producer_measurement_submission,
                                    ),
                                );
                                target.exit();
                                return;
                            }
                            let capture = match session.take_surface_capture() {
                                Ok(capture) => capture,
                                Err(error) => {
                                    store_surface_benchmark_error(
                                        &render_error_shared,
                                        format!("surface capture readback failed: {error}"),
                                    );
                                    target.exit();
                                    return;
                                }
                            };
                            if (capture.width, capture.height) != expected_size
                                || capture.rgba8.len()
                                    != expected_size.0 as usize
                                        * expected_size.1 as usize
                                        * 4
                            {
                                store_surface_benchmark_error(
                                    &render_error_shared,
                                    format!(
                                        "surface capture dimensions/bytes mismatch: got {}x{} and {} bytes, expected {}x{} and {} bytes",
                                        capture.width,
                                        capture.height,
                                        capture.rgba8.len(),
                                        expected_size.0,
                                        expected_size.1,
                                        expected_size.0 as usize
                                            * expected_size.1 as usize
                                            * 4,
                                    ),
                                );
                                target.exit();
                                return;
                            }
                            let capture_ticket = output.order_measurement_submission.ticket();
                            if let Err(error) = drain_surface_capture_receipts(
                                &mut session,
                                &output,
                                identity.source_count,
                            ) {
                                store_surface_benchmark_error(&render_error_shared, error);
                                target.exit();
                                return;
                            }
                            if let Err(error) = write_png(
                                png_path,
                                capture.width,
                                capture.height,
                                &capture.rgba8,
                            ) {
                                store_surface_benchmark_error(&render_error_shared, error);
                                target.exit();
                                return;
                            }
                            println!(
                                "SURFACE_CAPTURE status=ok path={} trace_frame={} requested_width={} requested_height={} captured_width={} captured_height={} gpu_order_producer_requested={} gpu_order_producer_actual={} producer_measurement_enabled=false measured=false order_ticket={} terminal_receipt=success",
                                png_path.display(),
                                capture_step.trace_frame_index,
                                expected_size.0,
                                expected_size.1,
                                capture.width,
                                capture.height,
                                gpu_order_producer_label(requested_capture_producer),
                                gpu_order_producer_label(actual_capture_producer),
                                option_u64(capture_ticket),
                            );
                        }
                        print_surface_benchmark_summary(&identity, &summary);
                        benchmark_completed = true;
                        target.exit();
                        return;
                    }

                    if let Some(step) = trace_step {
                        let render_started = Instant::now();
                        if args.surface_benchmark_mode == SurfaceBenchmarkMode::Throughput {
                            summary.record_throughput_start(step, render_started);
                        }
                        let output = match session.render_frame() {
                            Ok(output) => output,
                            Err(error) => {
                                store_surface_benchmark_error(
                                    &render_error_shared,
                                    format!("surface benchmark render failed: {error}"),
                                );
                                target.exit();
                                return;
                            }
                        };
                        if !output.frame_presented || output.tiled_preparation_pending {
                            store_surface_benchmark_error(
                                &render_error_shared,
                                format!(
                                    "surface trace frame {} did not present its full-resolution drawable (frame_presented={}, tiled_preparation_pending={})",
                                    step.playback_index,
                                    output.frame_presented,
                                    output.tiled_preparation_pending,
                                ),
                            );
                            target.exit();
                            return;
                        }
                        if output.raster_execution_plan != identity.raster_execution_plan {
                            store_surface_benchmark_error(
                                &render_error_shared,
                                format!(
                                    "surface trace frame {} changed raster plan from {:?} to {:?}",
                                    step.playback_index,
                                    identity.raster_execution_plan,
                                    output.raster_execution_plan,
                                ),
                            );
                            target.exit();
                            return;
                        }
                        let presented_size = session.last_presented_size();
                        if presented_size != Some(expected_size) {
                            store_surface_benchmark_error(
                                &render_error_shared,
                                format!(
                                    "surface trace frame {} presented size {:?}, expected {}x{}",
                                    step.playback_index,
                                    presented_size,
                                    expected_size.0,
                                    expected_size.1,
                                ),
                            );
                            target.exit();
                            return;
                        }
                        summary.record_output_state(&output);
                        if let Err(error) = summary.record_submission(
                            args.surface_benchmark_mode,
                            step.measured(),
                            &output,
                        ) {
                            store_surface_benchmark_error(&render_error_shared, error);
                            target.exit();
                            return;
                        }
                        if let Err(error) = summary.record_producer_submission(
                            args.surface_benchmark_mode,
                            identity.gpu_order_producer,
                            step.measured(),
                            &output,
                        ) {
                            store_surface_benchmark_error(&render_error_shared, error);
                            target.exit();
                            return;
                        }
                        if output.sort_refreshed != expected_sort_refresh {
                            store_surface_benchmark_error(
                                &render_error_shared,
                                format!(
                                    "surface trace playback {} sort refresh mismatch: expected={}, actual={}",
                                    step.playback_index,
                                    expected_sort_refresh,
                                    output.sort_refreshed,
                                ),
                            );
                            target.exit();
                            return;
                        }
                        print_surface_frame_receipt(
                            &identity,
                            Some(step),
                            summary.drain_frames,
                            presented_size,
                            &output,
                        );
                        summary.record_trace_frame(step, &output, presented_size);
                        next_step += 1;
                    } else {
                        // A ticket drain is deliberately not another frame:
                        // submitting reuse draws here would contaminate the
                        // next isolated queue-completion sample with backlog.
                        summary.receipt_polls += 1;
                        session.poll_order_measurement_receipts();
                        summary.final_adaptive_state = session.adaptive_state();
                        // Completion timestamps are captured by the callback,
                        // so a short poll cadence does not alter the measured
                        // interval and avoids a CPU-burning busy loop.
                        std::thread::sleep(Duration::from_millis(1));
                    }

                    for measurement in session.drain_cpu_order_measurements() {
                        if measurement.visible_count as usize > identity.source_count {
                            store_surface_benchmark_error(
                                &render_error_shared,
                                format!(
                                    "CPU measurement ticket {} violates V <= S: V={}, S={}",
                                    measurement.ticket, measurement.visible_count, identity.source_count
                                ),
                            );
                            target.exit();
                            return;
                        }
                        let was_measured = match summary.record_cpu_measurement(measurement) {
                            Ok(measured) => measured,
                            Err(error) => {
                                store_surface_benchmark_error(&render_error_shared, error);
                                target.exit();
                                return;
                            }
                        };
                        print_surface_cpu_measurement(measurement, was_measured);
                    }

                    for measurement in session.drain_order_measurements() {
                        if measurement.visible_count as usize > identity.source_count {
                            store_surface_benchmark_error(
                                &render_error_shared,
                                format!(
                                    "GPU measurement ticket {} violates V <= S: V={}, S={}",
                                    measurement.ticket, measurement.visible_count, identity.source_count
                                ),
                            );
                            target.exit();
                            return;
                        }
                        let was_measured = match summary.record_gpu_measurement(measurement) {
                            Ok(measured) => measured,
                            Err(error) => {
                                store_surface_benchmark_error(&render_error_shared, error);
                                target.exit();
                                return;
                            }
                        };
                        print_surface_gpu_measurement(measurement, was_measured);
                    }

                    for measurement in session.drain_gpu_producer_measurements() {
                        let was_measured = match summary
                            .record_producer_measurement(measurement, identity.source_count)
                        {
                            Ok(measured) => measured,
                            Err(error) => {
                                store_surface_benchmark_error(&render_error_shared, error);
                                target.exit();
                                return;
                            }
                        };
                        print_surface_gpu_producer_measurement(measurement, was_measured);
                    }

                    let mut terminal_failures = Vec::new();
                    for failure in session.drain_order_measurement_failures() {
                        let context = match summary.record_failure(failure) {
                            Ok(context) => context,
                            Err(error) => {
                                store_surface_benchmark_error(&render_error_shared, error);
                                target.exit();
                                return;
                            }
                        };
                        terminal_failures.push(format!(
                            "ticket={} revision={} backend={:?} reason={:?}",
                            failure.ticket, failure.camera_revision, context.backend, failure.reason
                        ));
                    }
                    for failure in session.drain_gpu_producer_measurement_failures() {
                        let context = match summary.record_producer_failure(failure) {
                            Ok(context) => context,
                            Err(error) => {
                                store_surface_benchmark_error(&render_error_shared, error);
                                target.exit();
                                return;
                            }
                        };
                        terminal_failures.push(format!(
                            "producer_ticket={} revision={} producer={:?} issued_producer={:?} reason={:?}",
                            failure.ticket,
                            failure.camera_revision,
                            failure.producer,
                            context.producer,
                            failure.reason
                        ));
                    }
                    if !terminal_failures.is_empty() {
                        store_surface_benchmark_error(
                            &render_error_shared,
                            format!(
                                "Surface measurement terminal failure(s): {}",
                                terminal_failures.join(", ")
                            ),
                        );
                        target.exit();
                        return;
                    }

                    let adaptive_pending = session.adaptive_pending_sample();
                    let waiting_for_receipts =
                        (summary.has_outstanding_tickets() || adaptive_pending.is_some())
                            && (args.surface_benchmark_mode == SurfaceBenchmarkMode::Isolated
                                || next_step >= steps.len());
                    if !waiting_for_receipts {
                        drain_started = None;
                    } else {
                        let started = drain_started.get_or_insert_with(Instant::now);
                        if started.elapsed() > SURFACE_TELEMETRY_DRAIN_TIMEOUT {
                            store_surface_benchmark_error(
                                &render_error_shared,
                                format!(
                                    "Surface telemetry did not complete within {:?}; outstanding_order_tickets={:?} outstanding_producer_tickets={:?} adaptive_pending={adaptive_pending:?}",
                                    SURFACE_TELEMETRY_DRAIN_TIMEOUT,
                                    summary.outstanding_ticket_snapshot(),
                                    summary.outstanding_producer_ticket_snapshot(),
                                ),
                            );
                            target.exit();
                        }
                    }
                }
                _ => {}
            },
            _ => {}
        })
        .map_err(|error| format!("surface benchmark event loop failed: {error}"))?;

    if let Some(error) = render_error
        .lock()
        .map_err(|_| "surface benchmark error state lock poisoned".to_owned())?
        .take()
    {
        return Err(error);
    }
    Ok(())
}

#[cfg(feature = "interactive-viewer")]
fn store_surface_benchmark_error(slot: &Mutex<Option<String>>, error: String) {
    if let Ok(mut slot) = slot.lock() {
        *slot = Some(error);
    }
}

#[cfg(feature = "interactive-viewer")]
fn drain_surface_capture_receipts(
    session: &mut SurfaceRenderSession,
    output: &SurfaceFrameOutput,
    source_count: usize,
) -> Result<(), String> {
    let (expected_backend, expected_ticket) = match output.order_measurement_submission {
        SurfaceOrderMeasurementSubmission::Issued { backend, ticket } => (backend, ticket),
        SurfaceOrderMeasurementSubmission::NotRequested => {
            return Err("forced-refresh surface capture did not issue an order ticket".to_owned());
        }
        SurfaceOrderMeasurementSubmission::Unsampled { backend, reason } => {
            return Err(format!(
                "forced-refresh surface capture could not issue its {backend:?} order ticket: {reason:?}"
            ));
        }
    };
    if expected_backend != SurfaceOrderBackendUsed::Gpu {
        return Err(format!(
            "surface producer capture requires a GPU terminal ticket, got {expected_backend:?}"
        ));
    }

    let deadline = Instant::now() + SURFACE_TELEMETRY_DRAIN_TIMEOUT;
    loop {
        session.poll_order_measurement_receipts();
        let cpu = session.drain_cpu_order_measurements();
        let projected = session.drain_projected_draw_measurements();
        let producer = session.drain_gpu_producer_measurements();
        let order_failures = session.drain_order_measurement_failures();
        let projected_failures = session.drain_projected_draw_measurement_failures();
        let producer_failures = session.drain_gpu_producer_measurement_failures();
        if !cpu.is_empty() || !projected.is_empty() || !producer.is_empty() {
            return Err(format!(
                "unmeasured surface capture produced unrelated terminal evidence: cpu={} projected={} producer={}",
                cpu.len(),
                projected.len(),
                producer.len(),
            ));
        }
        if !order_failures.is_empty()
            || !projected_failures.is_empty()
            || !producer_failures.is_empty()
        {
            return Err(format!(
                "unmeasured surface capture produced a terminal failure: order={order_failures:?} projected={projected_failures:?} producer={producer_failures:?}"
            ));
        }

        let completed = session.drain_order_measurements();
        if !completed.is_empty() {
            if completed.len() != 1 {
                return Err(format!(
                    "surface capture expected one terminal order receipt, got {}",
                    completed.len()
                ));
            }
            let measurement = completed[0];
            if measurement.ticket != expected_ticket
                || measurement.camera_revision != output.camera_revision
            {
                return Err(format!(
                    "surface capture terminal identity mismatch: expected ticket={} revision={}, got ticket={} revision={}",
                    expected_ticket,
                    output.camera_revision,
                    measurement.ticket,
                    measurement.camera_revision,
                ));
            }
            if measurement.visible_count as usize > source_count {
                return Err(format!(
                    "surface capture terminal count exceeds S: V={} S={source_count}",
                    measurement.visible_count
                ));
            }
            validate_surface_measurement_counts(
                measurement.visible_count,
                measurement.contributor_count,
                measurement.drawn_count,
                measurement.exact_contributor_compaction,
                "unmeasured Surface capture terminal",
            )?;
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "surface capture order ticket {expected_ticket} did not produce a terminal receipt within {:?}",
                SURFACE_TELEMETRY_DRAIN_TIMEOUT
            ));
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[cfg(feature = "interactive-viewer")]
fn print_surface_frame_receipt(
    identity: &SurfaceBenchmarkIdentity,
    step: Option<SurfaceTraceStep>,
    drain_frame: usize,
    presented_size: Option<(u32, u32)>,
    output: &SurfaceFrameOutput,
) {
    let completed = output.completed_order_measurement;
    let submitted_measurement_backend = output
        .order_measurement_submission
        .backend()
        .map_or("none", order_backend_used_label);
    let submitted_gpu_ticket = match output.order_measurement_submission {
        SurfaceOrderMeasurementSubmission::Issued {
            backend: SurfaceOrderBackendUsed::Gpu,
            ticket,
        } => Some(ticket),
        _ => None,
    };
    let unsampled_reason = match output.order_measurement_submission {
        SurfaceOrderMeasurementSubmission::Unsampled { reason, .. } => {
            measurement_unsampled_reason_label(reason)
        }
        _ => "none",
    };
    let producer_ticket = output.gpu_producer_measurement_submission.ticket();
    let producer_unsampled_reason = match output.gpu_producer_measurement_submission {
        SurfaceGpuProducerMeasurementSubmission::Unsampled { reason, .. } => {
            gpu_producer_unsampled_reason_label(reason)
        }
        _ => "none",
    };
    let phase = step.map_or("drain", |step| step.phase.as_str());
    println!(
        "SURFACE_FRAME_RECEIPT playback_index={} phase={} loop={} phase_frame={} measured_sample={} trace_frame={} trace_timestamp_ns={} drain_frame={} camera_revision={} applied_order_revision={} presented_order_revision_lag={} sort_policy={} requested_backend={} actual_backend={} adaptive_state={} raster_execution_plan={} gpu_order_producer_requested={} gpu_order_producer_actual={} producer_ticket_submitted={} producer_unsampled_reason={} frame_presented={} tiled_preparation_pending={} requested_width={} requested_height={} surface_width={} surface_height={} internal_render_width={} internal_render_height={} presented_width={} presented_height={} dynamic_resolution=disabled upscaling=disabled full_resolution={} sort_refreshed={} order_uploaded={} gpu_sort_fallback={} source_count={} resident_count={} visible_count={} drawn_count={} visible_count_revision={} visible_count_pending={} pending_camera_revision={} cpu_preprocess_ms={:.6} cpu_sort_ms={:.6} cpu_render_submit_ms={:.6} frame_wall_ms={:.6} measurement_ticket_submitted={} measurement_backend={} measurement_unsampled_reason={} gpu_ticket_submitted={} gpu_timestamp_query_enabled={} gpu_ticket_completed={} gpu_completed_revision={} gpu_timing_source={} gpu_preprocess_ms={} gpu_radix_ms={} gpu_order_ms={} gpu_completion_ms={} gpu_timestamp_period_ns={} gpu_below_timestamp_resolution={}",
        option_usize(step.map(|step| step.playback_index)),
        phase,
        option_usize(step.map(|step| step.loop_index)),
        option_usize(step.map(|step| step.phase_frame_index)),
        option_usize(step.and_then(|step| step.measured_sample_index)),
        option_usize(step.map(|step| step.trace_frame_index)),
        option_u64(step.map(|step| step.timestamp_ns)),
        drain_frame,
        output.camera_revision,
        output.applied_order_revision,
        output.presented_order_revision_lag,
        identity.sort_policy.label(),
        order_backend_label(identity.requested_backend),
        order_backend_used_label(output.order_backend),
        adaptive_state_label(output.adaptive_state),
        raster_execution_plan_label(output.raster_execution_plan),
        identity
            .gpu_order_producer
            .map_or("product-default", gpu_order_producer_label),
        output
            .gpu_order_producer
            .map_or("none", gpu_order_producer_label),
        option_u64(producer_ticket),
        producer_unsampled_reason,
        output.frame_presented,
        output.tiled_preparation_pending,
        identity.resolution.requested.0,
        identity.resolution.requested.1,
        identity.resolution.surface.0,
        identity.resolution.surface.1,
        identity.resolution.internal_render.0,
        identity.resolution.internal_render.1,
        option_u32(presented_size.map(|size| size.0)),
        option_u32(presented_size.map(|size| size.1)),
        output.frame_presented
            && presented_size == Some(identity.resolution.requested)
            && identity.resolution.full_resolution(),
        output.sort_refreshed,
        output.order_uploaded,
        output.gpu_sort_fallback,
        identity.source_count,
        identity.resident_count,
        output.stats.visible_count,
        output.stats.drawn_count,
        option_u64(output.visible_count_revision),
        output.visible_count_pending,
        option_u64(
            output
                .visible_count_pending
                .then_some(output.camera_revision)
        ),
        output.stats.preprocess_ms,
        output.stats.sort_ms,
        output.timings.render_submit_ms,
        output.timings.frame_wall_ms,
        option_u64(output.submitted_measurement_ticket),
        submitted_measurement_backend,
        unsampled_reason,
        option_u64(submitted_gpu_ticket),
        output.gpu_timestamp_queries_enabled,
        option_u64(completed.map(|measurement| measurement.ticket)),
        option_u64(completed.map(|measurement| measurement.camera_revision)),
        completed.map_or("none", |measurement| timing_source_label(
            measurement.timing_source
        )),
        option_f32(completed.and_then(|measurement| measurement.gpu_preprocess_ms)),
        option_f32(completed.and_then(|measurement| measurement.gpu_radix_ms)),
        option_f32(completed.and_then(|measurement| measurement.gpu_order_ms)),
        option_f32(completed.map(|measurement| measurement.gpu_complete_ms)),
        option_f32(completed.and_then(|measurement| measurement.timestamp_period_ns)),
        completed.is_some_and(|measurement| measurement.below_timestamp_resolution),
    );
}

#[cfg(feature = "interactive-viewer")]
fn print_surface_cpu_measurement(measurement: SurfaceCpuOrderMeasurement, measured: bool) {
    println!(
        "SURFACE_CPU_MEASUREMENT ticket={} camera_revision={} measured={} cpu_preprocess_ms={:.6} cpu_sort_ms={:.6} frame_completion_ms={:.6} count_semantics=candidate_visible_contributor_issued_v1 visible_count={} contributor_count={} drawn_count={} exact_contributor_compaction={}",
        measurement.ticket,
        measurement.camera_revision,
        measured,
        measurement.preprocess_ms,
        measurement.sort_ms,
        measurement.frame_complete_ms,
        measurement.visible_count,
        measurement.contributor_count,
        measurement.drawn_count,
        measurement.exact_contributor_compaction,
    );
}

#[cfg(feature = "interactive-viewer")]
fn print_surface_gpu_measurement(measurement: SurfaceOrderMeasurement, measured: bool) {
    println!(
        "SURFACE_GPU_MEASUREMENT ticket={} camera_revision={} measured={} timing_source={} gpu_preprocess_ms={} gpu_radix_ms={} gpu_order_ms={} gpu_completion_ms={:.6} timestamp_period_ns={} below_timestamp_resolution={} count_semantics=candidate_visible_contributor_issued_v1 visible_count={} contributor_count={} drawn_count={} exact_contributor_compaction={}",
        measurement.ticket,
        measurement.camera_revision,
        measured,
        timing_source_label(measurement.timing_source),
        option_f32(measurement.gpu_preprocess_ms),
        option_f32(measurement.gpu_radix_ms),
        option_f32(measurement.gpu_order_ms),
        measurement.gpu_complete_ms,
        option_f32(measurement.timestamp_period_ns),
        measurement.below_timestamp_resolution,
        measurement.visible_count,
        measurement.contributor_count,
        measurement.drawn_count,
        measurement.exact_contributor_compaction,
    );
}

#[cfg(feature = "interactive-viewer")]
fn print_surface_gpu_producer_measurement(
    measurement: SurfaceGpuProducerMeasurement,
    measured: bool,
) {
    println!(
        "SURFACE_GPU_PRODUCER_MEASUREMENT ticket={} camera_revision={} measured={} producer={} order_generation={} projection_generation={} source_count={} contributor_count={} drawn_count={} order_refreshed={} draw_scope={} exact_current_contributor_draw={} stale_order={} frame_completion_ms={:.6}",
        measurement.ticket,
        measurement.camera_revision,
        measured,
        gpu_order_producer_label(measurement.producer),
        measurement.order_generation,
        measurement.projection_generation,
        measurement.source_count,
        measurement.contributor_count,
        measurement.drawn_count,
        measurement.order_refreshed,
        gpu_producer_draw_scope_label(measurement.draw_scope),
        measurement.exact_current_contributor_draw(),
        measurement.stale_order(),
        measurement.frame_complete_ms,
    );
}

#[cfg(feature = "interactive-viewer")]
fn print_surface_benchmark_summary(
    identity: &SurfaceBenchmarkIdentity,
    summary: &SurfaceBenchmarkSummary,
) {
    println!(
        "SURFACE_BENCHMARK_SUMMARY status=ok trace_id={} trace_sha256={} benchmark_mode={} sort_policy={} requested_backend={} geometry_path={} raster_execution_plan={} gpu_order_producer={} projected_draw_policy={} producer_measurement_enabled={} requested_width={} requested_height={} surface_width={} surface_height={} internal_render_width={} internal_render_height={} presented_width={} presented_height={} dynamic_resolution=disabled upscaling=disabled full_resolution={} final_actual_backend={} final_adaptive_state={} source_count={} resident_count={} sh_degree={} trace_frames={} presented_frames={} measured_frames={} drain_frames={} receipt_polls={} measured_cpu_frames={} measured_gpu_frames={} sort_refreshes={} gpu_fallback_frames={} mean_cpu_preprocess_ms={} mean_cpu_sort_ms={} mean_cpu_completion_ms={} mean_frame_wall_ms={} surface_throughput_fps={} gpu_timestamp_measurements={} gpu_completion_only_measurements={} gpu_incomplete_timestamp_measurements={} gpu_refreshes_without_ticket={} cpu_requests_without_ticket={} surface_unavailable_measurements={} mean_gpu_order_ms={} mean_gpu_completion_ms={} mean_gpu_producer_completion_ms={} producer_exact_frames={} producer_stale_frames={} producer_ring_busy={} producer_surface_unavailable={} terminal_order_tickets={} terminal_producer_tickets={} outstanding_cpu_tickets={} outstanding_gpu_tickets={} outstanding_producer_tickets={}",
        identity.trace_id,
        identity.trace_sha256,
        identity.mode.label(),
        identity.sort_policy.label(),
        order_backend_label(identity.requested_backend),
        geometry_path_label(identity.geometry_path),
        raster_execution_plan_label(identity.raster_execution_plan),
        identity
            .gpu_order_producer
            .map_or("product-default", gpu_order_producer_label),
        projected_draw_policy_label(identity.projected_draw_policy),
        identity.gpu_order_producer.is_some(),
        identity.resolution.requested.0,
        identity.resolution.requested.1,
        identity.resolution.surface.0,
        identity.resolution.surface.1,
        identity.resolution.internal_render.0,
        identity.resolution.internal_render.1,
        option_u32(summary.presented_size.map(|size| size.0)),
        option_u32(summary.presented_size.map(|size| size.1)),
        summary.presented_frames == summary.trace_frames
            && summary.presented_frames > 0
            && summary.presented_size == Some(identity.resolution.requested)
            && identity.resolution.full_resolution(),
        summary
            .final_actual_backend
            .map_or("none", order_backend_used_label),
        adaptive_state_label(summary.final_adaptive_state),
        identity.source_count,
        identity.resident_count,
        identity.sh_degree,
        summary.trace_frames,
        summary.presented_frames,
        summary.measured_frames,
        summary.drain_frames,
        summary.receipt_polls,
        summary.measured_cpu_frames,
        summary.measured_gpu_frames,
        summary.sort_refreshes,
        summary.gpu_fallback_frames,
        option_f32(mean(&summary.cpu_preprocess_ms)),
        option_f32(mean(&summary.cpu_sort_ms)),
        option_f32(mean(&summary.cpu_completion_ms)),
        option_f32(mean(&summary.frame_wall_ms)),
        option_f32(summary.surface_throughput_fps()),
        summary.gpu_timestamp_measurements,
        summary.gpu_completion_only_measurements,
        summary.gpu_incomplete_timestamp_measurements,
        summary.gpu_refreshes_without_ticket,
        summary.cpu_requests_without_ticket,
        summary.surface_unavailable_measurements,
        option_f32(mean(&summary.gpu_order_ms)),
        option_f32(mean(&summary.gpu_completion_ms)),
        option_f32(mean(&summary.producer_completion_ms)),
        summary.producer_exact_frames,
        summary.producer_stale_frames,
        summary.producer_ring_busy,
        summary.producer_surface_unavailable,
        summary.terminal_order_tickets.len(),
        summary.terminal_producer_tickets.len(),
        summary.outstanding_ticket_count(SurfaceOrderBackendUsed::Cpu),
        summary.outstanding_ticket_count(SurfaceOrderBackendUsed::Gpu),
        summary.outstanding_producer_tickets.len(),
    );
}

#[cfg(feature = "interactive-viewer")]
fn mean(values: &[f32]) -> Option<f32> {
    (!values.is_empty()).then(|| values.iter().copied().sum::<f32>() / values.len() as f32)
}

#[cfg(feature = "interactive-viewer")]
fn option_f32(value: Option<f32>) -> String {
    value.map_or_else(|| "none".to_owned(), |value| format!("{value:.6}"))
}

#[cfg(feature = "interactive-viewer")]
fn option_u64(value: Option<u64>) -> String {
    value.map_or_else(|| "none".to_owned(), |value| value.to_string())
}

#[cfg(feature = "interactive-viewer")]
fn option_u32(value: Option<u32>) -> String {
    value.map_or_else(|| "none".to_owned(), |value| value.to_string())
}

#[cfg(feature = "interactive-viewer")]
fn option_usize(value: Option<usize>) -> String {
    value.map_or_else(|| "none".to_owned(), |value| value.to_string())
}

#[cfg(feature = "interactive-viewer")]
const fn order_backend_label(backend: SurfaceOrderBackend) -> &'static str {
    match backend {
        SurfaceOrderBackend::Cpu => "cpu",
        SurfaceOrderBackend::Gpu => "gpu",
        SurfaceOrderBackend::Adaptive => "adaptive",
    }
}

#[cfg(feature = "interactive-viewer")]
const fn order_backend_used_label(backend: SurfaceOrderBackendUsed) -> &'static str {
    match backend {
        SurfaceOrderBackendUsed::Cpu => "cpu",
        SurfaceOrderBackendUsed::Gpu => "gpu",
    }
}

#[cfg(feature = "interactive-viewer")]
const fn raster_execution_plan_label(plan: SurfaceRasterExecutionPlan) -> &'static str {
    match plan {
        SurfaceRasterExecutionPlan::GlobalQuads => "global_quads",
        SurfaceRasterExecutionPlan::ProjectedQuadsExact => "projected_quads_exact",
        SurfaceRasterExecutionPlan::TiledExact => "tiled_exact",
    }
}

#[cfg(feature = "interactive-viewer")]
const fn gpu_order_producer_label(producer: SurfaceGpuOrderProducer) -> &'static str {
    match producer {
        SurfaceGpuOrderProducer::PostSort => "post_sort",
        SurfaceGpuOrderProducer::Preproject => "preproject",
    }
}

#[cfg(feature = "interactive-viewer")]
const fn projected_draw_policy_label(policy: SurfaceProjectedDrawPolicy) -> &'static str {
    match policy {
        SurfaceProjectedDrawPolicy::Candidate => "candidate",
        SurfaceProjectedDrawPolicy::Compact => "compact",
        SurfaceProjectedDrawPolicy::Adaptive => "adaptive",
    }
}

#[cfg(feature = "interactive-viewer")]
const fn gpu_producer_draw_scope_label(scope: SurfaceGpuProducerDrawScope) -> &'static str {
    match scope {
        SurfaceGpuProducerDrawScope::ExactCurrentContributors => "exact_current_contributors",
        SurfaceGpuProducerDrawScope::StaleOrderCandidates => "stale_order_candidates",
    }
}

#[cfg(feature = "interactive-viewer")]
const fn gpu_producer_unsampled_reason_label(
    reason: SurfaceGpuProducerMeasurementUnsampledReason,
) -> &'static str {
    match reason {
        SurfaceGpuProducerMeasurementUnsampledReason::RingBusy => "ring_busy",
        SurfaceGpuProducerMeasurementUnsampledReason::SurfaceUnavailable => "surface_unavailable",
    }
}

#[cfg(feature = "interactive-viewer")]
const fn measurement_unsampled_reason_label(
    reason: SurfaceOrderMeasurementUnsampledReason,
) -> &'static str {
    match reason {
        SurfaceOrderMeasurementUnsampledReason::RingBusy => "ring_busy",
        SurfaceOrderMeasurementUnsampledReason::SurfaceUnavailable => "surface_unavailable",
    }
}

#[cfg(feature = "interactive-viewer")]
const fn adaptive_state_label(state: SurfaceAdaptiveState) -> &'static str {
    match state {
        SurfaceAdaptiveState::Disabled => "disabled",
        SurfaceAdaptiveState::CpuLearning => "cpu_learning",
        SurfaceAdaptiveState::CpuStable => "cpu_stable",
        SurfaceAdaptiveState::GpuProbe => "gpu_probe",
        SurfaceAdaptiveState::GpuStable => "gpu_stable",
        SurfaceAdaptiveState::CpuProbe => "cpu_probe",
        SurfaceAdaptiveState::Cooldown => "cooldown",
    }
}

#[cfg(feature = "interactive-viewer")]
const fn timing_source_label(source: SurfaceTimingSource) -> &'static str {
    match source {
        SurfaceTimingSource::TimestampQuery => "timestamp_query",
        SurfaceTimingSource::CompletionOnly => "completion_only",
    }
}

#[cfg(not(feature = "interactive-viewer"))]
fn run_interactive(
    _args: &Args,
    _renderer: Renderer,
    _camera: Camera,
    _trace_playback: Option<&CameraTracePlayback>,
) -> Result<(), String> {
    Err("interactive mode is not enabled. re-run with `--features interactive-viewer`".to_owned())
}

#[cfg(feature = "interactive-viewer")]
#[derive(Debug, Default, Clone)]
struct InputState {
    keys_down: HashSet<KeyCode>,
    mouse_left_down: bool,
    last_cursor: Option<(f32, f32)>,
    mouse_delta: (f32, f32),
    scroll_y: f32,
}

#[cfg(feature = "interactive-viewer")]
impl InputState {
    fn is_key_down(&self, key: KeyCode) -> bool {
        self.keys_down.contains(&key)
    }

    fn end_frame(&mut self) {
        self.mouse_delta = (0.0, 0.0);
        self.scroll_y = 0.0;
    }
}

#[cfg(feature = "interactive-viewer")]
#[derive(Debug, Clone)]
struct CameraController {
    position: Vec3f,
    target: Vec3f,
    distance: f32,
    intrinsics: CameraIntrinsics,
    yaw: f32,
    pitch: f32,
    move_speed: f32,
    look_speed: f32,
    mouse_sensitivity: f32,
}

#[cfg(feature = "interactive-viewer")]
impl CameraController {
    fn new(camera: Camera, target: Vec3f) -> Self {
        let to_target = Vec3f::new(
            target.x - camera.pose.position.x,
            target.y - camera.pose.position.y,
            target.z - camera.pose.position.z,
        );
        let mut distance =
            (to_target.x * to_target.x + to_target.y * to_target.y + to_target.z * to_target.z)
                .sqrt();
        if !distance.is_finite() || distance < 0.05 {
            distance = 1.0;
        }
        let forward = if distance > 1e-6 {
            Vec3f::new(
                to_target.x / distance,
                to_target.y / distance,
                to_target.z / distance,
            )
        } else {
            quat_rotate_vec3(camera.pose.rotation_xyzw, Vec3f::new(0.0, 0.0, 1.0))
        };
        let mut controller = Self {
            position: camera.pose.position,
            target,
            distance,
            intrinsics: camera.intrinsics,
            yaw: forward.x.atan2(forward.z),
            pitch: (-forward.y).asin().clamp(-1.45, 1.45),
            move_speed: 2.0,
            look_speed: 1.8,
            mouse_sensitivity: 0.003,
        };
        controller.sync_position_from_orbit();
        controller
    }

    fn set_yaw(&mut self, yaw: f32) {
        self.yaw = yaw;
        self.sync_position_from_orbit();
    }

    fn camera(&self) -> Camera {
        Camera {
            pose: CameraPose {
                position: self.position,
                rotation_xyzw: quat_from_yaw_pitch(self.yaw, self.pitch),
            },
            intrinsics: self.intrinsics,
        }
    }

    fn update(&mut self, input: &InputState, dt: f32) {
        let mut yaw_delta = 0.0_f32;
        let mut pitch_delta = 0.0_f32;
        if input.is_key_down(KeyCode::ArrowLeft) {
            yaw_delta -= 1.0;
        }
        if input.is_key_down(KeyCode::ArrowRight) {
            yaw_delta += 1.0;
        }
        if input.is_key_down(KeyCode::ArrowUp) {
            pitch_delta += 1.0;
        }
        if input.is_key_down(KeyCode::ArrowDown) {
            pitch_delta -= 1.0;
        }

        self.yaw += yaw_delta * self.look_speed * dt;
        self.pitch = (self.pitch + pitch_delta * self.look_speed * dt).clamp(-1.45, 1.45);

        self.yaw += input.mouse_delta.0 * self.mouse_sensitivity;
        self.pitch = (self.pitch - input.mouse_delta.1 * self.mouse_sensitivity).clamp(-1.45, 1.45);

        let mut dolly_axis = 0.0_f32;
        let mut right_axis = 0.0_f32;
        let mut up_axis = 0.0_f32;
        if input.is_key_down(KeyCode::KeyW) {
            dolly_axis += 1.0;
        }
        if input.is_key_down(KeyCode::KeyS) {
            dolly_axis -= 1.0;
        }
        if input.is_key_down(KeyCode::KeyD) {
            right_axis += 1.0;
        }
        if input.is_key_down(KeyCode::KeyA) {
            right_axis -= 1.0;
        }
        if input.is_key_down(KeyCode::KeyE) {
            up_axis += 1.0;
        }
        if input.is_key_down(KeyCode::KeyQ) {
            up_axis -= 1.0;
        }

        let mut speed = self.move_speed;
        if input.is_key_down(KeyCode::ShiftLeft) || input.is_key_down(KeyCode::ShiftRight) {
            speed *= 4.0;
        }
        if input.is_key_down(KeyCode::ControlLeft) || input.is_key_down(KeyCode::ControlRight) {
            speed *= 0.25;
        }

        let rotation = quat_from_yaw_pitch(self.yaw, self.pitch);
        let right = quat_rotate_vec3(rotation, Vec3f::new(1.0, 0.0, 0.0));
        let up = quat_rotate_vec3(rotation, Vec3f::new(0.0, 1.0, 0.0));

        let pan = Vec3f::new(
            right.x * right_axis + up.x * up_axis,
            right.y * right_axis + up.y * up_axis,
            right.z * right_axis + up.z * up_axis,
        );
        let pan = normalize_or_zero(pan);
        let pan_scale = speed * dt * self.distance * 0.5;
        self.target = Vec3f::new(
            self.target.x + pan.x * pan_scale,
            self.target.y + pan.y * pan_scale,
            self.target.z + pan.z * pan_scale,
        );

        let dolly = dolly_axis * speed * dt + input.scroll_y * speed * 0.2;
        self.distance = (self.distance - dolly).clamp(0.05, 1.0e5);
        self.sync_position_from_orbit();
    }

    fn sync_position_from_orbit(&mut self) {
        let rotation = quat_from_yaw_pitch(self.yaw, self.pitch);
        let forward = quat_rotate_vec3(rotation, Vec3f::new(0.0, 0.0, 1.0));
        self.position = Vec3f::new(
            self.target.x - forward.x * self.distance,
            self.target.y - forward.y * self.distance,
            self.target.z - forward.z * self.distance,
        );
    }
}

#[cfg(feature = "interactive-viewer")]
fn normalize_or_zero(v: Vec3f) -> Vec3f {
    let len_sq = v.x * v.x + v.y * v.y + v.z * v.z;
    if len_sq <= 1e-12 {
        return Vec3f::new(0.0, 0.0, 0.0);
    }

    let inv_len = len_sq.sqrt().recip();
    Vec3f::new(v.x * inv_len, v.y * inv_len, v.z * inv_len)
}

#[cfg(feature = "interactive-viewer")]
fn quat_from_yaw_pitch(yaw: f32, pitch: f32) -> [f32; 4] {
    let q_yaw = [0.0, (yaw * 0.5).sin(), 0.0, (yaw * 0.5).cos()];
    let q_pitch = [(pitch * 0.5).sin(), 0.0, 0.0, (pitch * 0.5).cos()];
    quat_normalize(quat_mul(q_yaw, q_pitch))
}

#[cfg(feature = "interactive-viewer")]
fn quat_mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

#[cfg(feature = "interactive-viewer")]
fn quat_normalize(q: [f32; 4]) -> [f32; 4] {
    let len_sq = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
    if len_sq <= 1e-12 {
        return [0.0, 0.0, 0.0, 1.0];
    }

    let inv_len = len_sq.sqrt().recip();
    [
        q[0] * inv_len,
        q[1] * inv_len,
        q[2] * inv_len,
        q[3] * inv_len,
    ]
}

#[cfg(feature = "interactive-viewer")]
fn quat_rotate_vec3(q: [f32; 4], v: Vec3f) -> Vec3f {
    let u = Vec3f::new(q[0], q[1], q[2]);
    let s = q[3];
    let uv = cross(u, v);
    let uuv = cross(u, uv);
    Vec3f::new(
        v.x + 2.0 * (s * uv.x + uuv.x),
        v.y + 2.0 * (s * uv.y + uuv.y),
        v.z + 2.0 * (s * uv.z + uuv.z),
    )
}

#[cfg(feature = "interactive-viewer")]
fn cross(a: Vec3f, b: Vec3f) -> Vec3f {
    Vec3f::new(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
}
