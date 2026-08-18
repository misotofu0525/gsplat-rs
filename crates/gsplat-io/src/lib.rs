//! Whole-scene PLY / SPZ v4 import facade.

use std::fs;
use std::path::Path;

use gsplat_core::{ErrorCode, SceneBuffers};
use gsplat_io_ply::{PlyLoadError, PlyLoadResult, load_ply, parse_ply_bytes};
use gsplat_io_spz::{SpzLoadError, SpzLoadResult, load_spz, parse_spz_bytes};
use thiserror::Error;

const SPZ_MAGIC: &[u8; 4] = b"NGSP";

/// On-disk / in-memory scene format selected by the import facade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneFormat {
    Ply,
    SpzV4,
}

impl SceneFormat {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ply => "ply",
            Self::SpzV4 => "spz-v4",
        }
    }
}

/// Summary shared by PLY and SPZ whole-scene loads.
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
    #[error("I/O error while reading scene")]
    Io,
    #[error("unrecognized scene format; expected PLY or SPZ v4")]
    UnrecognizedFormat,
}

impl SceneLoadError {
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Ply(err) => err.code(),
            Self::Spz(err) => err.code(),
            Self::Io => ErrorCode::NotFound,
            Self::UnrecognizedFormat => ErrorCode::Unsupported,
        }
    }
}

/// Detect PLY vs SPZ v4 from a path extension and an optional byte prefix.
pub fn detect_scene_format(
    path: Option<&Path>,
    prefix: &[u8],
) -> Result<SceneFormat, SceneLoadError> {
    if let Some(format) = format_from_extension(path) {
        return Ok(format);
    }
    detect_scene_format_from_bytes(prefix)
}

/// Detect PLY vs SPZ v4 from file magic.
pub fn detect_scene_format_from_bytes(prefix: &[u8]) -> Result<SceneFormat, SceneLoadError> {
    if prefix.starts_with(SPZ_MAGIC) {
        return Ok(SceneFormat::SpzV4);
    }
    if prefix.len() >= 4 && prefix.starts_with(b"ply") && matches!(prefix[3], b'\n' | b'\r' | b' ')
    {
        return Ok(SceneFormat::Ply);
    }
    Err(SceneLoadError::UnrecognizedFormat)
}

pub fn load_scene_path(path: &Path) -> Result<SceneLoadResult, SceneLoadError> {
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
    }
}

fn format_from_extension(path: Option<&Path>) -> Option<SceneFormat> {
    let ext = path
        .and_then(Path::extension)?
        .to_str()?
        .to_ascii_lowercase();
    match ext.as_str() {
        "ply" => Some(SceneFormat::Ply),
        "spz" => Some(SceneFormat::SpzV4),
        _ => None,
    }
}

fn load_known_path(path: &Path, format: SceneFormat) -> Result<SceneLoadResult, SceneLoadError> {
    match format {
        SceneFormat::Ply => Ok(from_ply(load_ply(path)?)),
        SceneFormat::SpzV4 => Ok(from_spz(load_spz(path)?)),
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
}
