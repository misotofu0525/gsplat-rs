//! Strict parser for the cross-platform `gsplat-camera-trace/v1` contract.
//!
//! The trace stores both a pose/intrinsics description and explicit matrices.
//! Rendering APIs consume [`Camera`], while validation keeps the f64 matrix
//! values as an independent oracle so every frontend starts from the same view.

use std::error::Error;
use std::fmt::{self, Display, Formatter};

use serde::Deserialize;

use crate::{Camera, CameraIntrinsics, CameraPose, Vec3f};

pub const CAMERA_TRACE_SCHEMA_V1: &str = "gsplat-camera-trace/v1";
const MATRIX_TOLERANCE: f64 = 1.0e-12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CameraTraceError(String);

impl CameraTraceError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl Display for CameraTraceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for CameraTraceError {}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct CameraTrace {
    pub schema: String,
    pub trace_id: String,
    pub content_sha256: String,
    coordinate_system: CoordinateSystem,
    matrix_convention: MatrixConvention,
    pub display: CameraTraceDisplay,
    pub frames: Vec<CameraTraceFrame>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
pub struct CameraTraceDisplay {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct CameraTraceFrame {
    pub frame_index: u32,
    pub timestamp_ns: u64,
    pub pose: CameraTracePose,
    pub intrinsics: CameraTraceIntrinsics,
    pub view_matrix: [f64; 16],
    pub projection_matrix: [f64; 16],
    pub view_projection_matrix: [f64; 16],
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
pub struct CameraTracePose {
    pub position: [f64; 3],
    pub rotation_xyzw: [f64; 4],
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
pub struct CameraTraceIntrinsics {
    pub vertical_fov_radians: f64,
    pub near_plane: f64,
    pub far_plane: f64,
}

/// Deterministic playback phases used by cross-platform camera-trace benchmarks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraTraceSequencePhase {
    Warmup,
    Measure,
}

impl CameraTraceSequencePhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Warmup => "warmup",
            Self::Measure => "measure",
        }
    }
}

/// One camera revision in a deterministic trace playback.
#[derive(Debug, Clone, Copy)]
pub struct CameraTraceSequenceStep<'a> {
    pub phase: CameraTraceSequencePhase,
    pub loop_index: usize,
    pub phase_frame_index: usize,
    pub measured_sample_index: Option<usize>,
    pub trace_frame_index: usize,
    pub frame: &'a CameraTraceFrame,
}

