import Foundation

#if canImport(GsplatFFI)
import GsplatFFI
#endif

#if canImport(UIKit)
import UIKit

/// Admission result for one non-blocking current-stats request.
///
/// Every case is a successful C call result. In particular, busy and
/// unavailable states are normal render-loop outcomes rather than errors.
public enum GsplatCurrentStatsRequestStatus: UInt32, Equatable {
    case requested = 1
    case busy = 2
    case gpuUnavailable = 3
    case resourceUnavailable = 4
    case ticketExhausted = 5
}

/// Complete Exact plan that produced a current-stats receipt.
public enum GsplatCurrentStatsPlan: UInt32, Equatable {
    case cpuPostSort = 1
    case gpuPostSort = 2
    case gpuPreproject = 3
}

/// Count relationship carried by an atomic Ready receipt.
public enum GsplatCurrentStatsCountSemantics: UInt32, Equatable {
    case directDrawEqualsVisible = 1
    case indirectDrawEqualsVisible = 2
    case indirectDrawEqualsContributor = 3

    public var exactContributorCompaction: Bool {
        self == .indirectDrawEqualsContributor
    }
}

/// Complete immutable identity shared by an Issued submission and terminal.
/// Generation, revision, attempt, and sequence values are opaque join values;
/// zero is valid when Renderer publishes it as part of an applicable payload.
public struct GsplatCurrentStatsIdentity: Equatable {
    public let sceneGeneration: UInt64
    public let cameraRevision: UInt64
    public let viewportGeneration: UInt64
    public let contractGeneration: UInt64
    public let planSetGeneration: UInt64
    public let executedPlan: GsplatCurrentStatsPlan
    public let orderGeneration: UInt64
    public let rasterGeneration: UInt64
    public let encodeAttempt: UInt64
    public let presentationSequence: UInt64

    init(native: GsplatSurfaceCurrentStatsIdentityV1, operation: String) throws {
        guard native.reserved == 0,
              let executedPlan = GsplatCurrentStatsPlan(rawValue: native.executed_plan) else {
            throw currentStatsPayloadError(
                operation: operation,
                detail: "native current-stats identity contains an invalid plan or reserved field"
            )
        }
        sceneGeneration = native.scene_generation
        cameraRevision = native.camera_revision
        viewportGeneration = native.viewport_generation
        contractGeneration = native.contract_generation
        planSetGeneration = native.plan_set_generation
        self.executedPlan = executedPlan
        orderGeneration = native.order_generation
        rasterGeneration = native.raster_generation
        encodeAttempt = native.encode_attempt
        presentationSequence = native.presentation_sequence
    }
}

/// Presentation-committed ticket and complete identity for one requested frame.
public struct GsplatCurrentStatsIssuedSubmission: Equatable {
    public let ticket: UInt64
    public let identity: GsplatCurrentStatsIdentity
}

public enum GsplatCurrentStatsSubmission: Equatable {
    case notRequested
    case issued(GsplatCurrentStatsIssuedSubmission)

    init(native: GsplatSurfaceCurrentStatsSubmissionV1) throws {
        let operation = "gsplat_surface_renderer_get_current_stats_submission_v1"
        try requireCurrentStatsV1Header(
            size: native.struct_size,
            version: native.version,
            expectedSize: MemoryLayout<GsplatSurfaceCurrentStatsSubmissionV1>.size,
            operation: operation
        )
        guard native.reserved == 0,
              native.reserved_u64.0 == 0,
              native.reserved_u64.1 == 0 else {
            throw currentStatsPayloadError(
                operation: operation,
                detail: "native current-stats submission contains nonzero reserved fields"
            )
        }
        switch native.status {
        case 1:
            guard native.ticket == 0, currentStatsIdentityIsZero(native.identity) else {
                throw currentStatsPayloadError(
                    operation: operation,
                    detail: "NotRequested submission exposed an inapplicable ticket or identity"
                )
            }
            self = .notRequested
        case 2:
            guard native.ticket != 0 else {
                throw currentStatsPayloadError(
                    operation: operation,
                    detail: "Issued submission contains the reserved zero ticket"
                )
            }
            self = .issued(GsplatCurrentStatsIssuedSubmission(
                ticket: native.ticket,
                identity: try GsplatCurrentStatsIdentity(
                    native: native.identity,
                    operation: operation
                )
            ))
        default:
            throw currentStatsPayloadError(
                operation: operation,
                detail: "native current-stats submission contains an unknown status"
            )
        }
    }
}

