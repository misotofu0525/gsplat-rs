//! Surface target adapter for the shared Exact renderer.
//!
//! This module owns no scene, plan, controller, generation, sampler or frame
//! result. It borrows the presenter's lifecycle leaves and the renderer's sole
//! `PreparedRuntimeSlot`, publishing semantics only after primitive present.

use gsplat_core::Camera;
use thiserror::Error;

use super::SurfaceCapture;
use super::{SurfaceConfigurationOwner, SurfaceLifecycle};
use crate::SurfacePresenterError;
use crate::plans::FrameIdentity;
#[cfg(test)]
use crate::plans::PlanId;
use crate::renderer::frame::Viewport;
use crate::renderer::{
    ExactPlanPolicy, FrameExecutionError, GpuFrameEncodeRequest, GpuFrameSubmission,
    PreparedRuntimeSlot, SubmittedGpuFrame, abandon_submitted_frame, encode_frame_gpu,
    submit_encoded_frame_unpublished, validate_submitted_frame,
};

pub(crate) struct SurfaceExactRequest<'a> {
    pub(crate) camera: &'a Camera,
    pub(crate) viewport: Viewport,
    pub(crate) clear: wgpu::Color,
    pub(crate) force_cpu_order_refresh: bool,
    pub(crate) host_frame_started: Option<crate::TimerInstant>,
}

/// Existing Surface owners borrowed as one host transaction. No
/// adapter, device, queue, configuration, capture, or lifecycle is duplicated
/// by the Exact route.
pub(crate) struct NativeSurfaceExactHost<'host, 'window> {
    pub(crate) surface: &'host wgpu::Surface<'window>,
    pub(crate) device: &'host wgpu::Device,
    pub(crate) configuration: &'host SurfaceConfigurationOwner,
    pub(crate) lifecycle: &'host mut SurfaceLifecycle,
    pub(crate) capture: &'host mut SurfaceCapture,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SurfaceTargetReceipt {
    frame: FrameIdentity,
    requested: (u32, u32),
    configured: (u32, u32),
    acquired: (u32, u32),
    internal_render: (u32, u32),
    presented: (u32, u32),
    presentation_sequence: u64,
}

impl SurfaceTargetReceipt {
    pub(crate) const fn presentation_sequence(self) -> u64 {
        self.presentation_sequence
    }
}

#[derive(Clone, Copy)]
struct UnpublishedSurfaceTarget {
    requested: (u32, u32),
    configured: (u32, u32),
    acquired: (u32, u32),
}

pub(crate) struct SurfaceExactFrameResult {
    submission: GpuFrameSubmission,
    target: SurfaceTargetReceipt,
}

#[derive(Debug, Error)]
pub(crate) enum SurfaceExactError {
    #[error("surface host failed: {0}")]
    Surface(#[from] SurfacePresenterError),
    #[error("Exact Surface frame failed: {0}")]
    Frame(#[from] FrameExecutionError),
    #[error(
        "surface target size mismatch: requested={requested:?}, configured={configured:?}, acquired={acquired:?}"
    )]
    TargetSizeMismatch {
        requested: (u32, u32),
        configured: (u32, u32),
        acquired: (u32, u32),
    },
}

impl SurfaceExactFrameResult {
    pub(crate) fn submission(&self) -> &GpuFrameSubmission {
        &self.submission
    }

    pub(crate) const fn target(&self) -> SurfaceTargetReceipt {
        self.target
    }
}

/// Product adapter against an actual `wgpu::Surface`. Native Packed sessions
/// enter here before any legacy session semantic writer can run.
pub(crate) fn render_surface_exact_frame(
    runtime: &mut PreparedRuntimeSlot,
    host: NativeSurfaceExactHost<'_, '_>,
    request: SurfaceExactRequest<'_>,
) -> Result<Option<SurfaceExactFrameResult>, SurfaceExactError> {
    let requested = (request.viewport.width(), request.viewport.height());
    let configured = host.configuration.size();
    begin_surface_exact_attempt(host.lifecycle, requested, configured, || {
        host.configuration.validate_size(requested.0, requested.1)
    })?;
    let Some(frame) = host
        .lifecycle
        .acquire(host.surface, host.device, host.configuration)?
    else {
        return Ok(None);
    };
    let acquired = (frame.texture.width(), frame.texture.height());
    if acquired != configured {
        return Err(SurfaceExactError::TargetSizeMismatch {
            requested,
            configured,
            acquired,
        });
    }

    let view = frame
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());
    let mut encode_request = match runtime.active_policy() {
        ExactPlanPolicy::Forced(plan) => GpuFrameEncodeRequest::new(
            plan,
            request.camera,
            request.viewport,
            &view,
            host.configuration.format(),
            request.clear,
        ),
        ExactPlanPolicy::Adaptive => GpuFrameEncodeRequest::adaptive(
            request.camera,
            request.viewport,
            &view,
            host.configuration.format(),
            request.clear,
        ),
    };
    if request.force_cpu_order_refresh || runtime.cpu_order_refresh_requested() {
        encode_request = encode_request.with_forced_cpu_order_refresh();
    }
    if let Some(started) = request.host_frame_started {
        encode_request = encode_request.with_host_frame_started(started);
    }
    #[allow(unused_mut)]
    let mut pending = encode_frame_gpu(runtime, encode_request)?;
    host.capture.encode(pending.encoder_mut(), &frame.texture);
    let mut submitted = submit_encoded_frame_unpublished(runtime, pending)?;

    finish_presented_exact_frame(
        runtime,
        host.lifecycle,
        host.capture,
        &mut submitted,
        UnpublishedSurfaceTarget {
            requested,
            configured,
            acquired,
        },
        || {
            frame.present();
            Ok(())
        },
    )
    .map(Some)
}

