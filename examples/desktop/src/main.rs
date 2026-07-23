use std::env;
use std::f32::consts::PI;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::Instant;

#[cfg(feature = "interactive-viewer")]
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::Duration,
};

use gsplat_core::camera_trace::CameraTrace;
#[cfg(feature = "interactive-viewer")]
use gsplat_core::camera_trace::CameraTraceSequencePhase;
use gsplat_core::{Camera, RenderMode, RendererConfig, Vec3f};
#[cfg(feature = "interactive-viewer")]
use gsplat_core::{CameraIntrinsics, CameraPose};
use gsplat_io_ply::{DecodedPlySplat, load_ply, load_ply_summary, visit_ply_splats};
use gsplat_render_wgpu::{
    GeometryPath, Renderer, ResidentSceneBuilder, ResidentSourceSplat, SurfaceGpuOrderProducer,
    SurfaceOrderBackend,
};
#[cfg(feature = "interactive-viewer")]
use gsplat_render_wgpu::{
    SurfaceAdaptivePendingSample, SurfaceAdaptiveState, SurfaceCpuOrderMeasurement,
    SurfaceFrameOutput, SurfaceGpuProducerDrawScope, SurfaceGpuProducerMeasurement,
    SurfaceGpuProducerMeasurementFailure, SurfaceGpuProducerMeasurementSubmission,
    SurfaceGpuProducerMeasurementUnsampledReason, SurfaceOrderBackendUsed, SurfaceOrderMeasurement,
    SurfaceOrderMeasurementFailure, SurfaceOrderMeasurementSubmission,
    SurfaceOrderMeasurementUnsampledReason, SurfacePresenter, SurfaceProjectedDrawPolicy,
    SurfaceRasterExecutionPlan, SurfaceRenderSession, SurfaceTimingSource,
};
#[cfg(feature = "interactive-viewer")]
use winit::{
    dpi::PhysicalSize,
    event::{ElementState, Event, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::EventLoop,
    keyboard::{KeyCode, PhysicalKey},
    window::WindowAttributes,
};

fn main() {
    let args = match Args::parse(env::args().skip(1)) {
        Ok(args) => args,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(2);
        }
    };

    if let Err(err) = run(args) {
        eprintln!("desktop-example failed: {err}");
        std::process::exit(1);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum SurfaceBenchmarkMode {
    #[default]
    Isolated,
    Throughput,
}

impl SurfaceBenchmarkMode {
    #[cfg(feature = "interactive-viewer")]
    const fn label(self) -> &'static str {
        match self {
            Self::Isolated => "isolated",
            Self::Throughput => "throughput",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum SurfaceSortPolicyArg {
    #[default]
    EveryFrame,
    CameraChange,
}

impl SurfaceSortPolicyArg {
    #[cfg(feature = "interactive-viewer")]
    const fn label(self) -> &'static str {
        match self {
            Self::EveryFrame => "every_frame",
            Self::CameraChange => "camera_change",
        }
    }
}

/// Explicit Packed Surface A/B control. Projected quads are the product path;
/// tiled compute is retained as a correctness oracle and pressure diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum SurfaceRasterPlanArg {
    #[default]
    Projected,
    Global,
    Tiled,
}

impl SurfaceRasterPlanArg {
    #[cfg(feature = "interactive-viewer")]
    const fn execution_plan(self) -> SurfaceRasterExecutionPlan {
        match self {
            Self::Projected => SurfaceRasterExecutionPlan::ProjectedQuadsExact,
            Self::Global => SurfaceRasterExecutionPlan::GlobalQuads,
            Self::Tiled => SurfaceRasterExecutionPlan::TiledExact,
        }
    }
}

#[derive(Debug, Clone)]
struct Args {
    dataset_path: PathBuf,
    config: RendererConfig,
    frames: u32,
    orbit: bool,
    auto_camera: bool,
    yaw_deg: Option<f32>,
    interactive: bool,
    geometry_path: GeometryPath,
    order_backend: SurfaceOrderBackend,
    surface_benchmark_mode: SurfaceBenchmarkMode,
    #[cfg_attr(not(feature = "interactive-viewer"), allow(dead_code))]
    surface_sort_policy: SurfaceSortPolicyArg,
    #[cfg_attr(not(feature = "interactive-viewer"), allow(dead_code))]
    surface_raster_plan: SurfaceRasterPlanArg,
    /// `None` preserves the ordinary product defaults. `Some` is the explicit
    /// producer A/B mode and also enables its independent terminal receipts.
    #[cfg_attr(not(feature = "interactive-viewer"), allow(dead_code))]
    surface_gpu_producer: Option<SurfaceGpuOrderProducer>,
    png_out: Option<PathBuf>,
    camera_trace_path: Option<PathBuf>,
    camera_frame: usize,
    camera_sequence: bool,
    camera_frame_indices: Option<Vec<usize>>,
    camera_warmup_frames: usize,
    camera_measured_frames: Option<usize>,
    camera_loops: usize,
    camera_frame_explicit: bool,
    frames_explicit: bool,
    width_explicit: bool,
    height_explicit: bool,
}

impl Args {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut dataset_path: Option<PathBuf> = None;
        let mut config = RendererConfig {
            mode: RenderMode::SortedAlpha,
            ..RendererConfig::default()
        };
        let mut frames = 1_u32;
        let mut orbit = false;
        let mut auto_camera = false;
        let mut yaw_deg: Option<f32> = None;
        let mut interactive = false;
        let mut geometry_path = GeometryPath::PackedAtlas;
        let mut order_backend = SurfaceOrderBackend::Adaptive;
        let mut surface_benchmark_mode = SurfaceBenchmarkMode::Isolated;
        let mut surface_sort_policy = SurfaceSortPolicyArg::EveryFrame;
        let mut surface_sort_policy_explicit = false;
        let mut surface_raster_plan = SurfaceRasterPlanArg::Projected;
        let mut surface_raster_plan_explicit = false;
        let mut surface_gpu_producer = None;
        let mut order_backend_explicit = false;
        let mut png_out: Option<PathBuf> = None;
        let mut camera_trace_path: Option<PathBuf> = None;
        let mut camera_frame = 0_usize;
        let mut camera_sequence = false;
        let mut camera_frame_indices = None;
        let mut camera_warmup_frames = 0_usize;
        let mut camera_measured_frames = None;
        let mut camera_loops = 1_usize;
        let mut camera_frame_explicit = false;
        let mut frames_explicit = false;
        let mut width_explicit = false;
        let mut height_explicit = false;

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--help" | "-h" => {
                    return Err(usage());
                }
                "--frames" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --frames".to_owned())?;
                    frames = value
                        .parse::<u32>()
                        .map_err(|_| "invalid --frames".to_owned())?;
                    frames = frames.max(1);
                    frames_explicit = true;
                }
                "--width" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --width".to_owned())?;
                    config.width = value
                        .parse::<u32>()
                        .map_err(|_| "invalid --width".to_owned())?;
                    width_explicit = true;
                }
                "--height" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --height".to_owned())?;
                    config.height = value
                        .parse::<u32>()
                        .map_err(|_| "invalid --height".to_owned())?;
                    height_explicit = true;
                }
                "--orbit" => orbit = true,
                "--auto-camera" => auto_camera = true,
                "--yaw-deg" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --yaw-deg".to_owned())?;
                    yaw_deg = Some(
                        value
                            .parse::<f32>()
                            .map_err(|_| "invalid --yaw-deg".to_owned())?,
                    );
                }
                "--interactive" => interactive = true,
                "--geometry-path" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --geometry-path".to_owned())?;
                    geometry_path = parse_geometry_path(&value)?;
                }
                "--order-backend" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --order-backend".to_owned())?;
                    order_backend = parse_order_backend(&value)?;
                    order_backend_explicit = true;
                }
                "--surface-benchmark-mode" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --surface-benchmark-mode".to_owned())?;
                    surface_benchmark_mode = match value.as_str() {
                        "isolated" => SurfaceBenchmarkMode::Isolated,
                        "throughput" => SurfaceBenchmarkMode::Throughput,
                        _ => {
                            return Err(
                                "invalid --surface-benchmark-mode; expected isolated|throughput"
                                    .to_owned(),
                            );
                        }
                    };
                }
                "--surface-raster-plan" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --surface-raster-plan".to_owned())?;
                    surface_raster_plan = match value.as_str() {
                        "projected" => SurfaceRasterPlanArg::Projected,
                        "global" => SurfaceRasterPlanArg::Global,
                        "tiled" => SurfaceRasterPlanArg::Tiled,
                        _ => {
                            return Err(
                                "invalid --surface-raster-plan; expected projected|global|tiled"
                                    .to_owned(),
                            );
                        }
                    };
                    surface_raster_plan_explicit = true;
                }
                "--surface-sort-policy" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --surface-sort-policy".to_owned())?;
                    surface_sort_policy = match value.as_str() {
                        "every-frame" => SurfaceSortPolicyArg::EveryFrame,
                        "camera-change" => SurfaceSortPolicyArg::CameraChange,
                        _ => {
                            return Err(
                                "invalid --surface-sort-policy; expected every-frame|camera-change"
                                    .to_owned(),
                            );
                        }
                    };
                    surface_sort_policy_explicit = true;
                }
                "--surface-gpu-producer" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --surface-gpu-producer".to_owned())?;
                    surface_gpu_producer = Some(parse_gpu_order_producer(&value)?);
                }
                "--png" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --png".to_owned())?;
                    png_out = Some(PathBuf::from(value));
                }
                "--camera-trace" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --camera-trace".to_owned())?;
                    camera_trace_path = Some(PathBuf::from(value));
                }
                "--camera-frame" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --camera-frame".to_owned())?;
                    camera_frame = value
                        .parse::<usize>()
                        .map_err(|_| "invalid --camera-frame".to_owned())?;
                    camera_frame_explicit = true;
                }
                "--camera-sequence" => camera_sequence = true,
                "--camera-frame-indices" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --camera-frame-indices".to_owned())?;
                    camera_frame_indices = Some(parse_frame_indices(&value)?);
                }
                "--camera-warmup-frames" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --camera-warmup-frames".to_owned())?;
                    camera_warmup_frames = value
                        .parse::<usize>()
                        .map_err(|_| "invalid --camera-warmup-frames".to_owned())?;
                }
                "--camera-measured-frames" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --camera-measured-frames".to_owned())?;
                    let measured = value
                        .parse::<usize>()
                        .map_err(|_| "invalid --camera-measured-frames".to_owned())?;
                    if measured == 0 {
                        return Err("--camera-measured-frames must be positive".to_owned());
                    }
                    camera_measured_frames = Some(measured);
                }
                "--camera-loops" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --camera-loops".to_owned())?;
                    camera_loops = value
                        .parse::<usize>()
                        .map_err(|_| "invalid --camera-loops".to_owned())?;
                    if camera_loops == 0 {
                        return Err("--camera-loops must be positive".to_owned());
                    }
                }
                _ if arg.starts_with("--") => {
                    return Err(format!("unknown flag: {arg}\n\n{}", usage()));
                }
                _ => {
                    if dataset_path.is_some() {
                        return Err(format!("unexpected extra arg: {arg}\n\n{}", usage()));
                    }
                    dataset_path = Some(PathBuf::from(arg));
                }
            }
        }

        if !interactive && order_backend_explicit && order_backend != SurfaceOrderBackend::Cpu {
            return Err(
                "--order-backend gpu|adaptive requires --interactive Surface rendering".to_owned(),
            );
        }
        if interactive
            && geometry_path == GeometryPath::PagedActiveAtlas
            && order_backend != SurfaceOrderBackend::Cpu
        {
            return Err("paged geometry only supports --order-backend cpu".to_owned());
        }
        if surface_benchmark_mode == SurfaceBenchmarkMode::Throughput && !interactive {
            return Err("--surface-benchmark-mode throughput requires --interactive".to_owned());
        }
        if surface_raster_plan_explicit && !interactive {
            return Err("--surface-raster-plan requires --interactive".to_owned());
        }
        if surface_raster_plan_explicit && geometry_path != GeometryPath::PackedAtlas {
            return Err("--surface-raster-plan requires --geometry-path packed".to_owned());
        }
        if surface_sort_policy_explicit && !interactive {
            return Err("--surface-sort-policy requires --interactive".to_owned());
        }
        if surface_gpu_producer.is_some() && !interactive {
            return Err("--surface-gpu-producer requires --interactive".to_owned());
        }
        if surface_gpu_producer.is_some() && geometry_path != GeometryPath::PackedAtlas {
            return Err("--surface-gpu-producer requires --geometry-path packed".to_owned());
        }
        if surface_gpu_producer.is_some() && surface_raster_plan != SurfaceRasterPlanArg::Projected
        {
            return Err(
                "--surface-gpu-producer requires --surface-raster-plan projected".to_owned(),
            );
        }
        if surface_gpu_producer.is_some() && order_backend == SurfaceOrderBackend::Cpu {
            return Err("--surface-gpu-producer requires --order-backend gpu|adaptive".to_owned());
        }
        if interactive && png_out.is_some() && camera_trace_path.is_none() {
            return Err(
                "interactive --png requires --camera-trace Surface benchmark mode".to_owned(),
            );
        }
        if interactive && png_out.is_some() && surface_gpu_producer.is_none() {
            return Err(
                "interactive --png requires an explicit --surface-gpu-producer post-sort|preproject"
                    .to_owned(),
            );
        }

        Ok(Self {
            dataset_path: dataset_path.unwrap_or_else(|| "tests/datasets/minimal_ascii.ply".into()),
            config,
            frames,
            orbit,
            auto_camera,
            yaw_deg,
            interactive,
            geometry_path,
            order_backend,
            surface_benchmark_mode,
            surface_sort_policy,
            surface_raster_plan,
            surface_gpu_producer,
            png_out,
            camera_trace_path,
            camera_frame,
            camera_sequence,
            camera_frame_indices,
            camera_warmup_frames,
            camera_measured_frames,
            camera_loops,
            camera_frame_explicit,
            frames_explicit,
            width_explicit,
            height_explicit,
        })
    }
}

