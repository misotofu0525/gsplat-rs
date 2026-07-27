//! Repository-local authoring entrypoint for the frozen formal S1 Bonsai asset.
//!
//! This binary emits authored hierarchy bytes and structural coverage receipts.
//! It deliberately does not render images, qualify endpoints, or unlock S2.

use gsplat_hierarchy::{
    BuildConfig, DrawableGaussian, FormalS1Cuts, HierarchyBundle, NodeId,
    build_formal_s1_proxy_hierarchy,
};
use gsplat_io_ply::{DecodedPlySplat, load_ply_summary, visit_ply_splats};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::error::Error;
use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

const RECEIPT_SCHEMA: &str = "gsplat-formal-s1-proxy-authoring/v1";
const CONFIGURATION_SCHEMA: &str = "gsplat-formal-s1-proxy-builder-configuration/v1";
const SOURCE_DRAWABLE_SCHEMA: &str = "gsplat-drawable-gaussian-le-v1";
const DRAWABLE_ENCODED_BYTES: u64 = 240;
const PAGE_HEADER_BYTES: u64 = 32;
const RECEIPT_BYTES_PER_PAGE_ESTIMATE: u64 = 512;
const ESTIMATE_FIXED_MARGIN_BYTES: u64 = 512 * 1024 * 1024;

const BONSAI_SOURCE_LOGICAL_PATH: &str =
    "tests/datasets/external/inria_3dgs/bonsai/point_cloud.ply";
const BONSAI_SOURCE_SHA256: &str =
    "a16af6d8815498ffbf9eb5d5ee93f5bcc9dca34c4e3eb6f7a796ef9e97c0d273";
const BONSAI_SOURCE_BYTES: u64 = 308_716_644;
const BONSAI_SPLAT_COUNT: u64 = 1_244_819;
const BONSAI_SH_DEGREE: u8 = 3;
const BONSAI_CAMERA_LOGICAL_PATH: &str = "tests/datasets/external/inria_3dgs/bonsai/cameras.json";
const BONSAI_CAMERA_SHA256: &str =
    "41e623748141d5b1a292c2bcafbf9e897a3876f90c11a14618e9ac6190b05af3";
const BONSAI_CAMERA_BYTES: u64 = 116_695;
const BONSAI_CAMERA_COUNT: usize = 292;
const BONSAI_CAMERA_IDS: [u64; 2] = [0, 146];

type DynError = Box<dyn Error + Send + Sync + 'static>;
type Result<T> = std::result::Result<T, DynError>;

#[derive(Debug)]
struct AuthorError(String);

impl fmt::Display for AuthorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for AuthorError {}

fn invalid(message: impl Into<String>) -> DynError {
    Box::new(AuthorError(message.into()))
}

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
                "--source" => source = Some(required_value(&mut arguments, "--source")?),
                "--cameras" => cameras = Some(required_value(&mut arguments, "--cameras")?),
                "--dataset-manifest" => {
                    dataset_manifest = Some(required_value(&mut arguments, "--dataset-manifest")?)
                }
                "--output" => output = Some(required_value(&mut arguments, "--output")?),
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

fn required_value(arguments: &mut impl Iterator<Item = String>, flag: &str) -> Result<PathBuf> {
    arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| invalid(format!("{flag} requires a path")))
}

fn usage() -> &'static str {
    "usage: author-formal-s1-bonsai \\\n+  --source <point_cloud.ply> \\\n+  --cameras <cameras.json> \\\n+  --dataset-manifest <bonsai.local-candidate.json> \\\n+  --output <fresh-directory> \\\n+  [--source-leaves-per-node <positive-u32>] [--estimate-only]"
}

#[derive(Clone, Copy)]
struct ExpectedAuthority<'a> {
    dataset_id: &'a str,
    source_logical_path: &'a str,
    source_sha256: &'a str,
    source_bytes: u64,
    splat_count: u64,
    sh_degree: u8,
    camera_logical_path: &'a str,
    camera_sha256: &'a str,
    camera_bytes: u64,
    camera_count: usize,
    selected_camera_ids: &'a [u64],
}

const BONSAI_AUTHORITY: ExpectedAuthority<'static> = ExpectedAuthority {
    dataset_id: "bonsai",
    source_logical_path: BONSAI_SOURCE_LOGICAL_PATH,
    source_sha256: BONSAI_SOURCE_SHA256,
    source_bytes: BONSAI_SOURCE_BYTES,
    splat_count: BONSAI_SPLAT_COUNT,
    sh_degree: BONSAI_SH_DEGREE,
    camera_logical_path: BONSAI_CAMERA_LOGICAL_PATH,
    camera_sha256: BONSAI_CAMERA_SHA256,
    camera_bytes: BONSAI_CAMERA_BYTES,
    camera_count: BONSAI_CAMERA_COUNT,
    selected_camera_ids: &BONSAI_CAMERA_IDS,
};

