use bytemuck::Zeroable;

use crate::data::{
    RESIDENT_SH_PLANES, RESIDENT_SH_WORDS_PER_PLANE, ResidentChunkMeta, ResidentShPlane,
};

use super::{ResidentEncodingReport, ResidentSceneError, ResidentSourceSplat};

pub(super) const RESIDENT_SH_BITS: usize = 11;
pub(super) const RESIDENT_SH_POINT_SCALE_BITS: usize = 5;
pub(super) const RESIDENT_SH_POINT_SCALE_MAX: u32 = (1 << RESIDENT_SH_POINT_SCALE_BITS) - 1;

pub const fn sh_coeffs_per_channel(degree: u8) -> usize {
    match degree {
        0 => 0,
        1 => 3,
        2 => 8,
        _ => 15,
    }
}

pub const fn resident_sh_plane_count(degree: u8) -> usize {
    let coeffs_per_channel = sh_coeffs_per_channel(degree);
    let value_count = coeffs_per_channel * 3;
    let active_band_count = if coeffs_per_channel == 0 {
        0
    } else if coeffs_per_channel <= 3 {
        1
    } else if coeffs_per_channel <= 8 {
        2
    } else {
        3
    };
    let bit_count =
        value_count * RESIDENT_SH_BITS + active_band_count * RESIDENT_SH_POINT_SCALE_BITS;
    let word_count = bit_count.div_ceil(32);
    word_count.div_ceil(RESIDENT_SH_WORDS_PER_PLANE)
}

pub(super) fn build_chunk_meta(
    splats: &[ResidentSourceSplat],
    coeffs_per_channel: usize,
    source_start: usize,
) -> Result<ResidentChunkMeta, ResidentSceneError> {
    let mut dc_min = [f32::INFINITY; 3];
    let mut dc_max = [f32::NEG_INFINITY; 3];
    let mut sh_scales = [[0.0_f32; 3]; 3];
    for splat in splats {
        for axis in 0..3 {
            dc_min[axis] = dc_min[axis].min(splat.color_dc[axis]);
            dc_max[axis] = dc_max[axis].max(splat.color_dc[axis]);
        }
        if coeffs_per_channel > 0 {
            for (channel, coefficients) in splat.sh_rest[..usize::from(splat.sh_len)]
                .chunks_exact(coeffs_per_channel)
                .enumerate()
            {
                for (lane, value) in coefficients.iter().enumerate() {
                    let band = sh_band(lane);
                    sh_scales[band][channel] = sh_scales[band][channel].max(value.abs());
                }
            }
        }
    }

    let mut meta = ResidentChunkMeta::zeroed();
    for lane in 0..3 {
        let extent = dc_max[lane] - dc_min[lane];
        if !dc_min[lane].is_finite() || !dc_max[lane].is_finite() || !extent.is_finite() {
            return Err(ResidentSceneError::InvalidSplat {
                index: source_start,
                field: "chunk DC color range cannot be represented as finite f32",
            });
        }
        meta.dc_min[lane] = dc_min[lane];
        meta.dc_extent[lane] = extent.max(0.0);
        meta.sh_scale_l1[lane] = nonzero_sh_scale(sh_scales[0][lane]);
        meta.sh_scale_l2[lane] = nonzero_sh_scale(sh_scales[1][lane]);
        meta.sh_scale_l3[lane] = nonzero_sh_scale(sh_scales[2][lane]);
    }
    Ok(meta)
}

pub(super) fn nonzero_sh_scale(value: f32) -> f32 {
    if value > 0.0 { value } else { 1.0 }
}

