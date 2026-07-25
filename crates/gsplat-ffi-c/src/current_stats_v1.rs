use gsplat_render_wgpu::{
    SurfaceCurrentStatsCountSemantics, SurfaceCurrentStatsPlan, SurfaceCurrentStatsPoll,
    SurfaceCurrentStatsRequest, SurfaceCurrentStatsSubmission,
    SurfaceCurrentStatsSubmissionReceipt, SurfaceCurrentStatsTerminal,
    SurfaceCurrentStatsUnsampledReason,
};

pub(super) const SURFACE_CURRENT_STATS_ABI_VERSION_V1: u32 = 1;
const SURFACE_CURRENT_STATS_REQUEST_NOT_APPLICABLE: u32 = 0;
const SURFACE_CURRENT_STATS_REQUEST_REQUESTED: u32 = 1;
const SURFACE_CURRENT_STATS_REQUEST_BUSY: u32 = 2;
const SURFACE_CURRENT_STATS_REQUEST_GPU_UNAVAILABLE: u32 = 3;
const SURFACE_CURRENT_STATS_REQUEST_RESOURCE_UNAVAILABLE: u32 = 4;
const SURFACE_CURRENT_STATS_REQUEST_TICKET_EXHAUSTED: u32 = 5;
const SURFACE_CURRENT_STATS_SUBMISSION_UNSPECIFIED: u32 = 0;
const SURFACE_CURRENT_STATS_SUBMISSION_NOT_REQUESTED: u32 = 1;
const SURFACE_CURRENT_STATS_SUBMISSION_ISSUED: u32 = 2;
const SURFACE_CURRENT_STATS_PLAN_NOT_APPLICABLE: u32 = 0;
const SURFACE_CURRENT_STATS_PLAN_CPU_POST_SORT: u32 = 1;
const SURFACE_CURRENT_STATS_PLAN_GPU_POST_SORT: u32 = 2;
const SURFACE_CURRENT_STATS_PLAN_GPU_PREPROJECT: u32 = 3;
const SURFACE_CURRENT_STATS_POLL_UNSPECIFIED: u32 = 0;
const SURFACE_CURRENT_STATS_POLL_EMPTY: u32 = 1;
const SURFACE_CURRENT_STATS_POLL_UNSAMPLED: u32 = 2;
const SURFACE_CURRENT_STATS_POLL_READY: u32 = 3;
const SURFACE_CURRENT_STATS_POLL_MAP_FAILURE: u32 = 4;
const SURFACE_CURRENT_STATS_POLL_GENERATION_INVALIDATED: u32 = 5;
const SURFACE_CURRENT_STATS_POLL_EXPIRED: u32 = 6;
const SURFACE_CURRENT_STATS_POLL_DROPPED: u32 = 7;
const SURFACE_CURRENT_STATS_COUNT_SEMANTICS_NONE: u32 = 0;
const SURFACE_CURRENT_STATS_COUNT_SEMANTICS_DIRECT_DRAW_EQUALS_VISIBLE: u32 = 1;
const SURFACE_CURRENT_STATS_COUNT_SEMANTICS_INDIRECT_DRAW_EQUALS_VISIBLE: u32 = 2;
const SURFACE_CURRENT_STATS_COUNT_SEMANTICS_INDIRECT_DRAW_EQUALS_CONTRIBUTOR: u32 = 3;

/// Complete Renderer-owned join identity shared by current-stats submission
/// and terminal receipts.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GsplatSurfaceCurrentStatsIdentityV1 {
    pub scene_generation: u64,
    pub camera_revision: u64,
    pub viewport_generation: u64,
    pub contract_generation: u64,
    pub plan_set_generation: u64,
    pub order_generation: u64,
    pub raster_generation: u64,
    pub encode_attempt: u64,
    pub presentation_sequence: u64,
    pub executed_plan: u32,
    pub reserved: u32,
}

