//! PlayCanvas SOG and Streamed SOG metadata.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::SogError;

#[derive(Debug, Clone, PartialEq)]
pub struct SogChunkMeta {
    pub version: u32,
    pub count: usize,
    pub antialiased: bool,
    pub means_mins: [f32; 3],
    pub means_maxs: [f32; 3],
    pub means_files: [String; 2],
    pub scales_codebook: [f32; 256],
    pub scales_file: String,
    pub quats_file: String,
    pub sh0_codebook: [f32; 256],
    pub sh0_file: String,
    pub shn: Option<SogShnMeta>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SogShnMeta {
    pub count: usize,
    pub bands: u8,
    pub codebook: [f32; 256],
    pub centroids_file: String,
    pub labels_file: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LodMeta {
    pub version: u32,
    pub count: usize,
    pub counts: Vec<usize>,
    pub lod_levels: u32,
    pub environment: Option<String>,
    pub filenames: Vec<String>,
    pub tree: LodNode,
    pub root: PathBuf,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LodNode {
    pub bound_min: [f32; 3],
    pub bound_max: [f32; 3],
    pub children: Option<(Box<LodNode>, Box<LodNode>)>,
    pub lods: BTreeMap<u32, LodRange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LodRange {
    pub file: usize,
    pub offset: usize,
    pub count: usize,
}

#[derive(Deserialize)]
struct ChunkMetaFile {
    version: Option<u32>,
    count: usize,
    #[serde(default)]
    antialias: bool,
    means: MeansFile,
    scales: CodebookFile,
    quats: FilesOne,
    sh0: CodebookFile,
    #[serde(rename = "shN")]
    shn: Option<ShnFile>,
}

#[derive(Deserialize)]
struct MeansFile {
    mins: [f32; 3],
    maxs: [f32; 3],
    files: Vec<String>,
}

#[derive(Deserialize)]
struct CodebookFile {
    codebook: Vec<f32>,
    files: Vec<String>,
}

#[derive(Deserialize)]
struct FilesOne {
    files: Vec<String>,
}

#[derive(Deserialize)]
struct ShnFile {
    count: usize,
    bands: u8,
    codebook: Vec<f32>,
    files: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LodMetaFile {
    version: Option<u32>,
    count: Option<usize>,
    counts: Option<Vec<usize>>,
    lod_levels: Option<u32>,
    #[serde(default)]
    environment: Option<String>,
    filenames: Vec<String>,
    tree: LodNodeFile,
}

#[derive(Deserialize)]
struct LodNodeFile {
    bound: BoundFile,
    children: Option<[Box<LodNodeFile>; 2]>,
    lods: Option<BTreeMap<String, LodRangeFile>>,
}

#[derive(Deserialize)]
struct BoundFile {
    min: [f32; 3],
    max: [f32; 3],
}

#[derive(Deserialize)]
struct LodRangeFile {
    file: usize,
    offset: usize,
    count: usize,
}

pub fn load_chunk_meta(path: &Path) -> Result<SogChunkMeta, SogError> {
    let raw = fs::read_to_string(path)?;
    parse_chunk_meta(&raw)
}

pub fn parse_chunk_meta(raw: &str) -> Result<SogChunkMeta, SogError> {
    let parsed: ChunkMetaFile = serde_json::from_str(raw)?;
    let version = parsed.version.unwrap_or(2);
    if version > 2 {
        return Err(SogError::UnsupportedVersion(version));
    }
    if parsed.means.files.len() != 2 {
        return Err(SogError::Malformed("means.files must have two entries"));
    }
    if parsed.scales.files.len() != 1
        || parsed.quats.files.len() != 1
        || parsed.sh0.files.len() != 1
    {
        return Err(SogError::Malformed(
            "property files arrays are the wrong length",
        ));
    }
    Ok(SogChunkMeta {
        version,
        count: parsed.count,
        antialiased: parsed.antialias,
        means_mins: parsed.means.mins,
        means_maxs: parsed.means.maxs,
        means_files: [parsed.means.files[0].clone(), parsed.means.files[1].clone()],
        scales_codebook: codebook_256(&parsed.scales.codebook)?,
        scales_file: parsed.scales.files[0].clone(),
        quats_file: parsed.quats.files[0].clone(),
        sh0_codebook: codebook_256(&parsed.sh0.codebook)?,
        sh0_file: parsed.sh0.files[0].clone(),
        shn: match parsed.shn {
            Some(shn) => {
                if shn.files.len() != 2 {
                    return Err(SogError::Malformed("shN.files must have two entries"));
                }
                if !(1..=3).contains(&shn.bands) {
                    return Err(SogError::Malformed("shN.bands must be 1, 2, or 3"));
                }
                Some(SogShnMeta {
                    count: shn.count,
                    bands: shn.bands,
                    codebook: codebook_256(&shn.codebook)?,
                    centroids_file: shn.files[0].clone(),
                    labels_file: shn.files[1].clone(),
                })
            }
            None => None,
        },
    })
}

pub fn load_lod_meta(path: &Path) -> Result<LodMeta, SogError> {
    let raw = fs::read_to_string(path)?;
    let mut meta = parse_lod_meta(&raw)?;
    meta.root = path
        .parent()
        .map(Path::to_path_buf)
        .ok_or(SogError::Malformed("lod-meta.json has no parent directory"))?;
    Ok(meta)
}

pub fn parse_lod_meta(raw: &str) -> Result<LodMeta, SogError> {
    let parsed: LodMetaFile = serde_json::from_str(raw)?;
    let version = parsed.version.unwrap_or(0);
    if version > 1 {
        return Err(SogError::UnsupportedVersion(version));
    }
    if parsed.filenames.is_empty() {
        return Err(SogError::Malformed("filenames must not be empty"));
    }
    let tree = convert_node(parsed.tree, parsed.filenames.len())?;
    let lod_levels = parsed
        .lod_levels
        .unwrap_or_else(|| tree.max_lod().map(|lod| lod.saturating_add(1)).unwrap_or(1));
    let counts = parsed
        .counts
        .unwrap_or_else(|| vec![0; lod_levels as usize]);
    if counts.len() != lod_levels as usize {
        return Err(SogError::Malformed("counts length must match lodLevels"));
    }
    Ok(LodMeta {
        version,
        count: parsed.count.unwrap_or(0),
        counts,
        lod_levels,
        environment: parsed.environment.filter(|value| !value.is_empty()),
        filenames: parsed.filenames,
        tree,
        root: PathBuf::new(),
    })
}

impl LodNode {
    pub fn is_leaf(&self) -> bool {
        self.children.is_none()
    }

    pub fn walk_leaves<'a>(&'a self, out: &mut Vec<&'a LodNode>) {
        if let Some((left, right)) = &self.children {
            left.walk_leaves(out);
            right.walk_leaves(out);
        } else {
            out.push(self);
        }
    }

    fn max_lod(&self) -> Option<u32> {
        if let Some((left, right)) = &self.children {
            return [left.max_lod(), right.max_lod()]
                .into_iter()
                .flatten()
                .max();
        }
        self.lods.keys().copied().max()
    }

    pub fn center(&self) -> [f32; 3] {
        [
            (self.bound_min[0] + self.bound_max[0]) * 0.5,
            (self.bound_min[1] + self.bound_max[1]) * 0.5,
            (self.bound_min[2] + self.bound_max[2]) * 0.5,
        ]
    }
}

fn convert_node(node: LodNodeFile, file_count: usize) -> Result<LodNode, SogError> {
    let has_children = node.children.is_some();
    let has_lods = node.lods.as_ref().is_some_and(|lods| !lods.is_empty());
    if has_children == has_lods {
        return Err(SogError::Malformed(
            "tree node must be either an interior node or a leaf",
        ));
    }
    let children = match node.children {
        Some(pair) => {
            let [left, right] = pair;
            Some((
                Box::new(convert_node(*left, file_count)?),
                Box::new(convert_node(*right, file_count)?),
            ))
        }
        None => None,
    };
    let mut lods = BTreeMap::new();
    if let Some(raw_lods) = node.lods {
        for (key, range) in raw_lods {
            let lod: u32 = key
                .parse()
                .map_err(|_| SogError::Malformed("lods keys must be decimal LOD levels"))?;
            if range.file >= file_count {
                return Err(SogError::Malformed("lod range file index is out of bounds"));
            }
            lods.insert(
                lod,
                LodRange {
                    file: range.file,
                    offset: range.offset,
                    count: range.count,
                },
            );
        }
    }
    Ok(LodNode {
        bound_min: node.bound.min,
        bound_max: node.bound.max,
        children,
        lods,
    })
}

fn codebook_256(values: &[f32]) -> Result<[f32; 256], SogError> {
    if values.len() != 256 {
        return Err(SogError::Malformed("codebook must contain 256 floats"));
    }
    let mut out = [0.0; 256];
    out.copy_from_slice(values);
    Ok(out)
}
