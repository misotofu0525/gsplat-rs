//! SPZ-aligned quantized resident packing. CPU-side only; shaders dequant in compute.

use bytemuck::{Pod, Zeroable};
use gsplat_core::SceneBuffers;

use crate::math::quat_normalize;

const COLOR_SCALE: f32 = 0.15;
const SQRT_ONE_HALF: f32 = std::f32::consts::FRAC_1_SQRT_2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResidentStorageProfile {
    #[default]
    FullF32,
    Quantized,
}

impl ResidentStorageProfile {
    /// Stable CLI/log token. Not part of the C ABI.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FullF32 => "full-f32",
            Self::Quantized => "quantized",
        }
    }
}

/// Compute storage buffers used by the quantized preprocess: order, source,
/// projected, and four per-degree SH sidecars.
pub(crate) const QUANTIZED_STORAGE_BUFFERS_PER_STAGE: u32 = 7;
/// WebGPU default; requested when the adapter exposes at least this many.
pub(crate) const REQUESTED_STORAGE_BUFFERS_PER_STAGE: u32 = 8;

pub(crate) fn apply_storage_buffer_stage_headroom(
    required: &mut wgpu::Limits,
    adapter: &wgpu::Limits,
) {
    let requested =
        REQUESTED_STORAGE_BUFFERS_PER_STAGE.min(adapter.max_storage_buffers_per_shader_stage);
    required.max_storage_buffers_per_shader_stage =
        required.max_storage_buffers_per_shader_stage.max(requested);
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct GpuQuantizedSource {
    pub(crate) pos_xy: u32,
    pub(crate) pos_z_alpha: u32,
    pub(crate) rotation: u32,
    pub(crate) scale_rgb: u32,
    pub(crate) color_dc: u32,
    pub(crate) _pad: [u32; 3],
}

pub(crate) const QUANTIZED_SOURCE_STRIDE: u64 = std::mem::size_of::<GpuQuantizedSource>() as u64;

pub(crate) fn quantized_sh_degree_coeff_count(degree: u8) -> u64 {
    u64::from(degree) * 2 + 1
}

pub(crate) fn quantized_sh_sidecar_bytes_per_splat(degree: u8) -> u64 {
    quantized_sh_degree_coeff_count(degree) * 3
}

pub(crate) fn quantized_sh_first_coeff(degree: u8) -> u64 {
    u64::from(degree) * u64::from(degree) - 1
}

pub(crate) fn quantized_sh_sidecar_buffer_bytes(
    splat_count: u64,
    sidecar_degree: u8,
    scene_degree: u8,
) -> Option<u64> {
    if sidecar_degree == 0 || scene_degree < sidecar_degree {
        return Some(4);
    }
    splat_count
        .max(1)
        .checked_mul(quantized_sh_sidecar_bytes_per_splat(sidecar_degree))
        .map(|bytes| bytes.div_ceil(4) * 4)
}

pub(crate) fn quantized_sh_max_sidecar_bytes(splat_count: u64, scene_degree: u8) -> Option<u64> {
    if scene_degree == 0 {
        return Some(4);
    }
    let mut max_bytes = 4_u64;
    for degree in 1..=scene_degree.min(4) {
        max_bytes = max_bytes.max(quantized_sh_sidecar_buffer_bytes(
            splat_count,
            degree,
            scene_degree,
        )?);
    }
    Some(max_bytes)
}

pub(crate) fn quantized_sh_max_sidecar_stride(scene_degree: u8) -> u64 {
    if scene_degree == 0 {
        u64::MAX
    } else {
        quantized_sh_sidecar_bytes_per_splat(scene_degree.min(4))
    }
}

pub(crate) fn pack_quantized_sources(
    scene: &SceneBuffers,
    alpha_values: &[f32],
) -> Vec<GpuQuantizedSource> {
    if scene.positions.is_empty() {
        return vec![GpuQuantizedSource::zeroed()];
    }
    (0..scene.positions.len())
        .map(|i| {
            let p = scene.positions[i];
            let alpha = alpha_values.get(i).copied().unwrap_or(0.0);
            let scale = scene.scale_xyz.get(i).copied().unwrap_or([0.0; 3]);
            let rotation = scene
                .rotation_xyzw
                .get(i)
                .copied()
                .unwrap_or([0.0, 0.0, 0.0, 1.0]);
            let dc = scene.color_dc.get(i).copied().unwrap_or([0.0; 3]);
            GpuQuantizedSource {
                pos_xy: pack2x16float(p.x, p.y),
                pos_z_alpha: pack2x16float(p.z, alpha.clamp(0.0, 1.0)),
                rotation: encode_smallest_three(quat_normalize(rotation)),
                scale_rgb: pack_u8x4(
                    quantize_log_scale(scale[0]),
                    quantize_log_scale(scale[1]),
                    quantize_log_scale(scale[2]),
                    0,
                ),
                color_dc: pack_u8x4(
                    quantize_color_dc(dc[0]),
                    quantize_color_dc(dc[1]),
                    quantize_color_dc(dc[2]),
                    0,
                ),
                _pad: [0; 3],
            }
        })
        .collect()
}

pub(crate) fn pack_quantized_sh_sidecar(scene: &SceneBuffers, sidecar_degree: u8) -> Vec<u32> {
    let total_bytes = quantized_sh_sidecar_buffer_bytes(
        scene.len().max(1) as u64,
        sidecar_degree,
        scene.sh_degree,
    )
    .unwrap_or(4) as usize;
    let mut out = vec![0_u32; (total_bytes / 4).max(1)];
    if sidecar_degree == 0 || scene.sh_degree < sidecar_degree {
        return out;
    }
    let Some(rest) = scene.sh_rest.as_deref() else {
        return out;
    };
    let rest_coeffs = ((u64::from(scene.sh_degree) + 1).pow(2) - 1) as usize;
    let first = quantized_sh_first_coeff(sidecar_degree) as usize;
    let count = quantized_sh_degree_coeff_count(sidecar_degree) as usize;
    for i in 0..scene.len() {
        for channel in 0..3 {
            let src_base = i * rest_coeffs * 3 + channel * rest_coeffs;
            let dst_base = i * count * 3 + channel * count;
            for local in 0..count {
                let packed = quantize_sh(rest[src_base + first + local]);
                let byte_index = dst_base + local;
                let word = byte_index / 4;
                let shift = (byte_index % 4) * 8;
                out[word] |= u32::from(packed) << shift;
            }
        }
    }
    out
}

pub(crate) fn pack2x16float(a: f32, b: f32) -> u32 {
    u32::from(f32_to_f16_bits(a)) | (u32::from(f32_to_f16_bits(b)) << 16)
}

fn pack_u8x4(a: u8, b: u8, c: u8, d: u8) -> u32 {
    u32::from(a) | (u32::from(b) << 8) | (u32::from(c) << 16) | (u32::from(d) << 24)
}

pub(crate) fn quantize_log_scale(log_scale: f32) -> u8 {
    ((log_scale + 10.0) * 16.0).round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
pub(crate) fn dequantize_log_scale(packed: u8) -> f32 {
    f32::from(packed) / 16.0 - 10.0
}

pub(crate) fn quantize_color_dc(value: f32) -> u8 {
    ((value * COLOR_SCALE + 0.5) * 255.0)
        .round()
        .clamp(0.0, 255.0) as u8
}

#[cfg(test)]
pub(crate) fn dequantize_color_dc(packed: u8) -> f32 {
    (f32::from(packed) / 255.0 - 0.5) / COLOR_SCALE
}

pub(crate) fn quantize_sh(value: f32) -> u8 {
    (value * 128.0 + 128.0).round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
pub(crate) fn dequantize_sh(packed: u8) -> f32 {
    (f32::from(packed) - 128.0) / 128.0
}

pub(crate) fn encode_smallest_three(q: [f32; 4]) -> u32 {
    let mut largest = 0_usize;
    let mut largest_abs = q[0].abs();
    for (index, component) in q.iter().enumerate().skip(1) {
        let abs = component.abs();
        if abs > largest_abs {
            largest = index;
            largest_abs = abs;
        }
    }
    let mut packed = (largest as u32) << 30;
    let mut shift = 0_u32;
    for index in (0..4).rev() {
        if index == largest {
            continue;
        }
        packed |= encode_smallest_component(q[index]) << shift;
        shift += 10;
    }
    packed
}

#[cfg(test)]
pub(crate) fn decode_smallest_three(mut packed: u32) -> [f32; 4] {
    let largest = (packed >> 30) as usize;
    let mut rotation = [0.0_f32; 4];
    let mut sum_squares = 0.0_f32;
    for index in (0..4).rev() {
        if index == largest {
            continue;
        }
        let magnitude = packed & 0x1ff;
        let negative = (packed >> 9) & 1 != 0;
        packed >>= 10;
        let value = SQRT_ONE_HALF * magnitude as f32 / 511.0;
        rotation[index] = if negative { -value } else { value };
        sum_squares += value * value;
    }
    rotation[largest] = (1.0 - sum_squares).max(0.0).sqrt();
    rotation
}

fn encode_smallest_component(value: f32) -> u32 {
    let mag = (value.abs() / SQRT_ONE_HALF * 511.0)
        .round()
        .clamp(0.0, 511.0) as u32;
    let sign = u32::from(value < 0.0) << 9;
    mag | sign
}

pub(crate) fn f32_to_f16_bits(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let mut exp = ((bits >> 23) & 0xff) as i32 - 127;
    let mut mant = bits & 0x7f_ffff;

    if exp == 128 {
        if mant == 0 {
            return sign | 0x7c00;
        }
        return sign | 0x7c00 | ((mant >> 13) as u16) | 0x0200;
    }
    if exp > 15 {
        return sign | 0x7c00;
    }
    if exp < -14 {
        let shift = (-14 - exp) as u32 + 13;
        if shift >= 24 {
            return sign;
        }
        mant |= 0x80_0000;
        let rounded = (mant + (1 << (shift.saturating_sub(1)))) >> shift;
        return sign | rounded as u16;
    }
    exp += 15;
    mant += 0x1000;
    if mant >= 0x80_0000 {
        mant = 0;
        exp += 1;
        if exp >= 31 {
            return sign | 0x7c00;
        }
    }
    sign | ((exp as u16) << 10) | ((mant >> 13) as u16)
}

#[cfg(test)]
pub(crate) fn f16_bits_to_f32(bits: u16) -> f32 {
    let sign = u32::from(bits & 0x8000) << 16;
    let exp = (bits >> 10) & 0x1f;
    let mant = u32::from(bits & 0x03ff);
    let value = if exp == 0 {
        if mant == 0 {
            sign
        } else {
            let mut m = mant;
            let mut e = 1_i32;
            while m & 0x0400 == 0 {
                m <<= 1;
                e -= 1;
            }
            m &= 0x03ff;
            (((e + 127 - 1) as u32) << 23) | (m << 13) | sign
        }
    } else if exp == 31 {
        if mant == 0 {
            sign | 0x7f80_0000
        } else {
            sign | 0x7fc0_0000 | (mant << 13)
        }
    } else {
        (((exp as u32 + 127 - 15) << 23) | (mant << 13)) | sign
    };
    f32::from_bits(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gsplat_core::{SceneBuffers, Vec3f};

    #[test]
    fn quantized_hot_record_is_32_bytes() {
        assert_eq!(QUANTIZED_SOURCE_STRIDE, 32);
        assert_eq!(std::mem::size_of::<GpuQuantizedSource>(), 32);
    }

    #[test]
    fn smallest_three_roundtrips_unit_quaternion() {
        let q = quat_normalize([0.1, -0.2, 0.3, 0.9]);
        let decoded = decode_smallest_three(encode_smallest_three(q));
        for i in 0..4 {
            assert!((q[i] - decoded[i]).abs() < 2.0e-3, "{q:?} vs {decoded:?}");
        }
    }

    #[test]
    fn spz_scale_and_sh_match_documented_maps() {
        assert_eq!(quantize_log_scale(-10.0), 0);
        assert!((dequantize_log_scale(160) - 0.0).abs() < 1e-6);
        assert_eq!(quantize_sh(0.0), 128);
        assert!((dequantize_sh(128)).abs() < 1e-6);
        let mid = dequantize_color_dc(128);
        assert_eq!(quantize_color_dc(mid), 128);
    }

    #[test]
    fn f16_roundtrip_keeps_splat_scale_positions() {
        for value in [0.0, 1.0, -2.5, 12.75, 0.001_f32] {
            let back = f16_bits_to_f32(f32_to_f16_bits(value));
            let rel = (back - value).abs() / value.abs().max(1.0);
            assert!(rel < 5e-4, "{value} -> {back}");
        }
    }

    #[test]
    fn packed_sh_sidecar_uses_u32_words() {
        let scene = SceneBuffers {
            positions: vec![Vec3f::new(1.0, 2.0, 3.0)],
            opacity: vec![0.0],
            scale_xyz: vec![[0.0; 3]],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]],
            color_dc: vec![[0.0; 3]],
            sh_degree: 1,
            sh_rest: Some(vec![0.25, -0.5, 0.0, 0.1, 0.2, 0.3, -0.1, 0.0, 0.4]),
        };
        scene.validate().unwrap();
        let packed = pack_quantized_sh_sidecar(&scene, 1);
        assert_eq!(quantized_sh_sidecar_bytes_per_splat(1), 9);
        assert_eq!(quantized_sh_sidecar_buffer_bytes(1, 1, 1), Some(12));
        assert_eq!(packed.len(), 3);
        assert_eq!(
            dequantize_sh((packed[0] & 0xff) as u8),
            dequantize_sh(quantize_sh(0.25))
        );
        assert_eq!(pack_quantized_sh_sidecar(&scene, 2).len(), 1);
    }

    #[test]
    fn degree_three_sidecars_split_channel_major_rest() {
        let mut rest = vec![0.0_f32; 45];
        rest[8] = 0.5;
        rest[15 + 8] = -0.25;
        rest[30 + 8] = 0.125;
        let scene = SceneBuffers {
            positions: vec![Vec3f::new(0.0, 0.0, 1.0)],
            opacity: vec![0.0],
            scale_xyz: vec![[0.0; 3]],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]],
            color_dc: vec![[0.0; 3]],
            sh_degree: 3,
            sh_rest: Some(rest),
        };
        scene.validate().unwrap();
        let sh3 = pack_quantized_sh_sidecar(&scene, 3);
        assert_eq!(quantized_sh_sidecar_bytes_per_splat(3), 21);
        assert_eq!(quantized_sh_max_sidecar_bytes(1, 3), Some(24));
        assert_eq!(
            dequantize_sh((sh3[0] & 0xff) as u8),
            dequantize_sh(quantize_sh(0.5))
        );
        assert_eq!(
            dequantize_sh(((sh3[1] >> 24) & 0xff) as u8),
            dequantize_sh(quantize_sh(-0.25))
        );
    }

    #[test]
    fn million_degree_three_quantized_bytes_fit_128_mib_bindings() {
        let n = 1_000_000_u64;
        let source = n * QUANTIZED_SOURCE_STRIDE;
        let sh = quantized_sh_max_sidecar_bytes(n, 3).unwrap();
        let projected = n * 48;
        let limit = 128 * 1024 * 1024_u64;
        assert_eq!(sh, 21_000_000);
        assert!(source < limit);
        assert!(sh < limit);
        assert!(projected < limit);
    }
}
