//! One-attempt Surface frame execution for [`crate::SurfaceRenderSession`].
//!
//! This owner is intentionally stateless. It borrows the renderer and the S1
//! Surface owner only long enough to prepare, encode, submit and present one
//! attempt, then returns an unpublished candidate to the Session facade.

use gsplat_core::{Camera, FrameStats};

use super::{SessionSurfaceOwner, SurfaceFramePlan};
use crate::gpu_telemetry::TelemetrySubmission;
use crate::surface::CpuCompletionSampleRequest;
use crate::surface::shadow::{SurfaceExactError, SurfaceExactFrameResult};
use crate::{
    GeometryPath, Renderer, RendererError, SurfaceGpuOrderProducer, SurfacePresenterError,
    SurfaceProjectedDrawExecution, SurfaceRasterExecutionPlan, timer_elapsed_ms, timer_now,
};

/// Result of one fallible frame attempt. Only `Presented` may enter the
/// Session's publication closure; an unavailable drawable returns its
/// unpublished candidate to the caller.
#[must_use = "a frame attempt must be committed or returned as unavailable"]
pub(crate) enum SessionFrameAttempt<Presented, Unavailable = Presented> {
    Presented(Presented),
    Unavailable(Unavailable),
}

impl<Presented, Unavailable> SessionFrameAttempt<Presented, Unavailable> {
    pub(crate) fn commit<Output>(
        self,
        commit_presented: impl FnOnce(Presented) -> Output,
    ) -> Result<Output, Unavailable> {
        match self {
            Self::Presented(candidate) => Ok(commit_presented(candidate)),
            Self::Unavailable(candidate) => Err(candidate),
        }
    }
}

pub(crate) struct StandaloneGpuFrameAttempt {
    pub(crate) presenter_submission: TelemetrySubmission,
    pub(crate) projected_draw_execution: SurfaceProjectedDrawExecution,
    pub(crate) projected_draw_submission: TelemetrySubmission,
    pub(crate) gpu_order_producer: Option<SurfaceGpuOrderProducer>,
    pub(crate) gpu_producer_submission: TelemetrySubmission,
    pub(crate) raster_execution_plan: SurfaceRasterExecutionPlan,
    pub(crate) gpu_timestamp_queries_enabled: bool,
    pub(crate) render_submit_ms: f32,
    pub(crate) frame_wall_ms: f32,
}

pub(crate) struct StandaloneCpuFrameAttempt {
    pub(crate) stats: FrameStats,
    pub(crate) presenter_submission: TelemetrySubmission,
    pub(crate) projected_draw_execution: SurfaceProjectedDrawExecution,
    pub(crate) projected_draw_submission: TelemetrySubmission,
    pub(crate) raster_execution_plan: SurfaceRasterExecutionPlan,
    pub(crate) gpu_timestamp_queries_enabled: bool,
    pub(crate) paged: bool,
    pub(crate) render_submit_ms: f32,
    pub(crate) frame_wall_ms: f32,
}

/// Zero-sized executor: all durable state remains in the existing owners.
pub(crate) struct SessionFrameExecutor;

impl SessionFrameExecutor {
    pub(crate) fn attempt_exact(
        runtime: &mut crate::renderer::PreparedRuntimeSlot,
        surface: &mut SessionSurfaceOwner,
        camera: &Camera,
        force_cpu_order_refresh: bool,
        frame_started: crate::TimerInstant,
    ) -> Result<SessionFrameAttempt<SurfaceExactFrameResult, ()>, SurfaceExactError> {
        let rendered = match surface {
            SessionSurfaceOwner::Standalone(_) => {
                unreachable!("Exact Surface render requires the Packed host owner")
            }
            SessionSurfaceOwner::ExactPacked(host) => {
                host.render_exact_frame(runtime, camera, force_cpu_order_refresh, frame_started)?
            }
            #[cfg(test)]
            SessionSurfaceOwner::Test(_) => {
                unreachable!("test Direct owner cannot execute Exact")
            }
        };
        Ok(match rendered {
            Some(rendered) => SessionFrameAttempt::Presented(rendered),
            None => SessionFrameAttempt::Unavailable(()),
        })
    }

