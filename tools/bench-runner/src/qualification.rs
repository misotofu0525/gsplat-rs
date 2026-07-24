use std::fs;
use std::path::Path;

use gsplat_io_ply::load_ply_summary;
use gsplat_render_wgpu::GeometryPath;
use serde::Deserialize;

use crate::artifact::{self, FileIdentity};
use crate::scene::SceneLoadReceipt;
use crate::trace::{Playback, TraceSelection};

#[derive(Debug, Clone, Copy)]
struct DatasetContract {
    manifest_path: &'static str,
    manifest_local_path: &'static str,
    id: &'static str,
    sha256: &'static str,
    bytes: u64,
    splat_count: usize,
    sh_degree: u8,
}

#[derive(Debug, Clone, Copy)]
struct TraceContract {
    id: &'static str,
    content_sha256: &'static str,
    raw_sha256: &'static str,
    width: u32,
    height: u32,
    frame_count: usize,
    frame_indices: &'static [usize],
    warmup_frames: usize,
    measured_frames: usize,
}

#[derive(Debug, Clone, Copy)]
struct FullQualityContract {
    dataset: DatasetContract,
    trace: TraceContract,
}

const M1_KITSUNE: FullQualityContract = FullQualityContract {
    dataset: DatasetContract {
        manifest_path: "tests/perf/datasets/kitsune.json",
        manifest_local_path: "tests/datasets/external/wakufactory_kitune/kitune1.ply",
        id: "kitsune",
        sha256: "3bea1ec48ea91861fc8fad1df688a2cdb1db9b103735498b35d16d146f2551a2",
        bytes: 65_892_441,
        splat_count: 279_199,
        sh_degree: 3,
    },
    trace: TraceContract {
        id: "candidate-kitsune-quality-2view-1920x1080-v1",
        content_sha256: "8821c193506cdf7d67aa200248a45088c4750a3cd128ee29f6e2dc2d3a5bdb99",
        raw_sha256: "c996e5fe757d9d6661cce9f1dc303edcfbe8e12059e08bf8ab9f8eb566e54657",
        width: 1920,
        height: 1080,
        frame_count: 2,
        frame_indices: &[0, 1],
        warmup_frames: 20,
        measured_frames: 80,
    },
};

// M1 freezes one input. Extending a later package means adding another complete
// identity tuple here, rather than accepting a path-name convention.
const FULL_QUALITY_CONTRACTS: &[FullQualityContract] = &[M1_KITSUNE];

#[derive(Debug, Deserialize)]
struct DatasetManifest {
    schema: String,
    id: String,
    qualification_status: String,
    local_path: String,
    sha256: String,
    bytes: u64,
    splat_count: usize,
    sh_degree: u8,
}

#[derive(Debug, Clone, Copy)]
pub struct ValidatedFullQuality {
    contract: FullQualityContract,
}

impl ValidatedFullQuality {
    pub const fn dataset_id(self) -> &'static str {
        self.contract.dataset.id
    }

    pub fn validate_scene_receipt(self, receipt: SceneLoadReceipt) -> Result<(), String> {
        let dataset = self.contract.dataset;
        for (name, actual) in [
            ("source", receipt.source_count),
            ("decoded", receipt.decoded_count),
            ("encoded", receipt.encoded_count),
            ("resident", receipt.resident_count),
            ("addressable", receipt.addressable_count),
        ] {
            if actual != dataset.splat_count {
                return Err(format!(
                    "--full-quality {name} splat count must equal the frozen dataset count {} (got {actual})",
                    dataset.splat_count
                ));
            }
        }
        for (name, actual) in [
            ("source", receipt.source_sh_degree),
            ("resident", receipt.resident_sh_degree),
        ] {
            if actual != dataset.sh_degree {
                return Err(format!(
                    "--full-quality {name} SH degree must equal the frozen source SH degree {} (got {actual})",
                    dataset.sh_degree
                ));
            }
        }
        Ok(())
    }
}

pub fn validate_inputs(
    repository_root: &Path,
    dataset_path: &Path,
    dataset_identity: &FileIdentity,
    geometry_path: GeometryPath,
    playback: &Playback,
    trace_path: &Path,
) -> Result<ValidatedFullQuality, String> {
    validate_geometry_path(geometry_path)?;
    let contract = select_dataset_contract(repository_root, dataset_identity)?;
    let summary = load_ply_summary(dataset_path).map_err(|error| {
        format!(
            "cannot inspect --full-quality dataset {}: {error}",
            dataset_path.display()
        )
    })?;
    if summary.gaussians != contract.dataset.splat_count
        || summary.sh_degree != contract.dataset.sh_degree
    {
        return Err(format!(
            "--full-quality PLY summary does not match frozen dataset identity: expected splats={} SH{}, got splats={} SH{}",
            contract.dataset.splat_count,
            contract.dataset.sh_degree,
            summary.gaussians,
            summary.sh_degree
        ));
    }
    validate_trace_contract(contract.trace, playback, trace_path)?;
    Ok(ValidatedFullQuality { contract })
}

fn validate_geometry_path(path: GeometryPath) -> Result<(), String> {
    match path {
        GeometryPath::SortedIndexDirect | GeometryPath::PackedAtlas => Ok(()),
        GeometryPath::PagedActiveAtlas => Err(
            "Paged/partial residency is diagnostic only; --full-quality forbids partial, LOD, and sampled paths"
                .to_owned(),
        ),
    }
}

fn select_dataset_contract(
    repository_root: &Path,
    actual: &FileIdentity,
) -> Result<FullQualityContract, String> {
    for contract in FULL_QUALITY_CONTRACTS {
        validate_dataset_manifest(repository_root, contract.dataset)?;
        if actual.sha256 == contract.dataset.sha256 && actual.bytes == contract.dataset.bytes {
            return Ok(*contract);
        }
    }
    Err(format!(
        "--full-quality dataset identity is not an M1 formal input: sha256={} bytes={}",
        actual.sha256, actual.bytes
    ))
}

