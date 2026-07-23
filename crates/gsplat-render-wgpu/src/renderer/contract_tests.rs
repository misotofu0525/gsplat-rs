use gsplat_core::{Camera, RenderMode, SceneBuffers, Vec3f};

use super::{PlanId, PreparedRuntimeSlot, execute_frame};
use crate::Renderer;
use crate::plans::{OrderLane, WorkUnavailable};
use crate::renderer::frame::Viewport;
use crate::scene::ResidentSceneCpu;

fn scene_with_positions(degree: u8, positions: Vec<Vec3f>) -> SceneBuffers {
    let count = positions.len();
    let rest_per_point = (((usize::from(degree) + 1).pow(2)) - 1) * 3;
    let sh_rest = (degree > 0).then(|| {
        (0..count * rest_per_point)
            .map(|index| ((index % 19) as f32 - 9.0) * 0.0125)
            .collect()
    });
    SceneBuffers {
        positions,
        opacity: vec![1.5; count],
        scale_xyz: vec![[-3.5, -3.25, -3.0]; count],
        rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; count],
        color_dc: (0..count)
            .map(|index| [0.1 + index as f32 * 0.001, -0.05, 0.2])
            .collect(),
        sh_degree: degree,
        sh_rest,
    }
}

fn camera(near_plane: f32, far_plane: f32) -> Camera {
    let mut camera = Camera::default();
    camera.intrinsics.near_plane = near_plane;
    camera.intrinsics.far_plane = far_plane;
    camera
}

fn assert_legacy_shadow_contract(scene: SceneBuffers, camera: Camera) -> Vec<u32> {
    let source_count = scene.len();
    let source_degree = scene.sh_degree;

    let mut legacy = Renderer::new_for_surface(RenderMode::SortedAlpha).expect("legacy renderer");
    legacy.load_scene(scene.clone()).expect("legacy scene");
    let (legacy_ids, legacy_stats) = legacy
        .build_sorted_indices(&camera)
        .expect("legacy authoritative order");

    let resident = ResidentSceneCpu::encode_owned(scene).expect("resident encode");
    assert_eq!(resident.report.source_count, source_count);
    assert_eq!(resident.report.encoded_count, source_count);
    assert_eq!(resident.len(), source_count);
    assert_eq!(resident.sh_degree, source_degree);

    let mut slot = PreparedRuntimeSlot::prepare(resident).expect("prepared runtime");
    assert_eq!(slot.scene().source_count(), source_count);
    assert_eq!(slot.scene().resident().len(), source_count);
    assert_eq!(slot.scene().sh_degree(), source_degree);
    assert_eq!(slot.scene().resident().sh_degree, source_degree);

    let viewport = Viewport::new(641, 479).expect("viewport");
    let (shadow_ids, frame_identity, order_generation) = {
        let work = execute_frame(&mut slot, PlanId::CpuPostSort, &camera, viewport)
            .expect("shadow CPU plan");
        assert_eq!(work.plan_id(), PlanId::CpuPostSort);
        assert_eq!(work.order_lane(), OrderLane::Cpu);
        assert_eq!(work.source_count() as usize, source_count);
        assert_eq!(work.visible_count(), Ok(legacy_stats.visible_count));
        assert_eq!(
            work.contributor_count(),
            Err(WorkUnavailable::ContributorCount)
        );
        assert_eq!(work.draw_count(), Err(WorkUnavailable::DrawCount));
        (
            work.cpu_order_ids()
                .expect("authoritative CPU IDs")
                .to_vec(),
            work.frame_identity(),
            work.order_generation(),
        )
    };

    assert_eq!(shadow_ids, legacy_ids);
    assert_eq!(slot.frame_state().identity(), frame_identity);
    assert_eq!(frame_identity.scene_generation(), 1);
    assert_eq!(frame_identity.camera_revision(), 1);
    assert_eq!(frame_identity.viewport_generation(), 1);
    assert_eq!(frame_identity.contract_generation(), 1);
    assert_eq!(frame_identity.plan_set_generation(), 1);
    assert_eq!(order_generation, 1);
    assert_eq!(slot.last_usable_cpu_order(), Some(shadow_ids.as_slice()));
    shadow_ids
}

#[test]
fn sh0_through_sh3_preserve_source_resident_work_count_degree_and_order() {
    for degree in 0..=3 {
        let positions = (0..19)
            .map(|index| {
                Vec3f::new(
                    (index as f32 - 9.0) * 0.025,
                    ((index % 5) as f32 - 2.0) * 0.02,
                    1.0 + (index % 7) as f32 * 0.125,
                )
            })
            .collect();
        let ids = assert_legacy_shadow_contract(
            scene_with_positions(degree, positions),
            camera(0.5, 4.0),
        );
        assert_eq!(ids.len(), 19, "SH{degree} must retain every source point");
    }
}

#[test]
fn empty_single_and_non_aligned_tail_match_legacy_element_for_element() {
    let empty =
        assert_legacy_shadow_contract(scene_with_positions(0, Vec::new()), camera(0.5, 4.0));
    assert!(empty.is_empty());

    let single = assert_legacy_shadow_contract(
        scene_with_positions(0, vec![Vec3f::new(0.0, 0.0, 2.0)]),
        camera(0.5, 4.0),
    );
    assert_eq!(single, [0]);

    let tail_count = 257;
    let tail = assert_legacy_shadow_contract(
        scene_with_positions(
            0,
            (0..tail_count)
                .map(|index| Vec3f::new(0.0, 0.0, 1.0 + (index % 23) as f32 * 0.01))
                .collect(),
        ),
        camera(0.5, 4.0),
    );
    assert_eq!(tail.len(), tail_count);
    let mut membership = tail;
    membership.sort_unstable();
    assert_eq!(membership, (0..tail_count as u32).collect::<Vec<_>>());
}

#[test]
fn equal_and_repeated_depths_use_source_id_ties_after_full32_depth_order() {
    let equal = assert_legacy_shadow_contract(
        scene_with_positions(0, vec![Vec3f::new(0.0, 0.0, 2.0); 73]),
        camera(0.5, 4.0),
    );
    assert_eq!(equal, (0..73).collect::<Vec<_>>());

    let below_two = f32::from_bits(2.0_f32.to_bits() - 1);
    let above_two = f32::from_bits(2.0_f32.to_bits() + 1);
    let repeated_depths = [2.0, 3.0, 2.0, below_two, above_two, 1.0, 3.0];
    let repeated = assert_legacy_shadow_contract(
        scene_with_positions(
            0,
            repeated_depths
                .into_iter()
                .map(|depth| Vec3f::new(0.0, 0.0, depth))
                .collect(),
        ),
        camera(0.5, 4.0),
    );
    assert_eq!(repeated, [1, 6, 4, 0, 2, 3, 5]);
}

#[test]
fn near_far_are_inclusive_and_behind_camera_is_excluded() {
    let below_near = f32::from_bits(1.0_f32.to_bits() - 1);
    let above_far = f32::from_bits(3.0_f32.to_bits() + 1);
    let ids = assert_legacy_shadow_contract(
        scene_with_positions(
            0,
            [1.0, below_near, 3.0, above_far, -1.0, 2.0]
                .into_iter()
                .map(|depth| Vec3f::new(0.0, 0.0, depth))
                .collect(),
        ),
        camera(1.0, 3.0),
    );
    assert_eq!(ids, [2, 5, 0]);
}
