//! Sort backend abstraction for depth ordering.

#[cfg(all(
    feature = "qualification-q3-cpu-scalar",
    feature = "qualification-q3-cpu-neon"
))]
compile_error!("Q3 CPU qualification must select exactly one Scalar or Neon kernel");

#[cfg(all(feature = "qualification-q3-cpu-neon", not(target_arch = "aarch64")))]
compile_error!("Q3 Neon qualification requires a native AArch64 target");

#[cfg(all(
    target_arch = "wasm32",
    any(
        feature = "qualification-q3-cpu-scalar",
        feature = "qualification-q3-cpu-neon"
    )
))]
compile_error!("Q3 CPU qualification selectors are native-only");

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
