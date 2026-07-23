//! Exact-count compact CPU scene ownership and encoding.

mod budget;
mod builder;
mod codec;
mod resident;

#[cfg(test)]
mod tests;

pub use budget::ResidentCpuByteAccounting;
pub use builder::ResidentSceneBuilder;
pub use codec::resident_sh_plane_count;
pub use resident::{
    ResidentEncodingReport, ResidentSceneCpu, ResidentSceneError, ResidentSourceSplat,
};
