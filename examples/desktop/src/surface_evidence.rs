//! Strict real-window Surface evidence for the M2b migration boundary.
//!
//! This host owns trace playback, receipt translation and final image I/O.
//! Renderer policy, ticket allocation, count readback and terminal publication
//! remain in `SurfaceRenderSession` and its Renderer-owned Exact runtime.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use gsplat_core::Camera;
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
#[cfg(feature = "qualification-q1-m4-native")]
use gsplat_render_wgpu::{SurfaceAdaptiveState, SurfaceProjectedDrawAdaptiveState};
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
    event_loop::{ActiveEventLoop, EventLoop},
};

use crate::{
    cli::{Args, SurfaceEvidencePlanArg},
    image_output::write_png,
    trace::CameraTracePlayback,
    viewer::{SurfaceResolutionReceipt, SurfaceTraceStep, surface_trace_steps},
};

pub(crate) const RECEIPT_TIMEOUT: Duration = Duration::from_secs(30);
const CAMERA_TOLERANCE: f64 = 5.0e-5;
const MAX_INELIGIBLE_RETRIES: usize = 64;
const DIAGNOSTIC_MULTI_CAPTURE_TRACE_FRAMES: [usize; 3] = [0, 1, 0];

#[derive(Debug, Default, PartialEq, Eq)]
struct MultiCaptureSchedule {
    next_capture: usize,
}

#[cfg_attr(not(test), allow(dead_code))]
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