#[derive(Debug, Deserialize)]
struct DatasetManifest {
    schema: String,
    id: String,
    qualification_status: String,
    local_path: String,
    allowed_use: String,
    redistribution: String,
    sha256: String,
    bytes: u64,
    splat_count: u64,
    sh_degree: u8,
}

struct AuthorityProof {
    dataset_manifest_sha256: String,
    dataset_manifest_bytes: u64,
}

fn verify_authority(
    source: &Path,
    cameras: &Path,
    dataset_manifest_path: &Path,
    expected: ExpectedAuthority<'_>,
) -> Result<AuthorityProof> {
    if matches!(
        env::var("GSPLAT_ROT_LAYOUT").ok().as_deref(),
        Some("xyzw") | Some("XYZW")
    ) {
        return Err(invalid(
            "formal authoring rejects GSPLAT_ROT_LAYOUT=xyzw; frozen Bonsai uses the loader's official 3DGS wxyz convention",
        ));
    }

    let manifest_bytes = fs::read(dataset_manifest_path)?;
    let manifest: DatasetManifest = serde_json::from_slice(&manifest_bytes)?;
    if manifest.schema != "gsplat-dataset/v1"
        || manifest.id != expected.dataset_id
        || manifest.qualification_status != "local_candidate"
        || manifest.local_path != expected.source_logical_path
        || manifest.allowed_use != "local research/evaluation only"
        || manifest.redistribution
            != "prohibited unless model and upstream dataset rights are clarified"
        || manifest.sha256 != expected.source_sha256
        || manifest.bytes != expected.source_bytes
        || manifest.splat_count != expected.splat_count
        || manifest.sh_degree != expected.sh_degree
    {
        return Err(invalid(
            "dataset manifest does not match the frozen local-only source authority",
        ));
    }

    verify_file_identity(
        source,
        expected.source_bytes,
        expected.source_sha256,
        "source PLY",
    )?;
    let summary = load_ply_summary(source)?;
    if summary.gaussians as u64 != expected.splat_count
        || summary.sh_degree != expected.sh_degree
        || !summary.has_sh_rest
    {
        return Err(invalid(format!(
            "source PLY header is not the frozen complete SH{} identity: count={}, degree={}, has_rest={}",
            expected.sh_degree, summary.gaussians, summary.sh_degree, summary.has_sh_rest
        )));
    }

    verify_file_identity(
        cameras,
        expected.camera_bytes,
        expected.camera_sha256,
        "camera metadata",
    )?;
    let camera_document: Value = serde_json::from_slice(&fs::read(cameras)?)?;
    let records = camera_document
        .as_array()
        .ok_or_else(|| invalid("camera metadata must be a JSON array"))?;
    if records.len() != expected.camera_count {
        return Err(invalid(format!(
            "camera metadata entry count mismatch: expected {}, got {}",
            expected.camera_count,
            records.len()
        )));
    }
    let ids = records
        .iter()
        .map(|record| {
            record
                .as_object()
                .and_then(|object| object.get("id"))
                .and_then(Value::as_u64)
                .ok_or_else(|| invalid("camera metadata contains a missing or invalid id"))
        })
        .collect::<Result<BTreeSet<_>>>()?;
    if !expected
        .selected_camera_ids
        .iter()
        .all(|id| ids.contains(id))
    {
        return Err(invalid(
            "camera metadata does not contain both frozen authored camera ids",
        ));
    }

    Ok(AuthorityProof {
        dataset_manifest_sha256: hash_bytes(&manifest_bytes),
        dataset_manifest_bytes: manifest_bytes.len() as u64,
    })
}

fn verify_file_identity(
    path: &Path,
    expected_bytes: u64,
    expected_sha256: &str,
    name: &str,
) -> Result<()> {
    let actual_bytes = fs::metadata(path)?.len();
    if actual_bytes != expected_bytes {
        return Err(invalid(format!(
            "{name} byte count mismatch: expected {expected_bytes}, got {actual_bytes}"
        )));
    }
    let actual_sha256 = hash_file(path)?;
    if actual_sha256 != expected_sha256 {
        return Err(invalid(format!(
            "{name} SHA-256 mismatch: expected {expected_sha256}, got {actual_sha256}"
        )));
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut buffer = vec![0_u8; 1024 * 1024];
    let mut hasher = Sha256::new();
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex_digest(hasher.finalize().into()))
}

