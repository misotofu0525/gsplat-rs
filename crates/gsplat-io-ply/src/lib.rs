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

// Common 3DGS PLYs (COLMAP/OpenCV-style) are authored in RDF coordinates:
// +X right, +Y down, +Z forward. The runtime camera path in this workspace uses
// +X right, +Y up, +Z forward (RUF), so we convert once at load time.
//
// SH sign flips mirror the coordinate conversion used by NVIDIA's SPZ converter
// for a Y-axis flip (RDF -> RUF). Indices map to per-channel `f_rest_*` order.
const SH_FLIP_RDF_TO_RUF: [f32; 15] = [
    -1.0, 1.0, 1.0, -1.0, -1.0, 1.0, 1.0, 1.0, -1.0, -1.0, -1.0, 1.0, 1.0, 1.0, 1.0,
];

fn rotation_wxyz_to_xyzw(wxyz: [f32; 4]) -> [f32; 4] {
    [wxyz[1], wxyz[2], wxyz[3], wxyz[0]]
}

fn convert_scene_rdf_to_ruf(scene: &mut SceneBuffers) {
    for p in &mut scene.positions {
        p.y = -p.y;
    }

    // Quaternion vector-part adjustment under Y-axis reflection. Internal layout is xyzw.
    for q in &mut scene.rotation_xyzw {
        q[0] = -q[0];
        q[2] = -q[2];
    }

    let sh_degree = scene.sh_degree;
    if let Some(rest) = scene.sh_rest.as_mut() {
        flip_sh_rest_rdf_to_ruf(sh_degree, rest);
    }
}

