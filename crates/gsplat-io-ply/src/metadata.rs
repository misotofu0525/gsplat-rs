//! PLY header metadata parsing and validation.

use std::collections::HashMap;
use std::io::BufRead;

use crate::{PlyLoadError, PlyLoadLimits, ensure_limit, try_vec_with_capacity};

const REQUIRED_VERTEX_FIELDS: [&str; 14] = [
    "x", "y", "z", "opacity", "scale_0", "scale_1", "scale_2", "rot_0", "rot_1", "rot_2", "rot_3",
    "f_dc_0", "f_dc_1", "f_dc_2",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlySceneSummary {
    pub gaussians: usize,
    pub sh_degree: u8,
    pub has_sh_rest: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PlyFormat {
    Ascii,
    BinaryLittleEndian,
    BinaryBigEndian,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PlyScalarType {
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
    pub(super) const fn size_bytes(self) -> usize {
        match self {
            Self::Int8 | Self::UInt8 => 1,
            Self::Int16 | Self::UInt16 => 2,
            Self::Int32 | Self::UInt32 | Self::Float32 => 4,
            Self::Float64 => 8,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PlyProperty {
    pub(super) name: String,
    pub(super) ty: PlyScalarType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PlyHeader {
    pub(super) format: PlyFormat,
    pub(super) vertex_count: usize,
    pub(super) vertex_properties: Vec<PlyProperty>,
}

pub(super) fn build_property_indices(
    header: &PlyHeader,
) -> Result<HashMap<&str, usize>, PlyLoadError> {
    let mut indices = HashMap::new();
    indices
        .try_reserve(header.vertex_properties.len())
        .map_err(|_| PlyLoadError::AllocationFailed("property indices"))?;
    for (idx, prop) in header.vertex_properties.iter().enumerate() {
        indices.insert(prop.name.as_str(), idx);
    }

    for required in REQUIRED_VERTEX_FIELDS {
        if !indices.contains_key(required) {
            return Err(PlyLoadError::MissingField(required));
        }
    }

    Ok(indices)
}

pub(super) fn sh_rest_stride(sh_degree: u8, has_sh_rest: bool) -> Result<usize, PlyLoadError> {
    if !has_sh_rest {
        return Ok(0);
    }

    let degree_with_dc = usize::from(sh_degree)
        .checked_add(1)
        .ok_or(PlyLoadError::ResourceSizeOverflow("SH degree"))?;
    let coeff_total = degree_with_dc
        .checked_mul(degree_with_dc)
        .ok_or(PlyLoadError::ResourceSizeOverflow("SH coefficient count"))?;
    let rest_per_channel = coeff_total
        .checked_sub(1)
        .ok_or(PlyLoadError::ResourceSizeOverflow("SH coefficient count"))?;
    3_usize
        .checked_mul(rest_per_channel)
        .ok_or(PlyLoadError::ResourceSizeOverflow("SH coefficient stride"))
}

pub(super) fn read_header_from_reader<R: BufRead>(
    reader: &mut R,
    limits: PlyLoadLimits,
) -> Result<(PlyHeader, usize), PlyLoadError> {
    let initial_capacity = limits.max_header_bytes.min(8 * 1024);
    let mut header_bytes = try_vec_with_capacity("header bytes", initial_capacity)?;
    let mut line_start = 0_usize;

    loop {
        let (take, has_newline) = {
            let available = reader.fill_buf().map_err(|_| PlyLoadError::Io)?;
            if available.is_empty() {
                if line_start < header_bytes.len()
                    && is_end_header_line(&header_bytes[line_start..])?
                {
                    let header_text = std::str::from_utf8(&header_bytes)
                        .map_err(|_| PlyLoadError::MalformedHeader)?;
                    let header = parse_header_text(header_text, limits)?;
                    return Ok((header, header_bytes.len()));
                }
                return Err(PlyLoadError::MalformedHeader);
            }

            match available.iter().position(|&byte| byte == b'\n') {
                Some(position) => (position + 1, true),
                None => (available.len(), false),
            }
        };

        let next_len = header_bytes
            .len()
            .checked_add(take)
            .ok_or(PlyLoadError::ResourceSizeOverflow("header bytes"))?;
        ensure_limit("header bytes", next_len, limits.max_header_bytes)?;
        header_bytes
            .try_reserve(take)
            .map_err(|_| PlyLoadError::AllocationFailed("header bytes"))?;

        {
            let available = reader.fill_buf().map_err(|_| PlyLoadError::Io)?;
            header_bytes.extend_from_slice(&available[..take]);
        }
        reader.consume(take);

        if has_newline {
            if is_end_header_line(&header_bytes[line_start..])? {
                let header_text = std::str::from_utf8(&header_bytes)
                    .map_err(|_| PlyLoadError::MalformedHeader)?;
                let header = parse_header_text(header_text, limits)?;
                return Ok((header, header_bytes.len()));
            }
            line_start = header_bytes.len();
        }
    }
}

fn is_end_header_line(line: &[u8]) -> Result<bool, PlyLoadError> {
    let line = std::str::from_utf8(line).map_err(|_| PlyLoadError::MalformedHeader)?;
    Ok(line.trim() == "end_header")
}

pub(super) fn summary_from_header(header: &PlyHeader) -> Result<PlySceneSummary, PlyLoadError> {
    for required in REQUIRED_VERTEX_FIELDS {
        if !header
            .vertex_properties
            .iter()
            .any(|property| property.name == required)
        {
            return Err(PlyLoadError::MissingField(required));
        }
    }

    let (sh_degree, sh_rest_prop_indices) = infer_sh_rest_layout(header)?;
    Ok(PlySceneSummary {
        gaussians: header.vertex_count,
        sh_degree,
        has_sh_rest: sh_degree > 0 && sh_rest_prop_indices.is_some(),
    })
}

pub(super) fn complete_header_end(
    input: &[u8],
    limits: PlyLoadLimits,
) -> Result<Option<usize>, PlyLoadError> {
    let mut cursor = 0_usize;
    while let Some(offset) = input[cursor..].iter().position(|&byte| byte == b'\n') {
        let line_end = cursor + offset;
        let next_cursor = line_end + 1;
        ensure_limit("header bytes", next_cursor, limits.max_header_bytes)?;
        if is_end_header_line(&input[cursor..line_end])? {
            return Ok(Some(next_cursor));
        }
        cursor = next_cursor;
    }
    ensure_limit("header bytes", input.len(), limits.max_header_bytes)?;
    Ok(None)
}

pub(super) fn complete_header_end_at_eof(
    input: &[u8],
    limits: PlyLoadLimits,
) -> Result<Option<usize>, PlyLoadError> {
    if let Some(end) = complete_header_end(input, limits)? {
        return Ok(Some(end));
    }
    let line_start = input
        .iter()
        .rposition(|&byte| byte == b'\n')
        .map_or(0, |position| position + 1);
    if line_start < input.len() && is_end_header_line(&input[line_start..])? {
        ensure_limit("header bytes", input.len(), limits.max_header_bytes)?;
        Ok(Some(input.len()))
    } else {
        Ok(None)
    }
}

pub(super) fn split_header_body(
    input: &[u8],
    limits: PlyLoadLimits,
) -> Result<(PlyHeader, &[u8]), PlyLoadError> {
    // The header is always ASCII, even for binary PLY.
    let mut cursor = 0_usize;
    let mut header_end: Option<usize> = None;
    while cursor < input.len() {
        let scan_end = input.len().min(limits.max_header_bytes.saturating_add(1));
        if cursor >= scan_end {
            return Err(PlyLoadError::ResourceLimit {
                resource: "header bytes",
                requested: cursor.saturating_add(1),
                limit: limits.max_header_bytes,
            });
        }
        let line_end = input[cursor..scan_end]
            .iter()
            .position(|&b| b == b'\n')
            .map(|idx| cursor + idx)
            .unwrap_or(scan_end);
        let next_cursor = if line_end < input.len() {
            line_end + 1
        } else {
            line_end
        };
        ensure_limit("header bytes", next_cursor, limits.max_header_bytes)?;
        let line_bytes = &input[cursor..line_end];
        let line = std::str::from_utf8(line_bytes).map_err(|_| PlyLoadError::MalformedHeader)?;
        if line.trim_end_matches('\r').trim() == "end_header" {
            header_end = Some(next_cursor);
            break;
        }
        cursor = line_end.saturating_add(1);
    }

    let header_end = header_end.ok_or(PlyLoadError::MalformedHeader)?;
    let header_text =
        std::str::from_utf8(&input[..header_end]).map_err(|_| PlyLoadError::MalformedHeader)?;
    let header = parse_header_text(header_text, limits)?;
    Ok((header, &input[header_end..]))
}

pub(super) fn parse_header_text(
    input: &str,
    limits: PlyLoadLimits,
) -> Result<PlyHeader, PlyLoadError> {
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
                    let count = count
                        .and_then(|s| s.parse::<usize>().ok())
                        .ok_or(PlyLoadError::MalformedHeader)?;
                    ensure_limit("vertices", count, limits.max_vertices)?;
                    vertex_count = Some(count);
                }
            }
            Some("property") if in_vertex_element => {
                let ty = parts.next().ok_or(PlyLoadError::MalformedHeader)?;
                if ty == "list" {
                    return Err(PlyLoadError::UnsupportedFormat);
                }
                let name = parts.next().ok_or(PlyLoadError::MalformedHeader)?;
                let scalar_ty = parse_scalar_type(ty).ok_or(PlyLoadError::MalformedHeader)?;
                let property_count = vertex_properties
                    .len()
                    .checked_add(1)
                    .ok_or(PlyLoadError::ResourceSizeOverflow("vertex property count"))?;
                ensure_limit(
                    "vertex properties",
                    property_count,
                    limits.max_vertex_properties,
                )?;
                vertex_properties
                    .try_reserve(1)
                    .map_err(|_| PlyLoadError::AllocationFailed("vertex properties"))?;
                let mut property_name = String::new();
                property_name
                    .try_reserve_exact(name.len())
                    .map_err(|_| PlyLoadError::AllocationFailed("vertex property name"))?;
                property_name.push_str(name);
                vertex_properties.push(PlyProperty {
                    name: property_name,
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

pub(super) fn infer_sh_rest_layout(
    header: &PlyHeader,
) -> Result<(u8, Option<Vec<usize>>), PlyLoadError> {
    // A declared SH payload is all-or-nothing. Rendering malformed or partial
    // coefficients as SH0 changes scene appearance and hides corrupted exports.
    let mut rest_pairs: Vec<(usize, usize)> = Vec::new();
    rest_pairs
        .try_reserve(header.vertex_properties.len())
        .map_err(|_| PlyLoadError::AllocationFailed("SH property indices"))?;
    for (prop_index, prop) in header.vertex_properties.iter().enumerate() {
        if let Some(suffix) = prop.name.strip_prefix("f_rest_") {
            let rest_idx = suffix
                .parse::<usize>()
                .map_err(|_| PlyLoadError::InvalidShLayout)?;
            rest_pairs.push((rest_idx, prop_index));
        }
    }

    if rest_pairs.is_empty() {
        return Ok((0, None));
    }

    rest_pairs.sort_unstable_by_key(|(rest_idx, _)| *rest_idx);
    for (expected, (rest_idx, _)) in rest_pairs.iter().enumerate() {
        if *rest_idx != expected {
            return Err(PlyLoadError::InvalidShLayout);
        }
    }

    let rest_count_total = rest_pairs.len();
    let sh_degree = infer_sh_degree(rest_count_total).ok_or(PlyLoadError::InvalidShLayout)?;

    let mut prop_indices = try_vec_with_capacity("SH property indices", rest_pairs.len())?;
    for (_, prop_index) in rest_pairs {
        prop_indices.push(prop_index);
    }

    Ok((sh_degree, Some(prop_indices)))
}

fn infer_sh_degree(rest_count_total: usize) -> Option<u8> {
    match rest_count_total {
        9 => Some(1),
        24 => Some(2),
        45 => Some(3),
        _ => None,
    }
}