/// Starts a new target attempt before any fallible preflight. A rejected size,
/// exhausted sequence or later acquire failure therefore cannot leave the
/// previous frame's presentation receipt visible to the host.
fn begin_surface_exact_attempt(
    lifecycle: &mut SurfaceLifecycle,
    requested: (u32, u32),
    configured: (u32, u32),
    validate_size: impl FnOnce() -> Result<(), SurfacePresenterError>,
) -> Result<(), SurfaceExactError> {
    lifecycle.begin_frame();
    validate_size()?;
    if requested != configured {
        return Err(SurfaceExactError::TargetSizeMismatch {
            requested,
            configured,
            acquired: configured,
        });
    }
    Ok(())
}

fn finish_presented_exact_frame(
    runtime: &mut PreparedRuntimeSlot,
    lifecycle: &mut SurfaceLifecycle,
    capture: &mut SurfaceCapture,
    submitted: &mut SubmittedGpuFrame,
    target: UnpublishedSurfaceTarget,
    present: impl FnOnce() -> Result<(), SurfacePresenterError>,
) -> Result<SurfaceExactFrameResult, SurfaceExactError> {
    let UnpublishedSurfaceTarget {
        requested,
        configured,
        acquired,
    } = target;
    if requested != configured || configured != acquired {
        return Err(SurfaceExactError::TargetSizeMismatch {
            requested,
            configured,
            acquired,
        });
    }
    // All fallible renderer identity checks happen before the target is made
    // visible. The guard holds the exclusive runtime borrow until its
    // infallible commit after the primitive present call.
    let validated = validate_submitted_frame(runtime, submitted)?;
    let presented = match lifecycle.try_present_with(acquired, present) {
        Ok(presented) => presented,
        Err(error) => {
            let _ = abandon_submitted_frame(submitted);
            return Err(error.into());
        }
    };
    capture.mark_presented();
    let submission = validated.publish();
    let frame = submission.frame_identity();
    let presentation_sequence = submission.presentation_sequence();
    Ok(SurfaceExactFrameResult {
        submission,
        target: SurfaceTargetReceipt {
            frame,
            requested,
            configured,
            acquired,
            // CanonicalRaster writes directly into the acquired Surface
            // texture; there is no hidden scaled intermediate target.
            internal_render: acquired,
            presented,
            presentation_sequence,
        },
    })
}

// Keep the historical injected-target harness readable while the product path
// above uses only lifecycle-owned sequencing. These aliases never enter a
// non-test build and therefore cannot become a second Surface semantic owner.
#[cfg(test)]
type SurfaceShadowFrameResult = SurfaceExactFrameResult;

#[cfg(test)]
type SurfaceShadowError = SurfaceExactError;

#[cfg(test)]
#[derive(Default)]
struct SurfaceShadowState {
    presentation_sequence: u64,
}

#[cfg(test)]
fn begin_surface_shadow_attempt(
    _state: &SurfaceShadowState,
    lifecycle: &mut SurfaceLifecycle,
    requested: (u32, u32),
    configured: (u32, u32),
    validate_size: impl FnOnce() -> Result<(), SurfacePresenterError>,
) -> Result<(), SurfaceShadowError> {
    begin_surface_exact_attempt(lifecycle, requested, configured, validate_size)
}