fn usage() -> String {
    let lines = [
        "usage: cargo run -p desktop-example -- [dataset.ply] [flags]",
        "",
        "flags:",
        "  --frames N       render N frames (default: 1)",
        "  --width W        output width (default: 1280)",
        "  --height H       output height (default: 720)",
        "  --orbit          animate camera yaw over frames",
        "  --auto-camera    place camera based on dataset bounds",
        "  --yaw-deg A      set a fixed yaw angle in degrees for static frame rendering",
        "  --interactive    launch realtime on-screen viewer loop (feature: interactive-viewer)",
        "  --geometry-path P use direct, packed, or paged geometry (default: packed)",
        "  --order-backend B select cpu, gpu, or adaptive Surface ordering (default: adaptive)",
        "  --surface-benchmark-mode M isolate each receipt or measure continuous throughput",
        "  --surface-raster-plan R select projected product, global reference, or tiled oracle",
        "  --surface-gpu-producer P run an exact post-sort|preproject GPU-producer A/B",
        "  --surface-sort-policy S refresh every frame or only when the trace camera changes",
        "  --png PATH       write the last rendered frame to PATH (requires GPU rasterizer)",
        "  --camera-trace P render one validated gsplat-camera-trace/v1 frame",
        "  --camera-frame N select trace frame N (default: 0; repeated for --frames)",
        "  --camera-sequence replay selected trace revisions in order",
        "  --camera-frame-indices L comma-separated sequence indices (default: all)",
        "  --camera-warmup-frames N repeat warmup poses before measurement (default: 0)",
        "  --camera-measured-frames N measured poses (fixed default: --frames; sequence: selected count)",
        "  --camera-loops N repeat the measured sequence N times (default: 1)",
    ];
    lines.join("\n")
}

