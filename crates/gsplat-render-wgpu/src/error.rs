//! Renderer and Surface presenter errors.

use gsplat_core::ErrorCode;
use gsplat_sort::SortError;
use thiserror::Error;

use crate::resident::ResidentSceneError;

#[derive(Debug, Error)]
pub enum RendererError {
    #[error("invalid renderer configuration")]
    InvalidConfig,
    #[error("invalid camera")]
    InvalidCamera,
    #[error("scene not loaded")]
    SceneNotLoaded,
    #[error("invalid scene buffers")]
    InvalidScene,
    #[error("gpu rasterizer unavailable")]
    GpuRasterizerUnavailable,
    #[error("gpu device creation failed")]
    GpuDeviceCreation,
    #[error(
        "render dimensions {width}x{height} exceed the device 2D texture limit {max_dimension}"
    )]
    GpuDimensionsUnsupported {
        width: u32,
        height: u32,
        max_dimension: u32,
    },
    #[error("gpu readback failed")]
    GpuReadback,
    #[error("waiting for gpu completion failed")]
    GpuWait,
    #[error("surface background worker failed")]
    SurfaceWorker,
    #[error("resident scene resource error: {0}")]
    ResidentScene(#[from] ResidentSceneError),
    #[error("sort backend error: {0}")]
    Sort(#[from] SortError),
    #[error("surface presenter error: {0}")]
    SurfacePresenter(#[from] SurfacePresenterError),
}

impl RendererError {
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidConfig | Self::InvalidCamera | Self::InvalidScene => {
                ErrorCode::InvalidArgument
            }
            Self::SceneNotLoaded => ErrorCode::SceneNotLoaded,
            Self::GpuRasterizerUnavailable
            | Self::GpuDeviceCreation
            | Self::GpuDimensionsUnsupported { .. }
            | Self::ResidentScene(ResidentSceneError::ResourceLimitExceeded(_))
            | Self::ResidentScene(ResidentSceneError::ResourceSizeOverflow) => {
                ErrorCode::Unsupported
            }
            Self::GpuReadback | Self::GpuWait | Self::SurfaceWorker => ErrorCode::Internal,
            Self::ResidentScene(ResidentSceneError::SortedIndexCapacityExceeded)
            | Self::ResidentScene(ResidentSceneError::GpuOrderInitialization(_)) => {
                ErrorCode::Internal
            }
            Self::Sort(_) => ErrorCode::Internal,
            Self::SurfacePresenter(err) => err.code(),
        }
    }
}

#[derive(Debug, Error)]
pub enum SurfacePresenterError {
    #[error("invalid surface size")]
    InvalidSurfaceSize,
    #[error("surface creation failed")]
    SurfaceCreation,
    #[error("no compatible surface adapter")]
    NoAdapter,
    #[error("surface device creation failed: {0}")]
    DeviceCreation(String),
    #[error("surface has no compatible format")]
    NoSurfaceFormat,
    #[error("surface configure failed: {0}")]
    SurfaceConfigure(String),
    #[error("surface requires a loaded scene")]
    SceneNotLoaded,
    #[error("surface acquire failed: {0}")]
    SurfaceAcquire(String),
    #[error("surface out of memory")]
    SurfaceOutOfMemory,
    #[error("resident scene resource error: {0}")]
    ResidentScene(#[from] ResidentSceneError),
}

impl SurfacePresenterError {
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidSurfaceSize => ErrorCode::InvalidArgument,
            Self::SurfaceCreation | Self::NoAdapter | Self::DeviceCreation(_) => {
                ErrorCode::Unsupported
            }
            Self::SceneNotLoaded => ErrorCode::SceneNotLoaded,
            Self::NoSurfaceFormat
            | Self::SurfaceConfigure(_)
            | Self::SurfaceAcquire(_)
            | Self::SurfaceOutOfMemory => ErrorCode::Internal,
            Self::ResidentScene(ResidentSceneError::ResourceLimitExceeded(_))
            | Self::ResidentScene(ResidentSceneError::ResourceSizeOverflow) => {
                ErrorCode::Unsupported
            }
            Self::ResidentScene(ResidentSceneError::SortedIndexCapacityExceeded)
            | Self::ResidentScene(ResidentSceneError::GpuOrderInitialization(_)) => {
                ErrorCode::Internal
            }
        }
    }
}
