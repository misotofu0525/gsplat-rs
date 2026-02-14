//! Core types shared by all gsplat-rs crates.

pub const GSPLAT_API_VERSION_MAJOR: u32 = 0;
pub const GSPLAT_API_VERSION_MINOR: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec3f {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3f {
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraPose {
    pub position: Vec3f,
    pub rotation_xyzw: [f32; 4],
}

impl Default for CameraPose {
    fn default() -> Self {
        Self {
            position: Vec3f::new(0.0, 0.0, 0.0),
            rotation_xyzw: [0.0, 0.0, 0.0, 1.0],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraIntrinsics {
    pub vertical_fov_radians: f32,
    pub near_plane: f32,
    pub far_plane: f32,
}

impl Default for CameraIntrinsics {
    fn default() -> Self {
        Self {
            vertical_fov_radians: 60.0_f32.to_radians(),
            near_plane: 0.01,
            far_plane: 1000.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Camera {
    pub pose: CameraPose,
    pub intrinsics: CameraIntrinsics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum RenderMode {
    SortedAlpha = 0,
    SortFree = 1,
}

impl Default for RenderMode {
    fn default() -> Self {
        Self::SortedAlpha
    }
}

impl RenderMode {
    pub const fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::SortedAlpha),
            1 => Some(Self::SortFree),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RendererConfig {
    pub width: u32,
    pub height: u32,
    pub mode: RenderMode,
}

impl Default for RendererConfig {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            mode: RenderMode::SortedAlpha,
        }
    }
}

impl RendererConfig {
    pub const fn validate(&self) -> Result<(), ErrorCode> {
        if self.width == 0 || self.height == 0 {
            return Err(ErrorCode::InvalidArgument);
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ErrorCode {
    Ok = 0,
    InvalidArgument = 1,
    NotFound = 2,
    ParseFailed = 3,
    Unsupported = 4,
    SceneNotLoaded = 5,
    Internal = 100,
}

impl ErrorCode {
    pub const fn as_i32(self) -> i32 {
        self as i32
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameStats {
    pub frame_ms: f32,
    pub preprocess_ms: f32,
    pub sort_ms: f32,
    pub raster_ms: f32,
    pub visible_count: u32,
    pub drawn_count: u32,
}

impl Default for FrameStats {
    fn default() -> Self {
        Self::zero()
    }
}

impl FrameStats {
    pub const fn zero() -> Self {
        Self {
            frame_ms: 0.0,
            preprocess_ms: 0.0,
            sort_ms: 0.0,
            raster_ms: 0.0,
            visible_count: 0,
            drawn_count: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SceneBuffers {
    pub positions: Vec<Vec3f>,
    pub opacity: Vec<f32>,
    pub scale_xyz: Vec<[f32; 3]>,
    pub rotation_xyzw: Vec<[f32; 4]>,
    pub color_dc: Vec<[f32; 3]>,
    /// Spherical harmonics degree for view-dependent color. `0` means DC-only.
    pub sh_degree: u8,
    /// Flattened SH coefficients excluding DC, in PLY `f_rest_*` order:
    /// `[R coeff1.., G coeff1.., B coeff1..]` per Gaussian.
    pub sh_rest: Option<Vec<f32>>,
}

impl SceneBuffers {
    pub fn len(&self) -> usize {
        self.positions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }

    pub fn validate(&self) -> Result<(), ErrorCode> {
        let n = self.positions.len();

        if self.opacity.len() != n
            || self.scale_xyz.len() != n
            || self.rotation_xyzw.len() != n
            || self.color_dc.len() != n
        {
            return Err(ErrorCode::ParseFailed);
        }

        if self.sh_degree > 4 {
            return Err(ErrorCode::Unsupported);
        }

        let coeff_total = (self.sh_degree as usize + 1).pow(2);
        let rest_coeffs_per_channel = coeff_total.saturating_sub(1);
        let rest_stride = rest_coeffs_per_channel * 3;

        match (&self.sh_rest, rest_stride) {
            (None, 0) => {}
            (None, _) => return Err(ErrorCode::ParseFailed),
            (Some(_), 0) => return Err(ErrorCode::ParseFailed),
            (Some(rest), stride) => {
                if rest.len() != n.saturating_mul(stride) {
                    return Err(ErrorCode::ParseFailed);
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{ErrorCode, RenderMode, RendererConfig, SceneBuffers};

    #[test]
    fn render_mode_from_u32() {
        assert_eq!(RenderMode::from_u32(0), Some(RenderMode::SortedAlpha));
        assert_eq!(RenderMode::from_u32(1), Some(RenderMode::SortFree));
        assert_eq!(RenderMode::from_u32(2), None);
    }

    #[test]
    fn renderer_config_validation_rejects_zero_dimension() {
        let config = RendererConfig {
            width: 0,
            height: 720,
            mode: RenderMode::SortedAlpha,
        };
        assert_eq!(config.validate(), Err(ErrorCode::InvalidArgument));
    }

    #[test]
    fn scene_buffer_validation_checks_lengths() {
        let scene = SceneBuffers {
            positions: vec![],
            opacity: vec![1.0],
            scale_xyz: vec![],
            rotation_xyzw: vec![],
            color_dc: vec![],
            sh_degree: 0,
            sh_rest: None,
        };

        assert_eq!(scene.validate(), Err(ErrorCode::ParseFailed));
    }

    #[test]
    fn scene_buffer_validation_rejects_nonzero_sh_degree_without_rest() {
        let scene = SceneBuffers {
            positions: vec![super::Vec3f::new(0.0, 0.0, 1.0)],
            opacity: vec![1.0],
            scale_xyz: vec![[1.0, 1.0, 1.0]],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]],
            color_dc: vec![[0.1, 0.2, 0.3]],
            sh_degree: 1,
            sh_rest: None,
        };

        assert_eq!(scene.validate(), Err(ErrorCode::ParseFailed));
    }
}
