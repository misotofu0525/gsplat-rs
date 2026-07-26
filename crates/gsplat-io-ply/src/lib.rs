//! PLY scene loading and parsing utilities.

mod metadata;

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, ErrorKind, Read};
use std::mem::size_of;
use std::path::Path;

use gsplat_core::{ErrorCode, SceneBuffers, Vec3f};
use thiserror::Error;

pub use metadata::PlySceneSummary;
use metadata::{
    PlyFormat, PlyHeader, PlyScalarType, build_property_indices, complete_header_end,
    complete_header_end_at_eof, infer_sh_rest_layout, parse_header_text, read_header_from_reader,
    sh_rest_stride, split_header_body, summary_from_header,
};

// Scaniverse can encode exactly transparent/opaque splats as infinite logits.
// A finite clamp preserves the post-sigmoid alpha while keeping SceneBuffers valid.
const OPACITY_LOGIT_LIMIT: f32 = 16.0;
const MIB: usize = 1024 * 1024;
const GIB: usize = 1024 * MIB;

// Keep byte budgets at or below `isize::MAX` on wasm32 and other 32-bit
// targets. This also leaves enough room for the complete 6,131,954-point SH3
// Bicycle validation scene without turning the default entrypoints into an
// effectively unbounded allocation request.
const DEFAULT_MAX_INPUT_BYTES: usize = 2 * GIB - 1;
const DEFAULT_MAX_VERTICES: usize = 8_388_608;
const DEFAULT_MAX_SCENE_BYTES: usize = 2 * GIB - 1;

const _: () = assert!(DEFAULT_MAX_INPUT_BYTES <= isize::MAX as usize);
const _: () = assert!(DEFAULT_MAX_SCENE_BYTES <= isize::MAX as usize);

/// Maximum number of non-DC spherical-harmonics coefficients carried by one
/// decoded splat (three channels at SH degree 3).
pub const MAX_SH_REST_COEFFICIENTS: usize = 45;

/// Resource budgets applied while reading and decoding a PLY scene.
///
/// The finite defaults admit the repository's complete large-scene validation
/// corpus, including the 6,131,954-point SH3 Bicycle scene, while rejecting
/// forged headers that would otherwise request unbounded work or allocation.
/// Byte budgets remain at or below the signed addressable range of wasm32 and
/// other 32-bit targets. Applications with tighter or deliberately different
/// budgets should use the limit-aware loading functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlyLoadLimits {
    pub max_input_bytes: usize,
    pub max_header_bytes: usize,
    pub max_vertices: usize,
    pub max_vertex_properties: usize,
    pub max_scene_bytes: usize,
}

impl Default for PlyLoadLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: DEFAULT_MAX_INPUT_BYTES,
            max_header_bytes: MIB,
            max_vertices: DEFAULT_MAX_VERTICES,
            max_vertex_properties: 128,
            max_scene_bytes: DEFAULT_MAX_SCENE_BYTES,
        }
    }
}

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
enum RotationLayout {
    Wxyz,
    Xyzw,
}

impl RotationLayout {
    fn from_env() -> Self {
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

/// One fully decoded 3D Gaussian in the renderer's runtime conventions.
///
/// The value is fixed-size and owns no heap allocation. Positions, rotations,
/// and spherical harmonics have already been converted from input RDF space to
/// runtime RUF space. `sh_rest[..usize::from(sh_rest_len)]` contains the valid
/// channel-major non-DC coefficients; the unused tail is zeroed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecodedPlySplat {
    pub position_ruf: Vec3f,
    pub opacity_logit: f32,
    pub log_scale_xyz: [f32; 3],
    pub rotation_xyzw: [f32; 4],
    pub color_dc: [f32; 3],
    pub sh_rest: [f32; MAX_SH_REST_COEFFICIENTS],
    pub sh_rest_len: u8,
    pub sh_degree: u8,
}

impl DecodedPlySplat {
    /// Returns only the valid non-DC SH coefficients for this splat.
    pub fn sh_rest_coefficients(&self) -> &[f32] {
        let len = usize::from(self.sh_rest_len).min(MAX_SH_REST_COEFFICIENTS);
        &self.sh_rest[..len]
    }
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
    #[error(
        "invalid spherical-harmonics layout; f_rest_* indices must be unique and contiguous with exactly 9, 24, or 45 properties"
    )]
    InvalidShLayout,
    #[error("PLY resource limit exceeded for {resource}: requested {requested}, limit {limit}")]
    ResourceLimit {
        resource: &'static str,
        requested: usize,
        limit: usize,
    },
    #[error("PLY resource size overflow while computing {0}")]
    ResourceSizeOverflow(&'static str),
    #[error("failed to reserve memory for PLY {0}")]
    AllocationFailed(&'static str),
    #[error("incremental PLY decoder has already been finished")]
    IncrementalDecoderFinished,
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
            | Self::InvalidScene
            | Self::InvalidShLayout => ErrorCode::ParseFailed,
            Self::ResourceLimit { .. } | Self::ResourceSizeOverflow(_) => ErrorCode::Unsupported,
            Self::AllocationFailed(_) => ErrorCode::Internal,
            Self::IncrementalDecoderFinished => ErrorCode::InvalidArgument,
        }
    }
}

pub fn load_ply(path: &Path) -> Result<PlyLoadResult, PlyLoadError> {
    load_ply_with_limits(path, PlyLoadLimits::default())
}

/// Load and decode a PLY scene with explicit resource budgets.
pub fn load_ply_with_limits(
    path: &Path,
    limits: PlyLoadLimits,
) -> Result<PlyLoadResult, PlyLoadError> {
    let (file, input_len) = open_ply_file_with_limits(path, limits)?;
    let mut reader = BufReader::new(file);
    parse_ply_reader_with_limits(&mut reader, input_len, limits)
}

/// Stream a file-backed PLY and invoke `visitor` once for every decoded splat.
///
/// The file body is never read in full and no `SceneBuffers` are constructed.
/// Each callback receives an allocation-free fixed-size value in runtime RUF
/// conventions with complete SH0-SH3 data. If a later vertex is malformed,
/// callbacks for earlier valid vertices have already occurred and the error is
/// returned instead of a summary.
pub fn visit_ply_splats(
    path: &Path,
    visitor: impl FnMut(&DecodedPlySplat),
) -> Result<PlySceneSummary, PlyLoadError> {
    visit_ply_splats_with_limits(path, PlyLoadLimits::default(), visitor)
}

/// Stream decoded splats with the same explicit resource budgets as
/// [`load_ply_with_limits`].
pub fn visit_ply_splats_with_limits(
    path: &Path,
    limits: PlyLoadLimits,
    visitor: impl FnMut(&DecodedPlySplat),
) -> Result<PlySceneSummary, PlyLoadError> {
    let (file, input_len) = open_ply_file_with_limits(path, limits)?;
    let mut reader = BufReader::new(file);
    visit_ply_reader_with_limits(&mut reader, input_len, limits, visitor)
}

/// Visit every decoded splat in an in-memory PLY payload without constructing
/// `SceneBuffers` or allocating per splat.
///
/// This is the byte-backed counterpart of [`visit_ply_splats`]. Both paths use
/// the same vertex decode plan and ASCII/binary vertex decoders, so a given
/// payload produces bit-identical [`DecodedPlySplat`] values.
pub fn visit_ply_bytes_splats(
    input: &[u8],
    visitor: impl FnMut(&DecodedPlySplat),
) -> Result<PlySceneSummary, PlyLoadError> {
    visit_ply_bytes_splats_with_limits(input, PlyLoadLimits::default(), visitor)
}

/// Visit in-memory decoded splats with the same explicit resource budgets as
/// [`parse_ply_bytes_with_limits`].
pub fn visit_ply_bytes_splats_with_limits(
    input: &[u8],
    limits: PlyLoadLimits,
    mut visitor: impl FnMut(&DecodedPlySplat),
) -> Result<PlySceneSummary, PlyLoadError> {
    ensure_limit("input bytes", input.len(), limits.max_input_bytes)?;
    let (header, body) = split_header_body(input, limits)?;
    let indices = build_property_indices(&header)?;
    let (sh_degree, sh_rest_prop_indices) = infer_sh_rest_layout(&header)?;
    let has_sh_rest = sh_degree > 0 && sh_rest_prop_indices.is_some();
    let rest_stride = sh_rest_stride(sh_degree, has_sh_rest)?;
    let decode_plan = VertexDecodePlan::new(
        &header,
        &indices,
        sh_rest_prop_indices.as_deref(),
        sh_degree,
        RotationLayout::from_env(),
    )?;

    validate_body_before_allocation(&header, body)?;
    validate_decoded_scene_budget(&header, rest_stride, has_sh_rest, limits)?;

    match header.format {
        PlyFormat::Ascii => visit_ascii_body(&header, body, &decode_plan, &mut visitor)?,
        PlyFormat::BinaryLittleEndian => {
            visit_binary_body(&header, body, &decode_plan, Endian::Little, &mut visitor)?
        }
        PlyFormat::BinaryBigEndian => {
            visit_binary_body(&header, body, &decode_plan, Endian::Big, &mut visitor)?
        }
    }

    Ok(PlySceneSummary {
        gaussians: header.vertex_count,
        sh_degree,
        has_sh_rest,
    })
}

/// Allocation-bounded incremental PLY decoder for streamed transports.
///
/// Chunks may split the header, an ASCII row, or a binary vertex record at any
/// byte. The decoder retains only the unfinished header/row/record; decoded
/// splats are delivered immediately in source order. When a chunk completes
/// the header, [`Self::push`] returns its summary without decoding the body tail
/// from that same chunk. This gives the caller a transactional point at which
/// to allocate its final scene builder; call `push(&[], visitor)` once to drain
/// that retained tail.
///
/// Any error returned by [`Self::push`] or [`Self::finish`] is terminal. The
/// failing call may already have delivered complete splats before detecting a
/// later malformed vertex, so callers must discard the in-progress destination
/// after an error. Subsequent calls return
/// [`PlyLoadError::IncrementalDecoderFinished`] without invoking the visitor,
/// which prevents replaying already-published callbacks.
pub struct IncrementalPlyDecoder {
    limits: PlyLoadLimits,
    buffer: Vec<u8>,
    body: Option<IncrementalPlyBody>,
    summary: Option<PlySceneSummary>,
    decoded_vertices: usize,
    total_input_bytes: usize,
    peak_buffered_bytes: usize,
    state: IncrementalPlyDecoderState,
}

