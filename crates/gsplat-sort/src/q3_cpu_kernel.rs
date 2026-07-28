//! Private, qualification-only CPU-kernel selection.
//!
//! Product builds do not compile this module. The runtime feature exists so a
//! physical-device matrix can install one immutable APK, select one kernel at
//! process launch, and attest the actual Rust leaf without repeatedly asking
//! Android's package manager to replace the application.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum QualificationCpuKernel {
    Scalar,
    Neon,
}

impl QualificationCpuKernel {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Scalar => "scalar",
            Self::Neon => "neon",
        }
    }
}

#[cfg(feature = "qualification-q3-cpu-runtime")]
const RUNTIME_SELECTOR_ENV: &str = "GSPLAT_Q3_CPU_KERNEL";

pub(crate) fn qualification_cpu_kernel() -> QualificationCpuKernel {
    #[cfg(feature = "qualification-q3-cpu-scalar")]
    {
        QualificationCpuKernel::Scalar
    }

    #[cfg(feature = "qualification-q3-cpu-neon")]
    {
        QualificationCpuKernel::Neon
    }

    #[cfg(feature = "qualification-q3-cpu-runtime")]
    {
        use std::sync::OnceLock;

        static SELECTED: OnceLock<QualificationCpuKernel> = OnceLock::new();
        *SELECTED.get_or_init(|| match std::env::var(RUNTIME_SELECTOR_ENV).as_deref() {
            Ok("scalar") => QualificationCpuKernel::Scalar,
            Ok("neon") => QualificationCpuKernel::Neon,
            Ok(value) => panic!(
                "{RUNTIME_SELECTOR_ENV} must be scalar or neon for Q3 qualification, got {value:?}"
            ),
            Err(_) => {
                panic!("{RUNTIME_SELECTOR_ENV} is required for the Q3 runtime qualification build")
            }
        })
    }
}

pub fn qualification_cpu_kernel_label() -> &'static str {
    qualification_cpu_kernel().label()
}

#[cfg(test)]
mod tests {
    #[cfg(any(
        feature = "qualification-q3-cpu-scalar",
        feature = "qualification-q3-cpu-neon"
    ))]
    use super::qualification_cpu_kernel_label;

    #[cfg(feature = "qualification-q3-cpu-scalar")]
    #[test]
    fn fixed_scalar_build_attests_scalar() {
        assert_eq!(qualification_cpu_kernel_label(), "scalar");
    }

    #[cfg(feature = "qualification-q3-cpu-neon")]
    #[test]
    fn fixed_neon_build_attests_neon() {
        assert_eq!(qualification_cpu_kernel_label(), "neon");
    }
}