/// Atomic current S/V/C/D value for one presentation-committed frame.
public struct GsplatCurrentStatsReceipt: Equatable {
    public let ticket: UInt64
    public let identity: GsplatCurrentStatsIdentity
    public let countSemantics: GsplatCurrentStatsCountSemantics
    public let sourceCount: UInt32
    public let visibleCount: UInt32
    public let contributorCount: UInt32
    public let drawnCount: UInt32
}

public enum GsplatCurrentStatsFailureReason: Equatable {
    case mapFailure
    case generationInvalidated
    case expired
    case dropped
}

/// Terminal failure for an Issued current-stats ticket. Counts are absent by contract.
public struct GsplatCurrentStatsFailure: Equatable {
    public let ticket: UInt64
    public let identity: GsplatCurrentStatsIdentity
    public let reason: GsplatCurrentStatsFailureReason
}

/// One non-blocking global single-pop result.
public enum GsplatCurrentStatsPoll: Equatable {
    case empty
    case unsampled(GsplatCurrentStatsRequestStatus)
    case ready(GsplatCurrentStatsReceipt)
    case failure(GsplatCurrentStatsFailure)

    init(native: GsplatSurfaceCurrentStatsPollV1) throws {
        let operation = "gsplat_surface_renderer_poll_current_stats_v1"
        try requireCurrentStatsV1Header(
            size: native.struct_size,
            version: native.version,
            expectedSize: MemoryLayout<GsplatSurfaceCurrentStatsPollV1>.size,
            operation: operation
        )
        guard native.reserved == 0,
              native.reserved_u64.0 == 0,
              native.reserved_u64.1 == 0 else {
            throw currentStatsPayloadError(
                operation: operation,
                detail: "native current-stats poll contains nonzero reserved fields"
            )
        }

        let countsAreZero = native.source_count == 0 &&
            native.visible_count == 0 &&
            native.contributor_count == 0 &&
            native.drawn_count == 0
        switch native.kind {
        case 1:
            guard native.request_status == 0,
                  native.count_semantics == 0,
                  native.ticket == 0,
                  currentStatsIdentityIsZero(native.identity),
                  countsAreZero else {
                throw currentStatsPayloadError(
                    operation: operation,
                    detail: "Empty poll exposed an inapplicable payload"
                )
            }
            self = .empty
        case 2:
            guard let status = GsplatCurrentStatsRequestStatus(rawValue: native.request_status),
                  status != .requested,
                  native.count_semantics == 0,
                  native.ticket == 0,
                  currentStatsIdentityIsZero(native.identity),
                  countsAreZero else {
                throw currentStatsPayloadError(
                    operation: operation,
                    detail: "Unsampled poll contains an invalid request status or payload"
                )
            }
            self = .unsampled(status)
        case 3:
            guard native.request_status == 0,
                  let semantics = GsplatCurrentStatsCountSemantics(
                      rawValue: native.count_semantics
                  ),
                  native.ticket != 0,
                  native.contributor_count <= native.visible_count,
                  native.visible_count <= native.source_count,
                  semantics.exactContributorCompaction
                    ? native.drawn_count == native.contributor_count
                    : native.drawn_count == native.visible_count else {
                throw currentStatsPayloadError(
                    operation: operation,
                    detail: "Ready poll contains invalid applicability or S/V/C/D semantics"
                )
            }
            self = .ready(GsplatCurrentStatsReceipt(
                ticket: native.ticket,
                identity: try GsplatCurrentStatsIdentity(
                    native: native.identity,
                    operation: operation
                ),
                countSemantics: semantics,
                sourceCount: native.source_count,
                visibleCount: native.visible_count,
                contributorCount: native.contributor_count,
                drawnCount: native.drawn_count
            ))
        case 4, 5, 6, 7:
            guard native.request_status == 0,
                  native.count_semantics == 0,
                  native.ticket != 0,
                  countsAreZero else {
                throw currentStatsPayloadError(
                    operation: operation,
                    detail: "terminal failure poll exposed inapplicable count data"
                )
            }
            let reason: GsplatCurrentStatsFailureReason
            switch native.kind {
            case 4: reason = .mapFailure
            case 5: reason = .generationInvalidated
            case 6: reason = .expired
            default: reason = .dropped
            }
            self = .failure(GsplatCurrentStatsFailure(
                ticket: native.ticket,
                identity: try GsplatCurrentStatsIdentity(
                    native: native.identity,
                    operation: operation
                ),
                reason: reason
            ))
        default:
            throw currentStatsPayloadError(
                operation: operation,
                detail: "native current-stats poll contains an unknown kind"
            )
        }
    }
}

public enum GsplatCurrentStatsCorrelationRejection: Equatable {
    case unknownTerminal(ticket: UInt64)
    case identityMismatch(
        ticket: UInt64,
        expected: GsplatCurrentStatsIdentity,
        actual: GsplatCurrentStatsIdentity
    )
}