#[cfg(test)]
fn finish_presented_shadow_frame(
    state: &mut SurfaceShadowState,
    runtime: &mut PreparedRuntimeSlot,
    lifecycle: &mut SurfaceLifecycle,
    capture: &mut SurfaceCapture,
    submitted: &mut SubmittedGpuFrame,
    target: UnpublishedSurfaceTarget,
    present: impl FnOnce(),
) -> Result<SurfaceShadowFrameResult, SurfaceShadowError> {
    let result =
        finish_presented_exact_frame(runtime, lifecycle, capture, submitted, target, || {
            present();
            Ok(())
        })?;
    state.presentation_sequence = result.target().presentation_sequence;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gsplat_core::{SceneBuffers, Vec3f};

    use super::*;
    use crate::evidence::PlanCountSemantics as RasterCountSemantics;
    use crate::plans::TestGpuAdmissionMode;
    use crate::scene::ResidentSceneCpu;

    const WIDTH: u32 = 64;
    const HEIGHT: u32 = 64;
    const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

    #[derive(Debug, Clone, Copy)]
    enum SurfaceShadowSelection {
        Forced(PlanId),
        Adaptive,
    }

    fn exact_scene(count: usize) -> ResidentSceneCpu {
        let buffers = SceneBuffers {
            positions: (0..count)
                .map(|index| {
                    Vec3f::new(
                        (index % 11) as f32 * 0.03 - 0.15,
                        ((index / 11) % 7) as f32 * 0.03 - 0.09,
                        1.0 + (index % 3) as f32 * 0.02,
                    )
                })
                .collect(),
            opacity: vec![1.0; count],
            scale_xyz: vec![[-2.5; 3]; count],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; count],
            color_dc: (0..count)
                .map(|index| {
                    let value = index as f32 / count.max(1) as f32;
                    [value * 0.3, 0.1 + value * 0.15, 0.05]
                })
                .collect(),
            sh_degree: 3,
            sh_rest: Some(vec![0.001; count * 45]),
        };
        ResidentSceneCpu::encode_owned(buffers).expect("Exact shadow Surface fixture")
    }

    fn portable_limits() -> wgpu::Limits {
        let mut limits = wgpu::Limits::downlevel_defaults();
        limits.max_storage_buffers_per_shader_stage =
            crate::resident_gpu::RESIDENT_COLOR_STORAGE_BINDINGS;
        limits.max_storage_buffer_binding_size = 128 << 20;
        limits.max_buffer_size = 128 << 20;
        limits
    }

    async fn request_device() -> Option<(Arc<wgpu::Device>, Arc<wgpu::Queue>)> {
        #[cfg(target_os = "macos")]
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            ..Default::default()
        });
        #[cfg(not(target_os = "macos"))]
        let instance = wgpu::Instance::default();

        let adapter = match instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
        {
            Ok(adapter) => adapter,
            #[cfg(target_os = "macos")]
            Err(error) => panic!("required E12 Metal adapter unavailable: {error}"),
            #[cfg(not(target_os = "macos"))]
            Err(error) => {
                eprintln!("skipping optional E12 Surface GPU test: {error}");
                return None;
            }
        };
        #[cfg(target_os = "macos")]
        assert_eq!(adapter.get_info().backend, wgpu::Backend::Metal);
        let limits = portable_limits();
        if !limits.check_limits(&adapter.limits()) {
            #[cfg(target_os = "macos")]
            panic!("required E12 Metal limits unavailable: {limits:?}");
            #[cfg(not(target_os = "macos"))]
            return None;
        }
        let descriptor = wgpu::DeviceDescriptor {
            label: Some("exact-e12-surface-shadow-device"),
            required_features: wgpu::Features::empty(),
            required_limits: limits,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        };
        match adapter.request_device(&descriptor).await {
            Ok((device, queue)) => Some((Arc::new(device), Arc::new(queue))),
            #[cfg(target_os = "macos")]
            Err(error) => panic!("required E12 Metal device unavailable: {error}"),
            #[cfg(not(target_os = "macos"))]
            Err(error) => {
                eprintln!("skipping optional E12 Surface GPU test: {error}");
                None
            }
        }
    }

    async fn prepared_slot(
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
    ) -> PreparedRuntimeSlot {
        let mut slot = PreparedRuntimeSlot::prepare(exact_scene(127)).expect("CPU runtime");
        slot.set_test_gpu_admission_mode(TestGpuAdmissionMode::ConcreteAll);
        let receipt = slot
            .prepare_gpu(device, queue, FORMAT)
            .await
            .expect("all Exact plans and canonical raster");
        assert_eq!(receipt.source_count(), 127);
        assert_eq!(receipt.capacity(), 127);
        assert_eq!(receipt.resident_count(), 127);
        assert_eq!(receipt.addressable_count(), 127);
        assert_eq!(receipt.sh_degree(), 3);
        slot
    }

    fn target(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("exact-e12-injected-surface-target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
    }

    fn encode_and_submit(
        slot: &mut PreparedRuntimeSlot,
        capture: &mut SurfaceCapture,
        texture: &wgpu::Texture,
        selection: SurfaceShadowSelection,
        viewport: Viewport,
    ) -> SubmittedGpuFrame {
        let camera = Camera::default();
        encode_and_submit_camera(slot, capture, texture, selection, &camera, viewport)
    }

    fn encode_and_submit_camera(
        slot: &mut PreparedRuntimeSlot,
        capture: &mut SurfaceCapture,
        texture: &wgpu::Texture,
        selection: SurfaceShadowSelection,
        camera: &Camera,
        viewport: Viewport,
    ) -> SubmittedGpuFrame {
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut request = match selection {
            SurfaceShadowSelection::Forced(plan) => GpuFrameEncodeRequest::new(
                plan,
                camera,
                viewport,
                &view,
                FORMAT,
                wgpu::Color::BLACK,
            ),
            SurfaceShadowSelection::Adaptive => {
                GpuFrameEncodeRequest::adaptive(camera, viewport, &view, FORMAT, wgpu::Color::BLACK)
            }
        };
        if slot.cpu_order_refresh_requested() {
            request = request.with_forced_cpu_order_refresh();
        }
        let mut pending = encode_frame_gpu(slot, request).expect("Exact Surface encode");
        capture.encode(pending.encoder_mut(), texture);
        submit_encoded_frame_unpublished(slot, pending).expect("unpublished Surface submit")
    }

    fn present_injected(
        state: &mut SurfaceShadowState,
        slot: &mut PreparedRuntimeSlot,
        lifecycle: &mut SurfaceLifecycle,
        capture: &mut SurfaceCapture,
        submitted: &mut SubmittedGpuFrame,
        size: (u32, u32),
        present_count: &mut u32,
    ) -> SurfaceShadowFrameResult {
        finish_presented_shadow_frame(
            state,
            slot,
            lifecycle,
            capture,
            submitted,
            UnpublishedSurfaceTarget {
                requested: size,
                configured: size,
                acquired: size,
            },
            || *present_count += 1,
        )
        .expect("injected present and publish")
    }

    fn wait(device: &wgpu::Device, submission: &GpuFrameSubmission) {
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission.submission_index().clone()),
                timeout: None,
            })
            .expect("wait Exact Surface submission");
    }

    #[test]
    fn forced_and_adaptive_plans_share_one_exact_presented_target() {
        pollster::block_on(async {
            let Some((device, queue)) = request_device().await else {
                return;
            };
            let viewport = Viewport::new(WIDTH, HEIGHT).expect("viewport");
            let cases = [
                SurfaceShadowSelection::Forced(PlanId::CpuPostSort),
                SurfaceShadowSelection::Forced(PlanId::GpuPostSort),
                SurfaceShadowSelection::Forced(PlanId::GpuPreproject),
                SurfaceShadowSelection::Adaptive,
            ];
            let mut reference = None;
            for selection in cases {
                let mut slot = prepared_slot(&device, &queue).await;
                let mut lifecycle = SurfaceLifecycle::new();
                lifecycle.begin_frame();
                let mut capture = SurfaceCapture::new(true);
                capture.publish(
                    capture
                        .prepare_request(&device, WIDTH, HEIGHT, FORMAT)
                        .expect("capture request"),
                );
                let texture = target(&device, WIDTH, HEIGHT);
                let before = slot.frame_state();
                let mut submitted =
                    encode_and_submit(&mut slot, &mut capture, &texture, selection, viewport);
                assert_eq!(slot.frame_state(), before);
                let mut state = SurfaceShadowState::default();
                let mut present_count = 0;
                let result = present_injected(
                    &mut state,
                    &mut slot,
                    &mut lifecycle,
                    &mut capture,
                    &mut submitted,
                    (WIDTH, HEIGHT),
                    &mut present_count,
                );
                assert_eq!(present_count, 1);
                let target = result.target();
                assert_eq!(target.frame, result.submission().frame_identity());
                assert_eq!(target.requested, (WIDTH, HEIGHT));
                assert_eq!(target.configured, target.requested);
                assert_eq!(target.acquired, target.requested);
                assert_eq!(target.internal_render, target.requested);
                assert_eq!(target.presented, target.requested);
                assert_eq!(target.presentation_sequence, 1);
                wait(&device, result.submission());
                let image = capture.take(&device).expect("presented exact capture");
                assert!(image.rgba8.iter().any(|value| *value != 0));
                if let Some(reference) = &reference {
                    assert_eq!(
                        &image.rgba8, reference,
                        "plan image mismatch: {selection:?}"
                    );
                } else {
                    reference = Some(image.rgba8);
                }

                let submission = result.submission();
                assert_eq!(submission.source_count(), 127);
                match submission.plan_id() {
                    PlanId::CpuPostSort => {
                        assert_eq!(submission.visible_count(), submission.draw_count());
                        assert!(submission.visible_count().is_some());
                        assert_eq!(submission.contributor_count(), None);
                        assert_eq!(
                            submission.count_semantics(),
                            RasterCountSemantics::DirectDrawEqualsVisible
                        );
                    }
                    PlanId::GpuPostSort => {
                        assert_eq!(submission.visible_count(), None);
                        assert_eq!(submission.contributor_count(), None);
                        assert_eq!(submission.draw_count(), None);
                        assert_eq!(
                            submission.count_semantics(),
                            RasterCountSemantics::IndirectDrawEqualsVisible
                        );
                    }
                    PlanId::GpuPreproject => {
                        assert_eq!(submission.visible_count(), None);
                        assert_eq!(submission.contributor_count(), None);
                        assert_eq!(submission.draw_count(), None);
                        assert_eq!(
                            submission.count_semantics(),
                            RasterCountSemantics::IndirectDrawEqualsContributor
                        );
                    }
                }
            }
        });
    }

    #[test]
    fn unpresented_capture_is_unpublishable_and_retry_succeeds() {
        pollster::block_on(async {
            let Some((device, queue)) = request_device().await else {
                return;
            };
            let mut slot = prepared_slot(&device, &queue).await;
            let viewport = Viewport::new(WIDTH, HEIGHT).expect("viewport");
            let mut lifecycle = SurfaceLifecycle::new();
            let mut capture = SurfaceCapture::new(true);
            capture.publish(
                capture
                    .prepare_request(&device, WIDTH, HEIGHT, FORMAT)
                    .expect("capture request"),
            );
            let before = slot.frame_state();
            let first_texture = target(&device, WIDTH, HEIGHT);
            let mut first = encode_and_submit(
                &mut slot,
                &mut capture,
                &first_texture,
                SurfaceShadowSelection::Forced(PlanId::CpuPostSort),
                viewport,
            );
            device
                .poll(wgpu::PollType::Wait {
                    submission_index: first.submission_index().cloned(),
                    timeout: None,
                })
                .expect("wait abandoned target");
            assert!(matches!(
                capture.take(&device),
                Err(SurfacePresenterError::SurfaceCaptureState(_))
            ));
            assert!(crate::renderer::abandon_submitted_frame(&mut first));
            assert_eq!(slot.frame_state(), before);

            lifecycle.begin_frame();
            let retry_texture = target(&device, WIDTH, HEIGHT);
            let mut retry = encode_and_submit(
                &mut slot,
                &mut capture,
                &retry_texture,
                SurfaceShadowSelection::Forced(PlanId::CpuPostSort),
                viewport,
            );
            let mut state = SurfaceShadowState::default();
            let mut present_count = 0;
            let result = present_injected(
                &mut state,
                &mut slot,
                &mut lifecycle,
                &mut capture,
                &mut retry,
                (WIDTH, HEIGHT),
                &mut present_count,
            );
            wait(&device, result.submission());
            assert_eq!(present_count, 1);
            assert_eq!(
                capture.take(&device).expect("retry capture").rgba8.len(),
                4 * WIDTH as usize * HEIGHT as usize
            );
        });
    }

    #[test]
    fn failed_adaptive_present_publishes_no_plan_or_controller_frame() {
        pollster::block_on(async {
            let Some((device, queue)) = request_device().await else {
                return;
            };
            let mut slot = prepared_slot(&device, &queue).await;
            slot.set_active_policy(ExactPlanPolicy::Adaptive);
            let viewport = Viewport::new(WIDTH, HEIGHT).expect("viewport");
            let mut lifecycle = SurfaceLifecycle::new();
            let mut capture = SurfaceCapture::new(false);
            let before = slot.frame_state();

            lifecycle.begin_frame();
            let failed_texture = target(&device, WIDTH, HEIGHT);
            let mut failed = encode_and_submit(
                &mut slot,
                &mut capture,
                &failed_texture,
                SurfaceShadowSelection::Adaptive,
                viewport,
            );
            let error = match finish_presented_exact_frame(
                &mut slot,
                &mut lifecycle,
                &mut capture,
                &mut failed,
                UnpublishedSurfaceTarget {
                    requested: (WIDTH, HEIGHT),
                    configured: (WIDTH, HEIGHT),
                    acquired: (WIDTH, HEIGHT),
                },
                || {
                    Err(SurfacePresenterError::SurfaceAcquire(
                        "injected adaptive present failure".into(),
                    ))
                },
            ) {
                Ok(_) => panic!("failed adaptive present must remain unpublished"),
                Err(error) => error,
            };
            assert!(matches!(error, SurfaceExactError::Surface(_)));
            assert_eq!(slot.frame_state(), before);
            assert_eq!(slot.last_published_plan(), None);
            assert!(!lifecycle.last_frame_presented());

            lifecycle.begin_frame();
            let retry_texture = target(&device, WIDTH, HEIGHT);
            let mut retry = encode_and_submit(
                &mut slot,
                &mut capture,
                &retry_texture,
                SurfaceShadowSelection::Adaptive,
                viewport,
            );
            let mut state = SurfaceShadowState::default();
            let mut present_count = 0;
            let result = present_injected(
                &mut state,
                &mut slot,
                &mut lifecycle,
                &mut capture,
                &mut retry,
                (WIDTH, HEIGHT),
                &mut present_count,
            );
            assert_eq!(
                slot.last_published_plan(),
                Some(result.submission().plan_id())
            );
            assert_eq!(result.target().presentation_sequence, 1);
            assert_eq!(present_count, 1);
        });
    }

    #[test]
    fn resize_is_published_only_by_the_successfully_presented_retry() {
        pollster::block_on(async {
            let Some((device, queue)) = request_device().await else {
                return;
            };
            let mut slot = prepared_slot(&device, &queue).await;
            let mut lifecycle = SurfaceLifecycle::new();
            let mut capture = SurfaceCapture::new(false);
            let first_viewport = Viewport::new(WIDTH, HEIGHT).expect("viewport");
            let first_texture = target(&device, WIDTH, HEIGHT);
            let mut first = encode_and_submit(
                &mut slot,
                &mut capture,
                &first_texture,
                SurfaceShadowSelection::Forced(PlanId::CpuPostSort),
                first_viewport,
            );
            lifecycle.begin_frame();
            let mut state = SurfaceShadowState::default();
            let mut present_count = 0;
            let first_result = present_injected(
                &mut state,
                &mut slot,
                &mut lifecycle,
                &mut capture,
                &mut first,
                (WIDTH, HEIGHT),
                &mut present_count,
            );
            let first_generation = first_result
                .submission()
                .frame_identity()
                .viewport_generation();

            let published_frame = slot.frame_state();
            let published_sequence = state.presentation_sequence;
            assert!(lifecycle.last_frame_presented());
            assert_eq!(lifecycle.last_presented_size(), Some((WIDTH, HEIGHT)));
            let pre_acquire_error = begin_surface_shadow_attempt(
                &state,
                &mut lifecycle,
                (WIDTH + 1, HEIGHT),
                (WIDTH, HEIGHT),
                || Ok(()),
            )
            .expect_err("mismatched target must fail before acquire");
            assert!(matches!(
                pre_acquire_error,
                SurfaceShadowError::TargetSizeMismatch { .. }
            ));
            assert!(!lifecycle.last_frame_presented());
            assert_eq!(lifecycle.last_presented_size(), None);
            assert_eq!(slot.frame_state(), published_frame);
            assert_eq!(state.presentation_sequence, published_sequence);

            let resized = (WIDTH + 1, HEIGHT);
            let resized_viewport = Viewport::new(resized.0, resized.1).expect("resized viewport");
            let aborted_texture = target(&device, resized.0, resized.1);
            let mut aborted = encode_and_submit(
                &mut slot,
                &mut capture,
                &aborted_texture,
                SurfaceShadowSelection::Forced(PlanId::CpuPostSort),
                resized_viewport,
            );
            assert!(crate::renderer::abandon_submitted_frame(&mut aborted));
            assert_eq!(
                slot.frame_state().identity().viewport_generation(),
                first_generation
            );

            let retry_texture = target(&device, resized.0, resized.1);
            let mut retry = encode_and_submit(
                &mut slot,
                &mut capture,
                &retry_texture,
                SurfaceShadowSelection::Forced(PlanId::CpuPostSort),
                resized_viewport,
            );
            lifecycle.begin_frame();
            let result = present_injected(
                &mut state,
                &mut slot,
                &mut lifecycle,
                &mut capture,
                &mut retry,
                resized,
                &mut present_count,
            );
            assert_eq!(
                result.submission().frame_identity().viewport_generation(),
                first_generation + 1
            );
            assert_eq!(result.target().presentation_sequence, 2);
            assert_eq!(present_count, 2);
        });
    }

    #[test]
    fn forced_cpu_refresh_survives_unavailable_and_failed_present_until_retry() {
        pollster::block_on(async {
            let Some((device, queue)) = request_device().await else {
                return;
            };
            let mut slot = prepared_slot(&device, &queue).await;
            let viewport = Viewport::new(WIDTH, HEIGHT).expect("viewport");
            let mut lifecycle = SurfaceLifecycle::new();
            let mut capture = SurfaceCapture::new(false);
            let mut state = SurfaceShadowState::default();
            let mut present_count = 0;

            lifecycle.begin_frame();
            let initial_texture = target(&device, WIDTH, HEIGHT);
            let mut initial = encode_and_submit(
                &mut slot,
                &mut capture,
                &initial_texture,
                SurfaceShadowSelection::Forced(PlanId::CpuPostSort),
                viewport,
            );
            let initial_result = present_injected(
                &mut state,
                &mut slot,
                &mut lifecycle,
                &mut capture,
                &mut initial,
                (WIDTH, HEIGHT),
                &mut present_count,
            );
            let initial_order_generation = initial_result.submission().order_generation();
            let initial_frame = initial_result.submission().frame_identity();

            slot.request_cpu_order_refresh();
            assert!(slot.cpu_order_refresh_requested());

            // A missing drawable does not encode, consume the refresh latch,
            // or advance either successful-presentation sequence.
            lifecycle.begin_frame();
            let unavailable = lifecycle
                .acquire_with::<u32>(|| Err(wgpu::SurfaceError::Timeout), || {})
                .expect("timeout is retryable");
            assert_eq!(unavailable, None);
            assert!(slot.cpu_order_refresh_requested());
            assert_eq!(state.presentation_sequence, 1);

            // A submitted target whose primitive present fails is abandoned.
            // Renderer identity and the latch stay unpublished for the retry.
            lifecycle.begin_frame();
            let failed_texture = target(&device, WIDTH, HEIGHT);
            let mut failed = encode_and_submit(
                &mut slot,
                &mut capture,
                &failed_texture,
                SurfaceShadowSelection::Forced(PlanId::CpuPostSort),
                viewport,
            );
            let error = match finish_presented_exact_frame(
                &mut slot,
                &mut lifecycle,
                &mut capture,
                &mut failed,
                UnpublishedSurfaceTarget {
                    requested: (WIDTH, HEIGHT),
                    configured: (WIDTH, HEIGHT),
                    acquired: (WIDTH, HEIGHT),
                },
                || {
                    Err(SurfacePresenterError::SurfaceAcquire(
                        "injected primitive present failure".into(),
                    ))
                },
            ) {
                Ok(_) => panic!("failed primitive present must abandon semantic publication"),
                Err(error) => error,
            };
            assert!(matches!(error, SurfaceExactError::Surface(_)));
            assert!(slot.cpu_order_refresh_requested());
            assert_eq!(slot.frame_state().identity(), initial_frame);
            assert_eq!(state.presentation_sequence, 1);

            lifecycle.begin_frame();
            let retry_texture = target(&device, WIDTH, HEIGHT);
            let mut retry = encode_and_submit(
                &mut slot,
                &mut capture,
                &retry_texture,
                SurfaceShadowSelection::Forced(PlanId::CpuPostSort),
                viewport,
            );
            let retry_result = present_injected(
                &mut state,
                &mut slot,
                &mut lifecycle,
                &mut capture,
                &mut retry,
                (WIDTH, HEIGHT),
                &mut present_count,
            );
            assert!(retry_result.submission().order_generation() > initial_order_generation);
            assert_eq!(
                retry_result.submission().frame_identity().camera_revision(),
                initial_frame.camera_revision()
            );
            assert_eq!(retry_result.target().presentation_sequence, 2);
            assert!(!slot.cpu_order_refresh_requested());
            assert_eq!(present_count, 2);
        });
    }

    #[test]
    fn surface_camera_baseline_matches_public_revision_on_first_and_changed_frames() {
        pollster::block_on(async {
            let Some((device, queue)) = request_device().await else {
                return;
            };
            let viewport = Viewport::new(WIDTH, HEIGHT).expect("viewport");
            let initial_camera = Camera::default();
            let mut slot = prepared_slot(&device, &queue).await;
            slot.seed_surface_frame_baseline(initial_camera, viewport);
            let mut lifecycle = SurfaceLifecycle::new();
            let mut capture = SurfaceCapture::new(false);
            let mut state = SurfaceShadowState::default();
            let mut present_count = 0;

            lifecycle.begin_frame();
            let first_texture = target(&device, WIDTH, HEIGHT);
            let mut first = encode_and_submit_camera(
                &mut slot,
                &mut capture,
                &first_texture,
                SurfaceShadowSelection::Forced(PlanId::CpuPostSort),
                &initial_camera,
                viewport,
            );
            let first = present_injected(
                &mut state,
                &mut slot,
                &mut lifecycle,
                &mut capture,
                &mut first,
                (WIDTH, HEIGHT),
                &mut present_count,
            );
            assert_eq!(first.submission().frame_identity().camera_revision(), 0);

            let mut moved_camera = initial_camera;
            moved_camera.pose.position.x = 0.25;
            lifecycle.begin_frame();
            let moved_texture = target(&device, WIDTH, HEIGHT);
            let mut moved = encode_and_submit_camera(
                &mut slot,
                &mut capture,
                &moved_texture,
                SurfaceShadowSelection::Forced(PlanId::CpuPostSort),
                &moved_camera,
                viewport,
            );
            let moved = present_injected(
                &mut state,
                &mut slot,
                &mut lifecycle,
                &mut capture,
                &mut moved,
                (WIDTH, HEIGHT),
                &mut present_count,
            );
            assert_eq!(moved.submission().frame_identity().camera_revision(), 1);
            assert_eq!(moved.target().presentation_sequence, 2);
        });
    }

    #[test]
    fn pre_submit_target_failure_keeps_semantic_state_unpublished() {
        pollster::block_on(async {
            let Some((device, queue)) = request_device().await else {
                return;
            };
            let mut slot = prepared_slot(&device, &queue).await;
            let texture = target(&device, WIDTH, HEIGHT);
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let before = slot.frame_state();
            let error = match encode_frame_gpu(
                &mut slot,
                GpuFrameEncodeRequest::new(
                    PlanId::CpuPostSort,
                    &Camera::default(),
                    Viewport::new(WIDTH, HEIGHT).expect("viewport"),
                    &view,
                    wgpu::TextureFormat::Bgra8Unorm,
                    wgpu::Color::BLACK,
                ),
            ) {
                Ok(_) => panic!("raster target mismatch must fail before submit"),
                Err(error) => error,
            };
            assert!(matches!(error, FrameExecutionError::Raster(_)));
            assert_eq!(slot.frame_state(), before);
        });
    }

    #[test]
    fn unavailable_and_failed_acquisition_never_publish_renderer_state() {
        pollster::block_on(async {
            let Some((device, queue)) = request_device().await else {
                return;
            };
            let slot = prepared_slot(&device, &queue).await;
            let before = slot.frame_state();
            let lifecycle = SurfaceLifecycle::new();

            let timeout = lifecycle
                .acquire_with::<u32>(|| Err(wgpu::SurfaceError::Timeout), || {})
                .expect("timeout is an unavailable drawable");
            assert_eq!(timeout, None);
            assert_eq!(slot.frame_state(), before);

            for lost in [true, false] {
                let mut attempts = 0;
                let mut reconfigured = 0;
                let result = lifecycle.acquire_with::<u32>(
                    || {
                        attempts += 1;
                        Err(if lost {
                            wgpu::SurfaceError::Lost
                        } else {
                            wgpu::SurfaceError::Outdated
                        })
                    },
                    || reconfigured += 1,
                );
                assert!(matches!(
                    result,
                    Err(SurfacePresenterError::SurfaceAcquire(_))
                ));
                assert_eq!(attempts, 2);
                assert_eq!(reconfigured, 1);
                assert_eq!(slot.frame_state(), before);
            }

            for lost in [true, false] {
                let mut first = true;
                let unavailable = lifecycle
                    .acquire_with::<u32>(
                        || {
                            if std::mem::take(&mut first) {
                                Err(if lost {
                                    wgpu::SurfaceError::Lost
                                } else {
                                    wgpu::SurfaceError::Outdated
                                })
                            } else {
                                Err(wgpu::SurfaceError::Timeout)
                            }
                        },
                        || {},
                    )
                    .expect("retry timeout remains unavailable");
                assert_eq!(unavailable, None);
                assert_eq!(slot.frame_state(), before);
            }
        });
    }

    #[test]
    fn shadow_module_compiles_a_real_surface_acquire_and_present_adapter() {
        let source = include_str!("shadow.rs");
        assert!(source.contains(".acquire(host.surface, host.device, host.configuration)"));
        assert!(source.contains("|| frame.present()"));
        assert!(source.contains("validate_submitted_frame(runtime, submitted)"));
        assert!(source.contains("capture.mark_presented()"));
        assert!(source.contains("validated.publish()"));
    }
}
