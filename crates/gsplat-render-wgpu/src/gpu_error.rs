use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ResidentGpuError {
    #[error("resident scene is incomplete")]
    IncompleteScene,
    #[error("resident upload staging has already been released")]
    UploadStagingUnavailable,
    #[error("resident scene exceeds u32 addressing")]
    AddressSpaceExceeded,
    #[cfg_attr(
        feature = "diagnostic-resident-sh-mantissa8",
        error("resident SH plane count must be one of 0, 1, 2, or 3; got {0}")
    )]
    #[cfg_attr(
        not(feature = "diagnostic-resident-sh-mantissa8"),
        error("resident SH plane count must be one of 0, 1, 3, or 4; got {0}")
    )]
    UnsupportedShPlaneCount(u32),
    #[error(
        "resident resource {resource} requires {required_bytes} bytes but the effective binding limit is {limit_bytes} bytes"
    )]
    BindingLimitExceeded {
        resource: &'static str,
        required_bytes: u64,
        limit_bytes: u64,
    },
    #[error("resident color resolve needs eight compute storage buffers, device exposes {0}")]
    StorageBindingCountUnsupported(u32),
    #[error("resident dispatch exceeds device workgroup dimensions")]
    DispatchLimitExceeded,
    #[error("resident order upload exceeds scene capacity")]
    OrderCapacityExceeded,
    #[error("resident GPU ordering initialization failed: {0}")]
    GpuOrderInitialization(String),
    #[error("resident GPU ordering allocation failed: {0}")]
    GpuOrderOutOfMemory(String),
    #[error("resident GPU ordering validation failed: {0}")]
    GpuOrderValidation(String),
    #[error("resident GPU ordering backend failed: {0}")]
    GpuOrderInternal(String),
}
