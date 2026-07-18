//! Compact packed-atlas encoding shared by diagnostic Packed and paged slots.
//! The 20-byte hot record rebuilds covariance from quantized scale/rotation;
//! optional degree-3 SH sidecars add at most 48 bytes per splat.

use gsplat_core::{SceneBuffers, Vec3f};

/// Bytes in the Phase B hot render record (position/opacity + scale/flags +
/// rotation + color).
///
/// The storage-buffer draw path rebuilds world covariance per vertex from
/// scene-adaptive scale + rotation.
pub const HOT_RECORD_BYTES: usize = 20;
/// Maximum degree-3 SH sidecar bytes before order/scratch.
pub const DEGREE3_SIDECAR_BYTES: usize = 48;
/// Full degree-3 attribute budget before the 4-byte order ID.
pub const FULL_DEGREE3_ATTRIBUTE_BYTES: usize = HOT_RECORD_BYTES + DEGREE3_SIDECAR_BYTES;
/// Current direct path attribute payload used for reduction claims.
pub const DIRECT_DEGREE3_ATTRIBUTE_BYTES: usize = 64 + 180;

/// Candidate texture formats for the hot record streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotStream {
    PositionOpacity,
    /// Three u10 log scales plus two format/LOD flag bits.
    ScaleFlags,
    /// Smallest-three-packed quaternion.
    Rotation,
    Color,
}

impl HotStream {
    pub const fn bytes_per_splat(self) -> usize {
        match self {
            Self::PositionOpacity => 8,
            Self::ScaleFlags => 4,
            Self::Rotation => 4,
            Self::Color => 4,
        }
    }

    pub const fn wgpu_format(self) -> wgpu::TextureFormat {
        match self {
            Self::PositionOpacity => wgpu::TextureFormat::Rgba16Uint,
            Self::ScaleFlags | Self::Rotation => wgpu::TextureFormat::R32Uint,
            Self::Color => wgpu::TextureFormat::R32Uint,
        }
    }
}

/// Axis-aligned bounds used to encode page-local (here: scene-local) positions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneBounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl SceneBounds {
    pub fn from_positions(positions: &[Vec3f]) -> Self {
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for position in positions {
            min[0] = min[0].min(position.x);
            min[1] = min[1].min(position.y);
            min[2] = min[2].min(position.z);
            max[0] = max[0].max(position.x);
            max[1] = max[1].max(position.y);
            max[2] = max[2].max(position.z);
        }
        for axis in 0..3 {
            if !min[axis].is_finite()
                || !max[axis].is_finite()
                || (max[axis] - min[axis]).abs() < 1e-6
            {
                min[axis] = 0.0;
                max[axis] = 1.0;
            }
        }
        Self { min, max }
    }

    pub fn encode_u16(&self, position: Vec3f) -> [u16; 3] {
        let mut out = [0_u16; 3];
        let values = [position.x, position.y, position.z];
        for axis in 0..3 {
            let span = (self.max[axis] - self.min[axis]).max(1e-6);
            let t = ((values[axis] - self.min[axis]) / span).clamp(0.0, 1.0);
            out[axis] = (t * 65535.0).round() as u16;
        }
        out
    }

    pub fn decode_u16(&self, encoded: [u16; 3]) -> Vec3f {
        let mut values = [0.0_f32; 3];
        for axis in 0..3 {
            let span = (self.max[axis] - self.min[axis]).max(1e-6);
            values[axis] = self.min[axis] + (f32::from(encoded[axis]) / 65535.0) * span;
        }
        Vec3f {
            x: values[0],
            y: values[1],
            z: values[2],
        }
    }
}

/// Packed hot-record payloads for one splat.
///
/// Layout (20 bytes):
/// - `position_opacity`: 8 B — bounds-relative xyz + opacity/flags
/// - `scale_flags`: 4 B — three u10 log scales + two flags
/// - `rotation`: 4 B — smallest-three-packed quaternion
/// - `color`: 4 B — view-evaluated RGB10 from color-refresh
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackedHotRecord {
    pub position_opacity: [u16; 4],
    pub scale_flags: u32,
    pub rotation: u32,
    pub color: u32,
}

