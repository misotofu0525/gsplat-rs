use gsplat_render_wgpu::{SurfaceCurrentStatsPoll, SurfaceCurrentStatsTerminal};

use crate::current_stats_v1::{
    GsplatSurfaceCurrentStatsIdentityV1, SURFACE_CURRENT_STATS_COUNT_SEMANTICS_NONE,
    SURFACE_CURRENT_STATS_POLL_DROPPED, SURFACE_CURRENT_STATS_POLL_EMPTY,
    SURFACE_CURRENT_STATS_POLL_EXPIRED, SURFACE_CURRENT_STATS_POLL_GENERATION_INVALIDATED,
    SURFACE_CURRENT_STATS_POLL_MAP_FAILURE, SURFACE_CURRENT_STATS_POLL_READY,
    SURFACE_CURRENT_STATS_POLL_UNSAMPLED, SURFACE_CURRENT_STATS_POLL_UNSPECIFIED,
    SURFACE_CURRENT_STATS_REQUEST_NOT_APPLICABLE, surface_current_stats_count_semantics_to_ffi,
    surface_current_stats_identity_to_ffi, surface_current_stats_request_status_to_ffi,
};

pub(super) const SURFACE_CURRENT_STATS_ABI_VERSION_V2: u32 = 2;
pub(super) const SURFACE_CURRENT_STATS_TIMING_FRAME_COMPLETE_VALID: u32 = 1 << 0;
pub(super) const SURFACE_CURRENT_STATS_TIMING_CPU_PREPROCESS_VALID: u32 = 1 << 1;
pub(super) const SURFACE_CURRENT_STATS_TIMING_CPU_SORT_VALID: u32 = 1 << 2;

/// One atomic current-stats terminal with optional same-ticket timing.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GsplatSurfaceCurrentStatsPollV2 {
    pub struct_size: u32,
    pub version: u32,
    pub kind: u32,
    pub request_status: u32,
    pub count_semantics: u32,
    pub timing_validity_flags: u32,
    pub ticket: u64,
    pub identity: GsplatSurfaceCurrentStatsIdentityV1,
    pub source_count: u32,
    pub visible_count: u32,
    pub contributor_count: u32,
    pub drawn_count: u32,
    pub frame_complete_ms: f32,
    pub cpu_preprocess_ms: f32,
    pub cpu_sort_ms: f32,
    pub reserved: u32,
    pub reserved_u64: [u64; 2],
}

impl Default for GsplatSurfaceCurrentStatsPollV2 {
    fn default() -> Self {
        Self {
            struct_size: std::mem::size_of::<Self>() as u32,
            version: SURFACE_CURRENT_STATS_ABI_VERSION_V2,
            kind: SURFACE_CURRENT_STATS_POLL_UNSPECIFIED,
            request_status: SURFACE_CURRENT_STATS_REQUEST_NOT_APPLICABLE,
            count_semantics: SURFACE_CURRENT_STATS_COUNT_SEMANTICS_NONE,
            timing_validity_flags: 0,
            ticket: 0,
            identity: GsplatSurfaceCurrentStatsIdentityV1::default(),
            source_count: 0,
            visible_count: 0,
            contributor_count: 0,
            drawn_count: 0,
            frame_complete_ms: 0.0,
            cpu_preprocess_ms: 0.0,
            cpu_sort_ms: 0.0,
            reserved: 0,
            reserved_u64: [0; 2],
        }
    }
}

const _: () = {
    assert!(std::mem::size_of::<GsplatSurfaceCurrentStatsPollV2>() == 160);
    assert!(std::mem::align_of::<GsplatSurfaceCurrentStatsPollV2>() == std::mem::align_of::<u64>());
    assert!(std::mem::offset_of!(GsplatSurfaceCurrentStatsPollV2, ticket) == 24);
    assert!(std::mem::offset_of!(GsplatSurfaceCurrentStatsPollV2, identity) == 32);
    assert!(std::mem::offset_of!(GsplatSurfaceCurrentStatsPollV2, source_count) == 112);
    assert!(std::mem::offset_of!(GsplatSurfaceCurrentStatsPollV2, frame_complete_ms) == 128);
    assert!(std::mem::offset_of!(GsplatSurfaceCurrentStatsPollV2, reserved_u64) == 144);
};

