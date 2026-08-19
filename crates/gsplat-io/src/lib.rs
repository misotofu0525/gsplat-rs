//! Whole-scene PLY / SPZ v4 / SOG import facade.
//!
//! Streamed SOG (`lod-meta.json`) is rejected here. Use
//! [`assemble_streamed_sog`] to select a budgeted subset from spatial metadata.

use std::fs;
use std::path::Path;

use gsplat_core::{ErrorCode, SceneBuffers};
use gsplat_io_ply::{PlyLoadError, PlyLoadResult, load_ply, parse_ply_bytes};
use gsplat_io_sog::{
    decode_sog_archive, decode_sog_archive_path, decode_sog_dir, is_zip_magic, parse_chunk_meta,
    parse_lod_meta,
};
use gsplat_io_spz::{SpzLoadError, SpzLoadResult, load_spz, parse_spz_bytes};
use thiserror::Error;

pub use gsplat_io_sog::{
    SogError, StreamAssembleResult, StreamedSogSession, StreamingBudgets, assemble_streamed_sog,
    is_bundled_sog_path, is_streamed_sog_path, is_unbundled_sog_path,
};

const SPZ_MAGIC: &[u8; 4] = b"NGSP";

/// On-disk / in-memory scene format selected by the import facade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneFormat {
    Ply,
    SpzV4,
    Sog,
}

impl SceneFormat {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ply => "ply",
            Self::SpzV4 => "spz-v4",
            Self::Sog => "sog",
        }
    }
}

/// Summary shared by whole-scene loads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneSummary {
    pub format: SceneFormat,
    pub gaussians: usize,
    pub sh_degree: u8,
    pub has_sh_rest: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneLoadResult {
    pub scene: SceneBuffers,
    pub summary: SceneSummary,
}

