//! Metadata-driven decoding for one PLY vertex.

use std::collections::HashMap;

use gsplat_core::Vec3f;

use crate::metadata::{PlyHeader, PlyScalarType};
use crate::{DecodedPlySplat, MAX_SH_REST_COEFFICIENTS, PlyLoadError, try_vec_with_capacity};

// Scaniverse can encode exactly transparent/opaque splats as infinite logits.
// A finite clamp preserves the post-sigmoid alpha while keeping SceneBuffers valid.
pub(super) const OPACITY_LOGIT_LIMIT: f32 = 16.0;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RotationLayout {
    Wxyz,
    Xyzw,
}

impl RotationLayout {
    pub(super) fn from_env() -> Self {
        // Most 3DGS exports use wxyz for rot_0..3, but some toolchains emit xyzw.
        // Parse the runtime override once per load rather than once per vertex.
        if matches!(
            std::env::var("GSPLAT_ROT_LAYOUT").ok().as_deref(),
            Some("xyzw") | Some("XYZW")
        ) {
            Self::Xyzw
        } else {
            Self::Wxyz
        }
    }

    fn input_to_xyzw(self, raw: [f32; 4]) -> [f32; 4] {
        match self {
            Self::Wxyz => rotation_wxyz_to_xyzw(raw),
            Self::Xyzw => raw,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DecodedField {
    Ignore,
    Position(usize),
    Opacity,
    Scale(usize),
    Rotation(usize),
    ColorDc(usize),
    ShRest(usize),
}

pub(super) struct VertexDecodePlan {
    fields: Vec<DecodedField>,
    sh_degree: u8,
    sh_rest_len: u8,
    rotation_layout: RotationLayout,
}

impl VertexDecodePlan {
    pub(super) fn new(
        header: &PlyHeader,
        indices: &HashMap<&str, usize>,
        sh_rest_prop_indices: Option<&[usize]>,
        sh_degree: u8,
        rotation_layout: RotationLayout,
    ) -> Result<Self, PlyLoadError> {
        let mut fields =
            try_vec_with_capacity("vertex decode fields", header.vertex_properties.len())?;
        fields.resize(header.vertex_properties.len(), DecodedField::Ignore);

        let required_fields = [
            ("x", DecodedField::Position(0)),
            ("y", DecodedField::Position(1)),
            ("z", DecodedField::Position(2)),
            ("opacity", DecodedField::Opacity),
            ("scale_0", DecodedField::Scale(0)),
            ("scale_1", DecodedField::Scale(1)),
            ("scale_2", DecodedField::Scale(2)),
            ("rot_0", DecodedField::Rotation(0)),
            ("rot_1", DecodedField::Rotation(1)),
            ("rot_2", DecodedField::Rotation(2)),
            ("rot_3", DecodedField::Rotation(3)),
            ("f_dc_0", DecodedField::ColorDc(0)),
            ("f_dc_1", DecodedField::ColorDc(1)),
            ("f_dc_2", DecodedField::ColorDc(2)),
        ];
        for (name, field) in required_fields {
            let property_index = indices
                .get(name)
                .copied()
                .ok_or(PlyLoadError::MalformedHeader)?;
            let target = fields
                .get_mut(property_index)
                .ok_or(PlyLoadError::MalformedHeader)?;
            *target = field;
        }

        let sh_rest_prop_indices = sh_rest_prop_indices.unwrap_or_default();
        if sh_rest_prop_indices.len() > MAX_SH_REST_COEFFICIENTS {
            return Err(PlyLoadError::InvalidShLayout);
        }
        for (coefficient_index, &property_index) in sh_rest_prop_indices.iter().enumerate() {
            let target = fields
                .get_mut(property_index)
                .ok_or(PlyLoadError::MalformedHeader)?;
            *target = DecodedField::ShRest(coefficient_index);
        }

        let sh_rest_len = u8::try_from(sh_rest_prop_indices.len())
            .map_err(|_| PlyLoadError::ResourceSizeOverflow("SH coefficient count"))?;
        Ok(Self {
            fields,
            sh_degree,
            sh_rest_len,
            rotation_layout,
        })
    }

    fn empty_splat(&self) -> DecodedPlySplat {
        DecodedPlySplat {
            position_ruf: Vec3f::new(0.0, 0.0, 0.0),
            opacity_logit: 0.0,
            log_scale_xyz: [0.0; 3],
            rotation_xyzw: [0.0; 4],
            color_dc: [0.0; 3],
            sh_rest: [0.0; MAX_SH_REST_COEFFICIENTS],
            sh_rest_len: self.sh_rest_len,
            sh_degree: self.sh_degree,
        }
    }

    fn finish_splat(&self, mut splat: DecodedPlySplat) -> DecodedPlySplat {
        splat.position_ruf.y = -splat.position_ruf.y;
        splat.rotation_xyzw = self.rotation_layout.input_to_xyzw(splat.rotation_xyzw);
        splat.rotation_xyzw[0] = -splat.rotation_xyzw[0];
        splat.rotation_xyzw[2] = -splat.rotation_xyzw[2];

        let sh_rest_len = usize::from(self.sh_rest_len);
        let per_channel = sh_rest_len / 3;
        if per_channel > 0 {
            for (index, coefficient) in splat.sh_rest[..sh_rest_len].iter_mut().enumerate() {
                let coefficient_index = index % per_channel;
                let sign = SH_FLIP_RDF_TO_RUF
                    .get(coefficient_index)
                    .copied()
                    .unwrap_or(1.0);
                *coefficient *= sign;
            }
        }
        splat
    }
}

fn assign_decoded_value(
    splat: &mut DecodedPlySplat,
    field: DecodedField,
    value: f32,
) -> Result<(), PlyLoadError> {
    match field {
        DecodedField::Ignore => {}
        DecodedField::Position(0) => splat.position_ruf.x = value,
        DecodedField::Position(1) => splat.position_ruf.y = value,
        DecodedField::Position(2) => splat.position_ruf.z = value,
        DecodedField::Position(_) => return Err(PlyLoadError::MalformedHeader),
        DecodedField::Opacity => splat.opacity_logit = value,
        DecodedField::Scale(index) => {
            *splat
                .log_scale_xyz
                .get_mut(index)
                .ok_or(PlyLoadError::MalformedHeader)? = value;
        }
        DecodedField::Rotation(index) => {
            *splat
                .rotation_xyzw
                .get_mut(index)
                .ok_or(PlyLoadError::MalformedHeader)? = value;
        }
        DecodedField::ColorDc(index) => {
            *splat
                .color_dc
                .get_mut(index)
                .ok_or(PlyLoadError::MalformedHeader)? = value;
        }
        DecodedField::ShRest(index) => {
            *splat
                .sh_rest
                .get_mut(index)
                .ok_or(PlyLoadError::InvalidShLayout)? = value;
        }
    }
    Ok(())
}

fn normalize_decoded_value(field: DecodedField, value: f32) -> Result<f32, PlyLoadError> {
    if field == DecodedField::Opacity {
        normalize_opacity_logit(value)
    } else if value.is_finite() {
        Ok(value)
    } else {
        Err(PlyLoadError::ParseNumber)
    }
}

pub(super) fn decode_ascii_vertex(
    row: &str,
    header: &PlyHeader,
    decode_plan: &VertexDecodePlan,
) -> Result<DecodedPlySplat, PlyLoadError> {
    if decode_plan.fields.len() != header.vertex_properties.len() {
        return Err(PlyLoadError::MalformedHeader);
    }
    if row.split_whitespace().count() < header.vertex_properties.len() {
        return Err(PlyLoadError::VertexFieldCount);
    }

    let mut splat = decode_plan.empty_splat();
    let mut values = row.split_whitespace();
    for field in decode_plan.fields.iter().copied() {
        let value = values.next().ok_or(PlyLoadError::VertexFieldCount)?;
        if field == DecodedField::Ignore {
            continue;
        }
        let value = value
            .parse::<f32>()
            .map_err(|_| PlyLoadError::ParseNumber)?;
        assign_decoded_value(&mut splat, field, normalize_decoded_value(field, value)?)?;
    }

    Ok(decode_plan.finish_splat(splat))
}

#[derive(Clone, Copy)]
pub(super) enum Endian {
    Little,
    Big,
}

pub(super) fn decode_binary_vertex(
    record: &[u8],
    header: &PlyHeader,
    offsets: &[usize],
    decode_plan: &VertexDecodePlan,
    endian: Endian,
) -> Result<DecodedPlySplat, PlyLoadError> {
    if decode_plan.fields.len() != header.vertex_properties.len()
        || offsets.len() != header.vertex_properties.len()
    {
        return Err(PlyLoadError::MalformedHeader);
    }

    let mut splat = decode_plan.empty_splat();
    for (property_index, field) in decode_plan.fields.iter().copied().enumerate() {
        if field == DecodedField::Ignore {
            continue;
        }
        let property = header
            .vertex_properties
            .get(property_index)
            .ok_or(PlyLoadError::MalformedHeader)?;
        let offset = offsets
            .get(property_index)
            .copied()
            .ok_or(PlyLoadError::MalformedHeader)?;
        let value = read_scalar_f32_raw(record, offset, property.ty, endian)?;
        assign_decoded_value(&mut splat, field, normalize_decoded_value(field, value)?)?;
    }

    Ok(decode_plan.finish_splat(splat))
}

pub(super) fn compute_vertex_layout(
    header: &PlyHeader,
) -> Result<(usize, Vec<usize>), PlyLoadError> {
    let mut offsets = try_vec_with_capacity(
        "binary vertex property offsets",
        header.vertex_properties.len(),
    )?;
    let mut offset = 0_usize;
    for prop in &header.vertex_properties {
        offsets.push(offset);
        offset = offset
            .checked_add(prop.ty.size_bytes())
            .ok_or(PlyLoadError::ResourceSizeOverflow("binary vertex stride"))?;
    }
    Ok((offset, offsets))
}

fn read_scalar_f32_raw(
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

    Ok(value)
}

fn normalize_opacity_logit(value: f32) -> Result<f32, PlyLoadError> {
    if value.is_finite() {
        Ok(value)
    } else if value == f32::INFINITY {
        Ok(OPACITY_LOGIT_LIMIT)
    } else if value == f32::NEG_INFINITY {
        Ok(-OPACITY_LOGIT_LIMIT)
    } else {
        Err(PlyLoadError::ParseNumber)
    }
}
