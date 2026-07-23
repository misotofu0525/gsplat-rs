use gsplat_core::{Camera, SceneBuffers, Vec3f};
use thiserror::Error;

use super::{PlanId, PreparedRuntimeSlot, execute_frame};
use crate::plans::{FrameIdentity, OrderLane, WorkUnavailable};
use crate::renderer::frame::Viewport;
use crate::scene::ResidentSceneCpu;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShadowCurrentnessError {
    FrameIdentity,
    CameraOrViewport,
    SceneContract,
    AuthoritativeOrder,
    WorkCounts,
    UnsupportedWork,
    GenerationExhausted,
}

#[derive(Debug, Error)]
pub(crate) enum ShadowFrameError {
    #[error("shadow frame execution failed: {0}")]
    Frame(#[from] super::FrameExecutionError),
    #[error("shadow work accessor failed: {0}")]
    Work(#[from] WorkUnavailable),
}

#[derive(Debug)]
pub(crate) struct ShadowFrame {
    plan_id: PlanId,
    order_lane: OrderLane,
    frame_identity: FrameIdentity,
    order_generation: u64,
    source_count: u32,
    sh_degree: u8,
    visible_count: Result<u32, WorkUnavailable>,
    contributor_count: Result<u32, WorkUnavailable>,
    draw_count: Result<u32, WorkUnavailable>,
    cpu_order_ids: Vec<u32>,
}

impl ShadowFrame {
    pub(crate) const fn plan_id(&self) -> PlanId {
        self.plan_id
    }

    pub(crate) const fn order_lane(&self) -> OrderLane {
        self.order_lane
    }

    pub(crate) const fn frame_identity(&self) -> FrameIdentity {
        self.frame_identity
    }

    pub(crate) const fn order_generation(&self) -> u64 {
        self.order_generation
    }

    pub(crate) const fn source_count(&self) -> u32 {
        self.source_count
    }

    pub(crate) const fn sh_degree(&self) -> u8 {
        self.sh_degree
    }

    pub(crate) const fn visible_count(&self) -> Result<u32, WorkUnavailable> {
        self.visible_count
    }

    pub(crate) const fn contributor_count(&self) -> Result<u32, WorkUnavailable> {
        self.contributor_count
    }

    pub(crate) const fn draw_count(&self) -> Result<u32, WorkUnavailable> {
        self.draw_count
    }

    pub(crate) fn cpu_order_ids(&self) -> &[u32] {
        &self.cpu_order_ids
    }
}

pub(crate) fn capture_shadow_frame(
    slot: &mut PreparedRuntimeSlot,
    requested: PlanId,
    camera: &Camera,
    viewport: Viewport,
) -> Result<ShadowFrame, ShadowFrameError> {
    let sh_degree = slot.scene().sh_degree();
    let frame = {
        let work = execute_frame(slot, requested, camera, viewport)?;
        ShadowFrame {
            plan_id: work.plan_id(),
            order_lane: work.order_lane(),
            frame_identity: work.frame_identity(),
            order_generation: work.order_generation(),
            source_count: work.source_count(),
            sh_degree,
            visible_count: work.visible_count(),
            contributor_count: work.contributor_count(),
            draw_count: work.draw_count(),
            cpu_order_ids: work.cpu_order_ids()?.to_vec(),
        }
    };
    Ok(frame)
}

pub(crate) fn validate_current_shadow_frame(
    slot: &PreparedRuntimeSlot,
    frame: &ShadowFrame,
    camera: &Camera,
    viewport: Viewport,
) -> Result<(), ShadowCurrentnessError> {
    if slot.frame_state().identity() != frame.frame_identity {
        return Err(ShadowCurrentnessError::FrameIdentity);
    }
    let candidate = slot
        .frame_state()
        .candidate_for_frame(*camera, viewport)
        .map_err(|_| ShadowCurrentnessError::GenerationExhausted)?
        .identity();
    if candidate != frame.frame_identity {
        return Err(ShadowCurrentnessError::CameraOrViewport);
    }

    let source_count = u32::try_from(slot.scene().source_count())
        .map_err(|_| ShadowCurrentnessError::SceneContract)?;
    if source_count != frame.source_count || slot.scene().sh_degree() != frame.sh_degree {
        return Err(ShadowCurrentnessError::SceneContract);
    }
    if frame.plan_id != PlanId::CpuPostSort || frame.order_lane != OrderLane::Cpu {
        return Err(ShadowCurrentnessError::UnsupportedWork);
    }
    if slot.last_usable_cpu_order() != Some(frame.cpu_order_ids()) {
        return Err(ShadowCurrentnessError::AuthoritativeOrder);
    }
    if frame.visible_count != Ok(frame.cpu_order_ids.len() as u32)
        || frame.contributor_count != Err(WorkUnavailable::ContributorCount)
        || frame.draw_count != Err(WorkUnavailable::DrawCount)
    {
        return Err(ShadowCurrentnessError::WorkCounts);
    }
    Ok(())
}

fn resident(depths: &[f32]) -> ResidentSceneCpu {
    let count = depths.len();
    ResidentSceneCpu::encode_owned(SceneBuffers {
        positions: depths
            .iter()
            .copied()
            .map(|z| Vec3f::new(0.0, 0.0, z))
            .collect(),
        opacity: vec![0.0; count],
        scale_xyz: vec![[-3.0; 3]; count],
        rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; count],
        color_dc: vec![[0.0; 3]; count],
        sh_degree: 0,
        sh_rest: None,
    })
    .expect("resident scene")
}

fn frame_receipt(
    slot: &mut PreparedRuntimeSlot,
    camera: &Camera,
    viewport: Viewport,
) -> (crate::plans::FrameIdentity, u64, Vec<u32>) {
    let work = execute_frame(slot, PlanId::CpuPostSort, camera, viewport).expect("CPU frame");
    (
        work.frame_identity(),
        work.order_generation(),
        work.cpu_order_ids().expect("CPU IDs").to_vec(),
    )
}

#[test]
fn failed_replacement_preserves_runtime_generations_fallback_and_order() {
    let mut slot =
        PreparedRuntimeSlot::prepare(resident(&[1.0, 3.0, 2.0])).expect("prepared runtime");
    let (_, _, old_order) = frame_receipt(
        &mut slot,
        &Camera::default(),
        Viewport::new(640, 480).expect("viewport"),
    );
    let old_frame = slot.frame_state();
    let old_fallback = slot.fallback();
    let old_scene_positions = slot.scene().positions().as_ptr();

    let mut invalid = resident(&[9.0]);
    invalid.sh_degree = 4;
    let result = slot.replace(invalid);

    assert!(result.is_err());
    assert_eq!(slot.frame_state(), old_frame);
    assert_eq!(slot.fallback(), old_fallback);
    assert_eq!(slot.eligible(), [PlanId::CpuPostSort]);
    assert_eq!(slot.scene().positions().as_ptr(), old_scene_positions);
    assert_eq!(slot.last_usable_cpu_order(), Some(old_order.as_slice()));
}

#[test]
fn successful_replacement_publishes_one_fresh_runtime() {
    let mut slot = PreparedRuntimeSlot::prepare(resident(&[1.0])).expect("prepared runtime");
    let old_frame = slot.frame_state().identity();

    slot.replace(resident(&[2.0, 3.0]))
        .expect("runtime replacement");

    let new_frame = slot.frame_state().identity();
    assert_eq!(
        new_frame.scene_generation(),
        old_frame.scene_generation() + 1
    );
    assert_eq!(
        new_frame.contract_generation(),
        old_frame.contract_generation() + 1
    );
    assert_eq!(
        new_frame.plan_set_generation(),
        old_frame.plan_set_generation() + 1
    );
    assert_eq!(slot.scene().source_count(), 2);
    assert_eq!(slot.last_usable_cpu_order(), None);
}

#[test]
fn complete_camera_and_viewport_identity_reuses_or_invalidates_exactly() {
    let mut slot =
        PreparedRuntimeSlot::prepare(resident(&[1.0, 3.0, 2.0])).expect("prepared runtime");
    let viewport = Viewport::new(640, 480).expect("viewport");
    let (first, first_order_generation, _) = frame_receipt(&mut slot, &Camera::default(), viewport);
    let (unchanged, unchanged_order_generation, _) =
        frame_receipt(&mut slot, &Camera::default(), viewport);

    let mut positioned = Camera::default();
    positioned.pose.position.z = -1.0;
    let (position_identity, position_order_generation, _) =
        frame_receipt(&mut slot, &positioned, viewport);

    let half_angle = 0.125_f32;
    let mut rotated = positioned;
    rotated.pose.rotation_xyzw = [0.0, half_angle.sin(), 0.0, half_angle.cos()];
    let (rotation_identity, rotation_order_generation, _) =
        frame_receipt(&mut slot, &rotated, viewport);

    let mut changed_intrinsics = rotated;
    changed_intrinsics.intrinsics.vertical_fov_radians *= 0.9;
    let (intrinsics_identity, intrinsics_order_generation, _) =
        frame_receipt(&mut slot, &changed_intrinsics, viewport);

    let (resized_identity, resized_order_generation, _) = frame_receipt(
        &mut slot,
        &changed_intrinsics,
        Viewport::new(800, 600).expect("resized viewport"),
    );

    assert_eq!(unchanged, first);
    assert_eq!(unchanged_order_generation, first_order_generation);
    assert_eq!(
        position_identity.camera_revision(),
        first.camera_revision() + 1
    );
    assert_eq!(position_order_generation, first_order_generation + 1);
    assert_eq!(
        rotation_identity.camera_revision(),
        position_identity.camera_revision() + 1
    );
    assert_eq!(rotation_order_generation, position_order_generation + 1);
    assert_eq!(
        intrinsics_identity.camera_revision(),
        rotation_identity.camera_revision() + 1
    );
    assert_eq!(intrinsics_order_generation, rotation_order_generation + 1);
    assert_eq!(
        resized_identity.viewport_generation(),
        intrinsics_identity.viewport_generation() + 1
    );
    assert_eq!(
        resized_identity.camera_revision(),
        intrinsics_identity.camera_revision()
    );
    assert_eq!(resized_order_generation, intrinsics_order_generation);
}

#[test]
fn failed_frame_keeps_identity_fallback_and_last_usable_order() {
    let mut slot =
        PreparedRuntimeSlot::prepare(resident(&[1.0, 3.0, 2.0])).expect("prepared runtime");
    let viewport = Viewport::new(640, 480).expect("viewport");
    let (_, old_order_generation, old_order) =
        frame_receipt(&mut slot, &Camera::default(), viewport);
    let old_frame = slot.frame_state();
    let old_fallback = slot.fallback();

    let mut invalid_camera = Camera::default();
    invalid_camera.intrinsics.near_plane = 2.0;
    invalid_camera.intrinsics.far_plane = 1.0;
    assert!(
        execute_frame(
            &mut slot,
            PlanId::CpuPostSort,
            &invalid_camera,
            Viewport::new(800, 600).expect("candidate viewport"),
        )
        .is_err()
    );
    assert!(execute_frame(&mut slot, PlanId::GpuPostSort, &Camera::default(), viewport,).is_err());
    assert_eq!(slot.frame_state(), old_frame);
    assert_eq!(slot.fallback(), old_fallback);
    assert_eq!(slot.last_usable_cpu_order(), Some(old_order.as_slice()));
    let (_, reused_order_generation, reused_order) =
        frame_receipt(&mut slot, &Camera::default(), viewport);
    assert_eq!(reused_order_generation, old_order_generation);
    assert_eq!(reused_order, old_order);
}
