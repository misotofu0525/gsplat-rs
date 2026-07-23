//! Closed Exact plan set and the private plan-to-renderer handoff.

mod cpu_post;

use gsplat_core::Camera;
use thiserror::Error;

use crate::scene::SceneRuntime;

use cpu_post::{CpuPostSortError, CpuPostSortPlan};

/// Closed identities for complete Exact execution plans.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlanId {
    CpuPostSort,
    GpuPostSort,
    GpuPreproject,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OrderLane {
    Cpu,
    Gpu,
}

/// Immutable generation identity copied from the sole renderer-owned
/// [`crate::renderer::frame::FrameState`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrameIdentity {
    scene_generation: u64,
    camera_revision: u64,
    viewport_generation: u64,
    contract_generation: u64,
    plan_set_generation: u64,
}

impl FrameIdentity {
    pub(crate) const fn new(
        scene_generation: u64,
        camera_revision: u64,
        viewport_generation: u64,
        contract_generation: u64,
        plan_set_generation: u64,
    ) -> Self {
        Self {
            scene_generation,
            camera_revision,
            viewport_generation,
            contract_generation,
            plan_set_generation,
        }
    }

    pub(crate) const fn scene_generation(self) -> u64 {
        self.scene_generation
    }

    pub(crate) const fn camera_revision(self) -> u64 {
        self.camera_revision
    }

    pub(crate) const fn viewport_generation(self) -> u64 {
        self.viewport_generation
    }

    pub(crate) const fn contract_generation(self) -> u64 {
        self.contract_generation
    }

    pub(crate) const fn plan_set_generation(self) -> u64 {
        self.plan_set_generation
    }
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkUnavailable {
    #[error("CPU order IDs are unavailable for this projected work")]
    CpuOrderIds,
    #[error("host-visible candidate count is not available")]
    VisibleCount,
    #[error("post-projection contributor count is not available")]
    ContributorCount,
    #[error("issued draw count is not available")]
    DrawCount,
}

/// Private, accessor-only plan handoff.
///
/// E1 stops after the authoritative CPU order and therefore reports only
/// host-known S/V. V, C and D have independent availability so a future GPU
/// plan cannot substitute capacity or zero while an indirect count remains
/// unresolved. The fields stay private so common tests and renderer code never
/// exhaustively match plan-internal CPU/GPU variants.
#[allow(dead_code)]
pub(crate) struct ProjectedWork<'a> {
    plan: PlanId,
    order_lane: OrderLane,
    frame: FrameIdentity,
    order_generation: u64,
    source_count: u32,
    visible_count: Option<u32>,
    contributor_count: Option<u32>,
    draw_count: Option<u32>,
    cpu_order_ids: Option<&'a [u32]>,
}

#[allow(dead_code)]
impl<'a> ProjectedWork<'a> {
    fn from_cpu_post_sort(
        frame: FrameIdentity,
        order_generation: u64,
        source_count: u32,
        ordered_ids: &'a [u32],
    ) -> Self {
        Self {
            plan: PlanId::CpuPostSort,
            order_lane: OrderLane::Cpu,
            frame,
            order_generation,
            source_count,
            visible_count: Some(ordered_ids.len() as u32),
            contributor_count: None,
            draw_count: None,
            cpu_order_ids: Some(ordered_ids),
        }
    }

    pub(crate) const fn plan_id(&self) -> PlanId {
        self.plan
    }

    pub(crate) const fn order_lane(&self) -> OrderLane {
        self.order_lane
    }

    pub(crate) const fn frame_identity(&self) -> FrameIdentity {
        self.frame
    }

    pub(crate) const fn order_generation(&self) -> u64 {
        self.order_generation
    }

    pub(crate) const fn source_count(&self) -> u32 {
        self.source_count
    }

    pub(crate) fn visible_count(&self) -> Result<u32, WorkUnavailable> {
        self.visible_count.ok_or(WorkUnavailable::VisibleCount)
    }

    pub(crate) fn contributor_count(&self) -> Result<u32, WorkUnavailable> {
        self.contributor_count
            .ok_or(WorkUnavailable::ContributorCount)
    }