fn run(mut args: Args) -> Result<(), String> {
    let trace_playback = load_camera_trace(&mut args)?;
    validate_surface_trace_geometry(&args, trace_playback.is_some())?;

    let mut renderer = if args.interactive {
        Renderer::with_config_for_surface(args.config)
    } else {
        Renderer::with_config(args.config)
    }
    .map_err(|err| err.to_string())?;
    renderer.set_geometry_path(args.geometry_path);
    load_ply_path_into_renderer(Path::new(&args.dataset_path), &mut renderer)?;

    let mut camera = if let Some(playback) = &trace_playback {
        playback.initial_camera()?
    } else if args.auto_camera {
        auto_camera(&renderer, args.config)
    } else {
        Camera::default()
    };
    if let Some(yaw_deg) = args.yaw_deg {
        let yaw = yaw_deg.to_radians();
        camera.pose.rotation_xyzw = [0.0, (yaw * 0.5).sin(), 0.0, (yaw * 0.5).cos()];
    }

    if args.interactive {
        return run_interactive(&args, renderer, camera, trace_playback.as_ref());
    }

    if let Some(playback) = &trace_playback {
        playback.print_header();
    }

    run_offscreen(&args, renderer, camera, trace_playback.as_ref())
}

fn validate_surface_trace_geometry(args: &Args, has_trace: bool) -> Result<(), String> {
    if args.interactive && args.geometry_path == GeometryPath::PagedActiveAtlas {
        if args.order_backend != SurfaceOrderBackend::Cpu {
            return Err("paged geometry only supports --order-backend cpu".to_owned());
        }
        if has_trace {
            return Err(
                "paged geometry is excluded from the full-quality Surface benchmark because it does not keep the complete source resident"
                    .to_owned(),
            );
        }
    }
    Ok(())
}

fn load_ply_path_into_renderer(path: &Path, renderer: &mut Renderer) -> Result<(), String> {
    if renderer.geometry_path() != GeometryPath::PackedAtlas {
        let loaded = load_ply(path).map_err(|error| error.to_string())?;
        return renderer
            .load_scene(loaded.scene)
            .map_err(|error| error.to_string());
    }

    let expected = load_ply_summary(path).map_err(|error| error.to_string())?;
    let mut builder = ResidentSceneBuilder::new(expected.gaussians, expected.sh_degree)
        .map_err(|error| error.to_string())?;
    let mut builder_error = None;
    let decoded = visit_ply_splats(path, |splat| {
        if builder_error.is_none() {
            builder_error = builder.push(resident_source_splat(splat)).err();
        }
    })
    .map_err(|error| error.to_string())?;
    if let Some(error) = builder_error {
        return Err(error.to_string());
    }
    if decoded != expected {
        return Err(format!(
            "PLY changed while loading: expected {expected:?}, decoded {decoded:?}"
        ));
    }
    let resident = builder.finish().map_err(|error| error.to_string())?;
    renderer
        .load_resident_scene(resident)
        .map_err(|error| error.to_string())
}

fn resident_source_splat(splat: &DecodedPlySplat) -> ResidentSourceSplat {
    ResidentSourceSplat {
        position: splat.position_ruf,
        opacity_logit: splat.opacity_logit,
        log_scale: splat.log_scale_xyz,
        rotation_xyzw: splat.rotation_xyzw,
        color_dc: splat.color_dc,
        sh_rest: splat.sh_rest,
        sh_len: splat.sh_rest_len,
        sh_degree: splat.sh_degree,
    }
}

#[derive(Debug, Clone)]
enum CameraTracePlayback {
    Fixed {
        trace: CameraTrace,
        frame_index: usize,
        warmup_frames: usize,
        measured_frames: usize,
    },
    Sequence {
        trace: CameraTrace,
        frame_indices: Vec<usize>,
        warmup_frames: usize,
        measured_frames: usize,
        loops: usize,
    },
}

impl CameraTracePlayback {
    fn trace(&self) -> &CameraTrace {
        match self {
            Self::Fixed { trace, .. } | Self::Sequence { trace, .. } => trace,
        }
    }

    fn initial_camera(&self) -> Result<Camera, String> {
        let index = match self {
            Self::Fixed { frame_index, .. } => *frame_index,
            Self::Sequence { frame_indices, .. } => frame_indices[0],
        };
        self.trace()
            .frame(index)
            .map_err(|error| error.to_string())?
            .camera()
            .map_err(|error| error.to_string())
    }

    fn print_header(&self) {
        let trace = self.trace();
        println!("camera_trace_id={}", trace.trace_id);
        println!("camera_trace_sha256={}", trace.content_sha256);
        println!("requested_backend=cpu");
        match self {
            Self::Fixed {
                frame_index,
                warmup_frames,
                measured_frames,
                ..
            } => {
                let frame = &trace.frames[*frame_index];
                println!("camera_trace_mode=fixed_frame");
                println!("camera_trace_frame={frame_index}");
                println!("camera_trace_timestamp_ns={}", frame.timestamp_ns);
                println!("camera_trace_warmup_frames={warmup_frames}");
                println!("camera_trace_measured_frames={measured_frames}");
            }
            Self::Sequence {
                frame_indices,
                warmup_frames,
                measured_frames,
                loops,
                ..
            } => {
                println!("camera_trace_mode=trace_sequence");
                println!(
                    "camera_trace_frame_indices={}",
                    frame_indices
                        .iter()
                        .map(usize::to_string)
                        .collect::<Vec<_>>()
                        .join(",")
                );
                println!("camera_trace_warmup_frames={warmup_frames}");
                println!("camera_trace_measured_frames={measured_frames}");
                println!("camera_trace_loops={loops}");
            }
        }
    }
}

fn load_camera_trace(args: &mut Args) -> Result<Option<CameraTracePlayback>, String> {
    let Some(path) = args.camera_trace_path.as_deref() else {
        if args.camera_frame_explicit
            || args.camera_sequence
            || args.camera_frame_indices.is_some()
            || args.camera_warmup_frames != 0
            || args.camera_measured_frames.is_some()
            || args.camera_loops != 1
            || args.surface_benchmark_mode == SurfaceBenchmarkMode::Throughput
        {
            return Err("camera trace playback flags require --camera-trace".to_owned());
        }
        return Ok(None);
    };
    if args.auto_camera || args.yaw_deg.is_some() || args.orbit {
        return Err(
            "--camera-trace cannot be combined with --auto-camera, --yaw-deg, or --orbit"
                .to_owned(),
        );
    }

    let bytes = fs::read(path)
        .map_err(|error| format!("cannot read camera trace {}: {error}", path.display()))?;
    let trace = CameraTrace::from_json_slice(&bytes)
        .map_err(|error| format!("invalid camera trace {}: {error}", path.display()))?;
    if args.width_explicit && args.config.width != trace.display.width {
        return Err(format!(
            "--width {} conflicts with trace display width {}",
            args.config.width, trace.display.width
        ));
    }
    if args.height_explicit && args.config.height != trace.display.height {
        return Err(format!(
            "--height {} conflicts with trace display height {}",
            args.config.height, trace.display.height
        ));
    }
    args.config.width = trace.display.width;
    args.config.height = trace.display.height;
    if args.camera_sequence {
        if args.camera_frame_explicit {
            return Err("--camera-frame cannot be combined with --camera-sequence".to_owned());
        }
        if args.frames_explicit {
            return Err(
                "--frames is fixed-frame playback only; use --camera-measured-frames for a sequence"
                    .to_owned(),
            );
        }
        let frame_indices = args
            .camera_frame_indices
            .clone()
            .unwrap_or_else(|| (0..trace.frames.len()).collect());
        let measured_frames = args.camera_measured_frames.unwrap_or(frame_indices.len());
        trace
            .sequence(
                frame_indices.clone(),
                args.camera_warmup_frames,
                measured_frames,
                args.camera_loops,
            )
            .map_err(|error| error.to_string())?;
        Ok(Some(CameraTracePlayback::Sequence {
            trace,
            frame_indices,
            warmup_frames: args.camera_warmup_frames,
            measured_frames,
            loops: args.camera_loops,
        }))
    } else {
        if args.camera_frame_indices.is_some() || args.camera_loops != 1 {
            return Err("sequence options require --camera-sequence".to_owned());
        }
        if args.frames_explicit && args.camera_measured_frames.is_some() {
            return Err(
                "--frames and --camera-measured-frames both control fixed-frame measurement length"
                    .to_owned(),
            );
        }
        trace
            .frame(args.camera_frame)
            .map_err(|error| error.to_string())?;
        Ok(Some(CameraTracePlayback::Fixed {
            trace,
            frame_index: args.camera_frame,
            warmup_frames: args.camera_warmup_frames,
            measured_frames: args.camera_measured_frames.unwrap_or(args.frames as usize),
        }))
    }
}