impl Default for GsplatSurfaceCurrentStatsIdentityV1 {
    fn default() -> Self {
        Self {
            scene_generation: 0,
            camera_revision: 0,
            viewport_generation: 0,
            contract_generation: 0,
            plan_set_generation: 0,
            order_generation: 0,
            raster_generation: 0,
            encode_attempt: 0,
            presentation_sequence: 0,
            executed_plan: SURFACE_CURRENT_STATS_PLAN_NOT_APPLICABLE,
            reserved: 0,
        }
    }
}

/// Immediate result of requesting one current-stats sample.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GsplatSurfaceCurrentStatsRequestV1 {
    pub struct_size: u32,
    pub version: u32,
    pub status: u32,
    pub reserved: u32,
    pub reserved_u64: [u64; 2],
}

impl Default for GsplatSurfaceCurrentStatsRequestV1 {
    fn default() -> Self {
        Self {
            struct_size: std::mem::size_of::<Self>() as u32,
            version: SURFACE_CURRENT_STATS_ABI_VERSION_V1,
            status: SURFACE_CURRENT_STATS_REQUEST_NOT_APPLICABLE,
            reserved: 0,
            reserved_u64: [0; 2],
        }
    }
}

/// Presentation-committed ticket and complete identity for the current frame.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GsplatSurfaceCurrentStatsSubmissionV1 {
    pub struct_size: u32,
    pub version: u32,
    pub status: u32,
    pub reserved: u32,
    pub ticket: u64,
    pub identity: GsplatSurfaceCurrentStatsIdentityV1,
    pub reserved_u64: [u64; 2],
}

impl Default for GsplatSurfaceCurrentStatsSubmissionV1 {
    fn default() -> Self {
        Self {
            struct_size: std::mem::size_of::<Self>() as u32,
            version: SURFACE_CURRENT_STATS_ABI_VERSION_V1,
            status: SURFACE_CURRENT_STATS_SUBMISSION_UNSPECIFIED,
            reserved: 0,
            ticket: 0,
            identity: GsplatSurfaceCurrentStatsIdentityV1::default(),
            reserved_u64: [0; 2],
        }
    }
}

/// One atomic global single-pop current-stats result.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GsplatSurfaceCurrentStatsPollV1 {
    pub struct_size: u32,
    pub version: u32,
    pub kind: u32,
    pub request_status: u32,
    pub count_semantics: u32,
    pub reserved: u32,
    pub ticket: u64,
    pub identity: GsplatSurfaceCurrentStatsIdentityV1,
    pub source_count: u32,
    pub visible_count: u32,
    pub contributor_count: u32,
    pub drawn_count: u32,
    pub reserved_u64: [u64; 2],
}

impl Default for GsplatSurfaceCurrentStatsPollV1 {
    fn default() -> Self {
        Self {
            struct_size: std::mem::size_of::<Self>() as u32,
            version: SURFACE_CURRENT_STATS_ABI_VERSION_V1,
            kind: SURFACE_CURRENT_STATS_POLL_UNSPECIFIED,
            request_status: SURFACE_CURRENT_STATS_REQUEST_NOT_APPLICABLE,
            count_semantics: SURFACE_CURRENT_STATS_COUNT_SEMANTICS_NONE,
            reserved: 0,
            ticket: 0,
            identity: GsplatSurfaceCurrentStatsIdentityV1::default(),
            source_count: 0,
            visible_count: 0,
            contributor_count: 0,
            drawn_count: 0,
            reserved_u64: [0; 2],
        }
    }
}

