//! Repository-local orchestration for the frozen formal S1 Bonsai author.

#[path = "formal_s1_bonsai/authoring.rs"]
mod authoring;

use authoring::{
    AuthorRequest, AuthorityPaths, Result, admit_output, author_bundle, bonsai_authority,
    builder_identity, estimate, invalid, verify_authority,
};
use gsplat_hierarchy::BuildConfig;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
struct Args {
    source: PathBuf,
    cameras: PathBuf,
    dataset_manifest: PathBuf,
    output: PathBuf,
    source_leaves_per_node: u32,
    estimate_only: bool,
}

impl Args {
    fn parse() -> Result<Self> {
        let mut source = None;
        let mut cameras = None;
        let mut dataset_manifest = None;
        let mut output = None;
        let mut source_leaves_per_node = BuildConfig::default().source_leaves_per_node;
        let mut estimate_only = false;
        let mut arguments = env::args().skip(1);
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--source" => source = Some(required_path(&mut arguments, "--source")?),
                "--cameras" => cameras = Some(required_path(&mut arguments, "--cameras")?),
                "--dataset-manifest" => {
                    dataset_manifest = Some(required_path(&mut arguments, "--dataset-manifest")?)
                }
                "--output" => output = Some(required_path(&mut arguments, "--output")?),
                "--source-leaves-per-node" => {
                    let value = arguments.next().ok_or_else(|| {
                        invalid("--source-leaves-per-node requires a positive u32")
                    })?;
                    source_leaves_per_node = value
                        .parse::<u32>()
                        .map_err(|_| invalid("--source-leaves-per-node must be a positive u32"))?;
                    if source_leaves_per_node == 0 {
                        return Err(invalid("--source-leaves-per-node must be a positive u32"));
                    }
                }
                "--estimate-only" => estimate_only = true,
                "--help" | "-h" => {
                    println!("{}", usage());
                    std::process::exit(0);
                }
                _ => {
                    return Err(invalid(format!(
                        "unknown argument {argument:?}\n{}",
                        usage()
                    )));
                }
            }
        }
        Ok(Self {
            source: source.ok_or_else(|| invalid(format!("missing --source\n{}", usage())))?,
            cameras: cameras.ok_or_else(|| invalid(format!("missing --cameras\n{}", usage())))?,
            dataset_manifest: dataset_manifest
                .ok_or_else(|| invalid(format!("missing --dataset-manifest\n{}", usage())))?,
            output: output.ok_or_else(|| invalid(format!("missing --output\n{}", usage())))?,
            source_leaves_per_node,
            estimate_only,
        })
    }
}

fn required_path(arguments: &mut impl Iterator<Item = String>, flag: &str) -> Result<PathBuf> {
    arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| invalid(format!("{flag} requires a path")))
}

fn usage() -> &'static str {
    concat!(
        "usage: author-formal-s1-bonsai \\\n",
        "  --source <point_cloud.ply> \\\n",
        "  --cameras <cameras.json> \\\n",
        "  --dataset-manifest <canonical bonsai.local-candidate.json> \\\n",
        "  --output <fresh-directory> \\\n",
        "  [--source-leaves-per-node <positive-u32>] [--estimate-only]",
    )
}

fn run() -> Result<()> {
    let args = Args::parse()?;
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let canonical_dataset_manifest =
        fs::canonicalize(repository.join("tests/perf/datasets/bonsai.local-candidate.json"))?;
    let paths = AuthorityPaths {
        source: &args.source,
        cameras: &args.cameras,
        dataset_manifest: &args.dataset_manifest,
        canonical_dataset_manifest: &canonical_dataset_manifest,
    };
    let expected = bonsai_authority();
    let authority = verify_authority(paths, expected)?;
    let estimate = estimate(expected, args.source_leaves_per_node)?;
    if args.estimate_only {
        println!("{}", serde_json::to_string_pretty(&estimate.document)?);
        return Ok(());
    }

    let admission = admit_output(&args.output, &estimate)?;
    eprintln!(
        "admission=accepted estimated_output_bytes={} available_disk_bytes={} required_disk_bytes={}",
        estimate.estimated_output_bytes, admission.available_bytes, admission.required_bytes
    );
    let builder = builder_identity()?;
    let outcome = author_bundle(AuthorRequest {
        paths,
        output: &args.output,
        expected,
        authority: &authority,
        builder: &builder,
        source_leaves_per_node: args.source_leaves_per_node,
    })?;
    println!(
        "authored={} builder_commit={} executable_sha256={} manifest_sha256={} receipt_sha256={} pages={} receipt={}",
        args.output.display(),
        builder.repository_commit,
        builder.executable_sha256,
        outcome.manifest_sha256,
        outcome.receipt_sha256,
        outcome.page_count,
        args.output.join("cut-receipt.json").display(),
    );
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("formal S1 Bonsai authoring failed: {error}");
        std::process::exit(1);
    }
}