pub(super) fn surface_current_stats_poll_to_ffi_v2(
    poll: SurfaceCurrentStatsPoll,
) -> GsplatSurfaceCurrentStatsPollV2 {
    let mut output = GsplatSurfaceCurrentStatsPollV2::default();
    match poll {
        SurfaceCurrentStatsPoll::Empty => {
            output.kind = SURFACE_CURRENT_STATS_POLL_EMPTY;
        }
        SurfaceCurrentStatsPoll::Unsampled(reason) => {
            output.kind = SURFACE_CURRENT_STATS_POLL_UNSAMPLED;
            output.request_status = surface_current_stats_request_status_to_ffi(reason);
        }
        SurfaceCurrentStatsPoll::Terminal(terminal) => {
            let submission = terminal.submission();
            output.ticket = submission.ticket();
            output.identity = surface_current_stats_identity_to_ffi(submission);
            match terminal {
                SurfaceCurrentStatsTerminal::Ready(receipt) => {
                    let counts = receipt.counts();
                    output.kind = SURFACE_CURRENT_STATS_POLL_READY;
                    output.count_semantics =
                        surface_current_stats_count_semantics_to_ffi(receipt.count_semantics());
                    output.source_count = counts.source();
                    output.visible_count = counts.visible();
                    output.contributor_count = counts.contributor();
                    output.drawn_count = counts.drawn();
                    output.frame_complete_ms = receipt.frame_complete_ms();
                    output.timing_validity_flags =
                        SURFACE_CURRENT_STATS_TIMING_FRAME_COMPLETE_VALID;
                    if let Some(value) = receipt.cpu_preprocess_ms() {
                        output.cpu_preprocess_ms = value;
                        output.timing_validity_flags |=
                            SURFACE_CURRENT_STATS_TIMING_CPU_PREPROCESS_VALID;
                    }
                    if let Some(value) = receipt.cpu_sort_ms() {
                        output.cpu_sort_ms = value;
                        output.timing_validity_flags |= SURFACE_CURRENT_STATS_TIMING_CPU_SORT_VALID;
                    }
                }
                SurfaceCurrentStatsTerminal::MapFailure(_) => {
                    output.kind = SURFACE_CURRENT_STATS_POLL_MAP_FAILURE;
                }
                SurfaceCurrentStatsTerminal::GenerationInvalidated(_) => {
                    output.kind = SURFACE_CURRENT_STATS_POLL_GENERATION_INVALIDATED;
                }
                SurfaceCurrentStatsTerminal::Expired(_) => {
                    output.kind = SURFACE_CURRENT_STATS_POLL_EXPIRED;
                }
                SurfaceCurrentStatsTerminal::Dropped(_) => {
                    output.kind = SURFACE_CURRENT_STATS_POLL_DROPPED;
                }
            }
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use std::ptr;

    use gsplat_core::ErrorCode;
    use gsplat_render_wgpu::{SurfaceCurrentStatsPoll, SurfaceCurrentStatsUnsampledReason};

    use super::*;
    use crate::gsplat_surface_renderer_poll_current_stats_v2;

    #[test]
    fn current_stats_v2_layout_is_stable_without_changing_v1() {
        assert_eq!(std::mem::size_of::<GsplatSurfaceCurrentStatsPollV2>(), 160);
        assert_eq!(
            std::mem::offset_of!(GsplatSurfaceCurrentStatsPollV2, ticket),
            24
        );
        assert_eq!(
            std::mem::offset_of!(GsplatSurfaceCurrentStatsPollV2, identity),
            32
        );
        assert_eq!(
            std::mem::offset_of!(GsplatSurfaceCurrentStatsPollV2, source_count),
            112
        );
        assert_eq!(
            std::mem::offset_of!(GsplatSurfaceCurrentStatsPollV2, frame_complete_ms),
            128
        );
        assert_eq!(
            std::mem::offset_of!(GsplatSurfaceCurrentStatsPollV2, reserved_u64),
            144
        );
        assert_eq!(
            std::mem::size_of::<crate::GsplatSurfaceCurrentStatsPollV1>(),
            144
        );
        assert_eq!(
            std::mem::offset_of!(crate::GsplatSurfaceCurrentStatsPollV1, ticket),
            24
        );
        assert_eq!(
            std::mem::offset_of!(crate::GsplatSurfaceCurrentStatsPollV1, identity),
            32
        );
        assert_eq!(
            std::mem::offset_of!(crate::GsplatSurfaceCurrentStatsPollV1, source_count),
            112
        );
    }

    #[test]
    fn current_stats_v2_empty_and_unsampled_zero_inapplicable_timing() {
        for output in [
            surface_current_stats_poll_to_ffi_v2(SurfaceCurrentStatsPoll::Empty),
            surface_current_stats_poll_to_ffi_v2(SurfaceCurrentStatsPoll::Unsampled(
                SurfaceCurrentStatsUnsampledReason::Busy,
            )),
        ] {
            assert_eq!(output.timing_validity_flags, 0);
            assert_eq!(output.frame_complete_ms.to_bits(), 0);
            assert_eq!(output.cpu_preprocess_ms.to_bits(), 0);
            assert_eq!(output.cpu_sort_ms.to_bits(), 0);
            assert_eq!(output.reserved, 0);
            assert_eq!(output.reserved_u64, [0; 2]);
        }
    }

    #[test]
    fn current_stats_v2_rejects_null_size_and_version_without_output_mutation() {
        let invalid = ErrorCode::InvalidArgument.as_i32();
        assert_eq!(
            unsafe {
                gsplat_surface_renderer_poll_current_stats_v2(ptr::null_mut(), ptr::null_mut())
            },
            invalid
        );

        for mut output in [
            GsplatSurfaceCurrentStatsPollV2 {
                version: 99,
                kind: 0xfeed,
                ..Default::default()
            },
            GsplatSurfaceCurrentStatsPollV2 {
                struct_size: std::mem::size_of::<GsplatSurfaceCurrentStatsPollV2>() as u32 - 1,
                kind: 0xfeed,
                ..Default::default()
            },
        ] {
            let before = output;
            assert_eq!(
                unsafe {
                    gsplat_surface_renderer_poll_current_stats_v2(ptr::null_mut(), &mut output)
                },
                invalid
            );
            assert_eq!(output, before);
        }
    }
}