const _: () = {
    assert!(std::mem::size_of::<GsplatSurfaceCurrentStatsIdentityV1>() == 80);
    assert!(
        std::mem::align_of::<GsplatSurfaceCurrentStatsIdentityV1>() == std::mem::align_of::<u64>()
    );
    assert!(std::mem::offset_of!(GsplatSurfaceCurrentStatsIdentityV1, executed_plan) == 72);
    assert!(std::mem::size_of::<GsplatSurfaceCurrentStatsRequestV1>() == 32);
    assert!(
        std::mem::align_of::<GsplatSurfaceCurrentStatsRequestV1>() == std::mem::align_of::<u64>()
    );
    assert!(std::mem::offset_of!(GsplatSurfaceCurrentStatsRequestV1, status) == 8);
    assert!(std::mem::size_of::<GsplatSurfaceCurrentStatsSubmissionV1>() == 120);
    assert!(
        std::mem::align_of::<GsplatSurfaceCurrentStatsSubmissionV1>()
            == std::mem::align_of::<u64>()
    );
    assert!(std::mem::offset_of!(GsplatSurfaceCurrentStatsSubmissionV1, ticket) == 16);
    assert!(std::mem::offset_of!(GsplatSurfaceCurrentStatsSubmissionV1, identity) == 24);
    assert!(std::mem::size_of::<GsplatSurfaceCurrentStatsPollV1>() == 144);
    assert!(std::mem::align_of::<GsplatSurfaceCurrentStatsPollV1>() == std::mem::align_of::<u64>());
    assert!(std::mem::offset_of!(GsplatSurfaceCurrentStatsPollV1, ticket) == 24);
    assert!(std::mem::offset_of!(GsplatSurfaceCurrentStatsPollV1, identity) == 32);
    assert!(std::mem::offset_of!(GsplatSurfaceCurrentStatsPollV1, source_count) == 112);
};

fn surface_current_stats_request_status_to_ffi(reason: SurfaceCurrentStatsUnsampledReason) -> u32 {
    match reason {
        SurfaceCurrentStatsUnsampledReason::Busy => SURFACE_CURRENT_STATS_REQUEST_BUSY,
        SurfaceCurrentStatsUnsampledReason::GpuUnavailable => {
            SURFACE_CURRENT_STATS_REQUEST_GPU_UNAVAILABLE
        }
        SurfaceCurrentStatsUnsampledReason::ResourceUnavailable => {
            SURFACE_CURRENT_STATS_REQUEST_RESOURCE_UNAVAILABLE
        }
        SurfaceCurrentStatsUnsampledReason::TicketExhausted => {
            SURFACE_CURRENT_STATS_REQUEST_TICKET_EXHAUSTED
        }
    }
}

fn surface_current_stats_plan_to_ffi(plan: SurfaceCurrentStatsPlan) -> u32 {
    match plan {
        SurfaceCurrentStatsPlan::CpuPostSort => SURFACE_CURRENT_STATS_PLAN_CPU_POST_SORT,
        SurfaceCurrentStatsPlan::GpuPostSort => SURFACE_CURRENT_STATS_PLAN_GPU_POST_SORT,
        SurfaceCurrentStatsPlan::GpuPreproject => SURFACE_CURRENT_STATS_PLAN_GPU_PREPROJECT,
    }
}

fn surface_current_stats_count_semantics_to_ffi(
    semantics: SurfaceCurrentStatsCountSemantics,
) -> u32 {
    match semantics {
        SurfaceCurrentStatsCountSemantics::DirectDrawEqualsVisible => {
            SURFACE_CURRENT_STATS_COUNT_SEMANTICS_DIRECT_DRAW_EQUALS_VISIBLE
        }
        SurfaceCurrentStatsCountSemantics::IndirectDrawEqualsVisible => {
            SURFACE_CURRENT_STATS_COUNT_SEMANTICS_INDIRECT_DRAW_EQUALS_VISIBLE
        }
        SurfaceCurrentStatsCountSemantics::IndirectDrawEqualsContributor => {
            SURFACE_CURRENT_STATS_COUNT_SEMANTICS_INDIRECT_DRAW_EQUALS_CONTRIBUTOR
        }
    }
}