#[derive(PartialEq, Eq)]
enum IncrementalPlyDecoderState {
    Active,
    Finished,
    Failed,
}

impl Default for IncrementalPlyDecoder {
    fn default() -> Self {
        Self::new(PlyLoadLimits::default())
    }
}

impl IncrementalPlyDecoder {
    pub fn new(limits: PlyLoadLimits) -> Self {
        Self {
            limits,
            buffer: Vec::new(),
            body: None,
            summary: None,
            decoded_vertices: 0,
            total_input_bytes: 0,
            peak_buffered_bytes: 0,
            state: IncrementalPlyDecoderState::Active,
        }
    }

    /// Feed one transport chunk. `Some(summary)` is returned exactly once,
    /// when the complete header has been validated.
    pub fn push(
        &mut self,
        input: &[u8],
        mut visitor: impl FnMut(&DecodedPlySplat),
    ) -> Result<Option<PlySceneSummary>, PlyLoadError> {
        if self.state != IncrementalPlyDecoderState::Active {
            return Err(PlyLoadError::IncrementalDecoderFinished);
        }
        let result = self.push_active(input, &mut visitor);
        if result.is_err() {
            self.state = IncrementalPlyDecoderState::Failed;
        }
        result
    }

    fn push_active(
        &mut self,
        input: &[u8],
        visitor: &mut impl FnMut(&DecodedPlySplat),
    ) -> Result<Option<PlySceneSummary>, PlyLoadError> {
        self.total_input_bytes = self
            .total_input_bytes
            .checked_add(input.len())
            .ok_or(PlyLoadError::ResourceSizeOverflow("input bytes"))?;
        ensure_limit(
            "input bytes",
            self.total_input_bytes,
            self.limits.max_input_bytes,
        )?;
        if self.body.is_none() {
            // A transport is free to deliver the short ASCII header together
            // with a very large binary-body prefix (Chrome commonly uses a
            // 2 MiB first chunk). Apply the header limit only to bytes that
            // can still belong to the header, not to that whole transport
            // chunk.
            let header_room = self
                .limits
                .max_header_bytes
                .saturating_sub(self.buffer.len());
            let header_input_len = input.len().min(header_room);
            self.extend_buffer(&input[..header_input_len])?;
            let Some(header_end) = complete_header_end(&self.buffer, self.limits)? else {
                if header_input_len < input.len() {
                    return Err(PlyLoadError::ResourceLimit {
                        resource: "header bytes",
                        requested: self.limits.max_header_bytes.saturating_add(1),
                        limit: self.limits.max_header_bytes,
                    });
                }
                return Ok(None);
            };
            let summary = self.initialize_body(header_end)?;
            // Preserve the remainder until the caller has allocated its exact
            // destination, then `push(&[], visitor)` drains it as documented.
            self.extend_buffer(&input[header_input_len..])?;
            // Deliberately leave the body tail untouched until the caller has
            // created its final exact-count destination.
            return Ok(Some(summary));
        }

        self.extend_buffer(input)?;
        self.decode_available(false, visitor)?;
        Ok(None)
    }

    /// Finish the stream, decoding a final non-newline-terminated ASCII row and
    /// rejecting truncated bodies. The returned count is always the header's
    /// exact vertex count.
    pub fn finish(
        &mut self,
        mut visitor: impl FnMut(&DecodedPlySplat),
    ) -> Result<PlySceneSummary, PlyLoadError> {
        if self.state != IncrementalPlyDecoderState::Active {
            return Err(PlyLoadError::IncrementalDecoderFinished);
        }
        let result = self.finish_active(&mut visitor);
        self.state = if result.is_ok() {
            IncrementalPlyDecoderState::Finished
        } else {
            IncrementalPlyDecoderState::Failed
        };
        result
    }

    fn finish_active(
        &mut self,
        visitor: &mut impl FnMut(&DecodedPlySplat),
    ) -> Result<PlySceneSummary, PlyLoadError> {
        if self.body.is_none() {
            // Permit a zero-body PLY whose end_header is the final input line.
            if let Some(header_end) = complete_header_end_at_eof(&self.buffer, self.limits)? {
                self.initialize_body(header_end)?;
            }
        }
        if self.body.is_none() {
            return Err(PlyLoadError::MalformedHeader);
        }
        self.decode_available(true, visitor)?;
        let expected = self.summary.ok_or(PlyLoadError::MalformedHeader)?.gaussians;
        if self.decoded_vertices != expected {
            return Err(PlyLoadError::VertexCountMismatch);
        }
        self.buffer.clear();
        self.summary.ok_or(PlyLoadError::MalformedHeader)
    }

    pub fn summary(&self) -> Option<PlySceneSummary> {
        self.summary
    }

    pub fn decoded_vertices(&self) -> usize {
        self.decoded_vertices
    }

    pub fn total_input_bytes(&self) -> usize {
        self.total_input_bytes
    }

    /// Largest undecoded transport fragment retained by this decoder.
    pub fn peak_buffered_bytes(&self) -> usize {
        self.peak_buffered_bytes
    }

    fn extend_buffer(&mut self, input: &[u8]) -> Result<(), PlyLoadError> {
        let next_len = self.buffer.len().checked_add(input.len()).ok_or(
            PlyLoadError::ResourceSizeOverflow("incremental buffer bytes"),
        )?;
        if self.body.is_none() {
            ensure_limit("header bytes", next_len, self.limits.max_header_bytes)?;
        }
        self.buffer
            .try_reserve(input.len())
            .map_err(|_| PlyLoadError::AllocationFailed("incremental input buffer"))?;
        self.buffer.extend_from_slice(input);
        self.peak_buffered_bytes = self.peak_buffered_bytes.max(self.buffer.len());
        Ok(())
    }

    fn initialize_body(&mut self, header_end: usize) -> Result<PlySceneSummary, PlyLoadError> {
        let header_text = std::str::from_utf8(&self.buffer[..header_end])
            .map_err(|_| PlyLoadError::MalformedHeader)?;
        let header = parse_header_text(header_text, self.limits)?;
        let indices = build_property_indices(&header)?;
        let (sh_degree, sh_rest_prop_indices) = infer_sh_rest_layout(&header)?;
        let has_sh_rest = sh_degree > 0 && sh_rest_prop_indices.is_some();
        let rest_stride = sh_rest_stride(sh_degree, has_sh_rest)?;
        validate_decoded_scene_budget(&header, rest_stride, has_sh_rest, self.limits)?;
        let decode_plan = VertexDecodePlan::new(
            &header,
            &indices,
            sh_rest_prop_indices.as_deref(),
            sh_degree,
            RotationLayout::from_env(),
        )?;
        let body = IncrementalPlyBody::new(header, decode_plan)?;
        let summary = PlySceneSummary {
            gaussians: body.header.vertex_count,
            sh_degree,
            has_sh_rest,
        };
        self.buffer.drain(..header_end);
        self.body = Some(body);
        self.summary = Some(summary);
        Ok(summary)
    }

    fn decode_available(
        &mut self,
        final_chunk: bool,
        visitor: &mut impl FnMut(&DecodedPlySplat),
    ) -> Result<(), PlyLoadError> {
        let body = self.body.as_ref().ok_or(PlyLoadError::MalformedHeader)?;
        let remaining = body
            .header
            .vertex_count
            .saturating_sub(self.decoded_vertices);
        if remaining == 0 {
            self.buffer.clear();
            return Ok(());
        }

        let (consumed, decoded) =
            body.decode_prefix(&self.buffer, remaining, final_chunk, visitor)?;
        self.decoded_vertices = self
            .decoded_vertices
            .checked_add(decoded)
            .ok_or(PlyLoadError::ResourceSizeOverflow("decoded vertex count"))?;
        self.buffer.drain(..consumed);
        if self.decoded_vertices == body.header.vertex_count {
            // Other PLY elements, when present, are outside this Gaussian
            // loader's vertex contract and must not remain resident.
            self.buffer.clear();
        }
        Ok(())
    }
}

struct IncrementalPlyBody {
    header: PlyHeader,
    decode_plan: VertexDecodePlan,
    binary_layout: Option<(usize, Vec<usize>, Endian)>,
}

impl IncrementalPlyBody {
    fn new(header: PlyHeader, decode_plan: VertexDecodePlan) -> Result<Self, PlyLoadError> {
        let binary_layout = match header.format {
            PlyFormat::Ascii => None,
            PlyFormat::BinaryLittleEndian => {
                let (stride, offsets) = compute_vertex_layout(&header)?;
                if stride == 0 && header.vertex_count > 0 {
                    return Err(PlyLoadError::MalformedHeader);
                }
                Some((stride, offsets, Endian::Little))
            }
            PlyFormat::BinaryBigEndian => {
                let (stride, offsets) = compute_vertex_layout(&header)?;
                if stride == 0 && header.vertex_count > 0 {
                    return Err(PlyLoadError::MalformedHeader);
                }
                Some((stride, offsets, Endian::Big))
            }
        };
        Ok(Self {
            header,
            decode_plan,
            binary_layout,
        })
    }

    fn decode_prefix(
        &self,
        input: &[u8],
        remaining: usize,
        final_chunk: bool,
        visitor: &mut impl FnMut(&DecodedPlySplat),
    ) -> Result<(usize, usize), PlyLoadError> {
        let Some((stride, offsets, endian)) = self.binary_layout.as_ref() else {
            return self.decode_ascii_prefix(input, remaining, final_chunk, visitor);
        };
        let available = if *stride == 0 {
            0
        } else {
            input.len() / *stride
        };
        let records = available.min(remaining);
        let consumed = records
            .checked_mul(*stride)
            .ok_or(PlyLoadError::ResourceSizeOverflow("binary body bytes"))?;
        for record in input[..consumed].chunks_exact(*stride) {
            let splat =
                decode_binary_vertex(record, &self.header, offsets, &self.decode_plan, *endian)?;
            visitor(&splat);
        }
        Ok((consumed, records))
    }

