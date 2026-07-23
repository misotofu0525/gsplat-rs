//! Test-only adapter from the private Exact plan handoff to legacy offscreen raster.

use gsplat_core::{Camera, FrameStats, RenderMode, RendererConfig, SceneBuffers};
use thiserror::Error;

use crate::renderer::frame::Viewport;
use crate::renderer::tests::{
    ShadowCurrentnessError, ShadowFrame, ShadowFrameError, validate_current_shadow_frame,
};
use crate::renderer::{PreparedRuntimeError, PreparedRuntimeSlot};
use crate::scene::{ResidentSceneCpu, ResidentSceneError};
use crate::{GeometryPath, Renderer, RendererError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LegacyCurrentnessError {
    ShadowSceneIdentity,
    PositionsOwner,
    GeometryPath,
    RenderMode,
    RendererConfig,
    BackingViewport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PositionsOwnerIdentity {
    non_empty_ptr: Option<usize>,
    len: usize,
}

impl PositionsOwnerIdentity {
    fn capture(renderer: &Renderer) -> Result<Self, RendererError> {
        let positions = renderer.positions().ok_or(RendererError::SceneNotLoaded)?;
        Ok(Self::from_slice(positions))
    }

    fn from_slice<T>(positions: &[T]) -> Self {
        Self {
            non_empty_ptr: (!positions.is_empty()).then_some(positions.as_ptr() as usize),
            len: positions.len(),
        }
    }
}

/// Immutable identity of the already materialized legacy offscreen oracle.
///
/// Empty legacy-renderer position owners are content-equivalent. Non-empty
/// scenes additionally bind each exact allocation, while the shadow slot's
/// scene generation makes every successful runtime replacement stale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LegacyOracleReceipt {
    positions: PositionsOwnerIdentity,
    shadow_positions: PositionsOwnerIdentity,
    shadow_scene_generation: u64,
    geometry_path: GeometryPath,
    mode: RenderMode,
    config: RendererConfig,
    backing_viewport: (u32, u32),
}

impl LegacyOracleReceipt {
    fn capture(renderer: &Renderer, slot: &PreparedRuntimeSlot) -> Result<Self, RendererError> {
        Ok(Self {
            positions: PositionsOwnerIdentity::capture(renderer)?,
            shadow_positions: PositionsOwnerIdentity::from_slice(slot.scene().positions()),
            shadow_scene_generation: slot.frame_state().identity().scene_generation(),
            geometry_path: renderer.geometry_path(),
            mode: renderer.mode(),
            config: renderer.config(),
            backing_viewport: backing_viewport(renderer)?,
        })
    }
}

#[derive(Debug, Error)]
pub(crate) enum ShadowAdapterError {
    #[error("legacy offscreen renderer failed: {0}")]
    Renderer(#[from] RendererError),
    #[error("resident scene encoding failed: {0}")]
    Resident(#[from] ResidentSceneError),
    #[error("shadow runtime preparation failed: {0}")]
    Prepare(#[from] PreparedRuntimeError),
    #[error("shadow renderer receipt failed: {0}")]
    Frame(#[from] ShadowFrameError),
    #[error("shadow work is stale or incompatible: {0:?}")]
    Currentness(ShadowCurrentnessError),
    #[error("legacy offscreen oracle is stale or incompatible: {0:?}")]
    LegacyCurrentness(LegacyCurrentnessError),
}

#[derive(Debug)]
pub(crate) struct ShadowPixels {
    stats: FrameStats,
    rgba: Vec<u8>,
}

impl ShadowPixels {
    pub(crate) const fn stats(&self) -> FrameStats {
        self.stats
    }

    pub(crate) fn rgba(&self) -> &[u8] {
        &self.rgba
    }
}

/// Materialize the legacy GPU resource and image before preparing E1 from the
/// same source value. This sequencing avoids borrowing plan work while the
/// runtime slot is inspected or passed back to the adapter.
pub(crate) fn prepare_legacy_oracle_then_shadow(
    renderer: &mut Renderer,
    source: SceneBuffers,
    camera: &Camera,
) -> Result<(PreparedRuntimeSlot, LegacyOracleReceipt, ShadowPixels), ShadowAdapterError> {
    renderer.load_scene(source.clone())?;
    let stats = renderer.render_frame(camera)?;
    let rgba = renderer.readback_rgba8()?;

    let resident = ResidentSceneCpu::encode_owned(source)?;
    let slot = PreparedRuntimeSlot::prepare(resident)?;
    let legacy = LegacyOracleReceipt::capture(renderer, &slot)?;
    Ok((slot, legacy, ShadowPixels { stats, rgba }))
}

/// Validate the complete frame/source/order identity, then inject the already
/// authoritative order through the existing legacy raster and readback path.
pub(crate) fn render_current_shadow_frame(
    renderer: &mut Renderer,
    slot: &PreparedRuntimeSlot,
    legacy: &LegacyOracleReceipt,
    frame: &ShadowFrame,
    camera: &Camera,
    viewport: Viewport,
) -> Result<ShadowPixels, ShadowAdapterError> {
    validate_current(renderer, slot, legacy, frame, camera, viewport)?;
    let stats =
        renderer.render_frame_with_external_order_for_test(camera, frame.cpu_order_ids())?;
    let rgba = renderer.readback_rgba8()?;
    Ok(ShadowPixels { stats, rgba })
}

fn validate_current(
    renderer: &Renderer,
    slot: &PreparedRuntimeSlot,
    legacy: &LegacyOracleReceipt,
    frame: &ShadowFrame,
    camera: &Camera,
    viewport: Viewport,
) -> Result<(), ShadowAdapterError> {
    validate_current_shadow_frame(slot, frame, camera, viewport)
        .map_err(ShadowAdapterError::Currentness)?;
    if PositionsOwnerIdentity::from_slice(slot.scene().positions()) != legacy.shadow_positions
        || slot.frame_state().identity().scene_generation() != legacy.shadow_scene_generation
    {
        return Err(ShadowAdapterError::LegacyCurrentness(
            LegacyCurrentnessError::ShadowSceneIdentity,
        ));
    }
    if PositionsOwnerIdentity::capture(renderer)? != legacy.positions {
        return Err(ShadowAdapterError::LegacyCurrentness(
            LegacyCurrentnessError::PositionsOwner,
        ));
    }
    if renderer.geometry_path() != legacy.geometry_path {
        return Err(ShadowAdapterError::LegacyCurrentness(
            LegacyCurrentnessError::GeometryPath,
        ));
    }
    if renderer.mode() != legacy.mode {
        return Err(ShadowAdapterError::LegacyCurrentness(
            LegacyCurrentnessError::RenderMode,
        ));
    }
    if renderer.config() != legacy.config {
        return Err(ShadowAdapterError::LegacyCurrentness(
            LegacyCurrentnessError::RendererConfig,
        ));
    }
    let current_backing = backing_viewport(renderer)?;
    if current_backing != legacy.backing_viewport
        || current_backing != (viewport.width(), viewport.height())
    {
        return Err(ShadowAdapterError::LegacyCurrentness(
            LegacyCurrentnessError::BackingViewport,
        ));
    }
    if renderer.scene_len() != Some(frame.source_count() as usize)
        || renderer.scene_sh_degree() != Some(frame.sh_degree())
    {
        return Err(ShadowAdapterError::Currentness(
            ShadowCurrentnessError::SceneContract,
        ));
    }
    Ok(())
}

fn backing_viewport(renderer: &Renderer) -> Result<(u32, u32), RendererError> {
    renderer
        .gpu_rasterizer
        .as_ref()
        .map(|rasterizer| rasterizer.offscreen_target.size())
        .ok_or(RendererError::GpuRasterizerUnavailable)
}

#[cfg(test)]
mod tests {
    use gsplat_core::{Camera, RenderMode, RendererConfig, SceneBuffers, Vec3f};

    use super::{
        LegacyCurrentnessError, ShadowAdapterError, prepare_legacy_oracle_then_shadow,
        render_current_shadow_frame,
    };
    use crate::plans::{OrderLane, PlanId, WorkUnavailable};
    use crate::renderer::PreparedRuntimeSlot;
    use crate::renderer::frame::Viewport;
    use crate::renderer::tests::{ShadowCurrentnessError, capture_shadow_frame};
    use crate::scene::ResidentSceneCpu;
    use crate::{GeometryPath, Renderer, RendererError};

    fn config() -> RendererConfig {
        RendererConfig {
            width: 96,
            height: 72,
            mode: RenderMode::SortedAlpha,
        }
    }

    fn scene(degree: u8, count: usize) -> SceneBuffers {
        let rest_per_point = (((usize::from(degree) + 1).pow(2)) - 1) * 3;
        SceneBuffers {
            positions: (0..count)
                .map(|index| {
                    Vec3f::new(
                        (index as f32 - count as f32 * 0.5) * 0.025,
                        ((index % 7) as f32 - 3.0) * 0.025,
                        1.25 + (index % 11) as f32 * 0.035,
                    )
                })
                .collect(),
            opacity: vec![1.75; count],
            scale_xyz: vec![[-3.4, -3.2, -3.0]; count],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; count],
            color_dc: (0..count)
                .map(|index| [0.15, -0.1 + (index % 5) as f32 * 0.025, 0.2])
                .collect(),
            sh_degree: degree,
            sh_rest: (degree > 0).then(|| {
                (0..count * rest_per_point)
                    .map(|index| ((index % 17) as f32 - 8.0) * 0.01)
                    .collect()
            }),
        }
    }

    fn renderer_or_skip(label: &str) -> Option<Renderer> {
        let require_gpu = matches!(
            std::env::var("GSPLAT_REQUIRE_GPU_CONFORMANCE").as_deref(),
            Ok("1")
        );
        match Renderer::with_config(config()) {
            Ok(renderer) => Some(renderer),
            Err(
                error
                @ (RendererError::GpuRasterizerUnavailable | RendererError::GpuDeviceCreation),
            ) if !require_gpu => {
                eprintln!("skipping {label}; adapter unavailable: {error}");
                None
            }
            Err(error) => panic!("{label} requires an offscreen adapter: {error}"),
        }
    }

    fn assert_image_unchanged(renderer: &mut Renderer, expected: &[u8], label: &str) {
        assert_eq!(
            renderer.readback_rgba8().expect("read existing image"),
            expected,
            "{label} must not mutate the offscreen target"
        );
    }

    fn invalid_resident(source: SceneBuffers) -> ResidentSceneCpu {
        let mut invalid = ResidentSceneCpu::encode_owned(source).expect("resident encode");
        invalid.sh_degree = 4;
        invalid
    }

    #[test]
    fn direct_and_packed_sh0_through_sh3_normal_images_equal_shadow_bytes() {
        let Some(mut renderer) = renderer_or_skip("E2 shadow image parity") else {
            return;
        };
        let viewport = Viewport::new(config().width, config().height).expect("viewport");
        let camera = Camera::default();

        for path in [GeometryPath::SortedIndexDirect, GeometryPath::PackedAtlas] {
            renderer.set_geometry_path(path);
            for degree in 0..=3 {
                let source = scene(degree, 37);
                let (mut slot, legacy, normal) =
                    prepare_legacy_oracle_then_shadow(&mut renderer, source, &camera)
                        .expect("legacy then shadow preparation");
                let legacy_order = renderer.current_sorted_indices().to_vec();
                let frame = capture_shadow_frame(&mut slot, PlanId::CpuPostSort, &camera, viewport)
                    .expect("shadow frame");

                assert_eq!(frame.plan_id(), PlanId::CpuPostSort);
                assert_eq!(frame.order_lane(), OrderLane::Cpu);
                assert_eq!(frame.source_count(), 37);
                assert_eq!(frame.sh_degree(), degree);
                assert_eq!(frame.visible_count(), Ok(37));
                assert_eq!(
                    frame.contributor_count(),
                    Err(WorkUnavailable::ContributorCount)
                );
                assert_eq!(frame.draw_count(), Err(WorkUnavailable::DrawCount));
                assert_eq!(frame.cpu_order_ids(), legacy_order);
                assert_eq!(frame.frame_identity(), slot.frame_state().identity());
                assert_eq!(normal.stats().visible_count, 37);
                assert_eq!(normal.stats().drawn_count, 37);

                let shadow = render_current_shadow_frame(
                    &mut renderer,
                    &slot,
                    &legacy,
                    &frame,
                    &camera,
                    viewport,
                )
                .expect("shadow raster");
                assert_eq!(shadow.stats().visible_count, 37);
                assert_eq!(shadow.stats().drawn_count, 37);
                assert_eq!(shadow.rgba(), normal.rgba());
                assert!(
                    shadow.rgba().chunks_exact(4).any(|pixel| pixel[3] > 0),
                    "{path:?} SH{degree} must produce covered pixels"
                );
            }
        }
    }

    #[test]
    fn legacy_receipt_rejects_scene_path_mode_and_config_mutations_before_raster() {
        let Some(mut renderer) = renderer_or_skip("E2 legacy receipt currentness") else {
            return;
        };
        renderer.set_geometry_path(GeometryPath::SortedIndexDirect);
        let source = scene(2, 17);
        let camera = Camera::default();
        let viewport = Viewport::new(config().width, config().height).expect("viewport");

        let (mut slot, legacy, normal) =
            prepare_legacy_oracle_then_shadow(&mut renderer, source.clone(), &camera)
                .expect("scene-owner baseline");
        let frame = capture_shadow_frame(&mut slot, PlanId::CpuPostSort, &camera, viewport)
            .expect("scene-owner frame");
        let old_image = normal.rgba().to_vec();
        let mut other_same_contract = source.clone();
        other_same_contract.positions[0].x += 0.5;
        renderer
            .load_scene(other_same_contract)
            .expect("same-count/degree renderer replacement");
        assert!(matches!(
            render_current_shadow_frame(&mut renderer, &slot, &legacy, &frame, &camera, viewport,),
            Err(ShadowAdapterError::LegacyCurrentness(
                LegacyCurrentnessError::PositionsOwner
            ))
        ));
        assert_image_unchanged(&mut renderer, &old_image, "same-contract scene replacement");

        let (mut slot, legacy, normal) =
            prepare_legacy_oracle_then_shadow(&mut renderer, source.clone(), &camera)
                .expect("geometry-path baseline");
        let frame = capture_shadow_frame(&mut slot, PlanId::CpuPostSort, &camera, viewport)
            .expect("geometry-path frame");
        let old_image = normal.rgba().to_vec();
        renderer.set_geometry_path(GeometryPath::PackedAtlas);
        assert!(matches!(
            render_current_shadow_frame(&mut renderer, &slot, &legacy, &frame, &camera, viewport,),
            Err(ShadowAdapterError::LegacyCurrentness(
                LegacyCurrentnessError::GeometryPath
            ))
        ));
        assert_image_unchanged(&mut renderer, &old_image, "geometry-path mutation");

        renderer.set_geometry_path(GeometryPath::SortedIndexDirect);
        let (mut slot, legacy, normal) =
            prepare_legacy_oracle_then_shadow(&mut renderer, source.clone(), &camera)
                .expect("render-mode baseline");
        let frame = capture_shadow_frame(&mut slot, PlanId::CpuPostSort, &camera, viewport)
            .expect("render-mode frame");
        let old_image = normal.rgba().to_vec();
        renderer.set_mode(RenderMode::SortFree);
        assert!(matches!(
            render_current_shadow_frame(&mut renderer, &slot, &legacy, &frame, &camera, viewport,),
            Err(ShadowAdapterError::LegacyCurrentness(
                LegacyCurrentnessError::RenderMode
            ))
        ));
        assert_image_unchanged(&mut renderer, &old_image, "render-mode mutation");

        renderer.set_mode(RenderMode::SortedAlpha);
        let (mut slot, legacy, normal) =
            prepare_legacy_oracle_then_shadow(&mut renderer, source.clone(), &camera)
                .expect("renderer-config baseline");
        let frame = capture_shadow_frame(&mut slot, PlanId::CpuPostSort, &camera, viewport)
            .expect("renderer-config frame");
        let old_image = normal.rgba().to_vec();
        renderer.config.width += 1;
        assert!(matches!(
            render_current_shadow_frame(&mut renderer, &slot, &legacy, &frame, &camera, viewport,),
            Err(ShadowAdapterError::LegacyCurrentness(
                LegacyCurrentnessError::RendererConfig
            ))
        ));
        assert_image_unchanged(&mut renderer, &old_image, "renderer-config mutation");

        renderer.config = config();
        let (mut slot, legacy, _) =
            prepare_legacy_oracle_then_shadow(&mut renderer, source, &camera)
                .expect("backing-viewport baseline");
        let frame = capture_shadow_frame(&mut slot, PlanId::CpuPostSort, &camera, viewport)
            .expect("backing-viewport frame");
        renderer
            .set_size(config().width + 1, config().height)
            .expect("resize backing target");
        renderer.config = config();
        let resized_image = renderer.readback_rgba8().expect("resized target image");
        assert!(matches!(
            render_current_shadow_frame(&mut renderer, &slot, &legacy, &frame, &camera, viewport,),
            Err(ShadowAdapterError::LegacyCurrentness(
                LegacyCurrentnessError::BackingViewport
            ))
        ));
        assert_image_unchanged(&mut renderer, &resized_image, "backing-viewport mutation");
    }

    #[test]
    fn legacy_receipt_rejects_same_contract_shadow_slot_replacement_before_raster() {
        let Some(mut renderer) = renderer_or_skip("E2 shadow slot receipt currentness") else {
            return;
        };
        renderer.set_geometry_path(GeometryPath::SortedIndexDirect);
        let source = scene(2, 17);
        let camera = Camera::default();
        let viewport = Viewport::new(config().width, config().height).expect("viewport");
        let (mut slot, legacy, normal) =
            prepare_legacy_oracle_then_shadow(&mut renderer, source.clone(), &camera)
                .expect("slot replacement baseline");
        let old_image = normal.rgba().to_vec();

        let mut replacement = source;
        replacement.positions.reverse();
        slot.replace(ResidentSceneCpu::encode_owned(replacement).expect("replacement resident"))
            .expect("same-count/degree slot replacement");
        let replacement_frame =
            capture_shadow_frame(&mut slot, PlanId::CpuPostSort, &camera, viewport)
                .expect("replacement frame");
        assert_ne!(
            replacement_frame.cpu_order_ids(),
            renderer.current_sorted_indices(),
            "replacement order must differ from the legacy renderer order"
        );

        assert!(matches!(
            render_current_shadow_frame(
                &mut renderer,
                &slot,
                &legacy,
                &replacement_frame,
                &camera,
                viewport,
            ),
            Err(ShadowAdapterError::LegacyCurrentness(
                LegacyCurrentnessError::ShadowSceneIdentity
            ))
        ));
        assert_image_unchanged(&mut renderer, &old_image, "shadow slot replacement");
    }

    #[test]
    fn empty_scene_replacement_is_content_equivalent_for_legacy_owner_identity() {
        let Some(mut renderer) = renderer_or_skip("E2 empty legacy receipt") else {
            return;
        };
        renderer.set_geometry_path(GeometryPath::SortedIndexDirect);
        let camera = Camera::default();
        let viewport = Viewport::new(config().width, config().height).expect("viewport");
        let (mut slot, legacy, normal) =
            prepare_legacy_oracle_then_shadow(&mut renderer, scene(0, 0), &camera)
                .expect("empty baseline");
        let frame = capture_shadow_frame(&mut slot, PlanId::CpuPostSort, &camera, viewport)
            .expect("empty frame");
        renderer
            .load_scene(scene(0, 0))
            .expect("equivalent empty replacement");

        let shadow =
            render_current_shadow_frame(&mut renderer, &slot, &legacy, &frame, &camera, viewport)
                .expect("empty replacement remains equivalent");
        assert_eq!(shadow.rgba(), normal.rgba());
    }

    #[test]
    fn stale_inputs_and_failed_transactions_preserve_runtime_order_and_image() {
        let Some(mut renderer) = renderer_or_skip("E2 shadow currentness") else {
            return;
        };
        renderer.set_geometry_path(GeometryPath::SortedIndexDirect);
        let source = scene(3, 17);
        let camera = Camera::default();
        let viewport = Viewport::new(config().width, config().height).expect("viewport");
        let (mut slot, legacy, normal) =
            prepare_legacy_oracle_then_shadow(&mut renderer, source.clone(), &camera)
                .expect("legacy then shadow preparation");
        let frame = capture_shadow_frame(&mut slot, PlanId::CpuPostSort, &camera, viewport)
            .expect("shadow frame");
        let rendered =
            render_current_shadow_frame(&mut renderer, &slot, &legacy, &frame, &camera, viewport)
                .expect("initial shadow raster");
        assert_eq!(rendered.rgba(), normal.rgba());
        let old_image = rendered.rgba().to_vec();
        let old_state = slot.frame_state();
        let old_fallback = slot.fallback();
        let old_order = slot
            .last_usable_cpu_order()
            .expect("last usable order")
            .to_vec();

        let mut positioned = camera;
        positioned.pose.position.x = 0.1;
        let half_angle = 0.1_f32;
        let mut rotated = camera;
        rotated.pose.rotation_xyzw = [0.0, half_angle.sin(), 0.0, half_angle.cos()];
        let mut changed_intrinsics = camera;
        changed_intrinsics.intrinsics.vertical_fov_radians *= 0.95;
        for (label, candidate_camera, candidate_viewport) in [
            ("camera position", positioned, viewport),
            ("camera rotation", rotated, viewport),
            ("camera intrinsics", changed_intrinsics, viewport),
            (
                "viewport",
                camera,
                Viewport::new(config().width + 1, config().height).expect("changed viewport"),
            ),
        ] {
            assert!(matches!(
                render_current_shadow_frame(
                    &mut renderer,
                    &slot,
                    &legacy,
                    &frame,
                    &candidate_camera,
                    candidate_viewport,
                ),
                Err(ShadowAdapterError::Currentness(
                    ShadowCurrentnessError::CameraOrViewport
                ))
            ));
            assert_image_unchanged(&mut renderer, &old_image, label);
        }

        let mut invalid_camera = camera;
        invalid_camera.intrinsics.near_plane = 2.0;
        invalid_camera.intrinsics.far_plane = 1.0;
        assert!(
            capture_shadow_frame(&mut slot, PlanId::CpuPostSort, &invalid_camera, viewport,)
                .is_err()
        );
        assert!(capture_shadow_frame(&mut slot, PlanId::GpuPostSort, &camera, viewport,).is_err());
        assert_eq!(slot.frame_state(), old_state);
        assert_eq!(slot.fallback(), old_fallback);
        assert_eq!(slot.last_usable_cpu_order(), Some(old_order.as_slice()));
        assert_image_unchanged(&mut renderer, &old_image, "failed frame");

        assert!(PreparedRuntimeSlot::prepare(invalid_resident(source.clone())).is_err());
        assert_eq!(slot.frame_state(), old_state);
        assert_eq!(slot.fallback(), old_fallback);
        assert_eq!(slot.last_usable_cpu_order(), Some(old_order.as_slice()));
        assert_image_unchanged(&mut renderer, &old_image, "failed prepare");

        assert!(slot.replace(invalid_resident(source.clone())).is_err());
        assert_eq!(slot.frame_state(), old_state);
        assert_eq!(slot.fallback(), old_fallback);
        assert_eq!(slot.last_usable_cpu_order(), Some(old_order.as_slice()));
        let after_failure =
            render_current_shadow_frame(&mut renderer, &slot, &legacy, &frame, &camera, viewport)
                .expect("old runtime remains usable");
        assert_eq!(after_failure.rgba(), old_image);

        let newer = capture_shadow_frame(&mut slot, PlanId::CpuPostSort, &positioned, viewport)
            .expect("new camera order");
        assert!(newer.order_generation() > frame.order_generation());
        assert!(matches!(
            render_current_shadow_frame(&mut renderer, &slot, &legacy, &frame, &camera, viewport,),
            Err(ShadowAdapterError::Currentness(
                ShadowCurrentnessError::FrameIdentity
            ))
        ));
        assert_image_unchanged(&mut renderer, &old_image, "stale order generation");

        let restored = capture_shadow_frame(&mut slot, PlanId::CpuPostSort, &camera, viewport)
            .expect("restored camera order");
        let restored_pixels = render_current_shadow_frame(
            &mut renderer,
            &slot,
            &legacy,
            &restored,
            &camera,
            viewport,
        )
        .expect("restored shadow raster");
        assert_eq!(restored_pixels.rgba(), old_image);

        let mut same_count_scene = source.clone();
        same_count_scene.positions[0].z += 0.25;
        slot.replace(
            ResidentSceneCpu::encode_owned(same_count_scene).expect("same-count resident"),
        )
        .expect("same-count replacement");
        assert!(matches!(
            render_current_shadow_frame(
                &mut renderer,
                &slot,
                &legacy,
                &restored,
                &camera,
                viewport,
            ),
            Err(ShadowAdapterError::Currentness(
                ShadowCurrentnessError::FrameIdentity
            ))
        ));
        assert_image_unchanged(&mut renderer, &old_image, "scene replacement");

        let same_count_frame =
            capture_shadow_frame(&mut slot, PlanId::CpuPostSort, &camera, viewport)
                .expect("same-count replacement frame");
        slot.replace(ResidentSceneCpu::encode_owned(scene(3, 18)).expect("changed-count resident"))
            .expect("changed-count replacement");
        assert!(matches!(
            render_current_shadow_frame(
                &mut renderer,
                &slot,
                &legacy,
                &same_count_frame,
                &camera,
                viewport,
            ),
            Err(ShadowAdapterError::Currentness(
                ShadowCurrentnessError::FrameIdentity
            ))
        ));
        assert_image_unchanged(&mut renderer, &old_image, "count replacement");
    }
}
