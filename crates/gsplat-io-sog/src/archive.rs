//! PlayCanvas bundled `.sog` ZIP (STORED or DEFLATE).
//!
//! The official writer uses uncompressed ZIP. Readers also accept method 8.
//! ZIP64, encryption, and path traversal are rejected.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use flate2::read::DeflateDecoder;

use crate::SogError;

const MIB: usize = 1024 * 1024;
const GIB: usize = 1024 * MIB;
const LOCAL_SIG: [u8; 4] = [0x50, 0x4b, 0x03, 0x04];
const CENTRAL_SIG: [u8; 4] = [0x50, 0x4b, 0x01, 0x02];
const EOCD_SIG: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
const EOCD_LEN: usize = 22;
const LOCAL_LEN: usize = 30;
const CENTRAL_LEN: usize = 46;
const MAX_COMMENT: usize = 65535;
const MAX_ARCHIVE_BYTES: usize = GIB;
const MAX_ZIP_ENTRIES: usize = 64;
const MAX_ENTRY_UNCOMPRESSED: usize = 256 * MIB;
const MAX_TOTAL_UNCOMPRESSED: usize = 512 * MIB;
const METHOD_STORED: u16 = 0;
const METHOD_DEFLATE: u16 = 8;
const FLAG_ENCRYPTED: u16 = 1;
const ZIP64_U16: usize = 0xFFFF;
const ZIP64_U32: usize = 0xFFFF_FFFF;

pub fn is_zip_magic(prefix: &[u8]) -> bool {
    prefix.starts_with(&LOCAL_SIG) || prefix.starts_with(&EOCD_SIG)
}

pub fn decode_sog_archive_path(path: &Path) -> Result<gsplat_core::SceneBuffers, SogError> {
    let len = usize::try_from(path.metadata()?.len()).unwrap_or(usize::MAX);
    ensure_limit("archive bytes", len, MAX_ARCHIVE_BYTES)?;
    decode_sog_archive(&std::fs::read(path)?)
}

pub fn decode_sog_archive(bytes: &[u8]) -> Result<gsplat_core::SceneBuffers, SogError> {
    ensure_limit("archive bytes", bytes.len(), MAX_ARCHIVE_BYTES)?;
    let files = extract_zip(bytes)?;
    let meta_bytes = lookup_file(&files, "meta.json")?;
    let meta_text = std::str::from_utf8(meta_bytes)
        .map_err(|_| SogError::Malformed("SOG archive meta.json is not UTF-8"))?;
    let meta = crate::meta::parse_chunk_meta(meta_text)?;
    crate::decode::decode_sog_range_from(&ArchiveAssets { files: &files }, &meta, 0, meta.count)
}

pub(crate) struct ArchiveAssets<'a> {
    pub files: &'a HashMap<String, Vec<u8>>,
}

impl crate::decode::SogAssets for ArchiveAssets<'_> {
    fn read(&self, name: &str) -> Result<std::borrow::Cow<'_, [u8]>, SogError> {
        Ok(std::borrow::Cow::Borrowed(lookup_file(self.files, name)?))
    }
}

pub(crate) fn extract_zip(data: &[u8]) -> Result<HashMap<String, Vec<u8>>, SogError> {
    let eocd = find_eocd(data)?;
    let disk = read_u16(data, eocd + 4)?;
    let cd_disk = read_u16(data, eocd + 6)?;
    let entries_on_disk = read_u16(data, eocd + 8)? as usize;
    let entry_count = read_u16(data, eocd + 10)? as usize;
    let cd_size = read_u32(data, eocd + 12)? as usize;
    let cd_offset = read_u32(data, eocd + 16)? as usize;
    if disk != 0 || cd_disk != 0 || entries_on_disk != entry_count {
        return Err(SogError::Malformed(
            "multi-disk zip archives are not supported",
        ));
    }
    if entry_count == ZIP64_U16 || cd_size == ZIP64_U32 || cd_offset == ZIP64_U32 {
        return Err(SogError::Malformed("zip64 archives are not supported"));
    }
    ensure_limit("zip entries", entry_count, MAX_ZIP_ENTRIES)?;
    if cd_offset
        .checked_add(cd_size)
        .is_none_or(|end| end > data.len())
    {
        return Err(SogError::Malformed("zip central directory is truncated"));
    }

    let mut files = HashMap::new();
    let mut cursor = cd_offset;
    let cd_end = cd_offset + cd_size;
    let mut total_uncompressed = 0_usize;
    for _ in 0..entry_count {
        let entry = parse_central_entry(data, cursor, cd_end)?;
        cursor = entry.next_offset;
        if entry.name.ends_with('/') {
            continue;
        }
        ensure_limit(
            "zip entry bytes",
            entry.uncompressed_size,
            MAX_ENTRY_UNCOMPRESSED,
        )?;
        total_uncompressed = total_uncompressed.saturating_add(entry.uncompressed_size);
        ensure_limit(
            "zip uncompressed bytes",
            total_uncompressed,
            MAX_TOTAL_UNCOMPRESSED,
        )?;
        if files.contains_key(&entry.name) {
            return Err(SogError::Malformed("zip archive contains duplicate names"));
        }
        let payload = extract_entry(data, &entry)?;
        files.insert(entry.name, payload);
    }
    if files.is_empty() {
        return Err(SogError::Malformed("zip archive has no files"));
    }
    Ok(files)
}