#[derive(Debug, Error, PartialEq)]
pub enum SceneLoadError {
    #[error(transparent)]
    Ply(#[from] PlyLoadError),
    #[error(transparent)]
    Spz(#[from] SpzLoadError),
    #[error(transparent)]
    Sog(#[from] SogError),
    #[error("I/O error while reading scene")]
    Io,
    #[error("unrecognized scene format; expected PLY, SPZ v4, or SOG")]
    UnrecognizedFormat,
}

impl SceneLoadError {
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Ply(err) => err.code(),
            Self::Spz(err) => err.code(),
            Self::Sog(err) => err.code(),
            Self::Io => ErrorCode::NotFound,
            Self::UnrecognizedFormat => ErrorCode::Unsupported,
        }
    }
}

/// Detect PLY, SPZ v4, or SOG from a path name and an optional prefix.
pub fn detect_scene_format(
    path: Option<&Path>,
    prefix: &[u8],
) -> Result<SceneFormat, SceneLoadError> {
    if let Some(format) = format_from_extension(path) {
        return Ok(format);
    }
    if path.is_some_and(is_unbundled_sog_path) {
        return Ok(SceneFormat::Sog);
    }
    detect_scene_format_from_bytes(prefix)
}

/// Detect PLY, SPZ v4, or bundled SOG ZIP from file magic.
///
/// Unbundled SOG and Streamed SOG are multi-file and cannot be sniffed from a
/// single JSON payload. Streamed SOG JSON returns [`SogError::StreamingRequired`].
pub fn detect_scene_format_from_bytes(prefix: &[u8]) -> Result<SceneFormat, SceneLoadError> {
    if prefix.starts_with(SPZ_MAGIC) {
        return Ok(SceneFormat::SpzV4);
    }
    if prefix.len() >= 4 && prefix.starts_with(b"ply") && matches!(prefix[3], b'\n' | b'\r' | b' ')
    {
        return Ok(SceneFormat::Ply);
    }
    if is_zip_magic(prefix) {
        return Ok(SceneFormat::Sog);
    }
    reject_sog_bytes(prefix)?;
    Err(SceneLoadError::UnrecognizedFormat)
}

pub fn load_scene_path(path: &Path) -> Result<SceneLoadResult, SceneLoadError> {
    if is_streamed_sog_path(path) {
        return Err(SogError::StreamingRequired.into());
    }
    if is_unbundled_sog_path(path) {
        return Ok(from_sog(decode_sog_dir(path)?));
    }
    if let Some(format) = format_from_extension(Some(path)) {
        return load_known_path(path, format);
    }
    let input = fs::read(path).map_err(|_| SceneLoadError::Io)?;
    parse_scene_bytes(&input)
}

pub fn parse_scene_bytes(input: &[u8]) -> Result<SceneLoadResult, SceneLoadError> {
    match detect_scene_format_from_bytes(input)? {
        SceneFormat::Ply => Ok(from_ply(parse_ply_bytes(input)?)),
        SceneFormat::SpzV4 => Ok(from_spz(parse_spz_bytes(input)?)),
        SceneFormat::Sog => Ok(from_sog(decode_sog_archive(input)?)),
    }
}

fn reject_sog_bytes(input: &[u8]) -> Result<(), SceneLoadError> {
    let Ok(text) = std::str::from_utf8(input) else {
        return Ok(());
    };
    let trimmed = text.trim_start();
    if !trimmed.starts_with('{') {
        return Ok(());
    }
    if parse_lod_meta(trimmed).is_ok() {
        return Err(SogError::StreamingRequired.into());
    }
    if parse_chunk_meta(trimmed).is_ok() {
        return Err(
            SogError::Malformed("unbundled SOG requires meta.json plus sibling images").into(),
        );
    }
    Ok(())
}

fn format_from_extension(path: Option<&Path>) -> Option<SceneFormat> {
    let ext = path
        .and_then(Path::extension)?
        .to_str()?
        .to_ascii_lowercase();
    match ext.as_str() {
        "ply" => Some(SceneFormat::Ply),
        "spz" => Some(SceneFormat::SpzV4),
        "sog" => Some(SceneFormat::Sog),
        _ => None,
    }
}

fn load_known_path(path: &Path, format: SceneFormat) -> Result<SceneLoadResult, SceneLoadError> {
    match format {
        SceneFormat::Ply => Ok(from_ply(load_ply(path)?)),
        SceneFormat::SpzV4 => Ok(from_spz(load_spz(path)?)),
        SceneFormat::Sog => Ok(from_sog(decode_sog_archive_path(path)?)),
    }
}

fn from_ply(loaded: PlyLoadResult) -> SceneLoadResult {
    SceneLoadResult {
        summary: SceneSummary {
            format: SceneFormat::Ply,
            gaussians: loaded.summary.gaussians,
            sh_degree: loaded.summary.sh_degree,
            has_sh_rest: loaded.summary.has_sh_rest,
        },
        scene: loaded.scene,
    }
}

fn from_spz(loaded: SpzLoadResult) -> SceneLoadResult {
    SceneLoadResult {
        summary: SceneSummary {
            format: SceneFormat::SpzV4,
            gaussians: loaded.summary.gaussians,
            sh_degree: loaded.summary.sh_degree,
            has_sh_rest: loaded.scene.sh_rest.is_some(),
        },
        scene: loaded.scene,
    }
}

fn from_sog(scene: SceneBuffers) -> SceneLoadResult {
    SceneLoadResult {
        summary: SceneSummary {
            format: SceneFormat::Sog,
            gaussians: scene.len(),
            sh_degree: scene.sh_degree,
            has_sh_rest: scene.sh_rest.is_some(),
        },
        scene,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn datasets_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/datasets")
    }

    fn ply_fixture() -> PathBuf {
        datasets_dir().join("minimal_ascii.ply")
    }

    fn spz_fixture() -> PathBuf {
        datasets_dir().join("minimal_v4_degree0.spz")
    }

    fn sog_fixture() -> PathBuf {
        datasets_dir().join("minimal_sog/meta.json")
    }

    fn bundled_sog_fixture() -> PathBuf {
        datasets_dir().join("minimal.sog")
    }

    fn streamed_fixture() -> PathBuf {
        datasets_dir().join("minimal_streamed_sog/lod-meta.json")
    }

    #[test]
    fn detects_ply_and_spz_magic() {
        assert_eq!(
            detect_scene_format_from_bytes(b"ply\nformat ascii 1.0\n").unwrap(),
            SceneFormat::Ply
        );
        assert_eq!(
            detect_scene_format_from_bytes(b"NGSP\x04\x00\x00\x00").unwrap(),
            SceneFormat::SpzV4
        );
        assert_eq!(
            detect_scene_format_from_bytes(b"PK\x03\x04"),
            Ok(SceneFormat::Sog)
        );
        assert_eq!(
            detect_scene_format_from_bytes(b"XXXX"),
            Err(SceneLoadError::UnrecognizedFormat)
        );
    }

    #[test]
    fn extension_wins_over_magic() {
        assert_eq!(
            detect_scene_format(Some(Path::new("scene.spz")), b"ply\n").unwrap(),
            SceneFormat::SpzV4
        );
        assert_eq!(
            detect_scene_format(Some(Path::new("scene.ply")), b"NGSP").unwrap(),
            SceneFormat::Ply
        );
        assert_eq!(
            detect_scene_format(Some(Path::new("scene.sog")), b"PK\x03\x04").unwrap(),
            SceneFormat::Sog
        );
    }

    #[test]
    fn load_scene_path_reads_committed_ply_and_spz_fixtures() {
        let ply = load_scene_path(&ply_fixture()).expect("minimal PLY");
        assert_eq!(ply.summary.format, SceneFormat::Ply);
        assert_eq!(ply.summary.gaussians, 3);

        let spz = load_scene_path(&spz_fixture()).expect("minimal SPZ");
        assert_eq!(spz.summary.format, SceneFormat::SpzV4);
        assert_eq!(spz.summary.gaussians, 8);
        assert_eq!(spz.summary.sh_degree, 0);
        assert!(!spz.summary.has_sh_rest);
    }

    #[test]
    fn parse_scene_bytes_sniffs_committed_fixtures() {
        let ply_bytes = fs::read(ply_fixture()).unwrap();
        let ply = parse_scene_bytes(&ply_bytes).expect("PLY bytes");
        assert_eq!(ply.summary.format, SceneFormat::Ply);
        assert_eq!(ply.summary.gaussians, 3);

        let spz_bytes = fs::read(spz_fixture()).unwrap();
        let spz = parse_scene_bytes(&spz_bytes).expect("SPZ bytes");
        assert_eq!(spz.summary.format, SceneFormat::SpzV4);
        assert_eq!(spz.summary.gaussians, 8);
    }

    #[test]
    fn unknown_extension_sniffs_spz_magic() {
        let spz_bytes = fs::read(spz_fixture()).unwrap();
        let path = std::env::temp_dir().join(format!("gsplat-io-sniff-{}.bin", std::process::id()));
        fs::write(&path, &spz_bytes).unwrap();
        let loaded = load_scene_path(&path).expect("sniff SPZ from unknown extension");
        let _ = fs::remove_file(&path);
        assert_eq!(loaded.summary.format, SceneFormat::SpzV4);
        assert_eq!(loaded.summary.gaussians, 8);
    }

    #[test]
    fn load_scene_path_rejects_streamed_sog_index() {
        if !streamed_fixture().is_file() {
            return;
        }
        assert_eq!(
            load_scene_path(&streamed_fixture()),
            Err(SceneLoadError::Sog(SogError::StreamingRequired))
        );
    }

    #[test]
    fn parse_scene_bytes_rejects_streamed_sog_json() {
        if !streamed_fixture().is_file() {
            return;
        }
        let bytes = fs::read(streamed_fixture()).unwrap();
        assert_eq!(
            parse_scene_bytes(&bytes),
            Err(SceneLoadError::Sog(SogError::StreamingRequired))
        );
    }

    #[test]
    fn load_scene_path_reads_committed_unbundled_sog() {
        if !sog_fixture().is_file() {
            return;
        }
        let loaded = load_scene_path(&sog_fixture()).expect("unbundled SOG");
        assert_eq!(loaded.summary.format, SceneFormat::Sog);
        assert_eq!(loaded.summary.gaussians, 2);
    }

    #[test]
    fn load_scene_path_reads_committed_bundled_sog() {
        if !bundled_sog_fixture().is_file() {
            return;
        }
        let loaded = load_scene_path(&bundled_sog_fixture()).expect("bundled SOG");
        assert_eq!(loaded.summary.format, SceneFormat::Sog);
        assert_eq!(loaded.summary.gaussians, 2);

        let bytes = fs::read(bundled_sog_fixture()).unwrap();
        let parsed = parse_scene_bytes(&bytes).expect("bundled SOG bytes");
        assert_eq!(parsed.summary.format, SceneFormat::Sog);
        assert_eq!(parsed.summary.gaussians, 2);
    }
}
