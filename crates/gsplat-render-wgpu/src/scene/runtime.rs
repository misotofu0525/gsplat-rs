use gsplat_core::Vec3f;

use super::{ResidentSceneCpu, ResidentSceneError};

/// Private Exact, all-resident scene owner used by the shadow renderer core.
///
/// The resident value is consumed at construction. Callers receive only
/// borrowed views, so the shadow core cannot create a second positions array
/// or a second scene owner.
#[derive(Debug)]
pub(crate) struct SceneRuntime {
    resident: ResidentSceneCpu,
}

impl SceneRuntime {
    pub(crate) fn prepare(resident: ResidentSceneCpu) -> Result<Self, ResidentSceneError> {
        resident.validate_complete()?;
        Ok(Self { resident })
    }

    pub(crate) fn source_count(&self) -> usize {
        self.resident.len()
    }

    pub(crate) fn sh_degree(&self) -> u8 {
        self.resident.sh_degree
    }

    pub(crate) fn positions(&self) -> &[Vec3f] {
        &self.resident.positions
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn resident(&self) -> &ResidentSceneCpu {
        &self.resident
    }
}

#[cfg(test)]
mod tests {
    use gsplat_core::{SceneBuffers, Vec3f};

    use super::SceneRuntime;
    use crate::scene::ResidentSceneCpu;

    fn scene() -> SceneBuffers {
        SceneBuffers {
            positions: vec![Vec3f::new(1.0, 2.0, 3.0)],
            opacity: vec![0.0],
            scale_xyz: vec![[0.0; 3]],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]],
            color_dc: vec![[0.0; 3]],
            sh_degree: 0,
            sh_rest: None,
        }
    }

    #[test]
    fn owns_one_resident_scene_and_borrows_its_positions() {
        let resident = ResidentSceneCpu::encode_owned(scene()).expect("resident scene");
        let positions_ptr = resident.positions.as_ptr();

        let runtime = SceneRuntime::prepare(resident).expect("scene runtime");

        assert_eq!(runtime.positions().as_ptr(), positions_ptr);
        assert_eq!(runtime.source_count(), 1);
        assert_eq!(runtime.sh_degree(), 0);
        assert_eq!(runtime.resident().len(), 1);
    }

    #[test]
    fn rejects_an_incomplete_resident_scene() {
        let mut resident = ResidentSceneCpu::encode_owned(scene()).expect("resident scene");
        resident.sh_degree = 4;

        assert!(SceneRuntime::prepare(resident).is_err());
    }
}
