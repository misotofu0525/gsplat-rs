//! Private Q1 M4 native control and terminal-throughput evidence host.
//!
//! This host is deliberately feature-gated and fixed to the admitted Truck
//! workload. Untimed sustained current-stats control is kept separate from a
//! cadence-matched presentation window with no current-stats observer load.
//! Renderer policy, ticket allocation and terminal publication stay in the
//! shared Exact owner.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use gsplat_render_wgpu::{
    SurfaceCurrentStatsCountSemantics, SurfaceCurrentStatsReceipt, SurfaceCurrentStatsSubmission,
    SurfaceCurrentStatsSubmissionReceipt, SurfaceFrameCapture, SurfaceFrameOutput,
    SurfaceProjectedDrawPolicy, SurfaceRasterExecutionPlan,
};
use winit::event_loop::{ActiveEventLoop, EventLoop};

use crate::{
    cli::Args,
    surface_evidence::{
        CurrentStatsLedger, CurrentStatsLedgerPoll, LiveCameraReceipt, RECEIPT_TIMEOUT,
        SurfaceEvidenceRuntime, SurfaceRuntimeCapture, SurfaceRuntimeCaptureMode,
        SurfaceRuntimeCommand, SurfaceRuntimeEvent, adaptive_state_label, backend_label,
        count_semantics_label, format_f32_values, output_plan_label, plan_label,
        projected_adaptive_state_label, projected_execution_label,
        publish_ordinary_surface_capture, run_surface_event_loop, validate_current_stats_counts,
        validate_executed_plan_output,
    },
    trace::CameraTracePlayback,
    viewer::{SurfaceResolutionReceipt, SurfaceTraceStep, surface_trace_steps},
};

const TRACE_ID: &str = "candidate-truck-quality-2view-1920x1080-v1";
const TRACE_CONTENT_SHA256: &str =
    "34d47dbddf73d915bfd55431b33da9430882767a40d9d74c636c508f7d7a5ab3";
const TRUCK_SPLAT_COUNT: usize = 2_541_226;
const WARMUP_FRAMES: usize = 20;
const MEASURED_FRAMES: usize = 80;
const CAPTURE_TRACE_FRAMES: [usize; 2] = [0, 1];
const CADENCE_NS: u64 = 16_666_667;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum MemberPhase {
    Warmup,
    Measure,
    Capture,
}