/// A validated playback schedule over selected trace frame indices.
///
/// Warmup is performed once. Measurement then contains `measured_frames` per
/// loop and restarts from the first selected trace frame for each loop. Within
/// either phase, selected indices are consumed in their declared order and
/// wrap as needed. The default schedule therefore visits every trace revision
/// exactly once: no warmup, one measured frame per trace frame, and one loop.
#[derive(Debug, Clone)]
pub struct CameraTraceSequence<'a> {
    trace: &'a CameraTrace,
    frame_indices: Vec<usize>,
    warmup_frames: usize,
    measured_frames: usize,
    loops: usize,
    total_frames: usize,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct CoordinateSystem {
    handedness: String,
    axes: String,
    camera_forward: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct MatrixConvention {
    storage_order: String,
    vector_convention: String,
    composition: String,
    ndc_xy: String,
    ndc_z: String,
    clip_w: String,
}

impl CameraTrace {
    pub fn from_json_slice(bytes: &[u8]) -> Result<Self, CameraTraceError> {
        let trace: Self = serde_json::from_slice(bytes).map_err(|error| {
            CameraTraceError::new(format!("invalid camera trace JSON: {error}"))
        })?;
        trace.validate()?;
        Ok(trace)
    }

    pub fn frame(&self, index: usize) -> Result<&CameraTraceFrame, CameraTraceError> {
        self.frames.get(index).ok_or_else(|| {
            CameraTraceError::new(format!(
                "camera frame {index} is out of range for {} frames",
                self.frames.len()
            ))
        })
    }

    pub fn default_sequence(&self) -> Result<CameraTraceSequence<'_>, CameraTraceError> {
        CameraTraceSequence::new(self, Vec::new(), 0, self.frames.len(), 1)
    }

    pub fn sequence(
        &self,
        frame_indices: Vec<usize>,
        warmup_frames: usize,
        measured_frames: usize,
        loops: usize,
    ) -> Result<CameraTraceSequence<'_>, CameraTraceError> {
        CameraTraceSequence::new(self, frame_indices, warmup_frames, measured_frames, loops)
    }

    fn validate(&self) -> Result<(), CameraTraceError> {
        if self.schema != CAMERA_TRACE_SCHEMA_V1 {
            return Err(CameraTraceError::new(format!(
                "schema must equal {CAMERA_TRACE_SCHEMA_V1}"
            )));
        }
        if self.trace_id.is_empty() {
            return Err(CameraTraceError::new("trace_id must be non-empty"));
        }
        // The canonical hash is generated by the repository's Python validator.
        // Re-serializing JSON floats in Rust is not byte-stable with CPython for
        // all halfway representations, so runtime consumers validate the receipt
        // shape and preserve it verbatim instead of inventing another hash.
        if self.content_sha256.len() != 64
            || !self
                .content_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(CameraTraceError::new(
                "content_sha256 must be 64 lowercase hexadecimal characters",
            ));
        }
        if self.coordinate_system
            != (CoordinateSystem {
                handedness: "right".to_owned(),
                axes: "RUF".to_owned(),
                camera_forward: "+Z".to_owned(),
            })
        {
            return Err(CameraTraceError::new(
                "coordinate_system does not match gsplat-camera-trace/v1",
            ));
        }
        if self.matrix_convention
            != (MatrixConvention {
                storage_order: "row-major".to_owned(),
                vector_convention: "column".to_owned(),
                composition: "projection * view * world_position".to_owned(),
                ndc_xy: "[-1,1]".to_owned(),
                ndc_z: "[0,1]".to_owned(),
                clip_w: "camera_z".to_owned(),
            })
        {
            return Err(CameraTraceError::new(
                "matrix_convention does not match gsplat-camera-trace/v1",
            ));
        }
        if self.display.width == 0 || self.display.height == 0 {
            return Err(CameraTraceError::new(
                "trace display dimensions must be positive",
            ));
        }
        if self.frames.is_empty() {
            return Err(CameraTraceError::new("trace frames must be non-empty"));
        }

        let aspect = f64::from(self.display.width) / f64::from(self.display.height);
        let mut previous_timestamp = None;
        for (expected_index, frame) in self.frames.iter().enumerate() {
            if frame.frame_index as usize != expected_index {
                return Err(CameraTraceError::new(format!(
                    "frames[{expected_index}].frame_index must equal {expected_index}"
                )));
            }
            if previous_timestamp.is_some_and(|previous| frame.timestamp_ns <= previous) {
                return Err(CameraTraceError::new(
                    "camera frame timestamps must be strictly increasing",
                ));
            }
            previous_timestamp = Some(frame.timestamp_ns);
            frame.validate(expected_index, aspect)?;
        }
        Ok(())
    }
}

impl<'a> CameraTraceSequence<'a> {
    fn new(
        trace: &'a CameraTrace,
        mut frame_indices: Vec<usize>,
        warmup_frames: usize,
        measured_frames: usize,
        loops: usize,
    ) -> Result<Self, CameraTraceError> {
        if frame_indices.is_empty() {
            frame_indices.extend(0..trace.frames.len());
        }
        if frame_indices.len() < 2 {
            return Err(CameraTraceError::new(
                "camera trace sequence requires at least two frame indices",
            ));
        }
        if measured_frames == 0 {
            return Err(CameraTraceError::new(
                "camera trace measured_frames must be positive",
            ));
        }
        if loops == 0 {
            return Err(CameraTraceError::new("camera trace loops must be positive"));
        }

        let mut seen = vec![false; trace.frames.len()];
        for &index in &frame_indices {
            if index >= trace.frames.len() {
                return Err(CameraTraceError::new(format!(
                    "camera frame {index} is out of range for {} frames",
                    trace.frames.len()
                )));
            }
            if seen[index] {
                return Err(CameraTraceError::new(format!(
                    "camera trace frame_indices contains duplicate {index}"
                )));
            }
            seen[index] = true;
        }

        let measured_total = measured_frames
            .checked_mul(loops)
            .ok_or_else(|| CameraTraceError::new("camera trace sequence length overflow"))?;
        let total_frames = warmup_frames
            .checked_add(measured_total)
            .ok_or_else(|| CameraTraceError::new("camera trace sequence length overflow"))?;

        Ok(Self {
            trace,
            frame_indices,
            warmup_frames,
            measured_frames,
            loops,
            total_frames,
        })
    }

