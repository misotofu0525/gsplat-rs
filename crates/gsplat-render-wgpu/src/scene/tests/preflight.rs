use super::super::{
    DirectSceneError, DirectScenePath, DirectSceneRemediation, DirectSceneResource,
    PackedScenePath, PackedScenePreflightFailure, RESIDENT_COLOR_STORAGE_BINDINGS,
    direct_scene_preflight, packed_scene_preflight, packed_scene_preflight_with_limits,
    resident_sh_plane_count,
};

fn limits_with_storage_binding_limit(bytes: u32) -> wgpu::Limits {
    let mut limits = wgpu::Limits::downlevel_defaults();
    limits.max_storage_buffer_binding_size = bytes;
    limits.max_buffer_size = u64::from(bytes);
    limits.max_storage_buffers_per_shader_stage = RESIDENT_COLOR_STORAGE_BINDINGS;
    limits
}

#[test]
fn direct_scene_preflight_accounts_for_empty_scene_fallback_buffers() {
    let report =
        direct_scene_preflight(0, 0, &limits_with_storage_binding_limit(128 * 1024 * 1024))
            .unwrap();

    assert_eq!(report.path, DirectScenePath::Direct);
    assert_eq!(report.requirements[0].required_bytes, 4);
    assert_eq!(report.requirements[1].required_bytes, 64);
    assert_eq!(report.requirements[2].required_bytes, 4);
}

#[test]
fn direct_scene_preflight_enforces_source_binding_boundary() {
    let limits = limits_with_storage_binding_limit(128 * 1024 * 1024);
    let at_limit = direct_scene_preflight(2_097_152, 0, &limits).unwrap();
    let above_limit = direct_scene_preflight(2_097_153, 0, &limits).unwrap();

    assert_eq!(at_limit.path, DirectScenePath::Direct);
    assert_eq!(at_limit.limiting_resource, DirectSceneResource::Source);
    assert_eq!(above_limit.path, DirectScenePath::ActiveAtlasRequired);
    assert_eq!(above_limit.requirements[1].required_bytes, 134_217_792);
    assert_eq!(
        above_limit.remediation,
        DirectSceneRemediation::UseActiveAtlasOrReduce {
            max_direct_splats: 2_097_152,
        }
    );
}

#[test]
fn direct_scene_preflight_enforces_degree_three_sh_boundary() {
    let limits = limits_with_storage_binding_limit(128 * 1024 * 1024);
    let at_limit = direct_scene_preflight(745_654, 3, &limits).unwrap();
    let above_limit = direct_scene_preflight(745_655, 3, &limits).unwrap();

    assert_eq!(at_limit.path, DirectScenePath::Direct);
    assert_eq!(at_limit.limiting_resource, DirectSceneResource::ShRest);
    assert_eq!(above_limit.path, DirectScenePath::ActiveAtlasRequired);
    assert_eq!(above_limit.requirements[2].required_bytes, 134_217_900);
}

#[test]
fn direct_scene_preflight_reports_nandi_without_allocating_scene_data() {
    let limits = limits_with_storage_binding_limit(128 * 1024 * 1024);
    let dc = direct_scene_preflight(3_454_040, 0, &limits).unwrap();
    let degree_three = direct_scene_preflight(3_454_040, 3, &limits).unwrap();

    assert_eq!(dc.path, DirectScenePath::ActiveAtlasRequired);
    assert_eq!(dc.limiting_resource, DirectSceneResource::Source);
    assert_eq!(dc.requirements[1].required_bytes, 221_058_560);
    assert_eq!(degree_three.path, DirectScenePath::ActiveAtlasRequired);
    assert_eq!(degree_three.limiting_resource, DirectSceneResource::ShRest);
    assert_eq!(degree_three.requirements[2].required_bytes, 621_727_200);
    assert!(!degree_three.requirements[1].fits);
    assert!(!degree_three.requirements[2].fits);
}

