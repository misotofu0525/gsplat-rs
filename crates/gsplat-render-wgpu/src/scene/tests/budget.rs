use crate::data::{RESIDENT_CHUNK_SPLATS, RESIDENT_SH_PLANES};
use crate::gpu_error::ResidentGpuError;

#[cfg(feature = "diagnostic-resident-sh-mantissa8")]
use super::super::ResidentCpuByteAccounting;
use super::super::{
    PROJECT_WORKGROUP_SIZE, RESIDENT_COLOR_STORAGE_BINDINGS, ResidentGpuBytePlan,
    resident_sh_plane_count,
};

#[test]
fn byte_plan_is_degree_specific_and_exact_count() {
    let n = 2_541_226_u64;
    let sh_plane_count = resident_sh_plane_count(3) as u32;
    let plan = ResidentGpuBytePlan::for_count(n as usize, sh_plane_count).expect("plan");
    assert_eq!(plan.position_alpha, 16 * n);
    assert_eq!(plan.covariance0, 16 * n);
    assert_eq!(plan.covariance1, 8 * n);
    assert_eq!(plan.color_auxiliary, 8 * n);
    assert_eq!(plan.sh_plane, 16 * n);
    assert_eq!(plan.sh_plane_count, sh_plane_count);
    assert_eq!(plan.resolved_color, 8 * n);
    assert_eq!(plan.order, 4 * n);
    assert_eq!(plan.projected_center_source, 16 * n);
    assert_eq!(plan.projected_axes, 16 * n);
    assert_eq!(plan.projected_contributor_ranks, 4 * n);
    assert_eq!(
        plan.projected_contributor_group_offsets,
        (n.div_ceil(u64::from(PROJECT_WORKGROUP_SIZE)) + 1) * 4
    );
    assert!(plan.projected_contributor_scan_sums > 0);
    assert!(plan.projected_contributor_largest_scan_sum > 0);
    assert!(plan.projected_contributor_scan_params >= 256);
    assert_eq!(plan.projected_contributor_args, 16);
    assert_eq!(
        plan.chunk_metadata,
        80 * n.div_ceil(RESIDENT_CHUNK_SPLATS as u64)
    );
    assert_eq!(
        plan.total_static,
        (16 + 16 + 8 + 8 + u64::from(sh_plane_count) * 16 + 8 + 4 + 16 + 16) * n
            + 80 * n.div_ceil(RESIDENT_CHUNK_SPLATS as u64)
            + plan.projected_contributor_group_offsets
            + plan.projected_contributor_ranks
            + plan.projected_contributor_scan_sums
            + plan.projected_contributor_scan_params
            + plan.projected_contributor_args
    );
}

#[test]
fn byte_plan_rejects_non_degree_sh_plane_counts() {
    #[cfg(not(feature = "diagnostic-resident-sh-mantissa8"))]
    assert_eq!(
        ResidentGpuBytePlan::for_count(1, 2),
        Err(ResidentGpuError::UnsupportedShPlaneCount(2))
    );
    #[cfg(feature = "diagnostic-resident-sh-mantissa8")]
    assert_eq!(
        ResidentGpuBytePlan::for_count(1, 4),
        Err(ResidentGpuError::UnsupportedShPlaneCount(4))
    );
    assert_eq!(
        ResidentGpuBytePlan::for_count(1, RESIDENT_SH_PLANES as u32 + 1),
        Err(ResidentGpuError::UnsupportedShPlaneCount(5))
    );
}

#[test]
#[cfg(feature = "diagnostic-resident-sh-mantissa8")]
fn sh8_resource_ledger_proves_logical_and_allocated_sh3_savings() {
    let count = 2_541_226_usize;
    let n = count as u64;
    let candidate_planes = resident_sh_plane_count(3) as u32;
    let gpu = ResidentGpuBytePlan::for_count(count, candidate_planes).expect("SH8 GPU plan");
    let cpu = ResidentCpuByteAccounting::for_count(count, 3).expect("SH8 CPU plan");
    let exact_signed11_staging = 112 * n + 80 * n.div_ceil(RESIDENT_CHUNK_SPLATS as u64);
    let exact_signed11_gpu_static = gpu.total_static + 16 * n;

    assert_eq!(candidate_planes, 3);
    assert_eq!(cpu.upload_staging_bytes, exact_signed11_staging - 16 * n);
    assert_eq!(gpu.total_static, exact_signed11_gpu_static - 16 * n);
    assert_eq!(gpu.allocated_sh_buffer_bytes(), 48 * n + 16);
    assert_eq!(64 * n - gpu.allocated_sh_buffer_bytes(), 16 * n - 16);
}