fn surface_current_stats_identity_to_ffi(
    submission: SurfaceCurrentStatsSubmissionReceipt,
) -> GsplatSurfaceCurrentStatsIdentityV1 {
    let join = submission.join();
    let frame = join.frame_identity();
    GsplatSurfaceCurrentStatsIdentityV1 {
        scene_generation: frame.scene_generation(),
        camera_revision: frame.camera_revision(),
        viewport_generation: frame.viewport_generation(),
        contract_generation: frame.contract_generation(),
        plan_set_generation: frame.plan_set_generation(),
        order_generation: join.order_generation(),
        raster_generation: join.raster_generation(),
        encode_attempt: join.encode_attempt(),
        presentation_sequence: join.presentation_sequence(),
        executed_plan: surface_current_stats_plan_to_ffi(join.executed_plan()),
        reserved: 0,
    }
}

pub(super) fn surface_current_stats_request_to_ffi(
    request: SurfaceCurrentStatsRequest,
) -> GsplatSurfaceCurrentStatsRequestV1 {
    GsplatSurfaceCurrentStatsRequestV1 {
        status: match request {
            SurfaceCurrentStatsRequest::Requested => SURFACE_CURRENT_STATS_REQUEST_REQUESTED,
            SurfaceCurrentStatsRequest::Unsampled(reason) => {
                surface_current_stats_request_status_to_ffi(reason)
            }
        },
        ..Default::default()
    }
}

pub(super) fn surface_current_stats_submission_to_ffi(
    submission: SurfaceCurrentStatsSubmission,
) -> GsplatSurfaceCurrentStatsSubmissionV1 {
    match submission {
        SurfaceCurrentStatsSubmission::NotRequested => GsplatSurfaceCurrentStatsSubmissionV1 {
            status: SURFACE_CURRENT_STATS_SUBMISSION_NOT_REQUESTED,
            ..Default::default()
        },
        SurfaceCurrentStatsSubmission::Issued(receipt) => GsplatSurfaceCurrentStatsSubmissionV1 {
            status: SURFACE_CURRENT_STATS_SUBMISSION_ISSUED,
            ticket: receipt.ticket(),
            identity: surface_current_stats_identity_to_ffi(receipt),
            ..Default::default()
        },
    }
}