#[test]
fn packed_scene_preflight_reports_final_resident_planes_for_nandi() {
    let limits = limits_with_storage_binding_limit(128 * 1024 * 1024);
    let kitsune = packed_scene_preflight_with_limits(279_199, 3, &limits).unwrap();
    let nandi = packed_scene_preflight_with_limits(3_454_040, 3, &limits).unwrap();
    let direct_nandi = direct_scene_preflight(3_454_040, 3, &limits).unwrap();

    // Direct Nandi fails because SH rest alone exceeds the storage binding.
    assert_eq!(direct_nandi.path, DirectScenePath::ActiveAtlasRequired);
    assert_eq!(direct_nandi.limiting_resource, DirectSceneResource::ShRest);
    assert!(!direct_nandi.requirements[2].fits);

    // Production Packed stores the complete scene in bounded resident
    // planes. This report must never describe the removed 20 B hot record.
    assert!(!kitsune.attributes_avoid_storage_binding);
    assert!(!nandi.attributes_avoid_storage_binding);
    assert!(kitsune.hot_record_fits_storage_binding);
    assert!(nandi.hot_record_fits_storage_binding);
    assert_eq!(nandi.resident_gpu.position_alpha, 3_454_040_u64 * 16);
    assert_eq!(nandi.resident_gpu.covariance0, 3_454_040_u64 * 16);
    assert_eq!(nandi.resident_gpu.covariance1, 3_454_040_u64 * 8);
    assert_eq!(nandi.resident_gpu.color_auxiliary, 3_454_040_u64 * 8);
    assert_eq!(nandi.resident_gpu.sh_plane, 3_454_040_u64 * 16);
    assert_eq!(
        nandi.resident_gpu.sh_plane_count,
        resident_sh_plane_count(3) as u32
    );
    assert_eq!(nandi.resident_gpu.resolved_color, 3_454_040_u64 * 8);
    assert_eq!(nandi.resident_gpu.order, 3_454_040_u64 * 4);
    assert_eq!(
        nandi.hot_record_storage_bytes,
        3_454_040_u64 * 16,
        "compatibility alias must report the largest resident plane"
    );
    assert!(kitsune.sorted_indices_fits_storage_binding);
    assert!(nandi.sorted_indices_fits_storage_binding);
    assert_eq!(kitsune.path, PackedScenePath::PackedAtlas);
    assert_eq!(nandi.path, PackedScenePath::PackedAtlas);
    assert_eq!(
        nandi.declared_attribute_resource_bytes,
        nandi.resident_gpu.total_static - nandi.resident_gpu.order
    );
    assert!(
        nandi.largest_storage_binding_bytes < direct_nandi.requirements[2].required_bytes,
        "split resident planes must remove Direct's monolithic SH binding failure"
    );
}

#[test]
fn packed_scene_preflight_accepts_kitsune_and_flowers_on_mobile_binding_limits() {
    let limits = limits_with_storage_binding_limit(128 * 1024 * 1024);
    for count in [279_199_usize, 562_974_usize] {
        let report = packed_scene_preflight_with_limits(count, 3, &limits).unwrap();
        assert_eq!(report.path, PackedScenePath::PackedAtlas);
        assert!(!report.attributes_avoid_storage_binding);
        assert!(report.hot_record_fits_storage_binding);
        assert!(report.sorted_indices_fits_storage_binding);
    }
}

#[test]
fn packed_scene_preflight_accepts_exact_128_mib_plane_boundary() {
    let limits = limits_with_storage_binding_limit(128 << 20);
    let report = packed_scene_preflight_with_limits(8_388_608, 3, &limits).unwrap();

    assert_eq!(report.path, PackedScenePath::PackedAtlas);
    assert_eq!(report.resident_gpu.position_alpha, 128 << 20);
    assert_eq!(report.resident_gpu.covariance0, 128 << 20);
    assert_eq!(report.resident_gpu.sh_plane, 128 << 20);
    assert_eq!(
        report.resident_gpu.sh_plane_count,
        resident_sh_plane_count(3) as u32
    );
    assert_eq!(report.largest_storage_binding_bytes, 128 << 20);
    assert_eq!(report.failure, None);
}