/// Result of the minimal consumer-side pending correlation ledger.
public enum GsplatCurrentStatsConsumerEvent: Equatable {
    case notRequested
    case pending(GsplatCurrentStatsIssuedSubmission)
    case empty
    case unsampled(GsplatCurrentStatsRequestStatus)
    case ready(GsplatCurrentStatsReceipt)
    case failure(GsplatCurrentStatsFailure)
    case rejected(GsplatCurrentStatsCorrelationRejection)

    public var isReady: Bool {
        if case .ready = self { return true }
        return false
    }
}

/// Consumer-only correlation for Issued submissions and terminal receipts.
///
/// This stores no renderer policy, sampling state, generation source, or count
/// fallback. A Ready value is returned only after ticket and every identity
/// field match the previously observed Issued submission.
public struct GsplatCurrentStatsConsumer {
    private var pendingByTicket: [UInt64: GsplatCurrentStatsIdentity] = [:]

    public init() {}

    public var pendingCount: Int { pendingByTicket.count }

    public mutating func observe(
        _ submission: GsplatCurrentStatsSubmission
    ) -> GsplatCurrentStatsConsumerEvent {
        switch submission {
        case .notRequested:
            return .notRequested
        case .issued(let issued):
            if let expected = pendingByTicket[issued.ticket] {
                guard expected == issued.identity else {
                    return .rejected(.identityMismatch(
                        ticket: issued.ticket,
                        expected: expected,
                        actual: issued.identity
                    ))
                }
                return .pending(issued)
            }
            pendingByTicket[issued.ticket] = issued.identity
            return .pending(issued)
        }
    }

    public mutating func consume(
        _ poll: GsplatCurrentStatsPoll
    ) -> GsplatCurrentStatsConsumerEvent {
        switch poll {
        case .empty:
            return .empty
        case .unsampled(let status):
            return .unsampled(status)
        case .ready(let receipt):
            return correlate(
                ticket: receipt.ticket,
                identity: receipt.identity,
                accepted: .ready(receipt)
            )
        case .failure(let failure):
            return correlate(
                ticket: failure.ticket,
                identity: failure.identity,
                accepted: .failure(failure)
            )
        }
    }

    private mutating func correlate(
        ticket: UInt64,
        identity: GsplatCurrentStatsIdentity,
        accepted: GsplatCurrentStatsConsumerEvent
    ) -> GsplatCurrentStatsConsumerEvent {
        guard let expected = pendingByTicket.removeValue(forKey: ticket) else {
            return .rejected(.unknownTerminal(ticket: ticket))
        }
        guard expected == identity else {
            return .rejected(.identityMismatch(
                ticket: ticket,
                expected: expected,
                actual: identity
            ))
        }
        return accepted
    }
}

private func requireCurrentStatsV1Header(
    size: UInt32,
    version: UInt32,
    expectedSize: Int,
    operation: String
) throws {
    guard size == UInt32(expectedSize), version == 1 else {
        throw currentStatsPayloadError(
            operation: operation,
            detail: "native current-stats V1 value has an incompatible size or version"
        )
    }
}

private func currentStatsPayloadError(operation: String, detail: String) -> GsplatKitError {
    GsplatKitError(code: gsplatInvalidArgument, operation: operation, detail: detail)
}

private func currentStatsIdentityIsZero(
    _ identity: GsplatSurfaceCurrentStatsIdentityV1
) -> Bool {
    identity.scene_generation == 0 &&
        identity.camera_revision == 0 &&
        identity.viewport_generation == 0 &&
        identity.contract_generation == 0 &&
        identity.plan_set_generation == 0 &&
        identity.order_generation == 0 &&
        identity.raster_generation == 0 &&
        identity.encode_attempt == 0 &&
        identity.presentation_sequence == 0 &&
        identity.executed_plan == 0 &&
        identity.reserved == 0
}

func currentStatsRequestStatus(
    native: GsplatSurfaceCurrentStatsRequestV1
) throws -> GsplatCurrentStatsRequestStatus {
    let operation = "gsplat_surface_renderer_request_current_stats_v1"
    try requireCurrentStatsV1Header(
        size: native.struct_size,
        version: native.version,
        expectedSize: MemoryLayout<GsplatSurfaceCurrentStatsRequestV1>.size,
        operation: operation
    )
    guard native.reserved == 0,
          native.reserved_u64.0 == 0,
          native.reserved_u64.1 == 0,
          let status = GsplatCurrentStatsRequestStatus(rawValue: native.status) else {
        throw currentStatsPayloadError(
            operation: operation,
            detail: "native current-stats request contains an invalid status or reserved field"
        )
    }
    return status
}
#endif
