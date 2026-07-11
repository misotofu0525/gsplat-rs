//! SPZ scene loading and parsing utilities.

use std::fs;
use std::io::{Cursor, Read};
use std::mem::size_of;
use std::path::Path;

use gsplat_core::{ErrorCode, SceneBuffers, Vec3f};
use thiserror::Error;

const MIB: usize = 1024 * 1024;
const GIB: usize = 1024 * MIB;
const HEADER_BYTES: usize = 32;
const TOC_ENTRY_BYTES: usize = 16;
const SPZ_MAGIC: u32 = 0x5053_474e;
const SPZ_VERSION: u32 = 4;
const FLAG_ANTIALIASED: u8 = 0x1;
const FLAG_HAS_EXTENSIONS: u8 = 0x2;
const COLOR_SCALE: f32 = 0.15;
const SQRT_ONE_HALF: f32 = std::f32::consts::FRAC_1_SQRT_2;
const OPACITY_LOGIT_LIMIT: f32 = 16.0;
const MAX_SUPPORTED_SH_DEGREE: u8 = 3;

// Niantic `coordinateConverter(RUB, RUF)` within-family flips: x=1, y=1, z=-1.
// Positions multiply flipP; xyzw rotations multiply flipQ on xyz; SH coeffs
// multiply flipSh[coeff] for each RGB channel. Indices match SPZ SH bands
// excluding DC (degree 1..3 use the first 3/8/15 entries).
const SH_FLIP_RUB_TO_RUF: [f32; 15] = [
    1.0, -1.0, 1.0, // degree 1
    1.0, -1.0, 1.0, -1.0, 1.0, // degree 2
    1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, // degree 3
];

/// Resource budgets applied while reading and decoding an SPZ scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpzLoadLimits {
    pub max_input_bytes: usize,
    pub max_points: usize,
    pub max_scene_bytes: usize,
}