pub(super) fn surface_current_stats_poll_to_ffi(
    poll: SurfaceCurrentStatsPoll,
) -> GsplatSurfaceCurrentStatsPollV1 {
    let mut output = GsplatSurfaceCurrentStatsPollV1::default();
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
    use gsplat_render_wgpu::{
        SurfaceCurrentStatsCountSemantics, SurfaceCurrentStatsPlan, SurfaceCurrentStatsPoll,
        SurfaceCurrentStatsRequest, SurfaceCurrentStatsSubmission,
        SurfaceCurrentStatsUnsampledReason,
    };

    use super::*;
    use crate::{
        gsplat_surface_renderer_get_current_stats_submission_v1,
        gsplat_surface_renderer_poll_current_stats_v1,
        gsplat_surface_renderer_request_current_stats_v1,
    };

    #[test]
    fn current_stats_v1_layout_is_stable() {
        assert_eq!(
            std::mem::size_of::<GsplatSurfaceCurrentStatsIdentityV1>(),
            80
        );
        assert_eq!(
            std::mem::size_of::<GsplatSurfaceCurrentStatsRequestV1>(),
            32
        );
        assert_eq!(
            std::mem::size_of::<GsplatSurfaceCurrentStatsSubmissionV1>(),
            120
        );
        assert_eq!(std::mem::size_of::<GsplatSurfaceCurrentStatsPollV1>(), 144);
        assert_eq!(
            std::mem::offset_of!(GsplatSurfaceCurrentStatsIdentityV1, executed_plan),
            72
        );
        assert_eq!(
            std::mem::offset_of!(GsplatSurfaceCurrentStatsRequestV1, status),
            8
        );
        assert_eq!(
            std::mem::offset_of!(GsplatSurfaceCurrentStatsSubmissionV1, ticket),
            16
        );
        assert_eq!(
            std::mem::offset_of!(GsplatSurfaceCurrentStatsSubmissionV1, identity),
            24
        );
        assert_eq!(
            std::mem::offset_of!(GsplatSurfaceCurrentStatsPollV1, ticket),
            24
        );
        assert_eq!(
            std::mem::offset_of!(GsplatSurfaceCurrentStatsPollV1, identity),
            32
        );
        assert_eq!(
            std::mem::offset_of!(GsplatSurfaceCurrentStatsPollV1, source_count),
            112
        );
        assert_eq!(
            std::mem::align_of::<GsplatSurfaceCurrentStatsIdentityV1>(),
            std::mem::align_of::<u64>()
        );
        assert_eq!(
            std::mem::align_of::<GsplatSurfaceCurrentStatsRequestV1>(),
            std::mem::align_of::<u64>()
        );
        assert_eq!(
            std::mem::align_of::<GsplatSurfaceCurrentStatsSubmissionV1>(),
            std::mem::align_of::<u64>()
        );
        assert_eq!(
            std::mem::align_of::<GsplatSurfaceCurrentStatsPollV1>(),
            std::mem::align_of::<u64>()
        );
    }

    #[test]
    fn current_stats_v1_numeric_domains_are_closed_and_stable() {
        assert_eq!(SURFACE_CURRENT_STATS_ABI_VERSION_V1, 1);
        assert_eq!(SURFACE_CURRENT_STATS_REQUEST_NOT_APPLICABLE, 0);
        assert_eq!(SURFACE_CURRENT_STATS_REQUEST_REQUESTED, 1);
        assert_eq!(SURFACE_CURRENT_STATS_REQUEST_BUSY, 2);
        assert_eq!(SURFACE_CURRENT_STATS_REQUEST_GPU_UNAVAILABLE, 3);
        assert_eq!(SURFACE_CURRENT_STATS_REQUEST_RESOURCE_UNAVAILABLE, 4);
        assert_eq!(SURFACE_CURRENT_STATS_REQUEST_TICKET_EXHAUSTED, 5);
        assert_eq!(SURFACE_CURRENT_STATS_SUBMISSION_UNSPECIFIED, 0);
        assert_eq!(SURFACE_CURRENT_STATS_SUBMISSION_NOT_REQUESTED, 1);
        assert_eq!(SURFACE_CURRENT_STATS_SUBMISSION_ISSUED, 2);
        assert_eq!(SURFACE_CURRENT_STATS_PLAN_NOT_APPLICABLE, 0);
        assert_eq!(
            surface_current_stats_plan_to_ffi(SurfaceCurrentStatsPlan::CpuPostSort),
            1
        );
        assert_eq!(
            surface_current_stats_plan_to_ffi(SurfaceCurrentStatsPlan::GpuPostSort),
            2
        );
        assert_eq!(
            surface_current_stats_plan_to_ffi(SurfaceCurrentStatsPlan::GpuPreproject),
            3
        );
        assert_eq!(SURFACE_CURRENT_STATS_POLL_UNSPECIFIED, 0);
        assert_eq!(SURFACE_CURRENT_STATS_POLL_EMPTY, 1);
        assert_eq!(SURFACE_CURRENT_STATS_POLL_UNSAMPLED, 2);
        assert_eq!(SURFACE_CURRENT_STATS_POLL_READY, 3);
        assert_eq!(SURFACE_CURRENT_STATS_POLL_MAP_FAILURE, 4);
        assert_eq!(SURFACE_CURRENT_STATS_POLL_GENERATION_INVALIDATED, 5);
        assert_eq!(SURFACE_CURRENT_STATS_POLL_EXPIRED, 6);
        assert_eq!(SURFACE_CURRENT_STATS_POLL_DROPPED, 7);
        assert_eq!(SURFACE_CURRENT_STATS_COUNT_SEMANTICS_NONE, 0);
        assert_eq!(
            surface_current_stats_count_semantics_to_ffi(
                SurfaceCurrentStatsCountSemantics::DirectDrawEqualsVisible,
            ),
            1
        );
        assert_eq!(
            surface_current_stats_count_semantics_to_ffi(
                SurfaceCurrentStatsCountSemantics::IndirectDrawEqualsVisible,
            ),
            2
        );
        assert_eq!(
            surface_current_stats_count_semantics_to_ffi(
                SurfaceCurrentStatsCountSemantics::IndirectDrawEqualsContributor,
            ),
            3
        );
    }

    #[test]
    fn current_stats_v1_translates_legacy_surface_three_states_as_success_payloads() {
        let request = surface_current_stats_request_to_ffi(SurfaceCurrentStatsRequest::Unsampled(
            SurfaceCurrentStatsUnsampledReason::GpuUnavailable,
        ));
        assert_eq!(request.version, 1);
        assert_eq!(
            request.struct_size as usize,
            std::mem::size_of_val(&request)
        );
        assert_eq!(request.status, 3);
        assert_eq!(request.reserved, 0);
        assert_eq!(request.reserved_u64, [0; 2]);

        let submission =
            surface_current_stats_submission_to_ffi(SurfaceCurrentStatsSubmission::NotRequested);
        assert_eq!(submission.status, 1);
        assert_eq!(submission.ticket, 0);
        assert_eq!(
            submission.identity,
            GsplatSurfaceCurrentStatsIdentityV1::default()
        );

        let poll = surface_current_stats_poll_to_ffi(SurfaceCurrentStatsPoll::Empty);
        assert_eq!(poll.kind, 1);
        assert_eq!(poll.request_status, 0);
        assert_eq!(poll.count_semantics, 0);
        assert_eq!(poll.ticket, 0);
        assert_eq!(
            poll.identity,
            GsplatSurfaceCurrentStatsIdentityV1::default()
        );
        assert_eq!(
            (
                poll.source_count,
                poll.visible_count,
                poll.contributor_count,
                poll.drawn_count,
            ),
            (0, 0, 0, 0)
        );
    }

    #[test]
    fn current_stats_v1_rejects_null_size_and_version_without_output_mutation() {
        let invalid = ErrorCode::InvalidArgument.as_i32();

        assert_eq!(
            unsafe {
                gsplat_surface_renderer_request_current_stats_v1(ptr::null_mut(), ptr::null_mut())
            },
            invalid
        );

        let mut request = GsplatSurfaceCurrentStatsRequestV1 {
            status: 0xfeed,
            ..Default::default()
        };
        let request_before = request;
        assert_eq!(
            unsafe {
                gsplat_surface_renderer_request_current_stats_v1(ptr::null_mut(), &mut request)
            },
            invalid
        );
        assert_eq!(request, request_before);

        let mut submission = GsplatSurfaceCurrentStatsSubmissionV1 {
            version: 99,
            status: 0xfeed,
            ..Default::default()
        };
        let submission_before = submission;
        assert_eq!(
            unsafe {
                gsplat_surface_renderer_get_current_stats_submission_v1(
                    ptr::null(),
                    &mut submission,
                )
            },
            invalid
        );
        assert_eq!(submission, submission_before);

        let mut poll = GsplatSurfaceCurrentStatsPollV1 {
            struct_size: std::mem::size_of::<GsplatSurfaceCurrentStatsPollV1>() as u32 - 1,
            kind: 0xfeed,
            ..Default::default()
        };
        let poll_before = poll;
        assert_eq!(
            unsafe { gsplat_surface_renderer_poll_current_stats_v1(ptr::null_mut(), &mut poll) },
            invalid
        );
        assert_eq!(poll, poll_before);
    }

    #[test]
    fn current_stats_bridge_adds_no_ffi_owned_renderer_state() {
        let source = include_str!("lib.rs");
        let struct_start = source
            .find("pub struct GsplatSurfaceRenderer {")
            .expect("Surface renderer definition");
        let after_start = &source[struct_start..];
        let struct_end = after_start
            .find("\n}\n\n#[derive(Debug, Clone, Copy)]\nstruct SurfaceOrderMeasurementContext")
            .expect("Surface renderer definition terminator");
        let fields = &after_start[..struct_end];
        assert!(
            !fields.contains("current_stats"),
            "the current-stats C bridge must not add queue/cache/policy/result fields",
        );
    }
}
