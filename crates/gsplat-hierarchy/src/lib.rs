//! Deterministic offline construction of replacement Gaussian hierarchies.
//!
//! This crate owns authored asset structure only. It deliberately has no
//! renderer, page-source, cache, platform or public FFI dependency. A node page
//! is independently drawable, while its [`LeafRange`] proves which contiguous
//! portion of the exact source leaf sequence it represents.

use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

const MANIFEST_MAGIC: &[u8; 4] = b"GSHM";
const PAGE_MAGIC: &[u8; 4] = b"GSHP";
const MAX_SH_DEGREE: u32 = 3;
const SH3_REST_COMPONENTS: usize = 45;
const DRAWABLE_GAUSSIAN_ENCODED_BYTES: usize =
    (14 + SH3_REST_COMPONENTS) * size_of::<f32>() + size_of::<u32>();

/// Private authored hierarchy schema emitted by this implementation slice.
pub const SCHEMA_VERSION: u32 = 1;

/// Stable manifest-local node identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub u32);

/// Half-open range in the canonical highest-detail source sequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeafRange {
    pub start: u64,
    pub end: u64,
}

impl LeafRange {
    pub fn new(start: u64, end: u64) -> Result<Self, HierarchyError> {
        if start >= end {
            return Err(HierarchyError::InvalidLeafRange { start, end });
        }
        Ok(Self { start, end })
    }

    pub fn contains(self, other: Self) -> bool {
        self.start <= other.start && other.end <= self.end
    }
}

/// Renderer-neutral Gaussian payload used by the offline authoring boundary.
///
/// Leaves preserve these values bit-for-bit. Interior nodes carry newly
/// authored proxies and therefore require later image qualification before
/// they can be promoted to a product path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrawableGaussian {
    pub position: [f32; 3],
    pub scale: [f32; 3],
    pub rotation_xyzw: [f32; 4],
    pub opacity: f32,
    pub sh_dc: [f32; 3],
    /// Source spherical-harmonic degree. The complete fixed SH3 rest plane is
    /// retained so leaf payloads round-trip every source bit at degrees 0--3.
    pub sh_degree: u32,
    pub sh_rest: [f32; SH3_REST_COMPONENTS],
}

impl DrawableGaussian {
    /// Checks the minimum data contract needed for an independent draw record.
    pub fn is_finite_and_drawable(self) -> bool {
        let rotation_norm_squared = self.rotation_xyzw.into_iter().map(|v| v * v).sum::<f32>();
        self.position.into_iter().all(f32::is_finite)
            && self
                .scale
                .into_iter()
                .all(|value| value.is_finite() && value > 0.0)
            && self.rotation_xyzw.into_iter().all(f32::is_finite)
            && rotation_norm_squared.is_finite()
            && rotation_norm_squared > 1.0e-12
            && self.opacity.is_finite()
            && (0.0..=1.0).contains(&self.opacity)
            && self.sh_dc.into_iter().all(f32::is_finite)
            && self.sh_degree <= MAX_SH_DEGREE
            && self.sh_rest.into_iter().all(f32::is_finite)
    }

    fn canonical_bytes(self, output: &mut Vec<u8>) {
        for value in self
            .position
            .into_iter()
            .chain(self.scale)
            .chain(self.rotation_xyzw)
            .chain([self.opacity])
            .chain(self.sh_dc)
        {
            put_f32(output, value);
        }
        put_u32(output, self.sh_degree);
        for value in self.sh_rest {
            put_f32(output, value);
        }
    }
}

/// SHA-256 identity of immutable canonical bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContentHash(pub [u8; 32]);

