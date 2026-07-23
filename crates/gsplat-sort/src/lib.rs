//! Sort backend abstraction for depth ordering.

mod cpu;
mod gpu_odd_even;
mod radix;

pub use cpu::CpuSortBackend;
pub use gpu_odd_even::GpuOddEvenSortBackend;

use thiserror::Error;

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum SortError {
    #[error("key/value length mismatch")]
    LengthMismatch,
    #[error("gpu backend unavailable")]
    BackendUnavailable,
    #[error("sort backend failure")]
    BackendFailure,
}

pub trait SortBackend {
    fn name(&self) -> &'static str;

    fn sort_pairs(&mut self, keys: &mut [u32], values: &mut [u32]) -> Result<(), SortError>;
}