#[cfg(test)]
pub(super) fn finite_or_zero(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

#[cfg(test)]
pub(super) fn finite_extent(min: f32, max: f32) -> f32 {
    let extent = max - min;
    if extent.is_finite() && extent > 0.0 {
        extent
    } else {
        0.0
    }
}

fn encode_u16(value: f32, min: f32, extent: f32) -> u32 {
    if extent <= 0.0 {
        return 0;
    }
    (((value - min) / extent).clamp(0.0, 1.0) * 65_535.0).round() as u32
}

pub(super) fn pack_vec3_u16(value: [f32; 3], min: [f32; 4], extent: [f32; 4]) -> [u32; 2] {
    [
        encode_u16(value[0], min[0], extent[0]) | (encode_u16(value[1], min[1], extent[1]) << 16),
        encode_u16(value[2], min[2], extent[2]),
    ]
}

pub(super) fn unpack_vec3_u16(bits: [u32; 2], min: [f32; 4], extent: [f32; 4]) -> [f32; 3] {
    [
        min[0] + f32::from((bits[0] & 0xffff) as u16) / 65_535.0 * extent[0],
        min[1] + f32::from((bits[0] >> 16) as u16) / 65_535.0 * extent[1],
        min[2] + f32::from((bits[1] & 0xffff) as u16) / 65_535.0 * extent[2],
    ]
}

pub(super) fn pack_sh_planes(
    source: &[f32],
    coeffs_per_channel: usize,
    plane_count: usize,
    meta: &ResidentChunkMeta,
    destination: &mut [Vec<ResidentShPlane>; RESIDENT_SH_PLANES],
    report: &mut ResidentEncodingReport,
) {
    let mut words = [0_u32; RESIDENT_SH_PLANES * RESIDENT_SH_WORDS_PER_PLANE];
    let mut point_scale_codes = [0_u32; 3];
    for lane in 0..coeffs_per_channel {
        for channel in 0..3 {
            let source_index = channel * coeffs_per_channel + lane;
            let band = sh_band(lane);
            let chunk_scale = sh_band_scale(meta, channel, lane);
            let ratio = (source[source_index].abs() / chunk_scale).clamp(0.0, 1.0);
            point_scale_codes[band] = point_scale_codes[band].max(
                (ratio * RESIDENT_SH_POINT_SCALE_MAX as f32)
                    .ceil()
                    .clamp(0.0, RESIDENT_SH_POINT_SCALE_MAX as f32) as u32,
            );
        }
    }
    let active_band_count = usize::from(coeffs_per_channel > 0)
        + usize::from(coeffs_per_channel > 3)
        + usize::from(coeffs_per_channel > 8);
    let point_scale_bit_base = coeffs_per_channel * 3 * RESIDENT_SH_BITS;
    for (band, code) in point_scale_codes
        .iter()
        .copied()
        .enumerate()
        .take(active_band_count)
    {
        pack_unsigned_bits(
            &mut words,
            point_scale_bit_base + band * RESIDENT_SH_POINT_SCALE_BITS,
            RESIDENT_SH_POINT_SCALE_BITS,
            code,
        );
    }

    for lane in 0..coeffs_per_channel {
        for channel in 0..3 {
            let source_index = channel * coeffs_per_channel + lane;
            let band = sh_band(lane);
            let point_scale = point_scale_codes[band] as f32 / RESIDENT_SH_POINT_SCALE_MAX as f32;
            let scale = sh_band_scale(meta, channel, lane) * point_scale;
            if point_scale_codes[band] == 0 {
                debug_assert_eq!(source[source_index], 0.0);
                continue;
            }
            let encoded = (source[source_index] / scale * 1023.0)
                .round()
                .clamp(-1023.0, 1023.0) as i32;
            let logical_value = lane * 3 + channel;
            pack_signed_11(&mut words, logical_value, encoded);
            let decoded = encoded as f32 / 1023.0 * scale;
            report.max_sh_error_by_band[band] =
                report.max_sh_error_by_band[band].max((decoded - source[source_index]).abs());
        }
    }
    for (plane, output) in destination.iter_mut().enumerate().take(plane_count) {
        let start = plane * RESIDENT_SH_WORDS_PER_PLANE;
        output.push(ResidentShPlane {
            words: words[start..start + RESIDENT_SH_WORDS_PER_PLANE]
                .try_into()
                .expect("fixed resident SH plane width"),
        });
    }
}

pub(super) fn pack_unsigned_bits(
    words: &mut [u32; 16],
    bit_offset: usize,
    bit_count: usize,
    value: u32,
) {
    debug_assert!(bit_count > 0 && bit_count < 32);
    debug_assert!(value < (1_u32 << bit_count));
    let word = bit_offset / 32;
    let shift = bit_offset % 32;
    words[word] |= value << shift;
    if shift + bit_count > 32 {
        words[word + 1] |= value >> (32 - shift);
    }
}

pub(super) fn pack_signed_11(words: &mut [u32; 16], logical_value: usize, value: i32) {
    let bits = (value as u32) & 0x7ff;
    let bit_offset = logical_value * RESIDENT_SH_BITS;
    let word = bit_offset / 32;
    let shift = bit_offset % 32;
    words[word] |= bits << shift;
    if shift > 21 {
        words[word + 1] |= bits >> (32 - shift);
    }
}

#[cfg(test)]
pub(super) fn unpack_signed_11(
    planes: &[Vec<ResidentShPlane>; RESIDENT_SH_PLANES],
    index: usize,
    logical_value: usize,
) -> i32 {
    let bit_offset = logical_value * RESIDENT_SH_BITS;
    let word_index = bit_offset / 32;
    let shift = bit_offset % 32;
    let load_word = |word_index: usize| {
        let plane = word_index / RESIDENT_SH_WORDS_PER_PLANE;
        let lane = word_index % RESIDENT_SH_WORDS_PER_PLANE;
        planes[plane][index].words[lane]
    };
    let mut bits = load_word(word_index) >> shift;
    if shift > 21 {
        bits |= load_word(word_index + 1) << (32 - shift);
    }
    ((bits & 0x7ff) << 21) as i32 >> 21
}

#[cfg(test)]
pub(super) fn unpack_unsigned_bits(
    planes: &[Vec<ResidentShPlane>; RESIDENT_SH_PLANES],
    index: usize,
    bit_offset: usize,
    bit_count: usize,
) -> u32 {
    let word_index = bit_offset / 32;
    let shift = bit_offset % 32;
    let load_word = |word_index: usize| {
        let plane = word_index / RESIDENT_SH_WORDS_PER_PLANE;
        let lane = word_index % RESIDENT_SH_WORDS_PER_PLANE;
        planes[plane][index].words[lane]
    };
    let mut bits = load_word(word_index) >> shift;
    if shift + bit_count > 32 {
        bits |= load_word(word_index + 1) << (32 - shift);
    }
    bits & ((1_u32 << bit_count) - 1)
}

pub(super) fn sh_band(lane: usize) -> usize {
    match lane {
        0..=2 => 0,
        3..=7 => 1,
        _ => 2,
    }
}

pub(super) fn sh_band_scale(meta: &ResidentChunkMeta, channel: usize, lane: usize) -> f32 {
    match sh_band(lane) {
        0 => meta.sh_scale_l1[channel],
        1 => meta.sh_scale_l2[channel],
        _ => meta.sh_scale_l3[channel],
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn update_base_error_report(
    report: &mut ResidentEncodingReport,
    dc: [f32; 3],
    meta: &ResidentChunkMeta,
    dc_bits: [u32; 2],
) {
    let decoded_dc = unpack_vec3_u16(dc_bits, meta.dc_min, meta.dc_extent);
    for lane in 0..3 {
        report.max_dc_error = report.max_dc_error.max((decoded_dc[lane] - dc[lane]).abs());
    }
}

pub(super) fn sigmoid(value: f32) -> f32 {
    1.0 / (1.0 + (-value).exp())
}
