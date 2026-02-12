//! PLY scene loading and parsing utilities.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use gsplat_core::{ErrorCode, SceneBuffers, Vec3f};
use thiserror::Error;

const REQUIRED_VERTEX_FIELDS: [&str; 14] = [
    "x", "y", "z", "opacity", "scale_0", "scale_1", "scale_2", "rot_0", "rot_1", "rot_2", "rot_3",
    "f_dc_0", "f_dc_1", "f_dc_2",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlySceneSummary {
    pub gaussians: usize,
    pub has_sh_rest: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlyLoadResult {
    pub scene: SceneBuffers,
    pub summary: PlySceneSummary,
}

#[derive(Debug, Error, PartialEq)]
pub enum PlyLoadError {
    #[error("I/O error while reading PLY")]
    Io,
    #[error("unsupported PLY format; only ascii 1.0 is currently supported")]
    UnsupportedFormat,
    #[error("malformed PLY header")]
    MalformedHeader,
    #[error("missing required field `{0}`")]
    MissingField(&'static str),
    #[error("vertex row does not match declared property count")]
    VertexFieldCount,
    #[error("failed to parse numeric value")]
    ParseNumber,
    #[error("vertex rows do not match declared vertex count")]
    VertexCountMismatch,
    #[error("parsed scene buffers are inconsistent")]
    InvalidScene,
}

impl PlyLoadError {
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Io => ErrorCode::NotFound,
            Self::UnsupportedFormat => ErrorCode::Unsupported,
            Self::MalformedHeader
            | Self::MissingField(_)
            | Self::VertexFieldCount
            | Self::ParseNumber
            | Self::VertexCountMismatch
            | Self::InvalidScene => ErrorCode::ParseFailed,
        }
    }
}

pub fn load_ply(path: &Path) -> Result<PlyLoadResult, PlyLoadError> {
    let raw = fs::read(path).map_err(|_| PlyLoadError::Io)?;
    let text = std::str::from_utf8(&raw).map_err(|_| PlyLoadError::UnsupportedFormat)?;
    parse_ply_text(text)
}

pub fn load_ply_summary(path: &Path) -> Result<PlySceneSummary, PlyLoadError> {
    Ok(load_ply(path)?.summary)
}

pub fn parse_ply_text(input: &str) -> Result<PlyLoadResult, PlyLoadError> {
    let mut lines = input.lines();

    if lines.next().map(str::trim) != Some("ply") {
        return Err(PlyLoadError::MalformedHeader);
    }

    let mut format_ascii = false;
    let mut saw_end_header = false;
    let mut vertex_count: Option<usize> = None;
    let mut in_vertex_element = false;
    let mut vertex_properties: Vec<String> = Vec::new();

    for line in lines.by_ref() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("comment") {
            continue;
        }

        if trimmed == "end_header" {
            saw_end_header = true;
            break;
        }

        let mut parts = trimmed.split_whitespace();
        match parts.next() {
            Some("format") => {
                format_ascii = matches!((parts.next(), parts.next()), (Some("ascii"), Some("1.0")));
            }
            Some("element") => {
                let element = parts.next();
                let count = parts.next();

                in_vertex_element = element == Some("vertex");
                if in_vertex_element {
                    vertex_count = count
                        .and_then(|s| s.parse::<usize>().ok())
                        .ok_or(PlyLoadError::MalformedHeader)
                        .map(Some)?;
                }
            }
            Some("property") if in_vertex_element => {
                let _ty = parts.next();
                let name = parts.next().ok_or(PlyLoadError::MalformedHeader)?;
                vertex_properties.push(name.to_owned());
            }
            _ => {}
        }
    }

    if !format_ascii {
        return Err(PlyLoadError::UnsupportedFormat);
    }

    if !saw_end_header {
        return Err(PlyLoadError::MalformedHeader);
    }

    let vertex_count = vertex_count.ok_or(PlyLoadError::MalformedHeader)?;

    let mut indices: HashMap<&str, usize> = HashMap::new();
    for (idx, field_name) in vertex_properties.iter().enumerate() {
        indices.insert(field_name.as_str(), idx);
    }

    for required in REQUIRED_VERTEX_FIELDS {
        if !indices.contains_key(required) {
            return Err(PlyLoadError::MissingField(required));
        }
    }

    let sh_rest_indices = match (
        indices.get("f_rest_0").copied(),
        indices.get("f_rest_1").copied(),
        indices.get("f_rest_2").copied(),
    ) {
        (Some(a), Some(b), Some(c)) => Some([a, b, c]),
        _ => None,
    };

    // Only claim SH-rest is present when we actually load any SH-rest data.
    let has_sh_rest = sh_rest_indices.is_some();

    let mut scene = SceneBuffers {
        positions: Vec::with_capacity(vertex_count),
        opacity: Vec::with_capacity(vertex_count),
        scale_xyz: Vec::with_capacity(vertex_count),
        rotation_xyzw: Vec::with_capacity(vertex_count),
        color_dc: Vec::with_capacity(vertex_count),
        sh_rest_rgb: sh_rest_indices.map(|_| Vec::with_capacity(vertex_count)),
    };

    for _ in 0..vertex_count {
        let line = lines.next().ok_or(PlyLoadError::VertexCountMismatch)?;
        let values: Vec<&str> = line.split_whitespace().collect();

        if values.len() < vertex_properties.len() {
            return Err(PlyLoadError::VertexFieldCount);
        }

        let x = parse_field_f32(&values, &indices, "x")?;
        let y = parse_field_f32(&values, &indices, "y")?;
        let z = parse_field_f32(&values, &indices, "z")?;
        scene.positions.push(Vec3f::new(x, y, z));

        scene
            .opacity
            .push(parse_field_f32(&values, &indices, "opacity")?);

        scene.scale_xyz.push([
            parse_field_f32(&values, &indices, "scale_0")?,
            parse_field_f32(&values, &indices, "scale_1")?,
            parse_field_f32(&values, &indices, "scale_2")?,
        ]);

        scene.rotation_xyzw.push([
            parse_field_f32(&values, &indices, "rot_0")?,
            parse_field_f32(&values, &indices, "rot_1")?,
            parse_field_f32(&values, &indices, "rot_2")?,
            parse_field_f32(&values, &indices, "rot_3")?,
        ]);

        scene.color_dc.push([
            parse_field_f32(&values, &indices, "f_dc_0")?,
            parse_field_f32(&values, &indices, "f_dc_1")?,
            parse_field_f32(&values, &indices, "f_dc_2")?,
        ]);

        if let Some(sh_indices) = sh_rest_indices {
            scene.sh_rest_rgb.as_mut().expect("allocated").push([
                parse_field_f32_idx(&values, sh_indices[0])?,
                parse_field_f32_idx(&values, sh_indices[1])?,
                parse_field_f32_idx(&values, sh_indices[2])?,
            ]);
        }
    }

    scene.validate().map_err(|_| PlyLoadError::InvalidScene)?;

    Ok(PlyLoadResult {
        summary: PlySceneSummary {
            gaussians: scene.len(),
            has_sh_rest,
        },
        scene,
    })
}

