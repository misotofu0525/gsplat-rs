#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GeometryPath {
    #[default]
    SortedIndexDirect,
    PackedAtlas,
    /// Phase D experimental path: spatial pages uploaded into a fixed GPU atlas.
    PagedActiveAtlas,
}

/// Backend that actually supplied the order presented by one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceOrderBackendUsed {
    Cpu,
    Gpu,
}

/// Exact projected draw path used for one presented frame.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SurfaceProjectedDrawExecution {
    /// Project every visible candidate, compute exact contributor count C,
    /// and draw the original V candidates without a compaction scatter.
    #[default]
    Candidate,
    /// Project, scan, and stably compact contributors before drawing exactly
    /// the compacted C instances.
    Compact,
}

impl SurfaceProjectedDrawExecution {
    pub const fn exact_contributor_compaction(self) -> bool {
        matches!(self, Self::Compact)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreprocessOutput {
    pub depth_keys: Vec<u32>,
    pub indices: Vec<u32>,
}

#[cfg(test)]
mod tests {
    use super::{SurfaceOrderBackendUsed, SurfaceProjectedDrawExecution};

    #[test]
    fn execution_identities_remain_available_at_the_crate_root() {
        let order: crate::SurfaceOrderBackendUsed = SurfaceOrderBackendUsed::Cpu;
        let projected: crate::SurfaceProjectedDrawExecution =
            SurfaceProjectedDrawExecution::Candidate;

        assert_eq!(order, SurfaceOrderBackendUsed::Cpu);
        assert_eq!(projected, SurfaceProjectedDrawExecution::Candidate);
    }

    #[test]
    fn execution_identity_equality_and_default_are_unchanged() {
        assert_eq!(SurfaceOrderBackendUsed::Cpu, SurfaceOrderBackendUsed::Cpu);
        assert_ne!(SurfaceOrderBackendUsed::Cpu, SurfaceOrderBackendUsed::Gpu);
        assert_eq!(
            SurfaceProjectedDrawExecution::default(),
            SurfaceProjectedDrawExecution::Candidate
        );
        assert_ne!(
            SurfaceProjectedDrawExecution::Candidate,
            SurfaceProjectedDrawExecution::Compact
        );
    }

    #[test]
    fn projected_execution_reports_exact_compaction_semantics() {
        assert!(!SurfaceProjectedDrawExecution::Candidate.exact_contributor_compaction());
        assert!(SurfaceProjectedDrawExecution::Compact.exact_contributor_compaction());
    }
}