impl MemberPhase {
    const fn label(self) -> &'static str {
        match self {
            Self::Warmup => "warmup",
            Self::Measure => "measure",
            Self::Capture => "capture",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunPhase {
    ControlWarmup,
    ControlWarmupDrain,
    ControlMeasure,
    ControlMeasureDrain,
    TimedWarmup,
    TimedWarmupDrain,
    TimedMeasure,
    TimedMeasureDrain,
    Capture,
    Complete,
}

impl RunPhase {
    const fn is_timed(self) -> bool {
        matches!(
            self,
            Self::TimedWarmup
                | Self::TimedWarmupDrain
                | Self::TimedMeasure
                | Self::TimedMeasureDrain
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum TimedPhase {
    Warmup,
    Measure,
}

impl TimedPhase {
    const fn label(self) -> &'static str {
        match self {
            Self::Warmup => "warmup",
            Self::Measure => "measure",
        }
    }
}

#[derive(Debug)]
struct PendingMember {
    phase: MemberPhase,
    member_index: usize,
    step: SurfaceTraceStep,
    member_first_input_ns: u64,
    issued_ns: u64,
    output: SurfaceFrameOutput,
    live_camera: LiveCameraReceipt,
    capture: Option<(PathBuf, SurfaceFrameCapture)>,
}

#[derive(Debug, Clone, Copy)]
struct PresentedAttempt {
    phase: MemberPhase,
    member_index: usize,
    attempt: usize,
    member_first_input_ns: u64,
    cadence_start_ns: u64,
    input_ns: u64,
    step: SurfaceTraceStep,
    output: SurfaceFrameOutput,
    live_camera: LiveCameraReceipt,
    submission: SurfaceCurrentStatsSubmission,
}

#[derive(Debug, Clone, Copy)]
struct TimedPresentation {
    phase: TimedPhase,
    member_index: usize,
    input_ns: u64,
    presented_ns: u64,
    step: SurfaceTraceStep,
    output: SurfaceFrameOutput,
    live_camera: LiveCameraReceipt,
}

#[derive(Debug, Default)]
struct TimedLedger {
    presentations_by_phase: BTreeMap<TimedPhase, usize>,
    actual_plans: BTreeSet<&'static str>,
    whole_plan_adaptive_states: BTreeSet<&'static str>,
    projected_adaptive_states: BTreeSet<&'static str>,
    projected_executions: BTreeSet<&'static str>,
    window_start_ns: Option<u64>,
    window_end_ns: Option<u64>,
    last_camera_revision: Option<u64>,
    current_stats_requests: usize,
    current_stats_submissions: usize,
    warmup_queue_drains: usize,
    terminal_queue_drains: usize,
    warmup_drain_start_ns: Option<u64>,
    warmup_drain_end_ns: Option<u64>,
}

impl TimedLedger {
    fn record(&mut self, record: TimedPresentation) -> Result<(), String> {
        let expected_member = self
            .presentations_by_phase
            .get(&record.phase)
            .copied()
            .unwrap_or(0);
        if record.member_index != expected_member {
            return Err(format!(
                "Q1 timed {} presentation membership is not contiguous: expected={expected_member} actual={}",
                record.phase.label(),
                record.member_index
            ));
        }
        if self
            .last_camera_revision
            .is_some_and(|previous| record.live_camera.revision <= previous)
        {
            return Err(format!(
                "Q1 timed camera revision is not strictly increasing: previous={:?} current={}",
                self.last_camera_revision, record.live_camera.revision
            ));
        }
        self.last_camera_revision = Some(record.live_camera.revision);
        if record.phase == TimedPhase::Measure {
            self.actual_plans.insert(output_plan_label(record.output)?);
            self.whole_plan_adaptive_states
                .insert(adaptive_state_label(record.output.adaptive_state));
            self.projected_adaptive_states
                .insert(projected_adaptive_state_label(
                    record.output.projected_draw_adaptive_state,
                ));
            self.projected_executions.insert(projected_execution_label(
                record.output.projected_draw_execution,
            ));
        }
        *self.presentations_by_phase.entry(record.phase).or_default() += 1;
        Ok(())
    }

    fn begin_warmup_drain(&mut self, start_ns: u64) -> Result<(), String> {
        let warmup_presentations = self
            .presentations_by_phase
            .get(&TimedPhase::Warmup)
            .copied()
            .unwrap_or(0);
        let measured_presentations = self
            .presentations_by_phase
            .get(&TimedPhase::Measure)
            .copied()
            .unwrap_or(0);
        if warmup_presentations != WARMUP_FRAMES || measured_presentations != 0 {
            return Err(format!(
                "Q1 timed warmup drain membership is invalid: warmup={warmup_presentations} measured={measured_presentations}"
            ));
        }
        if self.window_start_ns.is_some()
            || self.warmup_drain_start_ns.is_some()
            || self.warmup_drain_end_ns.is_some()
        {
            return Err(
                "Q1 timed warmup drain began outside its unique pre-measure boundary".to_owned(),
            );
        }
        self.warmup_drain_start_ns = Some(start_ns);
        Ok(())
    }

    fn complete_warmup_drain(&mut self, end_ns: u64) -> Result<(), String> {
        let Some(start_ns) = self.warmup_drain_start_ns else {
            return Err("Q1 timed warmup drain completed without a begin receipt".to_owned());
        };
        if end_ns < start_ns || self.window_start_ns.is_some() {
            return Err("Q1 timed warmup drain crossed the measured terminal window".to_owned());
        }
        if self.warmup_drain_end_ns.replace(end_ns).is_some() {
            return Err("Q1 timed warmup queue completed more than once".to_owned());
        }
        self.warmup_queue_drains += 1;
        Ok(())
    }

    fn note_terminal_queue_drain(&mut self) {
        self.terminal_queue_drains += 1;
    }

    fn require_complete(&self) -> Result<(), String> {
        for (phase, expected) in [
            (TimedPhase::Warmup, WARMUP_FRAMES),
            (TimedPhase::Measure, MEASURED_FRAMES),
        ] {
            let actual = self
                .presentations_by_phase
                .get(&phase)
                .copied()
                .unwrap_or(0);
            if actual != expected {
                return Err(format!(
                    "Q1 timed {} membership is incomplete: expected={expected} actual={actual}",
                    phase.label()
                ));
            }
        }
        if self.current_stats_requests != 0 || self.current_stats_submissions != 0 {
            return Err(format!(
                "Q1 timed window carried current-stats load: requests={} submissions={}",
                self.current_stats_requests, self.current_stats_submissions
            ));
        }
        if self.whole_plan_adaptive_states.is_empty()
            || self.projected_adaptive_states.is_empty()
            || self.projected_executions.is_empty()
        {
            return Err("Q1 timed measured Adaptive receipt sets are incomplete".to_owned());
        }
        if self.warmup_queue_drains != 1 {
            return Err(format!(
                "Q1 timed stage requires exactly one pre-measure warmup queue drain: actual={}",
                self.warmup_queue_drains
            ));
        }
        if self.terminal_queue_drains != 1 {
            return Err(format!(
                "Q1 timed window requires exactly one terminal queue drain: actual={}",
                self.terminal_queue_drains
            ));
        }
        let Some(warmup_end_ns) = self.warmup_drain_end_ns else {
            return Err("Q1 timed warmup queue drain has no completion timestamp".to_owned());
        };
        let Some(window_start_ns) = self.window_start_ns else {
            return Err("Q1 timed terminal window has no start timestamp".to_owned());
        };
        if warmup_end_ns > window_start_ns {
            return Err("Q1 timed measured input preceded warmup queue completion".to_owned());
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
struct TicketLedger {
    receipts: CurrentStatsLedger<PendingMember>,
    issued_by_phase: BTreeMap<MemberPhase, usize>,
    terminal_by_phase: BTreeMap<MemberPhase, usize>,
    actual_plans: BTreeSet<&'static str>,
    projected_adaptive_states: BTreeSet<&'static str>,
    projected_executions: BTreeSet<&'static str>,
    last_terminal_observed_ns: Option<u64>,
    measured_window_start_ns: Option<u64>,
    measured_window_end_ns: Option<u64>,
    auxiliary_presentations: usize,
    total_presentations: usize,
    receipt_polls: usize,
    capture_paths: Vec<PathBuf>,
}

impl TicketLedger {
    fn issue(
        &mut self,
        submission: SurfaceCurrentStatsSubmissionReceipt,
        pending: PendingMember,
        deadline: Instant,
    ) -> Result<(), String> {
        let phase = pending.phase;
        self.receipts.issue(submission, pending, deadline)?;
        *self.issued_by_phase.entry(phase).or_default() += 1;
        Ok(())
    }

    fn note_terminal(&mut self, phase: MemberPhase, observed_ns: u64) {
        *self.terminal_by_phase.entry(phase).or_default() += 1;
        self.last_terminal_observed_ns = Some(
            self.last_terminal_observed_ns
                .map_or(observed_ns, |previous| previous.max(observed_ns)),
        );
        if phase == MemberPhase::Measure {
            self.measured_window_end_ns = Some(
                self.measured_window_end_ns
                    .map_or(observed_ns, |previous| previous.max(observed_ns)),
            );
        }
    }

    fn outstanding_for(&self, phase: MemberPhase) -> usize {
        self.receipts
            .outstanding_payloads()
            .filter(|pending| pending.phase == phase)
            .count()
    }

    fn require_complete(&self) -> Result<(), String> {
        self.receipts.require_drained()?;
        for (phase, expected) in [
            (MemberPhase::Warmup, WARMUP_FRAMES),
            (MemberPhase::Measure, MEASURED_FRAMES),
            (MemberPhase::Capture, CAPTURE_TRACE_FRAMES.len()),
        ] {
            let issued = self.issued_by_phase.get(&phase).copied().unwrap_or(0);
            let terminal = self.terminal_by_phase.get(&phase).copied().unwrap_or(0);
            if issued != expected || terminal != expected {
                return Err(format!(
                    "Q1 {} ledger is incomplete: expected={expected} issued={issued} terminal={terminal}",
                    phase.label()
                ));
            }
        }
        if self.capture_paths.len() != CAPTURE_TRACE_FRAMES.len() {
            return Err(format!(
                "Q1 capture ledger is incomplete: expected={} actual={}",
                CAPTURE_TRACE_FRAMES.len(),
                self.capture_paths.len()
            ));
        }
        Ok(())
    }

    fn require_control_drained(&self) -> Result<(), String> {
        self.receipts.require_drained()?;
        for (phase, expected) in [
            (MemberPhase::Warmup, WARMUP_FRAMES),
            (MemberPhase::Measure, MEASURED_FRAMES),
        ] {
            let issued = self.issued_by_phase.get(&phase).copied().unwrap_or(0);
            let terminal = self.terminal_by_phase.get(&phase).copied().unwrap_or(0);
            if issued != expected || terminal != expected {
                return Err(format!(
                    "Q1 control {} ledger was not complete before timing: expected={expected} issued={issued} terminal={terminal}",
                    phase.label()
                ));
            }
        }
        Ok(())
    }

    fn check_deadlines(&self, now: Instant) -> Result<(), String> {
        self.receipts.check_deadlines(now, RECEIPT_TIMEOUT)
    }
}

#[derive(Debug)]
struct Cadence {
    next: Instant,
}

impl Cadence {
    fn starting_now() -> Self {
        Self {
            next: Instant::now(),
        }
    }

    fn wait_slice(&self, now: Instant) -> Option<Duration> {
        self.next
            .checked_duration_since(now)
            .map(|remaining| remaining.min(Duration::from_millis(1)))
    }

    fn advance(&mut self, frame_started: Instant) {
        self.next = frame_started + Duration::from_nanos(CADENCE_NS);
    }

    fn reset(&mut self) {
        self.next = Instant::now();
    }
}

#[derive(Debug, Clone, Copy)]
struct FrozenSchedule {
    warmup_start: usize,
    measure_start: usize,
}

impl FrozenSchedule {
    fn validate(
        playback: &CameraTracePlayback,
        steps: &[SurfaceTraceStep],
    ) -> Result<Self, String> {
        let CameraTracePlayback::Sequence {
            frame_indices,
            warmup_frames,
            measured_frames,
            loops,
            ..
        } = playback
        else {
            return Err("Q1 sustained evidence requires sequence trace playback".to_owned());
        };
        if frame_indices.as_slice() != [0, 1]
            || *warmup_frames != WARMUP_FRAMES
            || *measured_frames != MEASURED_FRAMES
            || *loops != 1
        {
            return Err(
                "Q1 sustained evidence schedule drifted from [0,1], 20+80, one loop".to_owned(),
            );
        }
        if steps.len() != WARMUP_FRAMES + MEASURED_FRAMES {
            return Err(format!(
                "Q1 sustained evidence expected {} steps, got {}",
                WARMUP_FRAMES + MEASURED_FRAMES,
                steps.len()
            ));
        }
        if steps[WARMUP_FRAMES].trace_frame_index != 0
            || steps[WARMUP_FRAMES].measured_sample_index != Some(0)
        {
            return Err(
                "Q1 measured trace did not restart at frame 0 after warmup drain".to_owned(),
            );
        }
        Ok(Self {
            warmup_start: 0,
            measure_start: WARMUP_FRAMES,
        })
    }

    fn step(
        self,
        phase: MemberPhase,
        member_index: usize,
        steps: &[SurfaceTraceStep],
        playback: &CameraTracePlayback,
    ) -> Result<SurfaceTraceStep, String> {
        match phase {
            MemberPhase::Warmup => steps
                .get(self.warmup_start + member_index)
                .copied()
                .ok_or_else(|| "Q1 warmup schedule overflow".to_owned()),
            MemberPhase::Measure => steps
                .get(self.measure_start + member_index)
                .copied()
                .ok_or_else(|| "Q1 measured schedule overflow".to_owned()),
            MemberPhase::Capture => {
                let trace_frame_index = *CAPTURE_TRACE_FRAMES
                    .get(member_index)
                    .ok_or_else(|| "Q1 capture schedule overflow".to_owned())?;
                let frame = playback
                    .trace()
                    .frame(trace_frame_index)
                    .map_err(|error| error.to_string())?;
                Ok(SurfaceTraceStep {
                    playback_index: WARMUP_FRAMES + MEASURED_FRAMES + member_index,
                    phase: gsplat_core::camera_trace::CameraTraceSequencePhase::Measure,
                    loop_index: 0,
                    phase_frame_index: member_index,
                    measured_sample_index: None,
                    trace_frame_index,
                    timestamp_ns: frame.timestamp_ns,
                    camera: frame.camera().map_err(|error| error.to_string())?,
                })
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct EvidenceIdentity {
    resolution: SurfaceResolutionReceipt,
    source_count: usize,
    resident_count: usize,
    sh_degree: u8,
}

#[allow(deprecated)] // Kept aligned with the existing desktop Surface loops.
pub(crate) fn run(
    args: &Args,
    event_loop: EventLoop<()>,
    window: Arc<winit::window::Window>,
    mut runtime: SurfaceEvidenceRuntime,
    playback: &CameraTracePlayback,
    adapter_info: wgpu::AdapterInfo,
) -> Result<(), String> {
    let capture_base = args
        .png_out
        .as_deref()
        .ok_or_else(|| "Q1 sustained capture base is missing".to_owned())?;
    let capture_paths = CAPTURE_TRACE_FRAMES
        .iter()
        .enumerate()
        .map(|(index, trace_frame)| capture_path(capture_base, index, *trace_frame))
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(path) = capture_paths.iter().find(|path| path.exists()) {
        return Err(format!(
            "Q1 sustained capture destination already exists: {}",
            path.display()
        ));
    }

    let trace = playback.trace();
    if trace.trace_id != TRACE_ID
        || trace.content_sha256 != TRACE_CONTENT_SHA256
        || trace.display.width != 1920
        || trace.display.height != 1080
        || trace.frames.len() != 2
    {
        return Err(
            "Q1 sustained evidence requires the frozen two-view 1920x1080 Truck trace".to_owned(),
        );
    }
    let steps = surface_trace_steps(playback)?;
    let schedule = FrozenSchedule::validate(playback, &steps)?;
    let runtime_identity = runtime.identity().clone();
    let source_count = runtime_identity.source_count();
    let resident_count = runtime_identity.resident_count();
    let sh_degree = runtime_identity.sh_degree();
    if source_count != TRUCK_SPLAT_COUNT || resident_count != TRUCK_SPLAT_COUNT || sh_degree != 3 {
        return Err(format!(
            "Q1 sustained evidence requires complete Truck SH3: source={source_count} resident={resident_count} sh={sh_degree}"
        ));
    }
    let resolution = runtime_identity.resolution();
    let identity = EvidenceIdentity {
        resolution,
        source_count,
        resident_count,
        sh_degree,
    };
    if adapter_info.backend != wgpu::Backend::Metal || !adapter_info.name.contains("Apple M4") {
        return Err(format!(
            "Q1 M4 native evidence requires Apple M4 Metal, got backend={:?} adapter={:?}",
            adapter_info.backend, adapter_info.name
        ));
    }
    if runtime_identity.raster_execution_plan() != SurfaceRasterExecutionPlan::ProjectedQuadsExact
        || runtime_identity.projected_draw_policy() != SurfaceProjectedDrawPolicy::Adaptive
    {
        return Err(
            "Q1 sustained host did not retain ProjectedQuadsExact plus Adaptive policy".to_owned(),
        );
    }

    println!(
        "SURFACE_Q1_SUSTAINED_BEGIN schema=gsplat-q1-m4-native-sustained/v3 stages=untimed_current_stats_control,terminal_throughput shared_process_identity=true trace_id={} trace_sha256={} geometry_path=packed_atlas raster_execution_plan=projected_quads_exact order_backend=adaptive projected_draw_policy=adaptive sort_policy=every_frame sort_interval=1 host_cadence_hz=60 cadence_ns={} control_warmup_frames={} control_measured_frames={} timed_warmup_frames={} timed_measured_frames={} capture_trace_frames=0,1 source_membership=all sampling=disabled lod=disabled dynamic_resolution=disabled upscaling=disabled source_count={} decoded_count={} encoded_count={} resident_count={} addressable_count={} sh_degree={} requested_width={} requested_height={} surface_width={} surface_height={} internal_render_width={} internal_render_height={} adapter_backend=metal adapter_name={:?}",
        trace.trace_id,
        trace.content_sha256,
        CADENCE_NS,
        WARMUP_FRAMES,
        MEASURED_FRAMES,
        WARMUP_FRAMES,
        MEASURED_FRAMES,
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
        adapter_info.name,
    );

    let playback = playback.clone();
    let error = Arc::new(Mutex::new(None::<String>));
    let shared_error = Arc::clone(&error);
    let completed = Arc::new(AtomicBool::new(false));
    let shared_completed = Arc::clone(&completed);
    let started = Instant::now();
    let mut phase = RunPhase::ControlWarmup;
    let mut member_index = 0_usize;
    let mut member_attempt = 0_usize;
    let mut member_first_input_ns = None::<u64>;
    let mut member_deadline = None::<Instant>;
    let mut request_pending = false;
    let mut capture_waiting = false;
    let mut ledger = TicketLedger::default();
    let mut timed = TimedLedger::default();
    let mut cadence = Cadence::starting_now();
    let mut drain_started_ns = None::<u64>;

    let window_container = window.inner_size();
    run_surface_event_loop(
        event_loop,
        Arc::clone(&window),
        (window_container.width, window_container.height),
        "Q1 sustained",
        Arc::clone(&shared_error),
        Arc::clone(&shared_completed),
        move |target| {
            if !phase.is_timed()
                && let Err(message) = ledger.check_deadlines(Instant::now()).and_then(|()| {
                    poll_all_terminals(&mut runtime, &identity, &mut ledger, started)
                })
            {
                fail(&shared_error, target, message);
                return;
            }

            match phase {
                RunPhase::ControlWarmupDrain
                    if ledger.outstanding_for(MemberPhase::Warmup) == 0 =>
                {
                    let end_ns = elapsed_ns(started);
                    println!(
                        "SURFACE_Q1_SUSTAINED_DRAIN phase=warmup event=end start_ns={} end_ns={} draws_during_drain=0 terminal_tickets={}",
                        drain_started_ns.unwrap_or(end_ns),
                        end_ns,
                        ledger
                            .terminal_by_phase
                            .get(&MemberPhase::Warmup)
                            .copied()
                            .unwrap_or(0),
                    );
                    phase = RunPhase::ControlMeasure;
                    member_index = 0;
                    member_attempt = 0;
                    member_first_input_ns = None;
                    member_deadline = None;
                    drain_started_ns = None;
                    cadence.reset();
                }
                RunPhase::ControlMeasureDrain
                    if ledger.outstanding_for(MemberPhase::Measure) == 0 =>
                {
                    let end_ns = ledger
                        .measured_window_end_ns
                        .unwrap_or_else(|| elapsed_ns(started));
                    println!(
                        "SURFACE_Q1_SUSTAINED_DRAIN phase=measure event=end start_ns={} end_ns={} draws_during_drain=0 terminal_tickets={}",
                        drain_started_ns.unwrap_or(end_ns),
                        end_ns,
                        ledger
                            .terminal_by_phase
                            .get(&MemberPhase::Measure)
                            .copied()
                            .unwrap_or(0),
                    );
                    if let Err(message) = ledger.require_control_drained() {
                        fail(&shared_error, target, message);
                        return;
                    }
                    phase = RunPhase::TimedWarmup;
                    member_index = 0;
                    member_attempt = 0;
                    member_first_input_ns = None;
                    member_deadline = None;
                    drain_started_ns = None;
                    cadence.reset();
                }
                RunPhase::TimedWarmupDrain => {
                    let Some(drain_start) = timed.warmup_drain_start_ns else {
                        fail(
                            &shared_error,
                            target,
                            "Q1 timed warmup drain has no begin receipt".to_owned(),
                        );
                        return;
                    };
                    match runtime.execute(SurfaceRuntimeCommand::CompleteQueue(RECEIPT_TIMEOUT)) {
                        Ok(SurfaceRuntimeEvent::QueueCompleted(true)) => {}
                        Ok(SurfaceRuntimeEvent::QueueCompleted(false)) => {
                            fail(
                                &shared_error,
                                target,
                                "Q1 timed warmup queue completion timed out".to_owned(),
                            );
                            return;
                        }
                        Ok(_) => {
                            fail(
                                &shared_error,
                                target,
                                "Q1 runtime returned the wrong warmup queue-completion event"
                                    .to_owned(),
                            );
                            return;
                        }
                        Err(error) => {
                            fail(
                                &shared_error,
                                target,
                                format!("Q1 timed warmup queue completion failed: {error}"),
                            );
                            return;
                        }
                    }
                    let end_ns = elapsed_ns(started);
                    if let Err(message) = timed.complete_warmup_drain(end_ns) {
                        fail(&shared_error, target, message);
                        return;
                    }
                    println!(
                        "SURFACE_Q1_TIMED_DRAIN phase=warmup event=end start_ns={} end_ns={} queue_completion=true draw_count=0 draws_during_drain=0 current_stats_count=0 current_stats_requests=0 current_stats_submissions=0 capture_count=0 capture_requests=0 warmup_submissions={}",
                        drain_start, end_ns, WARMUP_FRAMES,
                    );
                    phase = RunPhase::TimedMeasure;
                    member_index = 0;
                    drain_started_ns = None;
                    cadence.reset();
                    return;
                }
                RunPhase::TimedMeasureDrain => {
                    let drain_start = drain_started_ns.unwrap_or_else(|| elapsed_ns(started));
                    match runtime.execute(SurfaceRuntimeCommand::CompleteQueue(RECEIPT_TIMEOUT)) {
                        Ok(SurfaceRuntimeEvent::QueueCompleted(true)) => {}
                        Ok(SurfaceRuntimeEvent::QueueCompleted(false)) => {
                            fail(
                                &shared_error,
                                target,
                                "Q1 timed terminal queue completion timed out".to_owned(),
                            );
                            return;
                        }
                        Ok(_) => {
                            fail(
                                &shared_error,
                                target,
                                "Q1 runtime returned the wrong queue-completion event".to_owned(),
                            );
                            return;
                        }
                        Err(error) => {
                            fail(
                                &shared_error,
                                target,
                                format!("Q1 timed terminal queue completion failed: {error}"),
                            );
                            return;
                        }
                    }
                    let end_ns = elapsed_ns(started);
                    timed.window_end_ns = Some(end_ns);
                    timed.note_terminal_queue_drain();
                    if let Err(message) = timed.require_complete() {
                        fail(&shared_error, target, message);
                        return;
                    }
                    let Some(window_start_ns) = timed.window_start_ns else {
                        fail(
                            &shared_error,
                            target,
                            "Q1 timed terminal window has no start".to_owned(),
                        );
                        return;
                    };
                    let Some(window_duration_ns) = end_ns.checked_sub(window_start_ns) else {
                        fail(
                            &shared_error,
                            target,
                            "Q1 timed terminal window is not monotonic".to_owned(),
                        );
                        return;
                    };
                    if window_duration_ns == 0 {
                        fail(
                            &shared_error,
                            target,
                            "Q1 timed terminal window is empty".to_owned(),
                        );
                        return;
                    }
                    let fps = MEASURED_FRAMES as f64 * 1_000_000_000.0 / window_duration_ns as f64;
                    println!(
                        "SURFACE_Q1_TIMED_DRAIN phase=measure event=end start_ns={} end_ns={} queue_completion=true draw_count=0 draws_during_drain=0 current_stats_count=0 current_stats_requests=0 current_stats_submissions=0 capture_count=0 capture_requests=0 measured_submissions={}",
                        drain_start, end_ns, MEASURED_FRAMES,
                    );
                    println!(
                        "SURFACE_Q1_TIMED_SUMMARY status=ok evidence_role=formal_terminal_throughput clock=std_instant_monotonic window_start=first_measured_camera_input window_end=measured_queue_completion window_start_ns={} window_end_ns={} window_duration_ns={} n={} terminal_fps={:.9} warmup_presentations={} measured_presentations={} current_stats_requests=0 current_stats_submissions=0 warmup_queue_drains={} queue_drains={} terminal_drain_draws=0 actual_plan_set={} whole_plan_adaptive_state_set={} projected_adaptive_state_set={} projected_execution_set={}",
                        window_start_ns,
                        end_ns,
                        window_duration_ns,
                        MEASURED_FRAMES,
                        fps,
                        timed
                            .presentations_by_phase
                            .get(&TimedPhase::Warmup)
                            .copied()
                            .unwrap_or(0),
                        timed
                            .presentations_by_phase
                            .get(&TimedPhase::Measure)
                            .copied()
                            .unwrap_or(0),
                        timed.warmup_queue_drains,
                        timed.terminal_queue_drains,
                        timed
                            .actual_plans
                            .iter()
                            .copied()
                            .collect::<Vec<_>>()
                            .join(","),
                        timed
                            .whole_plan_adaptive_states
                            .iter()
                            .copied()
                            .collect::<Vec<_>>()
                            .join(","),
                        timed
                            .projected_adaptive_states
                            .iter()
                            .copied()
                            .collect::<Vec<_>>()
                            .join(","),
                        timed
                            .projected_executions
                            .iter()
                            .copied()
                            .collect::<Vec<_>>()
                            .join(","),
                    );
                    phase = RunPhase::Capture;
                    member_index = 0;
                    member_attempt = 0;
                    member_first_input_ns = None;
                    member_deadline = None;
                    drain_started_ns = None;
                    cadence.reset();
                }
                RunPhase::Capture if capture_waiting => {
                    if ledger.outstanding_for(MemberPhase::Capture) == 0 {
                        capture_waiting = false;
                        member_index += 1;
                        member_attempt = 0;
                        member_first_input_ns = None;
                        member_deadline = None;
                        if member_index == CAPTURE_TRACE_FRAMES.len() {
                            phase = RunPhase::Complete;
                        }
                    }
                }
                _ => {}
            }

            if phase == RunPhase::Complete {
                if let Err(message) = ledger.require_complete() {
                    fail(&shared_error, target, message);
                    return;
                }
                let Some(control_start_ns) = ledger.measured_window_start_ns else {
                    fail(
                        &shared_error,
                        target,
                        "Q1 control observation interval has no start".to_owned(),
                    );
                    return;
                };
                let Some(control_end_ns) = ledger.measured_window_end_ns else {
                    fail(
                        &shared_error,
                        target,
                        "Q1 control observation interval has no end".to_owned(),
                    );
                    return;
                };
                let Some(control_duration_ns) = control_end_ns.checked_sub(control_start_ns) else {
                    fail(
                        &shared_error,
                        target,
                        "Q1 control observation interval is not monotonic".to_owned(),
                    );
                    return;
                };
                if control_duration_ns == 0 {
                    fail(
                        &shared_error,
                        target,
                        "Q1 control observation interval is empty".to_owned(),
                    );
                    return;
                }
                println!(
                    "SURFACE_Q1_SUSTAINED_SUMMARY status=ok evidence_role=untimed_correctness_control observer_load=current_stats_every_member timing_eligible=false throughput_n=null throughput_fps=null clock=std_instant_monotonic control_start=first_control_measured_camera_input control_end=last_control_measured_current_stats_terminal control_start_ns={} control_end_ns={} control_duration_ns={} control_measured_members={} warmup_issued={} measured_issued={} capture_issued={} total_issued={} total_terminals={} total_presentations={} auxiliary_presentations={} receipt_polls={} actual_plan_set={} projected_adaptive_state_set={} projected_execution_set={} capture_count={} drain_draws=0",
                    control_start_ns,
                    control_end_ns,
                    control_duration_ns,
                    MEASURED_FRAMES,
                    ledger
                        .issued_by_phase
                        .get(&MemberPhase::Warmup)
                        .copied()
                        .unwrap_or(0),
                    ledger
                        .issued_by_phase
                        .get(&MemberPhase::Measure)
                        .copied()
                        .unwrap_or(0),
                    ledger
                        .issued_by_phase
                        .get(&MemberPhase::Capture)
                        .copied()
                        .unwrap_or(0),
                    ledger.receipts.issued_len(),
                    ledger.receipts.terminal_len(),
                    ledger.total_presentations,
                    ledger.auxiliary_presentations,
                    ledger.receipt_polls,
                    ledger
                        .actual_plans
                        .iter()
                        .copied()
                        .collect::<Vec<_>>()
                        .join(","),
                    ledger
                        .projected_adaptive_states
                        .iter()
                        .copied()
                        .collect::<Vec<_>>()
                        .join(","),
                    ledger
                        .projected_executions
                        .iter()
                        .copied()
                        .collect::<Vec<_>>()
                        .join(","),
                    ledger.capture_paths.len(),
                );
                shared_completed.store(true, Ordering::Release);
                target.exit();
                return;
            }

            if matches!(
                phase,
                RunPhase::ControlWarmupDrain
                    | RunPhase::ControlMeasureDrain
                    | RunPhase::TimedWarmupDrain
            ) || (phase == RunPhase::Capture && capture_waiting)
            {
                std::thread::sleep(Duration::from_millis(1));
                return;
            }

            if let Some(wait) = cadence.wait_slice(Instant::now()) {
                std::thread::sleep(wait);
                return;
            }
            let cadence_started = Instant::now();
            let cadence_start_ns = elapsed_ns(started);

            if matches!(phase, RunPhase::TimedWarmup | RunPhase::TimedMeasure) {
                let timed_phase = match phase {
                    RunPhase::TimedWarmup => TimedPhase::Warmup,
                    RunPhase::TimedMeasure => TimedPhase::Measure,
                    _ => unreachable!("guarded timed presentation phase"),
                };
                let schedule_phase = match timed_phase {
                    TimedPhase::Warmup => MemberPhase::Warmup,
                    TimedPhase::Measure => MemberPhase::Measure,
                };
                let step = match schedule.step(schedule_phase, member_index, &steps, &playback) {
                    Ok(step) => step,
                    Err(message) => {
                        fail(&shared_error, target, message);
                        return;
                    }
                };
                let input_ns = elapsed_ns(started);
                if timed_phase == TimedPhase::Measure && member_index == 0 {
                    timed.window_start_ns.get_or_insert(input_ns);
                }
                let presentation = match runtime.execute(SurfaceRuntimeCommand::Present {
                    step,
                    capture: SurfaceRuntimeCaptureMode::None,
                }) {
                    Ok(SurfaceRuntimeEvent::Presented(presentation)) => presentation,
                    Ok(_) => {
                        fail(
                            &shared_error,
                            target,
                            "Q1 runtime returned the wrong timed presentation event".to_owned(),
                        );
                        return;
                    }
                    Err(message) => {
                        fail(&shared_error, target, message);
                        return;
                    }
                };
                let (output, live_camera, submission, capture, _) = (*presentation).into_parts();
                cadence.advance(cadence_started);
                if let Err(message) = validate_output(&identity, output, live_camera) {
                    fail(&shared_error, target, message);
                    return;
                }
                if capture.is_some() {
                    fail(
                        &shared_error,
                        target,
                        "Q1 timed presentation returned an unrequested capture receipt".to_owned(),
                    );
                    return;
                }
                if submission != SurfaceCurrentStatsSubmission::NotRequested {
                    timed.current_stats_submissions += 1;
                    fail(
                        &shared_error,
                        target,
                        "Q1 timed presentation published a current-stats submission".to_owned(),
                    );
                    return;
                }
                let record = TimedPresentation {
                    phase: timed_phase,
                    member_index,
                    input_ns,
                    presented_ns: elapsed_ns(started),
                    step,
                    output,
                    live_camera,
                };
                if let Err(message) = timed.record(record) {
                    fail(&shared_error, target, message);
                    return;
                }
                print_timed_presentation(record);
                member_index += 1;
                let expected = match timed_phase {
                    TimedPhase::Warmup => WARMUP_FRAMES,
                    TimedPhase::Measure => MEASURED_FRAMES,
                };
                if member_index == expected {
                    match timed_phase {
                        TimedPhase::Warmup => {
                            let drain_start = elapsed_ns(started);
                            if let Err(message) = timed.begin_warmup_drain(drain_start) {
                                fail(&shared_error, target, message);
                                return;
                            }
                            phase = RunPhase::TimedWarmupDrain;
                            println!(
                                "SURFACE_Q1_TIMED_DRAIN phase=warmup event=begin start_ns={} queue_completion=pending draw_count=0 draws_during_drain=0 current_stats_count=0 current_stats_requests=0 current_stats_submissions=0 capture_count=0 capture_requests=0 warmup_submissions={}",
                                drain_start, WARMUP_FRAMES,
                            );
                        }
                        TimedPhase::Measure => {
                            drain_started_ns = Some(elapsed_ns(started));
                            phase = RunPhase::TimedMeasureDrain;
                            println!(
                                "SURFACE_Q1_TIMED_DRAIN phase=measure event=begin start_ns={} queue_completion=pending draw_count=0 draws_during_drain=0 current_stats_count=0 current_stats_requests=0 current_stats_submissions=0 capture_count=0 capture_requests=0 measured_submissions={}",
                                drain_started_ns.unwrap_or_default(),
                                MEASURED_FRAMES,
                            );
                        }
                    }
                }
                return;
            }

            let member_phase = match phase {
                RunPhase::ControlWarmup => MemberPhase::Warmup,
                RunPhase::ControlMeasure => MemberPhase::Measure,
                RunPhase::Capture => MemberPhase::Capture,
                _ => return,
            };
            let step = match schedule.step(member_phase, member_index, &steps, &playback) {
                Ok(step) => step,
                Err(message) => {
                    fail(&shared_error, target, message);
                    return;
                }
            };
            if !request_pending {
                match runtime.execute(SurfaceRuntimeCommand::RequestCurrentStats) {
                    Ok(SurfaceRuntimeEvent::CurrentStatsRequested) => {
                        request_pending = true;
                        member_deadline = Some(Instant::now() + RECEIPT_TIMEOUT);
                    }
                    Ok(_) => {
                        fail(
                            &shared_error,
                            target,
                            format!(
                                "Q1 runtime returned the wrong current-stats request event before {} member {}",
                                member_phase.label(),
                                member_index
                            ),
                        );
                        return;
                    }
                    Err(message) => {
                        fail(&shared_error, target, message);
                        return;
                    }
                }
            }

            let input_ns = elapsed_ns(started);
            member_first_input_ns.get_or_insert(input_ns);
            if member_phase == MemberPhase::Measure && member_index == 0 {
                ledger
                    .measured_window_start_ns
                    .get_or_insert(*member_first_input_ns.as_ref().expect("Q1 input timestamp"));
            }
            let capture_mode = if member_phase == MemberPhase::Capture {
                SurfaceRuntimeCaptureMode::Ordinary
            } else {
                SurfaceRuntimeCaptureMode::None
            };
            let presentation = match runtime.execute(SurfaceRuntimeCommand::Present {
                step,
                capture: capture_mode,
            }) {
                Ok(SurfaceRuntimeEvent::Presented(presentation)) => presentation,
                Ok(_) => {
                    fail(
                        &shared_error,
                        target,
                        "Q1 runtime returned the wrong control presentation event".to_owned(),
                    );
                    return;
                }
                Err(message) => {
                    fail(&shared_error, target, message);
                    return;
                }
            };
            let (output, live_camera, submission, capture, _) = (*presentation).into_parts();
            cadence.advance(cadence_started);
            ledger.total_presentations += 1;
            if let Err(message) = validate_output(&identity, output, live_camera) {
                fail(&shared_error, target, message);
                return;
            }
            ledger
                .projected_adaptive_states
                .insert(projected_adaptive_state_label(
                    output.projected_draw_adaptive_state,
                ));
            ledger
                .projected_executions
                .insert(projected_execution_label(output.projected_draw_execution));
            let capture = match capture {
                Some(SurfaceRuntimeCapture::Ordinary(capture)) => Some(capture),
                #[cfg(all(
                    feature = "diagnostic-surface-capture-receipt",
                    not(target_arch = "wasm32")
                ))]
                Some(SurfaceRuntimeCapture::Diagnostic(_)) => {
                    fail(
                        &shared_error,
                        target,
                        "Q1 runtime returned a non-ordinary capture receipt".to_owned(),
                    );
                    return;
                }
                None if member_phase == MemberPhase::Capture => {
                    fail(
                        &shared_error,
                        target,
                        "Q1 runtime omitted the requested capture receipt".to_owned(),
                    );
                    return;
                }
                None => None,
            };
            print_presentation(PresentedAttempt {
                phase: member_phase,
                member_index,
                attempt: member_attempt,
                member_first_input_ns: *member_first_input_ns.as_ref().expect("Q1 member input"),
                cadence_start_ns,
                input_ns,
                step,
                output,
                live_camera,
                submission,
            });
            member_attempt += 1;

            match submission {
                SurfaceCurrentStatsSubmission::Issued(submission) => {
                    if let Err(message) = validate_submission(submission, output, live_camera) {
                        fail(&shared_error, target, message);
                        return;
                    }
                    let pending = PendingMember {
                        phase: member_phase,
                        member_index,
                        step,
                        member_first_input_ns: member_first_input_ns
                            .take()
                            .expect("Q1 issued member input"),
                        issued_ns: elapsed_ns(started),
                        output,
                        live_camera,
                        capture: capture
                            .map(|capture| (capture_paths[member_index].clone(), capture)),
                    };
                    let deadline = member_deadline.take().expect("Q1 issued member deadline");
                    print_submission(&pending, submission);
                    if let Err(message) = ledger.issue(submission, pending, deadline) {
                        fail(&shared_error, target, message);
                        return;
                    }
                    request_pending = false;
                    member_attempt = 0;
                    match phase {
                        RunPhase::ControlWarmup => {
                            member_index += 1;
                            if member_index == WARMUP_FRAMES {
                                phase = RunPhase::ControlWarmupDrain;
                                drain_started_ns = Some(elapsed_ns(started));
                                println!(
                                    "SURFACE_Q1_SUSTAINED_DRAIN phase=warmup event=begin start_ns={} issued_tickets={} draws_during_drain=0",
                                    drain_started_ns.unwrap_or_default(),
                                    WARMUP_FRAMES,
                                );
                            }
                        }
                        RunPhase::ControlMeasure => {
                            member_index += 1;
                            if member_index == MEASURED_FRAMES {
                                phase = RunPhase::ControlMeasureDrain;
                                drain_started_ns = Some(elapsed_ns(started));
                                println!(
                                    "SURFACE_Q1_SUSTAINED_DRAIN phase=measure event=begin start_ns={} issued_tickets={} draws_during_drain=0",
                                    drain_started_ns.unwrap_or_default(),
                                    MEASURED_FRAMES,
                                );
                            }
                        }
                        RunPhase::Capture => {
                            capture_waiting = true;
                        }
                        _ => {}
                    }
                }
                SurfaceCurrentStatsSubmission::NotRequested => {
                    ledger.auxiliary_presentations += 1;
                    drop(capture);
                    if member_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                        fail(
                            &shared_error,
                            target,
                            format!(
                                "Q1 {} member {} remained ineligible for current-stats",
                                member_phase.label(),
                                member_index
                            ),
                        );
                        return;
                    }
                }
            }

            if let Err(message) = poll_all_terminals(&mut runtime, &identity, &mut ledger, started)
            {
                fail(&shared_error, target, message);
            }
        },
    )?;

    if let Some(message) = error
        .lock()
        .map_err(|_| "Q1 sustained error lock poisoned".to_owned())?
        .take()
    {
        return Err(message);
    }
    if !completed.load(Ordering::Acquire) {
        return Err("Q1 sustained host ended without terminal publication".to_owned());
    }
    Ok(())
}

fn poll_all_terminals(
    runtime: &mut SurfaceEvidenceRuntime,
    identity: &EvidenceIdentity,
    ledger: &mut TicketLedger,
    started: Instant,
) -> Result<(), String> {
    loop {
        ledger.receipt_polls += 1;
        match ledger.receipts.poll_one(runtime)? {
            CurrentStatsLedgerPoll::Empty => return Ok(()),
            CurrentStatsLedgerPoll::Ready(resolved) => {
                let receipt = resolved.receipt;
                let pending = resolved.payload;
                validate_terminal(identity, &pending, receipt)?;
                let observed_ns = elapsed_ns(started);
                ledger
                    .actual_plans
                    .insert(plan_label(receipt.submission().join().executed_plan()));
                if let Some((path, capture)) = pending.capture.as_ref() {
                    if capture.width != identity.resolution.requested.0
                        || capture.height != identity.resolution.requested.1
                    {
                        return Err(format!(
                            "Q1 capture dimensions drifted to {}x{}",
                            capture.width, capture.height
                        ));
                    }
                    publish_ordinary_surface_capture(path, capture)?;
                    print_capture(&pending, receipt, observed_ns, path);
                    ledger.capture_paths.push(path.clone());
                }
                print_terminal(&pending, receipt, observed_ns);
                ledger.note_terminal(pending.phase, observed_ns);
            }
        }
    }
}

fn validate_output(
    identity: &EvidenceIdentity,
    output: SurfaceFrameOutput,
    live: LiveCameraReceipt,
) -> Result<(), String> {
    if !output.frame_presented
        || output.gpu_order_preparation_pending
        || output.raster_execution_plan != SurfaceRasterExecutionPlan::ProjectedQuadsExact
        || output.projected_draw_policy != SurfaceProjectedDrawPolicy::Adaptive
        || live.surface_size != identity.resolution.requested
    {
        return Err("Q1 presentation is not a complete 1920x1080 Packed Exact member".to_owned());
    }
    if output.camera_revision != live.revision {
        return Err("Q1 live camera revision did not bind to the presented frame".to_owned());
    }
    Ok(())
}

fn validate_submission(
    submission: SurfaceCurrentStatsSubmissionReceipt,
    output: SurfaceFrameOutput,
    live: LiveCameraReceipt,
) -> Result<(), String> {
    let join = submission.join();
    if join.frame_identity().camera_revision() != live.revision
        || output.camera_revision != live.revision
    {
        return Err(format!(
            "Q1 ticket {} camera revision join mismatch",
            submission.ticket()
        ));
    }
    validate_executed_plan_output(join.executed_plan(), output)
}

fn validate_terminal(
    identity: &EvidenceIdentity,
    pending: &PendingMember,
    receipt: SurfaceCurrentStatsReceipt,
) -> Result<(), String> {
    validate_executed_plan_output(receipt.submission().join().executed_plan(), pending.output)?;
    validate_current_stats_counts(identity.source_count, receipt)
}

fn print_presentation(record: PresentedAttempt) {
    println!(
        "SURFACE_Q1_SUSTAINED_PRESENTATION evidence_role=untimed_correctness_control observer_load=current_stats phase={} member_index={} attempt={} member_first_input_ns={} cadence_start_ns={} input_ns={} trace_frame={} trace_timestamp_ns={} request_state={} submission={} ticket={} camera_revision={} frame_presented={} sort_refreshed={} order_uploaded={} actual_backend={} whole_plan_adaptive_state={} projected_adaptive_state={} projected_draw_execution={} frame_wall_ms={:.6}",
        record.phase.label(),
        record.member_index,
        record.attempt,
        record.member_first_input_ns,
        record.cadence_start_ns,
        record.input_ns,
        record.step.trace_frame_index,
        record.step.timestamp_ns,
        if record.attempt == 0 {
            "requested"
        } else {
            "pending"
        },
        if record.submission.receipt().is_some() {
            "issued"
        } else {
            "not_requested"
        },
        record
            .submission
            .receipt()
            .map_or_else(|| "none".to_owned(), |receipt| receipt.ticket().to_string()),
        record.live_camera.revision,
        record.output.frame_presented,
        record.output.sort_refreshed,
        record.output.order_uploaded,
        backend_label(record.output.order_backend),
        adaptive_state_label(record.output.adaptive_state),
        projected_adaptive_state_label(record.output.projected_draw_adaptive_state),
        projected_execution_label(record.output.projected_draw_execution),
        record.output.timings.frame_wall_ms,
    );
}

fn print_timed_presentation(record: TimedPresentation) {
    let camera = record.live_camera.camera;
    println!(
        "SURFACE_Q1_TIMED_PRESENTATION evidence_role=formal_terminal_throughput phase={} member_index={} input_ns={} presented_ns={} trace_frame={} trace_timestamp_ns={} camera_revision={} frame_presented={} sort_refreshed={} order_uploaded={} actual_plan={} actual_backend={} whole_plan_adaptive_state={} projected_adaptive_state={} projected_draw_execution={} current_stats_requests=0 current_stats_submission=not_requested position={} rotation_xyzw={} vertical_fov_radians={:.9} near_plane={:.9} far_plane={:.9} aspect={:.9} view_matrix={} projection_matrix={} view_projection_matrix={} frame_wall_ms={:.6}",
        record.phase.label(),
        record.member_index,
        record.input_ns,
        record.presented_ns,
        record.step.trace_frame_index,
        record.step.timestamp_ns,
        record.live_camera.revision,
        record.output.frame_presented,
        record.output.sort_refreshed,
        record.output.order_uploaded,
        output_plan_label(record.output).unwrap_or("invalid"),
        backend_label(record.output.order_backend),
        adaptive_state_label(record.output.adaptive_state),
        projected_adaptive_state_label(record.output.projected_draw_adaptive_state),
        projected_execution_label(record.output.projected_draw_execution),
        format_f32_values(&[
            camera.pose.position.x,
            camera.pose.position.y,
            camera.pose.position.z,
        ]),
        format_f32_values(&camera.pose.rotation_xyzw),
        camera.intrinsics.vertical_fov_radians,
        camera.intrinsics.near_plane,
        camera.intrinsics.far_plane,
        record.live_camera.aspect,
        format_f32_values(&record.live_camera.view_matrix),
        format_f32_values(&record.live_camera.projection_matrix),
        format_f32_values(&record.live_camera.view_projection_matrix),
        record.output.timings.frame_wall_ms,
    );
}

fn print_submission(pending: &PendingMember, submission: SurfaceCurrentStatsSubmissionReceipt) {
    let join = submission.join();
    let frame = join.frame_identity();
    let camera = pending.live_camera.camera;
    println!(
        "SURFACE_Q1_SUSTAINED_SUBMISSION evidence_role=untimed_correctness_control phase={} member_index={} trace_frame={} trace_timestamp_ns={} member_first_input_ns={} issued_ns={} ticket={} executed_plan={} scene_generation={} camera_revision={} viewport_generation={} contract_generation={} plan_set_generation={} order_generation={} raster_generation={} encode_attempt={} presentation_sequence={} actual_backend={} position={} rotation_xyzw={} vertical_fov_radians={:.9} near_plane={:.9} far_plane={:.9} aspect={:.9} view_matrix={} projection_matrix={} view_projection_matrix={} frame_presented=true",
        pending.phase.label(),
        pending.member_index,
        pending.step.trace_frame_index,
        pending.step.timestamp_ns,
        pending.member_first_input_ns,
        pending.issued_ns,
        submission.ticket(),
        plan_label(join.executed_plan()),
        frame.scene_generation(),
        frame.camera_revision(),
        frame.viewport_generation(),
        frame.contract_generation(),
        frame.plan_set_generation(),
        join.order_generation(),
        join.raster_generation(),
        join.encode_attempt(),
        join.presentation_sequence(),
        backend_label(pending.output.order_backend),
        format_f32_values(&[
            camera.pose.position.x,
            camera.pose.position.y,
            camera.pose.position.z,
        ]),
        format_f32_values(&camera.pose.rotation_xyzw),
        camera.intrinsics.vertical_fov_radians,
        camera.intrinsics.near_plane,
        camera.intrinsics.far_plane,
        pending.live_camera.aspect,
        format_f32_values(&pending.live_camera.view_matrix),
        format_f32_values(&pending.live_camera.projection_matrix),
        format_f32_values(&pending.live_camera.view_projection_matrix),
    );
}

fn print_terminal(pending: &PendingMember, receipt: SurfaceCurrentStatsReceipt, observed_ns: u64) {
    let submission = receipt.submission();
    let join = submission.join();
    let frame = join.frame_identity();
    let counts = receipt.counts();
    println!(
        "SURFACE_Q1_SUSTAINED_TERMINAL status=ready evidence_role=untimed_correctness_control phase={} member_index={} trace_frame={} observed_ns={} ticket={} executed_plan={} scene_generation={} camera_revision={} viewport_generation={} contract_generation={} plan_set_generation={} order_generation={} raster_generation={} encode_attempt={} presentation_sequence={} count_semantics={} source_count={} visible_count={} contributor_count={} drawn_count={} exact_contributor_compaction={}",
        pending.phase.label(),
        pending.member_index,
        pending.step.trace_frame_index,
        observed_ns,
        submission.ticket(),
        plan_label(join.executed_plan()),
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
    );
}

fn print_capture(
    pending: &PendingMember,
    receipt: SurfaceCurrentStatsReceipt,
    observed_ns: u64,
    path: &Path,
) {
    let join = receipt.submission().join();
    println!(
        "SURFACE_Q1_SUSTAINED_CAPTURE status=ok evidence_role=post_timing_capture_control capture_index={} trace_frame={} path={:?} ticket={} camera_revision={} presentation_sequence={} terminal_observed_ns={} terminal_receipt=ready width=1920 height=1080 view_matrix={} projection_matrix={} view_projection_matrix={}",
        pending.member_index,
        pending.step.trace_frame_index,
        path.to_string_lossy(),
        receipt.submission().ticket(),
        join.frame_identity().camera_revision(),
        join.presentation_sequence(),
        observed_ns,
        format_f32_values(&pending.live_camera.view_matrix),
        format_f32_values(&pending.live_camera.projection_matrix),
        format_f32_values(&pending.live_camera.view_projection_matrix),
    );
}

fn capture_path(base: &Path, capture_index: usize, trace_frame: usize) -> Result<PathBuf, String> {
    let stem = base
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "Q1 capture base requires a UTF-8 file stem".to_owned())?;
    let extension = base
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("png");
    Ok(base.with_file_name(format!(
        "{stem}.view-{capture_index}-trace-{trace_frame}.{extension}"
    )))
}

fn fail(slot: &Mutex<Option<String>>, target: &ActiveEventLoop, message: String) {
    if let Ok(mut slot) = slot.lock() {
        *slot = Some(message);
    }
    target.exit();
}

fn elapsed_ns(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_paths_are_fixed_view_zero_then_one() {
        let base = Path::new("capture.png");
        assert_eq!(
            capture_path(base, 0, 0).unwrap(),
            PathBuf::from("capture.view-0-trace-0.png")
        );
        assert_eq!(
            capture_path(base, 1, 1).unwrap(),
            PathBuf::from("capture.view-1-trace-1.png")
        );
    }

    #[test]
    fn cadence_never_schedules_earlier_than_sixty_hz_period() {
        assert_eq!(Duration::from_nanos(CADENCE_NS).as_nanos(), 16_666_667);
        assert_eq!(1_000_000_000_u64.div_ceil(CADENCE_NS), 60);
    }
}
