use std::fs;
use std::path::PathBuf;

use gsplat_core::camera_trace::{CameraTrace, CameraTraceSequencePhase};
use gsplat_core::{Camera, RendererConfig};

#[derive(Debug, Clone)]
pub struct TraceRequest {
    pub path: Option<PathBuf>,
    pub sequence: bool,
    pub frame_index: usize,
    pub frame_index_explicit: bool,
    pub frame_indices: Option<Vec<usize>>,
    pub warmup_frames: Option<usize>,
    pub measured_frames: Option<usize>,
    pub loops: usize,
    pub default_warmup_frames: usize,
    pub default_measured_frames: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackPhase {
    Warmup,
    Measure,
}

#[derive(Debug, Clone)]
pub struct PlaybackStep {
    pub phase: PlaybackPhase,
    pub camera: Camera,
    pub trace_frame_index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TraceSelection {
    DefaultCamera,
    Fixed { frame_index: usize },
    Sequence { frame_indices: Vec<usize> },
}

#[derive(Debug, Clone)]
pub struct Playback {
    trace: Option<CameraTrace>,
    selection: TraceSelection,
    steps: Vec<PlaybackStep>,
    warmup_count: usize,
    measured_count: usize,
}

impl Playback {
    pub fn build(request: TraceRequest) -> Result<Self, String> {
        let warmup_frames = request
            .warmup_frames
            .unwrap_or(request.default_warmup_frames);
        let measured_frames = request
            .measured_frames
            .unwrap_or(request.default_measured_frames);
        if measured_frames == 0 {
            return Err("camera measured frame count must be positive".to_owned());
        }
        if request.loops == 0 {
            return Err("--camera-loops must be positive".to_owned());
        }

        let Some(path) = request.path.as_deref() else {
            if request.sequence
                || request.frame_index_explicit
                || request.frame_indices.is_some()
                || request.warmup_frames.is_some()
                || request.measured_frames.is_some()
                || request.loops != 1
            {
                return Err("camera trace playback flags require --camera-trace".to_owned());
            }
            let mut steps = Vec::with_capacity(warmup_frames + measured_frames);
            for index in 0..warmup_frames + measured_frames {
                steps.push(PlaybackStep {
                    phase: if index < warmup_frames {
                        PlaybackPhase::Warmup
                    } else {
                        PlaybackPhase::Measure
                    },
                    camera: Camera::default(),
                    trace_frame_index: None,
                });
            }
            return Ok(Self {
                trace: None,
                selection: TraceSelection::DefaultCamera,
                steps,
                warmup_count: warmup_frames,
                measured_count: measured_frames,
            });
        };

        let bytes = fs::read(path)
            .map_err(|error| format!("cannot read camera trace {}: {error}", path.display()))?;
        let trace = CameraTrace::from_json_slice(&bytes)
            .map_err(|error| format!("invalid camera trace {}: {error}", path.display()))?;

        if request.sequence {
            if request.frame_index_explicit {
                return Err("--camera-frame cannot be combined with --camera-sequence".to_owned());
            }
            let frame_indices = request
                .frame_indices
                .clone()
                .unwrap_or_else(|| (0..trace.frames.len()).collect());
            let sequence = trace
                .sequence(
                    frame_indices.clone(),
                    warmup_frames,
                    measured_frames,
                    request.loops,
                )
                .map_err(|error| error.to_string())?;
            let mut steps = Vec::with_capacity(sequence.len());
            for step in sequence.steps() {
                steps.push(PlaybackStep {
                    phase: match step.phase {
                        CameraTraceSequencePhase::Warmup => PlaybackPhase::Warmup,
                        CameraTraceSequencePhase::Measure => PlaybackPhase::Measure,
                    },
                    camera: step.frame.camera().map_err(|error| error.to_string())?,
                    trace_frame_index: Some(step.trace_frame_index),
                });
            }
            let measured_count = sequence.measured_sample_count();
            return Ok(Self {
                trace: Some(trace),
                selection: TraceSelection::Sequence { frame_indices },
                steps,
                warmup_count: warmup_frames,
                measured_count,
            });
        }

        if request.frame_indices.is_some() || request.loops != 1 {
            return Err("sequence options require --camera-sequence".to_owned());
        }
        let camera = trace
            .frame(request.frame_index)
            .map_err(|error| error.to_string())?
            .camera()
            .map_err(|error| error.to_string())?;
        let mut steps = Vec::with_capacity(warmup_frames + measured_frames);
        for index in 0..warmup_frames + measured_frames {
            steps.push(PlaybackStep {
                phase: if index < warmup_frames {
                    PlaybackPhase::Warmup
                } else {
                    PlaybackPhase::Measure
                },
                camera,
                trace_frame_index: Some(request.frame_index),
            });
        }
        Ok(Self {
            trace: Some(trace),
            selection: TraceSelection::Fixed {
                frame_index: request.frame_index,
            },
            steps,
            warmup_count: warmup_frames,
            measured_count: measured_frames,
        })
    }