#[cfg(test)]
pub(crate) fn write_stored_zip(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    write_zip(entries, false)
}

#[cfg(test)]
fn write_zip(entries: &[(String, Vec<u8>)], deflate: bool) -> Vec<u8> {
    let mut locals = Vec::new();
    let mut central = Vec::new();
    for (name, data) in entries {
        let name_bytes = name.as_bytes();
        let crc = crc32fast::hash(data);
        let payload = if deflate {
            compress_deflate(data)
        } else {
            data.clone()
        };
        let method = if deflate {
            METHOD_DEFLATE
        } else {
            METHOD_STORED
        };
        let local_offset = locals.len() as u32;
        locals.extend_from_slice(&LOCAL_SIG);
        locals.extend_from_slice(&20u16.to_le_bytes());
        locals.extend_from_slice(&0u16.to_le_bytes());
        locals.extend_from_slice(&method.to_le_bytes());
        locals.extend_from_slice(&0u16.to_le_bytes());
        locals.extend_from_slice(&0u16.to_le_bytes());
        locals.extend_from_slice(&crc.to_le_bytes());
        locals.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        locals.extend_from_slice(&(data.len() as u32).to_le_bytes());
        locals.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        locals.extend_from_slice(&0u16.to_le_bytes());
        locals.extend_from_slice(name_bytes);
        locals.extend_from_slice(&payload);

        central.extend_from_slice(&CENTRAL_SIG);
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&method.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&local_offset.to_le_bytes());
        central.extend_from_slice(name_bytes);
    }
    let cd_offset = locals.len() as u32;
    let cd_size = central.len() as u32;
    let mut out = locals;
    out.extend_from_slice(&central);
    out.extend_from_slice(&EOCD_SIG);
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    let count = entries.len() as u16;
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

#[cfg(test)]
fn compress_deflate(data: &[u8]) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::DeflateEncoder;
    use std::io::Write;

    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(data)
        .expect("deflate encoder write to memory");
    encoder.finish().expect("deflate encoder finish")
}

fn lookup_file<'a>(files: &'a HashMap<String, Vec<u8>>, name: &str) -> Result<&'a [u8], SogError> {
    crate::decode::reject_unsafe_name(name)?;
    let needle = name.replace('\\', "/");
    let needle = needle.trim_start_matches("./");
    if let Some(bytes) = files.get(needle) {
        return Ok(bytes);
    }
    let base = Path::new(needle)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(needle);
    let mut matches = files.iter().filter(|(key, _)| {
        Path::new(key.as_str())
            .file_name()
            .and_then(|value| value.to_str())
            == Some(base)
    });
    match (matches.next(), matches.next()) {
        (Some((_, bytes)), None) => Ok(bytes),
        (Some(_), Some(_)) => Err(SogError::Malformed("SOG archive file name is ambiguous")),
        _ => Err(SogError::Malformed("SOG archive is missing a named file")),
    }
}

struct CentralEntry {
    name: String,
    method: u16,
    crc: u32,
    compressed_size: usize,
    uncompressed_size: usize,
    local_offset: usize,
    next_offset: usize,
}