    pub fn frame_indices(&self) -> &[usize] {
        &self.frame_indices
    }

    pub const fn warmup_frames(&self) -> usize {
        self.warmup_frames
    }

    pub const fn measured_frames(&self) -> usize {
        self.measured_frames
    }

    pub const fn loops(&self) -> usize {
        self.loops
    }

    pub const fn measured_sample_count(&self) -> usize {
        self.measured_frames * self.loops
    }

    pub const fn len(&self) -> usize {
        self.total_frames
    }

    pub const fn is_empty(&self) -> bool {
        self.total_frames == 0
    }

    pub fn step(&self, playback_index: usize) -> Option<CameraTraceSequenceStep<'a>> {
        if playback_index >= self.total_frames {
            return None;
        }
        let (phase, loop_index, phase_frame_index, measured_sample_index) =
            if playback_index < self.warmup_frames {
                (CameraTraceSequencePhase::Warmup, 0, playback_index, None)
            } else {
                let measured_sample_index = playback_index - self.warmup_frames;
                (
                    CameraTraceSequencePhase::Measure,
                    measured_sample_index / self.measured_frames,
                    measured_sample_index % self.measured_frames,
                    Some(measured_sample_index),
                )
            };
        let trace_frame_index = self.frame_indices[phase_frame_index % self.frame_indices.len()];
        Some(CameraTraceSequenceStep {
            phase,
            loop_index,
            phase_frame_index,
            measured_sample_index,
            trace_frame_index,
            frame: &self.trace.frames[trace_frame_index],
        })
    }

    pub fn steps(&self) -> impl ExactSizeIterator<Item = CameraTraceSequenceStep<'a>> + '_ {
        (0..self.total_frames).map(|index| {
            self.step(index)
                .expect("sequence iterator only yields in-range playback indices")
        })
    }
}

impl CameraTraceFrame {
    pub fn camera(&self) -> Result<Camera, CameraTraceError> {
        let camera = Camera {
            pose: CameraPose {
                position: Vec3f::new(
                    self.pose.position[0] as f32,
                    self.pose.position[1] as f32,
                    self.pose.position[2] as f32,
                ),
                rotation_xyzw: self.pose.rotation_xyzw.map(|value| value as f32),
            },
            intrinsics: CameraIntrinsics {
                vertical_fov_radians: self.intrinsics.vertical_fov_radians as f32,
                near_plane: self.intrinsics.near_plane as f32,
                far_plane: self.intrinsics.far_plane as f32,
            },
        };
        camera
            .validate()
            .map_err(|_| CameraTraceError::new("camera frame cannot be represented by Camera"))?;
        Ok(camera)
    }