fn hash_bytes(bytes: &[u8]) -> String {
    hex_digest(Sha256::digest(bytes).into())
}

fn hex_digest(bytes: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in bytes {
        output.push(HEX[usize::from(byte >> 4)] as char);
        output.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    output
}

fn estimate(expected: ExpectedAuthority<'_>, source_leaves_per_node: u32) -> Result<Value> {
    let leaf_nodes = expected
        .splat_count
        .div_ceil(u64::from(source_leaves_per_node));
    let interior_nodes = leaf_nodes.saturating_sub(1);
    let total_nodes = leaf_nodes
        .checked_add(interior_nodes)
        .ok_or_else(|| invalid("node-count estimate overflow"))?;
    let source_drawable_bytes = expected
        .splat_count
        .checked_mul(DRAWABLE_ENCODED_BYTES)
        .ok_or_else(|| invalid("source-memory estimate overflow"))?;
    let page_bytes = source_drawable_bytes
        .checked_add(
            interior_nodes
                .checked_mul(DRAWABLE_ENCODED_BYTES)
                .ok_or_else(|| invalid("interior-page estimate overflow"))?,
        )
        .and_then(|value| value.checked_add(total_nodes.checked_mul(PAGE_HEADER_BYTES)?))
        .ok_or_else(|| invalid("page-size estimate overflow"))?;
    let receipt_bytes = total_nodes
        .checked_mul(RECEIPT_BYTES_PER_PAGE_ESTIMATE)
        .ok_or_else(|| invalid("receipt-size estimate overflow"))?;
    let estimated_output_bytes = page_bytes
        .checked_add(receipt_bytes)
        .and_then(|value| value.checked_add(4 * 1024 * 1024))
        .ok_or_else(|| invalid("output-size estimate overflow"))?;
    let conservative_peak_working_set_bytes = source_drawable_bytes
        .checked_mul(4)
        .and_then(|value| value.checked_add(ESTIMATE_FIXED_MARGIN_BYTES))
        .ok_or_else(|| invalid("working-set estimate overflow"))?;
    let recommended_free_disk_bytes = estimated_output_bytes
        .checked_mul(2)
        .and_then(|value| value.checked_add(256 * 1024 * 1024))
        .ok_or_else(|| invalid("disk estimate overflow"))?;
    Ok(json!({
        "schema": "gsplat-formal-s1-proxy-authoring-estimate/v1",
        "source_splat_count": expected.splat_count,
        "source_leaves_per_node": source_leaves_per_node,
        "leaf_node_count": leaf_nodes,
        "interior_node_count": interior_nodes,
        "total_node_and_page_count": total_nodes,
        "source_drawable_bytes": source_drawable_bytes,
        "estimated_page_bytes": page_bytes,
        "estimated_output_bytes": estimated_output_bytes,
        "conservative_peak_working_set_bytes": conservative_peak_working_set_bytes,
        "recommended_free_disk_bytes": recommended_free_disk_bytes,
        "sampling": "disabled",
        "source_sh_degree": expected.sh_degree,
    }))
}

fn load_drawables(source: &Path, expected: ExpectedAuthority<'_>) -> Result<Vec<DrawableGaussian>> {
    let mut drawables = Vec::new();
    drawables
        .try_reserve_exact(expected.splat_count as usize)
        .map_err(|_| invalid("failed to reserve complete source drawable storage"))?;
    let mut invalid_sh = None;
    let summary = visit_ply_splats(source, |splat| {
        if invalid_sh.is_none()
            && (splat.sh_degree != expected.sh_degree || splat.sh_rest_len != 45)
        {
            invalid_sh = Some((drawables.len(), splat.sh_degree, splat.sh_rest_len));
        }
        drawables.push(drawable_from_ply(splat));
    })?;
    if let Some((index, degree, rest_len)) = invalid_sh {
        return Err(invalid(format!(
            "decoded source splat {index} is not complete SH3: degree={degree}, rest_len={rest_len}"
        )));
    }
    if summary.gaussians as u64 != expected.splat_count
        || drawables.len() as u64 != expected.splat_count
    {
        return Err(invalid(format!(
            "decoded source count mismatch: expected {}, summary {}, visited {}",
            expected.splat_count,
            summary.gaussians,
            drawables.len()
        )));
    }
    Ok(drawables)
}

fn drawable_from_ply(splat: &DecodedPlySplat) -> DrawableGaussian {
    DrawableGaussian {
        position: [
            splat.position_ruf.x,
            splat.position_ruf.y,
            splat.position_ruf.z,
        ],
        scale: splat.log_scale_xyz.map(f32::exp),
        rotation_xyzw: splat.rotation_xyzw,
        opacity: 1.0 / (1.0 + (-splat.opacity_logit).exp()),
        sh_dc: splat.color_dc,
        sh_degree: u32::from(splat.sh_degree),
        sh_rest: splat.sh_rest,
    }
}

fn drawable_sequence_hash(drawables: &[DrawableGaussian]) -> String {
    let mut hasher = Sha256::new();
    for drawable in drawables {
        for value in drawable
            .position
            .into_iter()
            .chain(drawable.scale)
            .chain(drawable.rotation_xyzw)
            .chain([drawable.opacity])
            .chain(drawable.sh_dc)
        {
            hasher.update(value.to_bits().to_le_bytes());
        }
        hasher.update(drawable.sh_degree.to_le_bytes());
        for value in drawable.sh_rest {
            hasher.update(value.to_bits().to_le_bytes());
        }
    }
    hex_digest(hasher.finalize().into())
}

fn drawable_bitwise_equal(left: &DrawableGaussian, right: &DrawableGaussian) -> bool {
    left.position
        .into_iter()
        .chain(left.scale)
        .chain(left.rotation_xyzw)
        .chain([left.opacity])
        .chain(left.sh_dc)
        .chain(left.sh_rest)
        .map(f32::to_bits)
        .eq(right
            .position
            .into_iter()
            .chain(right.scale)
            .chain(right.rotation_xyzw)
            .chain([right.opacity])
            .chain(right.sh_dc)
            .chain(right.sh_rest)
            .map(f32::to_bits))
        && left.sh_degree == right.sh_degree
}

fn canonical_json_hash(value: &Value) -> Result<String> {
    Ok(hash_bytes(&serde_json::to_vec(value)?))
}

fn node_depths(bundle: &HierarchyBundle) -> Result<Vec<u32>> {
    let mut depths = vec![None; bundle.manifest.nodes.len()];
    let mut stack = bundle
        .manifest
        .roots
        .iter()
        .rev()
        .map(|node| (*node, 0_u32))
        .collect::<Vec<_>>();
    while let Some((id, depth)) = stack.pop() {
        let node = bundle
            .manifest
            .node(id)
            .ok_or_else(|| invalid(format!("unknown hierarchy node {}", id.0)))?;
        let slot = depths
            .get_mut(id.0 as usize)
            .ok_or_else(|| invalid(format!("unknown hierarchy node {}", id.0)))?;
        if slot.replace(depth).is_some() {
            return Err(invalid("hierarchy node has more than one depth"));
        }
        let child_depth = depth
            .checked_add(1)
            .ok_or_else(|| invalid("node depth overflow"))?;
        for child in node.children.iter().rev() {
            stack.push((*child, child_depth));
        }
    }
    depths
        .into_iter()
        .map(|depth| depth.ok_or_else(|| invalid("hierarchy contains an unreachable node")))
        .collect()
}

fn configuration(source_leaves_per_node: u32) -> Value {
    json!({
        "schema": CONFIGURATION_SCHEMA,
        "builder": "build_formal_s1_proxy_hierarchy",
        "hierarchy_schema_version": gsplat_hierarchy::SCHEMA_VERSION,
        "source_leaves_per_node": source_leaves_per_node,
        "source_coordinate_conversion": "gsplat_io_ply_rdf_to_ruf",
        "source_rotation_layout": "official_3dgs_wxyz_to_runtime_xyzw",
        "scale_conversion": "f32_exp_log_scale",
        "opacity_conversion": "f32_sigmoid_logit",
        "source_sh_degree": 3,
        "sh_representation": "source_sh3",
        "sampling": "disabled",
        "partial_child_publication": "disabled",
        "cut_policy": "complete_leaf_roots_then_two_smallest_range_replacements",
    })
}

struct CoverageContext<'a, 'b> {
    bundle: &'a HierarchyBundle,
    manifest_sha256: &'a str,
    expected: ExpectedAuthority<'b>,
    depths: &'a [u32],
}

impl CoverageContext<'_, '_> {
    fn receipt(
        &self,
        name: &str,
        selected: &[NodeId],
        replacement_count: u64,
        exact: bool,
    ) -> Result<Value> {
        self.bundle.manifest.validate_cut(selected)?;
        let ordered_node_ids = selected
            .iter()
            .map(|node| format!("node:{}", node.0))
            .collect::<Vec<_>>();
        let ordered_node_value = json!(ordered_node_ids);
        let page_by_node = self
            .bundle
            .manifest
            .pages
            .iter()
            .map(|page| (page.node, page))
            .collect::<BTreeMap<_, _>>();
        let mut represented_source_leaves = 0_u64;
        let mut active_proxy_splats = 0_u64;
        let mut selected_depths = BTreeSet::new();
        let mut page_hashes = Vec::with_capacity(selected.len());
        for id in selected {
            let node = self
                .bundle
                .manifest
                .node(*id)
                .ok_or_else(|| invalid(format!("unknown cut node {}", id.0)))?;
            represented_source_leaves = represented_source_leaves
                .checked_add(node.leaf_range.end - node.leaf_range.start)
                .ok_or_else(|| invalid("represented source count overflow"))?;
            active_proxy_splats = active_proxy_splats
                .checked_add(u64::from(node.payload_splat_count))
                .ok_or_else(|| invalid("active proxy count overflow"))?;
            selected_depths.insert(self.depths[id.0 as usize]);
            let page = page_by_node
                .get(id)
                .ok_or_else(|| invalid(format!("missing cut page for node {}", id.0)))?;
            page_hashes.push(page.content_hash.hex());
        }
        if represented_source_leaves != self.expected.splat_count {
            return Err(invalid(format!(
                "cut {name} represents {represented_source_leaves} source leaves, expected {}",
                self.expected.splat_count
            )));
        }
        let page_hash_value = json!(page_hashes);
        Ok(json!({
            "source_sha256": self.expected.source_sha256,
            "hierarchy_manifest_sha256": self.manifest_sha256,
            "source_splat_count": self.expected.splat_count,
        "represented_source_leaves": represented_source_leaves,
        "active_proxy_splats": active_proxy_splats,
        "missing_leaves": 0,
        "overlap_count": 0,
        "missing_page_count": 0,
        "antichain_valid": true,
        "parent_descendant_overlap": false,
            "source_sh_degree": self.expected.sh_degree,
        "sh_representation": "source_sh3",
        "sampling": "disabled",
        "partial_child_publication": "disabled",
        "ordered_node_ids": ordered_node_value,
        "ordered_node_list_sha256": canonical_json_hash(&ordered_node_value)?,
        "page_sha256": page_hash_value,
        "page_list_sha256": canonical_json_hash(&page_hash_value)?,
        "replacement_count": replacement_count,
        "depth_count": selected_depths.len(),
        "payload_bit_exact_to_source": exact,
        }))
    }
}

fn cut_receipts(
    cuts: &FormalS1Cuts,
    bundle: &HierarchyBundle,
    manifest_sha256: &str,
    expected: ExpectedAuthority<'_>,
) -> Result<Vec<Value>> {
    let depths = node_depths(bundle)?;
    let context = CoverageContext {
        bundle,
        manifest_sha256,
        expected,
        depths: &depths,
    };
    [
        (
            "complete_leaf_exact",
            cuts.complete_leaf_exact.as_slice(),
            0,
            true,
        ),
        ("bootstrap_roots", cuts.bootstrap_roots.as_slice(), 0, false),
        (
            "mixed_depth_two_replacements",
            cuts.mixed_depth_two_replacements.as_slice(),
            2,
            false,
        ),
    ]
    .into_iter()
    .map(|(name, selected, replacement_count, exact)| {
        let coverage = context.receipt(name, selected, replacement_count, exact)?;
        Ok(json!({
            "name": name,
            "coverage_sha256": canonical_json_hash(&coverage)?,
            "coverage": coverage,
        }))
    })
    .collect()
}

fn staging_path(output: &Path) -> Result<PathBuf> {
    let name = output
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid("output must have a UTF-8 final path component"))?;
    Ok(output.with_file_name(format!(".{name}.staging")))
}