    pub fn renderer_config(&self) -> RendererConfig {
        match &self.trace {
            Some(trace) => RendererConfig {
                width: trace.display.width,
                height: trace.display.height,
                ..RendererConfig::default()
            },
            None => RendererConfig::default(),
        }
    }

    pub fn trace(&self) -> Option<&CameraTrace> {
        self.trace.as_ref()
    }

    pub fn selection(&self) -> &TraceSelection {
        &self.selection
    }

    pub fn steps(&self) -> &[PlaybackStep] {
        &self.steps
    }

    pub const fn warmup_count(&self) -> usize {
        self.warmup_count
    }

    pub const fn measured_count(&self) -> usize {
        self.measured_count
    }
}

pub fn parse_frame_indices(value: &str) -> Result<Vec<usize>, String> {
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{Playback, PlaybackPhase, TraceRequest, TraceSelection, parse_frame_indices};

    fn request() -> TraceRequest {
        TraceRequest {
            path: None,
            sequence: false,
            frame_index: 0,
            frame_index_explicit: false,
            frame_indices: None,
            warmup_frames: None,
            measured_frames: None,
            loops: 1,
            default_warmup_frames: 2,
            default_measured_frames: 3,
        }
    }

    #[test]
    fn default_camera_schedule_preserves_legacy_counts() {
        let playback = Playback::build(request()).expect("default playback");
        assert_eq!(playback.selection(), &TraceSelection::DefaultCamera);
        assert_eq!(playback.warmup_count(), 2);
        assert_eq!(playback.measured_count(), 3);
        assert_eq!(playback.steps().len(), 5);
        assert_eq!(playback.steps()[1].phase, PlaybackPhase::Warmup);
        assert_eq!(playback.steps()[2].phase, PlaybackPhase::Measure);
    }

    #[test]
    fn frozen_trace_sequence_uses_declared_order_and_dimensions() {
        let mut request = request();
        request.path = Some(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(
                    "tests/perf/trace/fixtures/quality/\
                     candidate-kitsune-quality-1920x1080-v1.json",
                ),
        );
        request.sequence = true;
        request.frame_indices = Some(vec![0, 1]);
        request.warmup_frames = Some(2);
        request.measured_frames = Some(4);

        let playback = Playback::build(request).expect("sequence playback");
        assert_eq!(playback.renderer_config().width, 1920);
        assert_eq!(playback.renderer_config().height, 1080);
        assert_eq!(playback.warmup_count(), 2);
        assert_eq!(playback.measured_count(), 4);
        assert_eq!(
            playback
                .steps()
                .iter()
                .map(|step| step.trace_frame_index)
                .collect::<Vec<_>>(),
            vec![Some(0), Some(1), Some(0), Some(1), Some(0), Some(1)]
        );
    }

    #[test]
    fn trace_flags_fail_closed_without_trace() {
        let mut value = request();
        value.sequence = true;
        assert!(
            Playback::build(value)
                .unwrap_err()
                .contains("require --camera-trace")
        );
    }

    #[test]
    fn frame_index_parser_rejects_empty_members() {
        assert_eq!(parse_frame_indices("0, 2").unwrap(), vec![0, 2]);
        assert!(parse_frame_indices("0,,2").is_err());
        assert!(parse_frame_indices("").is_err());
    }
}