    pub(crate) fn draw_count(&self) -> Result<u32, WorkUnavailable> {
        self.draw_count.ok_or(WorkUnavailable::DrawCount)
    }

    pub(crate) fn cpu_order_ids(&self) -> Result<&[u32], WorkUnavailable> {
        self.cpu_order_ids.ok_or(WorkUnavailable::CpuOrderIds)
    }
}

#[derive(Debug, Error)]
pub(crate) enum PlanSetError {
    #[error("Exact PlanSet must contain at least one eligible plan")]
    Empty,
    #[error("eligible plan {plan:?} has no prepared concrete entry")]
    EligiblePlanUnprepared { plan: PlanId },
    #[error("eligible plan {plan:?} is duplicated")]
    DuplicateEligiblePlan { plan: PlanId },
    #[error("fallback plan {fallback:?} has no prepared concrete entry")]
    FallbackUnprepared { fallback: PlanId },
    #[error("fallback plan {fallback:?} is not eligible")]
    FallbackIneligible { fallback: PlanId },
    #[error("requested plan {requested:?} has no prepared concrete entry")]
    RequestedPlanUnprepared { requested: PlanId },
    #[error("requested prepared plan {requested:?} is not eligible")]
    RequestedPlanIneligible { requested: PlanId },
    #[error("CPU PostSort failed: {0}")]
    CpuPostSort(#[from] CpuPostSortError),
}

/// Validated, immutable-membership set of prepared Exact plans.
pub(crate) struct PlanSet {
    fallback: PlanId,
    eligible: Box<[PlanId]>,
    cpu_post: Option<CpuPostSortPlan>,
}

impl PlanSet {
    pub(crate) fn prepare_cpu(source_count: usize) -> Result<Self, PlanSetError> {
        let cpu_post = CpuPostSortPlan::prepare(source_count)?;
        Self::try_new(
            Some(cpu_post),
            vec![PlanId::CpuPostSort].into_boxed_slice(),
            PlanId::CpuPostSort,
        )
    }

    fn try_new(
        cpu_post: Option<CpuPostSortPlan>,
        eligible: Box<[PlanId]>,
        fallback: PlanId,
    ) -> Result<Self, PlanSetError> {
        let candidate = Self {
            fallback,
            eligible,
            cpu_post,
        };
        candidate.validate()?;
        Ok(candidate)
    }

    fn validate(&self) -> Result<(), PlanSetError> {
        if self.eligible.is_empty() {
            return Err(PlanSetError::Empty);
        }
        for (index, plan) in self.eligible.iter().copied().enumerate() {
            if self.eligible[..index].contains(&plan) {
                return Err(PlanSetError::DuplicateEligiblePlan { plan });
            }
            if !self.is_prepared(plan) {
                return Err(PlanSetError::EligiblePlanUnprepared { plan });
            }
        }
        if !self.is_prepared(self.fallback) {
            return Err(PlanSetError::FallbackUnprepared {
                fallback: self.fallback,
            });
        }
        if !self.eligible.contains(&self.fallback) {
            return Err(PlanSetError::FallbackIneligible {
                fallback: self.fallback,
            });
        }
        Ok(())
    }

    fn is_prepared(&self, plan: PlanId) -> bool {
        match plan {
            PlanId::CpuPostSort => self.cpu_post.is_some(),
            PlanId::GpuPostSort | PlanId::GpuPreproject => false,
        }
    }