impl ContentHash {
    pub fn hex(self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut output = String::with_capacity(64);
        for byte in self.0 {
            output.push(HEX[usize::from(byte >> 4)] as char);
            output.push(HEX[usize::from(byte & 0x0f)] as char);
        }
        output
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NodeRecord {
    pub id: NodeId,
    pub leaf_range: LeafRange,
    pub children: Vec<NodeId>,
    pub bounds_min: [f32; 3],
    pub bounds_max: [f32; 3],
    /// Conservative object-space error. Parent error never decreases.
    pub geometric_error: f32,
    pub payload_splat_count: u32,
    pub page_hash: ContentHash,
}

impl NodeRecord {
    pub fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageRecord {
    pub node: NodeId,
    pub content_hash: ContentHash,
    pub encoded_bytes: u64,
    pub decoded_splat_count: u32,
}

/// Canonically ordered metadata. Node ids are dense indices in `nodes`.
#[derive(Clone, Debug, PartialEq)]
pub struct HierarchyManifest {
    pub schema_version: u32,
    pub source_leaf_count: u64,
    pub roots: Vec<NodeId>,
    pub nodes: Vec<NodeRecord>,
    pub pages: Vec<PageRecord>,
}

impl HierarchyManifest {
    /// Stable little-endian encoding used by fixtures and content receipts.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut output = Vec::new();
        output.extend_from_slice(MANIFEST_MAGIC);
        put_u32(&mut output, self.schema_version);
        put_u64(&mut output, self.source_leaf_count);
        let mut roots = self.roots.clone();
        roots.sort_unstable();
        put_u32(&mut output, usize_to_u32(roots.len()));
        for root in &roots {
            put_u32(&mut output, root.0);
        }
        put_u32(&mut output, usize_to_u32(self.nodes.len()));
        for node in &self.nodes {
            put_u32(&mut output, node.id.0);
            put_u64(&mut output, node.leaf_range.start);
            put_u64(&mut output, node.leaf_range.end);
            for value in node.bounds_min.into_iter().chain(node.bounds_max) {
                put_f32(&mut output, value);
            }
            put_f32(&mut output, node.geometric_error);
            put_u32(&mut output, node.payload_splat_count);
            put_u32(&mut output, usize_to_u32(node.children.len()));
            for child in &node.children {
                put_u32(&mut output, child.0);
            }
            output.extend_from_slice(&node.page_hash.0);
        }
        let mut pages = self.pages.iter().collect::<Vec<_>>();
        pages.sort_by_key(|page| {
            (
                page.node,
                page.content_hash,
                page.encoded_bytes,
                page.decoded_splat_count,
            )
        });
        put_u32(&mut output, usize_to_u32(pages.len()));
        for page in pages {
            put_u32(&mut output, page.node.0);
            output.extend_from_slice(&page.content_hash.0);
            put_u64(&mut output, page.encoded_bytes);
            put_u32(&mut output, page.decoded_splat_count);
        }
        output
    }

    pub fn content_hash(&self) -> ContentHash {
        hash_bytes(&self.canonical_bytes())
    }

    pub fn node(&self, id: NodeId) -> Option<&NodeRecord> {
        self.nodes.get(id.0 as usize).filter(|node| node.id == id)
    }

    pub fn leaf_cut(&self) -> Vec<NodeId> {
        let mut leaves = self
            .nodes
            .iter()
            .filter(|node| node.is_leaf())
            .collect::<Vec<_>>();
        leaves.sort_by_key(|node| node.leaf_range.start);
        leaves.into_iter().map(|node| node.id).collect()
    }

    /// Implements the recursive S0 coverage predicate over every root.
    pub fn validate_cut(&self, selected: &[NodeId]) -> Result<(), CutError> {
        let mut set = BTreeSet::new();
        for id in selected {
            if self.node(*id).is_none() {
                return Err(CutError::UnknownNode(*id));
            }
            if !set.insert(*id) {
                return Err(CutError::DuplicateNode(*id));
            }
        }
        let mut visited = BTreeSet::new();
        let mut stack = self
            .roots
            .iter()
            .rev()
            .map(|root| (*root, None))
            .collect::<Vec<_>>();
        while let Some((id, selected_ancestor)) = stack.pop() {
            let node = self.node(id).ok_or(CutError::UnknownNode(id))?;
            if !visited.insert(id) {
                return Err(CutError::InvalidTopology);
            }
            let descendant_owner = if let Some(ancestor) = selected_ancestor {
                if selected.contains(&id) {
                    return Err(CutError::AncestorDescendantOverlap {
                        ancestor,
                        descendant: id,
                    });
                }
                Some(ancestor)
            } else if selected.contains(&id) {
                Some(id)
            } else if node.is_leaf() {
                return Err(CutError::MissingCoverage(id));
            } else {
                None
            };
            for child in node.children.iter().rev() {
                stack.push((*child, descendant_owner));
            }
        }
        if visited.len() != self.nodes.len() {
            return Err(CutError::InvalidTopology);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedPage {
    pub node: NodeId,
    pub content_hash: ContentHash,
    pub bytes: Vec<u8>,
}

/// Complete deterministic offline result. Pages are ordered by node id.
#[derive(Clone, Debug, PartialEq)]
pub struct HierarchyBundle {
    pub manifest: HierarchyManifest,
    pub pages: Vec<EncodedPage>,
}

impl HierarchyBundle {
    /// Validates topology, exact leaf payloads and all content-addressed bytes.
    pub fn validate(&self, source: &[DrawableGaussian]) -> Result<(), HierarchyError> {
        validate_source(source)?;
        if self.manifest.schema_version != SCHEMA_VERSION {
            return Err(HierarchyError::InvalidSchema(self.manifest.schema_version));
        }
        if self.manifest.source_leaf_count != source.len() as u64 {
            return Err(HierarchyError::SourceCountMismatch {
                expected: self.manifest.source_leaf_count,
                actual: source.len() as u64,
            });
        }
        if self.manifest.nodes.is_empty() || self.manifest.roots.is_empty() {
            return Err(HierarchyError::Malformed(
                "hierarchy requires nodes and roots",
            ));
        }
        if self.manifest.nodes.len() != self.manifest.pages.len()
            || self.pages.len() != self.manifest.pages.len()
        {
            return Err(HierarchyError::Malformed(
                "every node must own exactly one page record and page object",
            ));
        }

        for (index, node) in self.manifest.nodes.iter().enumerate() {
            if node.id.0 as usize != index {
                return Err(HierarchyError::Malformed(
                    "node ids must be dense and ordered",
                ));
            }
            validate_node_metadata(node, self.manifest.source_leaf_count)?;
        }
        validate_roots_and_topology(&self.manifest)?;

        let page_objects = self
            .pages
            .iter()
            .map(|page| (page.content_hash, page))
            .collect::<BTreeMap<_, _>>();
        if page_objects.len() != self.pages.len() {
            return Err(HierarchyError::Malformed(
                "page content hashes must be unique",
            ));
        }
        let page_records = self
            .manifest
            .pages
            .iter()
            .map(|page| (page.content_hash, page))
            .collect::<BTreeMap<_, _>>();
        if page_records.len() != self.manifest.pages.len() {
            return Err(HierarchyError::Malformed(
                "manifest page hashes must be unique",
            ));
        }

        for node in &self.manifest.nodes {
            let record = page_records
                .get(&node.page_hash)
                .ok_or(HierarchyError::MissingPage(node.id))?;
            let page = page_objects
                .get(&node.page_hash)
                .ok_or(HierarchyError::MissingPage(node.id))?;
            if record.node != node.id || page.node != node.id {
                return Err(HierarchyError::PageNodeMismatch(node.id));
            }
            if hash_bytes(&page.bytes) != node.page_hash
                || record.encoded_bytes != page.bytes.len() as u64
            {
                return Err(HierarchyError::PageHashMismatch(node.id));
            }
            let decoded = decode_page(&page.bytes)?;
            if decoded.node != node.id
                || decoded.range != node.leaf_range
                || decoded.splats.len() as u32 != node.payload_splat_count
                || record.decoded_splat_count != node.payload_splat_count
            {
                return Err(HierarchyError::PageNodeMismatch(node.id));
            }
            for gaussian in &decoded.splats {
                if !gaussian.is_finite_and_drawable() {
                    return Err(HierarchyError::NonDrawableNode(node.id));
                }
            }
            if node.is_leaf() {
                let expected =
                    &source[node.leaf_range.start as usize..node.leaf_range.end as usize];
                if !gaussian_slices_bitwise_equal(&decoded.splats, expected) {
                    return Err(HierarchyError::LeafPayloadMismatch(node.id));
                }
                if node.geometric_error.to_bits() != 0.0_f32.to_bits() {
                    return Err(HierarchyError::Malformed(
                        "leaf geometric error must be zero",
                    ));
                }
            }
        }
        Ok(())
    }

    /// Decodes and concatenates one complete cut in source-range order.
    pub fn materialize_cut(
        &self,
        source: &[DrawableGaussian],
        selected: &[NodeId],
    ) -> Result<Vec<DrawableGaussian>, HierarchyError> {
        // Materialization is a trusted-bundle operation: topology, every page
        // length/hash and exact leaf payloads are validated before following
        // graph edges or decoding the requested cut.
        self.validate(source)?;
        self.manifest
            .validate_cut(selected)
            .map_err(HierarchyError::InvalidCut)?;
        let page_by_node = self
            .pages
            .iter()
            .map(|page| (page.node, page))
            .collect::<BTreeMap<_, _>>();
        let mut ordered = selected
            .iter()
            .map(|id| self.manifest.node(*id).expect("cut ids were validated"))
            .collect::<Vec<_>>();
        ordered.sort_by_key(|node| node.leaf_range.start);
        let mut output = Vec::new();
        for node in ordered {
            let page = page_by_node
                .get(&node.id)
                .ok_or(HierarchyError::MissingPage(node.id))?;
            output.extend(decode_page(&page.bytes)?.splats);
        }
        Ok(output)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildConfig {
    /// Number of exact source leaves stored in each highest-detail node.
    pub source_leaves_per_node: u32,
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            source_leaves_per_node: 256,
        }
    }
}

/// Builds a deterministic binary replacement hierarchy over contiguous source
/// ranges. Odd nodes are carried to the next level unchanged, producing valid
/// mixed-depth shapes without fabricating unary parents.
pub fn build_authored_proxy_hierarchy(
    source: &[DrawableGaussian],
    config: BuildConfig,
) -> Result<HierarchyBundle, HierarchyError> {
    validate_source(source)?;
    if source.is_empty() {
        return Err(HierarchyError::EmptySource);
    }
    if config.source_leaves_per_node == 0 {
        return Err(HierarchyError::InvalidConfig(
            "source_leaves_per_node must be non-zero",
        ));
    }
    if source.len() > u32::MAX as usize {
        return Err(HierarchyError::TooManySourceLeaves(source.len() as u64));
    }

    let mut nodes = Vec::<DraftNode>::new();
    let leaf_capacity = config.source_leaves_per_node as usize;
    let mut level = Vec::new();
    for (chunk_index, chunk) in source.chunks(leaf_capacity).enumerate() {
        let start = chunk_index
            .checked_mul(leaf_capacity)
            .ok_or(HierarchyError::ArithmeticOverflow)?;
        let end = start
            .checked_add(chunk.len())
            .ok_or(HierarchyError::ArithmeticOverflow)?;
        let id = next_node_id(nodes.len())?;
        nodes.push(DraftNode {
            id,
            leaf_range: LeafRange::new(start as u64, end as u64)?,
            children: Vec::new(),
            bounds: source_bounds(chunk),
            geometric_error: 0.0,
            payload: chunk.to_vec(),
        });
        level.push(id);
    }

    while level.len() > 1 {
        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        for pair in level.chunks(2) {
            if pair.len() == 1 {
                next.push(pair[0]);
                continue;
            }
            let left = &nodes[pair[0].0 as usize];
            let right = &nodes[pair[1].0 as usize];
            if left.leaf_range.end != right.leaf_range.start {
                return Err(HierarchyError::Malformed(
                    "builder produced a non-contiguous sibling pair",
                ));
            }
            let range = LeafRange::new(left.leaf_range.start, right.leaf_range.end)?;
            let source_slice = &source[range.start as usize..range.end as usize];
            let payload = vec![author_proxy(source_slice)];
            let child_error = left.geometric_error.max(right.geometric_error);
            let authored_error = proxy_error(source_slice, payload[0]);
            let id = next_node_id(nodes.len())?;
            nodes.push(DraftNode {
                id,
                leaf_range: range,
                children: pair.to_vec(),
                bounds: source_bounds(source_slice),
                geometric_error: authored_error.max(child_error),
                payload,
            });
            next.push(id);
        }
        level = next;
    }

    let mut records = Vec::with_capacity(nodes.len());
    let mut page_records = Vec::with_capacity(nodes.len());
    let mut pages = Vec::with_capacity(nodes.len());
    for node in nodes {
        let bytes = encode_page(&node);
        let content_hash = hash_bytes(&bytes);
        let payload_splat_count = usize_to_u32(node.payload.len());
        records.push(NodeRecord {
            id: node.id,
            leaf_range: node.leaf_range,
            children: node.children,
            bounds_min: node.bounds.0,
            bounds_max: node.bounds.1,
            geometric_error: node.geometric_error,
            payload_splat_count,
            page_hash: content_hash,
        });
        page_records.push(PageRecord {
            node: node.id,
            content_hash,
            encoded_bytes: bytes.len() as u64,
            decoded_splat_count: payload_splat_count,
        });
        pages.push(EncodedPage {
            node: node.id,
            content_hash,
            bytes,
        });
    }

    let bundle = HierarchyBundle {
        manifest: HierarchyManifest {
            schema_version: SCHEMA_VERSION,
            source_leaf_count: source.len() as u64,
            roots: level,
            nodes: records,
            pages: page_records,
        },
        pages,
    };
    bundle.validate(source)?;
    Ok(bundle)
}

#[derive(Clone, Debug)]
struct DraftNode {
    id: NodeId,
    leaf_range: LeafRange,
    children: Vec<NodeId>,
    bounds: ([f32; 3], [f32; 3]),
    geometric_error: f32,
    payload: Vec<DrawableGaussian>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CutError {
    UnknownNode(NodeId),
    DuplicateNode(NodeId),
    InvalidTopology,
    MissingCoverage(NodeId),
    AncestorDescendantOverlap {
        ancestor: NodeId,
        descendant: NodeId,
    },
}

impl fmt::Display for CutError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownNode(id) => write!(formatter, "unknown hierarchy node {}", id.0),
            Self::DuplicateNode(id) => write!(formatter, "duplicate cut node {}", id.0),
            Self::InvalidTopology => {
                formatter.write_str("cut requires an acyclic single-parent hierarchy")
            }
            Self::MissingCoverage(id) => {
                write!(formatter, "source lineage at node {} is uncovered", id.0)
            }
            Self::AncestorDescendantOverlap {
                ancestor,
                descendant,
            } => write!(
                formatter,
                "cut contains ancestor {} and descendant {}",
                ancestor.0, descendant.0
            ),
        }
    }
}

impl Error for CutError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HierarchyError {
    EmptySource,
    InvalidConfig(&'static str),
    InvalidLeafRange {
        start: u64,
        end: u64,
    },
    TooManySourceLeaves(u64),
    TooManyNodes(u64),
    ArithmeticOverflow,
    NonDrawableSource(usize),
    MixedSourceShDegree {
        expected: u32,
        actual: u32,
        index: usize,
    },
    NonDrawableNode(NodeId),
    InvalidSchema(u32),
    SourceCountMismatch {
        expected: u64,
        actual: u64,
    },
    Malformed(&'static str),
    MissingPage(NodeId),
    PageHashMismatch(NodeId),
    PageNodeMismatch(NodeId),
    LeafPayloadMismatch(NodeId),
    TruncatedPage,
    InvalidPage(&'static str),
    InvalidCut(CutError),
}

impl fmt::Display for HierarchyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySource => write!(formatter, "source hierarchy cannot be empty"),
            Self::InvalidConfig(message)
            | Self::Malformed(message)
            | Self::InvalidPage(message) => formatter.write_str(message),
            Self::InvalidLeafRange { start, end } => {
                write!(formatter, "invalid leaf range {start}..{end}")
            }
            Self::TooManySourceLeaves(count) => {
                write!(formatter, "source leaf count {count} exceeds schema limits")
            }
            Self::TooManyNodes(count) => {
                write!(formatter, "node count {count} exceeds schema limits")
            }
            Self::ArithmeticOverflow => formatter.write_str("hierarchy arithmetic overflow"),
            Self::NonDrawableSource(index) => write!(
                formatter,
                "source Gaussian {index} is not finite and drawable"
            ),
            Self::MixedSourceShDegree {
                expected,
                actual,
                index,
            } => write!(
                formatter,
                "source Gaussian {index} has SH degree {actual}, expected {expected}"
            ),
            Self::NonDrawableNode(id) => {
                write!(formatter, "node {} contains a non-drawable Gaussian", id.0)
            }
            Self::InvalidSchema(version) => {
                write!(formatter, "unsupported hierarchy schema {version}")
            }
            Self::SourceCountMismatch { expected, actual } => write!(
                formatter,
                "source count mismatch: manifest {expected}, provided {actual}"
            ),
            Self::MissingPage(id) => write!(formatter, "node {} has no page", id.0),
            Self::PageHashMismatch(id) => {
                write!(formatter, "node {} page hash or length mismatch", id.0)
            }
            Self::PageNodeMismatch(id) => write!(formatter, "node {} page metadata mismatch", id.0),
            Self::LeafPayloadMismatch(id) => write!(
                formatter,
                "leaf node {} does not reproduce its source range",
                id.0
            ),
            Self::TruncatedPage => formatter.write_str("truncated hierarchy page"),
            Self::InvalidCut(error) => error.fmt(formatter),
        }
    }
}

impl Error for HierarchyError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidCut(error) => Some(error),
            _ => None,
        }
    }
}