    fn decode_ascii_prefix(
        &self,
        input: &[u8],
        remaining: usize,
        final_chunk: bool,
        visitor: &mut impl FnMut(&DecodedPlySplat),
    ) -> Result<(usize, usize), PlyLoadError> {
        let mut cursor = 0_usize;
        let mut decoded = 0_usize;
        while decoded < remaining && cursor < input.len() {
            let tail = &input[cursor..];
            let newline = tail.iter().position(|&byte| byte == b'\n');
            let (line_end, next_cursor) = match newline {
                Some(offset) => (cursor + offset, cursor + offset + 1),
                None if final_chunk => (input.len(), input.len()),
                None => break,
            };
            let line = std::str::from_utf8(&input[cursor..line_end])
                .map_err(|_| PlyLoadError::UnsupportedFormat)?;
            cursor = next_cursor;
            let row = line.trim();
            if row.is_empty() || row.starts_with("comment") {
                continue;
            }
            let splat = decode_ascii_vertex(row, &self.header, &self.decode_plan)?;
            visitor(&splat);
            decoded += 1;
        }
        Ok((cursor, decoded))
    }
}

pub fn load_ply_summary(path: &Path) -> Result<PlySceneSummary, PlyLoadError> {
    load_ply_summary_with_limits(path, PlyLoadLimits::default())
}

/// Read PLY header metadata without constructing full scene buffers.
pub fn load_ply_summary_with_limits(
    path: &Path,
    limits: PlyLoadLimits,
) -> Result<PlySceneSummary, PlyLoadError> {
    let (file, input_len) = open_ply_file_with_limits(path, limits)?;
    let mut reader = BufReader::new(file);
    let (header, header_bytes) = read_header_from_reader(&mut reader, limits)?;
    validate_stream_body_before_allocation(&header, input_len, header_bytes)?;
    summary_from_header(&header)
}

/// Read summary metadata without decoding numeric vertex attributes. The body
/// shape is validated before the summary can be used for downstream capacity
/// allocation.
pub fn parse_ply_bytes_summary(input: &[u8]) -> Result<PlySceneSummary, PlyLoadError> {
    parse_ply_bytes_summary_with_limits(input, PlyLoadLimits::default())
}

/// Read in-memory PLY summary metadata with explicit resource budgets.
pub fn parse_ply_bytes_summary_with_limits(
    input: &[u8],
    limits: PlyLoadLimits,
) -> Result<PlySceneSummary, PlyLoadError> {
    ensure_limit("input bytes", input.len(), limits.max_input_bytes)?;
    let (header, body) = split_header_body(input, limits)?;
    validate_body_before_allocation(&header, body)?;
    summary_from_header(&header)
}

pub fn parse_ply_text(input: &str) -> Result<PlyLoadResult, PlyLoadError> {
    parse_ply_text_with_limits(input, PlyLoadLimits::default())
}

/// Decode an ASCII PLY payload with explicit resource budgets.
pub fn parse_ply_text_with_limits(
    input: &str,
    limits: PlyLoadLimits,
) -> Result<PlyLoadResult, PlyLoadError> {
    // `parse_ply_text` is intended for textual PLY payloads; reject binary headers early to avoid
    // mis-parsing an ASCII body as binary bytes.
    let (header, _) = split_header_body(input.as_bytes(), limits)?;
    if header.format != PlyFormat::Ascii {
        return Err(PlyLoadError::UnsupportedFormat);
    }
    parse_ply_bytes_with_limits(input.as_bytes(), limits)
}

pub fn parse_ply_bytes(input: &[u8]) -> Result<PlyLoadResult, PlyLoadError> {
    parse_ply_bytes_with_limits(input, PlyLoadLimits::default())
}

/// Decode a PLY payload with explicit resource budgets.
pub fn parse_ply_bytes_with_limits(
    input: &[u8],
    limits: PlyLoadLimits,
) -> Result<PlyLoadResult, PlyLoadError> {
    ensure_limit("input bytes", input.len(), limits.max_input_bytes)?;
    let (header, body) = split_header_body(input, limits)?;
    let indices = build_property_indices(&header)?;

    let (sh_degree, sh_rest_prop_indices) = infer_sh_rest_layout(&header)?;
    let has_sh_rest = sh_degree > 0 && sh_rest_prop_indices.is_some();

    let rest_stride = sh_rest_stride(sh_degree, has_sh_rest)?;
    let decode_plan = VertexDecodePlan::new(
        &header,
        &indices,
        sh_rest_prop_indices.as_deref(),
        sh_degree,
        RotationLayout::from_env(),
    )?;

    validate_body_before_allocation(&header, body)?;
    let mut scene = allocate_scene(&header, sh_degree, rest_stride, has_sh_rest, limits)?;

    match header.format {
        PlyFormat::Ascii => visit_ascii_body(&header, body, &decode_plan, &mut |splat| {
            push_decoded_splat(&mut scene, splat);
        })?,
        PlyFormat::BinaryLittleEndian => {
            visit_binary_body(&header, body, &decode_plan, Endian::Little, &mut |splat| {
                push_decoded_splat(&mut scene, splat)
            })?
        }
        PlyFormat::BinaryBigEndian => {
            visit_binary_body(&header, body, &decode_plan, Endian::Big, &mut |splat| {
                push_decoded_splat(&mut scene, splat)
            })?
        }
    }

    finish_scene(scene)
}

fn open_ply_file_with_limits(
    path: &Path,
    limits: PlyLoadLimits,
) -> Result<(File, usize), PlyLoadError> {
    let file = File::open(path).map_err(|_| PlyLoadError::Io)?;
    let input_len = file_len(&file)?;
    ensure_limit("input bytes", input_len, limits.max_input_bytes)?;
    Ok((file, input_len))
}

fn file_len(file: &File) -> Result<usize, PlyLoadError> {
    let metadata = file.metadata().map_err(|_| PlyLoadError::Io)?;
    usize::try_from(metadata.len()).map_err(|_| PlyLoadError::ResourceSizeOverflow("input bytes"))
}

fn parse_ply_reader_with_limits<R: BufRead>(
    reader: &mut R,
    input_len: usize,
    limits: PlyLoadLimits,
) -> Result<PlyLoadResult, PlyLoadError> {
    let (header, header_bytes) = read_header_from_reader(reader, limits)?;
    let indices = build_property_indices(&header)?;
    let (sh_degree, sh_rest_prop_indices) = infer_sh_rest_layout(&header)?;
    let has_sh_rest = sh_degree > 0 && sh_rest_prop_indices.is_some();
    let rest_stride = sh_rest_stride(sh_degree, has_sh_rest)?;
    let decode_plan = VertexDecodePlan::new(
        &header,
        &indices,
        sh_rest_prop_indices.as_deref(),
        sh_degree,
        RotationLayout::from_env(),
    )?;

    validate_stream_body_before_allocation(&header, input_len, header_bytes)?;
    let mut scene = allocate_scene(&header, sh_degree, rest_stride, has_sh_rest, limits)?;

    match header.format {
        PlyFormat::Ascii => visit_ascii_reader(reader, &header, &decode_plan, &mut |splat| {
            push_decoded_splat(&mut scene, splat);
        })?,
        PlyFormat::BinaryLittleEndian => visit_binary_reader(
            reader,
            &header,
            &decode_plan,
            Endian::Little,
            &mut |splat| push_decoded_splat(&mut scene, splat),
        )?,
        PlyFormat::BinaryBigEndian => {
            visit_binary_reader(reader, &header, &decode_plan, Endian::Big, &mut |splat| {
                push_decoded_splat(&mut scene, splat)
            })?
        }
    }

    finish_scene(scene)
}

fn visit_ply_reader_with_limits<R, F>(
    reader: &mut R,
    input_len: usize,
    limits: PlyLoadLimits,
    mut visitor: F,
) -> Result<PlySceneSummary, PlyLoadError>
where
    R: BufRead,
    F: FnMut(&DecodedPlySplat),
{
    let (header, header_bytes) = read_header_from_reader(reader, limits)?;
    let indices = build_property_indices(&header)?;
    let (sh_degree, sh_rest_prop_indices) = infer_sh_rest_layout(&header)?;
    let has_sh_rest = sh_degree > 0 && sh_rest_prop_indices.is_some();
    let rest_stride = sh_rest_stride(sh_degree, has_sh_rest)?;
    let decode_plan = VertexDecodePlan::new(
        &header,
        &indices,
        sh_rest_prop_indices.as_deref(),
        sh_degree,
        RotationLayout::from_env(),
    )?;

    validate_stream_body_before_allocation(&header, input_len, header_bytes)?;
    validate_decoded_scene_budget(&header, rest_stride, has_sh_rest, limits)?;

    match header.format {
        PlyFormat::Ascii => visit_ascii_reader(reader, &header, &decode_plan, &mut visitor)?,
        PlyFormat::BinaryLittleEndian => {
            visit_binary_reader(reader, &header, &decode_plan, Endian::Little, &mut visitor)?
        }
        PlyFormat::BinaryBigEndian => {
            visit_binary_reader(reader, &header, &decode_plan, Endian::Big, &mut visitor)?
        }
    }

    Ok(PlySceneSummary {
        gaussians: header.vertex_count,
        sh_degree,
        has_sh_rest,
    })
}