fn run_offscreen(
    args: &Args,
    mut renderer: Renderer,
    mut camera: Camera,
    trace_playback: Option<&CameraTracePlayback>,
) -> Result<(), String> {
    let start = Instant::now();
    let mut last_stats = None;
    let sequence = match trace_playback {
        Some(CameraTracePlayback::Sequence {
            trace,
            frame_indices,
            warmup_frames,
            measured_frames,
            loops,
            ..
        }) => Some(
            trace
                .sequence(
                    frame_indices.clone(),
                    *warmup_frames,
                    *measured_frames,
                    *loops,
                )
                .map_err(|error| error.to_string())?,
        ),
        _ => None,
    };
    let render_frames = match trace_playback {
        Some(CameraTracePlayback::Fixed {
            warmup_frames,
            measured_frames,
            ..
        }) => warmup_frames
            .checked_add(*measured_frames)
            .ok_or_else(|| "fixed camera trace schedule length overflowed usize".to_owned())?,
        Some(CameraTracePlayback::Sequence { .. }) => sequence
            .as_ref()
            .expect("sequence playback constructs a sequence")
            .len(),
        None => args.frames as usize,
    };
    for frame_index in 0..render_frames {
        if let Some(sequence) = &sequence {
            let step = sequence
                .step(frame_index)
                .ok_or_else(|| "camera trace sequence ended unexpectedly".to_owned())?;
            camera = step.frame.camera().map_err(|error| error.to_string())?;
            let trace = trace_playback
                .expect("a sequence can only exist with trace playback")
                .trace();
            println!(
                "CAMERA_TRACE_FRAME trace_id={} trace_sha256={} phase={} loop={} phase_frame={} frame_index={} timestamp_ns={} requested_backend=cpu",
                trace.trace_id,
                trace.content_sha256,
                step.phase.as_str(),
                step.loop_index,
                step.phase_frame_index,
                step.trace_frame_index,
                step.frame.timestamp_ns,
            );
        }
        if args.orbit && args.frames > 1 {
            let t = (frame_index as f32) / ((render_frames - 1) as f32);
            let angle = t * 2.0 * PI;
            camera.pose.rotation_xyzw = [0.0, (angle * 0.5).sin(), 0.0, (angle * 0.5).cos()];
        }

        let stats = renderer
            .render_frame(&camera)
            .map_err(|err| err.to_string())?;
        last_stats = Some(stats);
    }
    let elapsed = start.elapsed();
    let stats = last_stats.unwrap_or_default();

    if let Some(png_path) = args.png_out.as_deref() {
        let rgba = renderer.readback_rgba8().map_err(|err| err.to_string())?;
        write_png(png_path, args.config.width, args.config.height, &rgba)?;
        println!("wrote_png={}", png_path.display());
    }

    println!("desktop-example ok");
    println!("dataset={}", args.dataset_path.display());
    println!("gpu_rasterizer={}", renderer.has_gpu_rasterizer());
    println!(
        "offscreen_geometry_pipeline={}",
        geometry_path_label(args.geometry_path)
    );
    println!("frames={render_frames}");
    println!("elapsed_ms={:.4}", elapsed.as_secs_f32() * 1000.0);
    println!("frame_ms={:.4}", stats.frame_ms);
    println!("preprocess_ms={:.4}", stats.preprocess_ms);
    println!("sort_ms={:.4}", stats.sort_ms);
    println!("geometry_encode_submit_cpu_wall_ms={:.4}", stats.raster_ms);
    println!("visible_count={}", stats.visible_count);
    println!("drawn_count={}", stats.drawn_count);

    Ok(())
}

fn parse_frame_indices(value: &str) -> Result<Vec<usize>, String> {
    if value.trim().is_empty() {
        return Err("--camera-frame-indices must not be empty".to_owned());
    }
    value
        .split(',')
        .map(|part| {
            let part = part.trim();
            if part.is_empty() {
                return Err("invalid --camera-frame-indices".to_owned());
            }
            part.parse::<usize>()
                .map_err(|_| "invalid --camera-frame-indices".to_owned())
        })
        .collect()
}

fn parse_geometry_path(value: &str) -> Result<GeometryPath, String> {
    match value {
        "direct" => Ok(GeometryPath::SortedIndexDirect),
        "packed" => Ok(GeometryPath::PackedAtlas),
        "paged" => Ok(GeometryPath::PagedActiveAtlas),
        _ => Err(format!(
            "invalid --geometry-path '{value}' (expected direct|packed|paged)"
        )),
    }
}

fn parse_order_backend(value: &str) -> Result<SurfaceOrderBackend, String> {
    match value {
        "cpu" => Ok(SurfaceOrderBackend::Cpu),
        "gpu" => Ok(SurfaceOrderBackend::Gpu),
        "adaptive" => Ok(SurfaceOrderBackend::Adaptive),
        _ => Err(format!(
            "invalid --order-backend '{value}' (expected cpu|gpu|adaptive)"
        )),
    }
}

fn parse_gpu_order_producer(value: &str) -> Result<SurfaceGpuOrderProducer, String> {
    match value {
        "post-sort" => Ok(SurfaceGpuOrderProducer::PostSort),
        "preproject" => Ok(SurfaceGpuOrderProducer::Preproject),
        _ => Err(format!(
            "invalid --surface-gpu-producer '{value}' (expected post-sort|preproject)"
        )),
    }
}

const fn geometry_path_label(path: GeometryPath) -> &'static str {
    match path {
        GeometryPath::SortedIndexDirect => "sorted_index_direct",
        GeometryPath::PackedAtlas => "packed_atlas",
        GeometryPath::PagedActiveAtlas => "paged_active_atlas",
    }
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
    let presenter = pollster::block_on(SurfacePresenter::from_window(
        window.clone(),
        args.config.width,
        args.config.height,
        &renderer,
    ))
    .map_err(|err| err.to_string())?;
    let mut session =
        SurfaceRenderSession::new(renderer, presenter, camera).map_err(|err| err.to_string())?;
    session
        .set_sort_interval(1)
        .map_err(|err| err.to_string())?;
    if let Some(producer) = args.surface_gpu_producer {
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
    } else if args.geometry_path == GeometryPath::PackedAtlas {
        session
            .set_raster_execution_plan(args.surface_raster_plan.execution_plan())
            .map_err(|err| err.to_string())?;
    }
    session
        .set_order_backend(args.order_backend)
        .map_err(|err| err.to_string())?;

    if let Some(playback) = trace_playback {
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
struct SurfaceTraceStep {
    playback_index: usize,
    phase: CameraTraceSequencePhase,
    loop_index: usize,
    phase_frame_index: usize,
    measured_sample_index: Option<usize>,
    trace_frame_index: usize,
    timestamp_ns: u64,
    camera: Camera,
}

#[cfg(feature = "interactive-viewer")]
impl SurfaceTraceStep {
    const fn measured(self) -> bool {
        matches!(self.phase, CameraTraceSequencePhase::Measure)
    }
}

#[cfg(feature = "interactive-viewer")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SurfaceResolutionReceipt {
    requested: (u32, u32),
    surface: (u32, u32),
    internal_render: (u32, u32),
}

