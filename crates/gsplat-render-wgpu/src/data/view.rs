//! Narrow borrowed and owned inputs for strategy-free data work.

#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use gsplat_core::Camera;
use gsplat_core::{SceneBuffers, Vec3f};

/// Borrowed AoS position input for exact CPU ordering kernels.
///
/// This view deliberately retains the canonical `Vec3f` storage. Platform
/// leaves may read that proven 12-byte record layout but must not create a
/// duplicate SoA position owner.
#[derive(Clone, Copy)]
pub(crate) struct CpuPositionView<'a> {
    positions: &'a [Vec3f],
}

impl<'a> CpuPositionView<'a> {
    pub(crate) const fn new(positions: &'a [Vec3f]) -> Self {
        Self { positions }
    }

    pub(crate) const fn as_slice(self) -> &'a [Vec3f] {
        self.positions
    }

    pub(crate) const fn len(self) -> usize {
        self.positions.len()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn slice(self, range: std::ops::Range<usize>) -> Self {
        Self::new(&self.positions[range])
    }
}

#[derive(Clone, Copy)]
pub(crate) struct CameraCovarianceTerms {
    pub(crate) xx: f32,
    pub(crate) xy: f32,
    pub(crate) xz: f32,
    pub(crate) yy: f32,
    pub(crate) yz: f32,
    pub(crate) zz: f32,
}

impl CameraCovarianceTerms {
    pub(crate) fn from_matrix(cov: [[f32; 3]; 3]) -> Self {
        Self {
            xx: cov[0][0],
            xy: cov[0][1],
            xz: cov[0][2],
            yy: cov[1][1],
            yz: cov[1][2],
            zz: cov[2][2],
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ShColorLayout<'a> {
    pub(crate) rest: Option<&'a [f32]>,
    pub(crate) degree: u8,
    pub(crate) per_channel: usize,
    pub(crate) stride: usize,
}

impl<'a> ShColorLayout<'a> {
    pub(crate) fn new(scene: &'a SceneBuffers) -> Self {
        let rest = scene.sh_rest.as_deref();
        let coeff_total = if rest.is_some() {
            (scene.sh_degree as usize + 1).pow(2)
        } else {
            1
        };
        let per_channel = coeff_total.saturating_sub(1);
        Self {
            rest,
            degree: if rest.is_some() { scene.sh_degree } else { 0 },
            per_channel,
            stride: per_channel * 3,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct SplatSetView<'a> {
    positions: &'a [Vec3f],
    color_dc: &'a [[f32; 3]],
    world_covariance_terms: &'a [CameraCovarianceTerms],
    alpha_values: &'a [f32],
}

impl<'a> SplatSetView<'a> {
    pub(crate) const fn new(
        positions: &'a [Vec3f],
        color_dc: &'a [[f32; 3]],
        world_covariance_terms: &'a [CameraCovarianceTerms],
        alpha_values: &'a [f32],
    ) -> Self {
        Self {
            positions,
            color_dc,
            world_covariance_terms,
            alpha_values,
        }
    }

    pub(crate) const fn positions(self) -> &'a [Vec3f] {
        self.positions
    }

    pub(crate) const fn color_dc(self) -> &'a [[f32; 3]] {
        self.color_dc
    }

    pub(crate) const fn world_covariance_terms(self) -> &'a [CameraCovarianceTerms] {
        self.world_covariance_terms
    }

    pub(crate) const fn alpha_values(self) -> &'a [f32] {
        self.alpha_values
    }

    pub(crate) const fn len(self) -> usize {
        self.positions.len()
    }

    pub(crate) const fn is_empty(self) -> bool {
        self.positions.is_empty()
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct OwnedCpuOrderInput {
    positions: Arc<[Vec3f]>,
    camera: Camera,
}

#[cfg(not(target_arch = "wasm32"))]
impl OwnedCpuOrderInput {
    pub(crate) const fn new(positions: Arc<[Vec3f]>, camera: Camera) -> Self {
        Self { positions, camera }
    }

    pub(crate) fn position_view(&self) -> CpuPositionView<'_> {
        CpuPositionView::new(&self.positions)
    }

    pub(crate) const fn camera(&self) -> Camera {
        self.camera
    }

    pub(crate) fn into_positions(self) -> Arc<[Vec3f]> {
        self.positions
    }
}