fn parse_field_f32(
    values: &[&str],
    indices: &HashMap<&str, usize>,
    field: &str,
) -> Result<f32, PlyLoadError> {
    let idx = indices
        .get(field)
        .copied()
        .ok_or(PlyLoadError::MalformedHeader)?;
    parse_field_f32_idx(values, idx)
}

fn parse_field_f32_idx(values: &[&str], idx: usize) -> Result<f32, PlyLoadError> {
    let value = values.get(idx).ok_or(PlyLoadError::VertexFieldCount)?;
    value.parse::<f32>().map_err(|_| PlyLoadError::ParseNumber)
}

#[cfg(test)]
mod tests {
    use super::{PlyLoadError, parse_ply_text};

    const VALID_PLY: &str = "ply\nformat ascii 1.0\nelement vertex 1\nproperty float x\nproperty float y\nproperty float z\nproperty float opacity\nproperty float scale_0\nproperty float scale_1\nproperty float scale_2\nproperty float rot_0\nproperty float rot_1\nproperty float rot_2\nproperty float rot_3\nproperty float f_dc_0\nproperty float f_dc_1\nproperty float f_dc_2\nend_header\n0.0 0.1 1.0 0.9 1.0 1.1 1.2 0.0 0.0 0.0 1.0 0.2 0.3 0.4\n";
    const PLY_INCOMPLETE_SH_REST: &str = "ply\nformat ascii 1.0\nelement vertex 1\nproperty float x\nproperty float y\nproperty float z\nproperty float opacity\nproperty float scale_0\nproperty float scale_1\nproperty float scale_2\nproperty float rot_0\nproperty float rot_1\nproperty float rot_2\nproperty float rot_3\nproperty float f_dc_0\nproperty float f_dc_1\nproperty float f_dc_2\nproperty float f_rest_0\nend_header\n0.0 0.1 1.0 0.9 1.0 1.1 1.2 0.0 0.0 0.0 1.0 0.2 0.3 0.4 0.125\n";
    const PLY_WITH_SH_REST_RGB: &str = "ply\nformat ascii 1.0\nelement vertex 1\nproperty float x\nproperty float y\nproperty float z\nproperty float opacity\nproperty float scale_0\nproperty float scale_1\nproperty float scale_2\nproperty float rot_0\nproperty float rot_1\nproperty float rot_2\nproperty float rot_3\nproperty float f_dc_0\nproperty float f_dc_1\nproperty float f_dc_2\nproperty float f_rest_0\nproperty float f_rest_1\nproperty float f_rest_2\nend_header\n0.0 0.1 1.0 0.9 1.0 1.1 1.2 0.0 0.0 0.0 1.0 0.2 0.3 0.4 0.125 0.25 0.5\n";

