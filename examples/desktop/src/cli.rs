use std::path::PathBuf;

use gsplat_core::{RenderMode, RendererConfig};
use gsplat_render_wgpu::{GeometryPath, SurfaceGpuOrderProducer, SurfaceOrderBackend};

#[cfg(all(
    target_arch = "wasm32",
    any(
        feature = "diagnostic-surface-capture-receipt",
        feature = "diagnostic-surface-depth-key-candidate24",
        feature = "diagnostic-surface-projected-axes16",
        feature = "diagnostic-resident-sh-mantissa8"
    )
))]
compile_error!("desktop diagnostic Surface capture receipts are native-only");

#[cfg(all(feature = "qualification-q1-m4-native", target_arch = "wasm32"))]
compile_error!("Q1 M4 native sustained evidence is native-only");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SurfaceBenchmarkMode {
    #[default]
    Isolated,
    Throughput,
}

impl SurfaceBenchmarkMode {
    #[cfg(feature = "interactive-viewer")]
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Isolated => "isolated",
            Self::Throughput => "throughput",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SurfaceSortPolicyArg {
    #[default]
    EveryFrame,
    CameraChange,
}

impl SurfaceSortPolicyArg {
    #[cfg(feature = "interactive-viewer")]
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::EveryFrame => "every_frame",
            Self::CameraChange => "camera_change",
        }
    }
}

/// Closed Exact plan request used only by the real-window evidence harness.
/// The renderer remains the policy owner; this value selects one existing
/// complete policy tuple before trace playback begins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SurfaceEvidencePlanArg {
    CpuPostSort,
    GpuPostSort,
    GpuPreproject,
    Adaptive,
}

impl SurfaceEvidencePlanArg {
    pub(crate) const fn order_backend(self) -> SurfaceOrderBackend {
        match self {
            Self::CpuPostSort => SurfaceOrderBackend::Cpu,
            Self::GpuPostSort | Self::GpuPreproject => SurfaceOrderBackend::Gpu,
            Self::Adaptive => SurfaceOrderBackend::Adaptive,
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::CpuPostSort => "cpu_post_sort",
            Self::GpuPostSort => "gpu_post_sort",
            Self::GpuPreproject => "gpu_preproject",
            Self::Adaptive => "adaptive",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Args {
    pub(crate) dataset_path: PathBuf,
    pub(crate) config: RendererConfig,
    pub(crate) frames: u32,
    pub(crate) orbit: bool,
    pub(crate) auto_camera: bool,
    pub(crate) yaw_deg: Option<f32>,
    pub(crate) interactive: bool,
    pub(crate) geometry_path: GeometryPath,
    pub(crate) order_backend: SurfaceOrderBackend,
    pub(crate) surface_benchmark_mode: SurfaceBenchmarkMode,
    #[cfg_attr(not(feature = "interactive-viewer"), allow(dead_code))]
    pub(crate) surface_sort_policy: SurfaceSortPolicyArg,
    /// `None` preserves the ordinary product defaults. `Some` is the explicit
    /// producer A/B mode and also enables its independent terminal receipts.
    #[cfg_attr(not(feature = "interactive-viewer"), allow(dead_code))]
    pub(crate) surface_gpu_producer: Option<SurfaceGpuOrderProducer>,
    /// Enables the strict M2b real-window current-stats/capture path.
    #[cfg_attr(not(feature = "interactive-viewer"), allow(dead_code))]
    pub(crate) surface_evidence_plan: Option<SurfaceEvidencePlanArg>,
    /// Enables the private Q1 M4 control plus terminal-throughput host.
    #[cfg_attr(
        not(all(feature = "qualification-q1-m4-native", not(target_arch = "wasm32"))),
        allow(dead_code)
    )]
    pub(crate) surface_q1_m4_native: bool,
    /// Takes the atomic diagnostic capture/receipt pair in the strict native
    /// Surface evidence host. The package feature and CLI flag are both
    /// required so ordinary evidence runs retain their existing behavior.
    #[cfg_attr(
        not(all(
            feature = "diagnostic-surface-capture-receipt",
            not(target_arch = "wasm32")
        )),
        allow(dead_code)
    )]
    pub(crate) surface_diagnostic_capture_receipt: bool,
    /// Retains the diagnostic-only post-warmup trace-frame schedule [0, 1, 0]
    /// in one strict Surface evidence session.
    #[cfg_attr(
        not(all(
            feature = "diagnostic-surface-capture-receipt",
            not(target_arch = "wasm32")
        )),
        allow(dead_code)
    )]
    pub(crate) surface_diagnostic_multi_capture: bool,
    pub(crate) png_out: Option<PathBuf>,
    pub(crate) camera_trace_path: Option<PathBuf>,
    pub(crate) camera_frame: usize,
    pub(crate) camera_sequence: bool,
    pub(crate) camera_frame_indices: Option<Vec<usize>>,
    pub(crate) camera_warmup_frames: usize,
    pub(crate) camera_measured_frames: Option<usize>,
    pub(crate) camera_loops: usize,
    pub(crate) camera_frame_explicit: bool,
    pub(crate) frames_explicit: bool,
    pub(crate) width_explicit: bool,
    pub(crate) height_explicit: bool,
}