fn write_bundle(
    source: &Path,
    output: &Path,
    expected: ExpectedAuthority<'_>,
    authority: &AuthorityProof,
    repository_commit: &str,
    source_leaves_per_node: u32,
) -> Result<Value> {
    if output.exists() {
        return Err(invalid(format!(
            "output must be fresh; already exists: {}",
            output.display()
        )));
    }
    let parent = output
        .parent()
        .ok_or_else(|| invalid("output must have an existing parent directory"))?;
    if !parent.is_dir() {
        return Err(invalid(format!(
            "output parent does not exist: {}",
            parent.display()
        )));
    }
    let staging = staging_path(output)?;
    if staging.exists() {
        return Err(invalid(format!(
            "staging path already exists and was preserved: {}",
            staging.display()
        )));
    }

    let drawables = load_drawables(source, expected)?;
    let source_drawable_sha256 = drawable_sequence_hash(&drawables);
    let config = BuildConfig {
        source_leaves_per_node,
    };
    let (bundle, cuts) = build_formal_s1_proxy_hierarchy(&drawables, config)?;
    bundle.validate(&drawables)?;

    let complete_leaf = bundle.materialize_cut(&drawables, &cuts.complete_leaf_exact)?;
    if complete_leaf.len() != drawables.len()
        || !complete_leaf
            .iter()
            .zip(&drawables)
            .all(|(left, right)| drawable_bitwise_equal(left, right))
    {
        return Err(invalid(
            "complete leaf materialization is not bit-exact to the converted source",
        ));
    }
    let complete_leaf_drawable_sha256 = drawable_sequence_hash(&complete_leaf);
    if source_drawable_sha256 != complete_leaf_drawable_sha256 {
        return Err(invalid(
            "complete leaf drawable sequence hash differs from converted source",
        ));
    }
    drop(complete_leaf);

    fs::create_dir(&staging)?;
    let page_directory = staging.join("pages");
    fs::create_dir(&page_directory)?;
    let manifest_bytes = bundle.manifest.canonical_bytes();
    let manifest_sha256 = hash_bytes(&manifest_bytes);
    fs::write(staging.join("manifest.bin"), &manifest_bytes)?;

    let manifest_page_by_node = bundle
        .manifest
        .pages
        .iter()
        .map(|page| (page.node, page))
        .collect::<BTreeMap<_, _>>();
    let mut page_receipts = Vec::with_capacity(bundle.pages.len());
    for page in &bundle.pages {
        let page_hash = hash_bytes(&page.bytes);
        if page_hash != page.content_hash.hex() {
            return Err(invalid(format!(
                "page {} content hash differs from its authored identity",
                page.node.0
            )));
        }
        let path = format!("pages/{page_hash}.gshp");
        fs::write(staging.join(&path), &page.bytes)?;
        let record = manifest_page_by_node
            .get(&page.node)
            .ok_or_else(|| invalid(format!("manifest omits page node {}", page.node.0)))?;
        page_receipts.push(json!({
            "node_id": format!("node:{}", page.node.0),
            "path": path,
            "sha256": page_hash,
            "encoded_bytes": page.bytes.len(),
            "decoded_splat_count": record.decoded_splat_count,
        }));
    }

    let configuration = configuration(source_leaves_per_node);
    let configuration_sha256 = canonical_json_hash(&configuration)?;
    let cuts = cut_receipts(&cuts, &bundle, &manifest_sha256, expected)?;
    let receipt = json!({
        "schema": RECEIPT_SCHEMA,
        "authoring_status": "complete",
        "scope": "offline_hierarchy_authoring_only",
        "s1_promotion_status": "Active",
        "endpoint_image_gate": "not_run",
        "s2_s5_unlocked": false,
        "authority": {
            "dataset_manifest": {
                "logical_path": "tests/perf/datasets/bonsai.local-candidate.json",
                "sha256": authority.dataset_manifest_sha256,
                "bytes": authority.dataset_manifest_bytes,
                "qualification_status": "local_candidate",
                "allowed_use": "local research/evaluation only",
                "redistribution": "prohibited unless model and upstream dataset rights are clarified",
            },
            "source": {
                "dataset_id": expected.dataset_id,
                "logical_path": expected.source_logical_path,
                "sha256": expected.source_sha256,
                "bytes": expected.source_bytes,
                "splat_count": expected.splat_count,
                "sh_degree": expected.sh_degree,
                "sampling": "disabled",
            },
            "camera_metadata": {
                "logical_path": expected.camera_logical_path,
                "sha256": expected.camera_sha256,
                "bytes": expected.camera_bytes,
                "entry_count": expected.camera_count,
                "selected_camera_ids": expected.selected_camera_ids,
                "review_status": "not_performed_by_authoring",
            },
            "builder": {
                "repository_commit": repository_commit,
                "configuration": configuration,
                "configuration_sha256": configuration_sha256,
            },
        },
        "hierarchy": {
            "manifest": {
                "path": "manifest.bin",
                "sha256": manifest_sha256,
                "bytes": manifest_bytes.len(),
                "schema_version": bundle.manifest.schema_version,
                "source_leaf_count": bundle.manifest.source_leaf_count,
                "root_count": bundle.manifest.roots.len(),
                "node_count": bundle.manifest.nodes.len(),
                "page_count": bundle.manifest.pages.len(),
            },
            "pages": page_receipts,
        },
        "complete_leaf_exact_identity": {
            "schema": SOURCE_DRAWABLE_SCHEMA,
            "converted_source_sha256": source_drawable_sha256,
            "materialized_complete_leaf_sha256": complete_leaf_drawable_sha256,
            "canonical_drawable_bytes": expected.splat_count * DRAWABLE_ENCODED_BYTES,
            "source_splat_count": expected.splat_count,
            "materialized_splat_count": expected.splat_count,
            "payload_bit_exact_to_source": true,
        },
        "cuts": cuts,
    });
    fs::write(
        staging.join("cut-receipt.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    fs::rename(&staging, output)?;

    // Re-open every retained file named by the receipt before reporting success.
    if hash_file(&output.join("manifest.bin"))? != manifest_sha256 {
        return Err(invalid("retained manifest hash mismatch after publication"));
    }
    for page in receipt["hierarchy"]["pages"]
        .as_array()
        .ok_or_else(|| invalid("internal page receipt shape error"))?
    {
        let path = page["path"]
            .as_str()
            .ok_or_else(|| invalid("internal page receipt path error"))?;
        let expected_hash = page["sha256"]
            .as_str()
            .ok_or_else(|| invalid("internal page receipt hash error"))?;
        if hash_file(&output.join(path))? != expected_hash {
            return Err(invalid(format!(
                "retained page hash mismatch after publication: {path}"
            )));
        }
    }
    Ok(receipt)
}

fn repository_commit() -> Result<String> {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let status = Command::new("git")
        .arg("-C")
        .arg(&repository)
        .args(["status", "--porcelain"])
        .output()?;
    if !status.status.success() {
        return Err(invalid("git status failed while binding builder authority"));
    }
    if !status.stdout.is_empty() {
        return Err(invalid(
            "formal authoring requires a clean repository so the builder commit is exact",
        ));
    }
    let revision = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(["rev-parse", "HEAD"])
        .output()?;
    if !revision.status.success() {
        return Err(invalid(
            "git rev-parse failed while binding builder authority",
        ));
    }
    let commit = String::from_utf8(revision.stdout)?.trim().to_owned();
    if commit.len() != 40
        || !commit
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid("builder repository commit is not lowercase 40-hex"));
    }
    Ok(commit)
}