#[test]
fn portable_128_mib_binding_admits_known_six_million_scene() {
    let plan =
        ResidentGpuBytePlan::for_count(6_132_000, resident_sh_plane_count(3) as u32).expect("plan");
    let limit = 128_u64 << 20;
    assert!(plan.position_alpha < limit);
    assert!(plan.sh_plane < limit);
    assert!(plan.covariance0 < limit);
    assert!(plan.covariance1 < limit);
    assert!(plan.color_auxiliary < limit);
    assert!(plan.projected_contributor_ranks < limit);
}

fn portable_resident_limits() -> wgpu::Limits {
    let mut limits = wgpu::Limits::downlevel_defaults();
    limits.max_storage_buffer_binding_size = 128 << 20;
    limits.max_buffer_size = 128 << 20;
    limits.max_storage_buffers_per_shader_stage = RESIDENT_COLOR_STORAGE_BINDINGS;
    limits
}

#[test]
fn portable_binding_boundary_is_exactly_8_388_608_splats() {
    let limits = portable_resident_limits();
    let sh_plane_count = resident_sh_plane_count(3) as u32;
    let at_limit =
        ResidentGpuBytePlan::for_count(8_388_608, sh_plane_count).expect("boundary plan");
    let above_limit =
        ResidentGpuBytePlan::for_count(8_388_609, sh_plane_count).expect("overflow plan");

    assert_eq!(at_limit.position_alpha, 128 << 20);
    assert_eq!(at_limit.covariance0, 128 << 20);
    assert_eq!(at_limit.sh_plane, 128 << 20);
    assert_eq!(at_limit.largest_storage_binding_bytes(), 128 << 20);
    assert_eq!(at_limit.validate_limits(&limits), Ok(at_limit));
    assert_eq!(
        above_limit.largest_storage_binding_bytes(),
        (128 << 20) + 16
    );
    assert_eq!(
        above_limit.validate_limits(&limits),
        Err(ResidentGpuError::BindingLimitExceeded {
            resource: "position+alpha",
            required_bytes: (128 << 20) + 16,
            limit_bytes: 128 << 20,
        })
    );
}

#[test]
fn resident_limit_validation_uses_smaller_max_buffer_size() {
    let plan =
        ResidentGpuBytePlan::for_count(8_388_608, resident_sh_plane_count(3) as u32).expect("plan");
    let mut limits = portable_resident_limits();
    limits.max_storage_buffer_binding_size = 256 << 20;
    limits.max_buffer_size = (128 << 20) - 1;

    assert_eq!(
        plan.validate_limits(&limits),
        Err(ResidentGpuError::BindingLimitExceeded {
            resource: "position+alpha",
            required_bytes: 128 << 20,
            limit_bytes: (128 << 20) - 1,
        })
    );
}

#[test]
fn resident_limit_validation_requires_eight_storage_bindings() {
    let plan = ResidentGpuBytePlan::for_count(1, resident_sh_plane_count(3) as u32).expect("plan");
    let mut limits = portable_resident_limits();
    limits.max_storage_buffers_per_shader_stage = RESIDENT_COLOR_STORAGE_BINDINGS - 1;

    assert_eq!(
        plan.validate_limits(&limits),
        Err(ResidentGpuError::StorageBindingCountUnsupported(7))
    );
}

#[test]
fn degree_zero_does_not_charge_inactive_sh_planes_per_splat() {
    let plan = ResidentGpuBytePlan::for_count(8_388_608, 0).expect("plan");

    assert_eq!(plan.sh_plane_count, 0);
    assert_eq!(plan.sh_plane, 128 << 20);
    assert_eq!(plan.largest_storage_binding_bytes(), 128 << 20);
    assert_eq!(plan.validate_limits(&portable_resident_limits()), Ok(plan));
}
