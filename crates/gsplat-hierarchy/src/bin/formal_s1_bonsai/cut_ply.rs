//! Private deterministic PLY materialization for the two formal S1 proxy cuts.
//!
//! This is an author-time bridge only. It does not decode hierarchy artifacts
//! for a runtime, own renderer state, or establish any S2 streaming surface.

use crate::authoring::{Result, invalid};
use gsplat_hierarchy::DrawableGaussian;
use gsplat_io_ply::{PlyLoadLimits, visit_ply_bytes_splats_with_limits};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

pub(super) const CUT_PLY_SCHEMA: &str = "gsplat-formal-s1-cut-render-input/v1";
const SH3_REST_COMPONENTS: usize = 45;
const BODY_FLOATS_PER_SPLAT: usize = 3 + 3 + 1 + 3 + 4 + SH3_REST_COMPONENTS;
const BODY_BYTES_PER_SPLAT: usize = BODY_FLOATS_PER_SPLAT * size_of::<f32>();
const MAX_PARAMETER_NEIGHBORS: usize = 8_192;
pub(super) const MAX_NONLINEAR_ROUNDTRIP_ULPS: u32 = 4;

// These finite values map to exact f32 alpha endpoints under the same sigmoid
// used by the formal source loader: 1 / (1 + exp(-logit)).
const ZERO_OPACITY_LOGIT: f32 = -104.0;
const ONE_OPACITY_LOGIT: f32 = 17.0;