    fn validate(&self, index: usize, aspect: f64) -> Result<(), CameraTraceError> {
        let values = self
            .pose
            .position
            .iter()
            .chain(self.pose.rotation_xyzw.iter())
            .chain([
                &self.intrinsics.vertical_fov_radians,
                &self.intrinsics.near_plane,
                &self.intrinsics.far_plane,
            ])
            .chain(self.view_matrix.iter())
            .chain(self.projection_matrix.iter())
            .chain(self.view_projection_matrix.iter());
        if !values.into_iter().all(|value| value.is_finite()) {
            return Err(CameraTraceError::new(format!(
                "frames[{index}] contains a non-finite number"
            )));
        }

        let norm2 = self
            .pose
            .rotation_xyzw
            .iter()
            .map(|value| value * value)
            .sum::<f64>();
        if (norm2 - 1.0).abs() > MATRIX_TOLERANCE {
            return Err(CameraTraceError::new(format!(
                "frames[{index}].pose.rotation_xyzw must be normalized"
            )));
        }
        let intrinsics = self.intrinsics;
        if intrinsics.vertical_fov_radians <= 0.0
            || intrinsics.vertical_fov_radians >= std::f64::consts::PI
            || intrinsics.near_plane <= 0.0
            || intrinsics.far_plane <= intrinsics.near_plane
        {
            return Err(CameraTraceError::new(format!(
                "frames[{index}].intrinsics are invalid"
            )));
        }

        let expected_view = view_matrix(self.pose.position, self.pose.rotation_xyzw);
        close_matrix(
            &self.view_matrix,
            &expected_view,
            &format!("frames[{index}].view_matrix"),
        )?;
        let expected_projection = projection_matrix(intrinsics, aspect);
        close_matrix(
            &self.projection_matrix,
            &expected_projection,
            &format!("frames[{index}].projection_matrix"),
        )?;
        let expected_view_projection = mat4_multiply(expected_projection, expected_view);
        close_matrix(
            &self.view_projection_matrix,
            &expected_view_projection,
            &format!("frames[{index}].view_projection_matrix"),
        )?;
        self.camera()?;
        Ok(())
    }
}

fn close_matrix(
    actual: &[f64; 16],
    expected: &[f64; 16],
    field: &str,
) -> Result<(), CameraTraceError> {
    for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
        if (actual - expected).abs() > MATRIX_TOLERANCE {
            return Err(CameraTraceError::new(format!(
                "{field}[{index}] mismatch: expected {expected}, got {actual}"
            )));
        }
    }
    Ok(())
}

fn view_matrix(position: [f64; 3], rotation_xyzw: [f64; 4]) -> [f64; 16] {
    let [mut x, mut y, mut z, w] = rotation_xyzw;
    x = -x;
    y = -y;
    z = -z;
    let rotation = [
        1.0 - 2.0 * (y * y + z * z),
        2.0 * (x * y - w * z),
        2.0 * (x * z + w * y),
        2.0 * (x * y + w * z),
        1.0 - 2.0 * (x * x + z * z),
        2.0 * (y * z - w * x),
        2.0 * (x * z - w * y),
        2.0 * (y * z + w * x),
        1.0 - 2.0 * (x * x + y * y),
    ];
    let tx = -(rotation[0] * position[0] + rotation[1] * position[1] + rotation[2] * position[2]);
    let ty = -(rotation[3] * position[0] + rotation[4] * position[1] + rotation[5] * position[2]);
    let tz = -(rotation[6] * position[0] + rotation[7] * position[1] + rotation[8] * position[2]);
    [
        rotation[0],
        rotation[1],
        rotation[2],
        tx,
        rotation[3],
        rotation[4],
        rotation[5],
        ty,
        rotation[6],
        rotation[7],
        rotation[8],
        tz,
        0.0,
        0.0,
        0.0,
        1.0,
    ]
}

fn projection_matrix(intrinsics: CameraTraceIntrinsics, aspect: f64) -> [f64; 16] {
    let f = 1.0 / (intrinsics.vertical_fov_radians * 0.5).tan();
    let depth = intrinsics.far_plane / (intrinsics.far_plane - intrinsics.near_plane);
    [
        f / aspect,
        0.0,
        0.0,
        0.0,
        0.0,
        f,
        0.0,
        0.0,
        0.0,
        0.0,
        depth,
        -intrinsics.near_plane * depth,
        0.0,
        0.0,
        1.0,
        0.0,
    ]
}

fn mat4_multiply(a: [f64; 16], b: [f64; 16]) -> [f64; 16] {
    std::array::from_fn(|index| {
        let row = index / 4;
        let column = index % 4;
        (0..4).map(|k| a[row * 4 + k] * b[k * 4 + column]).sum()
    })
}