    #[test]
    fn parses_valid_ascii_ply() {
        let result = parse_ply_text(VALID_PLY).unwrap();
        assert_eq!(result.summary.gaussians, 1);
        assert!(!result.summary.has_sh_rest);
        assert_eq!(result.scene.positions[0].z, 1.0);
    }

    #[test]
    fn incomplete_sh_rest_does_not_set_summary_flag() {
        let result = parse_ply_text(PLY_INCOMPLETE_SH_REST).unwrap();
        assert_eq!(result.summary.gaussians, 1);
        assert!(!result.summary.has_sh_rest);
        assert!(result.scene.sh_rest_rgb.is_none());
    }

    #[test]
    fn parses_sh_rest_rgb_when_present() {
        let result = parse_ply_text(PLY_WITH_SH_REST_RGB).unwrap();
        assert_eq!(result.summary.gaussians, 1);
        assert!(result.summary.has_sh_rest);
        let sh = result.scene.sh_rest_rgb.as_ref().unwrap();
        assert_eq!(sh.len(), 1);
        assert_eq!(sh[0], [0.125, 0.25, 0.5]);
    }

    #[test]
    fn rejects_missing_required_field() {
        let broken = VALID_PLY.replace("property float opacity\n", "");
        let err = parse_ply_text(&broken).unwrap_err();
        assert_eq!(err, PlyLoadError::MissingField("opacity"));
    }

    #[test]
    fn rejects_non_ascii_format() {
        let broken = VALID_PLY.replace("format ascii 1.0", "format binary_little_endian 1.0");
        let err = parse_ply_text(&broken).unwrap_err();
        assert_eq!(err, PlyLoadError::UnsupportedFormat);
    }
}