fn parse_central_entry(
    data: &[u8],
    offset: usize,
    cd_end: usize,
) -> Result<CentralEntry, SogError> {
    if offset
        .checked_add(CENTRAL_LEN)
        .is_none_or(|end| end > cd_end || end > data.len())
    {
        return Err(SogError::Malformed("zip central directory is truncated"));
    }
    if data[offset..offset + 4] != CENTRAL_SIG {
        return Err(SogError::Malformed(
            "zip central directory signature mismatch",
        ));
    }
    let flags = read_u16(data, offset + 8)?;
    let method = read_u16(data, offset + 10)?;
    let crc = read_u32(data, offset + 16)?;
    let compressed_size = read_u32(data, offset + 20)? as usize;
    let uncompressed_size = read_u32(data, offset + 24)? as usize;
    let name_len = read_u16(data, offset + 28)? as usize;
    let extra_len = read_u16(data, offset + 30)? as usize;
    let comment_len = read_u16(data, offset + 32)? as usize;
    let local_offset = read_u32(data, offset + 42)? as usize;
    let name_start = offset + CENTRAL_LEN;
    let next_offset = name_start
        .checked_add(name_len)
        .and_then(|end| end.checked_add(extra_len))
        .and_then(|end| end.checked_add(comment_len))
        .ok_or(SogError::Malformed("zip central directory is truncated"))?;
    if next_offset > cd_end {
        return Err(SogError::Malformed("zip central directory is truncated"));
    }
    if compressed_size == ZIP64_U32 || uncompressed_size == ZIP64_U32 {
        return Err(SogError::Malformed("zip64 archives are not supported"));
    }
    if flags & FLAG_ENCRYPTED != 0 {
        return Err(SogError::Malformed(
            "encrypted zip entries are not supported",
        ));
    }
    if method != METHOD_STORED && method != METHOD_DEFLATE {
        return Err(SogError::Malformed("unsupported zip compression method"));
    }
    let name = std::str::from_utf8(&data[name_start..name_start + name_len])
        .map_err(|_| SogError::Malformed("zip entry name is not UTF-8"))?;
    Ok(CentralEntry {
        name: normalize_zip_name(name)?,
        method,
        crc,
        compressed_size,
        uncompressed_size,
        local_offset,
        next_offset,
    })
}

fn extract_entry(data: &[u8], entry: &CentralEntry) -> Result<Vec<u8>, SogError> {
    let local_end = entry
        .local_offset
        .checked_add(LOCAL_LEN)
        .ok_or(SogError::Malformed("zip local header is truncated"))?;
    if local_end > data.len() || data[entry.local_offset..entry.local_offset + 4] != LOCAL_SIG {
        return Err(SogError::Malformed("zip local header is truncated"));
    }
    let local_method = read_u16(data, entry.local_offset + 8)?;
    let local_flags = read_u16(data, entry.local_offset + 6)?;
    if local_method != entry.method {
        return Err(SogError::Malformed("zip local header method mismatch"));
    }
    if local_flags & FLAG_ENCRYPTED != 0 {
        return Err(SogError::Malformed(
            "encrypted zip entries are not supported",
        ));
    }
    let name_len = read_u16(data, entry.local_offset + 26)? as usize;
    let extra_len = read_u16(data, entry.local_offset + 28)? as usize;
    let data_start = local_end
        .checked_add(name_len)
        .and_then(|end| end.checked_add(extra_len))
        .ok_or(SogError::Malformed("zip local header is truncated"))?;
    let data_end = data_start
        .checked_add(entry.compressed_size)
        .ok_or(SogError::Malformed("zip local file data is truncated"))?;
    if data_end > data.len() {
        return Err(SogError::Malformed("zip local file data is truncated"));
    }
    let compressed = &data[data_start..data_end];
    let payload = match entry.method {
        METHOD_STORED => {
            if entry.compressed_size != entry.uncompressed_size {
                return Err(SogError::Malformed("stored zip entry size fields disagree"));
            }
            compressed.to_vec()
        }
        METHOD_DEFLATE => inflate_exact(compressed, entry.uncompressed_size)?,
        _ => return Err(SogError::Malformed("unsupported zip compression method")),
    };
    if payload.len() != entry.uncompressed_size {
        return Err(SogError::Malformed("zip entry size mismatch"));
    }
    if crc32fast::hash(&payload) != entry.crc {
        return Err(SogError::Malformed("zip entry CRC mismatch"));
    }
    Ok(payload)
}

fn inflate_exact(src: &[u8], expected: usize) -> Result<Vec<u8>, SogError> {
    let mut decoder = DeflateDecoder::new(src);
    let mut out = Vec::with_capacity(expected);
    let mut tmp = [0_u8; 8192];
    loop {
        let n = decoder
            .read(&mut tmp)
            .map_err(|_| SogError::Malformed("failed to inflate zip entry"))?;
        if n == 0 {
            break;
        }
        if out.len().saturating_add(n) > expected {
            return Err(SogError::Malformed("deflate output exceeds declared size"));
        }
        out.extend_from_slice(&tmp[..n]);
    }
    Ok(out)
}