#[allow(deprecated)] // Shared with the existing desktop Surface host until run_app migration.
pub(crate) fn run_surface_event_loop<F>(
    event_loop: EventLoop<()>,
    window: Arc<winit::window::Window>,
    expected_size: (u32, u32),
    label: &'static str,
    error: Arc<Mutex<Option<String>>>,
    completed: Arc<AtomicBool>,
    mut redraw: F,
) -> Result<(), String>
where
    F: FnMut(&ActiveEventLoop) + 'static,
{
    let window_id = window.id();
    event_loop
        .run(move |event, target| match event {
            _ if completed.load(Ordering::Acquire) => {}
            Event::AboutToWait if error.lock().is_ok_and(|slot| slot.is_none()) => {
                window.request_redraw();
            }
            Event::WindowEvent {
                window_id: id,
                event: WindowEvent::CloseRequested,
            } if id == window_id => {
                store_error(
                    &error,
                    format!("{label} window closed before terminal publication"),
                );
                target.exit();
            }
            Event::WindowEvent {
                window_id: id,
                event: WindowEvent::Resized(size),
            } if id == window_id && (size.width, size.height) != expected_size => {
                store_error(
                    &error,
                    format!(
                        "{label} resize {}x{} violates {}x{}",
                        size.width, size.height, expected_size.0, expected_size.1
                    ),
                );
                target.exit();
            }
            Event::WindowEvent {
                window_id: id,
                event: WindowEvent::RedrawRequested,
            } if id == window_id && error.lock().is_ok_and(|slot| slot.is_none()) => {
                redraw(target);
            }
            _ => {}
        })
        .map_err(|error| format!("{label} event loop failed: {error}"))
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LiveCameraReceipt {
    pub(crate) revision: u64,
    pub(crate) surface_size: (u32, u32),
    pub(crate) aspect: f32,
    pub(crate) camera: Camera,
    pub(crate) view_matrix: [f32; 16],
    pub(crate) projection_matrix: [f32; 16],
    pub(crate) view_projection_matrix: [f32; 16],
}

impl LiveCameraReceipt {
    fn validated(
        session: &SurfaceRenderSession,
        playback: &CameraTracePlayback,
        step: SurfaceTraceStep,
    ) -> Result<Self, String> {
        let surface_size = session.surface_size();
        if surface_size.0 == 0 || surface_size.1 == 0 {
            return Err("live camera receipt has a zero Surface dimension".to_owned());
        }
        let camera = session.camera();
        camera
            .validate()
            .map_err(|_| "live session camera is invalid".to_owned())?;
        let aspect = surface_size.0 as f32 / surface_size.1 as f32;
        let view_matrix = canonical_view_matrix_f32(camera);
        let projection_matrix = canonical_projection_matrix_f32(camera, aspect);
        let receipt = Self {
            revision: session.camera_revision(),
            surface_size,
            aspect,
            camera,
            view_matrix,
            projection_matrix,
            view_projection_matrix: multiply_mat4_f32(projection_matrix, view_matrix),
        };
        validate_live_camera(playback, step, receipt)?;
        Ok(receipt)
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(not(feature = "qualification-q1-m4-native"), allow(dead_code))]
pub(crate) struct SurfaceRuntimeIdentity {
    resolution: SurfaceResolutionReceipt,
    source_count: usize,
    resident_count: usize,
    sh_degree: u8,
    raster_execution_plan: SurfaceRasterExecutionPlan,
    projected_draw_policy: gsplat_render_wgpu::SurfaceProjectedDrawPolicy,
}

#[cfg_attr(not(feature = "qualification-q1-m4-native"), allow(dead_code))]
impl SurfaceRuntimeIdentity {
    pub(crate) const fn resolution(&self) -> SurfaceResolutionReceipt {
        self.resolution
    }

    pub(crate) const fn source_count(&self) -> usize {
        self.source_count
    }

    pub(crate) const fn resident_count(&self) -> usize {
        self.resident_count
    }

    pub(crate) const fn sh_degree(&self) -> u8 {
        self.sh_degree
    }

    pub(crate) const fn raster_execution_plan(&self) -> SurfaceRasterExecutionPlan {
        self.raster_execution_plan
    }

    pub(crate) const fn projected_draw_policy(
        &self,
    ) -> gsplat_render_wgpu::SurfaceProjectedDrawPolicy {
        self.projected_draw_policy
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SurfaceRuntimeCaptureMode {
    None,
    Ordinary,
    Diagnostic,
}

#[derive(Debug, Clone, Copy)]
#[cfg_attr(not(feature = "qualification-q1-m4-native"), allow(dead_code))]
pub(crate) enum SurfaceRuntimeCommand {
    RequestCurrentStats,
    Present {
        step: SurfaceTraceStep,
        capture: SurfaceRuntimeCaptureMode,
    },
    PollCurrentStats,
    CompleteQueue(Duration),
}

#[derive(Debug)]
#[cfg_attr(not(feature = "qualification-q1-m4-native"), allow(dead_code))]
pub(crate) struct SurfaceRuntimePresentation {
    output: SurfaceFrameOutput,
    live_camera: LiveCameraReceipt,
    current_stats_submission: SurfaceCurrentStatsSubmission,
    capture: Option<SurfaceRuntimeCapture>,
    call_ms: f32,
}

#[cfg_attr(not(feature = "qualification-q1-m4-native"), allow(dead_code))]
impl SurfaceRuntimePresentation {
    pub(crate) fn into_parts(
        self,
    ) -> (
        SurfaceFrameOutput,
        LiveCameraReceipt,
        SurfaceCurrentStatsSubmission,
        Option<SurfaceRuntimeCapture>,
        f32,
    ) {
        (
            self.output,
            self.live_camera,
            self.current_stats_submission,
            self.capture,
            self.call_ms,
        )
    }
}

#[derive(Debug)]
#[cfg_attr(not(feature = "qualification-q1-m4-native"), allow(dead_code))]
pub(crate) enum SurfaceRuntimeEvent {
    CurrentStatsRequested,
    Presented(Box<SurfaceRuntimePresentation>),
    CurrentStatsEmpty,
    CurrentStatsReady(SurfaceCurrentStatsReceipt),
    QueueCompleted(bool),
}

/// The sole private owner of a live desktop Surface evidence session.
///
/// Evidence phase policies submit commands and receive owned immutable
/// receipts. They never render, poll, capture, or read live camera state
/// through `SurfaceRenderSession` directly.
pub(crate) struct SurfaceEvidenceRuntime {
    session: SurfaceRenderSession,
    playback: CameraTracePlayback,
    identity: SurfaceRuntimeIdentity,
}

impl SurfaceEvidenceRuntime {
    pub(crate) fn new(
        session: SurfaceRenderSession,
        playback: &CameraTracePlayback,
        requested_size: (u32, u32),
    ) -> Result<Self, String> {
        let source_count = session
            .renderer()
            .scene_len()
            .ok_or_else(|| "Surface evidence scene is not loaded".to_owned())?;
        let resident_count = session
            .renderer()
            .resident_scene()
            .map_or(source_count, |scene| scene.len());
        let resolution = SurfaceResolutionReceipt::validated(
            requested_size,
            session.surface_size(),
            session.internal_render_size(),
        )?;
        let identity = SurfaceRuntimeIdentity {
            resolution,
            source_count,
            resident_count,
            sh_degree: session.renderer().scene_sh_degree().unwrap_or(0),
            raster_execution_plan: session.raster_execution_plan(),
            projected_draw_policy: session.projected_draw_policy(),
        };
        Ok(Self {
            session,
            playback: playback.clone(),
            identity,
        })
    }

    pub(crate) fn identity(&self) -> &SurfaceRuntimeIdentity {
        &self.identity
    }

    pub(crate) fn execute(
        &mut self,
        command: SurfaceRuntimeCommand,
    ) -> Result<SurfaceRuntimeEvent, String> {
        match command {
            SurfaceRuntimeCommand::RequestCurrentStats => {
                match self.session.request_current_stats() {
                    SurfaceCurrentStatsRequest::Requested => {
                        Ok(SurfaceRuntimeEvent::CurrentStatsRequested)
                    }
                    SurfaceCurrentStatsRequest::Unsampled(reason) => {
                        Err(format!("current-stats request unavailable: {reason:?}"))
                    }
                }
            }
            SurfaceRuntimeCommand::Present { step, capture } => {
                self.session
                    .set_camera(step.camera)
                    .map_err(|error| format!("Surface evidence camera update failed: {error}"))?;
                let live_camera =
                    LiveCameraReceipt::validated(&self.session, &self.playback, step)?;
                self.session.force_sort_refresh();
                if capture != SurfaceRuntimeCaptureMode::None {
                    self.session.request_surface_capture().map_err(|error| {
                        format!("Surface evidence capture request failed: {error}")
                    })?;
                }
                let call_started = Instant::now();
                let output = self
                    .session
                    .render_frame()
                    .map_err(|error| format!("Surface evidence render failed: {error}"))?;
                let call_ms = call_started.elapsed().as_secs_f32() * 1_000.0;
                if !output.frame_presented
                    || output.gpu_order_preparation_pending
                    || self.session.last_presented_size()
                        != Some(self.identity.resolution.requested)
                    || self.session.surface_size() != self.identity.resolution.requested
                    || self.session.internal_render_size() != self.identity.resolution.requested
                {
                    return Err(format!(
                        "Surface evidence frame was not a complete full-resolution presentation: presented={} preparation_pending={} presented_size={:?}",
                        output.frame_presented,
                        output.gpu_order_preparation_pending,
                        self.session.last_presented_size()
                    ));
                }
                if output.camera_revision != live_camera.revision
                    || self.session.camera_revision() != live_camera.revision
                {
                    return Err(
                        "Surface evidence live camera revision did not bind to the presented frame"
                            .to_owned(),
                    );
                }
                let capture = match capture {
                    SurfaceRuntimeCaptureMode::None => None,
                    SurfaceRuntimeCaptureMode::Ordinary => {
                        Some(take_pending_capture(&mut self.session, false)?)
                    }
                    SurfaceRuntimeCaptureMode::Diagnostic => {
                        Some(take_pending_capture(&mut self.session, true)?)
                    }
                };
                Ok(SurfaceRuntimeEvent::Presented(Box::new(
                    SurfaceRuntimePresentation {
                        output,
                        live_camera,
                        current_stats_submission: self.session.current_stats_submission(),
                        capture,
                        call_ms,
                    },
                )))
            }
            SurfaceRuntimeCommand::PollCurrentStats => match self.session.poll_current_stats() {
                SurfaceCurrentStatsPoll::Empty => Ok(SurfaceRuntimeEvent::CurrentStatsEmpty),
                SurfaceCurrentStatsPoll::Unsampled(reason) => Err(format!(
                    "current-stats request resolved unsampled without a joinable ticket: {reason:?}"
                )),
                SurfaceCurrentStatsPoll::Terminal(terminal) => {
                    ready_current_stats_terminal(terminal)
                        .map(SurfaceRuntimeEvent::CurrentStatsReady)
                }
            },
            SurfaceRuntimeCommand::CompleteQueue(timeout) => self
                .session
                .pump_receipts(timeout)
                .map(SurfaceRuntimeEvent::QueueCompleted)
                .map_err(|error| format!("Surface evidence queue completion failed: {error}")),
        }
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
        capture: Box<SurfaceRuntimeCapture>,
    },
}

#[derive(Debug)]
pub(crate) enum SurfaceRuntimeCapture {
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
    depth_precision_profile: &'static str,
    projected_cache_precision_profile: &'static str,
    projected_axis_record_bytes: u64,
    resident_sh_codec_profile: &'static str,
    resident_sh_mantissa_bits: u8,
    resident_sh_symmetric_max_code: u16,
    resident_sh_point_scale_bits: u8,
    resident_sh_point_scale_max_code: u8,
    resident_sh_range_chunk_splats: u16,
    resident_sh_source_count: u32,
    resident_sh_encoded_count: u32,
    resident_sh_resident_count: u32,
    resident_sh_addressable_count: u32,
    resident_sh_source_degree: u8,
    resident_sh_resident_degree: u8,
    resident_sh_residual_coefficients_per_source: u8,
    resident_sh_plane_count: u8,
    resident_sh_bytes_per_source: u16,
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
#[derive(Debug, Clone, Copy, PartialEq)]
struct DiagnosticCameraReceiptRecord {
    trace_frame_index: usize,
    camera_revision: u64,
    presentation_sequence: u64,
    position: [f32; 3],
    rotation_xyzw: [f32; 4],
    vertical_fov_radians: f32,
    near_plane: f32,
    far_plane: f32,
    focal_length_x_over_y: f32,
}

#[cfg(all(
    feature = "diagnostic-surface-capture-receipt",
    not(target_arch = "wasm32")
))]
impl DiagnosticCameraReceiptRecord {
    fn from_presented_capture(
        trace_frame_index: usize,
        live: LiveCameraReceipt,
        capture: &DiagnosticSurfaceCaptureReceipt,
    ) -> Result<Self, String> {
        Self::from_presented_identity(
            trace_frame_index,
            live,
            capture.frame_identity().camera_revision(),
            capture.presentation_sequence(),
        )
    }

    fn from_presented_identity(
        trace_frame_index: usize,
        live: LiveCameraReceipt,
        camera_revision: u64,
        presentation_sequence: u64,
    ) -> Result<Self, String> {
        if live.revision != camera_revision {
            return Err(
                "diagnostic capture camera receipt does not bind the presented camera revision"
                    .to_owned(),
            );
        }
        let camera = live.camera;
        Ok(Self {
            trace_frame_index,
            camera_revision: live.revision,
            presentation_sequence,
            position: [
                camera.pose.position.x,
                camera.pose.position.y,
                camera.pose.position.z,
            ],
            rotation_xyzw: camera.pose.rotation_xyzw,
            vertical_fov_radians: camera.intrinsics.vertical_fov_radians,
            near_plane: camera.intrinsics.near_plane,
            far_plane: camera.intrinsics.far_plane,
            focal_length_x_over_y: camera.intrinsics.focal_length_x_over_y,
        })
    }

    fn line(self) -> String {
        format!(
            "SURFACE_DIAGNOSTIC_CAPTURE_CAMERA_RECEIPT trace_frame={} camera_revision={} presentation_sequence={} position_x={} position_y={} position_z={} rotation_x={} rotation_y={} rotation_z={} rotation_w={} vertical_fov_radians={} near_plane={} far_plane={} focal_length_x_over_y={}",
            self.trace_frame_index,
            self.camera_revision,
            self.presentation_sequence,
            self.position[0],
            self.position[1],
            self.position[2],
            self.rotation_xyzw[0],
            self.rotation_xyzw[1],
            self.rotation_xyzw[2],
            self.rotation_xyzw[3],
            self.vertical_fov_radians,
            self.near_plane,
            self.far_plane,
            self.focal_length_x_over_y,
        )
    }
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
            depth_precision_profile: receipt.depth_precision_profile(),
            projected_cache_precision_profile: receipt.projected_cache_precision_profile(),
            projected_axis_record_bytes: receipt.projected_axis_record_bytes(),
            resident_sh_codec_profile: receipt.resident_sh_codec_profile(),
            resident_sh_mantissa_bits: receipt.resident_sh_mantissa_bits(),
            resident_sh_symmetric_max_code: receipt.resident_sh_symmetric_max_code(),
            resident_sh_point_scale_bits: receipt.resident_sh_point_scale_bits(),
            resident_sh_point_scale_max_code: receipt.resident_sh_point_scale_max_code(),
            resident_sh_range_chunk_splats: receipt.resident_sh_range_chunk_splats(),
            resident_sh_source_count: receipt.resident_sh_source_count(),
            resident_sh_encoded_count: receipt.resident_sh_encoded_count(),
            resident_sh_resident_count: receipt.resident_sh_resident_count(),
            resident_sh_addressable_count: receipt.resident_sh_addressable_count(),
            resident_sh_source_degree: receipt.resident_sh_source_degree(),
            resident_sh_resident_degree: receipt.resident_sh_resident_degree(),
            resident_sh_residual_coefficients_per_source: receipt
                .resident_sh_residual_coefficients_per_source(),
            resident_sh_plane_count: receipt.resident_sh_plane_count(),
            resident_sh_bytes_per_source: receipt.resident_sh_bytes_per_source(),
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
            "SURFACE_DIAGNOSTIC_CAPTURE_RECEIPT depth_precision_profile={} projected_cache_precision_profile={} projected_axis_record_bytes={} resident_sh_codec_profile={} resident_sh_mantissa_bits={} resident_sh_symmetric_max_code={} resident_sh_point_scale_bits={} resident_sh_point_scale_max_code={} resident_sh_range_chunk_splats={} resident_sh_source_count={} resident_sh_encoded_count={} resident_sh_resident_count={} resident_sh_addressable_count={} resident_sh_source_degree={} resident_sh_resident_degree={} resident_sh_residual_coefficients_per_source={} resident_sh_plane_count={} resident_sh_bytes_per_source={} scene_generation={} camera_revision={} viewport_generation={} contract_generation={} plan_set_generation={} plan_id={} order_generation={} presentation_sequence={} width={} height={} rgba8_sha256={}",
            self.depth_precision_profile,
            self.projected_cache_precision_profile,
            self.projected_axis_record_bytes,
            self.resident_sh_codec_profile,
            self.resident_sh_mantissa_bits,
            self.resident_sh_symmetric_max_code,
            self.resident_sh_point_scale_bits,
            self.resident_sh_point_scale_max_code,
            self.resident_sh_range_chunk_splats,
            self.resident_sh_source_count,
            self.resident_sh_encoded_count,
            self.resident_sh_resident_count,
            self.resident_sh_addressable_count,
            self.resident_sh_source_degree,
            self.resident_sh_resident_degree,
            self.resident_sh_residual_coefficients_per_source,
            self.resident_sh_plane_count,
            self.resident_sh_bytes_per_source,
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
struct OutstandingCurrentStats<T> {
    submission: SurfaceCurrentStatsSubmissionReceipt,
    payload: T,
    deadline: Instant,
}

#[derive(Debug)]
pub(crate) struct ResolvedCurrentStats<T> {
    pub(crate) submission: SurfaceCurrentStatsSubmissionReceipt,
    pub(crate) payload: T,
    pub(crate) receipt: SurfaceCurrentStatsReceipt,
}

#[derive(Debug)]
pub(crate) enum CurrentStatsLedgerPoll<T> {
    Empty,
    Ready(Box<ResolvedCurrentStats<T>>),
}

#[derive(Debug)]
pub(crate) struct CurrentStatsLedger<T> {
    outstanding: BTreeMap<u64, OutstandingCurrentStats<T>>,
    issued: BTreeSet<u64>,
    terminals: BTreeSet<u64>,
    last_ticket: Option<u64>,
    last_presentation_sequence: Option<u64>,
}

impl<T> Default for CurrentStatsLedger<T> {
    fn default() -> Self {
        Self {
            outstanding: BTreeMap::new(),
            issued: BTreeSet::new(),
            terminals: BTreeSet::new(),
            last_ticket: None,
            last_presentation_sequence: None,
        }
    }
}

impl<T> CurrentStatsLedger<T> {
    pub(crate) fn issue(
        &mut self,
        submission: SurfaceCurrentStatsSubmissionReceipt,
        payload: T,
        deadline: Instant,
    ) -> Result<(), String> {
        let ticket = submission.ticket();
        let presentation_sequence = submission.join().presentation_sequence();
        if self.last_ticket.is_some_and(|previous| ticket <= previous) {
            return Err(format!(
                "current-stats ticket is not strictly increasing: previous={:?}, current={ticket}",
                self.last_ticket
            ));
        }
        if self
            .last_presentation_sequence
            .is_some_and(|previous| presentation_sequence <= previous)
        {
            return Err(format!(
                "current-stats presentation sequence is not strictly increasing: previous={:?}, current={presentation_sequence}",
                self.last_presentation_sequence
            ));
        }
        if !self.issued.insert(ticket)
            || self
                .outstanding
                .insert(
                    ticket,
                    OutstandingCurrentStats {
                        submission,
                        payload,
                        deadline,
                    },
                )
                .is_some()
        {
            return Err(format!("duplicate current-stats ticket {ticket}"));
        }
        self.last_ticket = Some(ticket);
        self.last_presentation_sequence = Some(presentation_sequence);
        Ok(())
    }

    pub(crate) fn poll_one(
        &mut self,
        runtime: &mut SurfaceEvidenceRuntime,
    ) -> Result<CurrentStatsLedgerPoll<T>, String> {
        match runtime.execute(SurfaceRuntimeCommand::PollCurrentStats)? {
            SurfaceRuntimeEvent::CurrentStatsEmpty => Ok(CurrentStatsLedgerPoll::Empty),
            SurfaceRuntimeEvent::CurrentStatsReady(receipt) => {
                let submission = receipt.submission();
                let ticket = submission.ticket();
                if !self.terminals.insert(ticket) {
                    return Err(format!("duplicate current-stats terminal {ticket}"));
                }
                let pending = self
                    .outstanding
                    .remove(&ticket)
                    .ok_or_else(|| format!("unknown current-stats terminal ticket {ticket}"))?;
                if pending.submission != submission {
                    return Err(format!(
                        "current-stats terminal identity mismatch for ticket {ticket}"
                    ));
                }
                Ok(CurrentStatsLedgerPoll::Ready(Box::new(
                    ResolvedCurrentStats {
                        submission,
                        payload: pending.payload,
                        receipt,
                    },
                )))
            }
            _ => Err("Surface runtime returned the wrong event for current-stats poll".to_owned()),
        }
    }

    pub(crate) fn check_deadlines(&self, now: Instant, timeout: Duration) -> Result<(), String> {
        if let Some((ticket, _)) = self
            .outstanding
            .iter()
            .find(|(_, pending)| now >= pending.deadline)
        {
            return Err(format!(
                "current-stats ticket {ticket} did not resolve within {timeout:?}"
            ));
        }
        Ok(())
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.outstanding.is_empty()
    }

    #[cfg(feature = "qualification-q1-m4-native")]
    pub(crate) fn outstanding_payloads(&self) -> impl Iterator<Item = &T> {
        self.outstanding.values().map(|pending| &pending.payload)
    }

    #[cfg(feature = "qualification-q1-m4-native")]
    pub(crate) fn issued_len(&self) -> usize {
        self.issued.len()
    }

    #[cfg(feature = "qualification-q1-m4-native")]
    pub(crate) fn terminal_len(&self) -> usize {
        self.terminals.len()
    }

    #[cfg(feature = "qualification-q1-m4-native")]
    pub(crate) fn require_drained(&self) -> Result<(), String> {
        if self.outstanding.is_empty() && self.issued == self.terminals {
            Ok(())
        } else {
            Err(format!(
                "current-stats ledger is incomplete: issued={} terminals={} outstanding={}",
                self.issued.len(),
                self.terminals.len(),
                self.outstanding.len()
            ))
        }
    }
}

#[derive(Debug)]
struct PendingReceipt {
    kind: PendingKind,
    output: SurfaceFrameOutput,
    live_camera: LiveCameraReceipt,
    call_ms: f32,
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

#[allow(dead_code)]
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
    mut runtime: SurfaceEvidenceRuntime,
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
    #[allow(unused_variables)]
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
    let runtime_identity = runtime.identity().clone();
    let source_count = runtime_identity.source_count;
    let resident_count = runtime_identity.resident_count;
    if source_count != resident_count {
        return Err(format!(
            "surface evidence membership mismatch: source={source_count}, resident={resident_count}"
        ));
    }
    let resolution = runtime_identity.resolution;
    let identity = EvidenceIdentity {
        requested_plan,
        trace_id: trace.trace_id.clone(),
        trace_sha256: trace.content_sha256.clone(),
        resolution,
        source_count,
        resident_count,
        sh_degree: runtime_identity.sh_degree,
    };
    if identity.sh_degree != 3 {
        return Err(format!(
            "formal surface evidence requires source SH3, got SH{}",
            identity.sh_degree
        ));
    }
    if runtime_identity.raster_execution_plan != SurfaceRasterExecutionPlan::ProjectedQuadsExact {
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

    let error = Arc::new(Mutex::new(None::<String>));
    let shared_error = Arc::clone(&error);
    let completed = Arc::new(AtomicBool::new(false));
    let shared_completed = Arc::clone(&completed);
    let started = Instant::now();
    let mut next_step = 0_usize;
    let mut pending = CurrentStatsLedger::<PendingReceipt>::default();
    let mut request_pending = false;
    let mut ineligible_retries = 0_usize;
    let mut terminal_outcome = TerminalOutcome::default();
    let mut actual_plans = BTreeSet::new();
    let mut measured_frames = 0_usize;
    let mut capture_retries = 0_usize;
    #[allow(unused_mut)]
    let mut multi_capture_schedule = MultiCaptureSchedule::default();
    #[cfg(all(
        feature = "diagnostic-surface-capture-receipt",
        not(target_arch = "wasm32")
    ))]
    let mut validated_multi_captures = Vec::<ValidatedDiagnosticCapture>::new();

    run_surface_event_loop(
        event_loop,
        Arc::clone(&window),
        identity.resolution.requested,
        "surface evidence",
        Arc::clone(&shared_error),
        Arc::clone(&shared_completed),
        move |target| {
            if terminal_outcome.is_committed() {
                return;
            }
            if !pending.is_empty() {
                if let Err(message) = pending.check_deadlines(Instant::now(), RECEIPT_TIMEOUT) {
                    store_error(&shared_error, message);
                    target.exit();
                    return;
                }
                match pending.poll_one(&mut runtime) {
                    Ok(CurrentStatsLedgerPoll::Empty) => {
                        std::thread::sleep(Duration::from_millis(1));
                        return;
                    }
                    Err(message) => {
                        store_error(&shared_error, message);
                        target.exit();
                        return;
                    }
                    Ok(CurrentStatsLedgerPoll::Ready(resolved)) => {
                        let waiting = resolved.payload;
                        let receipt = match validate_terminal(&identity, &waiting, resolved.receipt)
                        {
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
                        if let PendingKind::Capture { capture, .. } = &waiting.kind
                            && let SurfaceRuntimeCapture::Diagnostic(capture_receipt) =
                                capture.as_ref()
                            && let Err(message) = validate_diagnostic_capture_join(
                                capture_receipt,
                                &receipt,
                                identity.resolution.requested,
                            )
                        {
                            store_error(&shared_error, message);
                            target.exit();
                        }
                        actual_plans
                            .insert(plan_label(receipt.submission().join().executed_plan()));
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
                                capture_index: _capture_index,
                                step,
                                path,
                                capture,
                            } => {
                                let capture = *capture;
                                #[cfg(all(
                                    feature = "diagnostic-surface-capture-receipt",
                                    not(target_arch = "wasm32")
                                ))]
                                if diagnostic_multi_capture {
                                    let Some(capture_index) = _capture_index else {
                                        store_error(
                                            &shared_error,
                                            "diagnostic multi-capture lost its capture index"
                                                .to_owned(),
                                        );
                                        target.exit();
                                        return;
                                    };
                                    let SurfaceRuntimeCapture::Diagnostic(capture) = capture else {
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
                                    validated_multi_captures.push(ValidatedDiagnosticCapture {
                                        capture_index,
                                        step,
                                        path,
                                        capture,
                                        output: waiting.output,
                                        receipt,
                                        call_ms: waiting.call_ms,
                                        elapsed_ns,
                                    });
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
                                    SurfaceRuntimeCapture::Ordinary(capture) => {
                                        if let Err(message) =
                                            publish_ordinary_surface_capture(&path, &capture)
                                        {
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
                                    SurfaceRuntimeCapture::Diagnostic(capture_receipt) => {
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
                                        match DiagnosticCameraReceiptRecord::from_presented_capture(
                                            step.trace_frame_index,
                                            waiting.live_camera,
                                            &capture_receipt,
                                        ) {
                                            Ok(record) => println!("{}", record.line()),
                                            Err(message) => {
                                                store_error(&shared_error, message);
                                                target.exit();
                                                return;
                                            }
                                        }
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
                match runtime.execute(SurfaceRuntimeCommand::RequestCurrentStats) {
                    Ok(SurfaceRuntimeEvent::CurrentStatsRequested) => request_pending = true,
                    Ok(_) => {
                        store_error(
                            &shared_error,
                            "Surface runtime returned the wrong event for current-stats request"
                                .to_owned(),
                        );
                        target.exit();
                        return;
                    }
                    Err(message) => {
                        store_error(&shared_error, message);
                        target.exit();
                        return;
                    }
                }
            }
            let capture_mode = if !capture_attempt {
                SurfaceRuntimeCaptureMode::None
            } else if diagnostic_capture_receipt {
                SurfaceRuntimeCaptureMode::Diagnostic
            } else {
                SurfaceRuntimeCaptureMode::Ordinary
            };
            let presentation = match runtime.execute(SurfaceRuntimeCommand::Present {
                step,
                capture: capture_mode,
            }) {
                Ok(SurfaceRuntimeEvent::Presented(presentation)) => presentation,
                Ok(_) => {
                    store_error(
                        &shared_error,
                        "Surface runtime returned the wrong event for presentation".to_owned(),
                    );
                    target.exit();
                    return;
                }
                Err(message) => {
                    store_error(&shared_error, message);
                    target.exit();
                    return;
                }
            };
            let SurfaceRuntimePresentation {
                output,
                live_camera,
                current_stats_submission,
                capture,
                call_ms,
            } = *presentation;
            match current_stats_submission {
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
                            capture: Box::new(capture),
                        },
                        None => PendingKind::Frame(step),
                    };
                    if let Err(message) = pending.issue(
                        submission,
                        PendingReceipt {
                            kind,
                            output,
                            live_camera,
                            call_ms,
                        },
                        Instant::now() + RECEIPT_TIMEOUT,
                    ) {
                        store_error(&shared_error, message);
                        target.exit();
                    }
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
                    match pending.poll_one(&mut runtime) {
                        Ok(CurrentStatsLedgerPoll::Empty) => {}
                        Err(message) => {
                            request_pending = false;
                            store_error(&shared_error, message);
                            target.exit();
                        }
                        Ok(CurrentStatsLedgerPoll::Ready(resolved)) => {
                            store_error(
                                &shared_error,
                                format!(
                                    "current-stats returned an unjoined terminal before issue: ticket {}",
                                    resolved.submission.ticket()
                                ),
                            );
                            target.exit();
                        }
                    }
                }
            }
        },
    )?;

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

pub(crate) fn ready_current_stats_terminal(
    terminal: SurfaceCurrentStatsTerminal,
) -> Result<SurfaceCurrentStatsReceipt, String> {
    match terminal {
        SurfaceCurrentStatsTerminal::Ready(receipt) => Ok(receipt),
        SurfaceCurrentStatsTerminal::MapFailure(failure) => Err(format!(
            "current-stats ticket {} map failed",
            failure.submission().ticket()
        )),
        SurfaceCurrentStatsTerminal::GenerationInvalidated(failure) => Err(format!(
            "current-stats ticket {} generation was invalidated",
            failure.submission().ticket()
        )),
        SurfaceCurrentStatsTerminal::Expired(failure) => Err(format!(
            "current-stats ticket {} expired",
            failure.submission().ticket()
        )),
        SurfaceCurrentStatsTerminal::Dropped(failure) => Err(format!(
            "current-stats ticket {} was dropped",
            failure.submission().ticket()
        )),
    }
}

fn validate_terminal(
    identity: &EvidenceIdentity,
    waiting: &PendingReceipt,
    receipt: SurfaceCurrentStatsReceipt,
) -> Result<SurfaceCurrentStatsReceipt, String> {
    let join = receipt.submission().join();
    if join.frame_identity().camera_revision() != waiting.output.camera_revision {
        return Err("current-stats terminal camera revision does not match its frame".to_owned());
    }
    validate_plan(
        identity.requested_plan,
        waiting.output,
        join.executed_plan(),
    )?;
    validate_current_stats_counts(identity.source_count, receipt)?;
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
    validate_executed_plan_output(actual, output)
}

pub(crate) fn validate_executed_plan_output(
    actual: SurfaceCurrentStatsPlan,
    output: SurfaceFrameOutput,
) -> Result<(), String> {
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

pub(crate) fn validate_current_stats_counts(
    source_count: usize,
    receipt: SurfaceCurrentStatsReceipt,
) -> Result<(), String> {
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
        "SURFACE_EXACT_EVIDENCE_FRAME trace_id={} trace_sha256={} playback_index={} phase={} measured_sample={} trace_frame={} trace_timestamp_ns={} elapsed_ns={} exact_plan_requested={} exact_plan_actual={} current_stats_ticket={} scene_generation={} camera_revision={} viewport_generation={} contract_generation={} plan_set_generation={} order_generation={} raster_generation={} encode_attempt={} presentation_sequence={} count_semantics={} source_count={} visible_count={} contributor_count={} drawn_count={} exact_contributor_compaction={} sort_refreshed={} order_uploaded={} actual_backend={} cpu_preprocess_ms={:.6} cpu_sort_ms={:.6} cpu_render_submit_ms={:.6} call_ms={:.6} frame_wall_ms={:.6} requested_width={} requested_height={} presented_width={} presented_height={} frame_presented=true terminal_receipt=ready",
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
        output.order_uploaded,
        backend_label(output.order_backend),
        output.stats.preprocess_ms,
        output.stats.sort_ms,
        output.timings.render_submit_ms,
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
        "SURFACE_DIAGNOSTIC_MULTI_CAPTURE_TERMINAL status=ok capture_index={} path={:?} trace_frame={} trace_timestamp_ns={} elapsed_ns={} call_ms={:.6} frame_wall_ms={:.6} current_stats_ticket={} current_stats_scene_generation={} current_stats_camera_revision={} current_stats_viewport_generation={} current_stats_contract_generation={} current_stats_plan_set_generation={} current_stats_plan_id={} current_stats_order_generation={} current_stats_raster_generation={} current_stats_encode_attempt={} current_stats_presentation_sequence={} count_semantics={} source_count={} visible_count={} contributor_count={} drawn_count={} exact_contributor_compaction={} capture_receipt_depth_precision_profile={} capture_receipt_projected_cache_precision_profile={} capture_receipt_projected_axis_record_bytes={} capture_receipt_resident_sh_codec_profile={} capture_receipt_resident_sh_mantissa_bits={} capture_receipt_resident_sh_symmetric_max_code={} capture_receipt_resident_sh_point_scale_bits={} capture_receipt_resident_sh_point_scale_max_code={} capture_receipt_resident_sh_range_chunk_splats={} capture_receipt_resident_sh_source_count={} capture_receipt_resident_sh_encoded_count={} capture_receipt_resident_sh_resident_count={} capture_receipt_resident_sh_addressable_count={} capture_receipt_resident_sh_source_degree={} capture_receipt_resident_sh_resident_degree={} capture_receipt_resident_sh_residual_coefficients_per_source={} capture_receipt_resident_sh_plane_count={} capture_receipt_resident_sh_bytes_per_source={} capture_receipt_scene_generation={} capture_receipt_camera_revision={} capture_receipt_viewport_generation={} capture_receipt_contract_generation={} capture_receipt_plan_set_generation={} capture_receipt_plan_id={} capture_receipt_order_generation={} capture_receipt_presentation_sequence={} capture_receipt_width={} capture_receipt_height={} capture_receipt_rgba8_sha256={} frame_presented={} terminal_receipt=ready",
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
        atomic.depth_precision_profile,
        atomic.projected_cache_precision_profile,
        atomic.projected_axis_record_bytes,
        atomic.resident_sh_codec_profile,
        atomic.resident_sh_mantissa_bits,
        atomic.resident_sh_symmetric_max_code,
        atomic.resident_sh_point_scale_bits,
        atomic.resident_sh_point_scale_max_code,
        atomic.resident_sh_range_chunk_splats,
        atomic.resident_sh_source_count,
        atomic.resident_sh_encoded_count,
        atomic.resident_sh_resident_count,
        atomic.resident_sh_addressable_count,
        atomic.resident_sh_source_degree,
        atomic.resident_sh_resident_degree,
        atomic.resident_sh_residual_coefficients_per_source,
        atomic.resident_sh_plane_count,
        atomic.resident_sh_bytes_per_source,
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
    let frame = join.frame_identity();
    let counts = receipt.counts();
    println!(
        "SURFACE_EXACT_EVIDENCE_CAPTURE status=ok path={:?} trace_frame={} exact_plan_requested={} exact_plan_actual={} current_stats_ticket={} scene_generation={} camera_revision={} viewport_generation={} contract_generation={} plan_set_generation={} order_generation={} raster_generation={} encode_attempt={} presentation_sequence={} count_semantics={} source_count={} visible_count={} contributor_count={} drawn_count={} exact_contributor_compaction={} actual_backend={} requested_width={} requested_height={} captured_width={} captured_height={} frame_presented={} terminal_receipt=ready",
        path.to_string_lossy(),
        step.trace_frame_index,
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
) -> Result<SurfaceRuntimeCapture, String> {
    #[cfg(all(
        feature = "diagnostic-surface-capture-receipt",
        not(target_arch = "wasm32")
    ))]
    if diagnostic {
        return session
            .take_diagnostic_surface_capture_receipt()
            .map(SurfaceRuntimeCapture::Diagnostic)
            .map_err(|error| error.to_string());
    }

    if diagnostic {
        return Err("diagnostic Surface capture receipt support is unavailable".to_owned());
    }
    take_ordinary_surface_capture(session).map(SurfaceRuntimeCapture::Ordinary)
}

fn take_ordinary_surface_capture(
    session: &mut SurfaceRenderSession,
) -> Result<SurfaceFrameCapture, String> {
    session
        .take_surface_capture()
        .map_err(|error| error.to_string())
}

pub(crate) fn publish_ordinary_surface_capture(
    path: &Path,
    capture: &SurfaceFrameCapture,
) -> Result<(), String> {
    write_png(path, capture.width, capture.height, &capture.rgba8)
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

fn validate_live_camera(
    playback: &CameraTracePlayback,
    step: SurfaceTraceStep,
    live: LiveCameraReceipt,
) -> Result<(), String> {
    let expected_size = (
        playback.trace().display.width,
        playback.trace().display.height,
    );
    let expected_aspect = expected_size.0 as f32 / expected_size.1 as f32;
    if live.surface_size != expected_size
        || (live.aspect - expected_aspect).abs() > f32::EPSILON
        || live.camera != step.camera
    {
        return Err("live session camera pose/intrinsics/aspect drifted".to_owned());
    }
    let frame = playback
        .trace()
        .frame(step.trace_frame_index)
        .map_err(|error| error.to_string())?;
    close_values(
        &[
            f64::from(live.camera.pose.position.x),
            f64::from(live.camera.pose.position.y),
            f64::from(live.camera.pose.position.z),
        ],
        &frame.pose.position,
        "position",
    )?;
    close_values(
        &live.camera.pose.rotation_xyzw.map(f64::from),
        &frame.pose.rotation_xyzw,
        "rotation_xyzw",
    )?;
    close_values(
        &[
            f64::from(live.camera.intrinsics.vertical_fov_radians),
            f64::from(live.camera.intrinsics.near_plane),
            f64::from(live.camera.intrinsics.far_plane),
        ],
        &[
            frame.intrinsics.vertical_fov_radians,
            frame.intrinsics.near_plane,
            frame.intrinsics.far_plane,
        ],
        "intrinsics",
    )?;
    close_values(
        &live.view_matrix.map(f64::from),
        &frame.view_matrix,
        "view_matrix",
    )?;
    close_values(
        &live.projection_matrix.map(f64::from),
        &frame.projection_matrix,
        "projection_matrix",
    )?;
    close_values(
        &live.view_projection_matrix.map(f64::from),
        &frame.view_projection_matrix,
        "view_projection_matrix",
    )
}

fn close_values<const N: usize>(
    actual: &[f64; N],
    expected: &[f64; N],
    field: &str,
) -> Result<(), String> {
    if let Some((index, (actual, expected))) = actual
        .iter()
        .zip(expected)
        .enumerate()
        .find(|(_, (actual, expected))| (*actual - *expected).abs() > CAMERA_TOLERANCE)
    {
        return Err(format!(
            "live camera {field}[{index}] mismatch: actual={actual} expected={expected}"
        ));
    }
    Ok(())
}

fn normalize_quaternion_f32(quaternion: [f32; 4]) -> [f32; 4] {
    let norm2 = quaternion.iter().map(|value| value * value).sum::<f32>();
    if norm2 <= 0.0 || !norm2.is_finite() {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let inverse_norm = 1.0 / norm2.sqrt();
    quaternion.map(|value| value * inverse_norm)
}

fn canonical_view_matrix_f32(camera: Camera) -> [f32; 16] {
    let [x, y, z, w] = normalize_quaternion_f32(camera.pose.rotation_xyzw);
    let [x, y, z, w] = normalize_quaternion_f32([-x, -y, -z, w]);
    let (xx, yy, zz) = (x * x, y * y, z * z);
    let (xy, xz, yz) = (x * y, x * z, y * z);
    let (wx, wy, wz) = (w * x, w * y, w * z);
    let rotation = [
        1.0 - 2.0 * (yy + zz),
        2.0 * (xy - wz),
        2.0 * (xz + wy),
        2.0 * (xy + wz),
        1.0 - 2.0 * (xx + zz),
        2.0 * (yz - wx),
        2.0 * (xz - wy),
        2.0 * (yz + wx),
        1.0 - 2.0 * (xx + yy),
    ];
    let position = [
        camera.pose.position.x,
        camera.pose.position.y,
        camera.pose.position.z,
    ];
    let translation = std::array::from_fn::<_, 3, _>(|row| {
        -rotation[row * 3 + 2].mul_add(
            position[2],
            rotation[row * 3 + 1].mul_add(position[1], rotation[row * 3] * position[0]),
        )
    });
    [
        rotation[0],
        rotation[1],
        rotation[2],
        translation[0],
        rotation[3],
        rotation[4],
        rotation[5],
        translation[1],
        rotation[6],
        rotation[7],
        rotation[8],
        translation[2],
        0.0,
        0.0,
        0.0,
        1.0,
    ]
}

fn canonical_projection_matrix_f32(camera: Camera, aspect: f32) -> [f32; 16] {
    let focal = 1.0 / (camera.intrinsics.vertical_fov_radians * 0.5).tan();
    let depth =
        camera.intrinsics.far_plane / (camera.intrinsics.far_plane - camera.intrinsics.near_plane);
    [
        focal / aspect,
        0.0,
        0.0,
        0.0,
        0.0,
        focal,
        0.0,
        0.0,
        0.0,
        0.0,
        depth,
        -camera.intrinsics.near_plane * depth,
        0.0,
        0.0,
        1.0,
        0.0,
    ]
}

fn multiply_mat4_f32(left: [f32; 16], right: [f32; 16]) -> [f32; 16] {
    std::array::from_fn(|index| {
        let row = index / 4;
        let column = index % 4;
        left[row * 4 + 3].mul_add(
            right[12 + column],
            left[row * 4 + 2].mul_add(
                right[8 + column],
                left[row * 4 + 1].mul_add(right[4 + column], left[row * 4] * right[column]),
            ),
        )
    })
}

#[cfg(feature = "qualification-q1-m4-native")]
pub(crate) fn format_f32_values(values: &[f32]) -> String {
    values
        .iter()
        .map(|value| format!("{value:.9}"))
        .collect::<Vec<_>>()
        .join(",")
}

pub(crate) const fn plan_label(plan: SurfaceCurrentStatsPlan) -> &'static str {
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

pub(crate) const fn count_semantics_label(
    semantics: SurfaceCurrentStatsCountSemantics,
) -> &'static str {
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

pub(crate) const fn backend_label(backend: SurfaceOrderBackendUsed) -> &'static str {
    match backend {
        SurfaceOrderBackendUsed::Cpu => "cpu",
        SurfaceOrderBackendUsed::Gpu => "gpu",
    }
}

#[cfg(feature = "qualification-q1-m4-native")]
pub(crate) const fn projected_execution_label(
    execution: SurfaceProjectedDrawExecution,
) -> &'static str {
    match execution {
        SurfaceProjectedDrawExecution::Candidate => "candidate",
        SurfaceProjectedDrawExecution::Compact => "compact",
    }
}

#[cfg(feature = "qualification-q1-m4-native")]
pub(crate) const fn adaptive_state_label(state: SurfaceAdaptiveState) -> &'static str {
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

#[cfg(feature = "qualification-q1-m4-native")]
pub(crate) const fn projected_adaptive_state_label(
    state: SurfaceProjectedDrawAdaptiveState,
) -> &'static str {
    match state {
        SurfaceProjectedDrawAdaptiveState::Disabled => "disabled",
        SurfaceProjectedDrawAdaptiveState::CandidateLearning => "candidate_learning",
        SurfaceProjectedDrawAdaptiveState::CandidateStable => "candidate_stable",
        SurfaceProjectedDrawAdaptiveState::CompactProbe => "compact_probe",
        SurfaceProjectedDrawAdaptiveState::CompactStable => "compact_stable",
        SurfaceProjectedDrawAdaptiveState::CandidateProbe => "candidate_probe",
        SurfaceProjectedDrawAdaptiveState::CandidateOnly => "candidate_only",
        SurfaceProjectedDrawAdaptiveState::Cooldown => "cooldown",
    }
}

#[cfg(feature = "qualification-q1-m4-native")]
pub(crate) fn output_plan_label(output: SurfaceFrameOutput) -> Result<&'static str, String> {
    match (
        output.order_backend,
        output.projected_draw_execution,
        output.gpu_order_producer,
    ) {
        (SurfaceOrderBackendUsed::Cpu, SurfaceProjectedDrawExecution::Candidate, None) => {
            Ok("cpu_post_sort")
        }
        (
            SurfaceOrderBackendUsed::Gpu,
            SurfaceProjectedDrawExecution::Candidate,
            Some(SurfaceGpuOrderProducer::PostSort),
        ) => Ok("gpu_post_sort"),
        (
            SurfaceOrderBackendUsed::Gpu,
            SurfaceProjectedDrawExecution::Compact,
            Some(SurfaceGpuOrderProducer::Preproject),
        ) => Ok("gpu_preproject"),
        actual => Err(format!(
            "presentation has an inadmissible executed plan: {actual:?}"
        )),
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
    fn live_f32_matrices_are_recomputed_close_to_frozen_trace() {
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/perf/trace/fixtures/quality/candidate-truck-quality-1920x1080-v1.json"
        ))
        .unwrap();
        let trace = gsplat_core::camera_trace::CameraTrace::from_json_slice(&bytes).unwrap();
        let playback = CameraTracePlayback::Sequence {
            trace: trace.clone(),
            frame_indices: vec![0, 1],
            warmup_frames: 20,
            measured_frames: 80,
            loops: 1,
        };
        for (frame_index, frame) in trace.frames.iter().enumerate() {
            let camera = frame.camera().unwrap();
            let view = canonical_view_matrix_f32(camera);
            let projection = canonical_projection_matrix_f32(camera, 1920.0 / 1080.0);
            let view_projection = multiply_mat4_f32(projection, view);
            let receipt = LiveCameraReceipt {
                revision: (frame_index + 1) as u64,
                surface_size: (1920, 1080),
                aspect: 1920.0_f32 / 1080.0_f32,
                camera,
                view_matrix: view,
                projection_matrix: projection,
                view_projection_matrix: view_projection,
            };
            assert_eq!(receipt.revision, (frame_index + 1) as u64);
            validate_live_camera(
                &playback,
                SurfaceTraceStep {
                    playback_index: frame_index,
                    phase: gsplat_core::camera_trace::CameraTraceSequencePhase::Measure,
                    loop_index: 0,
                    phase_frame_index: frame_index,
                    measured_sample_index: Some(frame_index),
                    trace_frame_index: frame_index,
                    timestamp_ns: frame.timestamp_ns,
                    camera,
                },
                receipt,
            )
            .unwrap();
        }
    }

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
            depth_precision_profile: "CandidateStable24",
            projected_cache_precision_profile: "CandidateAxes16",
            projected_axis_record_bytes: 8,
            resident_sh_codec_profile: "CandidateSigned8BandScale5",
            resident_sh_mantissa_bits: 8,
            resident_sh_symmetric_max_code: 127,
            resident_sh_point_scale_bits: 5,
            resident_sh_point_scale_max_code: 31,
            resident_sh_range_chunk_splats: 256,
            resident_sh_source_count: 11,
            resident_sh_encoded_count: 11,
            resident_sh_resident_count: 11,
            resident_sh_addressable_count: 11,
            resident_sh_source_degree: 3,
            resident_sh_resident_degree: 3,
            resident_sh_residual_coefficients_per_source: 45,
            resident_sh_plane_count: 3,
            resident_sh_bytes_per_source: 48,
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
            "SURFACE_DIAGNOSTIC_CAPTURE_RECEIPT depth_precision_profile=CandidateStable24 projected_cache_precision_profile=CandidateAxes16 projected_axis_record_bytes=8 resident_sh_codec_profile=CandidateSigned8BandScale5 resident_sh_mantissa_bits=8 resident_sh_symmetric_max_code=127 resident_sh_point_scale_bits=5 resident_sh_point_scale_max_code=31 resident_sh_range_chunk_splats=256 resident_sh_source_count=11 resident_sh_encoded_count=11 resident_sh_resident_count=11 resident_sh_addressable_count=11 resident_sh_source_degree=3 resident_sh_resident_degree=3 resident_sh_residual_coefficients_per_source=45 resident_sh_plane_count=3 resident_sh_bytes_per_source=48 scene_generation=1 camera_revision=2 viewport_generation=3 contract_generation=4 plan_set_generation=5 plan_id=GpuPostSort order_generation=6 presentation_sequence=7 width=1920 height=1080 rgba8_sha256=feed"
        );
    }

    #[cfg(all(
        feature = "diagnostic-surface-capture-receipt",
        not(target_arch = "wasm32")
    ))]
    #[test]
    fn diagnostic_camera_receipt_is_renderer_owned_and_presentation_bound() {
        let camera = Camera {
            pose: gsplat_core::CameraPose {
                position: gsplat_core::Vec3f::new(1.0, 2.0, 3.0),
                rotation_xyzw: [0.0, 0.0, 0.0, 1.0],
            },
            intrinsics: gsplat_core::CameraIntrinsics {
                vertical_fov_radians: 0.75,
                near_plane: 0.01,
                far_plane: 100.0,
                focal_length_x_over_y: 1.125,
            },
        };
        let live = LiveCameraReceipt {
            revision: 9,
            surface_size: (979, 546),
            aspect: 979.0 / 546.0,
            camera,
            view_matrix: [0.0; 16],
            projection_matrix: [0.0; 16],
            view_projection_matrix: [0.0; 16],
        };
        let receipt = DiagnosticCameraReceiptRecord::from_presented_identity(0, live, 9, 11)
            .expect("same-present camera receipt");
        assert_eq!(
            receipt.line(),
            "SURFACE_DIAGNOSTIC_CAPTURE_CAMERA_RECEIPT trace_frame=0 camera_revision=9 presentation_sequence=11 position_x=1 position_y=2 position_z=3 rotation_x=0 rotation_y=0 rotation_z=0 rotation_w=1 vertical_fov_radians=0.75 near_plane=0.01 far_plane=100 focal_length_x_over_y=1.125"
        );
        assert!(
            DiagnosticCameraReceiptRecord::from_presented_identity(0, live, 10, 11)
                .unwrap_err()
                .contains("camera revision")
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