    pub(crate) fn attempt_direct_gpu(
        surface: &mut SessionSurfaceOwner,
        camera: &Camera,
        refresh_order: bool,
        camera_revision: u64,
        frame_started: crate::TimerInstant,
    ) -> Result<SessionFrameAttempt<StandaloneGpuFrameAttempt>, SurfacePresenterError> {
        let render_started = timer_now();
        let presenter_submission = match surface {
            SessionSurfaceOwner::Standalone(presenter) => presenter.render_direct_gpu_order(
                camera,
                refresh_order,
                camera_revision,
                frame_started,
            )?,
            SessionSurfaceOwner::ExactPacked(_) => {
                return Err(SurfacePresenterError::GpuOrderUnsupported);
            }
            #[cfg(test)]
            SessionSurfaceOwner::Test(_) => {
                return Err(SurfacePresenterError::GpuOrderUnsupported);
            }
        };
        let frame_presented = surface.last_frame_presented();
        let candidate = StandaloneGpuFrameAttempt {
            presenter_submission,
            projected_draw_execution: SurfaceProjectedDrawExecution::Candidate,
            projected_draw_submission: TelemetrySubmission::NotRequested,
            gpu_order_producer: None,
            gpu_producer_submission: TelemetrySubmission::NotRequested,
            raster_execution_plan: surface.raster_execution_plan(),
            gpu_timestamp_queries_enabled: match surface {
                SessionSurfaceOwner::Standalone(presenter) => {
                    presenter.gpu_order_timestamps_enabled()
                }
                SessionSurfaceOwner::ExactPacked(host) => host.gpu_order_timestamps_enabled(),
                #[cfg(test)]
                SessionSurfaceOwner::Test(_) => false,
            },
            render_submit_ms: timer_elapsed_ms(render_started),
            frame_wall_ms: timer_elapsed_ms(frame_started),
        };
        Ok(if frame_presented {
            SessionFrameAttempt::Presented(candidate)
        } else {
            SessionFrameAttempt::Unavailable(candidate)
        })
    }