fn find_eocd(data: &[u8]) -> Result<usize, SogError> {
    if data.len() < EOCD_LEN {
        return Err(SogError::Malformed("zip archive is truncated"));
    }
    let min_start = data.len().saturating_sub(EOCD_LEN + MAX_COMMENT);
    for offset in (min_start..=data.len() - EOCD_LEN).rev() {
        if data[offset..offset + 4] != EOCD_SIG {
            continue;
        }
        let comment_len = read_u16(data, offset + 20)? as usize;
        if offset + EOCD_LEN + comment_len == data.len() {
            return Ok(offset);
        }
    }
    Err(SogError::Malformed(
        "zip end-of-central-directory is missing",
    ))
}

fn normalize_zip_name(name: &str) -> Result<String, SogError> {
    crate::decode::reject_unsafe_name(name)?;
    let mut normalized = name.replace('\\', "/");
    while let Some(rest) = normalized.strip_prefix("./") {
        normalized = rest.to_string();
    }
    if normalized.is_empty() {
        return Err(SogError::Malformed("zip entry name is empty"));
    }
    Ok(normalized)
}

fn ensure_limit(resource: &'static str, requested: usize, limit: usize) -> Result<(), SogError> {
    if requested > limit {
        Err(SogError::ResourceLimit {
            resource,
            requested,
            limit,
        })
    } else {
        Ok(())
    }
}

fn read_u16(data: &[u8], offset: usize) -> Result<u16, SogError> {
    let slice = data
        .get(offset..offset + 2)
        .ok_or(SogError::Malformed("truncated zip header"))?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

fn read_u32(data: &[u8], offset: usize) -> Result<u32, SogError> {
    let slice = data
        .get(offset..offset + 4)
        .ok_or(SogError::Malformed("truncated zip header"))?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_and_deflate_roundtrip_payloads() {
        let entries = vec![
            ("meta.json".to_string(), b"{\"ok\":true}".to_vec()),
            ("means_l.png".to_string(), b"png-bytes".to_vec()),
        ];
        let stored = write_zip(&entries, false);
        let deflated = write_zip(&entries, true);
        for archive in [stored, deflated] {
            let files = extract_zip(&archive).expect("extract");
            assert_eq!(
                files.get("meta.json").map(Vec::as_slice),
                Some(&b"{\"ok\":true}"[..])
            );
            assert_eq!(
                files.get("means_l.png").map(Vec::as_slice),
                Some(&b"png-bytes"[..])
            );
        }
    }

    #[test]
    fn rejects_parent_directory_names() {
        let archive = write_zip(&[("../evil.bin".to_string(), b"nope".to_vec())], false);
        assert_eq!(
            extract_zip(&archive),
            Err(SogError::Malformed(
                "SOG file name must not contain parent segments"
            ))
        );
    }

    #[test]
    fn rejects_zip_entry_count_above_limit() {
        let entries: Vec<(String, Vec<u8>)> = (0..MAX_ZIP_ENTRIES + 1)
            .map(|index| (format!("f{index}.bin"), b"x".to_vec()))
            .collect();
        let archive = write_zip(&entries, false);
        assert_eq!(
            extract_zip(&archive),
            Err(SogError::ResourceLimit {
                resource: "zip entries",
                requested: MAX_ZIP_ENTRIES + 1,
                limit: MAX_ZIP_ENTRIES,
            })
        );
    }

    #[test]
    fn rejects_declared_uncompressed_size_above_limit() {
        let mut archive = write_zip(&[("blob.bin".to_string(), b"hi".to_vec())], true);
        // Central-directory uncompressed size lives at CD offset + 24.
        let eocd = find_eocd(&archive).unwrap();
        let cd_offset = read_u32(&archive, eocd + 16).unwrap() as usize;
        let huge = (MAX_ENTRY_UNCOMPRESSED as u32)
            .saturating_add(1)
            .to_le_bytes();
        archive[cd_offset + 24..cd_offset + 28].copy_from_slice(&huge);
        assert_eq!(
            extract_zip(&archive),
            Err(SogError::ResourceLimit {
                resource: "zip entry bytes",
                requested: MAX_ENTRY_UNCOMPRESSED + 1,
                limit: MAX_ENTRY_UNCOMPRESSED,
            })
        );
    }
}