fn validate_source(source: &[DrawableGaussian]) -> Result<(), HierarchyError> {
    let expected_sh_degree = source.first().map(|gaussian| gaussian.sh_degree);
    for (index, gaussian) in source.iter().copied().enumerate() {
        if !gaussian.is_finite_and_drawable() {
            return Err(HierarchyError::NonDrawableSource(index));
        }
        if Some(gaussian.sh_degree) != expected_sh_degree {
            return Err(HierarchyError::MixedSourceShDegree {
                expected: expected_sh_degree.expect("non-empty iteration"),
                actual: gaussian.sh_degree,
                index,
            });
        }
    }
    Ok(())
}

fn validate_node_metadata(node: &NodeRecord, source_count: u64) -> Result<(), HierarchyError> {
    if node.leaf_range.start >= node.leaf_range.end || node.leaf_range.end > source_count {
        return Err(HierarchyError::InvalidLeafRange {
            start: node.leaf_range.start,
            end: node.leaf_range.end,
        });
    }
    if node.payload_splat_count == 0 {
        return Err(HierarchyError::NonDrawableNode(node.id));
    }
    if !node.geometric_error.is_finite() || node.geometric_error < 0.0 {
        return Err(HierarchyError::Malformed(
            "node error must be finite and non-negative",
        ));
    }
    for axis in 0..3 {
        if !node.bounds_min[axis].is_finite()
            || !node.bounds_max[axis].is_finite()
            || node.bounds_min[axis] > node.bounds_max[axis]
        {
            return Err(HierarchyError::Malformed(
                "node bounds must be finite and ordered",
            ));
        }
    }
    Ok(())
}