    pub(crate) fn attempt_cpu_or_paged(
        renderer: &mut Renderer,
        surface: &mut SessionSurfaceOwner,
        camera: &Camera,
        camera_revision: u64,
        frame_started: crate::TimerInstant,
        frame: SurfaceFramePlan,
        track_cpu_completion: bool,
    ) -> Result<SessionFrameAttempt<StandaloneCpuFrameAttempt>, RendererError> {
        let paged = renderer.geometry_path() == GeometryPath::PagedActiveAtlas;
        let mut stats = if paged {
            FrameStats::zero()
        } else {
            renderer.prepare_surface_sorted_indices_attempt(camera, frame.refresh_sort)?
        };
        #[cfg(test)]
        if let Some(frame_presented) = surface.take_test_frame_presented() {
            let frame_wall_ms = timer_elapsed_ms(frame_started);
            stats.frame_ms = frame_wall_ms;
            renderer.stage_surface_attempt_stats(stats);
            let candidate = StandaloneCpuFrameAttempt {
                stats,
                presenter_submission: TelemetrySubmission::NotRequested,
                projected_draw_execution: SurfaceProjectedDrawExecution::Candidate,
                projected_draw_submission: TelemetrySubmission::NotRequested,
                raster_execution_plan: surface.raster_execution_plan(),
                gpu_timestamp_queries_enabled: false,
                paged,
                render_submit_ms: 0.0,
                frame_wall_ms,
            };
            return Ok(if frame_presented {
                SessionFrameAttempt::Presented(candidate)
            } else {
                SessionFrameAttempt::Unavailable(candidate)
            });
        }
        let render_started = timer_now();
        let presenter_submission = match surface {
            SessionSurfaceOwner::Standalone(presenter) if paged => {
                let scene = renderer.scene().ok_or(RendererError::SceneNotLoaded)?;
                presenter.render_sorted_indices(scene, &[], camera, true)?;
                let (visible_count, drawn_count) =
                    paged_surface_counts(scene.len(), presenter.instance_count());
                stats.visible_count = visible_count;
                stats.drawn_count = drawn_count;
                TelemetrySubmission::NotRequested
            }
            SessionSurfaceOwner::Standalone(presenter) => {
                let completion = track_cpu_completion.then_some(CpuCompletionSampleRequest {
                    camera_revision,
                    started: frame_started,
                    preprocess_ms: stats.preprocess_ms,
                    sort_ms: stats.sort_ms,
                });
                presenter.render_cpu_sorted_indices_tracked(
                    renderer.surface_sorted_indices_for_attempt(),
                    camera,
                    frame.upload_order,
                    completion,
                )?
            }
            SessionSurfaceOwner::ExactPacked(_) => {
                return Err(SurfacePresenterError::SurfaceGeometrySwitchUnsupported.into());
            }
            #[cfg(test)]
            SessionSurfaceOwner::Test(_) => unreachable!("test owner returned above"),
        };
        let frame_presented = surface.last_frame_presented();
        let frame_wall_ms = timer_elapsed_ms(frame_started);
        stats.frame_ms = frame_wall_ms;
        renderer.stage_surface_attempt_stats(stats);
        let candidate = StandaloneCpuFrameAttempt {
            stats,
            presenter_submission,
            projected_draw_execution: SurfaceProjectedDrawExecution::Candidate,
            projected_draw_submission: TelemetrySubmission::NotRequested,
            raster_execution_plan: surface.raster_execution_plan(),
            gpu_timestamp_queries_enabled: match surface {
                SessionSurfaceOwner::Standalone(presenter) => {
                    presenter.gpu_order_timestamps_enabled()
                }
                SessionSurfaceOwner::ExactPacked(host) => host.gpu_order_timestamps_enabled(),
                #[cfg(test)]
                SessionSurfaceOwner::Test(_) => false,
            },
            paged,
            render_submit_ms: timer_elapsed_ms(render_started),
            frame_wall_ms,
        };
        Ok(if frame_presented {
            SessionFrameAttempt::Presented(candidate)
        } else {
            SessionFrameAttempt::Unavailable(candidate)
        })
    }
}

pub(crate) fn paged_surface_counts(source_count: usize, drawn_count: u32) -> (u32, u32) {
    (u32::try_from(source_count).unwrap_or(u32::MAX), drawn_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_presented_attempt_enters_the_commit_closure() {
        let unavailable = SessionFrameAttempt::<u64>::Unavailable(41);
        let mut commits = 0;
        assert_eq!(
            unavailable.commit(|_| {
                commits += 1;
                0
            }),
            Err(41)
        );
        assert_eq!(commits, 0);

        let presented = SessionFrameAttempt::<u64>::Presented(42);
        assert_eq!(
            presented.commit(|candidate| {
                commits += 1;
                candidate
            }),
            Ok(42)
        );
        assert_eq!(commits, 1);
    }

    #[test]
    fn executor_is_stateless_and_has_no_publication_owners() {
        assert_eq!(std::mem::size_of::<SessionFrameExecutor>(), 0);
        let source = include_str!("session_frame_executor.rs");
        for forbidden in [
            concat!("Adaptive", "OrderPolicy"),
            concat!("Session", "Evidence"),
            concat!("Current", "StatsSubmission"),
            concat!("exact_plan_", "receipt"),
            concat!("last_", "stats:"),
            concat!("cache_", "generation"),
        ] {
            assert!(!source.contains(forbidden), "executor retained {forbidden}");
        }
    }
}