fn flip_sh_rest_rdf_to_ruf(sh_degree: u8, sh_rest: &mut [f32]) {
    let coeff_total = (sh_degree as usize + 1).pow(2);
    let per_channel = coeff_total.saturating_sub(1);
    if per_channel == 0 {
        return;
    }
    let stride = per_channel * 3;
    if stride == 0 {
        return;
    }

    for gaussian_coeffs in sh_rest.chunks_exact_mut(stride) {
        for channel in 0..3 {
            let base = channel * per_channel;
            let coeffs = &mut gaussian_coeffs[base..base + per_channel];
            for (coeff_idx, coeff) in coeffs.iter_mut().enumerate() {
                let sign = SH_FLIP_RDF_TO_RUF.get(coeff_idx).copied().unwrap_or(1.0);
                *coeff *= sign;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlySceneSummary {
    pub gaussians: usize,
    pub sh_degree: u8,
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
    #[error(
        "unsupported PLY format; supported: ascii 1.0, binary_little_endian 1.0, binary_big_endian 1.0"
    )]
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
    parse_ply_bytes(&raw)
}

pub fn load_ply_summary(path: &Path) -> Result<PlySceneSummary, PlyLoadError> {
    Ok(load_ply(path)?.summary)
}

pub fn parse_ply_text(input: &str) -> Result<PlyLoadResult, PlyLoadError> {
    // `parse_ply_text` is intended for textual PLY payloads; reject binary headers early to avoid
    // mis-parsing an ASCII body as binary bytes.
    let (header, _) = split_header_body(input.as_bytes())?;
    if header.format != PlyFormat::Ascii {
        return Err(PlyLoadError::UnsupportedFormat);
    }
    parse_ply_bytes(input.as_bytes())
}

pub fn parse_ply_bytes(input: &[u8]) -> Result<PlyLoadResult, PlyLoadError> {
    let (header, body) = split_header_body(input)?;

    let mut indices: HashMap<&str, usize> = HashMap::new();
    for (idx, prop) in header.vertex_properties.iter().enumerate() {
        indices.insert(prop.name.as_str(), idx);
    }

    for required in REQUIRED_VERTEX_FIELDS {
        if !indices.contains_key(required) {
            return Err(PlyLoadError::MissingField(required));
        }
    }

    let (sh_degree, sh_rest_prop_indices) = infer_sh_rest_layout(&header)?;
    let has_sh_rest = sh_degree > 0 && sh_rest_prop_indices.is_some();

    let rest_stride = if has_sh_rest {
        let coeff_total = (sh_degree as usize + 1).pow(2);
        3 * (coeff_total - 1)
    } else {
        0
    };

    let mut scene = SceneBuffers {
        positions: Vec::with_capacity(header.vertex_count),
        opacity: Vec::with_capacity(header.vertex_count),
        scale_xyz: Vec::with_capacity(header.vertex_count),
        rotation_xyzw: Vec::with_capacity(header.vertex_count),
        color_dc: Vec::with_capacity(header.vertex_count),
        sh_degree,
        sh_rest: if has_sh_rest {
            Some(Vec::with_capacity(header.vertex_count * rest_stride))
        } else {
            None
        },
    };

    match header.format {
        PlyFormat::Ascii => {
            parse_ascii_body(&header, body, &indices, &sh_rest_prop_indices, &mut scene)?
        }
        PlyFormat::BinaryLittleEndian => parse_binary_body(
            &header,
            body,
            &indices,
            &sh_rest_prop_indices,
            Endian::Little,
            &mut scene,
        )?,
        PlyFormat::BinaryBigEndian => parse_binary_body(
            &header,
            body,
            &indices,
            &sh_rest_prop_indices,
            Endian::Big,
            &mut scene,
        )?,
    }

    convert_scene_rdf_to_ruf(&mut scene);
    scene.validate().map_err(|_| PlyLoadError::InvalidScene)?;

    Ok(PlyLoadResult {
        summary: PlySceneSummary {
            gaussians: scene.len(),
            sh_degree: scene.sh_degree,
            has_sh_rest: scene.sh_rest.is_some(),
        },
        scene,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlyFormat {
    Ascii,
    BinaryLittleEndian,
    BinaryBigEndian,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlyScalarType {
    Int8,
    UInt8,
    Int16,
    UInt16,
    Int32,
    UInt32,
    Float32,
    Float64,
}

impl PlyScalarType {
    const fn size_bytes(self) -> usize {
        match self {
            Self::Int8 | Self::UInt8 => 1,
            Self::Int16 | Self::UInt16 => 2,
            Self::Int32 | Self::UInt32 | Self::Float32 => 4,
            Self::Float64 => 8,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlyProperty {
    name: String,
    ty: PlyScalarType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlyHeader {
    format: PlyFormat,
    vertex_count: usize,
    vertex_properties: Vec<PlyProperty>,
}

fn split_header_body(input: &[u8]) -> Result<(PlyHeader, &[u8]), PlyLoadError> {
    // The header is always ASCII, even for binary PLY.
    let mut cursor = 0_usize;
    let mut header_end: Option<usize> = None;
    while cursor < input.len() {
        let line_end = input[cursor..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|idx| cursor + idx)
            .unwrap_or(input.len());
        let line_bytes = &input[cursor..line_end];
        let line = std::str::from_utf8(line_bytes).map_err(|_| PlyLoadError::MalformedHeader)?;
        if line.trim_end_matches('\r').trim() == "end_header" {
            header_end = Some(if line_end < input.len() {
                line_end + 1
            } else {
                line_end
            });
            break;
        }
        cursor = line_end.saturating_add(1);
    }

    let header_end = header_end.ok_or(PlyLoadError::MalformedHeader)?;
    let header_text =
        std::str::from_utf8(&input[..header_end]).map_err(|_| PlyLoadError::MalformedHeader)?;
    let header = parse_header_text(header_text)?;
    Ok((header, &input[header_end..]))
}

fn parse_header_text(input: &str) -> Result<PlyHeader, PlyLoadError> {
    let mut lines = input.lines();
    if lines.next().map(str::trim) != Some("ply") {
        return Err(PlyLoadError::MalformedHeader);
    }

    let mut format: Option<PlyFormat> = None;
    let mut saw_end_header = false;
    let mut vertex_count: Option<usize> = None;
    let mut in_vertex_element = false;
    let mut vertex_properties: Vec<PlyProperty> = Vec::new();

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
                format = match (parts.next(), parts.next()) {
                    (Some("ascii"), Some("1.0")) => Some(PlyFormat::Ascii),
                    (Some("binary_little_endian"), Some("1.0")) => {
                        Some(PlyFormat::BinaryLittleEndian)
                    }
                    (Some("binary_big_endian"), Some("1.0")) => Some(PlyFormat::BinaryBigEndian),
                    _ => None,
                };
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
                let ty = parts.next().ok_or(PlyLoadError::MalformedHeader)?;
                if ty == "list" {
                    return Err(PlyLoadError::UnsupportedFormat);
                }
                let name = parts.next().ok_or(PlyLoadError::MalformedHeader)?;
                let scalar_ty = parse_scalar_type(ty).ok_or(PlyLoadError::MalformedHeader)?;
                vertex_properties.push(PlyProperty {
                    name: name.to_owned(),
                    ty: scalar_ty,
                });
            }
            _ => {}
        }
    }

    let format = format.ok_or(PlyLoadError::UnsupportedFormat)?;
    if !saw_end_header {
        return Err(PlyLoadError::MalformedHeader);
    }

    let vertex_count = vertex_count.ok_or(PlyLoadError::MalformedHeader)?;

    Ok(PlyHeader {
        format,
        vertex_count,
        vertex_properties,
    })
}

fn parse_scalar_type(name: &str) -> Option<PlyScalarType> {
    match name {
        "char" | "int8" => Some(PlyScalarType::Int8),
        "uchar" | "uint8" => Some(PlyScalarType::UInt8),
        "short" | "int16" => Some(PlyScalarType::Int16),
        "ushort" | "uint16" => Some(PlyScalarType::UInt16),
        "int" | "int32" => Some(PlyScalarType::Int32),
        "uint" | "uint32" => Some(PlyScalarType::UInt32),
        "float" | "float32" => Some(PlyScalarType::Float32),
        "double" | "float64" => Some(PlyScalarType::Float64),
        _ => None,
    }
}

fn infer_sh_rest_layout(header: &PlyHeader) -> Result<(u8, Option<Vec<usize>>), PlyLoadError> {
    // Collect all f_rest_* fields, ensure they are contiguous, and infer SH degree.
    let mut rest_pairs: Vec<(usize, usize)> = Vec::new();
    for (prop_index, prop) in header.vertex_properties.iter().enumerate() {
        if let Some(rest_idx) = prop
            .name
            .strip_prefix("f_rest_")
            .and_then(|s| s.parse::<usize>().ok())
        {
            rest_pairs.push((rest_idx, prop_index));
        }
    }

    if rest_pairs.is_empty() {
        return Ok((0, None));
    }

    rest_pairs.sort_by_key(|(rest_idx, _)| *rest_idx);
    let max_idx = rest_pairs.last().map(|(i, _)| *i).unwrap_or(0);

    // Must contain every index 0..=max_idx exactly once.
    if rest_pairs.len() != max_idx.saturating_add(1) {
        return Ok((0, None));
    }
    for (expected, (rest_idx, _)) in rest_pairs.iter().enumerate() {
        if *rest_idx != expected {
            return Ok((0, None));
        }
    }

    let rest_count_total = rest_pairs.len();
    let sh_degree = infer_sh_degree(rest_count_total).unwrap_or(0);
    if sh_degree == 0 {
        return Ok((0, None));
    }

    let mut prop_indices = Vec::with_capacity(rest_pairs.len());
    for (_, prop_index) in rest_pairs {
        prop_indices.push(prop_index);
    }

    Ok((sh_degree, Some(prop_indices)))
}

fn infer_sh_degree(rest_count_total: usize) -> Option<u8> {
    if rest_count_total % 3 != 0 {
        return None;
    }
    let per_channel = rest_count_total / 3;
    // per_channel == (degree + 1)^2 - 1
    let coeff_total = per_channel.checked_add(1)?;
    let root = integer_sqrt(coeff_total)?;
    if root * root != coeff_total {
        return None;
    }
    let degree = root.checked_sub(1)?;
    if degree > 4 {
        return None;
    }
    Some(degree as u8)
}

fn integer_sqrt(value: usize) -> Option<usize> {
    if value == 0 {
        return Some(0);
    }
    let mut x = (value as f64).sqrt() as usize;
    while x * x > value {
        x = x.saturating_sub(1);
    }
    while (x + 1) * (x + 1) <= value {
        x = x.saturating_add(1);
    }
    Some(x)
}

fn parse_ascii_body(
    header: &PlyHeader,
    body: &[u8],
    indices: &HashMap<&str, usize>,
    sh_rest_prop_indices: &Option<Vec<usize>>,
    scene: &mut SceneBuffers,
) -> Result<(), PlyLoadError> {
    let body_text = std::str::from_utf8(body).map_err(|_| PlyLoadError::UnsupportedFormat)?;
    let mut lines = body_text.lines();

    for _ in 0..header.vertex_count {
        let mut row: Option<&str> = None;
        while let Some(line) = lines.next() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("comment") {
                continue;
            }
            row = Some(trimmed);
            break;
        }

        let row = row.ok_or(PlyLoadError::VertexCountMismatch)?;
        let values: Vec<&str> = row.split_whitespace().collect();
        if values.len() < header.vertex_properties.len() {
            return Err(PlyLoadError::VertexFieldCount);
        }

        let x = parse_field_f32_ascii(&values, indices, "x")?;
        let y = parse_field_f32_ascii(&values, indices, "y")?;
        let z = parse_field_f32_ascii(&values, indices, "z")?;
        scene.positions.push(Vec3f::new(x, y, z));

        scene
            .opacity
            .push(parse_field_f32_ascii(&values, indices, "opacity")?);

        scene.scale_xyz.push([
            parse_field_f32_ascii(&values, indices, "scale_0")?,
            parse_field_f32_ascii(&values, indices, "scale_1")?,
            parse_field_f32_ascii(&values, indices, "scale_2")?,
        ]);

        scene.rotation_xyzw.push(rotation_wxyz_to_xyzw([
            parse_field_f32_ascii(&values, indices, "rot_0")?,
            parse_field_f32_ascii(&values, indices, "rot_1")?,
            parse_field_f32_ascii(&values, indices, "rot_2")?,
            parse_field_f32_ascii(&values, indices, "rot_3")?,
        ]));

        scene.color_dc.push([
            parse_field_f32_ascii(&values, indices, "f_dc_0")?,
            parse_field_f32_ascii(&values, indices, "f_dc_1")?,
            parse_field_f32_ascii(&values, indices, "f_dc_2")?,
        ]);

        if let (Some(rest_prop_indices), Some(rest_out)) =
            (sh_rest_prop_indices.as_ref(), scene.sh_rest.as_mut())
        {
            for &prop_index in rest_prop_indices {
                rest_out.push(parse_field_f32_ascii_idx(&values, prop_index)?);
            }
        }
    }

    Ok(())
}

fn parse_field_f32_ascii(
    values: &[&str],
    indices: &HashMap<&str, usize>,
    field: &str,
) -> Result<f32, PlyLoadError> {
    let idx = indices
        .get(field)
        .copied()
        .ok_or(PlyLoadError::MalformedHeader)?;
    parse_field_f32_ascii_idx(values, idx)
}

fn parse_field_f32_ascii_idx(values: &[&str], idx: usize) -> Result<f32, PlyLoadError> {
    let value = values.get(idx).ok_or(PlyLoadError::VertexFieldCount)?;
    value.parse::<f32>().map_err(|_| PlyLoadError::ParseNumber)
}

#[derive(Clone, Copy)]
enum Endian {
    Little,
    Big,
}

fn parse_binary_body(
    header: &PlyHeader,
    body: &[u8],
    indices: &HashMap<&str, usize>,
    sh_rest_prop_indices: &Option<Vec<usize>>,
    endian: Endian,
    scene: &mut SceneBuffers,
) -> Result<(), PlyLoadError> {
    let (stride, offsets) = compute_vertex_layout(header)?;
    let required_bytes = header.vertex_count.saturating_mul(stride);
    if body.len() < required_bytes {
        return Err(PlyLoadError::VertexCountMismatch);
    }

    for vertex_index in 0..header.vertex_count {
        let base = vertex_index * stride;
        let record = &body[base..base + stride];

        let x = read_field_f32_binary(record, header, &offsets, indices, "x", endian)?;
        let y = read_field_f32_binary(record, header, &offsets, indices, "y", endian)?;
        let z = read_field_f32_binary(record, header, &offsets, indices, "z", endian)?;
        scene.positions.push(Vec3f::new(x, y, z));

        scene.opacity.push(read_field_f32_binary(
            record, header, &offsets, indices, "opacity", endian,
        )?);

        scene.scale_xyz.push([
            read_field_f32_binary(record, header, &offsets, indices, "scale_0", endian)?,
            read_field_f32_binary(record, header, &offsets, indices, "scale_1", endian)?,
            read_field_f32_binary(record, header, &offsets, indices, "scale_2", endian)?,
        ]);

        scene.rotation_xyzw.push(rotation_wxyz_to_xyzw([
            read_field_f32_binary(record, header, &offsets, indices, "rot_0", endian)?,
            read_field_f32_binary(record, header, &offsets, indices, "rot_1", endian)?,
            read_field_f32_binary(record, header, &offsets, indices, "rot_2", endian)?,
            read_field_f32_binary(record, header, &offsets, indices, "rot_3", endian)?,
        ]));

        scene.color_dc.push([
            read_field_f32_binary(record, header, &offsets, indices, "f_dc_0", endian)?,
            read_field_f32_binary(record, header, &offsets, indices, "f_dc_1", endian)?,
            read_field_f32_binary(record, header, &offsets, indices, "f_dc_2", endian)?,
        ]);

        if let (Some(rest_prop_indices), Some(rest_out)) =
            (sh_rest_prop_indices.as_ref(), scene.sh_rest.as_mut())
        {
            for &prop_index in rest_prop_indices {
                rest_out.push(read_field_f32_binary_idx(
                    record, header, &offsets, prop_index, endian,
                )?);
            }
        }
    }

    Ok(())
}

fn compute_vertex_layout(header: &PlyHeader) -> Result<(usize, Vec<usize>), PlyLoadError> {
    let mut offsets = Vec::with_capacity(header.vertex_properties.len());
    let mut offset = 0_usize;
    for prop in &header.vertex_properties {
        offsets.push(offset);
        offset = offset
            .checked_add(prop.ty.size_bytes())
            .ok_or(PlyLoadError::MalformedHeader)?;
    }
    Ok((offset, offsets))
}

fn read_field_f32_binary(
    record: &[u8],
    header: &PlyHeader,
    offsets: &[usize],
    indices: &HashMap<&str, usize>,
    field: &str,
    endian: Endian,
) -> Result<f32, PlyLoadError> {
    let prop_index = indices
        .get(field)
        .copied()
        .ok_or(PlyLoadError::MalformedHeader)?;
    read_field_f32_binary_idx(record, header, offsets, prop_index, endian)
}

fn read_field_f32_binary_idx(
    record: &[u8],
    header: &PlyHeader,
    offsets: &[usize],
    prop_index: usize,
    endian: Endian,
) -> Result<f32, PlyLoadError> {
    let prop = header
        .vertex_properties
        .get(prop_index)
        .ok_or(PlyLoadError::MalformedHeader)?;
    let offset = *offsets
        .get(prop_index)
        .ok_or(PlyLoadError::MalformedHeader)?;
    read_scalar_f32(record, offset, prop.ty, endian)
}

fn read_scalar_f32(
    record: &[u8],
    offset: usize,
    ty: PlyScalarType,
    endian: Endian,
) -> Result<f32, PlyLoadError> {
    let size = ty.size_bytes();
    let end = offset
        .checked_add(size)
        .ok_or(PlyLoadError::MalformedHeader)?;
    if end > record.len() {
        return Err(PlyLoadError::VertexFieldCount);
    }
    let bytes = &record[offset..end];

    let value = match (ty, endian) {
        (PlyScalarType::Int8, _) => i8::from_ne_bytes([bytes[0]]) as f32,
        (PlyScalarType::UInt8, _) => u8::from_ne_bytes([bytes[0]]) as f32,
        (PlyScalarType::Int16, Endian::Little) => {
            i16::from_le_bytes(bytes.try_into().unwrap()) as f32
        }
        (PlyScalarType::Int16, Endian::Big) => i16::from_be_bytes(bytes.try_into().unwrap()) as f32,
        (PlyScalarType::UInt16, Endian::Little) => {
            u16::from_le_bytes(bytes.try_into().unwrap()) as f32
        }
        (PlyScalarType::UInt16, Endian::Big) => {
            u16::from_be_bytes(bytes.try_into().unwrap()) as f32
        }
        (PlyScalarType::Int32, Endian::Little) => {
            i32::from_le_bytes(bytes.try_into().unwrap()) as f32
        }
        (PlyScalarType::Int32, Endian::Big) => i32::from_be_bytes(bytes.try_into().unwrap()) as f32,
        (PlyScalarType::UInt32, Endian::Little) => {
            u32::from_le_bytes(bytes.try_into().unwrap()) as f32
        }
        (PlyScalarType::UInt32, Endian::Big) => {
            u32::from_be_bytes(bytes.try_into().unwrap()) as f32
        }
        (PlyScalarType::Float32, Endian::Little) => f32::from_le_bytes(bytes.try_into().unwrap()),
        (PlyScalarType::Float32, Endian::Big) => f32::from_be_bytes(bytes.try_into().unwrap()),
        (PlyScalarType::Float64, Endian::Little) => {
            f64::from_le_bytes(bytes.try_into().unwrap()) as f32
        }
        (PlyScalarType::Float64, Endian::Big) => {
            f64::from_be_bytes(bytes.try_into().unwrap()) as f32
        }
    };

    if value.is_finite() {
        Ok(value)
    } else {
        Err(PlyLoadError::ParseNumber)
    }
}

#[cfg(test)]
mod tests {
    use super::{PlyLoadError, parse_ply_bytes, parse_ply_text};

    const VALID_PLY: &str = "ply\nformat ascii 1.0\nelement vertex 1\nproperty float x\nproperty float y\nproperty float z\nproperty float opacity\nproperty float scale_0\nproperty float scale_1\nproperty float scale_2\nproperty float rot_0\nproperty float rot_1\nproperty float rot_2\nproperty float rot_3\nproperty float f_dc_0\nproperty float f_dc_1\nproperty float f_dc_2\nend_header\n0.0 0.1 1.0 0.9 1.0 1.1 1.2 1.0 0.0 0.0 0.0 0.2 0.3 0.4\n";
    const VALID_PLY_NON_IDENTITY_QUAT: &str = "ply\nformat ascii 1.0\nelement vertex 1\nproperty float x\nproperty float y\nproperty float z\nproperty float opacity\nproperty float scale_0\nproperty float scale_1\nproperty float scale_2\nproperty float rot_0\nproperty float rot_1\nproperty float rot_2\nproperty float rot_3\nproperty float f_dc_0\nproperty float f_dc_1\nproperty float f_dc_2\nend_header\n0.0 0.1 1.0 0.9 1.0 1.1 1.2 0.9 0.2 0.3 0.4 0.2 0.3 0.4\n";
    const PLY_INCOMPLETE_SH_REST: &str = "ply\nformat ascii 1.0\nelement vertex 1\nproperty float x\nproperty float y\nproperty float z\nproperty float opacity\nproperty float scale_0\nproperty float scale_1\nproperty float scale_2\nproperty float rot_0\nproperty float rot_1\nproperty float rot_2\nproperty float rot_3\nproperty float f_dc_0\nproperty float f_dc_1\nproperty float f_dc_2\nproperty float f_rest_0\nend_header\n0.0 0.1 1.0 0.9 1.0 1.1 1.2 1.0 0.0 0.0 0.0 0.2 0.3 0.4 0.125\n";
    const PLY_WITH_SH_REST_DEG1: &str = "ply\nformat ascii 1.0\nelement vertex 1\nproperty float x\nproperty float y\nproperty float z\nproperty float opacity\nproperty float scale_0\nproperty float scale_1\nproperty float scale_2\nproperty float rot_0\nproperty float rot_1\nproperty float rot_2\nproperty float rot_3\nproperty float f_dc_0\nproperty float f_dc_1\nproperty float f_dc_2\nproperty float f_rest_0\nproperty float f_rest_1\nproperty float f_rest_2\nproperty float f_rest_3\nproperty float f_rest_4\nproperty float f_rest_5\nproperty float f_rest_6\nproperty float f_rest_7\nproperty float f_rest_8\nend_header\n0.0 0.1 1.0 0.9 1.0 1.1 1.2 1.0 0.0 0.0 0.0 0.2 0.3 0.4 0.01 0.02 0.03 0.04 0.05 0.06 0.07 0.08 0.09\n";

    #[test]
    fn parses_valid_ascii_ply() {
        let result = parse_ply_text(VALID_PLY).unwrap();
        assert_eq!(result.summary.gaussians, 1);
        assert_eq!(result.summary.sh_degree, 0);
        assert!(!result.summary.has_sh_rest);
        assert_eq!(result.scene.positions[0].y, -0.1);
        assert_eq!(result.scene.positions[0].z, 1.0);
        assert_eq!(result.scene.rotation_xyzw[0], [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn flips_quaternion_components_for_rdf_to_ruf() {
        let result = parse_ply_text(VALID_PLY_NON_IDENTITY_QUAT).unwrap();
        // Input rot is wxyz=(0.9, 0.2, 0.3, 0.4). Internal xyzw before coord conversion
        // would be (0.2, 0.3, 0.4, 0.9). RDF->RUF flips quaternion x and z.
        assert_eq!(result.scene.rotation_xyzw[0], [-0.2, 0.3, -0.4, 0.9]);
    }

    #[test]
    fn incomplete_sh_rest_does_not_set_summary_flag() {
        let result = parse_ply_text(PLY_INCOMPLETE_SH_REST).unwrap();
        assert_eq!(result.summary.gaussians, 1);
        assert_eq!(result.summary.sh_degree, 0);
        assert!(!result.summary.has_sh_rest);
        assert!(result.scene.sh_rest.is_none());
    }

    #[test]
    fn parses_sh_rest_when_present_and_complete() {
        let result = parse_ply_text(PLY_WITH_SH_REST_DEG1).unwrap();
        assert_eq!(result.summary.gaussians, 1);
        assert!(result.summary.has_sh_rest);
        assert_eq!(result.summary.sh_degree, 1);
        let sh = result.scene.sh_rest.as_ref().unwrap();
        assert_eq!(sh.len(), 9);
        assert_eq!(sh[0], -0.01);
        assert_eq!(sh[1], 0.02);
        assert_eq!(sh[2], 0.03);
        assert_eq!(sh[3], -0.04);
        assert_eq!(sh[8], 0.09);
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

    #[test]
    fn parses_header_without_trailing_newline() {
        let no_newline = "ply\nformat ascii 1.0\nelement vertex 0\nproperty float x\nproperty float y\nproperty float z\nproperty float opacity\nproperty float scale_0\nproperty float scale_1\nproperty float scale_2\nproperty float rot_0\nproperty float rot_1\nproperty float rot_2\nproperty float rot_3\nproperty float f_dc_0\nproperty float f_dc_1\nproperty float f_dc_2\nend_header";
        let result = parse_ply_bytes(no_newline.as_bytes()).unwrap();
        assert_eq!(result.summary.gaussians, 0);
    }

    #[test]
    fn parses_binary_little_endian_ply() {
        let header = "ply\nformat binary_little_endian 1.0\nelement vertex 1\nproperty float x\nproperty float y\nproperty float z\nproperty float opacity\nproperty float scale_0\nproperty float scale_1\nproperty float scale_2\nproperty float rot_0\nproperty float rot_1\nproperty float rot_2\nproperty float rot_3\nproperty float f_dc_0\nproperty float f_dc_1\nproperty float f_dc_2\nend_header\n";

        let mut bytes = header.as_bytes().to_vec();
        let values: [f32; 14] = [
            0.0, 0.1, 1.0, 0.9, 1.0, 1.1, 1.2, 1.0, 0.0, 0.0, 0.0, 0.2, 0.3, 0.4,
        ];
        for v in values {
            bytes.extend_from_slice(&v.to_le_bytes());
        }

        let result = parse_ply_bytes(&bytes).unwrap();
        assert_eq!(result.summary.gaussians, 1);
        assert_eq!(result.summary.sh_degree, 0);
        assert!(!result.summary.has_sh_rest);
        assert_eq!(result.scene.positions[0].y, -0.1);
        assert_eq!(result.scene.opacity[0], 0.9);
        assert_eq!(result.scene.rotation_xyzw[0], [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn parses_binary_sh_rest_degree_1() {
        let header = "ply\nformat binary_little_endian 1.0\nelement vertex 1\nproperty float x\nproperty float y\nproperty float z\nproperty float opacity\nproperty float scale_0\nproperty float scale_1\nproperty float scale_2\nproperty float rot_0\nproperty float rot_1\nproperty float rot_2\nproperty float rot_3\nproperty float f_dc_0\nproperty float f_dc_1\nproperty float f_dc_2\nproperty float f_rest_0\nproperty float f_rest_1\nproperty float f_rest_2\nproperty float f_rest_3\nproperty float f_rest_4\nproperty float f_rest_5\nproperty float f_rest_6\nproperty float f_rest_7\nproperty float f_rest_8\nend_header\n";

        let mut bytes = header.as_bytes().to_vec();
        let base: [f32; 14] = [
            0.0, 0.1, 1.0, 0.9, 1.0, 1.1, 1.2, 1.0, 0.0, 0.0, 0.0, 0.2, 0.3, 0.4,
        ];
        for v in base {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        let rest: [f32; 9] = [0.01, 0.02, 0.03, 0.04, 0.05, 0.06, 0.07, 0.08, 0.09];
        for v in rest {
            bytes.extend_from_slice(&v.to_le_bytes());
        }

        let result = parse_ply_bytes(&bytes).unwrap();
        assert_eq!(result.summary.gaussians, 1);
        assert_eq!(result.summary.sh_degree, 1);
        assert!(result.summary.has_sh_rest);
        let sh = result.scene.sh_rest.as_ref().unwrap();
        assert_eq!(sh.len(), 9);
        assert_eq!(sh[0], -0.01);
        assert_eq!(sh[8], 0.09);
    }
}