    pub(crate) fn execute<'a>(
        &'a mut self,
        requested: PlanId,
        scene: &SceneRuntime,
        camera: &Camera,
        frame: FrameIdentity,
        source_count: u32,
    ) -> Result<ProjectedWork<'a>, PlanSetError> {
        if !self.is_prepared(requested) {
            return Err(PlanSetError::RequestedPlanUnprepared { requested });
        }
        if !self.eligible.contains(&requested) {
            return Err(PlanSetError::RequestedPlanIneligible { requested });
        }
        match requested {
            PlanId::CpuPostSort => self
                .cpu_post
                .as_mut()
                .ok_or(PlanSetError::RequestedPlanUnprepared { requested })?
                .execute(scene, camera, frame, source_count)
                .map_err(PlanSetError::from),
            PlanId::GpuPostSort | PlanId::GpuPreproject => {
                Err(PlanSetError::RequestedPlanUnprepared { requested })
            }
        }
    }

    pub(crate) const fn fallback(&self) -> PlanId {
        self.fallback
    }

    pub(crate) fn eligible(&self) -> &[PlanId] {
        &self.eligible
    }

    pub(crate) fn last_usable_cpu_order(&self) -> Option<&[u32]> {
        self.cpu_post
            .as_ref()
            .and_then(CpuPostSortPlan::last_usable_order)
    }
}

#[cfg(test)]
mod tests {
    use gsplat_core::{Camera, SceneBuffers};

    use super::{CpuPostSortPlan, FrameIdentity, PlanId, PlanSet, PlanSetError, WorkUnavailable};
    use crate::scene::{ResidentSceneCpu, SceneRuntime};

    fn empty_scene() -> SceneRuntime {
        let resident = ResidentSceneCpu::encode_owned(SceneBuffers {
            positions: Vec::new(),
            opacity: Vec::new(),
            scale_xyz: Vec::new(),
            rotation_xyzw: Vec::new(),
            color_dc: Vec::new(),
            sh_degree: 0,
            sh_rest: None,
        })
        .expect("resident scene");
        SceneRuntime::prepare(resident).expect("scene runtime")
    }

    fn cpu_plan() -> CpuPostSortPlan {
        CpuPostSortPlan::prepare(0).expect("CPU plan")
    }

    #[test]
    fn empty_plan_set_fails_closed() {
        assert!(matches!(
            PlanSet::try_new(None, Vec::new().into_boxed_slice(), PlanId::CpuPostSort),
            Err(PlanSetError::Empty)
        ));
    }

    #[test]
    fn missing_fallback_fails_closed() {
        assert!(matches!(
            PlanSet::try_new(
                Some(cpu_plan()),
                vec![PlanId::CpuPostSort].into_boxed_slice(),
                PlanId::GpuPostSort,
            ),
            Err(PlanSetError::FallbackUnprepared {
                fallback: PlanId::GpuPostSort
            })
        ));
    }

    #[test]
    fn request_for_unprepared_plan_fails_closed() {
        let mut plans = PlanSet::prepare_cpu(0).expect("plan set");
        let scene = empty_scene();
        let frame = FrameIdentity::new(1, 1, 1, 1, 1);

        assert!(matches!(
            plans.execute(PlanId::GpuPostSort, &scene, &Camera::default(), frame, 0,),
            Err(PlanSetError::RequestedPlanUnprepared {
                requested: PlanId::GpuPostSort
            })
        ));
        assert!(matches!(
            plans.execute(PlanId::GpuPreproject, &scene, &Camera::default(), frame, 0,),
            Err(PlanSetError::RequestedPlanUnprepared {
                requested: PlanId::GpuPreproject
            })
        ));
        assert_eq!(plans.fallback(), PlanId::CpuPostSort);
        assert_eq!(plans.eligible(), [PlanId::CpuPostSort]);
    }

    #[test]
    fn cpu_handoff_reports_only_source_and_visible_counts() {
        let mut plans = PlanSet::prepare_cpu(0).expect("plan set");
        let scene = empty_scene();
        let work = plans
            .execute(
                PlanId::CpuPostSort,
                &scene,
                &Camera::default(),
                FrameIdentity::new(1, 1, 1, 1, 1),
                0,
            )
            .expect("CPU work");

        assert_eq!(work.source_count(), 0);
        assert_eq!(work.visible_count(), Ok(0));
        assert!(work.cpu_order_ids().expect("CPU IDs").is_empty());
        assert_eq!(
            work.contributor_count(),
            Err(WorkUnavailable::ContributorCount)
        );
        assert_eq!(work.draw_count(), Err(WorkUnavailable::DrawCount));
    }
}