fn validate_dataset_manifest(
    repository_root: &Path,
    contract: DatasetContract,
) -> Result<(), String> {
    let path = repository_root.join(contract.manifest_path);
    let bytes = fs::read(&path)
        .map_err(|error| format!("cannot read dataset manifest {}: {error}", path.display()))?;
    let manifest: DatasetManifest = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid dataset manifest {}: {error}", path.display()))?;
    let matches = manifest.schema == "gsplat-dataset/v1"
        && manifest.id == contract.id
        && manifest.qualification_status == "qualified"
        && manifest.local_path == contract.manifest_local_path
        && manifest.sha256 == contract.sha256
        && manifest.bytes == contract.bytes
        && manifest.splat_count == contract.splat_count
        && manifest.sh_degree == contract.sh_degree;
    if !matches {
        return Err(format!(
            "dataset manifest {} does not match the frozen M1 contract",
            path.display()
        ));
    }
    Ok(())
}

fn validate_trace_contract(
    contract: TraceContract,
    playback: &Playback,
    trace_path: &Path,
) -> Result<(), String> {
    let trace = playback
        .trace()
        .ok_or_else(|| "--full-quality requires the frozen camera trace".to_owned())?;
    if (trace.display.width, trace.display.height) != (contract.width, contract.height) {
        return Err(format!(
            "--full-quality trace resolution must be {}x{} (got {}x{})",
            contract.width, contract.height, trace.display.width, trace.display.height
        ));
    }
    if trace.trace_id != contract.id
        || trace.content_sha256 != contract.content_sha256
        || trace.frames.len() != contract.frame_count
    {
        return Err("--full-quality trace is not the frozen Kitsune quality trace".to_owned());
    }
    let raw_identity = artifact::file_identity(trace_path)?;
    if raw_identity.sha256 != contract.raw_sha256 {
        return Err(format!(
            "--full-quality trace raw SHA-256 mismatch: expected {}, got {}",
            contract.raw_sha256, raw_identity.sha256
        ));
    }
    match playback.selection() {
        TraceSelection::Sequence { frame_indices }
            if frame_indices.as_slice() == contract.frame_indices => {}
        _ => {
            return Err(format!(
                "--full-quality requires the frozen trace sequence {:?}",
                contract.frame_indices
            ));
        }
    }
    if playback.warmup_count() != contract.warmup_frames
        || playback.measured_count() != contract.measured_frames
    {
        return Err(format!(
            "--full-quality requires warmup={} and measured={} frames",
            contract.warmup_frames, contract.measured_frames
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::trace::TraceRequest;

    fn repository_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn trace_path(relative: &str) -> PathBuf {
        repository_root().join(relative)
    }

    fn playback(relative: &str, warmup: usize, measured: usize) -> (Playback, PathBuf) {
        let path = trace_path(relative);
        let playback = Playback::build(TraceRequest {
            path: Some(path.clone()),
            sequence: true,
            frame_index: 0,
            frame_index_explicit: false,
            frame_indices: Some(vec![0, 1]),
            warmup_frames: Some(warmup),
            measured_frames: Some(measured),
            loops: 1,
            default_warmup_frames: 0,
            default_measured_frames: 1,
        })
        .expect("trace playback");
        (playback, path)
    }

    #[test]
    fn frozen_dataset_is_selected_by_identity_not_input_path() {
        let identity = FileIdentity {
            sha256: M1_KITSUNE.dataset.sha256.to_owned(),
            bytes: M1_KITSUNE.dataset.bytes,
        };
        let selected = select_dataset_contract(&repository_root(), &identity).unwrap();
        assert_eq!(selected.dataset.id, "kitsune");
    }

    #[test]
    fn minimal_fixture_cannot_be_promoted_to_full_quality() {
        let identity = artifact::file_identity(&trace_path("tests/datasets/minimal_ascii.ply"))
            .expect("minimal identity");
        let error = select_dataset_contract(&repository_root(), &identity).unwrap_err();
        assert!(error.contains("not an M1 formal input"));
    }

    #[test]
    fn wrong_scene_family_trace_is_rejected() {
        let (playback, path) = playback(
            "tests/perf/trace/fixtures/quality/candidate-flowers-quality-1920x1080-v1.json",
            20,
            80,
        );
        let error = validate_trace_contract(M1_KITSUNE.trace, &playback, &path).unwrap_err();
        assert!(error.contains("not the frozen Kitsune"));
    }

    #[test]
    fn low_resolution_trace_is_rejected() {
        let (playback, path) = playback(
            "tests/perf/trace/fixtures/quality/candidate-flowers-quality-640x360-v1.json",
            20,
            80,
        );
        let error = validate_trace_contract(M1_KITSUNE.trace, &playback, &path).unwrap_err();
        assert!(error.contains("trace resolution must be 1920x1080"));
    }

    #[test]
    fn implicit_schedule_drift_is_rejected() {
        let (playback, path) = playback(
            "tests/perf/trace/fixtures/quality/candidate-kitsune-quality-1920x1080-v1.json",
            19,
            80,
        );
        let error = validate_trace_contract(M1_KITSUNE.trace, &playback, &path).unwrap_err();
        assert!(error.contains("requires warmup=20 and measured=80"));
    }

    #[test]
    fn paged_partial_residency_is_rejected() {
        let error = validate_geometry_path(GeometryPath::PagedActiveAtlas).unwrap_err();
        assert!(error.contains("forbids partial, LOD, and sampled paths"));
    }
}
