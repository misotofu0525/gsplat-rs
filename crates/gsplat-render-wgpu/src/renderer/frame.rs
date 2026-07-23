use gsplat_core::Camera;
use thiserror::Error;

use crate::plans::FrameIdentity;

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GenerationError {
    #[error("{generation} generation is exhausted")]
    Exhausted { generation: &'static str },
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ViewportError {
    #[error("shadow renderer viewport must be non-zero")]
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Viewport {
    width: u32,
    height: u32,
}

impl Viewport {
    pub(crate) const fn new(width: u32, height: u32) -> Result<Self, ViewportError> {
        if width == 0 || height == 0 {
            return Err(ViewportError::Invalid);
        }
        Ok(Self { width, height })
    }

    pub(crate) const fn width(self) -> u32 {
        self.width
    }

    pub(crate) const fn height(self) -> u32 {
        self.height
    }
}

/// Sole semantic generation and complete input-identity owner for the shadow
/// renderer core.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FrameState {
    scene_generation: u64,
    camera_revision: u64,
    viewport_generation: u64,
    contract_generation: u64,
    plan_set_generation: u64,
    camera: Option<Camera>,
    viewport: Option<Viewport>,
}

impl FrameState {
    pub(crate) const fn initial() -> Self {
        Self {
            scene_generation: 1,
            camera_revision: 0,
            viewport_generation: 0,
            contract_generation: 1,
            plan_set_generation: 1,
            camera: None,
            viewport: None,
        }
    }

    pub(crate) const fn identity(self) -> FrameIdentity {
        FrameIdentity::new(
            self.scene_generation,
            self.camera_revision,
            self.viewport_generation,
            self.contract_generation,
            self.plan_set_generation,
        )
    }

    pub(crate) fn after_runtime_replacement(self) -> Result<Self, GenerationError> {
        Ok(Self {
            scene_generation: increment(self.scene_generation, "scene")?,
            camera_revision: self.camera_revision,
            viewport_generation: self.viewport_generation,
            contract_generation: increment(self.contract_generation, "contract")?,
            plan_set_generation: increment(self.plan_set_generation, "plan set")?,
            camera: self.camera,
            viewport: self.viewport,
        })
    }

    /// Computes the next frame identity without mutating the published state.
    /// The caller publishes this value only after plan execution succeeds.
    pub(crate) fn candidate_for_frame(
        self,
        camera: Camera,
        viewport: Viewport,
    ) -> Result<Self, GenerationError> {
        let camera_changed = self.camera != Some(camera);
        let viewport_changed = self.viewport != Some(viewport);
        Ok(Self {
            scene_generation: self.scene_generation,
            camera_revision: if camera_changed {
                increment(self.camera_revision, "camera")?
            } else {
                self.camera_revision
            },
            viewport_generation: if viewport_changed {
                increment(self.viewport_generation, "viewport")?
            } else {
                self.viewport_generation
            },
            contract_generation: self.contract_generation,
            plan_set_generation: self.plan_set_generation,
            camera: Some(camera),
            viewport: Some(viewport),
        })
    }
}

fn increment(value: u64, generation: &'static str) -> Result<u64, GenerationError> {
    value
        .checked_add(1)
        .ok_or(GenerationError::Exhausted { generation })
}

#[cfg(test)]
mod tests {
    use gsplat_core::Camera;

    use super::{FrameState, Viewport};

    #[test]
    fn runtime_replacement_advances_only_runtime_generations() {
        let initial = FrameState::initial();
        let next = initial
            .after_runtime_replacement()
            .expect("next generations");

        assert_eq!(next.identity().scene_generation(), 2);
        assert_eq!(next.identity().contract_generation(), 2);
        assert_eq!(next.identity().plan_set_generation(), 2);
        assert_eq!(next.identity().camera_revision(), 0);
        assert_eq!(next.identity().viewport_generation(), 0);
    }

    #[test]
    fn frame_candidate_binds_complete_camera_and_viewport_values() {
        let viewport = Viewport::new(640, 480).expect("viewport");
        assert_eq!((viewport.width(), viewport.height()), (640, 480));
        let first = FrameState::initial()
            .candidate_for_frame(Camera::default(), viewport)
            .expect("first frame");
        let unchanged = first
            .candidate_for_frame(Camera::default(), viewport)
            .expect("unchanged frame");
        let mut moved_camera = Camera::default();
        moved_camera.pose.position.x = 1.0;
        let moved = unchanged
            .candidate_for_frame(moved_camera, viewport)
            .expect("moved frame");
        let resized = moved
            .candidate_for_frame(
                moved_camera,
                Viewport::new(800, 600).expect("resized viewport"),
            )
            .expect("resized frame");

        assert_eq!(first.identity().camera_revision(), 1);
        assert_eq!(first.identity().viewport_generation(), 1);
        assert_eq!(unchanged.identity(), first.identity());
        assert_eq!(moved.identity().camera_revision(), 2);
        assert_eq!(moved.identity().viewport_generation(), 1);
        assert_eq!(resized.identity().camera_revision(), 2);
        assert_eq!(resized.identity().viewport_generation(), 2);
    }
}