#[cfg(feature = "interactive-viewer")]
impl SurfaceResolutionReceipt {
    fn validated(
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

    const fn full_resolution(self) -> bool {
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
fn surface_trace_steps(playback: &CameraTracePlayback) -> Result<Vec<SurfaceTraceStep>, String> {
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
struct SurfaceBenchmarkTicket {
    backend: SurfaceOrderBackendUsed,
    camera_revision: u64,
    measured: bool,
}

#[cfg(feature = "interactive-viewer")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SurfaceBenchmarkProducerTicket {
    producer: SurfaceGpuOrderProducer,
    camera_revision: u64,
    measured: bool,
}

#[cfg(feature = "interactive-viewer")]
#[derive(Debug, Default)]
struct SurfaceBenchmarkSummary {
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
    cpu_completion_ms: Vec<f32>,
    frame_wall_ms: Vec<f32>,
    gpu_order_ms: Vec<f32>,
    gpu_completion_ms: Vec<f32>,
    gpu_timestamp_measurements: usize,
    gpu_completion_only_measurements: usize,
    gpu_incomplete_timestamp_measurements: usize,
    gpu_refreshes_without_ticket: usize,
    cpu_requests_without_ticket: usize,
    surface_unavailable_measurements: usize,
    outstanding_order_tickets: HashMap<u64, SurfaceBenchmarkTicket>,
    terminal_order_tickets: HashSet<u64>,
    outstanding_producer_tickets: HashMap<u64, SurfaceBenchmarkProducerTicket>,
    terminal_producer_tickets: HashSet<u64>,
    producer_completion_ms: Vec<f32>,
    producer_exact_frames: usize,
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

    fn record_producer_measurement(
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

    fn record_cpu_measurement(
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

    fn has_outstanding_tickets(&self) -> bool {
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
enum SurfaceBenchmarkAction {
    RenderTraceStep,
    PollReceipts,
    Complete,
}

#[cfg(feature = "interactive-viewer")]
fn surface_benchmark_action(
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
        && raster_execution_plan != args.surface_raster_plan.execution_plan()
    {
        return Err(format!(
            "full-quality Packed Surface benchmark requested {:?}, got {raster_execution_plan:?}",
            args.surface_raster_plan.execution_plan(),
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

fn auto_camera(renderer: &Renderer, config: RendererConfig) -> Camera {
    let mut camera = Camera::default();
    camera.intrinsics.vertical_fov_radians = 60.0_f32.to_radians();

    let Some(positions) = renderer.positions() else {
        return camera;
    };
    let Some((min, max)) = positions_bounds(positions) else {
        return camera;
    };

    let center = Vec3f::new(
        (min.x + max.x) * 0.5,
        (min.y + max.y) * 0.5,
        (min.z + max.z) * 0.5,
    );
    let extent = Vec3f::new(max.x - min.x, max.y - min.y, max.z - min.z);
    let half_x = (extent.x * 0.5).max(1e-3);
    let half_y = (extent.y * 0.5).max(1e-3);
    let half_z = (extent.z * 0.5).max(1e-3);

    let aspect = (config.width as f32) / (config.height as f32);
    let vfov = camera.intrinsics.vertical_fov_radians.max(1e-3);
    let hfov = 2.0 * ((vfov * 0.5).tan() * aspect).atan();

    let dist_y = half_y / (vfov * 0.5).tan();
    let dist_x = half_x / (hfov * 0.5).tan();
    // Keep enough standoff for thick scenes; using only x/y fit can place the camera too close
    // to the frontmost Gaussians and amplify projection anisotropy into visible streaking.
    let base_dist = dist_y.max(dist_x);
    let depth_aware_dist = base_dist + half_z;
    let dist = depth_aware_dist * 1.2;
    camera.pose.position = Vec3f::new(center.x, center.y, center.z - dist);
    camera.pose.rotation_xyzw = [0.0, 0.0, 0.0, 1.0];

    // Keep conservative planes to avoid accidental clipping when orbiting.
    let radius = half_x.max(half_y).max((extent.z * 0.5).max(1e-3));
    camera.intrinsics.near_plane = (dist - radius * 2.0).max(0.01);
    camera.intrinsics.far_plane = (dist + radius * 8.0).max(100.0);

    camera
}

#[cfg(test)]
fn scene_bounds(scene: &gsplat_core::SceneBuffers) -> Option<(Vec3f, Vec3f)> {
    positions_bounds(&scene.positions)
}

fn positions_bounds(positions: &[Vec3f]) -> Option<(Vec3f, Vec3f)> {
    if positions.is_empty() {
        return None;
    }
    let mut min = Vec3f::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
    let mut max = Vec3f::new(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    for p in positions {
        min.x = min.x.min(p.x);
        min.y = min.y.min(p.y);
        min.z = min.z.min(p.z);
        max.x = max.x.max(p.x);
        max.y = max.y.max(p.y);
        max.z = max.z.max(p.z);
    }
    Some((min, max))
}

#[cfg(feature = "interactive-viewer")]
fn positions_center(positions: &[Vec3f]) -> Option<Vec3f> {
    let (min, max) = positions_bounds(positions)?;
    Some(Vec3f::new(
        (min.x + max.x) * 0.5,
        (min.y + max.y) * 0.5,
        (min.z + max.z) * 0.5,
    ))
}

fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    if rgba.len()
        != (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4)
    {
        return Err("png write failed: rgba buffer size mismatch".to_owned());
    }

    let file = File::create(path).map_err(|err| err.to_string())?;
    let mut encoder = png::Encoder::new(file, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);

    let mut writer = encoder.write_header().map_err(|err| err.to_string())?;
    writer
        .write_image_data(rgba)
        .map_err(|err| err.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use gsplat_core::{RenderMode, RendererConfig, SceneBuffers, Vec3f};
    use gsplat_render_wgpu::{
        GeometryPath, Renderer, SurfaceGpuOrderProducer, SurfaceOrderBackend,
    };
    #[cfg(feature = "interactive-viewer")]
    use gsplat_render_wgpu::{
        SurfaceAdaptivePendingSample, SurfaceCpuOrderMeasurement, SurfaceGpuProducerDrawScope,
        SurfaceGpuProducerMeasurement, SurfaceOrderBackendUsed,
    };

    use super::{
        Args, CameraTracePlayback, SurfaceBenchmarkMode, SurfaceRasterPlanArg,
        SurfaceSortPolicyArg, auto_camera, load_camera_trace, load_ply_path_into_renderer,
        scene_bounds, validate_surface_trace_geometry, write_png,
    };
    #[cfg(feature = "interactive-viewer")]
    use super::{
        SurfaceBenchmarkAction, SurfaceBenchmarkProducerTicket, SurfaceBenchmarkSummary,
        SurfaceBenchmarkTicket, SurfaceResolutionReceipt, surface_benchmark_action,
    };

    const CAMERA_TRACE_FIXTURE: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/perf/trace/fixtures/camera-trace-v1.json"
    );

    fn parse_args(args: &[&str]) -> Result<Args, String> {
        Args::parse(args.iter().map(|value| value.to_string()))
    }

    fn scene_with_positions(positions: Vec<Vec3f>) -> SceneBuffers {
        let len = positions.len();
        SceneBuffers {
            positions,
            opacity: vec![1.0; len],
            scale_xyz: vec![[0.0, 0.0, 0.0]; len],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; len],
            color_dc: vec![[0.1, 0.2, 0.3]; len],
            sh_degree: 0,
            sh_rest: None,
        }
    }

    #[cfg(feature = "interactive-viewer")]
    #[test]
    fn adaptive_pending_sample_blocks_trace_advance_and_benchmark_completion() {
        let pending = Some(SurfaceAdaptivePendingSample {
            backend: SurfaceOrderBackendUsed::Cpu,
            ticket: 2,
        });
        assert_eq!(
            surface_benchmark_action(SurfaceBenchmarkMode::Isolated, 0, 1, false, pending),
            SurfaceBenchmarkAction::PollReceipts
        );
        assert_eq!(
            surface_benchmark_action(SurfaceBenchmarkMode::Isolated, 1, 1, false, pending),
            SurfaceBenchmarkAction::PollReceipts
        );
        assert_eq!(
            surface_benchmark_action(SurfaceBenchmarkMode::Isolated, 0, 1, true, None),
            SurfaceBenchmarkAction::PollReceipts
        );
        assert_eq!(
            surface_benchmark_action(SurfaceBenchmarkMode::Isolated, 1, 1, true, None),
            SurfaceBenchmarkAction::PollReceipts
        );
        assert_eq!(
            surface_benchmark_action(SurfaceBenchmarkMode::Isolated, 0, 1, false, None),
            SurfaceBenchmarkAction::RenderTraceStep
        );
        assert_eq!(
            surface_benchmark_action(SurfaceBenchmarkMode::Isolated, 1, 1, false, None),
            SurfaceBenchmarkAction::Complete
        );
        assert_eq!(
            surface_benchmark_action(SurfaceBenchmarkMode::Throughput, 0, 1, true, pending),
            SurfaceBenchmarkAction::RenderTraceStep
        );
        assert_eq!(
            surface_benchmark_action(SurfaceBenchmarkMode::Throughput, 1, 1, true, pending),
            SurfaceBenchmarkAction::PollReceipts
        );
    }

    #[cfg(feature = "interactive-viewer")]
    #[test]
    fn surface_resolution_receipt_fails_closed_on_any_scaled_stage() {
        let receipt =
            SurfaceResolutionReceipt::validated((1920, 1080), (1920, 1080), (1920, 1080)).unwrap();
        assert!(receipt.full_resolution());
        assert!(
            SurfaceResolutionReceipt::validated((1920, 1080), (1280, 720), (1920, 1080),).is_err()
        );
        assert!(
            SurfaceResolutionReceipt::validated((1920, 1080), (1920, 1080), (960, 540),).is_err()
        );
    }

    #[cfg(feature = "interactive-viewer")]
    #[test]
    fn benchmark_ticket_ledger_accepts_exactly_one_matching_cpu_terminal() {
        let mut summary = SurfaceBenchmarkSummary::default();
        summary.outstanding_order_tickets.insert(
            2,
            SurfaceBenchmarkTicket {
                backend: SurfaceOrderBackendUsed::Cpu,
                camera_revision: 7,
                measured: true,
            },
        );
        let measurement = SurfaceCpuOrderMeasurement {
            ticket: 2,
            camera_revision: 7,
            preprocess_ms: 1.0,
            sort_ms: 2.0,
            frame_complete_ms: 4.0,
            visible_count: 10,
            contributor_count: 8,
            drawn_count: 8,
            exact_contributor_compaction: true,
        };

        assert_eq!(summary.record_cpu_measurement(measurement), Ok(true));
        assert!(!summary.has_outstanding_tickets());
        assert_eq!(summary.cpu_completion_ms, vec![4.0]);
        assert!(
            summary
                .record_cpu_measurement(measurement)
                .unwrap_err()
                .contains("more than one terminal")
        );
    }

    #[cfg(feature = "interactive-viewer")]
    #[test]
    fn producer_ticket_ledger_requires_matching_identity_and_exact_scope_counts() {
        let ticket = 1_u64 << 51;
        let mut summary = SurfaceBenchmarkSummary::default();
        summary.outstanding_producer_tickets.insert(
            ticket,
            SurfaceBenchmarkProducerTicket {
                producer: SurfaceGpuOrderProducer::Preproject,
                camera_revision: 11,
                measured: true,
            },
        );
        let measurement = SurfaceGpuProducerMeasurement {
            ticket,
            camera_revision: 11,
            producer: SurfaceGpuOrderProducer::Preproject,
            order_generation: 3,
            projection_generation: 5,
            source_count: 100,
            contributor_count: 73,
            drawn_count: 73,
            order_refreshed: true,
            draw_scope: SurfaceGpuProducerDrawScope::ExactCurrentContributors,
            frame_complete_ms: 7.5,
        };

        assert_eq!(
            summary.record_producer_measurement(measurement, 100),
            Ok(true)
        );
        assert!(!summary.has_outstanding_tickets());
        assert_eq!(summary.producer_completion_ms, vec![7.5]);
        assert_eq!(summary.producer_exact_frames, 1);
        assert!(
            summary
                .record_producer_measurement(measurement, 100)
                .unwrap_err()
                .contains("more than one terminal")
        );

        let invalid_ticket = ticket + 1;
        summary.outstanding_producer_tickets.insert(
            invalid_ticket,
            SurfaceBenchmarkProducerTicket {
                producer: SurfaceGpuOrderProducer::Preproject,
                camera_revision: 12,
                measured: false,
            },
        );
        let invalid = SurfaceGpuProducerMeasurement {
            ticket: invalid_ticket,
            camera_revision: 12,
            drawn_count: 72,
            ..measurement
        };
        assert!(
            summary
                .record_producer_measurement(invalid, 100)
                .unwrap_err()
                .contains("requires refreshed D=C")
        );
    }

    #[test]
    fn packed_path_loader_builds_resident_scene_without_wide_source() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/datasets/minimal_ascii.ply");
        let mut direct = Renderer::with_config_for_surface(RendererConfig::default()).unwrap();
        load_ply_path_into_renderer(&path, &mut direct).unwrap();
        let direct_positions = direct.positions().unwrap().to_vec();
        assert!(direct.scene().is_some());

        let mut packed = Renderer::with_config_for_surface(RendererConfig::default()).unwrap();
        packed.set_geometry_path(GeometryPath::PackedAtlas);
        load_ply_path_into_renderer(&path, &mut packed).unwrap();
        assert!(packed.scene().is_none());
        assert!(packed.resident_scene().is_some());
        assert_eq!(packed.positions(), Some(direct_positions.as_slice()));
        assert_eq!(packed.scene_len(), direct.scene_len());
        assert_eq!(packed.scene_sh_degree(), direct.scene_sh_degree());
    }

    #[test]
    fn packed_path_loader_does_not_publish_a_partially_decoded_scene() {
        let valid_path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/datasets/minimal_ascii.ply");
        let mut renderer = Renderer::with_config_for_surface(RendererConfig::default()).unwrap();
        renderer.set_geometry_path(GeometryPath::PackedAtlas);
        load_ply_path_into_renderer(&valid_path, &mut renderer).unwrap();
        let original_positions = renderer.positions().unwrap().to_vec();

        let mut malformed = fs::read_to_string(&valid_path)
            .unwrap()
            .replace("element vertex 3", "element vertex 4");
        malformed.push_str("not a valid vertex row\n");
        let malformed_path = std::env::temp_dir().join(format!(
            "gsplat-desktop-transactional-resident-{}.ply",
            std::process::id()
        ));
        fs::write(&malformed_path, malformed).unwrap();
        let result = load_ply_path_into_renderer(&malformed_path, &mut renderer);
        let _ = fs::remove_file(&malformed_path);

        assert!(result.is_err());
        assert_eq!(renderer.positions(), Some(original_positions.as_slice()));
        assert_eq!(renderer.scene_len(), Some(original_positions.len()));
    }

    #[test]
    fn args_parse_defaults_to_minimal_dataset() {
        let args = parse_args(&[]).unwrap();

        assert_eq!(
            args.dataset_path,
            std::path::PathBuf::from("tests/datasets/minimal_ascii.ply")
        );
        assert_eq!(args.config.width, 1280);
        assert_eq!(args.config.height, 720);
        assert_eq!(args.config.mode, RenderMode::SortedAlpha);
        assert_eq!(args.frames, 1);
        assert!(!args.orbit);
        assert!(!args.auto_camera);
        assert!(!args.interactive);
        assert_eq!(args.geometry_path, GeometryPath::PackedAtlas);
        assert_eq!(args.order_backend, SurfaceOrderBackend::Adaptive);
        assert_eq!(args.surface_benchmark_mode, SurfaceBenchmarkMode::Isolated);
        assert_eq!(args.surface_sort_policy, SurfaceSortPolicyArg::EveryFrame);
        assert_eq!(args.surface_raster_plan, SurfaceRasterPlanArg::Projected);
        assert_eq!(args.surface_gpu_producer, None);
        assert!(args.png_out.is_none());
        assert!(args.camera_trace_path.is_none());
        assert_eq!(args.camera_frame, 0);
        assert!(!args.camera_sequence);
        assert!(args.camera_frame_indices.is_none());
        assert_eq!(args.camera_warmup_frames, 0);
        assert!(args.camera_measured_frames.is_none());
        assert_eq!(args.camera_loops, 1);
    }

    #[test]
    fn args_parse_flags_and_clamps_frames() {
        let args = parse_args(&[
            "scene.ply",
            "--frames",
            "0",
            "--width",
            "640",
            "--height",
            "480",
            "--orbit",
            "--auto-camera",
            "--yaw-deg",
            "15",
            "--geometry-path",
            "paged",
            "--png",
            "target/out.png",
        ])
        .unwrap();

        assert_eq!(args.dataset_path, std::path::PathBuf::from("scene.ply"));
        assert_eq!(args.frames, 1);
        assert_eq!(args.config.width, 640);
        assert_eq!(args.config.height, 480);
        assert!(args.orbit);
        assert!(args.auto_camera);
        assert_eq!(args.yaw_deg, Some(15.0));
        assert_eq!(args.geometry_path, GeometryPath::PagedActiveAtlas);
        assert_eq!(
            args.png_out,
            Some(std::path::PathBuf::from("target/out.png"))
        );
    }

    #[test]
    fn args_parse_rejects_unknown_and_extra_args() {
        let err = parse_args(&["--nope"]).unwrap_err();
        assert!(err.contains("unknown flag: --nope"));

        let err = parse_args(&["a.ply", "b.ply"]).unwrap_err();
        assert!(err.contains("unexpected extra arg: b.ply"));
    }

    #[test]
    fn args_parse_surface_order_backends_and_rejects_non_surface_gpu() {
        for (label, expected) in [
            ("cpu", SurfaceOrderBackend::Cpu),
            ("gpu", SurfaceOrderBackend::Gpu),
            ("adaptive", SurfaceOrderBackend::Adaptive),
        ] {
            let args = parse_args(&["--interactive", "--order-backend", label]).unwrap();
            assert_eq!(args.order_backend, expected);
        }

        assert!(
            parse_args(&["--order-backend", "gpu"])
                .unwrap_err()
                .contains("requires --interactive")
        );
        assert!(
            parse_args(&["--interactive", "--order-backend", "automatic"])
                .unwrap_err()
                .contains("expected cpu|gpu|adaptive")
        );
        assert!(
            parse_args(&[
                "--interactive",
                "--geometry-path",
                "paged",
                "--order-backend",
                "adaptive",
            ])
            .unwrap_err()
            .contains("paged geometry")
        );

        let paged_cpu = parse_args(&[
            "--interactive",
            "--geometry-path",
            "paged",
            "--order-backend",
            "cpu",
        ])
        .unwrap();
        assert!(validate_surface_trace_geometry(&paged_cpu, false).is_ok());
        assert!(
            validate_surface_trace_geometry(&paged_cpu, true)
                .unwrap_err()
                .contains("full-quality")
        );

        assert!(
            parse_args(&["--surface-benchmark-mode", "throughput"])
                .unwrap_err()
                .contains("requires --interactive")
        );
        let throughput =
            parse_args(&["--interactive", "--surface-benchmark-mode", "throughput"]).unwrap();
        assert_eq!(
            throughput.surface_benchmark_mode,
            SurfaceBenchmarkMode::Throughput
        );

        for (label, expected) in [
            ("projected", SurfaceRasterPlanArg::Projected),
            ("global", SurfaceRasterPlanArg::Global),
            ("tiled", SurfaceRasterPlanArg::Tiled),
        ] {
            let args = parse_args(&[
                "--interactive",
                "--geometry-path",
                "packed",
                "--surface-raster-plan",
                label,
            ])
            .unwrap();
            assert_eq!(args.surface_raster_plan, expected);
        }
        assert!(
            parse_args(&["--surface-raster-plan", "projected"])
                .unwrap_err()
                .contains("requires --interactive")
        );
        assert!(
            parse_args(&[
                "--interactive",
                "--geometry-path",
                "direct",
                "--surface-raster-plan",
                "tiled",
            ])
            .unwrap_err()
            .contains("requires --geometry-path packed")
        );
        assert!(
            parse_args(&["--interactive", "--surface-raster-plan", "software",])
                .unwrap_err()
                .contains("expected projected|global|tiled")
        );
        for (label, expected) in [
            ("every-frame", SurfaceSortPolicyArg::EveryFrame),
            ("camera-change", SurfaceSortPolicyArg::CameraChange),
        ] {
            let args = parse_args(&["--interactive", "--surface-sort-policy", label]).unwrap();
            assert_eq!(args.surface_sort_policy, expected);
        }
        assert!(
            parse_args(&["--surface-sort-policy", "camera-change"])
                .unwrap_err()
                .contains("requires --interactive")
        );
        assert!(
            parse_args(&["--interactive", "--surface-sort-policy", "never"])
                .unwrap_err()
                .contains("expected every-frame|camera-change")
        );

        for (label, expected) in [
            ("post-sort", SurfaceGpuOrderProducer::PostSort),
            ("preproject", SurfaceGpuOrderProducer::Preproject),
        ] {
            let args = parse_args(&[
                "--interactive",
                "--order-backend",
                "gpu",
                "--surface-gpu-producer",
                label,
            ])
            .unwrap();
            assert_eq!(args.surface_gpu_producer, Some(expected));
        }
        assert!(
            parse_args(&["--surface-gpu-producer", "preproject"])
                .unwrap_err()
                .contains("requires --interactive")
        );
        assert!(
            parse_args(&[
                "--interactive",
                "--order-backend",
                "cpu",
                "--surface-gpu-producer",
                "preproject",
            ])
            .unwrap_err()
            .contains("requires --order-backend gpu|adaptive")
        );
        assert!(
            parse_args(&[
                "--interactive",
                "--geometry-path",
                "direct",
                "--order-backend",
                "gpu",
                "--surface-gpu-producer",
                "preproject",
            ])
            .unwrap_err()
            .contains("requires --geometry-path packed")
        );
        assert!(
            parse_args(&[
                "--interactive",
                "--order-backend",
                "gpu",
                "--surface-raster-plan",
                "global",
                "--surface-gpu-producer",
                "preproject",
            ])
            .unwrap_err()
            .contains("requires --surface-raster-plan projected")
        );
        assert!(
            parse_args(&[
                "--interactive",
                "--order-backend",
                "gpu",
                "--surface-gpu-producer",
                "automatic",
            ])
            .unwrap_err()
            .contains("expected post-sort|preproject")
        );
    }

    #[test]
    fn interactive_png_is_restricted_to_explicit_producer_trace_benchmarks() {
        assert!(
            parse_args(&["--interactive", "--png", "target/capture.png"])
                .unwrap_err()
                .contains("requires --camera-trace")
        );
        assert!(
            parse_args(&[
                "--interactive",
                "--camera-trace",
                CAMERA_TRACE_FIXTURE,
                "--png",
                "target/capture.png",
            ])
            .unwrap_err()
            .contains("requires an explicit --surface-gpu-producer")
        );
        let args = parse_args(&[
            "--interactive",
            "--camera-trace",
            CAMERA_TRACE_FIXTURE,
            "--order-backend",
            "gpu",
            "--surface-gpu-producer",
            "post-sort",
            "--png",
            "target/capture.png",
        ])
        .expect("valid diagnostic capture");
        assert_eq!(
            args.png_out,
            Some(std::path::PathBuf::from("target/capture.png"))
        );
    }

    #[test]
    fn fixed_camera_trace_sets_contract_display_and_selected_frame() {
        let mut args = parse_args(&[
            "--camera-trace",
            CAMERA_TRACE_FIXTURE,
            "--camera-frame",
            "2",
        ])
        .unwrap();

        let selected = load_camera_trace(&mut args).unwrap().unwrap();

        assert_eq!((args.config.width, args.config.height), (640, 360));
        assert!(matches!(
            &selected,
            CameraTracePlayback::Fixed { frame_index, .. } if *frame_index == 2
        ));
        assert_eq!(
            selected.initial_camera().unwrap().pose.position,
            Vec3f::new(0.5, 0.125, -3.0)
        );
    }

    #[test]
    fn fixed_camera_trace_rejects_conflicting_controls_and_display() {
        let mut args = parse_args(&["--camera-trace", CAMERA_TRACE_FIXTURE, "--orbit"]).unwrap();
        assert!(
            load_camera_trace(&mut args)
                .unwrap_err()
                .contains("cannot be combined")
        );

        let mut args =
            parse_args(&["--camera-trace", CAMERA_TRACE_FIXTURE, "--width", "800"]).unwrap();
        assert!(
            load_camera_trace(&mut args)
                .unwrap_err()
                .contains("conflicts")
        );

        let mut args = parse_args(&[
            "--interactive",
            "--camera-trace",
            CAMERA_TRACE_FIXTURE,
            "--order-backend",
            "gpu",
        ])
        .unwrap();
        assert!(load_camera_trace(&mut args).unwrap().is_some());

        let mut args = parse_args(&[
            "--camera-trace",
            CAMERA_TRACE_FIXTURE,
            "--frames",
            "3",
            "--camera-measured-frames",
            "4",
        ])
        .unwrap();
        assert!(
            load_camera_trace(&mut args)
                .unwrap_err()
                .contains("both control fixed-frame measurement length")
        );
    }

    #[test]
    fn throughput_surface_benchmark_requires_a_trace_but_accepts_a_fixed_pose() {
        let mut no_trace =
            parse_args(&["--interactive", "--surface-benchmark-mode", "throughput"]).unwrap();
        assert!(
            load_camera_trace(&mut no_trace)
                .unwrap_err()
                .contains("require --camera-trace")
        );

        let mut fixed = parse_args(&[
            "--interactive",
            "--camera-trace",
            CAMERA_TRACE_FIXTURE,
            "--surface-benchmark-mode",
            "throughput",
        ])
        .unwrap();
        assert!(matches!(
            load_camera_trace(&mut fixed).unwrap(),
            Some(CameraTracePlayback::Fixed {
                warmup_frames: 0,
                measured_frames: 1,
                ..
            })
        ));
    }

    #[test]
    fn trace_sequence_defaults_to_each_revision_once_and_accepts_explicit_schedule() {
        let mut args =
            parse_args(&["--camera-trace", CAMERA_TRACE_FIXTURE, "--camera-sequence"]).unwrap();
        let playback = load_camera_trace(&mut args).unwrap().unwrap();
        match playback {
            CameraTracePlayback::Sequence {
                frame_indices,
                warmup_frames,
                measured_frames,
                loops,
                ..
            } => {
                assert_eq!(frame_indices, vec![0, 1, 2]);
                assert_eq!(warmup_frames, 0);
                assert_eq!(measured_frames, 3);
                assert_eq!(loops, 1);
            }
            CameraTracePlayback::Fixed { .. } => panic!("expected sequence"),
        }

        let mut args = parse_args(&[
            "--camera-trace",
            CAMERA_TRACE_FIXTURE,
            "--camera-sequence",
            "--camera-frame-indices",
            "2,0",
            "--camera-warmup-frames",
            "3",
            "--camera-measured-frames",
            "5",
            "--camera-loops",
            "2",
        ])
        .unwrap();
        let playback = load_camera_trace(&mut args).unwrap().unwrap();
        assert!(matches!(
            playback,
            CameraTracePlayback::Sequence {
                frame_indices,
                warmup_frames: 3,
                measured_frames: 5,
                loops: 2,
                ..
            } if frame_indices == vec![2, 0]
        ));
    }

    #[test]
    fn trace_sequence_rejects_fixed_only_and_duplicate_controls() {
        let mut args = parse_args(&[
            "--camera-trace",
            CAMERA_TRACE_FIXTURE,
            "--camera-sequence",
            "--camera-frame",
            "0",
        ])
        .unwrap();
        assert!(
            load_camera_trace(&mut args)
                .unwrap_err()
                .contains("cannot be combined")
        );

        let mut args = parse_args(&[
            "--camera-trace",
            CAMERA_TRACE_FIXTURE,
            "--camera-sequence",
            "--camera-frame-indices",
            "0,0",
        ])
        .unwrap();
        assert!(
            load_camera_trace(&mut args)
                .unwrap_err()
                .contains("duplicate")
        );
    }

    #[cfg(feature = "interactive-viewer")]
    #[test]
    fn surface_trace_schedule_preserves_warmup_measurement_and_loop_order() {
        let mut fixed_args = parse_args(&[
            "--interactive",
            "--camera-trace",
            CAMERA_TRACE_FIXTURE,
            "--camera-frame",
            "1",
            "--frames",
            "3",
        ])
        .unwrap();
        let fixed = load_camera_trace(&mut fixed_args).unwrap().unwrap();
        let fixed_steps = super::surface_trace_steps(&fixed).unwrap();
        assert_eq!(fixed_steps.len(), 3);
        assert!(fixed_steps.iter().all(|step| step.measured()));
        assert!(fixed_steps.iter().all(|step| step.trace_frame_index == 1));
        assert_eq!(
            fixed_steps
                .iter()
                .map(|step| step.playback_index)
                .collect::<Vec<_>>(),
            vec![0, 1, 2]
        );

        let mut fixed_warmup_args = parse_args(&[
            "--interactive",
            "--camera-trace",
            CAMERA_TRACE_FIXTURE,
            "--camera-frame",
            "1",
            "--camera-warmup-frames",
            "2",
            "--camera-measured-frames",
            "3",
        ])
        .unwrap();
        let fixed_warmup = load_camera_trace(&mut fixed_warmup_args).unwrap().unwrap();
        let fixed_warmup_steps = super::surface_trace_steps(&fixed_warmup).unwrap();
        assert_eq!(fixed_warmup_steps.len(), 5);
        assert_eq!(
            fixed_warmup_steps
                .iter()
                .map(|step| step.phase.as_str())
                .collect::<Vec<_>>(),
            vec!["warmup", "warmup", "measure", "measure", "measure"]
        );
        assert_eq!(
            fixed_warmup_steps
                .iter()
                .map(|step| step.measured_sample_index)
                .collect::<Vec<_>>(),
            vec![None, None, Some(0), Some(1), Some(2)]
        );
        assert!(
            fixed_warmup_steps
                .iter()
                .all(|step| step.trace_frame_index == 1)
        );

        let mut sequence_args = parse_args(&[
            "--interactive",
            "--camera-trace",
            CAMERA_TRACE_FIXTURE,
            "--camera-sequence",
            "--camera-frame-indices",
            "2,0",
            "--camera-warmup-frames",
            "2",
            "--camera-measured-frames",
            "3",
            "--camera-loops",
            "2",
        ])
        .unwrap();
        let sequence = load_camera_trace(&mut sequence_args).unwrap().unwrap();
        let steps = super::surface_trace_steps(&sequence).unwrap();
        assert_eq!(steps.len(), 8);
        assert_eq!(
            steps
                .iter()
                .map(|step| step.phase.as_str())
                .collect::<Vec<_>>(),
            vec![
                "warmup", "warmup", "measure", "measure", "measure", "measure", "measure",
                "measure"
            ]
        );
        assert_eq!(
            steps
                .iter()
                .map(|step| step.trace_frame_index)
                .collect::<Vec<_>>(),
            vec![2, 0, 2, 0, 2, 2, 0, 2]
        );
        assert_eq!(steps[2].loop_index, 0);
        assert_eq!(steps[5].loop_index, 1);
    }

    #[test]
    fn scene_bounds_returns_min_and_max_positions() {
        let scene = scene_with_positions(vec![
            Vec3f::new(-1.0, 2.0, 0.5),
            Vec3f::new(3.0, -4.0, 8.0),
            Vec3f::new(0.0, 1.0, -2.0),
        ]);

        let (min, max) = scene_bounds(&scene).unwrap();

        assert_eq!(min, Vec3f::new(-1.0, -4.0, -2.0));
        assert_eq!(max, Vec3f::new(3.0, 2.0, 8.0));
    }

    #[test]
    fn auto_camera_sets_valid_planes_for_deep_scene() {
        let scene = scene_with_positions(vec![
            Vec3f::new(-1.0, -1.0, 1.0),
            Vec3f::new(1.0, 1.0, 20.0),
        ]);
        let mut renderer = Renderer::new_for_surface(RenderMode::SortedAlpha).unwrap();
        renderer.load_scene(scene).unwrap();

        let camera = auto_camera(&renderer, RendererConfig::default());

        assert!(camera.intrinsics.near_plane > 0.0);
        assert!(camera.intrinsics.far_plane > camera.intrinsics.near_plane);
        assert_eq!(camera.validate(), Ok(()));
    }

    #[test]
    fn write_png_rejects_mismatched_rgba_len() {
        let err = write_png(std::path::Path::new("target/unused.png"), 2, 2, &[0; 4]).unwrap_err();

        assert_eq!(err, "png write failed: rgba buffer size mismatch");
    }
}