impl Default for SpzLoadLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: GIB,
            max_points: 5_000_000,
            max_scene_bytes: GIB,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpzSceneSummary {
    pub gaussians: usize,
    pub sh_degree: u8,
    pub antialiased: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpzLoadResult {
    pub scene: SceneBuffers,
    pub summary: SpzSceneSummary,
}

#[derive(Debug, Error, PartialEq)]
pub enum SpzLoadError {
    #[error("I/O error while reading SPZ")]
    Io,
    #[error("invalid SPZ magic")]
    InvalidMagic,
    #[error("unsupported SPZ version {0}; only version 4 is supported")]
    UnsupportedVersion(u32),
    #[error("SPZ spherical harmonics degree {0} is not supported in this loader slice")]
    UnsupportedShDegree(u8),
    #[error("malformed SPZ v4 header")]
    MalformedHeader,
    #[error("SPZ extensions are not supported in this loader slice")]
    ExtensionsUnsupported,
    #[error("SPZ resource limit exceeded for {resource}: requested {requested}, limit {limit}")]
    ResourceLimit {
        resource: &'static str,
        requested: usize,
        limit: usize,
    },
    #[error("SPZ resource size overflow while computing {0}")]
    ResourceSizeOverflow(&'static str),
    #[error("invalid SPZ attribute stream layout")]
    InvalidStreamLayout,
    #[error("failed to decompress an SPZ ZSTD attribute stream")]
    DecompressionFailed,
    #[error("decoded SPZ scene buffers are inconsistent")]
    InvalidScene,
    #[error("failed to reserve memory for SPZ {0}")]
    AllocationFailed(&'static str),
}

impl SpzLoadError {
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Io => ErrorCode::NotFound,
            Self::UnsupportedVersion(_)
            | Self::UnsupportedShDegree(_)
            | Self::ExtensionsUnsupported => ErrorCode::Unsupported,
            Self::ResourceLimit { .. } | Self::ResourceSizeOverflow(_) => ErrorCode::Unsupported,
            Self::InvalidMagic
            | Self::MalformedHeader
            | Self::InvalidStreamLayout
            | Self::DecompressionFailed
            | Self::InvalidScene => ErrorCode::ParseFailed,
            Self::AllocationFailed(_) => ErrorCode::Internal,
        }
    }
}

pub fn load_spz(path: &Path) -> Result<SpzLoadResult, SpzLoadError> {
    load_spz_with_limits(path, SpzLoadLimits::default())
}

pub fn load_spz_with_limits(
    path: &Path,
    limits: SpzLoadLimits,
) -> Result<SpzLoadResult, SpzLoadError> {
    let metadata = fs::metadata(path).map_err(|_| SpzLoadError::Io)?;
    let input_len = usize::try_from(metadata.len())
        .map_err(|_| SpzLoadError::ResourceSizeOverflow("input bytes"))?;
    ensure_limit("input bytes", input_len, limits.max_input_bytes)?;
    let input = fs::read(path).map_err(|_| SpzLoadError::Io)?;
    parse_spz_bytes_with_limits(&input, limits)
}

pub fn parse_spz_bytes(input: &[u8]) -> Result<SpzLoadResult, SpzLoadError> {
    parse_spz_bytes_with_limits(input, SpzLoadLimits::default())
}

pub fn parse_spz_bytes_with_limits(
    input: &[u8],
    limits: SpzLoadLimits,
) -> Result<SpzLoadResult, SpzLoadError> {
    ensure_limit("input bytes", input.len(), limits.max_input_bytes)?;
    let header = parse_header(input, limits)?;
    let expected_sizes = expected_stream_sizes(header)?;
    let streams = decompress_streams(input, header, &expected_sizes)?;
    let mut scene = allocate_scene(header, limits)?;
    unpack_scene(header, &streams, &mut scene)?;
    scene.validate().map_err(|_| SpzLoadError::InvalidScene)?;

    Ok(SpzLoadResult {
        summary: SpzSceneSummary {
            gaussians: scene.len(),
            sh_degree: header.sh_degree,
            antialiased: header.flags & FLAG_ANTIALIASED != 0,
        },
        scene,
    })
}

#[derive(Debug, Clone, Copy)]
struct SpzHeader {
    num_points: usize,
    sh_degree: u8,
    fractional_bits: u8,
    flags: u8,
    num_streams: usize,
    toc_byte_offset: usize,
}

fn dim_for_degree(degree: u8) -> Result<usize, SpzLoadError> {
    match degree {
        0 => Ok(0),
        1 => Ok(3),
        2 => Ok(8),
        3 => Ok(15),
        other => Err(SpzLoadError::UnsupportedShDegree(other)),
    }
}

fn expected_stream_count(sh_degree: u8) -> usize {
    if sh_degree == 0 { 5 } else { 6 }
}

fn parse_header(input: &[u8], limits: SpzLoadLimits) -> Result<SpzHeader, SpzLoadError> {
    if input.len() < HEADER_BYTES {
        return Err(SpzLoadError::MalformedHeader);
    }
    let magic = read_u32(input, 0)?;
    if magic != SPZ_MAGIC {
        return Err(SpzLoadError::InvalidMagic);
    }
    let version = read_u32(input, 4)?;
    if version != SPZ_VERSION {
        return Err(SpzLoadError::UnsupportedVersion(version));
    }

    let num_points = usize::try_from(read_u32(input, 8)?)
        .map_err(|_| SpzLoadError::ResourceSizeOverflow("point count"))?;
    ensure_limit("points", num_points, limits.max_points)?;
    let sh_degree = input[12];
    if sh_degree > MAX_SUPPORTED_SH_DEGREE {
        return Err(SpzLoadError::UnsupportedShDegree(sh_degree));
    }
    let _ = dim_for_degree(sh_degree)?;
    let fractional_bits = input[13];
    if fractional_bits > 30 {
        return Err(SpzLoadError::MalformedHeader);
    }
    let flags = input[14];
    if flags & FLAG_HAS_EXTENSIONS != 0 {
        return Err(SpzLoadError::ExtensionsUnsupported);
    }
    if flags & !(FLAG_ANTIALIASED | FLAG_HAS_EXTENSIONS) != 0 {
        return Err(SpzLoadError::MalformedHeader);
    }
    let num_streams = usize::from(input[15]);
    if num_streams != expected_stream_count(sh_degree) {
        return Err(SpzLoadError::InvalidStreamLayout);
    }
    let toc_byte_offset = usize::try_from(read_u32(input, 16)?)
        .map_err(|_| SpzLoadError::ResourceSizeOverflow("TOC byte offset"))?;
    if toc_byte_offset != HEADER_BYTES || input[20..HEADER_BYTES].iter().any(|&byte| byte != 0) {
        return Err(SpzLoadError::MalformedHeader);
    }

    Ok(SpzHeader {
        num_points,
        sh_degree,
        fractional_bits,
        flags,
        num_streams,
        toc_byte_offset,
    })
}

fn expected_stream_sizes(header: SpzHeader) -> Result<Vec<usize>, SpzLoadError> {
    let count = header.num_points;
    let sh_dim = dim_for_degree(header.sh_degree)?;
    let mut sizes = vec![
        count
            .checked_mul(9)
            .ok_or(SpzLoadError::ResourceSizeOverflow("position stream"))?,
        count,
        count
            .checked_mul(3)
            .ok_or(SpzLoadError::ResourceSizeOverflow("color stream"))?,
        count
            .checked_mul(3)
            .ok_or(SpzLoadError::ResourceSizeOverflow("scale stream"))?,
        count
            .checked_mul(4)
            .ok_or(SpzLoadError::ResourceSizeOverflow("rotation stream"))?,
    ];
    if sh_dim > 0 {
        sizes.push(
            count
                .checked_mul(sh_dim)
                .and_then(|value| value.checked_mul(3))
                .ok_or(SpzLoadError::ResourceSizeOverflow("SH stream"))?,
        );
    }
    Ok(sizes)
}

fn decompress_streams(
    input: &[u8],
    header: SpzHeader,
    expected_sizes: &[usize],
) -> Result<Vec<Vec<u8>>, SpzLoadError> {
    if expected_sizes.len() != header.num_streams {
        return Err(SpzLoadError::InvalidStreamLayout);
    }
    let toc_bytes = header
        .num_streams
        .checked_mul(TOC_ENTRY_BYTES)
        .ok_or(SpzLoadError::ResourceSizeOverflow("TOC bytes"))?;
    let toc_end = header
        .toc_byte_offset
        .checked_add(toc_bytes)
        .ok_or(SpzLoadError::ResourceSizeOverflow("TOC end"))?;
    if toc_end > input.len() {
        return Err(SpzLoadError::InvalidStreamLayout);
    }

    let mut compressed_offset = toc_end;
    let mut ranges = Vec::with_capacity(header.num_streams);
    for (index, expected_size) in expected_sizes.iter().copied().enumerate() {
        let entry_offset = header.toc_byte_offset + index * TOC_ENTRY_BYTES;
        let compressed_size = read_u64_as_usize(input, entry_offset)?;
        let uncompressed_size = read_u64_as_usize(input, entry_offset + 8)?;
        if uncompressed_size != expected_size {
            return Err(SpzLoadError::InvalidStreamLayout);
        }
        let compressed_end = compressed_offset
            .checked_add(compressed_size)
            .ok_or(SpzLoadError::ResourceSizeOverflow("compressed stream end"))?;
        if compressed_end > input.len() {
            return Err(SpzLoadError::InvalidStreamLayout);
        }
        ranges.push((compressed_offset, compressed_end));
        compressed_offset = compressed_end;
    }
    if compressed_offset != input.len() {
        return Err(SpzLoadError::InvalidStreamLayout);
    }

    let mut streams = Vec::with_capacity(header.num_streams);
    for (index, &(start, end)) in ranges.iter().enumerate() {
        let mut decoder = ruzstd::decoding::StreamingDecoder::new(Cursor::new(&input[start..end]))
            .map_err(|_| SpzLoadError::DecompressionFailed)?;
        let mut decoded = try_vec_with_capacity("decoded attribute stream", expected_sizes[index])?;
        decoded.resize(expected_sizes[index], 0);
        decoder
            .read_exact(&mut decoded)
            .map_err(|_| SpzLoadError::DecompressionFailed)?;
        let mut extra = [0_u8; 1];
        if decoder
            .read(&mut extra)
            .map_err(|_| SpzLoadError::DecompressionFailed)?
            != 0
        {
            return Err(SpzLoadError::DecompressionFailed);
        }
        streams.push(decoded);
    }
    Ok(streams)
}

fn allocate_scene(header: SpzHeader, limits: SpzLoadLimits) -> Result<SceneBuffers, SpzLoadError> {
    let sh_dim = dim_for_degree(header.sh_degree)?;
    let rest_stride = sh_dim
        .checked_mul(3)
        .ok_or(SpzLoadError::ResourceSizeOverflow("SH coefficient stride"))?;
    let base_stride = size_of::<Vec3f>()
        .checked_add(size_of::<f32>())
        .and_then(|value| value.checked_add(size_of::<[f32; 3]>() * 2))
        .and_then(|value| value.checked_add(size_of::<[f32; 4]>()))
        .ok_or(SpzLoadError::ResourceSizeOverflow("scene stride"))?;
    let base_bytes = header
        .num_points
        .checked_mul(base_stride)
        .ok_or(SpzLoadError::ResourceSizeOverflow("base scene bytes"))?;
    let rest_capacity = if rest_stride == 0 {
        0
    } else {
        header
            .num_points
            .checked_mul(rest_stride)
            .ok_or(SpzLoadError::ResourceSizeOverflow(
                "SH coefficient capacity",
            ))?
    };
    let rest_bytes = rest_capacity
        .checked_mul(size_of::<f32>())
        .ok_or(SpzLoadError::ResourceSizeOverflow("SH coefficient bytes"))?;
    let scene_bytes = base_bytes
        .checked_add(rest_bytes)
        .ok_or(SpzLoadError::ResourceSizeOverflow("decoded scene bytes"))?;
    ensure_limit("decoded scene bytes", scene_bytes, limits.max_scene_bytes)?;

    Ok(SceneBuffers {
        positions: try_vec_with_capacity("positions", header.num_points)?,
        opacity: try_vec_with_capacity("opacities", header.num_points)?,
        scale_xyz: try_vec_with_capacity("scales", header.num_points)?,
        rotation_xyzw: try_vec_with_capacity("rotations", header.num_points)?,
        color_dc: try_vec_with_capacity("DC colors", header.num_points)?,
        sh_degree: header.sh_degree,
        sh_rest: if rest_stride == 0 {
            None
        } else {
            Some(try_vec_with_capacity("SH coefficients", rest_capacity)?)
        },
    })
}

fn unpack_scene(
    header: SpzHeader,
    streams: &[Vec<u8>],
    scene: &mut SceneBuffers,
) -> Result<(), SpzLoadError> {
    let sh_dim = dim_for_degree(header.sh_degree)?;
    let expected_streams = expected_stream_count(header.sh_degree);
    if streams.len() != expected_streams {
        return Err(SpzLoadError::InvalidStreamLayout);
    }

    let position_scale = 2_f32.powi(-i32::from(header.fractional_bits));
    for point in 0..header.num_points {
        let position_base = point * 9;
        let x = decode_i24(&streams[0][position_base..position_base + 3]) as f32 * position_scale;
        let y =
            decode_i24(&streams[0][position_base + 3..position_base + 6]) as f32 * position_scale;
        let z =
            decode_i24(&streams[0][position_base + 6..position_base + 9]) as f32 * position_scale;
        // Extension-free SPZ v4 stores RUB. Runtime SceneBuffers use RUF, so flip Z once on load.
        scene.positions.push(Vec3f::new(x, y, -z));

        let alpha = f32::from(streams[1][point]) / 255.0;
        scene.opacity.push(if alpha <= 0.0 {
            -OPACITY_LOGIT_LIMIT
        } else if alpha >= 1.0 {
            OPACITY_LOGIT_LIMIT
        } else {
            (alpha / (1.0 - alpha)).ln()
        });

        let base3 = point * 3;
        scene.color_dc.push([
            decode_color(streams[2][base3]),
            decode_color(streams[2][base3 + 1]),
            decode_color(streams[2][base3 + 2]),
        ]);
        scene.scale_xyz.push([
            f32::from(streams[3][base3]) / 16.0 - 10.0,
            f32::from(streams[3][base3 + 1]) / 16.0 - 10.0,
            f32::from(streams[3][base3 + 2]) / 16.0 - 10.0,
        ]);

        let rotation_base = point * 4;
        let mut rotation = decode_smallest_three(&streams[4][rotation_base..rotation_base + 4])?;
        // RUB -> RUF is a Z-axis reflection. For an xyzw quaternion this flips x and y.
        rotation[0] = -rotation[0];
        rotation[1] = -rotation[1];
        scene.rotation_xyzw.push(rotation);

        if let Some(rest_out) = scene.sh_rest.as_mut() {
            let sh_stream = &streams[5];
            let sh_base = point * sh_dim * 3;
            // SPZ stores coeff-major RGB triples; SceneBuffers use PLY channel-major rest order.
            for channel in 0..3 {
                for coeff in 0..sh_dim {
                    let packed = sh_stream[sh_base + coeff * 3 + channel];
                    let value = unquantize_sh(packed) * SH_FLIP_RUB_TO_RUF[coeff];
                    rest_out.push(value);
                }
            }
        }
    }
    Ok(())
}

fn decode_smallest_three(bytes: &[u8]) -> Result<[f32; 4], SpzLoadError> {
    let mut packed_bytes = [0_u8; 4];
    packed_bytes.copy_from_slice(bytes);
    let mut packed = u32::from_le_bytes(packed_bytes);
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
    if sum_squares > 1.0 + f32::EPSILON {
        return Err(SpzLoadError::InvalidStreamLayout);
    }
    rotation[largest] = (1.0 - sum_squares).max(0.0).sqrt();
    Ok(rotation)
}

fn decode_i24(bytes: &[u8]) -> i32 {
    let unsigned = i32::from(bytes[0]) | (i32::from(bytes[1]) << 8) | (i32::from(bytes[2]) << 16);
    if unsigned & 0x80_0000 != 0 {
        unsigned | !0xff_ffff
    } else {
        unsigned
    }
}

fn decode_color(value: u8) -> f32 {
    (f32::from(value) / 255.0 - 0.5) / COLOR_SCALE
}

fn unquantize_sh(value: u8) -> f32 {
    (f32::from(value) - 128.0) / 128.0
}

fn read_u32(input: &[u8], offset: usize) -> Result<u32, SpzLoadError> {
    let bytes = input
        .get(offset..offset + 4)
        .ok_or(SpzLoadError::MalformedHeader)?;
    Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
}

fn read_u64_as_usize(input: &[u8], offset: usize) -> Result<usize, SpzLoadError> {
    let bytes = input
        .get(offset..offset + 8)
        .ok_or(SpzLoadError::InvalidStreamLayout)?;
    usize::try_from(u64::from_le_bytes(bytes.try_into().unwrap()))
        .map_err(|_| SpzLoadError::ResourceSizeOverflow("stream size"))
}

fn ensure_limit(
    resource: &'static str,
    requested: usize,
    limit: usize,
) -> Result<(), SpzLoadError> {
    if requested > limit {
        Err(SpzLoadError::ResourceLimit {
            resource,
            requested,
            limit,
        })
    } else {
        Ok(())
    }
}

fn try_vec_with_capacity<T>(
    resource: &'static str,
    capacity: usize,
) -> Result<Vec<T>, SpzLoadError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(capacity)
        .map_err(|_| SpzLoadError::AllocationFailed(resource))?;
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::{
        SH_FLIP_RUB_TO_RUF, SpzLoadError, SpzLoadLimits, dim_for_degree, parse_spz_bytes,
        parse_spz_bytes_with_limits, unquantize_sh,
    };

    const HEADER_BYTES: usize = 32;
    const MAGIC: u32 = 0x5053_474e;
    const VERSION: u32 = 4;

    fn pack_i24(value: f32, fractional_bits: u8) -> [u8; 3] {
        let fixed = (value * (1_u32 << fractional_bits) as f32).round() as i32;
        let bytes = fixed.to_le_bytes();
        [bytes[0], bytes[1], bytes[2]]
    }

    fn synthetic_spz(count: usize, sh_degree: u8) -> Vec<u8> {
        assert!(count >= 8);
        let sh_dim = dim_for_degree(sh_degree).unwrap();
        let stream_count = if sh_degree == 0 { 5_u8 } else { 6 };

        let mut positions = Vec::with_capacity(count * 9);
        let mut alphas = Vec::with_capacity(count);
        let mut colors = Vec::with_capacity(count * 3);
        let mut scales = Vec::with_capacity(count * 3);
        let mut rotations = Vec::with_capacity(count * 4);
        let mut sh = Vec::with_capacity(count * sh_dim * 3);

        for index in 0..count {
            let coords = [
                index as f32 * 0.25,
                index as f32 * -0.125,
                1.0 + index as f32 * 0.0625,
            ];
            for coordinate in coords {
                positions.extend_from_slice(&pack_i24(coordinate, 12));
            }
            alphas.push(32 + index as u8 * 20);
            colors.extend_from_slice(&[96 + index as u8, 112 + index as u8, 128 + index as u8]);
            scales.extend_from_slice(&[144 + index as u8, 152, 160 - index as u8]);
            rotations.extend_from_slice(&[0, 0, 0, 0]);

            for coeff in 0..sh_dim {
                for channel in 0..3 {
                    let packed =
                        128_i16 + ((index as i16 + coeff as i16 + channel as i16) % 40) - 20;
                    sh.push(packed.clamp(0, 255) as u8);
                }
            }
        }

        let mut streams = vec![positions, alphas, colors, scales, rotations];
        if sh_dim > 0 {
            streams.push(sh);
        }
        let compressed: Vec<Vec<u8>> = streams
            .iter()
            .map(|stream| {
                ruzstd::encoding::compress_to_vec(
                    stream.as_slice(),
                    ruzstd::encoding::CompressionLevel::Fastest,
                )
            })
            .collect();

        let toc_offset = HEADER_BYTES;
        let mut output = Vec::new();
        output.extend_from_slice(&MAGIC.to_le_bytes());
        output.extend_from_slice(&VERSION.to_le_bytes());
        output.extend_from_slice(&(count as u32).to_le_bytes());
        output.extend_from_slice(&[sh_degree, 12, 0, stream_count]);
        output.extend_from_slice(&(toc_offset as u32).to_le_bytes());
        output.extend_from_slice(&[0; 12]);

        for (stream, chunk) in streams.iter().zip(&compressed) {
            output.extend_from_slice(&(chunk.len() as u64).to_le_bytes());
            output.extend_from_slice(&(stream.len() as u64).to_le_bytes());
        }
        for chunk in compressed {
            output.extend_from_slice(&chunk);
        }
        output
    }

    fn synthetic_degree_0_spz(count: usize) -> Vec<u8> {
        synthetic_spz(count, 0)
    }

    fn expected_sh_rest(count: usize, sh_degree: u8) -> Vec<f32> {
        let sh_dim = dim_for_degree(sh_degree).unwrap();
        let mut expected = Vec::with_capacity(count * sh_dim * 3);
        for index in 0..count {
            for channel in 0..3 {
                for (coeff, flip) in SH_FLIP_RUB_TO_RUF.iter().copied().enumerate().take(sh_dim) {
                    let packed =
                        128_i16 + ((index as i16 + coeff as i16 + channel as i16) % 40) - 20;
                    let packed = packed.clamp(0, 255) as u8;
                    expected.push(unquantize_sh(packed) * flip);
                }
            }
        }
        expected
    }

    #[test]
    fn rejects_bad_magic() {
        let mut bytes = synthetic_degree_0_spz(8);
        bytes[0] ^= 0xff;
        assert_eq!(parse_spz_bytes(&bytes), Err(SpzLoadError::InvalidMagic));
    }

    #[test]
    fn rejects_non_v4_header() {
        let mut bytes = synthetic_degree_0_spz(8);
        bytes[4..8].copy_from_slice(&3_u32.to_le_bytes());
        assert_eq!(
            parse_spz_bytes(&bytes),
            Err(SpzLoadError::UnsupportedVersion(3))
        );
    }

    #[test]
    fn rejects_point_and_scene_budgets() {
        let bytes = synthetic_degree_0_spz(8);
        let input_limits = SpzLoadLimits {
            max_input_bytes: bytes.len() - 1,
            ..SpzLoadLimits::default()
        };
        assert!(matches!(
            parse_spz_bytes_with_limits(&bytes, input_limits),
            Err(SpzLoadError::ResourceLimit {
                resource: "input bytes",
                ..
            })
        ));

        let point_limits = SpzLoadLimits {
            max_points: 7,
            ..SpzLoadLimits::default()
        };
        assert!(matches!(
            parse_spz_bytes_with_limits(&bytes, point_limits),
            Err(SpzLoadError::ResourceLimit {
                resource: "points",
                requested: 8,
                limit: 7,
            })
        ));

        let scene_limits = SpzLoadLimits {
            max_scene_bytes: 8 * 56 - 1,
            ..SpzLoadLimits::default()
        };
        assert!(matches!(
            parse_spz_bytes_with_limits(&bytes, scene_limits),
            Err(SpzLoadError::ResourceLimit {
                resource: "decoded scene bytes",
                ..
            })
        ));
    }

    #[test]
    fn rejects_extensions_for_now() {
        let mut bytes = synthetic_degree_0_spz(8);
        bytes[14] = 0x2;
        assert_eq!(
            parse_spz_bytes(&bytes),
            Err(SpzLoadError::ExtensionsUnsupported)
        );
    }

    #[test]
    fn rejects_degree_4_and_wrong_stream_counts() {
        let mut degree4 = synthetic_spz(8, 3);
        degree4[12] = 4;
        degree4[15] = 6;
        assert_eq!(
            parse_spz_bytes(&degree4),
            Err(SpzLoadError::UnsupportedShDegree(4))
        );

        let mut wrong_streams = synthetic_degree_0_spz(8);
        wrong_streams[15] = 6;
        assert_eq!(
            parse_spz_bytes(&wrong_streams),
            Err(SpzLoadError::InvalidStreamLayout)
        );

        let mut missing_sh_stream = synthetic_spz(8, 1);
        missing_sh_stream[15] = 5;
        assert_eq!(
            parse_spz_bytes(&missing_sh_stream),
            Err(SpzLoadError::InvalidStreamLayout)
        );
    }

    #[test]
    fn decodes_degree_0_synthetic_v4_to_finite_ruf_scene() {
        let result = parse_spz_bytes(&synthetic_degree_0_spz(8)).unwrap();

        assert_eq!(result.summary.gaussians, 8);
        assert_eq!(result.summary.sh_degree, 0);
        assert_eq!(result.scene.len(), 8);
        assert_eq!(result.scene.sh_degree, 0);
        assert!(result.scene.sh_rest.is_none());
        assert_eq!(result.scene.positions[1].x, 0.25);
        assert_eq!(result.scene.positions[1].y, -0.125);
        assert_eq!(result.scene.positions[1].z, -(1.0 + 0.0625));
        assert!(result.scene.positions.iter().all(|value| value.is_finite()));
        assert!(result.scene.opacity.iter().all(|value| value.is_finite()));
        assert!(
            result
                .scene
                .scale_xyz
                .iter()
                .flatten()
                .all(|value| value.is_finite())
        );
        assert!(
            result
                .scene
                .rotation_xyzw
                .iter()
                .flatten()
                .all(|value| value.is_finite())
        );
        assert!(
            result
                .scene
                .color_dc
                .iter()
                .flatten()
                .all(|value| value.is_finite())
        );
    }

    #[test]
    fn decodes_degree_1_synthetic_v4_with_ruf_sh_flips() {
        let result = parse_spz_bytes(&synthetic_spz(8, 1)).unwrap();
        assert_eq!(result.summary.gaussians, 8);
        assert_eq!(result.summary.sh_degree, 1);
        assert_eq!(result.scene.sh_degree, 1);
        let sh = result.scene.sh_rest.as_ref().unwrap();
        assert_eq!(sh.len(), 8 * 9);
        assert_eq!(sh.as_slice(), expected_sh_rest(8, 1).as_slice());
        assert!(sh.iter().all(|value| value.is_finite()));
        assert_eq!(result.scene.positions[2].z, -(1.0 + 2.0 * 0.0625));
    }

    #[test]
    fn decodes_degree_3_synthetic_v4_with_ruf_sh_flips() {
        let result = parse_spz_bytes(&synthetic_spz(8, 3)).unwrap();
        assert_eq!(result.summary.gaussians, 8);
        assert_eq!(result.summary.sh_degree, 3);
        assert_eq!(result.scene.sh_degree, 3);
        let sh = result.scene.sh_rest.as_ref().unwrap();
        assert_eq!(sh.len(), 8 * 45);
        assert_eq!(sh.as_slice(), expected_sh_rest(8, 3).as_slice());
        assert!(sh.iter().all(|value| value.is_finite()));
        // Coeff 1 uses Niantic flipSh z = -1 for RUB→RUF.
        // For point 0 / channel R / coeff 1 the packed byte is 128+1-20 = 109.
        let raw_coeff1 = unquantize_sh(109);
        assert_eq!(sh[1], -raw_coeff1);
    }
}
