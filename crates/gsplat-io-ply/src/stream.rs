//! File, byte, reader, and incremental PLY transport lifecycle.

use std::fs::File;
use std::io::{BufRead, ErrorKind, Read};
use std::path::Path;

use crate::decode::{
    Endian, VertexDecodePlan, compute_vertex_layout, decode_ascii_vertex, decode_binary_vertex,
};
use crate::metadata::{
    PlyFormat, PlyHeader, complete_header_end, complete_header_end_at_eof, parse_header_text,
    read_header_from_reader, split_header_body,
};
use crate::{
    DecodedPlySplat, MIB, PlyLoadError, PlyLoadLimits, PlySceneSummary, ensure_limit,
    prepare_decode, prepare_decode_after_metadata, try_vec_with_capacity,
    validate_body_before_allocation, validate_decoded_scene_budget,
    validate_stream_body_before_allocation,
};

pub(super) fn open_ply_file_with_limits(
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

pub(super) fn visit_ply_bytes_splats_with_limits(
    input: &[u8],
    limits: PlyLoadLimits,
    mut visitor: impl FnMut(&DecodedPlySplat),
) -> Result<PlySceneSummary, PlyLoadError> {
    ensure_limit("input bytes", input.len(), limits.max_input_bytes)?;
    let (header, body) = split_header_body(input, limits)?;
    let prepared = prepare_decode(&header)?;

    validate_body_before_allocation(&header, body)?;
    validate_decoded_scene_budget(
        &header,
        prepared.rest_stride,
        prepared.summary.has_sh_rest,
        limits,
    )?;
    visit_body(&header, body, &prepared.decode_plan, &mut visitor)?;

    Ok(prepared.summary)
}

pub(super) fn visit_ply_reader_with_limits<R, F>(
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
    let prepared = prepare_decode(&header)?;

    validate_stream_body_before_allocation(&header, input_len, header_bytes)?;
    validate_decoded_scene_budget(
        &header,
        prepared.rest_stride,
        prepared.summary.has_sh_rest,
        limits,
    )?;
    visit_reader(reader, &header, &prepared.decode_plan, &mut visitor)?;

    Ok(prepared.summary)
}

pub(super) fn visit_body<F>(
    header: &PlyHeader,
    body: &[u8],
    decode_plan: &VertexDecodePlan,
    visitor: &mut F,
) -> Result<(), PlyLoadError>
where
    F: FnMut(&DecodedPlySplat),
{
    match header.format {
        PlyFormat::Ascii => visit_ascii_body(header, body, decode_plan, visitor),
        PlyFormat::BinaryLittleEndian => {
            visit_binary_body(header, body, decode_plan, Endian::Little, visitor)
        }
        PlyFormat::BinaryBigEndian => {
            visit_binary_body(header, body, decode_plan, Endian::Big, visitor)
        }
    }
}

pub(super) fn visit_reader<R, F>(
    reader: &mut R,
    header: &PlyHeader,
    decode_plan: &VertexDecodePlan,
    visitor: &mut F,
) -> Result<(), PlyLoadError>
where
    R: BufRead,
    F: FnMut(&DecodedPlySplat),
{
    match header.format {
        PlyFormat::Ascii => visit_ascii_reader(reader, header, decode_plan, visitor),
        PlyFormat::BinaryLittleEndian => {
            visit_binary_reader(reader, header, decode_plan, Endian::Little, visitor)
        }
        PlyFormat::BinaryBigEndian => {
            visit_binary_reader(reader, header, decode_plan, Endian::Big, visitor)
        }
    }
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
            self.extend_buffer(&input[header_input_len..])?;
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
        if self.body.is_none()
            && let Some(header_end) = complete_header_end_at_eof(&self.buffer, self.limits)?
        {
            self.initialize_body(header_end)?;
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
        let prepared = prepare_decode_after_metadata(&header, |rest_stride, has_sh_rest| {
            validate_decoded_scene_budget(&header, rest_stride, has_sh_rest, self.limits)
                .map(|_| ())
        })?;
        let body = IncrementalPlyBody::new(header, prepared.decode_plan)?;
        self.buffer.drain(..header_end);
        self.body = Some(body);
        self.summary = Some(prepared.summary);
        Ok(prepared.summary)
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
