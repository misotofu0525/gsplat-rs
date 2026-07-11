use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

pub const SCHEMA: &str = "gsplat-benchmark/v1";

#[derive(Debug, Clone, Serialize)]
pub struct FrameSample {
    pub schema: &'static str,
    pub record_type: &'static str,
    pub run_id: String,
    pub frame_index: u64,
    pub elapsed_ns: u64,
    pub call_ms: f64,
    pub frame_wall_ms: f64,
    pub preprocess_ms: f64,
    pub sort_ms: f64,
    pub geometry_submit_ms: f64,
    pub gpu_wait_ms: Option<f64>,
    pub gpu_complete_ms: Option<f64>,
    pub visible: u64,
    pub drawn: u64,
    pub sort_refreshed: Option<bool>,
}

impl FrameSample {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SCHEMA || self.record_type != "frame" || self.run_id.is_empty() {
            return Err("invalid benchmark frame identity".to_owned());
        }
        for (name, value) in [
            ("call_ms", Some(self.call_ms)),
            ("frame_wall_ms", Some(self.frame_wall_ms)),
            ("preprocess_ms", Some(self.preprocess_ms)),
            ("sort_ms", Some(self.sort_ms)),
            ("geometry_submit_ms", Some(self.geometry_submit_ms)),
            ("gpu_wait_ms", self.gpu_wait_ms),
            ("gpu_complete_ms", self.gpu_complete_ms),
        ] {
            if let Some(value) = value
                && (!value.is_finite() || value < 0.0)
            {
                return Err(format!(
                    "benchmark frame {name} must be finite and non-negative"
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Manifest {
    pub schema: &'static str,
    pub record_type: &'static str,
    pub run_id: String,
    pub identity: Identity,
    pub build: Build,
    pub dataset: Dataset,
    pub trace: Trace,
    pub renderer: Renderer,
    pub display: Display,
    pub environment: Environment,
    pub unavailable_fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Identity {
    pub series_id: String,
    pub started_at_utc: String,
    pub ended_at_utc: String,
    pub measurement_started_at_utc: String,
    pub measurement_ended_at_utc: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Build {
    pub repository_commit: String,
    pub dirty: bool,
    pub profile: String,
    pub package_version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Dataset {
    pub id: String,
    pub sha256: String,
    pub bytes: u64,
    pub splat_count: u64,
    pub sh_degree: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileIdentity {
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Trace {
    pub id: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Renderer {
    pub implementation: String,
    pub path: String,
    pub backend: String,
    pub sort_policy: String,
    pub resource_preflight: Option<ResourcePreflight>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResourcePreflight {
    pub path: String,
    pub splat_count: u64,
    pub sh_degree: u8,
    pub storage_binding_limit_bytes: u64,
    pub max_buffer_size_bytes: u64,
    pub limiting_resource: String,
    pub max_direct_splats: u64,
    pub remediation: String,
    pub requirements: Vec<ResourceRequirement>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResourceRequirement {
    pub resource: String,
    pub required_bytes: u64,
    pub limit_bytes: u64,
    pub fits: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Display {
    pub width: u32,
    pub height: u32,
    pub dpr: f64,
    pub refresh_hz: f64,
    pub frame_budget_ms: f64,
    pub refresh_hz_source: String,
    pub frame_budget_source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Environment {
    pub platform: String,
    pub os: String,
    pub device: Option<String>,
    pub browser: Option<String>,
    pub adapter: Option<String>,
    pub adapter_device_type: Option<String>,
    pub driver: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Distribution {
    pub count: usize,
    pub mean: f64,
    pub p50: f64,
    pub p90: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Distributions {
    pub call_ms: Option<Distribution>,
    pub frame_wall_ms: Option<Distribution>,
    pub preprocess_ms: Option<Distribution>,
    pub sort_ms: Option<Distribution>,
    pub geometry_submit_ms: Option<Distribution>,
    pub gpu_wait_ms: Option<Distribution>,
    pub gpu_complete_ms: Option<Distribution>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub schema: &'static str,
    pub record_type: &'static str,
    pub run_id: String,
    pub sample_count: usize,
    pub warmup_count: usize,
    pub frame_budget_ms: f64,
    pub missed_frame_count: usize,
    pub distributions: Distributions,
}

#[derive(Debug, Clone)]
pub struct ArtifactContext {
    pub run_id: String,
    pub series_id: String,
    pub started_at_utc: String,
    pub measurement_started_at_utc: String,
    pub measurement_ended_at_utc: String,
    pub build: Build,
    pub dataset: Dataset,
    pub trace: Trace,
    pub renderer: Renderer,
    pub display: Display,
    pub environment: Environment,
    pub unavailable_fields: Vec<String>,
}

pub fn summarize(
    run_id: &str,
    warmup_count: usize,
    frame_budget_ms: f64,
    frames: &[FrameSample],
) -> Result<Summary, String> {
    if run_id.is_empty() || frames.is_empty() {
        return Err("benchmark summary requires a run id and at least one frame".to_owned());
    }
    if !frame_budget_ms.is_finite() || frame_budget_ms <= 0.0 {
        return Err("frame budget must be finite and positive".to_owned());
    }
    for (index, frame) in frames.iter().enumerate() {
        frame.validate()?;
        if frame.run_id != run_id || frame.frame_index != index as u64 {
            return Err(
                "benchmark frames must have matching identity and contiguous indices".to_owned(),
            );
        }
        if index > 0 && frame.elapsed_ns < frames[index - 1].elapsed_ns {
            return Err("benchmark frame elapsed_ns must be monotonic".to_owned());
        }
    }

    let required = |value: fn(&FrameSample) -> f64| {
        distribution(frames.iter().map(|frame| Some(value(frame))))
    };
    let optional = |value: fn(&FrameSample) -> Option<f64>| distribution(frames.iter().map(value));
    let missed_frame_count = frames
        .iter()
        .filter(|frame| frame.frame_wall_ms > frame_budget_ms)
        .count();

    Ok(Summary {
        schema: SCHEMA,
        record_type: "summary",
        run_id: run_id.to_owned(),
        sample_count: frames.len(),
        warmup_count,
        frame_budget_ms,
        missed_frame_count,
        distributions: Distributions {
            call_ms: required(|frame| frame.call_ms),
            frame_wall_ms: required(|frame| frame.frame_wall_ms),
            preprocess_ms: required(|frame| frame.preprocess_ms),
            sort_ms: required(|frame| frame.sort_ms),
            geometry_submit_ms: required(|frame| frame.geometry_submit_ms),
            gpu_wait_ms: optional(|frame| frame.gpu_wait_ms),
            gpu_complete_ms: optional(|frame| frame.gpu_complete_ms),
        },
    })
}

pub fn write_artifacts(
    directory: &Path,
    context: ArtifactContext,
    warmup_count: usize,
    frames: &[FrameSample],
) -> Result<Summary, String> {
    if !context.display.frame_budget_ms.is_finite() || context.display.frame_budget_ms <= 0.0 {
        return Err("artifact display frame budget must be finite and positive".to_owned());
    }
    if directory.exists() {
        return Err(format!(
            "benchmark artifact directory already exists: {}",
            directory.display()
        ));
    }
    let parent = directory
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create artifact parent directory: {error}"))?;
    let staging = staging_directory_path(directory)?;
    fs::create_dir(&staging)
        .map_err(|error| format!("failed to create artifact staging directory: {error}"))?;
    let summary = summarize(
        &context.run_id,
        warmup_count,
        context.display.frame_budget_ms,
        frames,
    )?;
    let manifest = Manifest {
        schema: SCHEMA,
        record_type: "manifest",
        run_id: context.run_id,
        identity: Identity {
            series_id: context.series_id,
            started_at_utc: context.started_at_utc,
            ended_at_utc: utc_now()?,
            measurement_started_at_utc: context.measurement_started_at_utc,
            measurement_ended_at_utc: context.measurement_ended_at_utc,
        },
        build: context.build,
        dataset: context.dataset,
        trace: context.trace,
        renderer: context.renderer,
        display: context.display,
        environment: context.environment,
        unavailable_fields: context.unavailable_fields,
    };

    let write_result = (|| {
        write_json_atomic(&staging.join("manifest.json"), &manifest)?;
        write_frames_atomic(&staging.join("frames.jsonl"), frames)?;
        write_json_atomic(&staging.join("summary.json"), &summary)?;
        Ok::<(), String>(())
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    if let Err(error) = fs::rename(&staging, directory) {
        let _ = fs::remove_dir_all(&staging);
        return Err(format!(
            "failed to publish benchmark artifact directory {}: {error}",
            directory.display()
        ));
    }
    Ok(summary)
}

pub fn distribution<I>(values: I) -> Option<Distribution>
where
    I: IntoIterator<Item = Option<f64>>,
{
    let mut values = values.into_iter().flatten().collect::<Vec<_>>();
    if values.is_empty()
        || values
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0)
    {
        return None;
    }
    let count = values.len();
    let mut sum = 0.0_f64;
    for value in &values {
        sum += *value;
    }
    let mean = sum / count as f64;
    values.sort_by(f64::total_cmp);
    Some(Distribution {
        count,
        mean,
        p50: nearest_rank(&values, 0.50),
        p90: nearest_rank(&values, 0.90),
        p95: nearest_rank(&values, 0.95),
        p99: nearest_rank(&values, 0.99),
        max: values[count - 1],
    })
}

pub fn dataset_with_identity(
    path: &Path,
    splat_count: usize,
    sh_degree: u8,
    identity: FileIdentity,
) -> Result<Dataset, String> {
    let id = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("dataset")
        .to_owned();
    Ok(Dataset {
        id,
        sha256: identity.sha256,
        bytes: identity.bytes,
        splat_count: u64::try_from(splat_count)
            .map_err(|_| "dataset splat count exceeds u64".to_owned())?,
        sh_degree,
    })
}

pub fn file_identity(path: &Path) -> Result<FileIdentity, String> {
    let metadata =
        fs::metadata(path).map_err(|error| format!("failed to read dataset metadata: {error}"))?;
    Ok(FileIdentity {
        sha256: sha256_file(path)?,
        bytes: metadata.len(),
    })
}

pub fn trace(id: &str, canonical_bytes: &[u8]) -> Trace {
    Trace {
        id: id.to_owned(),
        sha256: sha256_bytes(canonical_bytes),
    }
}

pub fn repository_build() -> Result<Build, String> {
    let repository_commit = command_stdout("git", &["rev-parse", "HEAD"])?;
    let status = command_stdout("git", &["status", "--porcelain"])?;
    Ok(Build {
        repository_commit,
        dirty: !status.is_empty(),
        profile: if cfg!(debug_assertions) {
            "debug".to_owned()
        } else {
            "release".to_owned()
        },
        package_version: env!("CARGO_PKG_VERSION").to_owned(),
    })
}

pub fn default_run_id() -> Result<String, String> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock is before UNIX epoch: {error}"))?
        .as_millis();
    Ok(format!("run-{millis}-{}", std::process::id()))
}

pub fn utc_now() -> Result<String, String> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| format!("failed to format UTC timestamp: {error}"))
}

fn nearest_rank(sorted: &[f64], percentile: f64) -> f64 {
    let rank = (percentile * sorted.len() as f64).ceil().max(1.0) as usize;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let temp = temporary_path(path);
    let file = File::create(&temp)
        .map_err(|error| format!("failed to create {}: {error}", temp.display()))?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, value)
        .map_err(|error| format!("failed to encode {}: {error}", path.display()))?;
    writer
        .write_all(b"\n")
        .map_err(|error| format!("failed to finish {}: {error}", path.display()))?;
    writer
        .flush()
        .map_err(|error| format!("failed to flush {}: {error}", path.display()))?;
    fs::rename(&temp, path)
        .map_err(|error| format!("failed to publish {}: {error}", path.display()))
}

fn write_frames_atomic(path: &Path, frames: &[FrameSample]) -> Result<(), String> {
    let temp = temporary_path(path);
    let file = File::create(&temp)
        .map_err(|error| format!("failed to create {}: {error}", temp.display()))?;
    let mut writer = BufWriter::new(file);
    for frame in frames {
        frame.validate()?;
        serde_json::to_writer(&mut writer, frame)
            .map_err(|error| format!("failed to encode {}: {error}", path.display()))?;
        writer
            .write_all(b"\n")
            .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
    }
    writer
        .flush()
        .map_err(|error| format!("failed to flush {}: {error}", path.display()))?;
    fs::rename(&temp, path)
        .map_err(|error| format!("failed to publish {}: {error}", path.display()))
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut value = path.as_os_str().to_owned();
    value.push(format!(".tmp-{}", std::process::id()));
    PathBuf::from(value)
}

fn staging_directory_path(directory: &Path) -> Result<PathBuf, String> {
    let parent = directory
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = directory
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "artifact directory must have a UTF-8 file name".to_owned())?;
    Ok(parent.join(format!(
        ".{name}.stage-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("system clock is before UNIX epoch: {error}"))?
            .as_nanos()
    )))
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let file =
        File::open(path).map_err(|error| format!("failed to open dataset for hashing: {error}"))?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| format!("failed to hash dataset: {error}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn command_stdout(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|error| format!("failed to run {program}: {error}"))?;
    if !output.status.success() {
        return Err(format!("{program} exited with {}", output.status));
    }
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_owned())
        .map_err(|error| format!("{program} output is not UTF-8: {error}"))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;

    use super::{
        ArtifactContext, Build, Dataset, Display, Environment, FrameSample, Renderer, SCHEMA,
        Trace, distribution, summarize, write_artifacts,
    };

    fn sample(run_id: &str, index: u64, value: f64) -> FrameSample {
        FrameSample {
            schema: SCHEMA,
            record_type: "frame",
            run_id: run_id.to_owned(),
            frame_index: index,
            elapsed_ns: index * 1_000,
            call_ms: value,
            frame_wall_ms: value,
            preprocess_ms: value / 10.0,
            sort_ms: value / 5.0,
            geometry_submit_ms: value / 4.0,
            gpu_wait_ms: None,
            gpu_complete_ms: None,
            visible: 2,
            drawn: 2,
            sort_refreshed: None,
        }
    }

    #[test]
    fn nearest_rank_matches_golden_five_frame_vector() {
        let distribution = distribution([1.0, 2.0, 3.0, 4.0, 5.0].map(Some)).unwrap();
        assert_eq!(distribution.mean, 3.0);
        assert_eq!(distribution.p50, 3.0);
        assert_eq!(distribution.p90, 5.0);
        assert_eq!(distribution.p95, 5.0);
        assert_eq!(distribution.p99, 5.0);
        assert_eq!(distribution.max, 5.0);
    }

    #[test]
    fn summary_recomputes_missed_frames_and_null_optional_metrics() {
        let frames = (0..5)
            .map(|index| sample("run", index, index as f64 + 1.0))
            .collect::<Vec<_>>();
        let summary = summarize("run", 2, 3.5, &frames).unwrap();

        assert_eq!(summary.sample_count, 5);
        assert_eq!(summary.missed_frame_count, 2);
        assert!(summary.distributions.gpu_wait_ms.is_none());
        assert!(summary.distributions.gpu_complete_ms.is_none());
    }

    #[test]
    fn invalid_frame_values_are_rejected_before_encoding() {
        let mut frame = sample("run", 0, 1.0);
        frame.call_ms = f64::NAN;
        assert!(frame.validate().is_err());
    }

    #[test]
    fn sha256_file_matches_known_value() {
        let path = std::env::temp_dir().join(format!(
            "gsplat-benchmark-sha-{}-{}.txt",
            std::process::id(),
            super::default_run_id().unwrap()
        ));
        fs::write(&path, b"abc").unwrap();
        let actual = super::sha256_file(&path).unwrap();
        let _ = fs::remove_file(path);

        assert_eq!(
            actual,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn producer_output_passes_canonical_validator_and_rejects_reuse() {
        let root = std::env::temp_dir().join(format!(
            "gsplat-benchmark-artifact-{}-{}",
            std::process::id(),
            super::default_run_id().unwrap()
        ));
        let directory = root.join("run");
        let frames = (0..5)
            .map(|index| sample("validator-run", index, index as f64 + 1.0))
            .collect::<Vec<_>>();
        let context = ArtifactContext {
            run_id: "validator-run".to_owned(),
            series_id: "validator-series".to_owned(),
            started_at_utc: "2026-07-11T00:00:00Z".to_owned(),
            measurement_started_at_utc: "2026-07-11T00:00:00.100Z".to_owned(),
            measurement_ended_at_utc: "2026-07-11T00:00:00.900Z".to_owned(),
            build: Build {
                repository_commit: "0123456789abcdef0123456789abcdef01234567".to_owned(),
                dirty: false,
                profile: "test".to_owned(),
                package_version: env!("CARGO_PKG_VERSION").to_owned(),
            },
            dataset: Dataset {
                id: "fixture".to_owned(),
                sha256: "a".repeat(64),
                bytes: 1,
                splat_count: 2,
                sh_degree: 0,
            },
            trace: Trace {
                id: "static".to_owned(),
                sha256: "b".repeat(64),
            },
            renderer: Renderer {
                implementation: "gsplat-rs".to_owned(),
                path: "sorted_index_direct".to_owned(),
                backend: "test".to_owned(),
                sort_policy: "cpu_every_frame".to_owned(),
                resource_preflight: None,
            },
            display: Display {
                width: 640,
                height: 480,
                dpr: 1.0,
                refresh_hz: 60.0,
                frame_budget_ms: 3.5,
                refresh_hz_source: "configured".to_owned(),
                frame_budget_source: "configured".to_owned(),
            },
            environment: Environment {
                platform: "test".to_owned(),
                os: "test-os".to_owned(),
                device: None,
                browser: None,
                adapter: None,
                adapter_device_type: None,
                driver: None,
            },
            unavailable_fields: vec![
                "environment.device".to_owned(),
                "environment.browser".to_owned(),
                "environment.adapter".to_owned(),
                "environment.adapter_device_type".to_owned(),
                "environment.driver".to_owned(),
                "frames[*].gpu_wait_ms".to_owned(),
                "frames[*].gpu_complete_ms".to_owned(),
                "frames[*].sort_refreshed".to_owned(),
            ],
        };

        write_artifacts(&directory, context.clone(), 2, &frames).unwrap();
        let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let status = Command::new("python3")
            .arg(repository.join("tests/perf/validate-benchmark-artifacts.py"))
            .arg(&directory)
            .status()
            .unwrap();
        assert!(status.success());

        let error = write_artifacts(&directory, context, 2, &frames).unwrap_err();
        assert!(error.contains("already exists"));
        let _ = fs::remove_dir_all(root);
    }
}