fn run() -> Result<()> {
    let args = Args::parse()?;
    let authority = verify_authority(
        &args.source,
        &args.cameras,
        &args.dataset_manifest,
        BONSAI_AUTHORITY,
    )?;
    if args.estimate_only {
        println!(
            "{}",
            serde_json::to_string_pretty(&estimate(
                BONSAI_AUTHORITY,
                args.source_leaves_per_node
            )?)?
        );
        return Ok(());
    }
    let commit = repository_commit()?;
    let receipt = write_bundle(
        &args.source,
        &args.output,
        BONSAI_AUTHORITY,
        &authority,
        &commit,
        args.source_leaves_per_node,
    )?;
    println!(
        "authored={} builder_commit={} manifest_sha256={} pages={} receipt={}",
        args.output.display(),
        commit,
        receipt["hierarchy"]["manifest"]["sha256"]
            .as_str()
            .ok_or_else(|| invalid("internal manifest receipt shape error"))?,
        receipt["hierarchy"]["manifest"]["page_count"]
            .as_u64()
            .ok_or_else(|| invalid("internal page count receipt shape error"))?,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempDirectory(PathBuf);

    impl TempDirectory {
        fn new() -> Self {
            let path = env::temp_dir().join(format!(
                "gsplat-formal-s1-author-test-{}-{}",
                std::process::id(),
                TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).expect("create test directory");
            Self(path)
        }
    }

    impl Drop for TempDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn small_sh3_fixture_authors_deterministic_hashed_exact_bundle() {
        let root = TempDirectory::new();
        let source = root.0.join("fixture.ply");
        let cameras = root.0.join("cameras.json");
        let dataset_manifest = root.0.join("dataset.json");
        write_ascii_sh3_fixture(&source, 8);
        fs::write(&cameras, br#"[{"id":0},{"id":146}]"#).expect("write cameras");

        let source_bytes = fs::metadata(&source).expect("source metadata").len();
        let source_sha256 = hash_file(&source).expect("source hash");
        let camera_bytes = fs::metadata(&cameras).expect("camera metadata").len();
        let camera_sha256 = hash_file(&cameras).expect("camera hash");
        let expected = ExpectedAuthority {
            dataset_id: "fixture",
            source_logical_path: "fixture.ply",
            source_sha256: &source_sha256,
            source_bytes,
            splat_count: 8,
            sh_degree: 3,
            camera_logical_path: "cameras.json",
            camera_sha256: &camera_sha256,
            camera_bytes,
            camera_count: 2,
            selected_camera_ids: &[0, 146],
        };
        fs::write(
            &dataset_manifest,
            serde_json::to_vec_pretty(&json!({
                "schema": "gsplat-dataset/v1",
                "id": "fixture",
                "qualification_status": "local_candidate",
                "local_path": "fixture.ply",
                "allowed_use": "local research/evaluation only",
                "redistribution": "prohibited unless model and upstream dataset rights are clarified",
                "sha256": source_sha256,
                "bytes": source_bytes,
                "splat_count": 8,
                "sh_degree": 3,
            }))
            .expect("serialize dataset manifest"),
        )
        .expect("write dataset manifest");

        let authority = verify_authority(&source, &cameras, &dataset_manifest, expected)
            .expect("verify fixture authority");
        let output_one = root.0.join("output-one");
        let output_two = root.0.join("output-two");
        let receipt_one = write_bundle(
            &source,
            &output_one,
            expected,
            &authority,
            "1111111111111111111111111111111111111111",
            1,
        )
        .expect("author first bundle");
        let receipt_two = write_bundle(
            &source,
            &output_two,
            expected,
            &authority,
            "1111111111111111111111111111111111111111",
            1,
        )
        .expect("author second bundle");

        assert_eq!(receipt_one, receipt_two);
        assert_eq!(read_tree(&output_one), read_tree(&output_two));
        let exact = &receipt_one["complete_leaf_exact_identity"];
        assert_eq!(
            exact["converted_source_sha256"],
            exact["materialized_complete_leaf_sha256"]
        );
        assert_eq!(exact["payload_bit_exact_to_source"], true);
        assert_eq!(receipt_one["hierarchy"]["manifest"]["page_count"], 15);
        assert_eq!(receipt_one["cuts"].as_array().expect("cuts").len(), 3);
        for page in receipt_one["hierarchy"]["pages"].as_array().expect("pages") {
            let path = page["path"].as_str().expect("page path");
            assert_eq!(
                hash_file(&output_one.join(path)).expect("retained page hash"),
                page["sha256"].as_str().expect("page hash")
            );
        }
        let complete = &receipt_one["cuts"][0]["coverage"];
        assert_eq!(complete["active_proxy_splats"], 8);
        assert_eq!(complete["represented_source_leaves"], 8);
        assert_eq!(complete["source_sh_degree"], 3);
        assert_eq!(complete["sampling"], "disabled");
        let mixed = &receipt_one["cuts"][2]["coverage"];
        assert_eq!(mixed["replacement_count"], 2);
        assert!(mixed["depth_count"].as_u64().expect("depth count") >= 2);
    }

    fn write_ascii_sh3_fixture(path: &Path, count: usize) {
        let mut text = String::from("ply\nformat ascii 1.0\n");
        text.push_str(&format!("element vertex {count}\n"));
        for property in [
            "x", "y", "z", "f_dc_0", "f_dc_1", "f_dc_2", "opacity", "scale_0", "scale_1",
            "scale_2", "rot_0", "rot_1", "rot_2", "rot_3",
        ] {
            text.push_str(&format!("property float {property}\n"));
        }
        for index in 0..45 {
            text.push_str(&format!("property float f_rest_{index}\n"));
        }
        text.push_str("end_header\n");
        for splat in 0..count {
            let x = splat as f32;
            text.push_str(&format!(
                "{x} {} {} {} {} {} {} {} {} {} 1 0 0 0",
                x * 0.5,
                1.0 + x * 0.25,
                0.1 + x * 0.01,
                0.2 + x * 0.01,
                0.3 + x * 0.01,
                -0.5 + x * 0.05,
                -2.0 + x * 0.01,
                -2.1 + x * 0.01,
                -2.2 + x * 0.01,
            ));
            for coefficient in 0..45 {
                text.push_str(&format!(
                    " {}",
                    (splat * 45 + coefficient + 1) as f32 * 0.001
                ));
            }
            text.push('\n');
        }
        fs::write(path, text).expect("write SH3 fixture");
    }

    fn read_tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
        fn visit(base: &Path, current: &Path, output: &mut BTreeMap<String, Vec<u8>>) {
            let mut entries = fs::read_dir(current)
                .expect("read output directory")
                .collect::<io::Result<Vec<_>>>()
                .expect("collect output directory");
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                let path = entry.path();
                if path.is_dir() {
                    visit(base, &path, output);
                } else {
                    output.insert(
                        path.strip_prefix(base)
                            .expect("relative output path")
                            .to_string_lossy()
                            .into_owned(),
                        fs::read(path).expect("read output file"),
                    );
                }
            }
        }
        let mut output = BTreeMap::new();
        visit(root, root, &mut output);
        output
    }
}