#[cfg(test)]
mod tests {
    use super::{CameraTrace, CameraTraceSequencePhase};

    const FIXTURE: &[u8] =
        include_bytes!("../../../tests/perf/trace/fixtures/camera-trace-v1.json");

    #[test]
    fn parses_and_validates_contract_fixture() {
        let trace = CameraTrace::from_json_slice(FIXTURE).unwrap();
        let frame = trace.frame(2).unwrap();
        let camera = frame.camera().unwrap();

        assert_eq!(trace.trace_id, "contract-lateral-three-frame-v1");
        assert_eq!((trace.display.width, trace.display.height), (640, 360));
        assert_eq!(camera.pose.position, crate::Vec3f::new(0.5, 0.125, -3.0));
        assert_eq!(camera.pose.rotation_xyzw, [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn rejects_invalid_hash_receipt_and_out_of_range_frame() {
        let modified = String::from_utf8(FIXTURE.to_vec()).unwrap().replace(
            "e73f23c44f0cc1fb3fc2e533bcbce70989afe5f0b739e38601198f271484bca6",
            "not-a-sha256",
        );
        let error = CameraTrace::from_json_slice(modified.as_bytes()).unwrap_err();
        assert!(error.to_string().contains("64 lowercase hexadecimal"));

        let trace = CameraTrace::from_json_slice(FIXTURE).unwrap();
        assert!(
            trace
                .frame(3)
                .unwrap_err()
                .to_string()
                .contains("out of range")
        );
    }

    #[test]
    fn default_sequence_visits_each_revision_once_in_order() {
        let trace = CameraTrace::from_json_slice(FIXTURE).unwrap();
        let sequence = trace.default_sequence().unwrap();
        let receipts = sequence
            .steps()
            .map(|step| {
                (
                    step.phase,
                    step.loop_index,
                    step.trace_frame_index,
                    step.frame.timestamp_ns,
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(sequence.frame_indices(), &[0, 1, 2]);
        assert_eq!(sequence.warmup_frames(), 0);
        assert_eq!(sequence.measured_frames(), 3);
        assert_eq!(sequence.loops(), 1);
        assert_eq!(sequence.measured_sample_count(), 3);
        assert_eq!(
            receipts,
            vec![
                (CameraTraceSequencePhase::Measure, 0, 0, 0),
                (CameraTraceSequencePhase::Measure, 0, 1, 16_666_667),
                (CameraTraceSequencePhase::Measure, 0, 2, 33_333_334),
            ]
        );
    }

    #[test]
    fn sequence_restarts_selected_order_for_measurement_and_each_loop() {
        let trace = CameraTrace::from_json_slice(FIXTURE).unwrap();
        let sequence = trace.sequence(vec![2, 0], 3, 3, 2).unwrap();
        let receipts = sequence
            .steps()
            .map(|step| {
                (
                    step.phase.as_str(),
                    step.loop_index,
                    step.phase_frame_index,
                    step.measured_sample_index,
                    step.trace_frame_index,
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            receipts,
            vec![
                ("warmup", 0, 0, None, 2),
                ("warmup", 0, 1, None, 0),
                ("warmup", 0, 2, None, 2),
                ("measure", 0, 0, Some(0), 2),
                ("measure", 0, 1, Some(1), 0),
                ("measure", 0, 2, Some(2), 2),
                ("measure", 1, 0, Some(3), 2),
                ("measure", 1, 1, Some(4), 0),
                ("measure", 1, 2, Some(5), 2),
            ]
        );
    }

    #[test]
    fn sequence_rejects_bad_lengths_indices_and_duplicates() {
        let trace = CameraTrace::from_json_slice(FIXTURE).unwrap();
        assert!(trace.sequence(vec![0, 1], 0, 0, 1).is_err());
        assert!(trace.sequence(vec![0, 1], 0, 1, 0).is_err());
        assert!(trace.sequence(vec![0], 0, 1, 1).is_err());
        assert!(trace.sequence(vec![0, 3], 0, 1, 1).is_err());
        assert!(trace.sequence(vec![1, 1], 0, 1, 1).is_err());
    }
}