/// Number of `u32` words in one tightly packed hot storage record.
pub const HOT_RECORD_U32_WORDS: usize = HOT_RECORD_BYTES / 4;

impl PackedHotRecord {
    pub const fn byte_len() -> usize {
        HOT_RECORD_BYTES
    }

    pub fn to_storage_words(self) -> [u32; HOT_RECORD_U32_WORDS] {
        let po0 = u32::from(self.position_opacity[0]) | (u32::from(self.position_opacity[1]) << 16);
        let po1 = u32::from(self.position_opacity[2]) | (u32::from(self.position_opacity[3]) << 16);
        [po0, po1, self.scale_flags, self.rotation, self.color]
    }
}

/// Quantized degree-3 SH sidecar (45 i8 coeffs + 3 pad).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PackedShSidecar {
    pub coeffs_i8: [i8; 45],
    pub pad: [i8; 3],
}

impl PackedShSidecar {
    pub const fn byte_len() -> usize {
        DEGREE3_SIDECAR_BYTES
    }

    pub fn as_texel_bytes(&self) -> [u8; DEGREE3_SIDECAR_BYTES] {
        let mut out = [0_u8; DEGREE3_SIDECAR_BYTES];
        for (index, value) in self.coeffs_i8.iter().enumerate() {
            out[index] = *value as u8;
        }
        for (index, value) in self.pad.iter().enumerate() {
            out[45 + index] = *value as u8;
        }
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LogScaleRange {
    pub min: f32,
    pub max: f32,
}

impl LogScaleRange {
    pub fn from_scales(scales: &[[f32; 3]]) -> Self {
        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;
        for scale in scales {
            for value in scale {
                if value.is_finite() {
                    min = min.min(*value);
                    max = max.max(*value);
                }
            }
        }
        if !min.is_finite() || !max.is_finite() {
            return Self {
                min: -8.0,
                max: 8.0,
            };
        }
        if (max - min).abs() < 1e-6 {
            return Self {
                min,
                max: min + 1e-3,
            };
        }
        Self { min, max }
    }

    pub fn encode_u10(self, log_scale: f32) -> u32 {
        let t = ((log_scale - self.min) / (self.max - self.min).max(1e-6)).clamp(0.0, 1.0);
        (t * 1023.0).round() as u32
    }

    pub fn decode_u10(self, encoded: u32) -> f32 {
        self.min + ((encoded & 0x3ff) as f32 / 1023.0) * (self.max - self.min)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PackedSceneCpu {
    pub bounds: SceneBounds,
    pub log_scale_range: LogScaleRange,
    pub hot: Vec<PackedHotRecord>,
    pub sh_sidecars: Vec<PackedShSidecar>,
    /// Scene/page-wide dequant scales applied per RGB channel.
    pub sh_scales: [f32; 3],
    pub sh_degree: u8,
    pub splat_count: usize,
}

impl PackedSceneCpu {
    pub fn attribute_bytes_per_splat(&self) -> usize {
        if self.sh_degree >= 3 {
            FULL_DEGREE3_ATTRIBUTE_BYTES
        } else if self.sh_degree == 0 {
            HOT_RECORD_BYTES
        } else {
            // Conservative upper bound until degree-1/2 layouts are specialized.
            FULL_DEGREE3_ATTRIBUTE_BYTES
        }
    }

    pub fn total_attribute_bytes(&self) -> u64 {
        (self.splat_count as u64) * (self.attribute_bytes_per_splat() as u64)
    }

    pub fn direct_degree3_attribute_bytes(splat_count: usize) -> u64 {
        (splat_count as u64) * (DIRECT_DEGREE3_ATTRIBUTE_BYTES as u64)
    }

    pub fn degree3_reduction_factor(&self) -> f64 {
        let packed = self.total_attribute_bytes().max(1) as f64;
        Self::direct_degree3_attribute_bytes(self.splat_count).max(1) as f64 / packed
    }
}

/// Atlas width chosen so `width * height >= splat_count` with a power-of-two width.
pub fn atlas_dimensions(splat_count: usize) -> (u32, u32) {
    let count = splat_count.max(1) as u32;
    let width = count.next_power_of_two().clamp(1, 2048);
    let height = count.div_ceil(width).max(1);
    (width, height)
}

pub fn slot_to_texel(slot: u32, width: u32) -> (u32, u32) {
    (slot % width, slot / width)
}

/// SH sidecar atlas size: 12 RGBA8Uint texels (= 48 bytes) per splat.
pub fn sh_sidecar_atlas_dimensions(splat_count: usize) -> (u32, u32) {
    let total_texels = (splat_count.max(1) * 12) as u32;
    let width = 2048_u32.min(total_texels.next_power_of_two().max(12));
    let height = total_texels.div_ceil(width).max(1);
    (width, height)
}

pub fn decode_opacity_u8(encoded: u8) -> f32 {
    let alpha = f32::from(encoded) / 255.0;
    // Inverse sigmoid; clamp away from 0/1 to keep finite logits.
    let alpha = alpha.clamp(1e-4, 1.0 - 1e-4);
    (alpha / (1.0 - alpha)).ln()
}

fn encode_opacity_u8(opacity: f32) -> u8 {
    let alpha = (1.0 / (1.0 + (-opacity).exp())).clamp(0.0, 1.0);
    (alpha * 255.0).round() as u8
}

/// Smallest-three quaternion packing into 32 bits (10+10+10+2).
pub fn pack_quat_smallest_three(q: [f32; 4]) -> u32 {
    let mut q = quat_normalize(q);
    let mut max_index = 0_usize;
    let mut max_abs = q[0].abs();
    for (index, value) in q.iter().enumerate().skip(1) {
        let abs = value.abs();
        if abs > max_abs {
            max_abs = abs;
            max_index = index;
        }
    }
    // Force the omitted (largest) component positive so reconstruction via +sqrt
    // recovers an orientation-equivalent quaternion.
    if q[max_index] < 0.0 {
        q = [-q[0], -q[1], -q[2], -q[3]];
    }
    let mut components = [0.0_f32; 3];
    let mut write = 0;
    for (index, value) in q.iter().enumerate() {
        if index == max_index {
            continue;
        }
        components[write] = *value;
        write += 1;
    }
    // Map [-1/sqrt(2), 1/sqrt(2)] roughly into 10-bit unsigned.
    let scale = std::f32::consts::FRAC_1_SQRT_2;
    let mut packed = (max_index as u32) & 0b11;
    for (lane, value) in components.iter().enumerate() {
        let t = ((*value / scale) * 0.5 + 0.5).clamp(0.0, 1.0);
        let bits = (t * 1023.0).round() as u32;
        packed |= (bits & 0x3FF) << (2 + 10 * lane);
    }
    packed
}

pub fn unpack_quat_smallest_three(packed: u32) -> [f32; 4] {
    let max_index = (packed & 0b11) as usize;
    let scale = std::f32::consts::FRAC_1_SQRT_2;
    let mut components = [0.0_f32; 3];
    for (lane, component) in components.iter_mut().enumerate() {
        let bits = (packed >> (2 + 10 * lane)) & 0x3FF;
        let t = bits as f32 / 1023.0;
        *component = (t * 2.0 - 1.0) * scale;
    }
    let mut q = [0.0_f32; 4];
    let mut read = 0;
    for (index, slot) in q.iter_mut().enumerate() {
        if index == max_index {
            continue;
        }
        *slot = components[read];
        read += 1;
    }
    let sum_sq = components[0] * components[0]
        + components[1] * components[1]
        + components[2] * components[2];
    q[max_index] = (1.0 - sum_sq).max(0.0).sqrt();
    quat_normalize(q)
}

pub fn pack_color_rgb10(rgb: [f32; 3]) -> u32 {
    let mut packed = 0_u32;
    for (lane, value) in rgb.iter().enumerate() {
        let bits = (value.clamp(0.0, 1.0) * 1023.0).round() as u32;
        packed |= (bits & 0x3FF) << (10 * lane);
    }
    packed
}

pub fn unpack_color_rgb10(packed: u32) -> [f32; 3] {
    let mut rgb = [0.0_f32; 3];
    for (lane, value) in rgb.iter_mut().enumerate() {
        let bits = (packed >> (10 * lane)) & 0x3FF;
        *value = bits as f32 / 1023.0;
    }
    rgb
}

/// Pack signed SH DC into 10-bit/channel over `[-max_abs, max_abs]`.
#[cfg(test)]
fn pack_signed_dc_rgb10(dc: [f32; 3], max_abs: f32) -> u32 {
    let range = max_abs.max(1e-6);
    let mut packed = 0_u32;
    for (lane, value) in dc.iter().enumerate() {
        let t = ((*value + range) / (2.0 * range)).clamp(0.0, 1.0);
        let bits = (t * 1023.0).round() as u32;
        packed |= (bits & 0x3FF) << (10 * lane);
    }
    packed
}

#[cfg(test)]
fn unpack_signed_dc_rgb10(packed: u32, max_abs: f32) -> [f32; 3] {
    let range = max_abs.max(1e-6);
    let mut dc = [0.0_f32; 3];
    for (lane, value) in dc.iter_mut().enumerate() {
        let bits = (packed >> (10 * lane)) & 0x3FF;
        let t = bits as f32 / 1023.0;
        *value = (t * 2.0 - 1.0) * range;
    }
    dc
}

fn quat_normalize(q: [f32; 4]) -> [f32; 4] {
    let len2 = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
    if len2 <= 1e-20 {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let inv = 1.0 / len2.sqrt();
    [q[0] * inv, q[1] * inv, q[2] * inv, q[3] * inv]
}

fn quantize_sh_rest_with_scales(
    rest: &[f32],
    coeffs_per_channel: usize,
    scales: [f32; 3],
) -> PackedShSidecar {
    let mut coeffs = [0_i8; 45];
    for (channel, scale) in scales.iter().enumerate() {
        let source_base = channel * coeffs_per_channel;
        let destination_base = channel * 15;
        let scale = scale.max(1e-6);
        for lane in 0..coeffs_per_channel.min(15) {
            let value = rest.get(source_base + lane).copied().unwrap_or(0.0);
            let q = (value / scale * 127.0).round().clamp(-127.0, 127.0) as i8;
            coeffs[destination_base + lane] = q;
        }
    }
    PackedShSidecar {
        coeffs_i8: coeffs,
        pad: [0; 3],
    }
}

pub fn dequantize_sh_rest(sidecar: &PackedShSidecar, scales: [f32; 3]) -> [f32; 45] {
    let mut out = [0.0_f32; 45];
    for (channel, scale) in scales.into_iter().enumerate() {
        let base = channel * 15;
        for lane in 0..15 {
            let index = base + lane;
            out[index] = f32::from(sidecar.coeffs_i8[index]) / 127.0 * scale;
        }
    }
    out
}

/// Pack an entire scene into the Phase B CPU-side atlas representation.
pub fn pack_scene(scene: &SceneBuffers) -> PackedSceneCpu {
    let bounds = SceneBounds::from_positions(&scene.positions);
    let log_scale_range = LogScaleRange::from_scales(&scene.scale_xyz);
    pack_scene_with_encoding(scene, bounds, log_scale_range, None)
}

/// Compute scene-wide SH quantization scales without building packed records.
pub fn scene_sh_scales(scene: &SceneBuffers) -> [f32; 3] {
    let rest = scene.sh_rest.as_deref().unwrap_or(&[]);
    let coeffs_per_channel = ((scene.sh_degree as usize + 1).pow(2))
        .saturating_sub(1)
        .min(15);
    let rest_stride = coeffs_per_channel * 3;
    let mut scales = [1e-6_f32; 3];
    if rest_stride == 0 {
        return scales;
    }

    for index in 0..scene.len() {
        let base = index * rest_stride;
        if base + rest_stride > rest.len() {
            continue;
        }
        for (channel, scale) in scales.iter_mut().enumerate() {
            let channel_base = base + channel * coeffs_per_channel;
            for lane in 0..coeffs_per_channel {
                *scale = scale.max(rest[channel_base + lane].abs());
            }
        }
    }
    scales
}

/// Pack a scene (or page subset) using shared encoding ranges.
///
/// Phase D multi-page GPU draw keeps one shader bounds/log-scale uniform, so
/// pages must encode positions/scales against the parent scene ranges rather
/// than page-local AABBs.
pub fn pack_scene_with_encoding(
    scene: &SceneBuffers,
    bounds: SceneBounds,
    log_scale_range: LogScaleRange,
    sh_scales: Option<[f32; 3]>,
) -> PackedSceneCpu {
    let mut hot = Vec::with_capacity(scene.len());
    let mut sh_sidecars = Vec::with_capacity(scene.len());
    let rest = scene.sh_rest.as_deref().unwrap_or(&[]);
    let coeffs_per_channel = ((scene.sh_degree as usize + 1).pow(2))
        .saturating_sub(1)
        .min(15);
    let rest_stride = coeffs_per_channel * 3;

    let scene_scales = sh_scales.unwrap_or_else(|| scene_sh_scales(scene));

    for index in 0..scene.len() {
        let position = scene.positions[index];
        let encoded_pos = bounds.encode_u16(position);
        let opacity = encode_opacity_u8(scene.opacity[index]);
        let opacity_flags = 0_u8;
        let scale = scene.scale_xyz[index];
        let scale_flags = log_scale_range.encode_u10(scale[0])
            | (log_scale_range.encode_u10(scale[1]) << 10)
            | (log_scale_range.encode_u10(scale[2]) << 20);
        let rotation = pack_quat_smallest_three(scene.rotation_xyzw[index]);
        // Initial hot color is degree-0 bake; view-dependent refresh overwrites
        // this from float SH before draw (design: color-refresh → hot RGB).
        const C0: f32 = 0.282_094_8_f32;
        let dc = scene.color_dc[index];
        let rgb = [
            (C0 * dc[0] + 0.5).clamp(0.0, 1.0),
            (C0 * dc[1] + 0.5).clamp(0.0, 1.0),
            (C0 * dc[2] + 0.5).clamp(0.0, 1.0),
        ];
        hot.push(PackedHotRecord {
            position_opacity: [
                encoded_pos[0],
                encoded_pos[1],
                encoded_pos[2],
                u16::from(opacity) | (u16::from(opacity_flags) << 8),
            ],
            scale_flags,
            rotation,
            color: pack_color_rgb10(rgb),
        });

        if rest_stride > 0 {
            let base = index * rest_stride;
            let slice = if base + rest_stride <= rest.len() {
                &rest[base..base + rest_stride]
            } else {
                &[]
            };
            sh_sidecars.push(quantize_sh_rest_with_scales(
                slice,
                coeffs_per_channel,
                scene_scales,
            ));
        } else {
            sh_sidecars.push(PackedShSidecar {
                coeffs_i8: [0; 45],
                pad: [0; 3],
            });
        }
    }

    PackedSceneCpu {
        bounds,
        log_scale_range,
        hot,
        sh_sidecars,
        sh_scales: scene_scales,
        sh_degree: scene.sh_degree,
        splat_count: scene.len(),
    }
}

/// Descriptor bytes requested for the tightly packed hot storage buffer.
///
/// The historical function name is retained for compatibility; the Phase B
/// implementation uses a storage buffer rather than a hot texture.
pub fn measured_hot_texture_bytes(splat_count: usize) -> u64 {
    (splat_count.max(1) as u64) * (HOT_RECORD_BYTES as u64)
}

/// Descriptor bytes requested for the RGBA8Uint degree-3 SH sidecar texture,
/// including final-row padding implied by its width and height.
pub fn measured_sh_sidecar_texture_bytes(splat_count: usize) -> u64 {
    let (width, height) = sh_sidecar_atlas_dimensions(splat_count);
    u64::from(width) * u64::from(height) * 4
}

/// Flatten hot records into tightly packed atlas pixel buffers for GPU upload.
pub struct PackedAtlasCpuBuffers {
    pub width: u32,
    pub height: u32,
    pub sh_coeffs: Vec<u8>, // 48 bytes/texel occupancy in a byte buffer
}

impl PackedAtlasCpuBuffers {
    pub fn from_packed_scene(scene: &PackedSceneCpu) -> Self {
        let (width, height) = atlas_dimensions(scene.splat_count);
        let texels = (width as usize) * (height as usize);
        let mut sh_coeffs = vec![0_u8; texels * DEGREE3_SIDECAR_BYTES];

        for (slot, sidecar) in scene.sh_sidecars.iter().enumerate() {
            let (x, y) = slot_to_texel(slot as u32, width);
            let texel = (y * width + x) as usize;
            let sh_base = texel * DEGREE3_SIDECAR_BYTES;
            sh_coeffs[sh_base..sh_base + DEGREE3_SIDECAR_BYTES]
                .copy_from_slice(&sidecar.as_texel_bytes());
        }

        Self {
            width,
            height,
            sh_coeffs,
        }
    }

    pub fn declared_cpu_staging_bytes(&self) -> u64 {
        self.sh_coeffs.len() as u64
    }

    /// Tightly packed hot records for a storage-buffer draw path: five `u32`
    /// words per splat with no atlas padding.
    pub fn hot_storage_words(scene: &PackedSceneCpu) -> Vec<u32> {
        let mut words = Vec::with_capacity(scene.hot.len() * HOT_RECORD_U32_WORDS);
        for record in &scene.hot {
            words.extend_from_slice(&record.to_storage_words());
        }
        words
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gsplat_core::Vec3f;

    fn sample_scene(count: usize, sh_degree: u8) -> SceneBuffers {
        let mut scene = SceneBuffers {
            positions: (0..count)
                .map(|i| Vec3f {
                    x: i as f32 * 0.01,
                    y: (i as f32 * 0.02).sin(),
                    z: 1.0 + i as f32 * 0.001,
                })
                .collect(),
            opacity: vec![0.0; count],
            scale_xyz: vec![[-2.0, -1.5, -1.0]; count],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; count],
            color_dc: vec![[0.2, -0.1, 0.4]; count],
            sh_degree,
            sh_rest: None,
        };
        if sh_degree > 0 {
            let rest_stride = ((sh_degree as usize + 1).pow(2) - 1) * 3;
            scene.sh_rest = Some(vec![0.01; count * rest_stride]);
            for (index, value) in scene.sh_rest.as_mut().unwrap().iter_mut().enumerate() {
                *value = ((index % 17) as f32 - 8.0) * 0.02;
            }
        }
        scene
    }

    #[test]
    fn hot_record_is_exactly_twenty_bytes() {
        assert_eq!(PackedHotRecord::byte_len(), 20);
        assert_eq!(
            HotStream::PositionOpacity.bytes_per_splat()
                + HotStream::ScaleFlags.bytes_per_splat()
                + HotStream::Rotation.bytes_per_splat()
                + HotStream::Color.bytes_per_splat(),
            20
        );
    }

    #[test]
    fn degree3_sidecar_and_full_attribute_budgets() {
        assert_eq!(PackedShSidecar::byte_len(), 48);
        assert_eq!(FULL_DEGREE3_ATTRIBUTE_BYTES, 68);
    }

    #[test]
    fn degree3_attribute_reduction_is_at_least_three_x() {
        let scene = sample_scene(1024, 3);
        let packed = pack_scene(&scene);
        assert_eq!(packed.attribute_bytes_per_splat(), 68);
        let factor = packed.degree3_reduction_factor();
        assert!(
            factor >= 3.0,
            "expected >=3x reduction, got {factor} (direct {} vs packed {})",
            DIRECT_DEGREE3_ATTRIBUTE_BYTES,
            packed.attribute_bytes_per_splat()
        );
        // 244 / 68 ≈ 3.588
        assert!((factor - (244.0 / 68.0)).abs() < 1e-9);
    }

    #[test]
    fn metadata_only_scans_match_full_scene_packing() {
        let mut scene = sample_scene(8, 3);
        scene.scale_xyz[0] = [-7.0, -2.0, 4.0];
        scene.scale_xyz[7] = [5.0, -3.0, 2.0];
        let packed = pack_scene(&scene);

        assert_eq!(
            LogScaleRange::from_scales(&scene.scale_xyz),
            packed.log_scale_range
        );
        assert_eq!(scene_sh_scales(&scene), packed.sh_scales);
    }

    #[test]
    fn position_roundtrip_stays_within_bounds_error() {
        let scene = sample_scene(8, 0);
        let packed = pack_scene(&scene);
        for (index, record) in packed.hot.iter().enumerate() {
            let decoded = packed.bounds.decode_u16([
                record.position_opacity[0],
                record.position_opacity[1],
                record.position_opacity[2],
            ]);
            let original = scene.positions[index];
            assert!((decoded.x - original.x).abs() < 1e-3);
            assert!((decoded.y - original.y).abs() < 1e-3);
            assert!((decoded.z - original.z).abs() < 1e-3);
        }
    }

    #[test]
    fn quat_smallest_three_roundtrip_preserves_orientation() {
        let samples = [
            [0.0, 0.0, 0.0, 1.0],
            [0.1, -0.2, 0.3, 0.9],
            [0.5, 0.5, 0.5, 0.5],
            [-0.3, 0.1, 0.2, 0.9],
            // Largest component is not w; previously broke when only w was forced positive.
            [0.9, -0.2, 0.1, -0.3],
            [-0.7, 0.1, 0.2, 0.1],
            [0.1, 0.8, -0.2, -0.4],
        ];
        for sample in samples {
            let packed = pack_quat_smallest_three(sample);
            let unpacked = unpack_quat_smallest_three(packed);
            let a = quat_normalize(sample);
            let b = unpacked;
            // Absolute orientation equivalence (q and -q).
            let same = (0..4).map(|i| (a[i] - b[i]).abs()).sum::<f32>();
            let flipped = (0..4).map(|i| (a[i] + b[i]).abs()).sum::<f32>();
            assert!(
                same.min(flipped) < 0.05,
                "quat drift too large: {a:?} vs {b:?}"
            );
        }
    }

    #[test]
    fn sh_sidecar_roundtrip_keeps_sign_and_scale() {
        let scene = sample_scene(4, 3);
        let packed = pack_scene(&scene);
        let original = scene.sh_rest.as_ref().unwrap();
        for index in 0..4 {
            let restored = dequantize_sh_rest(&packed.sh_sidecars[index], packed.sh_scales);
            let base = index * 45;
            for lane in 0..45 {
                let err = (restored[lane] - original[base + lane]).abs();
                assert!(err < 0.03, "coeff {lane} err {err}");
            }
        }
    }

    #[test]
    fn sh_sidecar_transitions_preserve_degree_0_through_3_coefficients() {
        for degree in 0_u8..=3 {
            let scene = sample_scene(4, degree);
            scene.validate().expect("sample scene must be valid");
            let packed = pack_scene(&scene);
            assert_eq!(packed.sh_degree, degree);
            assert_eq!(packed.sh_sidecars.len(), scene.len());

            let coeffs_per_channel = ((degree as usize + 1).pow(2) - 1).min(15);
            let restored = dequantize_sh_rest(&packed.sh_sidecars[0], packed.sh_scales);
            if degree == 0 {
                assert!(restored.iter().all(|value| *value == 0.0));
                continue;
            }

            let original = scene.sh_rest.as_ref().unwrap();
            for channel in 0..3 {
                for lane in 0..coeffs_per_channel {
                    let source = original[channel * coeffs_per_channel + lane];
                    let decoded = restored[channel * 15 + lane];
                    assert!(
                        (decoded - source).abs() < 0.03,
                        "degree {degree} channel {channel} lane {lane}: {decoded} vs {source}"
                    );
                }
                assert!(
                    restored[channel * 15 + coeffs_per_channel..channel * 15 + 15]
                        .iter()
                        .all(|value| *value == 0.0)
                );
            }
        }
    }

    #[test]
    fn atlas_dimensions_cover_all_slots() {
        for count in [1, 2, 2048, 2049, 4096, 279_199] {
            let (width, height) = atlas_dimensions(count);
            assert!(width >= 1 && height >= 1);
            assert!(u64::from(width) * u64::from(height) >= count as u64);
            let (x, y) = slot_to_texel((count as u32).saturating_sub(1), width);
            assert!(x < width && y < height);
        }
    }

    #[test]
    fn color_rgb10_roundtrip() {
        let rgb = [0.0, 0.5, 1.0];
        let packed = pack_color_rgb10(rgb);
        let unpacked = unpack_color_rgb10(packed);
        for (a, b) in rgb.iter().zip(unpacked) {
            assert!((a - b).abs() < 1.0 / 1023.0 + 1e-6);
        }
    }

    #[test]
    fn signed_dc_rgb10_roundtrip() {
        let dc = [0.2, -0.1, 0.4];
        let max_abs = 0.4;
        let packed = pack_signed_dc_rgb10(dc, max_abs);
        let unpacked = unpack_signed_dc_rgb10(packed, max_abs);
        for (a, b) in dc.iter().zip(unpacked) {
            assert!((a - b).abs() < (2.0 * max_abs) / 1023.0 + 1e-6);
        }
    }

    #[test]
    fn opacity_and_scale_codecs_are_finite() {
        for value in [-4.0, -1.0, 0.0, 1.0, 3.0] {
            let encoded = encode_opacity_u8(value);
            assert!(decode_opacity_u8(encoded).is_finite());
        }
        let range = LogScaleRange {
            min: -4.0,
            max: 4.0,
        };
        for value in [-4.0, -1.0, 0.0, 1.0, 3.0] {
            let encoded = range.encode_u10(value);
            assert!((range.decode_u10(encoded) - value).abs() < 0.01);
        }
    }

    #[test]
    fn scene_packs_u10_scales_rotation_and_color_into_words_two_through_four() {
        let scene = sample_scene(1, 0);
        let packed = pack_scene(&scene);
        let record = packed.hot[0];
        let words = record.to_storage_words();
        assert_eq!(words.len(), 5);
        assert_eq!(words[2], record.scale_flags);
        assert_eq!(words[3], record.rotation);
        assert_eq!(words[4], record.color);
        for axis in 0..3 {
            let encoded = (record.scale_flags >> (axis * 10)) & 0x3ff;
            let decoded = packed.log_scale_range.decode_u10(encoded);
            assert!((decoded - scene.scale_xyz[0][axis]).abs() < 0.01);
        }
    }

    #[test]
    fn descriptor_resource_bytes_match_created_buffer_and_texture_shapes() {
        for count in [1_usize, 100, 2048, 279_199] {
            let capacity = count.max(1) as u64;
            let (sh_width, sh_height) = sh_sidecar_atlas_dimensions(count);
            let sh_texels = u64::from(sh_width) * u64::from(sh_height);
            let descriptor_total =
                measured_hot_texture_bytes(count) + measured_sh_sidecar_texture_bytes(count);
            assert_eq!(measured_hot_texture_bytes(count), capacity * 20);
            assert_eq!(measured_sh_sidecar_texture_bytes(count), sh_texels * 4);
            assert!(descriptor_total >= (count as u64) * 68);
            assert!(descriptor_total - (count as u64) * 68 < u64::from(sh_width) * 4);
        }
    }

    #[test]
    fn optional_kitsune_pack_meets_byte_gates() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/datasets/external/wakufactory_kitune/kitune1.ply");
        if !path.is_file() {
            eprintln!(
                "skipping kitsune pack gate; dataset missing at {}",
                path.display()
            );
            return;
        }
        let loaded = gsplat_io_ply::load_ply(&path).expect("load kitsune");
        let packed = pack_scene(&loaded.scene);
        let buffers = PackedAtlasCpuBuffers::from_packed_scene(&packed);
        assert_eq!(packed.splat_count, 279_199);
        assert_eq!(packed.attribute_bytes_per_splat(), 68);
        assert!(packed.degree3_reduction_factor() >= 3.0);
        assert_eq!(
            buffers.declared_cpu_staging_bytes(),
            buffers.sh_coeffs.len() as u64,
        );
    }
}
