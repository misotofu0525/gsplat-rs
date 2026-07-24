use std::fs;

use gsplat_core::Camera;
use gsplat_core::camera_trace::CameraTrace;

use crate::cli::{Args, SurfaceBenchmarkMode};

#[derive(Debug, Clone)]
pub(crate) enum CameraTracePlayback {
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
    pub(crate) fn trace(&self) -> &CameraTrace {
        match self {
            Self::Fixed { trace, .. } | Self::Sequence { trace, .. } => trace,
        }
    }

    pub(crate) fn initial_camera(&self) -> Result<Camera, String> {
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

    pub(crate) fn print_header(&self) {
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

pub(crate) fn load_camera_trace(args: &mut Args) -> Result<Option<CameraTracePlayback>, String> {
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
