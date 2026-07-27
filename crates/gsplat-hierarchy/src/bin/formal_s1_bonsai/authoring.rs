//! Private implementation for the repository-local formal S1 Bonsai author.
//!
//! This binary emits authored hierarchy bytes and structural coverage receipts.
//! It deliberately does not render images, qualify endpoints, or unlock S2.

use crate::cut_ply::{self, CutPlyIdentity};
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
const CUT_PLY_ENCODED_BYTES_PER_SPLAT: u64 = 236;
const RECEIPT_BYTES_PER_PAGE_ESTIMATE: u64 = 512;
const ESTIMATE_FIXED_MARGIN_BYTES: u64 = 512 * 1024 * 1024;

const BONSAI_MANIFEST_LOGICAL_PATH: &str = "tests/perf/datasets/bonsai.local-candidate.json";
const BONSAI_MANIFEST_SHA256: &str =
    "5d6bd3ca7f6a334a15941e231677a50796458b98ff82d7bb5e2e096a69cbd56a";
const BONSAI_MANIFEST_BYTES: u64 = 1_694;
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

pub(super) type DynError = Box<dyn Error + Send + Sync + 'static>;
pub(super) type Result<T> = std::result::Result<T, DynError>;

#[derive(Debug)]
struct AuthorError(String);

impl fmt::Display for AuthorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for AuthorError {}

pub(super) fn invalid(message: impl Into<String>) -> DynError {
    Box::new(AuthorError(message.into()))
}

#[derive(Clone, Copy)]
pub(super) struct ExpectedAuthority<'a> {
    dataset_manifest_logical_path: &'a str,
    dataset_manifest_sha256: &'a str,
    dataset_manifest_bytes: u64,
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
    dataset_manifest_logical_path: BONSAI_MANIFEST_LOGICAL_PATH,
    dataset_manifest_sha256: BONSAI_MANIFEST_SHA256,
    dataset_manifest_bytes: BONSAI_MANIFEST_BYTES,
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

pub(super) fn bonsai_authority() -> ExpectedAuthority<'static> {
    BONSAI_AUTHORITY
}

#[derive(Clone, Copy)]
pub(super) struct AuthorityPaths<'a> {
    pub(super) source: &'a Path,
    pub(super) cameras: &'a Path,
    pub(super) dataset_manifest: &'a Path,
    pub(super) canonical_dataset_manifest: &'a Path,
}

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct AuthorityProof {
    dataset_manifest_sha256: String,
    dataset_manifest_bytes: u64,
}

pub(super) fn verify_authority(
    paths: AuthorityPaths<'_>,
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

    require_exact_manifest_path(paths.dataset_manifest, paths.canonical_dataset_manifest)?;
    let manifest_bytes = read_verified_bytes(
        paths.dataset_manifest,
        expected.dataset_manifest_bytes,
        expected.dataset_manifest_sha256,
        "canonical dataset manifest",
    )?;
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
        paths.source,
        expected.source_bytes,
        expected.source_sha256,
        "source PLY",
    )?;
    let summary = load_ply_summary(paths.source)?;
    if summary.gaussians as u64 != expected.splat_count
        || summary.sh_degree != expected.sh_degree
        || !summary.has_sh_rest
    {
        return Err(invalid(format!(
            "source PLY header is not the frozen complete SH{} identity: count={}, degree={}, has_rest={}",
            expected.sh_degree, summary.gaussians, summary.sh_degree, summary.has_sh_rest
        )));
    }

    let camera_bytes = read_verified_bytes(
        paths.cameras,
        expected.camera_bytes,
        expected.camera_sha256,
        "camera metadata",
    )?;
    let camera_document: Value = serde_json::from_slice(&camera_bytes)?;
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
        dataset_manifest_sha256: expected.dataset_manifest_sha256.to_owned(),
        dataset_manifest_bytes: manifest_bytes.len() as u64,
    })
}

fn require_exact_manifest_path(actual: &Path, canonical: &Path) -> Result<()> {
    let current = env::current_dir()?;
    let actual_absolute = if actual.is_absolute() {
        actual.to_owned()
    } else {
        current.join(actual)
    };
    let canonical_absolute = if canonical.is_absolute() {
        canonical.to_owned()
    } else {
        current.join(canonical)
    };
    if actual_absolute != canonical_absolute {
        return Err(invalid(format!(
            "dataset manifest must use the canonical repository path {}; got {}",
            canonical_absolute.display(),
            actual_absolute.display()
        )));
    }
    Ok(())
}