fn validate_roots_and_topology(manifest: &HierarchyManifest) -> Result<(), HierarchyError> {
    let mut root_ids = BTreeSet::new();
    for root in &manifest.roots {
        if !root_ids.insert(*root) {
            return Err(HierarchyError::Malformed("root ids must be unique"));
        }
    }
    let mut root_ranges = manifest
        .roots
        .iter()
        .map(|id| {
            manifest
                .node(*id)
                .ok_or(HierarchyError::Malformed("unknown root"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    root_ranges.sort_by_key(|node| node.leaf_range.start);
    let mut cursor = 0_u64;
    for root in root_ranges {
        if root.leaf_range.start != cursor {
            return Err(HierarchyError::Malformed(
                "root ranges must be ordered and gap-free",
            ));
        }
        cursor = root.leaf_range.end;
    }
    if cursor != manifest.source_leaf_count {
        return Err(HierarchyError::Malformed(
            "root ranges must cover every source leaf",
        ));
    }

    let mut parent_count = vec![0_u32; manifest.nodes.len()];
    for node in &manifest.nodes {
        if node.is_leaf() {
            continue;
        }
        let mut cursor = node.leaf_range.start;
        for child_id in &node.children {
            let child = manifest.node(*child_id).ok_or(HierarchyError::Malformed(
                "node references an unknown child",
            ))?;
            if child.leaf_range.start != cursor
                || !node.leaf_range.contains(child.leaf_range)
                || child.geometric_error > node.geometric_error
            {
                return Err(HierarchyError::Malformed(
                    "children must partition the parent range with monotone error",
                ));
            }
            cursor = child.leaf_range.end;
            parent_count[child_id.0 as usize] = parent_count[child_id.0 as usize]
                .checked_add(1)
                .ok_or(HierarchyError::ArithmeticOverflow)?;
        }
        if cursor != node.leaf_range.end {
            return Err(HierarchyError::Malformed(
                "children must cover the complete parent range",
            ));
        }
    }

    // Iterative color-marked DFS keeps malformed cycles on the error path
    // without consuming an unbounded native stack.
    let mut state = vec![0_u8; manifest.nodes.len()];
    for root in &manifest.roots {
        let mut stack = vec![(*root, false)];
        while let Some((id, leaving)) = stack.pop() {
            let index = id.0 as usize;
            let node = manifest
                .node(id)
                .ok_or(HierarchyError::Malformed("unknown root"))?;
            if leaving {
                state[index] = 2;
                continue;
            }
            match state[index] {
                1 => return Err(HierarchyError::Malformed("hierarchy contains a cycle")),
                2 => continue,
                _ => state[index] = 1,
            }
            stack.push((id, true));
            for child in node.children.iter().rev() {
                stack.push((*child, false));
            }
        }
    }

    if state.iter().any(|entry| *entry != 2) {
        return Err(HierarchyError::Malformed(
            "all nodes must be reachable from a root",
        ));
    }
    for node in &manifest.nodes {
        let expected = u32::from(!root_ids.contains(&node.id));
        if parent_count[node.id.0 as usize] != expected {
            return Err(HierarchyError::Malformed(
                "every non-root node must have exactly one parent",
            ));
        }
    }
    Ok(())
}

fn author_proxy(source: &[DrawableGaussian]) -> DrawableGaussian {
    let count = source.len() as f64;
    let mut center = [0.0_f64; 3];
    let mut color = [0.0_f64; 3];
    let mut sh_rest = [0.0_f64; SH3_REST_COMPONENTS];
    let mut color_weight = 0.0_f64;
    let mut alpha_product = 1.0_f64;
    for splat in source {
        for (axis, total) in center.iter_mut().enumerate() {
            *total += f64::from(splat.position[axis]);
        }
        let weight = f64::from(splat.opacity).max(1.0e-6);
        color_weight += weight;
        for (channel, total) in color.iter_mut().enumerate() {
            *total += f64::from(splat.sh_dc[channel]) * weight;
        }
        for (component, total) in sh_rest.iter_mut().enumerate() {
            *total += f64::from(splat.sh_rest[component]) * weight;
        }
        alpha_product *= 1.0 - f64::from(splat.opacity);
    }
    let position = center.map(|value| (value / count) as f32);
    let mut scale = [f32::MIN_POSITIVE; 3];
    for splat in source {
        let radius = splat.scale.into_iter().fold(0.0_f32, f32::max);
        for axis in 0..3 {
            scale[axis] = scale[axis].max((splat.position[axis] - position[axis]).abs() + radius);
        }
    }
    DrawableGaussian {
        position,
        scale,
        rotation_xyzw: [0.0, 0.0, 0.0, 1.0],
        opacity: (1.0 - alpha_product) as f32,
        sh_dc: color.map(|value| (value / color_weight) as f32),
        sh_degree: source[0].sh_degree,
        sh_rest: sh_rest.map(|value| (value / color_weight) as f32),
    }
}

fn proxy_error(source: &[DrawableGaussian], proxy: DrawableGaussian) -> f32 {
    source.iter().fold(0.0_f32, |error, splat| {
        let distance_squared = (0..3)
            .map(|axis| {
                let delta = splat.position[axis] - proxy.position[axis];
                delta * delta
            })
            .sum::<f32>();
        let radius = splat.scale.into_iter().fold(0.0_f32, f32::max);
        error.max(distance_squared.sqrt() + radius)
    })
}

fn source_bounds(source: &[DrawableGaussian]) -> ([f32; 3], [f32; 3]) {
    let mut minimum = [f32::INFINITY; 3];
    let mut maximum = [f32::NEG_INFINITY; 3];
    for splat in source {
        let radius = splat.scale.into_iter().fold(0.0_f32, f32::max);
        for axis in 0..3 {
            minimum[axis] = minimum[axis].min(splat.position[axis] - radius);
            maximum[axis] = maximum[axis].max(splat.position[axis] + radius);
        }
    }
    (minimum, maximum)
}

fn encode_page(node: &DraftNode) -> Vec<u8> {
    let mut output = Vec::new();
    output.extend_from_slice(PAGE_MAGIC);
    put_u32(&mut output, SCHEMA_VERSION);
    put_u32(&mut output, node.id.0);
    put_u64(&mut output, node.leaf_range.start);
    put_u64(&mut output, node.leaf_range.end);
    put_u32(&mut output, usize_to_u32(node.payload.len()));
    for gaussian in &node.payload {
        gaussian.canonical_bytes(&mut output);
    }
    output
}

struct DecodedPage {
    node: NodeId,
    range: LeafRange,
    splats: Vec<DrawableGaussian>,
}

fn decode_page(bytes: &[u8]) -> Result<DecodedPage, HierarchyError> {
    let mut cursor = Cursor::new(bytes);
    if cursor.take(4)? != PAGE_MAGIC {
        return Err(HierarchyError::InvalidPage("invalid hierarchy page magic"));
    }
    let version = cursor.u32()?;
    if version != SCHEMA_VERSION {
        return Err(HierarchyError::InvalidSchema(version));
    }
    let node = NodeId(cursor.u32()?);
    let range = LeafRange::new(cursor.u64()?, cursor.u64()?)?;
    let splat_count = cursor.u32()? as usize;
    let expected_payload_bytes = splat_count
        .checked_mul(DRAWABLE_GAUSSIAN_ENCODED_BYTES)
        .ok_or(HierarchyError::ArithmeticOverflow)?;
    if cursor.remaining() != expected_payload_bytes {
        return Err(HierarchyError::InvalidPage(
            "page payload length does not match splat count",
        ));
    }
    let mut splats = Vec::with_capacity(splat_count);
    for _ in 0..splat_count {
        let position = [cursor.f32()?, cursor.f32()?, cursor.f32()?];
        let scale = [cursor.f32()?, cursor.f32()?, cursor.f32()?];
        let rotation_xyzw = [cursor.f32()?, cursor.f32()?, cursor.f32()?, cursor.f32()?];
        let opacity = cursor.f32()?;
        let sh_dc = [cursor.f32()?, cursor.f32()?, cursor.f32()?];
        let sh_degree = cursor.u32()?;
        let mut sh_rest = [0.0_f32; SH3_REST_COMPONENTS];
        for value in &mut sh_rest {
            *value = cursor.f32()?;
        }
        splats.push(DrawableGaussian {
            position,
            scale,
            rotation_xyzw,
            opacity,
            sh_dc,
            sh_degree,
            sh_rest,
        });
    }
    Ok(DecodedPage {
        node,
        range,
        splats,
    })
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], HierarchyError> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or(HierarchyError::ArithmeticOverflow)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(HierarchyError::TruncatedPage)?;
        self.offset = end;
        Ok(value)
    }

    fn u32(&mut self) -> Result<u32, HierarchyError> {
        let bytes: [u8; 4] = self.take(4)?.try_into().expect("fixed-size slice");
        Ok(u32::from_le_bytes(bytes))
    }

    fn u64(&mut self) -> Result<u64, HierarchyError> {
        let bytes: [u8; 8] = self.take(8)?.try_into().expect("fixed-size slice");
        Ok(u64::from_le_bytes(bytes))
    }

    fn f32(&mut self) -> Result<f32, HierarchyError> {
        Ok(f32::from_bits(self.u32()?))
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.offset
    }
}

fn gaussian_slices_bitwise_equal(left: &[DrawableGaussian], right: &[DrawableGaussian]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut left_bytes = Vec::new();
    let mut right_bytes = Vec::new();
    for gaussian in left {
        gaussian.canonical_bytes(&mut left_bytes);
    }
    for gaussian in right {
        gaussian.canonical_bytes(&mut right_bytes);
    }
    left_bytes == right_bytes
}

fn hash_bytes(bytes: &[u8]) -> ContentHash {
    ContentHash(Sha256::digest(bytes).into())
}

fn next_node_id(count: usize) -> Result<NodeId, HierarchyError> {
    u32::try_from(count)
        .map(NodeId)
        .map_err(|_| HierarchyError::TooManyNodes(count as u64))
}

fn usize_to_u32(value: usize) -> u32 {
    u32::try_from(value).expect("validated hierarchy schema bounds")
}

fn put_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn put_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn put_f32(output: &mut Vec<u8>, value: f32) {
    put_u32(output, value.to_bits());
}