impl Args {
    pub(crate) fn parse(mut args: impl Iterator<Item = String>) -> Result<Self, String> {
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
        let mut surface_gpu_producer = None;
        let mut surface_evidence_plan = None;
        #[cfg(all(feature = "qualification-q1-m4-native", not(target_arch = "wasm32")))]
        let mut surface_q1_m4_native = false;
        #[cfg(not(all(feature = "qualification-q1-m4-native", not(target_arch = "wasm32"))))]
        let surface_q1_m4_native = false;
        #[cfg(all(
            feature = "diagnostic-surface-capture-receipt",
            not(target_arch = "wasm32")
        ))]
        let mut surface_diagnostic_capture_receipt = false;
        #[cfg(all(
            feature = "diagnostic-surface-capture-receipt",
            not(target_arch = "wasm32")
        ))]
        let mut surface_diagnostic_multi_capture = false;
        #[cfg(not(all(
            feature = "diagnostic-surface-capture-receipt",
            not(target_arch = "wasm32")
        )))]
        let surface_diagnostic_capture_receipt = false;
        #[cfg(not(all(
            feature = "diagnostic-surface-capture-receipt",
            not(target_arch = "wasm32")
        )))]
        let surface_diagnostic_multi_capture = false;
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
                "--surface-evidence-plan" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --surface-evidence-plan".to_owned())?;
                    surface_evidence_plan = Some(parse_surface_evidence_plan(&value)?);
                }
                #[cfg(all(feature = "qualification-q1-m4-native", not(target_arch = "wasm32")))]
                "--surface-q1-m4-native" => {
                    surface_q1_m4_native = true;
                }
                #[cfg(all(
                    feature = "diagnostic-surface-capture-receipt",
                    not(target_arch = "wasm32")
                ))]
                "--surface-diagnostic-capture-receipt" => {
                    surface_diagnostic_capture_receipt = true;
                }
                #[cfg(all(
                    feature = "diagnostic-surface-capture-receipt",
                    not(target_arch = "wasm32")
                ))]
                "--surface-diagnostic-multi-capture" => {
                    surface_diagnostic_multi_capture = true;
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
        if surface_sort_policy_explicit && !interactive {
            return Err("--surface-sort-policy requires --interactive".to_owned());
        }
        if surface_gpu_producer.is_some() && !interactive {
            return Err("--surface-gpu-producer requires --interactive".to_owned());
        }
        if surface_gpu_producer.is_some() && geometry_path != GeometryPath::PackedAtlas {
            return Err("--surface-gpu-producer requires --geometry-path packed".to_owned());
        }
        if surface_gpu_producer.is_some() && order_backend == SurfaceOrderBackend::Cpu {
            return Err("--surface-gpu-producer requires --order-backend gpu|adaptive".to_owned());
        }
        if let Some(plan) = surface_evidence_plan {
            if !interactive {
                return Err("--surface-evidence-plan requires --interactive".to_owned());
            }
            if geometry_path != GeometryPath::PackedAtlas {
                return Err("--surface-evidence-plan requires --geometry-path packed".to_owned());
            }
            if camera_trace_path.is_none() {
                return Err("--surface-evidence-plan requires --camera-trace".to_owned());
            }
            if png_out.is_none() {
                return Err("--surface-evidence-plan requires --png".to_owned());
            }
            if surface_benchmark_mode != SurfaceBenchmarkMode::Isolated {
                return Err(
                    "--surface-evidence-plan requires --surface-benchmark-mode isolated".to_owned(),
                );
            }
            if surface_sort_policy != SurfaceSortPolicyArg::EveryFrame {
                return Err(
                    "--surface-evidence-plan requires --surface-sort-policy every-frame".to_owned(),
                );
            }
            if surface_gpu_producer.is_some() {
                return Err(
                    "--surface-evidence-plan cannot be combined with --surface-gpu-producer"
                        .to_owned(),
                );
            }
            if order_backend_explicit && order_backend != plan.order_backend() {
                return Err(format!(
                    "--surface-evidence-plan {} conflicts with --order-backend",
                    plan.label()
                ));
            }
            order_backend = plan.order_backend();
        }
        if surface_q1_m4_native {
            if !interactive {
                return Err("--surface-q1-m4-native requires --interactive".to_owned());
            }
            if geometry_path != GeometryPath::PackedAtlas {
                return Err("--surface-q1-m4-native requires --geometry-path packed".to_owned());
            }
            if camera_trace_path.is_none() || !camera_sequence {
                return Err(
                    "--surface-q1-m4-native requires --camera-trace and --camera-sequence"
                        .to_owned(),
                );
            }
            if camera_frame_indices.as_deref() != Some(&[0, 1]) {
                return Err("--surface-q1-m4-native requires --camera-frame-indices 0,1".to_owned());
            }
            if camera_warmup_frames != 20 || camera_measured_frames != Some(80) || camera_loops != 1
            {
                return Err(
                    "--surface-q1-m4-native requires exactly 20 warmup, 80 measured, and one loop"
                        .to_owned(),
                );
            }
            if surface_benchmark_mode != SurfaceBenchmarkMode::Throughput {
                return Err(
                    "--surface-q1-m4-native requires --surface-benchmark-mode throughput"
                        .to_owned(),
                );
            }
            if surface_sort_policy != SurfaceSortPolicyArg::EveryFrame {
                return Err(
                    "--surface-q1-m4-native requires --surface-sort-policy every-frame".to_owned(),
                );
            }
            if order_backend != SurfaceOrderBackend::Adaptive {
                return Err("--surface-q1-m4-native requires --order-backend adaptive".to_owned());
            }
            if png_out.is_none() {
                return Err("--surface-q1-m4-native requires --png".to_owned());
            }
            if surface_evidence_plan.is_some()
                || surface_gpu_producer.is_some()
                || surface_diagnostic_capture_receipt
                || surface_diagnostic_multi_capture
            {
                return Err(
                    "--surface-q1-m4-native cannot be combined with other private Surface evidence modes"
                        .to_owned(),
                );
            }
        }
        if surface_diagnostic_capture_receipt && surface_evidence_plan.is_none() {
            return Err(
                "--surface-diagnostic-capture-receipt requires --surface-evidence-plan".to_owned(),
            );
        }
        if surface_diagnostic_multi_capture && !surface_diagnostic_capture_receipt {
            return Err(
                "--surface-diagnostic-multi-capture requires --surface-diagnostic-capture-receipt"
                    .to_owned(),
            );
        }
        #[cfg(feature = "diagnostic-surface-depth-key-candidate24")]
        if !surface_diagnostic_capture_receipt {
            return Err(
                "Candidate24 desktop execution requires --surface-diagnostic-capture-receipt"
                    .to_owned(),
            );
        }
        if interactive && png_out.is_some() && camera_trace_path.is_none() {
            return Err(
                "interactive --png requires --camera-trace Surface benchmark mode".to_owned(),
            );
        }
        if interactive
            && png_out.is_some()
            && surface_gpu_producer.is_none()
            && surface_evidence_plan.is_none()
            && !surface_q1_m4_native
        {
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
            surface_gpu_producer,
            surface_evidence_plan,
            surface_q1_m4_native,
            surface_diagnostic_capture_receipt,
            surface_diagnostic_multi_capture,
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
    let lines = vec![
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
        "  --surface-gpu-producer P run an exact post-sort|preproject GPU-producer A/B",
        "  --surface-evidence-plan P collect strict cpu-post-sort|gpu-post-sort|gpu-preproject|adaptive evidence",
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
    #[cfg(all(feature = "qualification-q1-m4-native", not(target_arch = "wasm32")))]
    let lines = {
        let mut lines = lines;
        lines.push(
            "  --surface-q1-m4-native collect private Q1 M4 control and terminal-throughput evidence",
        );
        lines
    };
    #[cfg(all(
        feature = "diagnostic-surface-capture-receipt",
        not(target_arch = "wasm32")
    ))]
    let lines = {
        let mut lines = lines;
        lines.push(
            "  --surface-diagnostic-capture-receipt take the atomic native diagnostic capture receipt",
        );
        lines.push(
            "  --surface-diagnostic-multi-capture retain post-warmup diagnostic trace frames [0,1,0]",
        );
        lines
    };
    lines.join("\n")
}

pub(crate) fn validate_surface_trace_geometry(args: &Args, has_trace: bool) -> Result<(), String> {
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

fn parse_surface_evidence_plan(value: &str) -> Result<SurfaceEvidencePlanArg, String> {
    match value {
        "cpu-post-sort" => Ok(SurfaceEvidencePlanArg::CpuPostSort),
        "gpu-post-sort" => Ok(SurfaceEvidencePlanArg::GpuPostSort),
        "gpu-preproject" => Ok(SurfaceEvidencePlanArg::GpuPreproject),
        "adaptive" => Ok(SurfaceEvidencePlanArg::Adaptive),
        _ => Err(format!(
            "invalid --surface-evidence-plan '{value}' (expected cpu-post-sort|gpu-post-sort|gpu-preproject|adaptive)"
        )),
    }
}

pub(crate) const fn geometry_path_label(path: GeometryPath) -> &'static str {
    match path {
        GeometryPath::SortedIndexDirect => "sorted_index_direct",
        GeometryPath::PackedAtlas => "packed_atlas",
        GeometryPath::PagedActiveAtlas => "paged_active_atlas",
    }
}