fn read_verified_bytes(
    path: &Path,
    expected_bytes: u64,
    expected_sha256: &str,
    name: &str,
) -> Result<Vec<u8>> {
    let bytes = fs::read(path)?;
    verify_identity(
        bytes.len() as u64,
        &hash_bytes(&bytes),
        expected_bytes,
        expected_sha256,
        name,
    )?;
    Ok(bytes)
}

fn verify_file_identity(
    path: &Path,
    expected_bytes: u64,
    expected_sha256: &str,
    name: &str,
) -> Result<()> {
    let (actual_bytes, actual_sha256) = hash_file_identity(path)?;
    verify_identity(
        actual_bytes,
        &actual_sha256,
        expected_bytes,
        expected_sha256,
        name,
    )
}

fn verify_identity(
    actual_bytes: u64,
    actual_sha256: &str,
    expected_bytes: u64,
    expected_sha256: &str,
    name: &str,
) -> Result<()> {
    if actual_bytes != expected_bytes || actual_sha256 != expected_sha256 {
        return Err(invalid(format!(
            "{name} identity mismatch: expected bytes={expected_bytes} sha256={expected_sha256}, got bytes={actual_bytes} sha256={actual_sha256}"
        )));
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<String> {
    Ok(hash_file_identity(path)?.1)
}

fn hash_file_identity(path: &Path) -> Result<(u64, String)> {
    let mut file = fs::File::open(path)?;
    let mut buffer = vec![0_u8; 1024 * 1024];
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read as u64)
            .ok_or_else(|| invalid("file byte count overflow while hashing"))?;
        hasher.update(&buffer[..read]);
    }
    Ok((total, hex_digest(hasher.finalize().into())))
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

#[derive(Clone, Debug, PartialEq)]
pub(super) struct ResourceEstimate {
    pub(super) document: Value,
    pub(super) estimated_output_bytes: u64,
    pub(super) recommended_free_disk_bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct DiskAdmission {
    pub(super) available_bytes: u64,
    pub(super) required_bytes: u64,
}

pub(super) fn estimate(
    expected: ExpectedAuthority<'_>,
    source_leaves_per_node: u32,
) -> Result<ResourceEstimate> {
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
    // The two proxy cuts cannot contain more drawables than the complete
    // source cut. Reserve that conservative upper bound even though formal
    // roots/mixed cuts are expected to be much smaller.
    let cut_ply_bytes = expected
        .splat_count
        .checked_mul(CUT_PLY_ENCODED_BYTES_PER_SPLAT)
        .and_then(|value| value.checked_mul(2))
        .and_then(|value| value.checked_add(2 * 1024 * 1024))
        .ok_or_else(|| invalid("cut PLY size estimate overflow"))?;
    let estimated_output_bytes = page_bytes
        .checked_add(receipt_bytes)
        .and_then(|value| value.checked_add(cut_ply_bytes))
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
    let document = json!({
        "schema": "gsplat-formal-s1-proxy-authoring-estimate/v1",
        "source_splat_count": expected.splat_count,
        "source_leaves_per_node": source_leaves_per_node,
        "leaf_node_count": leaf_nodes,
        "interior_node_count": interior_nodes,
        "total_node_and_page_count": total_nodes,
        "source_drawable_bytes": source_drawable_bytes,
        "estimated_page_bytes": page_bytes,
        "estimated_proxy_cut_ply_bytes": cut_ply_bytes,
        "estimated_output_bytes": estimated_output_bytes,
        "conservative_peak_working_set_bytes": conservative_peak_working_set_bytes,
        "recommended_free_disk_bytes": recommended_free_disk_bytes,
        "sampling": "disabled",
        "source_sh_degree": expected.sh_degree,
    });
    Ok(ResourceEstimate {
        document,
        estimated_output_bytes,
        recommended_free_disk_bytes,
    })
}

pub(super) fn admit_output(output: &Path, estimate: &ResourceEstimate) -> Result<DiskAdmission> {
    if output.exists() {
        return Err(invalid(format!(
            "output must be fresh; already exists: {}",
            output.display()
        )));
    }
    let staging = staging_path(output)?;
    if staging.exists() {
        return Err(invalid(format!(
            "staging path already exists and was preserved: {}",
            staging.display()
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
    let available_bytes = disk_available_bytes(parent)?;
    if available_bytes < estimate.recommended_free_disk_bytes {
        return Err(invalid(format!(
            "disk admission rejected: available_bytes={available_bytes}, required_bytes={}",
            estimate.recommended_free_disk_bytes
        )));
    }
    Ok(DiskAdmission {
        available_bytes,
        required_bytes: estimate.recommended_free_disk_bytes,
    })
}

fn disk_available_bytes(path: &Path) -> Result<u64> {
    let output = Command::new("df").args(["-P", "-k"]).arg(path).output()?;
    if !output.status.success() {
        return Err(invalid("df -Pk failed during normal authoring admission"));
    }
    parse_df_available_bytes(&String::from_utf8(output.stdout)?)
}

fn parse_df_available_bytes(output: &str) -> Result<u64> {
    let line = output
        .lines()
        .rfind(|line| !line.trim().is_empty())
        .ok_or_else(|| invalid("df -Pk returned no filesystem row"))?;
    let columns = line.split_whitespace().collect::<Vec<_>>();
    let available_kib = columns
        .get(3)
        .ok_or_else(|| invalid("df -Pk filesystem row is missing available blocks"))?
        .parse::<u64>()
        .map_err(|_| invalid("df -Pk available blocks are not an integer"))?;
    available_kib
        .checked_mul(1024)
        .ok_or_else(|| invalid("df -Pk available-byte count overflow"))
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
        "proxy_cut_render_input_schema": cut_ply::CUT_PLY_SCHEMA,
        "proxy_cut_ply_format": "binary_little_endian_1.0_complete_sh3",
        "proxy_cut_readback": "gsplat_io_ply_exact_linear_and_max_4_ulp_nonlinear",
        "complete_leaf_render_input": "content_addressed_source_ply_alias_without_copy",
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
    bootstrap_ply: &CutPlyIdentity,
    mixed_ply: &CutPlyIdentity,
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
        let render_input = match name {
            "complete_leaf_exact" => source_alias_render_input(name, &coverage, expected)?,
            "bootstrap_roots" => proxy_render_input(name, &coverage, bootstrap_ply)?,
            "mixed_depth_two_replacements" => proxy_render_input(name, &coverage, mixed_ply)?,
            _ => return Err(invalid(format!("unknown formal cut {name}"))),
        };
        Ok(json!({
            "name": name,
            "coverage_sha256": canonical_json_hash(&coverage)?,
            "coverage": coverage,
            "render_input": render_input,
        }))
    })
    .collect()
}

fn common_render_input_binding(name: &str, coverage: &Value) -> Result<Value> {
    Ok(json!({
        "schema": cut_ply::CUT_PLY_SCHEMA,
        "cut_name": name,
        "source_sha256": required_string(coverage, "source_sha256")?,
        "hierarchy_manifest_sha256": required_string(coverage, "hierarchy_manifest_sha256")?,
        "ordered_node_ids": coverage["ordered_node_ids"].clone(),
        "ordered_node_list_sha256": required_string(coverage, "ordered_node_list_sha256")?,
        "page_sha256": coverage["page_sha256"].clone(),
        "page_list_sha256": required_string(coverage, "page_list_sha256")?,
        "P": required_u64(coverage, "active_proxy_splats")?,
        "sh_degree": required_u64(coverage, "source_sh_degree")?,
        "sampling": "disabled",
    }))
}

fn source_alias_render_input(
    name: &str,
    coverage: &Value,
    expected: ExpectedAuthority<'_>,
) -> Result<Value> {
    let mut binding = common_render_input_binding(name, coverage)?;
    let object = binding
        .as_object_mut()
        .ok_or_else(|| invalid("internal render-input binding shape error"))?;
    object.insert(
        "kind".to_owned(),
        Value::String("content_addressed_source_ply_alias".to_owned()),
    );
    object.insert(
        "logical_path".to_owned(),
        Value::String(expected.source_logical_path.to_owned()),
    );
    object.insert(
        "sha256".to_owned(),
        Value::String(expected.source_sha256.to_owned()),
    );
    object.insert("bytes".to_owned(), Value::from(expected.source_bytes));
    object.insert("copied_into_package".to_owned(), Value::Bool(false));
    Ok(binding)
}

fn proxy_render_input(name: &str, coverage: &Value, ply: &CutPlyIdentity) -> Result<Value> {
    let active = required_u64(coverage, "active_proxy_splats")?;
    if ply.splat_count != active {
        return Err(invalid(format!(
            "cut {name} PLY P={} differs from coverage P={active}",
            ply.splat_count
        )));
    }
    let mut binding = common_render_input_binding(name, coverage)?;
    let object = binding
        .as_object_mut()
        .ok_or_else(|| invalid("internal render-input binding shape error"))?;
    object.insert(
        "kind".to_owned(),
        Value::String("materialized_binary_little_endian_sh3_ply".to_owned()),
    );
    object.insert("path".to_owned(), Value::String(ply.path.clone()));
    object.insert("sha256".to_owned(), Value::String(ply.sha256.clone()));
    object.insert("bytes".to_owned(), Value::from(ply.bytes));
    object.insert(
        "coordinate_conversion".to_owned(),
        Value::String("runtime_ruf_to_ply_rdf".to_owned()),
    );
    object.insert(
        "rotation_conversion".to_owned(),
        Value::String("runtime_xyzw_to_ply_wxyz".to_owned()),
    );
    object.insert(
        "scale_encoding".to_owned(),
        Value::String("finite_f32_ln".to_owned()),
    );
    object.insert(
        "opacity_encoding".to_owned(),
        Value::String("finite_f32_logit".to_owned()),
    );
    object.insert(
        "readback_validator".to_owned(),
        Value::String("gsplat_io_ply_same_runtime_proxy_attributes".to_owned()),
    );
    object.insert(
        "nonlinear_roundtrip_max_ulps".to_owned(),
        Value::from(cut_ply::MAX_NONLINEAR_ROUNDTRIP_ULPS),
    );
    Ok(binding)
}

fn required_string<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value[field]
        .as_str()
        .ok_or_else(|| invalid(format!("internal receipt field {field} is not a string")))
}

fn required_u64(value: &Value, field: &str) -> Result<u64> {
    value[field]
        .as_u64()
        .ok_or_else(|| invalid(format!("internal receipt field {field} is not a u64")))
}

fn staging_path(output: &Path) -> Result<PathBuf> {
    let name = output
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid("output must have a UTF-8 final path component"))?;
    Ok(output.with_file_name(format!(".{name}.staging")))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct BuilderIdentity {
    pub(super) repository_commit: String,
    pub(super) executable_sha256: String,
    pub(super) rustc_verbose_version: String,
    pub(super) cargo_version: String,
}

#[derive(Clone, Copy)]
pub(super) struct AuthorRequest<'a> {
    pub(super) paths: AuthorityPaths<'a>,
    pub(super) output: &'a Path,
    pub(super) expected: ExpectedAuthority<'a>,
    pub(super) authority: &'a AuthorityProof,
    pub(super) builder: &'a BuilderIdentity,
    pub(super) source_leaves_per_node: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct AuthoringOutcome {
    pub(super) receipt: Value,
    pub(super) receipt_sha256: String,
    pub(super) manifest_sha256: String,
    pub(super) page_count: usize,
}

pub(super) fn author_bundle(request: AuthorRequest<'_>) -> Result<AuthoringOutcome> {
    write_bundle_with_hooks(request, |_| Ok(()), verify_staged_bundle)
}

struct StagedCut<'a> {
    name: &'static str,
    drawables: &'a [DrawableGaussian],
}

fn write_bundle_with_hooks<BeforePublication, VerifyStaging>(
    request: AuthorRequest<'_>,
    mut before_publication: BeforePublication,
    verify_staging: VerifyStaging,
) -> Result<AuthoringOutcome>
where
    BeforePublication: FnMut(&Path) -> Result<()>,
    VerifyStaging: Fn(&Path, &Value, &[StagedCut<'_>]) -> Result<String>,
{
    let AuthorRequest {
        paths,
        output,
        expected,
        authority,
        builder,
        source_leaves_per_node,
    } = request;
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

    let drawables = load_drawables(paths.source, expected)?;
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

    let cut_directory = staging.join("cuts");
    fs::create_dir(&cut_directory)?;
    let bootstrap_drawables = bundle.materialize_cut(&drawables, &cuts.bootstrap_roots)?;
    let bootstrap_ply =
        cut_ply::author_file(&staging, "cuts/bootstrap_roots.ply", &bootstrap_drawables)?;
    let mixed_drawables = bundle.materialize_cut(&drawables, &cuts.mixed_depth_two_replacements)?;
    let mixed_ply = cut_ply::author_file(
        &staging,
        "cuts/mixed_depth_two_replacements.ply",
        &mixed_drawables,
    )?;

    let configuration = configuration(source_leaves_per_node);
    let configuration_sha256 = canonical_json_hash(&configuration)?;
    let cuts = cut_receipts(
        &cuts,
        &bundle,
        &manifest_sha256,
        expected,
        &bootstrap_ply,
        &mixed_ply,
    )?;
    let receipt = json!({
        "schema": RECEIPT_SCHEMA,
        "authoring_status": "complete",
        "scope": "offline_hierarchy_authoring_with_s1_cut_render_inputs",
        "s1_promotion_status": "Active",
        "endpoint_image_gate": "not_run",
        "s2_s5_unlocked": false,
        "authority": {
            "dataset_manifest": {
                "logical_path": expected.dataset_manifest_logical_path,
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
                "repository_commit": builder.repository_commit,
                "executable_sha256": builder.executable_sha256,
                "rustc_verbose_version": builder.rustc_verbose_version,
                "cargo_version": builder.cargo_version,
                "identity_semantics": {
                    "repository_commit": "binds the clean checkout inspected at invocation; alone it does not prove executable provenance",
                    "executable_sha256": "binds the exact current executable bytes; alone it does not prove source-commit provenance",
                    "toolchain": "records the invoked rustc and cargo version text; it is not a reproducible-build claim",
                    "combined": "review must independently reproduce the executable from commit, toolchain and configuration before treating them as equivalent",
                },
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
    before_publication(&staging)?;
    let staged_cuts = [
        StagedCut {
            name: "bootstrap_roots",
            drawables: &bootstrap_drawables,
        },
        StagedCut {
            name: "mixed_depth_two_replacements",
            drawables: &mixed_drawables,
        },
    ];
    let receipt_sha256 = verify_staging(&staging, &receipt, &staged_cuts)?;
    let final_authority = verify_authority(paths, expected)?;
    if final_authority != *authority {
        return Err(invalid(
            "authority identity changed between admission and publication",
        ));
    }

    let outcome = AuthoringOutcome {
        receipt,
        receipt_sha256,
        manifest_sha256,
        page_count: bundle.manifest.pages.len(),
    };

    // Atomic rename is deliberately the final filesystem/publication action.
    // Every fallible re-read and authority check has completed in staging.
    fs::rename(&staging, output)?;
    Ok(outcome)
}

fn verify_staged_bundle(
    staging: &Path,
    expected_receipt: &Value,
    expected_cuts: &[StagedCut<'_>],
) -> Result<String> {
    let expected_receipt_bytes = serde_json::to_vec_pretty(expected_receipt)?;
    let receipt_path = staging.join("cut-receipt.json");
    let retained_receipt_bytes = fs::read(&receipt_path)?;
    let expected_receipt_sha256 = hash_bytes(&expected_receipt_bytes);
    let retained_receipt_sha256 = hash_bytes(&retained_receipt_bytes);
    if retained_receipt_bytes != expected_receipt_bytes
        || retained_receipt_sha256 != expected_receipt_sha256
    {
        return Err(invalid("staged cut receipt bytes or SHA-256 mismatch"));
    }
    let retained_receipt: Value = serde_json::from_slice(&retained_receipt_bytes)?;
    if retained_receipt != *expected_receipt {
        return Err(invalid(
            "staged cut receipt JSON does not round-trip exactly",
        ));
    }

    let manifest = expected_receipt["hierarchy"]["manifest"]
        .as_object()
        .ok_or_else(|| invalid("internal manifest receipt shape error"))?;
    let manifest_path = manifest
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("internal manifest receipt path error"))?;
    let manifest_sha256 = manifest
        .get("sha256")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("internal manifest receipt hash error"))?;
    let manifest_bytes = manifest
        .get("bytes")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid("internal manifest receipt byte-count error"))?;
    verify_file_identity(
        &staging.join(manifest_path),
        manifest_bytes,
        manifest_sha256,
        "staged hierarchy manifest",
    )?;

    let pages = expected_receipt["hierarchy"]["pages"]
        .as_array()
        .ok_or_else(|| invalid("internal page receipt shape error"))?;
    let mut expected_page_paths = BTreeSet::new();
    for page in pages {
        let path = page["path"]
            .as_str()
            .ok_or_else(|| invalid("internal page receipt path error"))?;
        let expected_hash = page["sha256"]
            .as_str()
            .ok_or_else(|| invalid("internal page receipt hash error"))?;
        let expected_bytes = page["encoded_bytes"]
            .as_u64()
            .ok_or_else(|| invalid("internal page receipt byte-count error"))?;
        if !expected_page_paths.insert(path.to_owned()) {
            return Err(invalid(format!("duplicate staged page path {path}")));
        }
        verify_file_identity(
            &staging.join(path),
            expected_bytes,
            expected_hash,
            "staged hierarchy page",
        )?;
    }
    let actual_page_paths = fs::read_dir(staging.join("pages"))?
        .map(|entry| {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                return Err(invalid("staged pages directory contains a non-file entry"));
            }
            Ok(format!("pages/{}", entry.file_name().to_string_lossy()))
        })
        .collect::<Result<BTreeSet<_>>>()?;
    if actual_page_paths != expected_page_paths {
        return Err(invalid(
            "staged page file set differs from the complete receipt",
        ));
    }

    verify_staged_cut_inputs(staging, expected_receipt, expected_cuts)?;

    let root_entries = fs::read_dir(staging)?
        .map(|entry| Ok(entry?.file_name().to_string_lossy().into_owned()))
        .collect::<Result<BTreeSet<_>>>()?;
    if root_entries
        != BTreeSet::from([
            "cut-receipt.json".to_owned(),
            "cuts".to_owned(),
            "manifest.bin".to_owned(),
            "pages".to_owned(),
        ])
    {
        return Err(invalid(
            "staged package root contains an unreceipted or missing entry",
        ));
    }
    Ok(retained_receipt_sha256)
}

fn verify_staged_cut_inputs(
    staging: &Path,
    receipt: &Value,
    expected_cuts: &[StagedCut<'_>],
) -> Result<()> {
    let cuts = receipt["cuts"]
        .as_array()
        .ok_or_else(|| invalid("internal cut receipt shape error"))?;
    let complete = cuts
        .iter()
        .find(|cut| cut["name"] == "complete_leaf_exact")
        .ok_or_else(|| invalid("complete-leaf receipt is missing"))?;
    let alias = &complete["render_input"];
    let source = &receipt["authority"]["source"];
    verify_render_input_coverage_binding(complete)?;
    if alias["kind"] != "content_addressed_source_ply_alias"
        || alias["logical_path"] != source["logical_path"]
        || alias["sha256"] != source["sha256"]
        || alias["bytes"] != source["bytes"]
        || alias["P"] != source["splat_count"]
        || alias["copied_into_package"] != false
    {
        return Err(invalid(
            "complete-leaf render input is not the exact content-addressed source PLY alias",
        ));
    }

    let mut expected_paths = BTreeSet::new();
    for expected in expected_cuts {
        let cut = cuts
            .iter()
            .find(|cut| cut["name"] == expected.name)
            .ok_or_else(|| invalid(format!("cut receipt {} is missing", expected.name)))?;
        let coverage = &cut["coverage"];
        let input = &cut["render_input"];
        verify_render_input_coverage_binding(cut)?;
        if input["schema"] != cut_ply::CUT_PLY_SCHEMA
            || input["cut_name"] != expected.name
            || input["kind"] != "materialized_binary_little_endian_sh3_ply"
            || input["P"] != coverage["active_proxy_splats"]
            || input["sh_degree"] != 3
        {
            return Err(invalid(format!(
                "cut {} render input metadata mismatch",
                expected.name
            )));
        }
        let identity = CutPlyIdentity {
            path: required_string(input, "path")?.to_owned(),
            sha256: required_string(input, "sha256")?.to_owned(),
            bytes: required_u64(input, "bytes")?,
            splat_count: required_u64(input, "P")?,
        };
        let canonical_path = format!("cuts/{}.ply", expected.name);
        if identity.path != canonical_path || !expected_paths.insert(identity.path.clone()) {
            return Err(invalid(format!(
                "cut {} PLY path is non-canonical or duplicated",
                expected.name
            )));
        }
        cut_ply::verify_file(staging, &identity, expected.drawables)?;
    }
    let actual_paths = fs::read_dir(staging.join("cuts"))?
        .map(|entry| {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                return Err(invalid("staged cuts directory contains a non-file entry"));
            }
            Ok(format!("cuts/{}", entry.file_name().to_string_lossy()))
        })
        .collect::<Result<BTreeSet<_>>>()?;
    if actual_paths != expected_paths {
        return Err(invalid(
            "staged cut PLY file set differs from the complete receipt",
        ));
    }
    Ok(())
}

fn verify_render_input_coverage_binding(cut: &Value) -> Result<()> {
    let name = required_string(cut, "name")?;
    let coverage = &cut["coverage"];
    let input = &cut["render_input"];
    for field in [
        "source_sha256",
        "hierarchy_manifest_sha256",
        "ordered_node_ids",
        "ordered_node_list_sha256",
        "page_sha256",
        "page_list_sha256",
    ] {
        if input[field] != coverage[field] {
            return Err(invalid(format!(
                "cut {name} render input does not bind coverage field {field}"
            )));
        }
    }
    if input["cut_name"] != name || input["P"] != coverage["active_proxy_splats"] {
        return Err(invalid(format!(
            "cut {name} render input does not bind its name and P"
        )));
    }
    Ok(())
}

pub(super) fn builder_identity() -> Result<BuilderIdentity> {
    let repository_commit = repository_commit()?;
    let executable = env::current_exe()?;
    Ok(BuilderIdentity {
        repository_commit,
        executable_sha256: hash_file(&executable)?,
        rustc_verbose_version: command_version("rustc", &["-Vv"])?,
        cargo_version: command_version("cargo", &["-V"])?,
    })
}

fn command_version(program: &str, arguments: &[&str]) -> Result<String> {
    let output = Command::new(program).args(arguments).output()?;
    if !output.status.success() {
        return Err(invalid(format!(
            "{program} version probe failed while binding builder identity"
        )));
    }
    let text = String::from_utf8(output.stdout)?.trim().to_owned();
    if text.is_empty() {
        return Err(invalid(format!(
            "{program} version probe returned an empty identity"
        )));
    }
    Ok(text)
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

    struct Fixture {
        root: TempDirectory,
        source: PathBuf,
        cameras: PathBuf,
        dataset_manifest: PathBuf,
        source_sha256: String,
        source_bytes: u64,
        camera_sha256: String,
        camera_bytes: u64,
        manifest_sha256: String,
        manifest_bytes: u64,
    }

    impl Fixture {
        fn new() -> Self {
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
            let manifest_bytes = fs::metadata(&dataset_manifest)
                .expect("manifest metadata")
                .len();
            let manifest_sha256 = hash_file(&dataset_manifest).expect("manifest hash");
            Self {
                root,
                source,
                cameras,
                dataset_manifest,
                source_sha256,
                source_bytes,
                camera_sha256,
                camera_bytes,
                manifest_sha256,
                manifest_bytes,
            }
        }

        fn expected(&self) -> ExpectedAuthority<'_> {
            ExpectedAuthority {
                dataset_manifest_logical_path: "dataset.json",
                dataset_manifest_sha256: &self.manifest_sha256,
                dataset_manifest_bytes: self.manifest_bytes,
                dataset_id: "fixture",
                source_logical_path: "fixture.ply",
                source_sha256: &self.source_sha256,
                source_bytes: self.source_bytes,
                splat_count: 8,
                sh_degree: 3,
                camera_logical_path: "cameras.json",
                camera_sha256: &self.camera_sha256,
                camera_bytes: self.camera_bytes,
                camera_count: 2,
                selected_camera_ids: &[0, 146],
            }
        }

        fn paths(&self) -> AuthorityPaths<'_> {
            AuthorityPaths {
                source: &self.source,
                cameras: &self.cameras,
                dataset_manifest: &self.dataset_manifest,
                canonical_dataset_manifest: &self.dataset_manifest,
            }
        }
    }

    fn fixture_builder() -> BuilderIdentity {
        BuilderIdentity {
            repository_commit: "1111111111111111111111111111111111111111".to_owned(),
            executable_sha256: "22".repeat(32),
            rustc_verbose_version: "rustc fixture (fixture)".to_owned(),
            cargo_version: "cargo fixture (fixture)".to_owned(),
        }
    }

    fn fixture_request<'a>(
        fixture: &'a Fixture,
        output: &'a Path,
        authority: &'a AuthorityProof,
        builder: &'a BuilderIdentity,
    ) -> AuthorRequest<'a> {
        AuthorRequest {
            paths: fixture.paths(),
            output,
            expected: fixture.expected(),
            authority,
            builder,
            source_leaves_per_node: 1,
        }
    }

    #[test]
    fn small_sh3_fixture_authors_deterministic_hashed_exact_bundle() {
        let fixture = Fixture::new();
        let expected = fixture.expected();
        let authority =
            verify_authority(fixture.paths(), expected).expect("verify fixture authority");
        let builder = fixture_builder();
        let output_one = fixture.root.0.join("output-one");
        let output_two = fixture.root.0.join("output-two");
        let outcome_one =
            author_bundle(fixture_request(&fixture, &output_one, &authority, &builder))
                .expect("author first bundle");
        let outcome_two =
            author_bundle(fixture_request(&fixture, &output_two, &authority, &builder))
                .expect("author second bundle");

        assert_eq!(outcome_one, outcome_two);
        assert_eq!(read_tree(&output_one), read_tree(&output_two));
        let receipt_one = &outcome_one.receipt;
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
        let complete_input = &receipt_one["cuts"][0]["render_input"];
        assert_eq!(complete["active_proxy_splats"], 8);
        assert_eq!(complete["represented_source_leaves"], 8);
        assert_eq!(complete["source_sh_degree"], 3);
        assert_eq!(complete["sampling"], "disabled");
        assert_eq!(complete_input["kind"], "content_addressed_source_ply_alias");
        assert_eq!(complete_input["sha256"], fixture.source_sha256);
        assert_eq!(complete_input["bytes"], fixture.source_bytes);
        assert_eq!(complete_input["P"], 8);
        assert_eq!(complete_input["copied_into_package"], false);
        assert!(!output_one.join("cuts/complete_leaf_exact.ply").exists());

        for cut_index in [1, 2] {
            let cut = &receipt_one["cuts"][cut_index];
            let coverage = &cut["coverage"];
            let input = &cut["render_input"];
            assert_eq!(input["kind"], "materialized_binary_little_endian_sh3_ply");
            assert_eq!(input["P"], coverage["active_proxy_splats"]);
            assert_eq!(input["ordered_node_ids"], coverage["ordered_node_ids"]);
            assert_eq!(input["page_sha256"], coverage["page_sha256"]);
            let path = input["path"].as_str().expect("cut PLY path");
            let retained = fs::read(output_one.join(path)).expect("read cut PLY");
            assert!(retained.starts_with(b"ply\nformat binary_little_endian 1.0\n"));
            assert_eq!(hash_bytes(&retained), input["sha256"]);
        }
        let mixed = &receipt_one["cuts"][2]["coverage"];
        assert_eq!(mixed["replacement_count"], 2);
        assert!(mixed["depth_count"].as_u64().expect("depth count") >= 2);
        assert_eq!(
            receipt_one["authority"]["builder"]["executable_sha256"],
            builder.executable_sha256
        );
        assert!(
            receipt_one["authority"]["builder"]["identity_semantics"]["combined"]
                .as_str()
                .expect("combined identity semantics")
                .contains("independently reproduce")
        );
    }

    #[test]
    fn staged_manifest_page_cut_ply_and_receipt_tampering_never_publish_final_output() {
        for target in ["manifest", "page", "cut-ply", "receipt"] {
            let fixture = Fixture::new();
            let expected = fixture.expected();
            let authority =
                verify_authority(fixture.paths(), expected).expect("verify fixture authority");
            let output = fixture.root.0.join(format!("tampered-{target}"));
            let builder = fixture_builder();
            let result = write_bundle_with_hooks(
                fixture_request(&fixture, &output, &authority, &builder),
                |staging| {
                    let path = match target {
                        "manifest" => staging.join("manifest.bin"),
                        "receipt" => staging.join("cut-receipt.json"),
                        "page" => fs::read_dir(staging.join("pages"))?
                            .next()
                            .ok_or_else(|| invalid("missing staged test page"))??
                            .path(),
                        "cut-ply" => staging.join("cuts/bootstrap_roots.ply"),
                        _ => unreachable!(),
                    };
                    let mut bytes = fs::read(&path)?;
                    bytes.push(0x5a);
                    fs::write(path, bytes)?;
                    Ok(())
                },
                verify_staged_bundle,
            );
            assert!(result.is_err(), "{target} tampering must fail");
            assert!(
                !output.exists(),
                "{target} tampering published final output"
            );
            assert!(
                staging_path(&output)
                    .expect("staging path")
                    .join("cut-receipt.json")
                    .is_file(),
                "failed staging evidence must be preserved"
            );
        }
    }

    #[test]
    fn source_camera_and_manifest_drift_before_publication_are_rejected() {
        for target in ["source", "camera", "manifest"] {
            let fixture = Fixture::new();
            let expected = fixture.expected();
            let authority =
                verify_authority(fixture.paths(), expected).expect("verify fixture authority");
            let output = fixture.root.0.join(format!("drifted-{target}"));
            let builder = fixture_builder();
            let result = write_bundle_with_hooks(
                fixture_request(&fixture, &output, &authority, &builder),
                |_| {
                    let path = match target {
                        "source" => &fixture.source,
                        "camera" => &fixture.cameras,
                        "manifest" => &fixture.dataset_manifest,
                        _ => unreachable!(),
                    };
                    let mut bytes = fs::read(path)?;
                    bytes.push(b' ');
                    fs::write(path, bytes)?;
                    Ok(())
                },
                verify_staged_bundle,
            );
            assert!(result.is_err(), "{target} drift must fail");
            assert!(!output.exists(), "{target} drift published final output");
            assert!(staging_path(&output).expect("staging path").is_dir());
        }
    }

    #[test]
    fn identical_manifest_bytes_at_an_alternate_path_are_rejected() {
        let fixture = Fixture::new();
        let alternate = fixture.root.0.join("copied-dataset.json");
        fs::copy(&fixture.dataset_manifest, &alternate).expect("copy manifest");
        let paths = AuthorityPaths {
            dataset_manifest: &alternate,
            ..fixture.paths()
        };
        let error = verify_authority(paths, fixture.expected()).expect_err("reject alternate path");
        assert!(error.to_string().contains("canonical repository path"));
    }

    #[test]
    fn disk_admission_parser_uses_posix_available_kib_column() {
        let output = "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk2s5 100000 40000 60000 40% /\n";
        assert_eq!(
            parse_df_available_bytes(output).expect("parse df"),
            61_440_000
        );
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
