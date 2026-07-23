#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GeometryPath {
    #[default]
    SortedIndexDirect,
    PackedAtlas,
    /// Phase D experimental path: spatial pages uploaded into a fixed GPU atlas.
    PagedActiveAtlas,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreprocessOutput {
    pub depth_keys: Vec<u32>,
    pub indices: Vec<u32>,
}