#[test]
fn packed_scene_preflight_rejects_one_splat_above_128_mib_plane_boundary() {
    let limits = limits_with_storage_binding_limit(128 << 20);
    let report = packed_scene_preflight_with_limits(8_388_609, 3, &limits).unwrap();

    assert_eq!(report.path, PackedScenePath::PagingRequired);
    assert_eq!(report.largest_storage_binding_bytes, (128 << 20) + 16);
    assert_eq!(
        report.failure,
        Some(PackedScenePreflightFailure::StorageBindingSize {
            required_bytes: (128 << 20) + 16,
            limit_bytes: 128 << 20,
        })
    );
}

#[test]
fn packed_scene_preflight_uses_max_buffer_size_when_it_is_smaller() {
    let mut limits = limits_with_storage_binding_limit(256 << 20);
    limits.max_buffer_size = (128 << 20) - 1;
    let report = packed_scene_preflight_with_limits(8_388_608, 3, &limits).unwrap();

    assert_eq!(report.effective_storage_binding_limit, (128 << 20) - 1);
    assert_eq!(report.path, PackedScenePath::PagingRequired);
    assert_eq!(
        report.failure,
        Some(PackedScenePreflightFailure::StorageBindingSize {
            required_bytes: 128 << 20,
            limit_bytes: (128 << 20) - 1,
        })
    );
}

#[test]
fn packed_scene_preflight_rejects_fewer_than_eight_storage_bindings() {
    let mut limits = limits_with_storage_binding_limit(128 << 20);
    limits.max_storage_buffers_per_shader_stage = 7;
    let report = packed_scene_preflight_with_limits(1, 3, &limits).unwrap();

    assert!(report.storage_binding_size_fits);
    assert!(!report.storage_binding_count_fits);
    assert_eq!(report.path, PackedScenePath::PagingRequired);
    assert_eq!(
        report.failure,
        Some(PackedScenePreflightFailure::StorageBindingCount {
            required: 8,
            available: 7,
        })
    );
}

#[test]
fn packed_scene_preflight_rejects_unsupported_sh_degree() {
    let limits = limits_with_storage_binding_limit(128 << 20);
    let report = packed_scene_preflight_with_limits(1, 4, &limits).unwrap();

    assert_eq!(report.path, PackedScenePath::PagingRequired);
    assert_eq!(
        report.failure,
        Some(PackedScenePreflightFailure::UnsupportedShDegree {
            requested: 4,
            maximum: 3,
        })
    );
}

#[test]
fn packed_scene_preflight_retains_u64_limit_call_compatibility() {
    let report = packed_scene_preflight(8_388_608, 3, 128 << 20).unwrap();

    assert_eq!(report.path, PackedScenePath::PackedAtlas);
    assert_eq!(report.available_storage_buffers_per_shader_stage, 8);
    assert_eq!(report.effective_storage_binding_limit, 128 << 20);
}

#[cfg(target_pointer_width = "64")]
#[test]
fn direct_scene_preflight_rejects_more_than_u32_draw_instances() {
    let count = usize::try_from(u64::from(u32::MAX) + 1).unwrap();
    let report = direct_scene_preflight(count, 0, &wgpu::Limits::default()).unwrap();

    assert_eq!(report.path, DirectScenePath::ActiveAtlasRequired);
    assert!(report.splat_count > u64::from(u32::MAX));
}

#[cfg(target_pointer_width = "64")]
#[test]
fn direct_scene_preflight_rejects_byte_arithmetic_overflow() {
    let error = direct_scene_preflight(usize::MAX, 3, &wgpu::Limits::default()).unwrap_err();

    assert_eq!(error, DirectSceneError::ResourceSizeOverflow);
}