fn finish_scene(scene: SceneBuffers) -> Result<PlyLoadResult, PlyLoadError> {
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

fn push_decoded_splat(scene: &mut SceneBuffers, splat: &DecodedPlySplat) {
    scene.positions.push(splat.position_ruf);
    scene.opacity.push(splat.opacity_logit);
    scene.scale_xyz.push(splat.log_scale_xyz);
    scene.rotation_xyzw.push(splat.rotation_xyzw);
    scene.color_dc.push(splat.color_dc);

    if let Some(rest) = scene.sh_rest.as_mut() {
        rest.extend_from_slice(splat.sh_rest_coefficients());
    }
}

fn validate_stream_body_before_allocation(
    header: &PlyHeader,
    input_len: usize,
    header_bytes: usize,
) -> Result<(), PlyLoadError> {
    let available_body_bytes = input_len
        .checked_sub(header_bytes)
        .ok_or(PlyLoadError::ResourceSizeOverflow("input body bytes"))?;

    match header.format {
        PlyFormat::Ascii => {
            // Every non-empty ASCII vertex row needs at least one byte. This cheap
            // lower bound rejects forged huge counts before reserving scene memory;
            // exact row validation remains streaming below.
            if available_body_bytes < header.vertex_count {
                return Err(PlyLoadError::VertexCountMismatch);
            }
        }
        PlyFormat::BinaryLittleEndian | PlyFormat::BinaryBigEndian => {
            let (stride, _) = compute_vertex_layout(header)?;
            let required_bytes = header
                .vertex_count
                .checked_mul(stride)
                .ok_or(PlyLoadError::ResourceSizeOverflow("binary body bytes"))?;
            if available_body_bytes < required_bytes {
                return Err(PlyLoadError::VertexCountMismatch);
            }
        }
    }

    Ok(())
}

fn ensure_limit(
    resource: &'static str,
    requested: usize,
    limit: usize,
) -> Result<(), PlyLoadError> {
    if requested > limit {
        Err(PlyLoadError::ResourceLimit {
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
) -> Result<Vec<T>, PlyLoadError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(capacity)
        .map_err(|_| PlyLoadError::AllocationFailed(resource))?;
    Ok(values)
}

fn allocate_scene(
    header: &PlyHeader,
    sh_degree: u8,
    rest_stride: usize,
    has_sh_rest: bool,
    limits: PlyLoadLimits,
) -> Result<SceneBuffers, PlyLoadError> {
    let rest_capacity = validate_decoded_scene_budget(header, rest_stride, has_sh_rest, limits)?;

    Ok(SceneBuffers {
        positions: try_vec_with_capacity("positions", header.vertex_count)?,
        opacity: try_vec_with_capacity("opacity", header.vertex_count)?,
        scale_xyz: try_vec_with_capacity("scales", header.vertex_count)?,
        rotation_xyzw: try_vec_with_capacity("rotations", header.vertex_count)?,
        color_dc: try_vec_with_capacity("DC colors", header.vertex_count)?,
        sh_degree,
        sh_rest: if has_sh_rest {
            Some(try_vec_with_capacity("SH coefficients", rest_capacity)?)
        } else {
            None
        },
    })
}

fn validate_decoded_scene_budget(
    header: &PlyHeader,
    rest_stride: usize,
    has_sh_rest: bool,
    limits: PlyLoadLimits,
) -> Result<usize, PlyLoadError> {
    let base_stride = size_of::<Vec3f>()
        .checked_add(size_of::<f32>())
        .and_then(|value| value.checked_add(size_of::<[f32; 3]>() * 2))
        .and_then(|value| value.checked_add(size_of::<[f32; 4]>()))
        .ok_or(PlyLoadError::ResourceSizeOverflow("base scene stride"))?;
    let base_bytes = header
        .vertex_count
        .checked_mul(base_stride)
        .ok_or(PlyLoadError::ResourceSizeOverflow("base scene bytes"))?;
    let rest_capacity =
        if has_sh_rest {
            header.vertex_count.checked_mul(rest_stride).ok_or(
                PlyLoadError::ResourceSizeOverflow("SH coefficient capacity"),
            )?
        } else {
            0
        };
    let rest_bytes = rest_capacity
        .checked_mul(size_of::<f32>())
        .ok_or(PlyLoadError::ResourceSizeOverflow("SH coefficient bytes"))?;
    let scene_bytes = base_bytes
        .checked_add(rest_bytes)
        .ok_or(PlyLoadError::ResourceSizeOverflow("total scene bytes"))?;
    ensure_limit("decoded scene bytes", scene_bytes, limits.max_scene_bytes)?;
    Ok(rest_capacity)
}

fn validate_body_before_allocation(header: &PlyHeader, body: &[u8]) -> Result<(), PlyLoadError> {
    match header.format {
        PlyFormat::Ascii => {
            let body_text =
                std::str::from_utf8(body).map_err(|_| PlyLoadError::UnsupportedFormat)?;
            let available_rows = body_text
                .lines()
                .filter(|line| {
                    let trimmed = line.trim();
                    !trimmed.is_empty() && !trimmed.starts_with("comment")
                })
                .take(header.vertex_count)
                .count();
            if available_rows < header.vertex_count {
                return Err(PlyLoadError::VertexCountMismatch);
            }
        }
        PlyFormat::BinaryLittleEndian | PlyFormat::BinaryBigEndian => {
            let (stride, _) = compute_vertex_layout(header)?;
            let required_bytes = header
                .vertex_count
                .checked_mul(stride)
                .ok_or(PlyLoadError::ResourceSizeOverflow("binary body bytes"))?;
            if body.len() < required_bytes {
                return Err(PlyLoadError::VertexCountMismatch);
            }
        }
    }
    Ok(())
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

struct VertexDecodePlan {
    fields: Vec<DecodedField>,
    sh_degree: u8,
    sh_rest_len: u8,
    rotation_layout: RotationLayout,
}

impl VertexDecodePlan {
    fn new(
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

fn visit_ascii_body<F>(
    header: &PlyHeader,
    body: &[u8],
    decode_plan: &VertexDecodePlan,
    visitor: &mut F,
) -> Result<(), PlyLoadError>
where
    F: FnMut(&DecodedPlySplat),
{
    let body_text = std::str::from_utf8(body).map_err(|_| PlyLoadError::UnsupportedFormat)?;
    let mut lines = body_text.lines();

    for _ in 0..header.vertex_count {
        let mut row: Option<&str> = None;
        for line in lines.by_ref() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("comment") {
                continue;
            }
            row = Some(trimmed);
            break;
        }

        let row = row.ok_or(PlyLoadError::VertexCountMismatch)?;
        let splat = decode_ascii_vertex(row, header, decode_plan)?;
        visitor(&splat);
    }

    Ok(())
}

fn visit_ascii_reader<R, F>(
    reader: &mut R,
    header: &PlyHeader,
    decode_plan: &VertexDecodePlan,
    visitor: &mut F,
) -> Result<(), PlyLoadError>
where
    R: BufRead,
    F: FnMut(&DecodedPlySplat),
{
    let mut line = Vec::new();

    for _ in 0..header.vertex_count {
        let row = loop {
            let bytes_read = read_ascii_line(reader, &mut line)?;
            if bytes_read == 0 {
                return Err(PlyLoadError::VertexCountMismatch);
            }

            let line_text =
                std::str::from_utf8(&line).map_err(|_| PlyLoadError::UnsupportedFormat)?;
            let trimmed = line_text.trim();
            if !trimmed.is_empty() && !trimmed.starts_with("comment") {
                break trimmed;
            }
        };

        let splat = decode_ascii_vertex(row, header, decode_plan)?;
        visitor(&splat);
    }

    Ok(())
}

fn read_ascii_line<R: BufRead>(reader: &mut R, line: &mut Vec<u8>) -> Result<usize, PlyLoadError> {
    line.clear();
    loop {
        let (take, has_newline) = {
            let available = reader.fill_buf().map_err(|_| PlyLoadError::Io)?;
            if available.is_empty() {
                return Ok(line.len());
            }
            match available.iter().position(|&byte| byte == b'\n') {
                Some(position) => (position + 1, true),
                None => (available.len(), false),
            }
        };

        line.len()
            .checked_add(take)
            .ok_or(PlyLoadError::ResourceSizeOverflow("ASCII vertex row bytes"))?;
        line.try_reserve(take)
            .map_err(|_| PlyLoadError::AllocationFailed("ASCII vertex row"))?;
        {
            let available = reader.fill_buf().map_err(|_| PlyLoadError::Io)?;
            line.extend_from_slice(&available[..take]);
        }
        reader.consume(take);
        if has_newline {
            return Ok(line.len());
        }
    }
}

fn decode_ascii_vertex(
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
enum Endian {
    Little,
    Big,
}

fn visit_binary_body<F>(
    header: &PlyHeader,
    body: &[u8],
    decode_plan: &VertexDecodePlan,
    endian: Endian,
    visitor: &mut F,
) -> Result<(), PlyLoadError>
where
    F: FnMut(&DecodedPlySplat),
{
    let (stride, offsets) = compute_vertex_layout(header)?;
    let required_bytes = header
        .vertex_count
        .checked_mul(stride)
        .ok_or(PlyLoadError::ResourceSizeOverflow("binary body bytes"))?;
    if body.len() < required_bytes {
        return Err(PlyLoadError::VertexCountMismatch);
    }

    for vertex_index in 0..header.vertex_count {
        let base = vertex_index * stride;
        let record = &body[base..base + stride];
        let splat = decode_binary_vertex(record, header, &offsets, decode_plan, endian)?;
        visitor(&splat);
    }

    Ok(())
}

fn visit_binary_reader<R, F>(
    reader: &mut R,
    header: &PlyHeader,
    decode_plan: &VertexDecodePlan,
    endian: Endian,
    visitor: &mut F,
) -> Result<(), PlyLoadError>
where
    R: Read,
    F: FnMut(&DecodedPlySplat),
{
    let (stride, offsets) = compute_vertex_layout(header)?;
    if header.vertex_count == 0 {
        return Ok(());
    }
    if stride == 0 {
        return Err(PlyLoadError::MalformedHeader);
    }

    let records_per_batch = (MIB / stride).max(1).min(header.vertex_count);
    let batch_capacity =
        records_per_batch
            .checked_mul(stride)
            .ok_or(PlyLoadError::ResourceSizeOverflow(
                "binary streaming batch bytes",
            ))?;
    let mut batch = try_vec_with_capacity("binary streaming batch", batch_capacity)?;
    batch.resize(batch_capacity, 0);

    let mut remaining = header.vertex_count;
    while remaining > 0 {
        let current_records = remaining.min(records_per_batch);
        let current_bytes =
            current_records
                .checked_mul(stride)
                .ok_or(PlyLoadError::ResourceSizeOverflow(
                    "binary streaming batch bytes",
                ))?;
        if let Err(error) = reader.read_exact(&mut batch[..current_bytes]) {
            return if error.kind() == ErrorKind::UnexpectedEof {
                Err(PlyLoadError::VertexCountMismatch)
            } else {
                Err(PlyLoadError::Io)
            };
        }

        for record in batch[..current_bytes].chunks_exact(stride) {
            let splat = decode_binary_vertex(record, header, &offsets, decode_plan, endian)?;
            visitor(&splat);
        }
        remaining -= current_records;
    }

    Ok(())
}

fn decode_binary_vertex(
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

fn compute_vertex_layout(header: &PlyHeader) -> Result<(usize, Vec<usize>), PlyLoadError> {
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Cursor;
    use std::mem::{needs_drop, size_of};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use gsplat_core::ErrorCode;

    use super::{
        DecodedPlySplat, Endian, GIB, IncrementalPlyDecoder, MIB, OPACITY_LOGIT_LIMIT, PlyFormat,
        PlyHeader, PlyLoadError, PlyLoadLimits, allocate_scene, load_ply, load_ply_summary,
        load_ply_with_limits, parse_ply_bytes, parse_ply_bytes_summary,
        parse_ply_bytes_with_limits, parse_ply_text, parse_ply_text_with_limits,
        read_header_from_reader, validate_decoded_scene_budget, visit_ply_bytes_splats,
        visit_ply_bytes_splats_with_limits, visit_ply_reader_with_limits, visit_ply_splats,
        visit_ply_splats_with_limits,
    };

    static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct TestFile {
        path: PathBuf,
    }

    impl TestFile {
        fn new(label: &str, bytes: &[u8]) -> Self {
            let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "gsplat-io-ply-{label}-{}-{sequence}.ply",
                std::process::id()
            ));
            fs::write(&path, bytes).unwrap();
            Self { path }
        }
    }

    impl Drop for TestFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
        }
    }

    const VALID_PLY: &str = "ply\nformat ascii 1.0\nelement vertex 1\nproperty float x\nproperty float y\nproperty float z\nproperty float opacity\nproperty float scale_0\nproperty float scale_1\nproperty float scale_2\nproperty float rot_0\nproperty float rot_1\nproperty float rot_2\nproperty float rot_3\nproperty float f_dc_0\nproperty float f_dc_1\nproperty float f_dc_2\nend_header\n0.0 0.1 1.0 0.9 1.0 1.1 1.2 1.0 0.0 0.0 0.0 0.2 0.3 0.4\n";
    const VALID_PLY_NON_IDENTITY_QUAT: &str = "ply\nformat ascii 1.0\nelement vertex 1\nproperty float x\nproperty float y\nproperty float z\nproperty float opacity\nproperty float scale_0\nproperty float scale_1\nproperty float scale_2\nproperty float rot_0\nproperty float rot_1\nproperty float rot_2\nproperty float rot_3\nproperty float f_dc_0\nproperty float f_dc_1\nproperty float f_dc_2\nend_header\n0.0 0.1 1.0 0.9 1.0 1.1 1.2 0.9 0.2 0.3 0.4 0.2 0.3 0.4\n";
    const PLY_INCOMPLETE_SH_REST: &str = "ply\nformat ascii 1.0\nelement vertex 1\nproperty float x\nproperty float y\nproperty float z\nproperty float opacity\nproperty float scale_0\nproperty float scale_1\nproperty float scale_2\nproperty float rot_0\nproperty float rot_1\nproperty float rot_2\nproperty float rot_3\nproperty float f_dc_0\nproperty float f_dc_1\nproperty float f_dc_2\nproperty float f_rest_0\nend_header\n0.0 0.1 1.0 0.9 1.0 1.1 1.2 1.0 0.0 0.0 0.0 0.2 0.3 0.4 0.125\n";
    const PLY_WITH_SH_REST_DEG1: &str = "ply\nformat ascii 1.0\nelement vertex 1\nproperty float x\nproperty float y\nproperty float z\nproperty float opacity\nproperty float scale_0\nproperty float scale_1\nproperty float scale_2\nproperty float rot_0\nproperty float rot_1\nproperty float rot_2\nproperty float rot_3\nproperty float f_dc_0\nproperty float f_dc_1\nproperty float f_dc_2\nproperty float f_rest_0\nproperty float f_rest_1\nproperty float f_rest_2\nproperty float f_rest_3\nproperty float f_rest_4\nproperty float f_rest_5\nproperty float f_rest_6\nproperty float f_rest_7\nproperty float f_rest_8\nend_header\n0.0 0.1 1.0 0.9 1.0 1.1 1.2 1.0 0.0 0.0 0.0 0.2 0.3 0.4 0.01 0.02 0.03 0.04 0.05 0.06 0.07 0.08 0.09\n";

    fn ascii_ply_with_sh_rest_count(rest_count: usize) -> String {
        let (header, body) = VALID_PLY.split_once("end_header\n").unwrap();
        let properties = (0..rest_count)
            .map(|index| format!("property float f_rest_{index}\n"))
            .collect::<String>();
        let values = (0..rest_count)
            .map(|index| format!("{}", (index + 1) as f32 / 100.0))
            .collect::<Vec<_>>()
            .join(" ");
        format!("{header}{properties}end_header\n{} {values}\n", body.trim())
    }

    fn binary_ply(rest_count: usize, vertex_count: usize, endian: Endian) -> Vec<u8> {
        let format = match endian {
            Endian::Little => "binary_little_endian",
            Endian::Big => "binary_big_endian",
        };
        let rest_properties = (0..rest_count)
            .map(|index| format!("property float f_rest_{index}\n"))
            .collect::<String>();
        let header = format!(
            "ply\nformat {format} 1.0\nelement vertex {vertex_count}\nproperty float x\nproperty float y\nproperty float z\nproperty float opacity\nproperty float scale_0\nproperty float scale_1\nproperty float scale_2\nproperty float rot_0\nproperty float rot_1\nproperty float rot_2\nproperty float rot_3\nproperty float f_dc_0\nproperty float f_dc_1\nproperty float f_dc_2\n{rest_properties}end_header\n"
        );
        let stride = (14 + rest_count) * size_of::<f32>();
        let body_bytes = vertex_count.checked_mul(stride).unwrap();
        let mut bytes = Vec::with_capacity(header.len().checked_add(body_bytes).unwrap());
        bytes.extend_from_slice(header.as_bytes());

        for vertex_index in 0..vertex_count {
            let base = [
                vertex_index as f32,
                0.25,
                1.5,
                0.9,
                1.0,
                1.1,
                1.2,
                0.9,
                0.2,
                0.3,
                0.4,
                0.5,
                0.6,
                0.7,
            ];
            for value in base
                .into_iter()
                .chain((0..rest_count).map(|index| (index + 1) as f32 / 100.0))
            {
                match endian {
                    Endian::Little => bytes.extend_from_slice(&value.to_le_bytes()),
                    Endian::Big => bytes.extend_from_slice(&value.to_be_bytes()),
                }
            }
        }
        bytes
    }

    fn assert_f32_bits_eq(actual: f32, expected: f32) {
        assert_eq!(actual.to_bits(), expected.to_bits());
    }

    fn assert_splat_matches_scene(
        splat: &DecodedPlySplat,
        loaded: &super::PlyLoadResult,
        index: usize,
    ) {
        let position = loaded.scene.positions[index];
        assert_f32_bits_eq(splat.position_ruf.x, position.x);
        assert_f32_bits_eq(splat.position_ruf.y, position.y);
        assert_f32_bits_eq(splat.position_ruf.z, position.z);
        assert_f32_bits_eq(splat.opacity_logit, loaded.scene.opacity[index]);
        for (actual, expected) in splat
            .log_scale_xyz
            .iter()
            .zip(loaded.scene.scale_xyz[index])
        {
            assert_f32_bits_eq(*actual, expected);
        }
        for (actual, expected) in splat
            .rotation_xyzw
            .iter()
            .zip(loaded.scene.rotation_xyzw[index])
        {
            assert_f32_bits_eq(*actual, expected);
        }
        for (actual, expected) in splat.color_dc.iter().zip(loaded.scene.color_dc[index]) {
            assert_f32_bits_eq(*actual, expected);
        }

        assert_eq!(splat.sh_degree, loaded.scene.sh_degree);
        let rest_len = usize::from(splat.sh_rest_len);
        let expected_rest = loaded
            .scene
            .sh_rest
            .as_deref()
            .map(|rest| &rest[index * rest_len..(index + 1) * rest_len])
            .unwrap_or_default();
        assert_eq!(splat.sh_rest_coefficients().len(), expected_rest.len());
        for (actual, expected) in splat.sh_rest_coefficients().iter().zip(expected_rest) {
            assert_f32_bits_eq(*actual, *expected);
        }
        assert!(
            splat.sh_rest[rest_len..]
                .iter()
                .all(|coefficient| coefficient.to_bits() == 0.0_f32.to_bits())
        );
    }

    fn incremental_decode(
        bytes: &[u8],
        chunk_size: usize,
    ) -> (super::PlySceneSummary, Vec<DecodedPlySplat>, usize) {
        let mut decoder = IncrementalPlyDecoder::default();
        let mut visited = Vec::new();
        let mut header_events = 0_usize;
        for chunk in bytes.chunks(chunk_size) {
            if decoder
                .push(chunk, |splat| visited.push(*splat))
                .unwrap()
                .is_some()
            {
                header_events += 1;
                assert!(
                    decoder
                        .push(&[], |splat| visited.push(*splat))
                        .unwrap()
                        .is_none()
                );
            }
        }
        let summary = decoder.finish(|splat| visited.push(*splat)).unwrap();
        assert_eq!(header_events, 1);
        assert_eq!(decoder.total_input_bytes(), bytes.len());
        assert_eq!(decoder.decoded_vertices(), summary.gaussians);
        (summary, visited, decoder.peak_buffered_bytes())
    }

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
    fn rejects_incomplete_sh_rest_instead_of_silently_using_sh0() {
        let error = parse_ply_text(PLY_INCOMPLETE_SH_REST).unwrap_err();
        assert_eq!(error, PlyLoadError::InvalidShLayout);
        assert_eq!(error.code(), ErrorCode::ParseFailed);
    }

    #[test]
    fn rejects_gapped_duplicate_malformed_and_unsupported_sh_rest_layouts() {
        let gapped = PLY_WITH_SH_REST_DEG1.replace("f_rest_8", "f_rest_9");
        let duplicate = PLY_WITH_SH_REST_DEG1.replace("f_rest_8", "f_rest_7");
        let malformed = PLY_WITH_SH_REST_DEG1.replace("f_rest_8", "f_rest_bad");
        let unsupported_count =
            PLY_WITH_SH_REST_DEG1.replace("end_header\n", "property float f_rest_9\nend_header\n");

        for input in [gapped, duplicate, malformed, unsupported_count] {
            assert_eq!(
                parse_ply_text(&input).unwrap_err(),
                PlyLoadError::InvalidShLayout
            );
        }
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
    fn accepts_exact_sh1_sh2_and_sh3_property_counts() {
        for (rest_count, expected_degree) in [(9, 1), (24, 2), (45, 3)] {
            let result = parse_ply_text(&ascii_ply_with_sh_rest_count(rest_count)).unwrap();
            assert_eq!(result.summary.sh_degree, expected_degree);
            assert_eq!(result.scene.sh_rest.as_ref().unwrap().len(), rest_count);
        }
    }

    #[test]
    fn rejects_sh4_property_count() {
        assert_eq!(
            parse_ply_text(&ascii_ply_with_sh_rest_count(72)).unwrap_err(),
            PlyLoadError::InvalidShLayout
        );
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
    fn rejects_ascii_non_finite_value() {
        let broken = VALID_PLY.replacen("0.0 0.1", "NaN 0.1", 1);
        let err = parse_ply_text(&broken).unwrap_err();
        assert_eq!(err, PlyLoadError::ParseNumber);
    }

    #[test]
    fn normalizes_ascii_infinite_opacity_logits() {
        let positive = VALID_PLY.replacen("0.0 0.1 1.0 0.9", "0.0 0.1 1.0 inf", 1);
        let negative = VALID_PLY.replacen("0.0 0.1 1.0 0.9", "0.0 0.1 1.0 -inf", 1);

        assert_eq!(
            parse_ply_text(&positive).unwrap().scene.opacity[0],
            OPACITY_LOGIT_LIMIT
        );
        assert_eq!(
            parse_ply_text(&negative).unwrap().scene.opacity[0],
            -OPACITY_LOGIT_LIMIT
        );
    }

    #[test]
    fn rejects_ascii_nan_opacity() {
        let broken = VALID_PLY.replacen("0.0 0.1 1.0 0.9", "0.0 0.1 1.0 NaN", 1);
        let err = parse_ply_text(&broken).unwrap_err();
        assert_eq!(err, PlyLoadError::ParseNumber);
    }

    #[test]
    fn rejects_ascii_vertex_count_mismatch() {
        let broken = VALID_PLY.replace("element vertex 1", "element vertex 2");
        let err = parse_ply_text(&broken).unwrap_err();
        assert_eq!(err, PlyLoadError::VertexCountMismatch);
    }

    #[test]
    fn default_limits_are_finite_and_admit_the_full_bicycle_sh3_corpus() {
        const BICYCLE_INPUT_BYTES: usize = 1_520_726_124;
        const BICYCLE_VERTICES: usize = 6_131_954;

        let limits = PlyLoadLimits::default();
        assert_eq!(limits.max_input_bytes, 2 * GIB - 1);
        assert_eq!(limits.max_vertices, 8_388_608);
        assert_eq!(limits.max_scene_bytes, 2 * GIB - 1);
        assert!(limits.max_input_bytes <= isize::MAX as usize);
        assert!(limits.max_scene_bytes <= isize::MAX as usize);
        assert!(BICYCLE_INPUT_BYTES <= limits.max_input_bytes);
        assert!(BICYCLE_VERTICES <= limits.max_vertices);

        let bicycle_header = PlyHeader {
            format: PlyFormat::BinaryLittleEndian,
            vertex_count: BICYCLE_VERTICES,
            vertex_properties: Vec::new(),
        };
        let rest_capacity =
            validate_decoded_scene_budget(&bicycle_header, 45, true, limits).unwrap();
        assert_eq!(rest_capacity, BICYCLE_VERTICES * 45);
        assert_eq!(
            BICYCLE_VERTICES * (56 + 45 * size_of::<f32>()),
            1_447_141_144
        );

        let over_default = VALID_PLY.replace(
            "element vertex 1",
            &format!("element vertex {}", limits.max_vertices + 1),
        );
        assert_eq!(
            parse_ply_text(&over_default).unwrap_err(),
            PlyLoadError::ResourceLimit {
                resource: "vertices",
                requested: limits.max_vertices + 1,
                limit: limits.max_vertices,
            }
        );
    }

    #[test]
    fn enforces_an_explicit_vertex_limit_below_the_finite_default() {
        let broken = VALID_PLY.replace("element vertex 1", "element vertex 5000001");
        let limits = PlyLoadLimits {
            max_vertices: 5_000_000,
            ..PlyLoadLimits::default()
        };
        let err = parse_ply_text_with_limits(&broken, limits).unwrap_err();
        assert_eq!(
            err,
            PlyLoadError::ResourceLimit {
                resource: "vertices",
                requested: 5_000_001,
                limit: 5_000_000,
            }
        );
        assert_eq!(err.code(), ErrorCode::Unsupported);
    }

    #[test]
    fn header_reader_stops_at_end_header_without_consuming_payload() {
        let expected_header_bytes = VALID_PLY.find("end_header\n").unwrap() + "end_header\n".len();
        let mut reader = Cursor::new(VALID_PLY.as_bytes());

        let (header, consumed) =
            read_header_from_reader(&mut reader, PlyLoadLimits::default()).unwrap();

        assert_eq!(header.vertex_count, 1);
        assert_eq!(consumed, expected_header_bytes);
        assert_eq!(reader.position() as usize, expected_header_bytes);
    }

    #[test]
    fn streams_ascii_file_and_reads_summary_from_header_only() {
        let file = TestFile::new("ascii", VALID_PLY.as_bytes());

        let summary = load_ply_summary(&file.path).unwrap();
        assert_eq!(summary.gaussians, 1);
        assert_eq!(summary.sh_degree, 0);

        let loaded = load_ply(&file.path).unwrap();
        assert_eq!(loaded.summary, summary);
        assert_eq!(
            loaded.scene.positions[0],
            gsplat_core::Vec3f::new(0.0, -0.1, 1.0)
        );
    }

    #[test]
    fn decoded_splat_is_fixed_size_copy_data() {
        assert!(!needs_drop::<DecodedPlySplat>());
        let splat = DecodedPlySplat {
            position_ruf: gsplat_core::Vec3f::new(1.0, 2.0, 3.0),
            opacity_logit: 0.5,
            log_scale_xyz: [0.0; 3],
            rotation_xyzw: [0.0, 0.0, 0.0, 1.0],
            color_dc: [0.0; 3],
            sh_rest: [0.0; super::MAX_SH_REST_COEFFICIENTS],
            sh_rest_len: 0,
            sh_degree: 0,
        };
        let copied = splat;
        assert_eq!(copied, splat);
    }

    #[test]
    fn streams_ascii_sh0_through_sh3_bit_exact_with_load_ply() {
        for rest_count in [0, 9, 24, 45] {
            let input = if rest_count == 0 {
                VALID_PLY.to_owned()
            } else {
                ascii_ply_with_sh_rest_count(rest_count)
            };
            let file = TestFile::new(&format!("visit-ascii-sh-{rest_count}"), input.as_bytes());
            let loaded = load_ply(&file.path).unwrap();
            let mut visited = Vec::new();
            let summary = visit_ply_splats(&file.path, |splat| visited.push(*splat)).unwrap();

            assert_eq!(summary, loaded.summary);
            assert_eq!(visited.len(), loaded.scene.len());
            for (index, splat) in visited.iter().enumerate() {
                assert_splat_matches_scene(splat, &loaded, index);
            }
        }
    }

    #[test]
    fn streams_little_and_big_endian_binary_sh0_through_sh3_bit_exact() {
        for endian in [Endian::Little, Endian::Big] {
            for rest_count in [0, 9, 24, 45] {
                let bytes = binary_ply(rest_count, 2, endian);
                let label = match endian {
                    Endian::Little => "little",
                    Endian::Big => "big",
                };
                let file = TestFile::new(&format!("visit-binary-{label}-sh-{rest_count}"), &bytes);
                let loaded = load_ply(&file.path).unwrap();
                let mut visited = Vec::new();
                let summary = visit_ply_splats(&file.path, |splat| visited.push(*splat)).unwrap();

                assert_eq!(summary, loaded.summary);
                assert_eq!(visited.len(), 2);
                for (index, splat) in visited.iter().enumerate() {
                    assert_splat_matches_scene(splat, &loaded, index);
                }
            }
        }
    }

    #[test]
    fn byte_visitor_matches_file_visitor_bit_exact_for_ascii_and_binary_sh0_through_sh3() {
        let mut fixtures = Vec::new();
        for rest_count in [0, 9, 24, 45] {
            fixtures.push(if rest_count == 0 {
                VALID_PLY.as_bytes().to_vec()
            } else {
                ascii_ply_with_sh_rest_count(rest_count).into_bytes()
            });
            fixtures.push(binary_ply(rest_count, 2, Endian::Little));
            fixtures.push(binary_ply(rest_count, 2, Endian::Big));
        }

        for (fixture_index, bytes) in fixtures.iter().enumerate() {
            let file = TestFile::new(&format!("visit-bytes-{fixture_index}"), bytes);
            let loaded = load_ply(&file.path).expect("wide reference");
            let mut file_splats = Vec::new();
            let file_summary = visit_ply_splats(&file.path, |splat| file_splats.push(*splat))
                .expect("file visitor");
            let mut byte_splats = Vec::new();
            let byte_summary = visit_ply_bytes_splats(bytes, |splat| byte_splats.push(*splat))
                .expect("byte visitor");

            assert_eq!(byte_summary, file_summary);
            assert_eq!(byte_summary, loaded.summary);
            assert_eq!(byte_splats.len(), file_splats.len());
            for (index, (byte_splat, file_splat)) in
                byte_splats.iter().zip(&file_splats).enumerate()
            {
                assert_splat_matches_scene(byte_splat, &loaded, index);
                assert_splat_matches_scene(file_splat, &loaded, index);
                assert_eq!(byte_splat, file_splat);
            }
        }
    }

    #[test]
    fn incremental_decoder_is_bit_exact_across_every_chunk_boundary_class() {
        let mut fixtures = Vec::new();
        for rest_count in [0, 9, 24, 45] {
            fixtures.push(if rest_count == 0 {
                VALID_PLY.as_bytes().to_vec()
            } else {
                ascii_ply_with_sh_rest_count(rest_count).into_bytes()
            });
            fixtures.push(binary_ply(rest_count, 3, Endian::Little));
            fixtures.push(binary_ply(rest_count, 3, Endian::Big));
        }

        for bytes in fixtures {
            let mut expected = Vec::new();
            let expected_summary =
                visit_ply_bytes_splats(&bytes, |splat| expected.push(*splat)).unwrap();
            for chunk_size in [1, 2, 7, 31, 257, bytes.len().max(1)] {
                let (summary, actual, _) = incremental_decode(&bytes, chunk_size);
                assert_eq!(summary, expected_summary);
                assert_eq!(actual, expected);
            }
        }
    }

    #[test]
    fn incremental_decoder_retains_only_a_transport_fragment_after_header() {
        let bytes = binary_ply(45, 1_000, Endian::Little);
        let (summary, splats, peak_buffered_bytes) = incremental_decode(&bytes, 257);
        assert_eq!(summary.gaussians, 1_000);
        assert_eq!(splats.len(), 1_000);
        assert!(peak_buffered_bytes < 2_048, "peak={peak_buffered_bytes}");
        assert!(peak_buffered_bytes * 20 < bytes.len());
    }

    #[test]
    fn incremental_decoder_accepts_a_large_first_chunk_with_a_short_header() {
        let bytes = binary_ply(45, 10_000, Endian::Little);
        assert!(bytes.len() > 2 * MIB);

        let (summary, splats, peak_buffered_bytes) = incremental_decode(&bytes, bytes.len());

        assert_eq!(summary.gaussians, 10_000);
        assert_eq!(splats.len(), 10_000);
        assert!(peak_buffered_bytes <= bytes.len());
    }

    #[test]
    fn incremental_decoder_rejects_truncation_and_reuse_after_finish() {
        let mut truncated = binary_ply(9, 2, Endian::Little);
        truncated.pop();
        let mut decoder = IncrementalPlyDecoder::default();
        let header = decoder.push(&truncated, |_| {}).unwrap();
        assert!(header.is_some());
        let mut truncated_callbacks = 0_usize;
        assert_eq!(
            decoder.finish(|_| truncated_callbacks += 1).unwrap_err(),
            PlyLoadError::VertexCountMismatch
        );
        assert_eq!(truncated_callbacks, 1);
        assert_eq!(
            decoder.finish(|_| truncated_callbacks += 1).unwrap_err(),
            PlyLoadError::IncrementalDecoderFinished
        );
        assert_eq!(truncated_callbacks, 1);

        let mut decoder = IncrementalPlyDecoder::default();
        let mut visited = Vec::new();
        assert!(
            decoder
                .push(VALID_PLY.as_bytes(), |splat| visited.push(*splat))
                .unwrap()
                .is_some()
        );
        let summary = decoder.finish(|splat| visited.push(*splat)).unwrap();
        assert_eq!(summary.gaussians, 1);
        assert_eq!(visited.len(), 1);
        assert_eq!(
            decoder.push(&[], |_| {}).unwrap_err(),
            PlyLoadError::IncrementalDecoderFinished
        );
    }

    #[test]
    fn incremental_decoder_failure_never_replays_published_callbacks() {
        let input = format!(
            "{}0.0 0.1\n",
            VALID_PLY.replace("element vertex 1", "element vertex 2")
        );
        let mut decoder = IncrementalPlyDecoder::default();
        let mut visited = Vec::new();

        let summary = decoder
            .push(input.as_bytes(), |splat| visited.push(*splat))
            .expect("validated header")
            .expect("header summary");
        assert_eq!(summary.gaussians, 2);
        assert!(visited.is_empty());

        assert_eq!(
            decoder.push(&[], |splat| visited.push(*splat)).unwrap_err(),
            PlyLoadError::VertexFieldCount
        );
        assert_eq!(visited.len(), 1);
        assert_eq!(
            visited[0].position_ruf,
            gsplat_core::Vec3f::new(0.0, -0.1, 1.0)
        );

        assert_eq!(
            decoder.push(&[], |splat| visited.push(*splat)).unwrap_err(),
            PlyLoadError::IncrementalDecoderFinished
        );
        assert_eq!(
            decoder.finish(|splat| visited.push(*splat)).unwrap_err(),
            PlyLoadError::IncrementalDecoderFinished
        );
        assert_eq!(visited.len(), 1);
    }

    #[test]
    fn incremental_decoder_treats_pre_callback_errors_as_terminal() {
        let limits = PlyLoadLimits {
            max_input_bytes: 2,
            ..PlyLoadLimits::default()
        };
        let mut decoder = IncrementalPlyDecoder::new(limits);
        let mut callbacks = 0_usize;

        assert_eq!(
            decoder.push(b"ply", |_| callbacks += 1).unwrap_err(),
            PlyLoadError::ResourceLimit {
                resource: "input bytes",
                requested: 3,
                limit: 2,
            }
        );
        assert_eq!(
            decoder.push(&[], |_| callbacks += 1).unwrap_err(),
            PlyLoadError::IncrementalDecoderFinished
        );
        assert_eq!(
            decoder.finish(|_| callbacks += 1).unwrap_err(),
            PlyLoadError::IncrementalDecoderFinished
        );
        assert_eq!(callbacks, 0);
    }

    #[test]
    fn byte_summary_skips_numeric_decode_and_byte_visitor_checks_limits_before_callbacks() {
        let invalid_body = VALID_PLY.replace(
            "0.0 0.1 1.0 0.9 1.0 1.1 1.2 1.0 0.0 0.0 0.0 0.2 0.3 0.4",
            "this payload is deliberately not numeric",
        );
        assert_eq!(
            parse_ply_bytes_summary(invalid_body.as_bytes())
                .unwrap()
                .gaussians,
            1
        );

        let limits = PlyLoadLimits {
            max_scene_bytes: 55,
            ..PlyLoadLimits::default()
        };
        let mut callbacks = 0;
        assert_eq!(
            visit_ply_bytes_splats_with_limits(VALID_PLY.as_bytes(), limits, |_| callbacks += 1)
                .unwrap_err(),
            PlyLoadError::ResourceLimit {
                resource: "decoded scene bytes",
                requested: 56,
                limit: 55,
            }
        );
        assert_eq!(callbacks, 0);
    }

    #[test]
    fn summaries_reject_forged_vertex_counts_before_capacity_allocation() {
        let forged = VALID_PLY.replace("element vertex 1", "element vertex 1000000");
        assert_eq!(
            parse_ply_bytes_summary(forged.as_bytes()).unwrap_err(),
            PlyLoadError::VertexCountMismatch
        );

        let file = TestFile::new("summary-forged-count", forged.as_bytes());
        assert_eq!(
            load_ply_summary(&file.path).unwrap_err(),
            PlyLoadError::VertexCountMismatch
        );
    }

    #[test]
    fn binary_stream_crosses_the_one_mib_batch_boundary_without_losing_vertices() {
        let rest_count = 45;
        let stride = (14 + rest_count) * size_of::<f32>();
        let records_per_batch = (MIB / stride).max(1);
        let vertex_count = records_per_batch + 7;
        let bytes = binary_ply(rest_count, vertex_count, Endian::Little);
        assert!(vertex_count * stride > MIB);
        let file = TestFile::new("visit-binary-multi-batch", &bytes);

        let mut first = None;
        let mut last = None;
        let mut count = 0_usize;
        let summary = visit_ply_splats(&file.path, |splat| {
            if first.is_none() {
                first = Some(*splat);
            }
            last = Some(*splat);
            count += 1;
        })
        .unwrap();

        assert_eq!(summary.gaussians, vertex_count);
        assert_eq!(summary.sh_degree, 3);
        assert_eq!(count, vertex_count);
        assert_f32_bits_eq(first.unwrap().position_ruf.x, 0.0);
        assert_f32_bits_eq(last.unwrap().position_ruf.x, (vertex_count - 1) as f32);
    }

    #[test]
    fn binary_stream_reports_short_read_without_partial_record_callbacks() {
        let mut bytes = binary_ply(0, 2, Endian::Little);
        let declared_input_len = bytes.len();
        bytes.truncate(bytes.len() - size_of::<f32>());
        let mut reader = Cursor::new(bytes);
        let mut callbacks = 0_usize;

        let error = visit_ply_reader_with_limits(
            &mut reader,
            declared_input_len,
            PlyLoadLimits::default(),
            |_| callbacks += 1,
        )
        .unwrap_err();

        assert_eq!(error, PlyLoadError::VertexCountMismatch);
        assert_eq!(callbacks, 0);
    }

    #[test]
    fn stream_delivers_complete_vertices_before_a_later_ascii_error() {
        let input = format!(
            "{}0.0 0.1\n",
            VALID_PLY.replace("element vertex 1", "element vertex 2")
        );
        let file = TestFile::new("visit-ascii-late-error", input.as_bytes());
        let mut visited = Vec::new();

        let error = visit_ply_splats(&file.path, |splat| visited.push(*splat)).unwrap_err();

        assert_eq!(error, PlyLoadError::VertexFieldCount);
        assert_eq!(visited.len(), 1);
        assert_eq!(
            visited[0].position_ruf,
            gsplat_core::Vec3f::new(0.0, -0.1, 1.0)
        );
    }

    #[test]
    fn stream_honors_explicit_decoded_scene_budget_before_callbacks() {
        let file = TestFile::new("visit-scene-budget", VALID_PLY.as_bytes());
        let limits = PlyLoadLimits {
            max_scene_bytes: 55,
            ..PlyLoadLimits::default()
        };
        let mut callbacks = 0_usize;

        let error =
            visit_ply_splats_with_limits(&file.path, limits, |_| callbacks += 1).unwrap_err();

        assert_eq!(
            error,
            PlyLoadError::ResourceLimit {
                resource: "decoded scene bytes",
                requested: 56,
                limit: 55,
            }
        );
        assert_eq!(callbacks, 0);
    }

    #[test]
    fn summary_rejects_invalid_sh_layout_without_decoding_vertex_values() {
        let invalid_sh = TestFile::new("summary-invalid-sh", PLY_INCOMPLETE_SH_REST.as_bytes());
        assert_eq!(
            load_ply_summary(&invalid_sh.path).unwrap_err(),
            PlyLoadError::InvalidShLayout
        );

        let invalid_body = VALID_PLY.replace(
            "0.0 0.1 1.0 0.9 1.0 1.1 1.2 1.0 0.0 0.0 0.0 0.2 0.3 0.4",
            "this payload is deliberately not numeric",
        );
        let invalid_body = TestFile::new("summary-invalid-body", invalid_body.as_bytes());
        assert_eq!(load_ply_summary(&invalid_body.path).unwrap().gaussians, 1);
        assert_eq!(
            load_ply(&invalid_body.path).unwrap_err(),
            PlyLoadError::VertexFieldCount
        );
    }

    #[test]
    fn path_loader_enforces_explicit_input_limit_from_metadata() {
        let file = TestFile::new("input-limit", VALID_PLY.as_bytes());
        let limits = PlyLoadLimits {
            max_input_bytes: VALID_PLY.len() - 1,
            ..PlyLoadLimits::default()
        };

        assert_eq!(
            load_ply_with_limits(&file.path, limits).unwrap_err(),
            PlyLoadError::ResourceLimit {
                resource: "input bytes",
                requested: VALID_PLY.len(),
                limit: VALID_PLY.len() - 1,
            }
        );
    }

    #[test]
    fn enforces_custom_input_and_header_limits() {
        let input_limit = PlyLoadLimits {
            max_input_bytes: VALID_PLY.len() - 1,
            ..PlyLoadLimits::default()
        };
        assert_eq!(
            parse_ply_bytes_with_limits(VALID_PLY.as_bytes(), input_limit).unwrap_err(),
            PlyLoadError::ResourceLimit {
                resource: "input bytes",
                requested: VALID_PLY.len(),
                limit: VALID_PLY.len() - 1,
            }
        );

        let header_limit = PlyLoadLimits {
            max_header_bytes: 3,
            ..PlyLoadLimits::default()
        };
        assert!(matches!(
            parse_ply_text_with_limits(VALID_PLY, header_limit),
            Err(PlyLoadError::ResourceLimit {
                resource: "header bytes",
                ..
            })
        ));
    }

    #[test]
    fn enforces_custom_property_and_scene_limits() {
        let property_limit = PlyLoadLimits {
            max_vertex_properties: 13,
            ..PlyLoadLimits::default()
        };
        assert_eq!(
            parse_ply_text_with_limits(VALID_PLY, property_limit).unwrap_err(),
            PlyLoadError::ResourceLimit {
                resource: "vertex properties",
                requested: 14,
                limit: 13,
            }
        );

        let scene_limit = PlyLoadLimits {
            max_scene_bytes: 55,
            ..PlyLoadLimits::default()
        };
        assert_eq!(
            parse_ply_text_with_limits(VALID_PLY, scene_limit).unwrap_err(),
            PlyLoadError::ResourceLimit {
                resource: "decoded scene bytes",
                requested: 56,
                limit: 55,
            }
        );
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

        let file = TestFile::new("binary-little", &bytes);
        let streamed = load_ply(&file.path).unwrap();
        assert_eq!(streamed, result);
    }

    #[test]
    fn parses_binary_big_endian_ply() {
        let header = "ply\nformat binary_big_endian 1.0\nelement vertex 1\nproperty float x\nproperty float y\nproperty float z\nproperty float opacity\nproperty float scale_0\nproperty float scale_1\nproperty float scale_2\nproperty float rot_0\nproperty float rot_1\nproperty float rot_2\nproperty float rot_3\nproperty float f_dc_0\nproperty float f_dc_1\nproperty float f_dc_2\nend_header\n";

        let mut bytes = header.as_bytes().to_vec();
        let values: [f32; 14] = [
            0.0, 0.1, 1.0, 0.9, 1.0, 1.1, 1.2, 1.0, 0.0, 0.0, 0.0, 0.2, 0.3, 0.4,
        ];
        for v in values {
            bytes.extend_from_slice(&v.to_be_bytes());
        }

        let result = parse_ply_bytes(&bytes).unwrap();
        assert_eq!(result.summary.gaussians, 1);
        assert_eq!(result.summary.sh_degree, 0);
        assert_eq!(result.scene.positions[0].y, -0.1);
        assert_eq!(result.scene.opacity[0], 0.9);
        assert_eq!(result.scene.rotation_xyzw[0], [0.0, 0.0, 0.0, 1.0]);

        let file = TestFile::new("binary-big", &bytes);
        let streamed = load_ply(&file.path).unwrap();
        assert_eq!(streamed, result);
    }

    #[test]
    fn rejects_truncated_binary_body() {
        let header = "ply\nformat binary_little_endian 1.0\nelement vertex 2\nproperty float x\nproperty float y\nproperty float z\nproperty float opacity\nproperty float scale_0\nproperty float scale_1\nproperty float scale_2\nproperty float rot_0\nproperty float rot_1\nproperty float rot_2\nproperty float rot_3\nproperty float f_dc_0\nproperty float f_dc_1\nproperty float f_dc_2\nend_header\n";

        let mut bytes = header.as_bytes().to_vec();
        let values: [f32; 14] = [
            0.0, 0.1, 1.0, 0.9, 1.0, 1.1, 1.2, 1.0, 0.0, 0.0, 0.0, 0.2, 0.3, 0.4,
        ];
        for v in values {
            bytes.extend_from_slice(&v.to_le_bytes());
        }

        let err = parse_ply_bytes(&bytes).unwrap_err();
        assert_eq!(err, PlyLoadError::VertexCountMismatch);
    }

    #[test]
    fn rejects_binary_body_size_overflow_before_allocation() {
        let header = format!(
            "ply\nformat binary_little_endian 1.0\nelement vertex {}\nproperty float x\nproperty float y\nproperty float z\nproperty float opacity\nproperty float scale_0\nproperty float scale_1\nproperty float scale_2\nproperty float rot_0\nproperty float rot_1\nproperty float rot_2\nproperty float rot_3\nproperty float f_dc_0\nproperty float f_dc_1\nproperty float f_dc_2\nend_header\n",
            usize::MAX
        );
        let limits = PlyLoadLimits {
            max_vertices: usize::MAX,
            ..PlyLoadLimits::default()
        };

        let err = parse_ply_bytes_with_limits(header.as_bytes(), limits).unwrap_err();
        assert_eq!(err, PlyLoadError::ResourceSizeOverflow("binary body bytes"));
    }

    #[test]
    fn rejects_sh_capacity_overflow_before_allocation() {
        let header = PlyHeader {
            format: PlyFormat::Ascii,
            vertex_count: usize::MAX / 56,
            vertex_properties: Vec::new(),
        };
        let limits = PlyLoadLimits {
            max_vertices: usize::MAX,
            max_scene_bytes: usize::MAX,
            ..PlyLoadLimits::default()
        };

        let err = allocate_scene(&header, 1, 64, true, limits).unwrap_err();
        assert_eq!(
            err,
            PlyLoadError::ResourceSizeOverflow("SH coefficient capacity")
        );
    }

    #[test]
    fn rejects_binary_non_finite_value() {
        let header = "ply\nformat binary_little_endian 1.0\nelement vertex 1\nproperty float x\nproperty float y\nproperty float z\nproperty float opacity\nproperty float scale_0\nproperty float scale_1\nproperty float scale_2\nproperty float rot_0\nproperty float rot_1\nproperty float rot_2\nproperty float rot_3\nproperty float f_dc_0\nproperty float f_dc_1\nproperty float f_dc_2\nend_header\n";

        let mut bytes = header.as_bytes().to_vec();
        let values: [f32; 14] = [
            f32::NAN,
            0.1,
            1.0,
            0.9,
            1.0,
            1.1,
            1.2,
            1.0,
            0.0,
            0.0,
            0.0,
            0.2,
            0.3,
            0.4,
        ];
        for v in values {
            bytes.extend_from_slice(&v.to_le_bytes());
        }

        let err = parse_ply_bytes(&bytes).unwrap_err();
        assert_eq!(err, PlyLoadError::ParseNumber);
    }

    #[test]
    fn normalizes_binary_infinite_opacity_logit() {
        let header = "ply\nformat binary_little_endian 1.0\nelement vertex 1\nproperty float x\nproperty float y\nproperty float z\nproperty float opacity\nproperty float scale_0\nproperty float scale_1\nproperty float scale_2\nproperty float rot_0\nproperty float rot_1\nproperty float rot_2\nproperty float rot_3\nproperty float f_dc_0\nproperty float f_dc_1\nproperty float f_dc_2\nend_header\n";

        let mut bytes = header.as_bytes().to_vec();
        let values: [f32; 14] = [
            0.0,
            0.1,
            1.0,
            f32::INFINITY,
            1.0,
            1.1,
            1.2,
            1.0,
            0.0,
            0.0,
            0.0,
            0.2,
            0.3,
            0.4,
        ];
        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }

        let result = parse_ply_bytes(&bytes).unwrap();
        assert_eq!(result.scene.opacity[0], OPACITY_LOGIT_LIMIT);
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