const SH_FLIP_RUF_TO_RDF: [f32; 15] = [
    -1.0, 1.0, 1.0, -1.0, -1.0, 1.0, 1.0, 1.0, -1.0, -1.0, -1.0, 1.0, 1.0, 1.0, 1.0,
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CutPlyIdentity {
    pub(super) path: String,
    pub(super) sha256: String,
    pub(super) bytes: u64,
    pub(super) splat_count: u64,
}

pub(super) fn author_file(
    staging: &Path,
    relative_path: &str,
    drawables: &[DrawableGaussian],
) -> Result<CutPlyIdentity> {
    let bytes = encode_verified(drawables)?;
    let identity = CutPlyIdentity {
        path: relative_path.to_owned(),
        sha256: hash_bytes(&bytes),
        bytes: bytes.len() as u64,
        splat_count: drawables.len() as u64,
    };
    fs::write(staging.join(relative_path), bytes)?;
    Ok(identity)
}

pub(super) fn verify_file(
    staging: &Path,
    identity: &CutPlyIdentity,
    expected: &[DrawableGaussian],
) -> Result<()> {
    let bytes = fs::read(staging.join(&identity.path))?;
    if bytes.len() as u64 != identity.bytes || hash_bytes(&bytes) != identity.sha256 {
        return Err(invalid(format!(
            "staged cut PLY identity mismatch for {}",
            identity.path
        )));
    }
    if expected.len() as u64 != identity.splat_count {
        return Err(invalid(format!(
            "staged cut PLY count binding mismatch for {}",
            identity.path
        )));
    }
    verify_payload(&bytes, expected)
}

fn encode_verified(drawables: &[DrawableGaussian]) -> Result<Vec<u8>> {
    let bytes = encode(drawables)?;
    verify_payload(&bytes, drawables)?;
    Ok(bytes)
}

fn encode(drawables: &[DrawableGaussian]) -> Result<Vec<u8>> {
    if drawables.is_empty() {
        return Err(invalid("formal cut PLY cannot be empty"));
    }
    let header = canonical_header(drawables.len());
    let body_bytes = drawables
        .len()
        .checked_mul(BODY_BYTES_PER_SPLAT)
        .ok_or_else(|| invalid("formal cut PLY body byte count overflow"))?;
    let capacity = header
        .len()
        .checked_add(body_bytes)
        .ok_or_else(|| invalid("formal cut PLY byte count overflow"))?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(capacity)
        .map_err(|_| invalid("failed to reserve formal cut PLY bytes"))?;
    output.extend_from_slice(header.as_bytes());
    for (index, drawable) in drawables.iter().copied().enumerate() {
        let fields = encoded_fields(drawable, index)?;
        for value in fields {
            output.extend_from_slice(&value.to_bits().to_le_bytes());
        }
    }
    if output.len() != capacity {
        return Err(invalid("formal cut PLY encoder produced an invalid length"));
    }
    Ok(output)
}

fn verify_payload(bytes: &[u8], expected: &[DrawableGaussian]) -> Result<()> {
    let canonical = encode(expected)?;
    if bytes != canonical {
        return Err(invalid(
            "formal cut PLY bytes are not the deterministic canonical encoding",
        ));
    }
    let header_len = canonical_header(expected.len()).len();
    let max_scene_bytes = expected
        .len()
        .checked_mul(512)
        .ok_or_else(|| invalid("formal cut PLY decoded budget overflow"))?;
    let limits = PlyLoadLimits {
        max_input_bytes: bytes.len(),
        max_header_bytes: header_len,
        max_vertices: expected.len(),
        max_vertex_properties: BODY_FLOATS_PER_SPLAT,
        max_scene_bytes,
    };
    let mut visited = 0_usize;
    let mut mismatch = None;
    let summary = visit_ply_bytes_splats_with_limits(bytes, limits, |decoded| {
        let index = visited;
        visited += 1;
        if mismatch.is_none() {
            let Some(expected) = expected.get(index) else {
                mismatch = Some(format!("decoded unexpected splat {index}"));
                return;
            };
            if let Err(error) = verify_decoded(expected, decoded, index) {
                mismatch = Some(error);
            }
        }
    })?;
    if let Some(mismatch) = mismatch {
        return Err(invalid(mismatch));
    }
    if summary.gaussians != expected.len()
        || visited != expected.len()
        || summary.sh_degree != 3
        || !summary.has_sh_rest
    {
        return Err(invalid(format!(
            "formal cut PLY readback summary mismatch: expected P={}, visited={}, summary P={}, SH={}, has_rest={}",
            expected.len(),
            visited,
            summary.gaussians,
            summary.sh_degree,
            summary.has_sh_rest
        )));
    }
    Ok(())
}

fn encoded_fields(
    drawable: DrawableGaussian,
    index: usize,
) -> Result<[f32; BODY_FLOATS_PER_SPLAT]> {
    if !drawable.is_finite_and_drawable() || drawable.sh_degree != 3 {
        return Err(invalid(format!(
            "formal cut splat {index} is not finite drawable SH3"
        )));
    }
    let [scale_x, scale_y, scale_z] = drawable.scale;
    let log_scale = [
        roundtripping_parameter(scale_x, scale_x.ln(), f32::exp)?,
        roundtripping_parameter(scale_y, scale_y.ln(), f32::exp)?,
        roundtripping_parameter(scale_z, scale_z.ln(), f32::exp)?,
    ];
    let opacity_logit = opacity_logit(drawable.opacity)?;
    let mut fields = [0.0_f32; BODY_FLOATS_PER_SPLAT];
    let mut cursor = 0;
    for value in [
        drawable.position[0],
        -drawable.position[1],
        drawable.position[2],
    ] {
        fields[cursor] = value;
        cursor += 1;
    }
    for value in drawable.sh_dc {
        fields[cursor] = value;
        cursor += 1;
    }
    fields[cursor] = opacity_logit;
    cursor += 1;
    for value in log_scale {
        fields[cursor] = value;
        cursor += 1;
    }
    // The loader interprets rot_0..3 as PLY wxyz, reorders them to xyzw,
    // then flips runtime x/z for RDF -> RUF. This is the exact inverse.
    for value in [
        drawable.rotation_xyzw[3],
        -drawable.rotation_xyzw[0],
        drawable.rotation_xyzw[1],
        -drawable.rotation_xyzw[2],
    ] {
        fields[cursor] = value;
        cursor += 1;
    }
    for (coefficient, value) in drawable.sh_rest.into_iter().enumerate() {
        fields[cursor] = value * SH_FLIP_RUF_TO_RDF[coefficient % 15];
        cursor += 1;
    }
    debug_assert_eq!(cursor, BODY_FLOATS_PER_SPLAT);
    Ok(fields)
}

fn verify_decoded(
    expected: &DrawableGaussian,
    decoded: &gsplat_io_ply::DecodedPlySplat,
    index: usize,
) -> std::result::Result<(), String> {
    let position = [
        decoded.position_ruf.x,
        decoded.position_ruf.y,
        decoded.position_ruf.z,
    ];
    if !bits_equal(position, expected.position) {
        return Err(format!(
            "formal cut PLY splat {index} failed RDF/RUF position roundtrip"
        ));
    }
    if !bits_equal(decoded.rotation_xyzw, expected.rotation_xyzw) {
        return Err(format!(
            "formal cut PLY splat {index} failed wxyz/xyzw rotation roundtrip"
        ));
    }
    if !bits_equal(decoded.color_dc, expected.sh_dc)
        || !bits_equal(decoded.sh_rest, expected.sh_rest)
        || decoded.sh_degree != 3
        || decoded.sh_rest_len != SH3_REST_COMPONENTS as u8
    {
        return Err(format!(
            "formal cut PLY splat {index} failed complete SH3 inverse-sign roundtrip"
        ));
    }
    if !decoded.log_scale_xyz.into_iter().all(f32::is_finite)
        || !within_ulps(
            decoded.log_scale_xyz.map(f32::exp),
            expected.scale,
            MAX_NONLINEAR_ROUNDTRIP_ULPS,
        )
    {
        return Err(format!(
            "formal cut PLY splat {index} failed finite ln(scale) runtime roundtrip"
        ));
    }
    let opacity = 1.0 / (1.0 + (-decoded.opacity_logit).exp());
    if !decoded.opacity_logit.is_finite()
        || ulp_distance(opacity, expected.opacity) > MAX_NONLINEAR_ROUNDTRIP_ULPS
    {
        return Err(format!(
            "formal cut PLY splat {index} failed finite opacity-logit runtime roundtrip"
        ));
    }
    Ok(())
}

fn opacity_logit(opacity: f32) -> Result<f32> {
    let initial = if opacity.to_bits() == 0.0_f32.to_bits() {
        ZERO_OPACITY_LOGIT
    } else if opacity.to_bits() == 1.0_f32.to_bits() {
        ONE_OPACITY_LOGIT
    } else {
        (opacity / (1.0 - opacity)).ln()
    };
    roundtripping_parameter(opacity, initial, |logit| 1.0 / (1.0 + (-logit).exp()))
}

fn roundtripping_parameter(target: f32, initial: f32, decode: impl Fn(f32) -> f32) -> Result<f32> {
    if initial.is_finite() && ulp_distance(decode(initial), target) <= MAX_NONLINEAR_ROUNDTRIP_ULPS
    {
        return Ok(initial);
    }
    let mut lower = initial;
    let mut upper = initial;
    for _ in 0..MAX_PARAMETER_NEIGHBORS {
        lower = next_down(lower);
        if lower.is_finite() && ulp_distance(decode(lower), target) <= MAX_NONLINEAR_ROUNDTRIP_ULPS
        {
            return Ok(lower);
        }
        upper = next_up(upper);
        if upper.is_finite() && ulp_distance(decode(upper), target) <= MAX_NONLINEAR_ROUNDTRIP_ULPS
        {
            return Ok(upper);
        }
    }
    Err(invalid(format!(
        "no finite PLY parameter round-trips runtime f32 value 0x{:08x} within {MAX_NONLINEAR_ROUNDTRIP_ULPS} ULP",
        target.to_bits(),
    )))
}

fn next_up(value: f32) -> f32 {
    if value.is_nan() || value == f32::INFINITY {
        return value;
    }
    if value == 0.0 {
        return f32::from_bits(1);
    }
    let bits = value.to_bits();
    f32::from_bits(if value > 0.0 { bits + 1 } else { bits - 1 })
}

fn next_down(value: f32) -> f32 {
    if value.is_nan() || value == f32::NEG_INFINITY {
        return value;
    }
    if value == 0.0 {
        return f32::from_bits(0x8000_0001);
    }
    let bits = value.to_bits();
    f32::from_bits(if value > 0.0 { bits - 1 } else { bits + 1 })
}

fn bits_equal<const N: usize>(left: [f32; N], right: [f32; N]) -> bool {
    left.into_iter()
        .map(f32::to_bits)
        .eq(right.into_iter().map(f32::to_bits))
}

fn within_ulps<const N: usize>(left: [f32; N], right: [f32; N], max_ulps: u32) -> bool {
    left.into_iter()
        .zip(right)
        .all(|(left, right)| ulp_distance(left, right) <= max_ulps)
}

fn ulp_distance(left: f32, right: f32) -> u32 {
    fn ordered(bits: u32) -> u32 {
        if bits & 0x8000_0000 == 0 {
            bits | 0x8000_0000
        } else {
            !bits
        }
    }
    ordered(left.to_bits()).abs_diff(ordered(right.to_bits()))
}

fn canonical_header(count: usize) -> String {
    let mut header = format!(
        "ply\nformat binary_little_endian 1.0\nelement vertex {count}\nproperty float x\nproperty float y\nproperty float z\nproperty float f_dc_0\nproperty float f_dc_1\nproperty float f_dc_2\nproperty float opacity\nproperty float scale_0\nproperty float scale_1\nproperty float scale_2\nproperty float rot_0\nproperty float rot_1\nproperty float rot_2\nproperty float rot_3\n"
    );
    for index in 0..SH3_REST_COMPONENTS {
        header.push_str(&format!("property float f_rest_{index}\n"));
    }
    header.push_str("end_header\n");
    header
}

fn hash_bytes(bytes: &[u8]) -> String {
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in digest {
        output.push(HEX[usize::from(byte >> 4)] as char);
        output.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_drawable() -> DrawableGaussian {
        let mut sh_rest = [0.0; SH3_REST_COMPONENTS];
        for (index, coefficient) in sh_rest.iter_mut().enumerate() {
            *coefficient = (index as f32 - 22.0) * 0.03125;
        }
        DrawableGaussian {
            position: [1.25, -2.5, 3.75],
            scale: [0.125, 1.5, 9.25],
            rotation_xyzw: [0.25, -0.5, 0.125, 0.819_679_8],
            opacity: 0.8125,
            sh_dc: [-0.75, 0.5, 1.25],
            sh_degree: 3,
            sh_rest,
        }
    }

    #[test]
    fn deterministic_binary_sh3_roundtrip_inverts_every_runtime_transform() {
        let mut opaque = fixture_drawable();
        opaque.position[1] = -0.0;
        opaque.opacity = 1.0;
        let mut transparent = fixture_drawable();
        transparent.opacity = 0.0;
        let drawables = [fixture_drawable(), opaque, transparent];

        let one = encode_verified(&drawables).expect("encode first cut PLY");
        let two = encode_verified(&drawables).expect("encode second cut PLY");
        assert_eq!(one, two);
        assert!(one.starts_with(b"ply\nformat binary_little_endian 1.0\n"));
        assert!(
            one.windows(20)
                .any(|window| window == b"property float rot_0")
        );
    }

    #[test]
    fn canonical_verifier_rejects_truncation_trailing_bytes_and_body_drift() {
        let drawables = [fixture_drawable()];
        let bytes = encode_verified(&drawables).expect("encode cut PLY");
        let mut truncated = bytes.clone();
        truncated.pop();
        assert!(verify_payload(&truncated, &drawables).is_err());
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(verify_payload(&trailing, &drawables).is_err());
        let mut drifted = bytes;
        *drifted.last_mut().expect("body byte") ^= 1;
        assert!(verify_payload(&drifted, &drawables).is_err());
    }

    #[test]
    fn encoder_rejects_non_sh3_and_non_drawable_values() {
        let mut wrong_degree = fixture_drawable();
        wrong_degree.sh_degree = 2;
        assert!(encode_verified(&[wrong_degree]).is_err());
        let mut non_finite = fixture_drawable();
        non_finite.scale[0] = f32::NAN;
        assert!(encode_verified(&[non_finite]).is_err());
    }
}
