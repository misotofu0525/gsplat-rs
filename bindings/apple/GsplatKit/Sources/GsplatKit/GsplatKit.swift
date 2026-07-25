import Foundation

#if canImport(GsplatFFI)
import GsplatFFI
#endif

#if canImport(UIKit)
import UIKit
#endif

// Keep internal fallback codes aligned with GsplatErrorCode in gsplat.h without
// exposing raw C enum types in the public Swift API.
private let gsplatOk: Int32 = 0
let gsplatInvalidArgument: Int32 = 1
private let gsplatUnsupported: Int32 = 4

public struct GsplatKitError: Error, CustomStringConvertible, Equatable {
    public let code: Int32
    public let operation: String
    private let detail: String?

    init(code: Int32, operation: String, detail: String? = nil) {
        self.code = code
        self.operation = operation
        self.detail = detail
    }

    public var message: String {
        detail ?? String(cString: gsplat_error_message(code))
    }

    public var description: String {
        "\(operation): \(message) (code=\(code))"
    }
}

public struct GsplatKitVersion: Equatable {
    public static let supportedMajor: UInt32 = 0
    public static let supportedMinor: UInt32 = 1

    public let major: UInt32
    public let minor: UInt32

    public static var current: GsplatKitVersion {
        GsplatKitVersion(
            major: gsplat_version_major(),
            minor: gsplat_version_minor()
        )
    }

    public static func requireSupported() throws {
        let version = GsplatKitVersion.current
        guard version.major == supportedMajor && version.minor == supportedMinor else {
            throw GsplatKitError(
                code: gsplatUnsupported,
                operation: "gsplat_version",
                detail: "unsupported ABI version \(version.major).\(version.minor)"
            )
        }
    }
}

public struct GsplatRenderConfiguration: Equatable {
    public var width: UInt32
    public var height: UInt32

    public init(width: UInt32 = 800, height: UInt32 = 600) {
        self.width = width
        self.height = height
    }

    func makeRawValue() -> GsplatConfig {
        var config = gsplat_config_default()
        config.width = width
        config.height = height
        return config
    }
}

public struct GsplatFrameStats: Equatable {
    public let frameMs: Float
    public let preprocessMs: Float
    public let sortMs: Float
    public let rasterMs: Float
    public let visibleCount: UInt32
    public let drawnCount: UInt32

    public var hasDrawnContent: Bool {
        visibleCount > 0 && drawnCount > 0
    }

    init(_ raw: GsplatStats) {
        frameMs = raw.frame_ms
        preprocessMs = raw.preprocess_ms
        sortMs = raw.sort_ms
        rasterMs = raw.raster_ms
        visibleCount = raw.visible_count
        drawnCount = raw.drawn_count
    }
}

public final class GsplatContextRenderer {
    private var context: OpaquePointer?
    private let lock = NSLock()

    public init(configuration: GsplatRenderConfiguration = GsplatRenderConfiguration()) throws {
        try GsplatKitVersion.requireSupported()

        var handle: OpaquePointer?
        try check(
            gsplat_context_create(configuration.makeRawValue(), &handle),
            operation: "gsplat_context_create"
        )
        guard let handle else {
            throw GsplatKitError(
                code: gsplatInvalidArgument,
                operation: "gsplat_context_create",
                detail: "native context was nil after a successful create call"
            )
        }
        context = handle
    }

    deinit {
        close()
    }

    public func close() {
        lock.lock()
        defer { lock.unlock() }

        if let context {
            gsplat_context_destroy(context)
            self.context = nil
        }
    }

    public func setDefaultCamera() throws {
        try withContext(operation: "gsplat_context_set_camera") { context in
            try check(
                gsplat_context_set_camera(context, gsplat_camera_default()),
                operation: "gsplat_context_set_camera"
            )
        }
    }

    public func setAutoCamera() throws {
        try withContext(operation: "gsplat_context_set_auto_camera") { context in
            try check(
                gsplat_context_set_auto_camera(context),
                operation: "gsplat_context_set_auto_camera"
            )
        }
    }

    public func loadScene(path: String) throws {
        try withContext(operation: "gsplat_context_load_scene_path") { context in
            try path.withCString { pathPointer in
                try check(
                    gsplat_context_load_scene_path(context, pathPointer),
                    operation: "gsplat_context_load_scene_path"
                )
            }
        }
    }

    public func renderFrame() throws {
        try withContext(operation: "gsplat_context_render_frame") { context in
            try check(
                gsplat_context_render_frame(context),
                operation: "gsplat_context_render_frame"
            )
        }
    }

    public func stats() throws -> GsplatFrameStats {
        try withContext(operation: "gsplat_context_get_stats") { context in
            var stats = GsplatStats()
            try check(
                gsplat_context_get_stats(context, &stats),
                operation: "gsplat_context_get_stats"
            )
            return GsplatFrameStats(stats)
        }
    }

    private func withContext<Result>(
        operation: String,
        _ body: (OpaquePointer) throws -> Result
    ) throws -> Result {
        lock.lock()
        defer { lock.unlock() }

        guard let context else {
            throw GsplatKitError(
                code: gsplatInvalidArgument,
                operation: operation,
                detail: "renderer is closed"
            )
        }
        return try body(context)
    }
}

#if canImport(UIKit)
public enum GsplatGeometryPath: UInt32, Equatable {
    case direct = 0
    case packedAtlas = 1
    case pagedActiveAtlas = 2
}

public enum GsplatOrderBackend: UInt32, Equatable {
    case cpu = 0
    case gpu = 1
    case adaptive = 2
}

public enum GsplatOrderTimingSource: UInt32, Equatable {
    case gpuTimestampQuery = 1
    case gpuCompletion = 2
}

public enum GsplatAdaptiveState: UInt32, Equatable {
    case disabled = 0
    case cpuLearning = 1
    case cpuStable = 2
    case gpuProbe = 3
    case gpuStable = 4
    case cpuProbe = 5
    case cooldown = 6
}

public enum GsplatOrderMeasurementFailureReason: UInt32, Equatable {
    case readbackMap = 1
    case generationInvalidated = 2
}

public enum GsplatOrderUnsampledReason: Equatable {
    case ringBusy
    case surfaceUnavailable
}

public enum GsplatAdaptiveGpuFailureReason: UInt32, Equatable {
    case unsupported = 1
    case initialization = 2
    case outOfMemory = 3
    case validation = 4
}

/// Independent draw-list policy applied after projection.
///
/// Candidate draws every near/far candidate (`D == V`). Compact draws only
/// exact post-projection contributors (`D == C`). Adaptive measures both on
/// the active CPU/GPU order lane and keeps the faster full-quality execution.
public enum GsplatProjectedDrawPolicy: UInt32, Equatable {
    case candidate = 1
    case compact = 2
    case adaptive = 3
}

public enum GsplatProjectedDrawExecution: UInt32, Equatable {
    case candidate = 1
    case compact = 2
}

public enum GsplatProjectedDrawAdaptiveState: UInt32, Equatable {
    case disabled = 0
    case candidateLearning = 1
    case candidateStable = 2
    case compactProbe = 3
    case compactStable = 4
    case candidateProbe = 5
    case cooldown = 6
    case candidateOnly = 7
}

public enum GsplatProjectedDrawUnsampledReason: Equatable {
    case ringBusy
    case surfaceUnavailable
}

public enum GsplatProjectedDrawMeasurementFailureReason: UInt32, Equatable {
    case readbackMap = 1
    case generationInvalidated = 2
    case invariantViolation = 3
}

/// Projected-draw measurement identity for the last successfully rendered frame.
public struct GsplatProjectedDrawSubmission: Equatable {
    public let ticket: UInt64?
    public let cameraRevision: UInt64
    public let requestedPolicy: GsplatProjectedDrawPolicy
    public let actualExecution: GsplatProjectedDrawExecution
    public let orderBackend: GsplatOrderBackend
    public let adaptiveState: GsplatProjectedDrawAdaptiveState
    public let unsampledReason: GsplatProjectedDrawUnsampledReason?
    public let flags: UInt32

    init(_ native: GsplatSurfaceProjectedSubmissionV1) throws {
        try requireProjectedV1Header(
            size: native.struct_size,
            version: native.version,
            expectedSize: MemoryLayout<GsplatSurfaceProjectedSubmissionV1>.size,
            operation: "gsplat_surface_renderer_get_projected_submission_v1"
        )
        guard native.reserved == 0,
              let requestedPolicy = GsplatProjectedDrawPolicy(rawValue: native.requested_policy),
              let actualExecution = GsplatProjectedDrawExecution(rawValue: native.actual_execution),
              let orderBackend = GsplatOrderBackend(rawValue: native.order_backend),
              orderBackend != .adaptive,
              let adaptiveState = GsplatProjectedDrawAdaptiveState(rawValue: native.adaptive_state)
        else {
            throw GsplatKitError(
                code: gsplatInvalidArgument,
                operation: "gsplat_surface_renderer_get_projected_submission_v1",
                detail: "native projected-draw submission contains an invalid header or enum value"
            )
        }

        let ticketIssued = native.flags & (1 << 0) != 0
        let ringBusy = native.flags & (1 << 1) != 0
        let surfaceUnavailable = native.flags & (1 << 2) != 0
        let forcedPolicyIsConsistent = requestedPolicy == .adaptive || (
            adaptiveState == .disabled &&
                ((requestedPolicy == .candidate && actualExecution == .candidate) ||
                    (requestedPolicy == .compact && actualExecution == .compact))
        )
        guard native.flags & ~UInt32(0b111) == 0,
              !(ringBusy && surfaceUnavailable),
              ticketIssued
                ? (isProjectedDrawTicket(native.ticket) && !ringBusy && !surfaceUnavailable)
                : native.ticket == 0,
              !ticketIssued || requestedPolicy == .adaptive,
              !ringBusy || requestedPolicy == .adaptive,
              !surfaceUnavailable || requestedPolicy == .adaptive,
              forcedPolicyIsConsistent
        else {
            throw GsplatKitError(
                code: gsplatInvalidArgument,
                operation: "gsplat_surface_renderer_get_projected_submission_v1",
                detail: "native projected-draw submission flags are internally inconsistent"
            )
        }

        self.ticket = ticketIssued ? native.ticket : nil
        self.cameraRevision = native.camera_revision
        self.requestedPolicy = requestedPolicy
        self.actualExecution = actualExecution
        self.orderBackend = orderBackend
        self.adaptiveState = adaptiveState
        if ringBusy {
            unsampledReason = .ringBusy
        } else if surfaceUnavailable {
            unsampledReason = .surfaceUnavailable
        } else {
            unsampledReason = nil
        }
        self.flags = native.flags
    }
}

/// Terminal queue-completion receipt for one Adaptive projected-draw probe.
public struct GsplatProjectedDrawMeasurement: Equatable {
    public let ticket: UInt64
    public let cameraRevision: UInt64
    public let projectionGeneration: UInt64
    public let probeGeneration: UInt64
    /// Frame start through graphics-queue completion, not CPU submit-wall time.
    public let frameCompleteMs: Float
    public let execution: GsplatProjectedDrawExecution
    public let orderBackend: GsplatOrderBackend
    public let projectionRebuilt: Bool
    public let orderRefreshed: Bool
    public let exactContributorDraw: Bool
    public let droppedPriorMeasurements: Bool
    public let visibleCount: UInt32
    public let contributorCount: UInt32
    public let drawnCount: UInt32
    public let flags: UInt32

    fileprivate init(
        _ native: GsplatSurfaceProjectedMeasurementV1,
        counts: GsplatSurfaceProjectedCountsV1
    ) throws {
        try requireProjectedV1Header(
            size: native.struct_size,
            version: native.version,
            expectedSize: MemoryLayout<GsplatSurfaceProjectedMeasurementV1>.size,
            operation: "gsplat_surface_renderer_poll_projected_measurement_v1"
        )
        try requireProjectedV1Header(
            size: counts.struct_size,
            version: counts.version,
            expectedSize: MemoryLayout<GsplatSurfaceProjectedCountsV1>.size,
            operation: "gsplat_surface_renderer_take_projected_counts_v1"
        )
        guard isProjectedDrawTicket(native.ticket),
              native.frame_complete_ms.isFinite,
              native.frame_complete_ms >= 0,
              let execution = GsplatProjectedDrawExecution(rawValue: native.execution),
              let orderBackend = GsplatOrderBackend(rawValue: native.order_backend),
              orderBackend != .adaptive
        else {
            throw GsplatKitError(
                code: gsplatInvalidArgument,
                operation: "gsplat_surface_renderer_poll_projected_measurement_v1",
                detail: "native projected-draw measurement contains an invalid identity, timing, or enum"
            )
        }

        let projectionRebuilt = native.flags & (1 << 0) != 0
        let orderRefreshed = native.flags & (1 << 1) != 0
        let exactContributorDraw = native.flags & (1 << 2) != 0
        let countsExactContributorDraw = counts.flags & (1 << 0) != 0
        guard native.flags & ~UInt32(0b1111) == 0,
              counts.flags & ~UInt32(0b1) == 0,
              counts.ticket == native.ticket,
              counts.camera_revision == native.camera_revision,
              counts.contributor_count <= counts.visible_count,
              exactContributorDraw == countsExactContributorDraw,
              projectionRebuilt,
              !orderRefreshed,
              execution == .compact
                ? (exactContributorDraw && counts.drawn_count == counts.contributor_count)
                : (!exactContributorDraw && counts.drawn_count == counts.visible_count)
        else {
            throw GsplatKitError(
                code: gsplatInvalidArgument,
                operation: "gsplat_surface_renderer_take_projected_counts_v1",
                detail: "native projected-draw timing and V/C/D receipts disagree"
            )
        }

        self.ticket = native.ticket
        self.cameraRevision = native.camera_revision
        self.projectionGeneration = native.projection_generation
        self.probeGeneration = native.probe_generation
        self.frameCompleteMs = native.frame_complete_ms
        self.execution = execution
        self.orderBackend = orderBackend
        self.projectionRebuilt = projectionRebuilt
        self.orderRefreshed = orderRefreshed
        self.exactContributorDraw = exactContributorDraw
        self.droppedPriorMeasurements = native.flags & (1 << 3) != 0
        self.visibleCount = counts.visible_count
        self.contributorCount = counts.contributor_count
        self.drawnCount = counts.drawn_count
        self.flags = native.flags
    }
}

/// Terminal failure for one issued projected-draw measurement ticket.
public struct GsplatProjectedDrawMeasurementFailure: Equatable {
    public let ticket: UInt64
    public let cameraRevision: UInt64
    public let projectionGeneration: UInt64
    public let probeGeneration: UInt64
    public let reason: GsplatProjectedDrawMeasurementFailureReason
    public let execution: GsplatProjectedDrawExecution
    public let orderBackend: GsplatOrderBackend
    public let droppedPriorFailures: Bool
    public let flags: UInt32

    fileprivate init(_ native: GsplatSurfaceProjectedFailureV1) throws {
        try requireProjectedV1Header(
            size: native.struct_size,
            version: native.version,
            expectedSize: MemoryLayout<GsplatSurfaceProjectedFailureV1>.size,
            operation: "gsplat_surface_renderer_poll_projected_failure_v1"
        )
        guard isProjectedDrawTicket(native.ticket),
              native.flags & ~UInt32(0b1) == 0,
              let reason = GsplatProjectedDrawMeasurementFailureReason(rawValue: native.reason),
              let execution = GsplatProjectedDrawExecution(rawValue: native.execution),
              let orderBackend = GsplatOrderBackend(rawValue: native.order_backend),
              orderBackend != .adaptive
        else {
            throw GsplatKitError(
                code: gsplatInvalidArgument,
                operation: "gsplat_surface_renderer_poll_projected_failure_v1",
                detail: "native projected-draw failure contains an invalid identity or enum"
            )
        }
        self.ticket = native.ticket
        self.cameraRevision = native.camera_revision
        self.projectionGeneration = native.projection_generation
        self.probeGeneration = native.probe_generation
        self.reason = reason
        self.execution = execution
        self.orderBackend = orderBackend
        self.droppedPriorFailures = native.flags & (1 << 0) != 0
        self.flags = native.flags
    }
}

/// One serialized drain of both projected-draw terminal queues.
///
/// Success counts are taken before the next native poll, and no render call can
/// interleave while the wrapper lock is held.
public struct GsplatProjectedDrawTerminalBatch: Equatable {
    public let measurements: [GsplatProjectedDrawMeasurement]
    public let failures: [GsplatProjectedDrawMeasurementFailure]
}

public struct GsplatOrderMeasurement: Equatable {
    public let ticket: UInt64
    public let cameraRevision: UInt64
    public let timingSource: GsplatOrderTimingSource
    public let requestedBackend: GsplatOrderBackend
    public let actualBackend: GsplatOrderBackend
    public let adaptiveState: GsplatAdaptiveState
    public let gpuPreprocessMs: Float?
    public let gpuRadixMs: Float?
    public let gpuOrderMs: Float?
    public let gpuCompleteMs: Float
    public let timestampPeriodNs: Float?
    public let belowTimestampResolution: Bool
    /// True when this bounded queue receipt follows at least one dropped older receipt.
    public let droppedPriorMeasurements: Bool
    public let visibleCount: UInt32
    /// Exact post-projection contributor count for this ticket and camera revision.
    public let contributorCount: UInt32
    public let drawnCount: UInt32
    /// True only when the issued count is the exact contributor count (`D == C`).
    public let exactContributorCompaction: Bool
    public let flags: UInt32

    fileprivate init(
        _ native: GsplatSurfaceOrderMeasurement,
        counts: GsplatSurfaceOrderCounts
    ) throws {
        guard let timingSource = GsplatOrderTimingSource(rawValue: native.timing_source),
              let requestedBackend = GsplatOrderBackend(rawValue: native.requested_backend),
              let actualBackend = GsplatOrderBackend(rawValue: native.actual_backend),
              let adaptiveState = GsplatAdaptiveState(rawValue: native.adaptive_state)
        else {
            throw GsplatKitError(
                code: gsplatInvalidArgument,
                operation: "gsplat_surface_renderer_poll_order_measurement",
                detail: "native order receipt contains an unknown enum value"
            )
        }
        let preprocessValid: UInt32 = 1 << 0
        let radixValid: UInt32 = 1 << 1
        let orderValid: UInt32 = 1 << 2
        let timestampPeriodValid: UInt32 = 1 << 3
        let belowTimestampResolution: UInt32 = 1 << 4
        let droppedPrior: UInt32 = 1 << 5
        let exactContributorDraw: UInt32 = 1 << 6
        let countsExactContributorDraw: UInt32 = 1 << 0

        let exactContributorCompaction = counts.flags & countsExactContributorDraw != 0
        guard counts.ticket == native.ticket,
              counts.camera_revision == native.camera_revision,
              counts.visible_count == native.visible_count,
              counts.drawn_count == native.drawn_count,
              counts.contributor_count <= counts.visible_count,
              exactContributorCompaction == (native.flags & exactContributorDraw != 0),
              exactContributorCompaction
                ? counts.drawn_count == counts.contributor_count
                : counts.drawn_count == counts.visible_count
        else {
            throw GsplatKitError(
                code: gsplatInvalidArgument,
                operation: "gsplat_surface_renderer_take_order_counts",
                detail: "native GPU order receipt has inconsistent ticket/revision or V/C/D counts"
            )
        }

        self.ticket = native.ticket
        self.cameraRevision = native.camera_revision
        self.timingSource = timingSource
        self.requestedBackend = requestedBackend
        self.actualBackend = actualBackend
        self.adaptiveState = adaptiveState
        self.gpuPreprocessMs = (native.flags & preprocessValid) != 0 ? native.gpu_preprocess_ms : nil
        self.gpuRadixMs = (native.flags & radixValid) != 0 ? native.gpu_radix_ms : nil
        self.gpuOrderMs = (native.flags & orderValid) != 0 ? native.gpu_order_ms : nil
        self.gpuCompleteMs = native.gpu_complete_ms
        self.timestampPeriodNs = (native.flags & timestampPeriodValid) != 0
            ? native.timestamp_period_ns
            : nil
        self.belowTimestampResolution = (native.flags & belowTimestampResolution) != 0
        self.droppedPriorMeasurements = (native.flags & droppedPrior) != 0
        self.visibleCount = counts.visible_count
        self.contributorCount = counts.contributor_count
        self.drawnCount = counts.drawn_count
        self.exactContributorCompaction = exactContributorCompaction
        self.flags = native.flags
    }
}

/// Queue-completion receipt for one formally sampled CPU order refresh.
public struct GsplatCpuOrderMeasurement: Equatable {
    public let ticket: UInt64
    public let cameraRevision: UInt64
    public let preprocessMs: Float
    public let sortMs: Float
    /// Frame start through graphics-queue completion; never CPU submit-wall time.
    public let frameCompleteMs: Float
    public let requestedBackend: GsplatOrderBackend
    public let actualBackend: GsplatOrderBackend
    public let adaptiveState: GsplatAdaptiveState
    /// Near/far candidate count for this ticket and camera revision.
    public let visibleCount: UInt32
    /// Exact post-projection contributor count for this ticket and camera revision.
    public let contributorCount: UInt32
    /// Actual issued/drawn count for this ticket and camera revision.
    public let drawnCount: UInt32
    public let exactContributorCompaction: Bool
    public let droppedPriorMeasurements: Bool
    public let flags: UInt32

    fileprivate init(
        _ native: GsplatSurfaceCpuOrderMeasurement,
        counts: GsplatSurfaceOrderCounts
    ) throws {
        guard let requestedBackend = GsplatOrderBackend(rawValue: native.requested_backend),
              let actualBackend = GsplatOrderBackend(rawValue: native.actual_backend),
              let adaptiveState = GsplatAdaptiveState(rawValue: native.adaptive_state),
              actualBackend == .cpu
        else {
            throw GsplatKitError(
                code: gsplatInvalidArgument,
                operation: "gsplat_surface_renderer_poll_cpu_order_measurement",
                detail: "native CPU order receipt contains an unknown or invalid enum value"
            )
        }
        self.ticket = native.ticket
        self.cameraRevision = native.camera_revision
        self.preprocessMs = native.preprocess_ms
        self.sortMs = native.sort_ms
        self.frameCompleteMs = native.frame_complete_ms
        self.requestedBackend = requestedBackend
        self.actualBackend = actualBackend
        self.adaptiveState = adaptiveState
        let exactContributorDraw: UInt32 = 1 << 1
        let contributorCountValid: UInt32 = 1 << 2
        let countsExactContributorDraw: UInt32 = 1 << 0
        let exactContributorCompaction = counts.flags & countsExactContributorDraw != 0
        guard counts.ticket == native.ticket,
              counts.camera_revision == native.camera_revision,
              native.flags & contributorCountValid != 0,
              native.reserved == counts.contributor_count,
              counts.contributor_count <= counts.visible_count,
              exactContributorCompaction == (native.flags & exactContributorDraw != 0),
              exactContributorCompaction
                ? counts.drawn_count == counts.contributor_count
                : counts.drawn_count == counts.visible_count
        else {
            throw GsplatKitError(
                code: gsplatInvalidArgument,
                operation: "gsplat_surface_renderer_take_order_counts",
                detail: "native CPU order receipt has inconsistent ticket/revision or V/C/D counts"
            )
        }
        self.visibleCount = counts.visible_count
        self.contributorCount = counts.contributor_count
        self.drawnCount = counts.drawn_count
        self.exactContributorCompaction = exactContributorCompaction
        self.droppedPriorMeasurements = native.flags & 1 != 0
        self.flags = native.flags
    }
}

/// Terminal failure for one CPU or GPU measurement ticket that native code issued.
public struct GsplatOrderMeasurementFailure: Equatable {
    public let ticket: UInt64
    public let cameraRevision: UInt64
    public let reason: GsplatOrderMeasurementFailureReason
    public let requestedBackend: GsplatOrderBackend
    public let actualBackend: GsplatOrderBackend
    public let adaptiveState: GsplatAdaptiveState
    /// True when this bounded queue receipt follows at least one dropped failure.
    public let droppedPriorFailures: Bool
    public let flags: UInt32

    fileprivate init(_ native: GsplatSurfaceOrderMeasurementFailure) throws {
        guard let reason = GsplatOrderMeasurementFailureReason(rawValue: native.reason),
              let requestedBackend = GsplatOrderBackend(rawValue: native.requested_backend),
              let actualBackend = GsplatOrderBackend(rawValue: native.actual_backend),
              let adaptiveState = GsplatAdaptiveState(rawValue: native.adaptive_state)
        else {
            throw GsplatKitError(
                code: gsplatInvalidArgument,
                operation: "gsplat_surface_renderer_poll_order_measurement_failure",
                detail: "native order failure contains an unknown enum value"
            )
        }
        let droppedPrior: UInt32 = 1 << 0
        self.ticket = native.ticket
        self.cameraRevision = native.camera_revision
        self.reason = reason
        self.requestedBackend = requestedBackend
        self.actualBackend = actualBackend
        self.adaptiveState = adaptiveState
        self.droppedPriorFailures = native.flags & droppedPrior != 0
        self.flags = native.flags
    }
}

/// CPU/GPU measurement submission identity for one successfully rendered frame.
public struct GsplatOrderSubmission: Equatable {
    public let ticket: UInt64?
    public let cameraRevision: UInt64
    public let requestedBackend: GsplatOrderBackend
    public let actualBackend: GsplatOrderBackend
    public let adaptiveState: GsplatAdaptiveState
    public let gpuRefresh: Bool
    public let cpuFrameCompletionSample: Bool
    public let unsampledRingBusy: Bool
    public let unsampledSurfaceUnavailable: Bool
    public let flags: UInt32

    public var measurementBackend: GsplatOrderBackend? {
        if gpuRefresh { return .gpu }
        if cpuFrameCompletionSample { return .cpu }
        return nil
    }

    public var unsampledReason: GsplatOrderUnsampledReason? {
        if unsampledRingBusy { return .ringBusy }
        if unsampledSurfaceUnavailable { return .surfaceUnavailable }
        return nil
    }

    fileprivate init(_ native: GsplatSurfaceOrderSubmission) throws {
        guard let requestedBackend = GsplatOrderBackend(rawValue: native.requested_backend),
              let actualBackend = GsplatOrderBackend(rawValue: native.actual_backend),
              let adaptiveState = GsplatAdaptiveState(rawValue: native.adaptive_state)
        else {
            throw GsplatKitError(
                code: gsplatInvalidArgument,
                operation: "gsplat_surface_renderer_get_order_submission",
                detail: "native order submission contains an unknown enum value"
            )
        }
        let ticketIssued: UInt32 = 1 << 1
        self.ticket = native.flags & ticketIssued != 0 ? native.ticket : nil
        self.cameraRevision = native.camera_revision
        self.requestedBackend = requestedBackend
        self.actualBackend = actualBackend
        self.adaptiveState = adaptiveState
        self.gpuRefresh = native.flags & 1 != 0
        self.cpuFrameCompletionSample = native.flags & (1 << 3) != 0
        self.unsampledRingBusy = native.flags & (1 << 2) != 0
        self.unsampledSurfaceUnavailable = native.flags & (1 << 4) != 0
        self.flags = native.flags
        guard !(gpuRefresh && cpuFrameCompletionSample),
              !(unsampledRingBusy && unsampledSurfaceUnavailable),
              measurementBackend != nil || (ticket == nil && unsampledReason == nil),
              ticket == nil || unsampledReason == nil,
              measurementBackend == nil || ticket != nil || unsampledReason != nil
        else {
            throw GsplatKitError(
                code: gsplatInvalidArgument,
                operation: "gsplat_surface_renderer_get_order_submission",
                detail: "native order submission flags are internally inconsistent"
            )
        }
    }
}

/// Per-frame ordering status, including Adaptive GPU availability.
public struct GsplatSurfaceOrderStatus: Equatable {
    public let cameraRevision: UInt64
    public let appliedOrderRevision: UInt64
    public let scheduledRevision: UInt64?
    public let completedRevision: UInt64?
    public let presentedOrderRevisionLag: UInt32
    public let observedResultRevisionLag: UInt32?
    public let sortRefreshed: Bool
    public let orderUploaded: Bool
    public let actualBackend: GsplatOrderBackend
    public let gpuSortFallback: Bool
    public let adaptiveState: GsplatAdaptiveState
    public let adaptiveGpuFailure: GsplatAdaptiveGpuFailureReason?
    public let measurementTicketIssued: Bool
    public let flags: UInt32

    fileprivate init(_ native: GsplatSurfaceSortStats) throws {
        let flags = native.flags
        guard let actualBackend = GsplatOrderBackend(rawValue: (flags >> 9) & 0b11),
              let adaptiveState = GsplatAdaptiveState(rawValue: (flags >> 12) & 0b111)
        else {
            throw GsplatKitError(
                code: gsplatInvalidArgument,
                operation: "gsplat_surface_renderer_get_sort_stats",
                detail: "native Surface order status contains an unknown enum value"
            )
        }
        let adaptiveGpuFailure: GsplatAdaptiveGpuFailureReason?
        if flags & (1 << 15) != 0 {
            guard let reason = GsplatAdaptiveGpuFailureReason(
                rawValue: (flags >> 16) & 0b111
            ) else {
                throw GsplatKitError(
                    code: gsplatInvalidArgument,
                    operation: "gsplat_surface_renderer_get_sort_stats",
                    detail: "native Surface order status contains an unknown GPU failure reason"
                )
            }
            adaptiveGpuFailure = reason
        } else {
            adaptiveGpuFailure = nil
        }

        self.cameraRevision = native.camera_revision
        self.appliedOrderRevision = native.applied_order_revision
        self.scheduledRevision = flags & (1 << 3) != 0 ? native.scheduled_revision : nil
        self.completedRevision = flags & (1 << 4) != 0 ? native.completed_revision : nil
        self.presentedOrderRevisionLag = native.presented_order_revision_lag
        self.observedResultRevisionLag = flags & (1 << 8) != 0
            ? native.observed_result_revision_lag
            : nil
        self.sortRefreshed = flags & 1 != 0
        self.orderUploaded = flags & (1 << 1) != 0
        self.actualBackend = actualBackend
        self.gpuSortFallback = flags & (1 << 11) != 0
        self.adaptiveState = adaptiveState
        self.adaptiveGpuFailure = adaptiveGpuFailure
        self.measurementTicketIssued = flags & (1 << 19) != 0
        self.flags = flags
    }
}

/// Source-to-GPU exactness and adapter-admission receipt for a native Surface.
public struct GsplatSurfaceExactnessReceipt: Equatable {
    public let sourceSplatCount: UInt64
    public let decodedSplatCount: UInt64
    public let encodedSplatCount: UInt64
    public let residentSplatCount: UInt64
    public let addressableSplatCount: UInt64
    public let sourceShDegree: UInt32
    public let residentShDegree: UInt32
    public let sourceMembershipAll: Bool
    public let samplingDisabled: Bool
    public let lodDisabled: Bool
    public let sourceShDegreePreserved: Bool
    public let partialSceneNotPublished: Bool
    public let maxStorageBuffersPerShaderStage: UInt32
    public let maxStorageBufferBindingSize: UInt64
    public let qualityFlags: UInt32

    public var isFullQuality: Bool {
        let fullQualityFlags: UInt32 = 0b1_1111
        return qualityFlags & fullQualityFlags == fullQualityFlags &&
            sourceSplatCount == decodedSplatCount &&
            sourceSplatCount == encodedSplatCount &&
            sourceSplatCount == residentSplatCount &&
            sourceSplatCount == addressableSplatCount &&
            sourceShDegree == residentShDegree
    }

    fileprivate init(_ native: GsplatSurfaceExactness) {
        let sourceMembershipAll: UInt32 = 1 << 0
        let samplingDisabled: UInt32 = 1 << 1
        let lodDisabled: UInt32 = 1 << 2
        let sourceShDegree: UInt32 = 1 << 3
        let partialSceneNotPublished: UInt32 = 1 << 4
        self.sourceSplatCount = native.source_splat_count
        self.decodedSplatCount = native.decoded_splat_count
        self.encodedSplatCount = native.encoded_splat_count
        self.residentSplatCount = native.resident_splat_count
        self.addressableSplatCount = native.addressable_splat_count
        self.sourceShDegree = native.source_sh_degree
        self.residentShDegree = native.resident_sh_degree
        self.sourceMembershipAll = native.quality_flags & sourceMembershipAll != 0
        self.samplingDisabled = native.quality_flags & samplingDisabled != 0
        self.lodDisabled = native.quality_flags & lodDisabled != 0
        self.sourceShDegreePreserved = native.quality_flags & sourceShDegree != 0
        self.partialSceneNotPublished = native.quality_flags & partialSceneNotPublished != 0
        self.maxStorageBuffersPerShaderStage = native.max_storage_buffers_per_shader_stage
        self.maxStorageBufferBindingSize = native.max_storage_buffer_binding_size
        self.qualityFlags = native.quality_flags
    }
}

/// Native pixel-resolution and actual-presentation receipt for a Surface.
public struct GsplatSurfacePresentationReceipt: Equatable {
    public let requestedWidth: UInt32
    public let requestedHeight: UInt32
    public let surfaceWidth: UInt32
    public let surfaceHeight: UInt32
    public let internalRenderWidth: UInt32
    public let internalRenderHeight: UInt32
    public let presentedWidth: UInt32
    public let presentedHeight: UInt32
    public let presentedCameraRevision: UInt64
    public let lastFramePresented: Bool
    public let everPresented: Bool
    public let dynamicResolutionDisabled: Bool
    public let upscalingDisabled: Bool
    public let fullResolution: Bool
    public let flags: UInt32

    public var dimensionsMatch: Bool {
        requestedWidth == surfaceWidth &&
            requestedHeight == surfaceHeight &&
            surfaceWidth == internalRenderWidth &&
            surfaceHeight == internalRenderHeight &&
            internalRenderWidth == presentedWidth &&
            internalRenderHeight == presentedHeight
    }

    fileprivate init(_ native: GsplatSurfacePresentation) throws {
        let lastFramePresented = native.flags & (1 << 0) != 0
        let everPresented = native.flags & (1 << 1) != 0
        let dynamicResolutionDisabled = native.flags & (1 << 2) != 0
        let upscalingDisabled = native.flags & (1 << 3) != 0
        let fullResolution = native.flags & (1 << 4) != 0
        guard native.reserved == 0,
              native.requested_width > 0,
              native.requested_height > 0,
              native.surface_width > 0,
              native.surface_height > 0,
              native.internal_render_width > 0,
              native.internal_render_height > 0,
              !lastFramePresented || everPresented
        else {
            throw GsplatKitError(
                code: gsplatInvalidArgument,
                operation: "gsplat_surface_renderer_get_presentation",
                detail: "native Surface presentation receipt is invalid"
            )
        }

        self.requestedWidth = native.requested_width
        self.requestedHeight = native.requested_height
        self.surfaceWidth = native.surface_width
        self.surfaceHeight = native.surface_height
        self.internalRenderWidth = native.internal_render_width
        self.internalRenderHeight = native.internal_render_height
        self.presentedWidth = native.presented_width
        self.presentedHeight = native.presented_height
        self.presentedCameraRevision = native.presented_camera_revision
        self.lastFramePresented = lastFramePresented
        self.everPresented = everPresented
        self.dynamicResolutionDisabled = dynamicResolutionDisabled
        self.upscalingDisabled = upscalingDisabled
        self.fullResolution = fullResolution
        self.flags = native.flags

        guard !fullResolution || (
            lastFramePresented &&
                everPresented &&
                dynamicResolutionDisabled &&
                upscalingDisabled &&
                dimensionsMatch
        ) else {
            throw GsplatKitError(
                code: gsplatInvalidArgument,
                operation: "gsplat_surface_renderer_get_presentation",
                detail: "native full-resolution presentation receipt is inconsistent"
            )
        }
    }
}

public struct GsplatSurfaceOptions: Equatable {
    public var sortInterval: UInt32
    public var asyncSort: Bool
    public var frameLatency: UInt32
    public var geometryPath: GsplatGeometryPath
    public var orderBackend: GsplatOrderBackend
    public var projectedDrawPolicy: GsplatProjectedDrawPolicy

    public init(
        sortInterval: UInt32 = 1,
        asyncSort: Bool = false,
        frameLatency: UInt32 = 2,
        geometryPath: GsplatGeometryPath = .packedAtlas,
        orderBackend: GsplatOrderBackend = .adaptive,
        projectedDrawPolicy: GsplatProjectedDrawPolicy = .adaptive
    ) {
        self.sortInterval = sortInterval
        self.asyncSort = asyncSort
        self.frameLatency = frameLatency
        self.geometryPath = geometryPath
        self.orderBackend = orderBackend
        self.projectedDrawPolicy = projectedDrawPolicy
    }
}

public final class GsplatUIKitSurfaceRenderer {
    private var renderer: OpaquePointer?
    private let lock = NSLock()

    public init(
        view: UIView,
        viewController: UIViewController,
        datasetPath: String,
        width: UInt32,
        height: UInt32,
        options: GsplatSurfaceOptions = GsplatSurfaceOptions()
    ) throws {
        try GsplatKitVersion.requireSupported()

        var handle: OpaquePointer?
        let viewPointer = Unmanaged.passUnretained(view).toOpaque()
        let controllerPointer = Unmanaged.passUnretained(viewController).toOpaque()
        try datasetPath.withCString { pathPointer in
            try check(
                gsplat_surface_renderer_create_uikit_with_geometry_path(
                    viewPointer,
                    controllerPointer,
                    pathPointer,
                    width,
                    height,
                    options.geometryPath.rawValue,
                    &handle
                ),
                operation: "gsplat_surface_renderer_create_uikit_with_geometry_path"
            )
        }
        guard let handle else {
            throw GsplatKitError(
                code: gsplatInvalidArgument,
                operation: "gsplat_surface_renderer_create_uikit_with_geometry_path",
                detail: "native renderer was nil after a successful create call"
            )
        }

        do {
            try Self.apply(options: options, to: handle)
            renderer = handle
        } catch {
            gsplat_surface_renderer_destroy(handle)
            throw error
        }
    }

    deinit {
        close()
    }

    public func close() {
        lock.lock()
        defer { lock.unlock() }

        if let renderer {
            gsplat_surface_renderer_destroy(renderer)
            self.renderer = nil
        }
    }

    public func resize(width: UInt32, height: UInt32) throws {
        try withRenderer(operation: "gsplat_surface_renderer_resize") { renderer in
            try check(
                gsplat_surface_renderer_resize(renderer, width, height),
                operation: "gsplat_surface_renderer_resize"
            )
        }
    }

    public func resetCamera() throws {
        try withRenderer(operation: "gsplat_surface_renderer_reset_camera") { renderer in
            try check(
                gsplat_surface_renderer_reset_camera(renderer),
                operation: "gsplat_surface_renderer_reset_camera"
            )
        }
    }

    public func orbit(yawRadians: Float, pitchRadians: Float) throws {
        try withRenderer(operation: "gsplat_surface_renderer_orbit") { renderer in
            try check(
                gsplat_surface_renderer_orbit(renderer, yawRadians, pitchRadians),
                operation: "gsplat_surface_renderer_orbit"
            )
        }
    }

    public func zoom(distanceScale: Float) throws {
        try withRenderer(operation: "gsplat_surface_renderer_zoom") { renderer in
            try check(
                gsplat_surface_renderer_zoom(renderer, distanceScale),
                operation: "gsplat_surface_renderer_zoom"
            )
        }
    }

    public func pan(normalizedDeltaX: Float, normalizedDeltaY: Float) throws {
        try withRenderer(operation: "gsplat_surface_renderer_pan") { renderer in
            try check(
                gsplat_surface_renderer_pan(renderer, normalizedDeltaX, normalizedDeltaY),
                operation: "gsplat_surface_renderer_pan"
            )
        }
    }

    public func renderFrame() throws {
        try withRenderer(operation: "gsplat_surface_renderer_render_frame") { renderer in
            try check(
                gsplat_surface_renderer_render_frame(renderer),
                operation: "gsplat_surface_renderer_render_frame"
            )
        }
    }

    /// Legacy Surface compatibility snapshot.
    ///
    /// This method performs only the historical getter call. It never requests
    /// a current sample, renders a frame, polls readback, or caches an older
    /// value. Live consumers should use the current-stats v1 request,
    /// submission, and poll API instead.
    @available(*, deprecated, message: "Use requestCurrentStats(), currentStatsSubmission(), and pollCurrentStats() for live Surface counts")
    public func stats() throws -> GsplatFrameStats {
        try withRenderer(operation: "gsplat_surface_renderer_get_stats") { renderer in
            var stats = GsplatStats()
            try check(
                gsplat_surface_renderer_get_stats(renderer, &stats),
                operation: "gsplat_surface_renderer_get_stats"
            )
            return GsplatFrameStats(stats)
        }
    }

    /// Requests an optional current S/V/C/D sample from the next eligible frame.
    /// Busy and unavailable results are returned as values and do not throw.
    public func requestCurrentStats() throws -> GsplatCurrentStatsRequestStatus {
        try withRenderer(operation: "gsplat_surface_renderer_request_current_stats_v1") {
            renderer in
            var native = GsplatSurfaceCurrentStatsRequestV1()
            native.struct_size = UInt32(
                MemoryLayout<GsplatSurfaceCurrentStatsRequestV1>.size
            )
            native.version = 1
            try check(
                gsplat_surface_renderer_request_current_stats_v1(renderer, &native),
                operation: "gsplat_surface_renderer_request_current_stats_v1"
            )
            return try currentStatsRequestStatus(native: native)
        }
    }

    /// Returns the last successful frame's presentation-committed submission.
    public func currentStatsSubmission() throws -> GsplatCurrentStatsSubmission {
        try withRenderer(
            operation: "gsplat_surface_renderer_get_current_stats_submission_v1"
        ) { renderer in
            var native = GsplatSurfaceCurrentStatsSubmissionV1()
            native.struct_size = UInt32(
                MemoryLayout<GsplatSurfaceCurrentStatsSubmissionV1>.size
            )
            native.version = 1
            try check(
                gsplat_surface_renderer_get_current_stats_submission_v1(renderer, &native),
                operation: "gsplat_surface_renderer_get_current_stats_submission_v1"
            )
            return try GsplatCurrentStatsSubmission(native: native)
        }
    }

    /// Consumes at most one global current-stats resolution without blocking.
    public func pollCurrentStats() throws -> GsplatCurrentStatsPoll {
        try withRenderer(operation: "gsplat_surface_renderer_poll_current_stats_v1") {
            renderer in
            var native = GsplatSurfaceCurrentStatsPollV1()
            native.struct_size = UInt32(MemoryLayout<GsplatSurfaceCurrentStatsPollV1>.size)
            native.version = 1
            try check(
                gsplat_surface_renderer_poll_current_stats_v1(renderer, &native),
                operation: "gsplat_surface_renderer_poll_current_stats_v1"
            )
            return try GsplatCurrentStatsPoll(native: native)
        }
    }

    /// Returns the last frame's ordering backend, revisions, and Adaptive GPU status.
    public func orderStatus() throws -> GsplatSurfaceOrderStatus {
        try withRenderer(operation: "gsplat_surface_renderer_get_sort_stats") { renderer in
            var native = GsplatSurfaceSortStats()
            try check(
                gsplat_surface_renderer_get_sort_stats(renderer, &native),
                operation: "gsplat_surface_renderer_get_sort_stats"
            )
            return try GsplatSurfaceOrderStatus(native)
        }
    }

    /// Returns the CPU/GPU measurement submission identity for the last successful frame.
    public func orderSubmission() throws -> GsplatOrderSubmission {
        try withRenderer(operation: "gsplat_surface_renderer_get_order_submission") { renderer in
            var native = GsplatSurfaceOrderSubmission()
            try check(
                gsplat_surface_renderer_get_order_submission(renderer, &native),
                operation: "gsplat_surface_renderer_get_order_submission"
            )
            return try GsplatOrderSubmission(native)
        }
    }

    /// Changes projected Candidate/Compact/Adaptive execution transactionally.
    public func setProjectedDrawPolicy(_ policy: GsplatProjectedDrawPolicy) throws {
        try withRenderer(operation: "gsplat_surface_renderer_set_projected_policy_v1") { renderer in
            try check(
                gsplat_surface_renderer_set_projected_policy_v1(renderer, policy.rawValue),
                operation: "gsplat_surface_renderer_set_projected_policy_v1"
            )
        }
    }

    /// Returns projected-draw identity for the last successfully rendered frame.
    public func projectedDrawSubmission() throws -> GsplatProjectedDrawSubmission {
        try withRenderer(
            operation: "gsplat_surface_renderer_get_projected_submission_v1"
        ) { renderer in
            var native = GsplatSurfaceProjectedSubmissionV1()
            native.struct_size = UInt32(MemoryLayout<GsplatSurfaceProjectedSubmissionV1>.size)
            native.version = 1
            try check(
                gsplat_surface_renderer_get_projected_submission_v1(renderer, &native),
                operation: "gsplat_surface_renderer_get_projected_submission_v1"
            )
            return try GsplatProjectedDrawSubmission(native)
        }
    }

    /// Returns the current exactness/capability receipt for this Surface.
    public func exactness() throws -> GsplatSurfaceExactnessReceipt {
        try withRenderer(operation: "gsplat_surface_renderer_get_exactness") { renderer in
            var native = GsplatSurfaceExactness()
            try check(
                gsplat_surface_renderer_get_exactness(renderer, &native),
                operation: "gsplat_surface_renderer_get_exactness"
            )
            return GsplatSurfaceExactnessReceipt(native)
        }
    }

    /// Returns requested, Surface, internal-render, and presented pixel dimensions.
    public func presentation() throws -> GsplatSurfacePresentationReceipt {
        try withRenderer(operation: "gsplat_surface_renderer_get_presentation") { renderer in
            var native = GsplatSurfacePresentation()
            try check(
                gsplat_surface_renderer_get_presentation(renderer, &native),
                operation: "gsplat_surface_renderer_get_presentation"
            )
            return try GsplatSurfacePresentationReceipt(native)
        }
    }

    /// Returns one completed GPU order receipt, or nil without blocking.
    public func pollOrderMeasurement() throws -> GsplatOrderMeasurement? {
        try withRenderer(operation: "gsplat_surface_renderer_poll_order_measurement") { renderer in
            var native = GsplatSurfaceOrderMeasurement()
            var available: UInt32 = 0
            try check(
                gsplat_surface_renderer_poll_order_measurement(
                    renderer,
                    &native,
                    &available
                ),
                operation: "gsplat_surface_renderer_poll_order_measurement"
            )
            guard available != 0 else { return nil }
            let counts = try Self.takeOrderCounts(
                renderer,
                ticket: native.ticket,
                cameraRevision: native.camera_revision
            )
            return try GsplatOrderMeasurement(native, counts: counts)
        }
    }

    /// Drains all GPU order receipts currently available in native ticket order.
    public func drainOrderMeasurements() throws -> [GsplatOrderMeasurement] {
        try withRenderer(operation: "gsplat_surface_renderer_poll_order_measurement") { renderer in
            var completed: [GsplatOrderMeasurement] = []
            while true {
                var native = GsplatSurfaceOrderMeasurement()
                var available: UInt32 = 0
                try check(
                    gsplat_surface_renderer_poll_order_measurement(
                        renderer,
                        &native,
                        &available
                    ),
                    operation: "gsplat_surface_renderer_poll_order_measurement"
                )
                if available == 0 {
                    return completed
                }
                let counts = try Self.takeOrderCounts(
                    renderer,
                    ticket: native.ticket,
                    cameraRevision: native.camera_revision
                )
                completed.append(try GsplatOrderMeasurement(native, counts: counts))
            }
        }
    }

    /// Returns one CPU frame-start-to-queue-completion receipt, or nil without blocking.
    public func pollCpuOrderMeasurement() throws -> GsplatCpuOrderMeasurement? {
        try withRenderer(
            operation: "gsplat_surface_renderer_poll_cpu_order_measurement"
        ) { renderer in
            var native = GsplatSurfaceCpuOrderMeasurement()
            var available: UInt32 = 0
            try check(
                gsplat_surface_renderer_poll_cpu_order_measurement(
                    renderer,
                    &native,
                    &available
                ),
                operation: "gsplat_surface_renderer_poll_cpu_order_measurement"
            )
            guard available != 0 else { return nil }
            let counts = try Self.takeOrderCounts(
                renderer,
                ticket: native.ticket,
                cameraRevision: native.camera_revision
            )
            return try GsplatCpuOrderMeasurement(native, counts: counts)
        }
    }

    /// Drains all CPU queue-completion receipts currently available in ticket order.
    public func drainCpuOrderMeasurements() throws -> [GsplatCpuOrderMeasurement] {
        try withRenderer(
            operation: "gsplat_surface_renderer_poll_cpu_order_measurement"
        ) { renderer in
            var completed: [GsplatCpuOrderMeasurement] = []
            while true {
                var native = GsplatSurfaceCpuOrderMeasurement()
                var available: UInt32 = 0
                try check(
                    gsplat_surface_renderer_poll_cpu_order_measurement(
                        renderer,
                        &native,
                        &available
                    ),
                    operation: "gsplat_surface_renderer_poll_cpu_order_measurement"
                )
                if available == 0 { return completed }
                let counts = try Self.takeOrderCounts(
                    renderer,
                    ticket: native.ticket,
                    cameraRevision: native.camera_revision
                )
                completed.append(try GsplatCpuOrderMeasurement(native, counts: counts))
            }
        }
    }

    /// Returns one terminal CPU/GPU measurement failure, or nil without blocking.
    public func pollOrderMeasurementFailure() throws -> GsplatOrderMeasurementFailure? {
        try withRenderer(
            operation: "gsplat_surface_renderer_poll_order_measurement_failure"
        ) { renderer in
            var native = GsplatSurfaceOrderMeasurementFailure()
            var available: UInt32 = 0
            try check(
                gsplat_surface_renderer_poll_order_measurement_failure(
                    renderer,
                    &native,
                    &available
                ),
                operation: "gsplat_surface_renderer_poll_order_measurement_failure"
            )
            return available == 0 ? nil : try GsplatOrderMeasurementFailure(native)
        }
    }

    /// Drains all terminal CPU/GPU measurement failures currently available.
    public func drainOrderMeasurementFailures() throws -> [GsplatOrderMeasurementFailure] {
        try withRenderer(
            operation: "gsplat_surface_renderer_poll_order_measurement_failure"
        ) { renderer in
            var failures: [GsplatOrderMeasurementFailure] = []
            while true {
                var native = GsplatSurfaceOrderMeasurementFailure()
                var available: UInt32 = 0
                try check(
                    gsplat_surface_renderer_poll_order_measurement_failure(
                        renderer,
                        &native,
                        &available
                    ),
                    operation: "gsplat_surface_renderer_poll_order_measurement_failure"
                )
                if available == 0 { break }
                failures.append(try GsplatOrderMeasurementFailure(native))
            }
            return failures
        }
    }

    /// Returns one completed projected-draw probe and its exact V/C/D receipt.
    public func pollProjectedDrawMeasurement() throws -> GsplatProjectedDrawMeasurement? {
        try withRenderer(
            operation: "gsplat_surface_renderer_poll_projected_measurement_v1"
        ) { renderer in
            var native = GsplatSurfaceProjectedMeasurementV1()
            native.struct_size = UInt32(MemoryLayout<GsplatSurfaceProjectedMeasurementV1>.size)
            native.version = 1
            var available: UInt32 = 0
            try check(
                gsplat_surface_renderer_poll_projected_measurement_v1(
                    renderer,
                    &native,
                    &available
                ),
                operation: "gsplat_surface_renderer_poll_projected_measurement_v1"
            )
            try requireProjectedV1Header(
                size: native.struct_size,
                version: native.version,
                expectedSize: MemoryLayout<GsplatSurfaceProjectedMeasurementV1>.size,
                operation: "gsplat_surface_renderer_poll_projected_measurement_v1"
            )
            try requireProjectedAvailability(
                available,
                operation: "gsplat_surface_renderer_poll_projected_measurement_v1"
            )
            guard available != 0 else { return nil }
            let counts = try Self.takeProjectedDrawCounts(renderer, ticket: native.ticket)
            return try GsplatProjectedDrawMeasurement(native, counts: counts)
        }
    }

    /// Drains projected-draw successes currently available in native ticket order.
    public func drainProjectedDrawMeasurements() throws -> [GsplatProjectedDrawMeasurement] {
        try withRenderer(
            operation: "gsplat_surface_renderer_poll_projected_measurement_v1"
        ) { renderer in
            var completed: [GsplatProjectedDrawMeasurement] = []
            while true {
                var native = GsplatSurfaceProjectedMeasurementV1()
                native.struct_size = UInt32(MemoryLayout<GsplatSurfaceProjectedMeasurementV1>.size)
                native.version = 1
                var available: UInt32 = 0
                try check(
                    gsplat_surface_renderer_poll_projected_measurement_v1(
                        renderer,
                        &native,
                        &available
                    ),
                    operation: "gsplat_surface_renderer_poll_projected_measurement_v1"
                )
                try requireProjectedV1Header(
                    size: native.struct_size,
                    version: native.version,
                    expectedSize: MemoryLayout<GsplatSurfaceProjectedMeasurementV1>.size,
                    operation: "gsplat_surface_renderer_poll_projected_measurement_v1"
                )
                try requireProjectedAvailability(
                    available,
                    operation: "gsplat_surface_renderer_poll_projected_measurement_v1"
                )
                if available == 0 { return completed }
                let counts = try Self.takeProjectedDrawCounts(renderer, ticket: native.ticket)
                completed.append(try GsplatProjectedDrawMeasurement(native, counts: counts))
            }
        }
    }

    /// Returns one terminal projected-draw failure, or nil without blocking.
    public func pollProjectedDrawMeasurementFailure() throws
        -> GsplatProjectedDrawMeasurementFailure? {
        try withRenderer(
            operation: "gsplat_surface_renderer_poll_projected_failure_v1"
        ) { renderer in
            var native = GsplatSurfaceProjectedFailureV1()
            native.struct_size = UInt32(MemoryLayout<GsplatSurfaceProjectedFailureV1>.size)
            native.version = 1
            var available: UInt32 = 0
            try check(
                gsplat_surface_renderer_poll_projected_failure_v1(
                    renderer,
                    &native,
                    &available
                ),
                operation: "gsplat_surface_renderer_poll_projected_failure_v1"
            )
            try requireProjectedV1Header(
                size: native.struct_size,
                version: native.version,
                expectedSize: MemoryLayout<GsplatSurfaceProjectedFailureV1>.size,
                operation: "gsplat_surface_renderer_poll_projected_failure_v1"
            )
            try requireProjectedAvailability(
                available,
                operation: "gsplat_surface_renderer_poll_projected_failure_v1"
            )
            return available == 0 ? nil : try GsplatProjectedDrawMeasurementFailure(native)
        }
    }

    /// Drains terminal projected-draw failures currently available.
    public func drainProjectedDrawMeasurementFailures() throws
        -> [GsplatProjectedDrawMeasurementFailure] {
        try withRenderer(
            operation: "gsplat_surface_renderer_poll_projected_failure_v1"
        ) { renderer in
            var failures: [GsplatProjectedDrawMeasurementFailure] = []
            while true {
                var native = GsplatSurfaceProjectedFailureV1()
                native.struct_size = UInt32(MemoryLayout<GsplatSurfaceProjectedFailureV1>.size)
                native.version = 1
                var available: UInt32 = 0
                try check(
                    gsplat_surface_renderer_poll_projected_failure_v1(
                        renderer,
                        &native,
                        &available
                    ),
                    operation: "gsplat_surface_renderer_poll_projected_failure_v1"
                )
                try requireProjectedV1Header(
                    size: native.struct_size,
                    version: native.version,
                    expectedSize: MemoryLayout<GsplatSurfaceProjectedFailureV1>.size,
                    operation: "gsplat_surface_renderer_poll_projected_failure_v1"
                )
                try requireProjectedAvailability(
                    available,
                    operation: "gsplat_surface_renderer_poll_projected_failure_v1"
                )
                if available == 0 { return failures }
                failures.append(try GsplatProjectedDrawMeasurementFailure(native))
            }
        }
    }

    /// Drains successes and failures under one ownership lock.
    ///
    /// Strict collectors should prefer this over separate drain calls so frame
    /// progression cannot interleave between the two terminal queues.
    public func drainProjectedDrawTerminals() throws -> GsplatProjectedDrawTerminalBatch {
        try withRenderer(operation: "gsplat_surface_renderer_poll_projected_measurement_v1") {
            renderer in
            var measurements: [GsplatProjectedDrawMeasurement] = []
            while true {
                var native = GsplatSurfaceProjectedMeasurementV1()
                native.struct_size = UInt32(
                    MemoryLayout<GsplatSurfaceProjectedMeasurementV1>.size
                )
                native.version = 1
                var available: UInt32 = 0
                try check(
                    gsplat_surface_renderer_poll_projected_measurement_v1(
                        renderer,
                        &native,
                        &available
                    ),
                    operation: "gsplat_surface_renderer_poll_projected_measurement_v1"
                )
                try requireProjectedV1Header(
                    size: native.struct_size,
                    version: native.version,
                    expectedSize: MemoryLayout<GsplatSurfaceProjectedMeasurementV1>.size,
                    operation: "gsplat_surface_renderer_poll_projected_measurement_v1"
                )
                try requireProjectedAvailability(
                    available,
                    operation: "gsplat_surface_renderer_poll_projected_measurement_v1"
                )
                guard available != 0 else { break }
                let counts = try Self.takeProjectedDrawCounts(renderer, ticket: native.ticket)
                measurements.append(try GsplatProjectedDrawMeasurement(native, counts: counts))
            }

            var failures: [GsplatProjectedDrawMeasurementFailure] = []
            while true {
                var native = GsplatSurfaceProjectedFailureV1()
                native.struct_size = UInt32(MemoryLayout<GsplatSurfaceProjectedFailureV1>.size)
                native.version = 1
                var available: UInt32 = 0
                try check(
                    gsplat_surface_renderer_poll_projected_failure_v1(
                        renderer,
                        &native,
                        &available
                    ),
                    operation: "gsplat_surface_renderer_poll_projected_failure_v1"
                )
                try requireProjectedV1Header(
                    size: native.struct_size,
                    version: native.version,
                    expectedSize: MemoryLayout<GsplatSurfaceProjectedFailureV1>.size,
                    operation: "gsplat_surface_renderer_poll_projected_failure_v1"
                )
                try requireProjectedAvailability(
                    available,
                    operation: "gsplat_surface_renderer_poll_projected_failure_v1"
                )
                guard available != 0 else { break }
                failures.append(try GsplatProjectedDrawMeasurementFailure(native))
            }
            return GsplatProjectedDrawTerminalBatch(
                measurements: measurements,
                failures: failures
            )
        }
    }

    /// Takes the additive count receipt immediately after the legacy terminal
    /// receipt so no caller can accidentally join V/C/D from another revision.
    private static func takeOrderCounts(
        _ renderer: OpaquePointer,
        ticket: UInt64,
        cameraRevision: UInt64
    ) throws -> GsplatSurfaceOrderCounts {
        var counts = GsplatSurfaceOrderCounts()
        var available: UInt32 = 0
        try check(
            gsplat_surface_renderer_take_order_counts(
                renderer,
                ticket,
                &counts,
                &available
            ),
            operation: "gsplat_surface_renderer_take_order_counts"
        )
        guard available != 0,
              counts.ticket == ticket,
              counts.camera_revision == cameraRevision
        else {
            throw GsplatKitError(
                code: gsplatInvalidArgument,
                operation: "gsplat_surface_renderer_take_order_counts",
                detail: "terminal order receipt lacks matching V/C/D counts"
            )
        }
        return counts
    }

    private static func takeProjectedDrawCounts(
        _ renderer: OpaquePointer,
        ticket: UInt64
    ) throws -> GsplatSurfaceProjectedCountsV1 {
        var counts = GsplatSurfaceProjectedCountsV1()
        counts.struct_size = UInt32(MemoryLayout<GsplatSurfaceProjectedCountsV1>.size)
        counts.version = 1
        var available: UInt32 = 0
        try check(
            gsplat_surface_renderer_take_projected_counts_v1(
                renderer,
                ticket,
                &counts,
                &available
            ),
            operation: "gsplat_surface_renderer_take_projected_counts_v1"
        )
        try requireProjectedV1Header(
            size: counts.struct_size,
            version: counts.version,
            expectedSize: MemoryLayout<GsplatSurfaceProjectedCountsV1>.size,
            operation: "gsplat_surface_renderer_take_projected_counts_v1"
        )
        try requireProjectedAvailability(
            available,
            operation: "gsplat_surface_renderer_take_projected_counts_v1"
        )
        guard available != 0, counts.ticket == ticket else {
            throw GsplatKitError(
                code: gsplatInvalidArgument,
                operation: "gsplat_surface_renderer_take_projected_counts_v1",
                detail: "terminal projected-draw receipt lacks matching V/C/D counts"
            )
        }
        return counts
    }

    private static func apply(options: GsplatSurfaceOptions, to renderer: OpaquePointer) throws {
        guard options.sortInterval > 0 else {
            throw GsplatKitError(
                code: gsplatInvalidArgument,
                operation: "GsplatSurfaceOptions",
                detail: "sortInterval must be positive"
            )
        }
        guard (1...4).contains(options.frameLatency) else {
            throw GsplatKitError(
                code: gsplatInvalidArgument,
                operation: "GsplatSurfaceOptions",
                detail: "frameLatency must be in 1...4"
            )
        }
        guard !options.asyncSort || options.orderBackend == .cpu else {
            throw GsplatKitError(
                code: gsplatInvalidArgument,
                operation: "GsplatSurfaceOptions",
                detail: "asyncSort is available only with the CPU order backend"
            )
        }
        guard options.geometryPath != .pagedActiveAtlas || options.orderBackend == .cpu else {
            throw GsplatKitError(
                code: gsplatInvalidArgument,
                operation: "GsplatSurfaceOptions",
                detail: "pagedActiveAtlas is diagnostic-only and supports the CPU order backend"
            )
        }

        var steps: [(String, Int32)] = [
            (
                "gsplat_surface_renderer_set_sort_interval",
                gsplat_surface_renderer_set_sort_interval(renderer, options.sortInterval)
            )
        ]
        if options.asyncSort {
            steps.append((
                "gsplat_surface_renderer_set_order_backend",
                gsplat_surface_renderer_set_order_backend(renderer, GsplatOrderBackend.cpu.rawValue)
            ))
            steps.append((
                "gsplat_surface_renderer_set_async_sort",
                gsplat_surface_renderer_set_async_sort(renderer, 1)
            ))
        } else {
            steps.append((
                "gsplat_surface_renderer_set_async_sort",
                gsplat_surface_renderer_set_async_sort(renderer, 0)
            ))
            steps.append((
                "gsplat_surface_renderer_set_order_backend",
                gsplat_surface_renderer_set_order_backend(renderer, options.orderBackend.rawValue)
            ))
        }
        steps.append(
            (
                "gsplat_surface_renderer_set_frame_latency",
                gsplat_surface_renderer_set_frame_latency(renderer, options.frameLatency)
            )
        )
        steps.append(
            (
                "gsplat_surface_renderer_set_projected_policy_v1",
                gsplat_surface_renderer_set_projected_policy_v1(
                    renderer,
                    options.projectedDrawPolicy.rawValue
                )
            )
        )

        for (operation, rc) in steps {
            try check(rc, operation: operation)
        }
    }

    private func withRenderer<Result>(
        operation: String,
        _ body: (OpaquePointer) throws -> Result
    ) throws -> Result {
        lock.lock()
        defer { lock.unlock() }

        guard let renderer else {
            throw GsplatKitError(
                code: gsplatInvalidArgument,
                operation: operation,
                detail: "surface renderer is closed"
            )
        }
        return try body(renderer)
    }
}
#endif

private func requireProjectedV1Header(
    size: UInt32,
    version: UInt32,
    expectedSize: Int,
    operation: String
) throws {
    guard size == UInt32(expectedSize), version == 1 else {
        throw GsplatKitError(
            code: gsplatInvalidArgument,
            operation: operation,
            detail: "native projected-draw V1 receipt has an incompatible size or version"
        )
    }
}

// Projected tickets are opaque ABI identities; only zero means "no ticket".
func isProjectedDrawTicket(_ ticket: UInt64) -> Bool {
    ticket != 0
}

private func requireProjectedAvailability(_ available: UInt32, operation: String) throws {
    guard available <= 1 else {
        throw GsplatKitError(
            code: gsplatInvalidArgument,
            operation: operation,
            detail: "native projected-draw V1 availability must equal 0 or 1"
        )
    }
}

private func check(_ code: Int32, operation: String) throws {
    guard code == gsplatOk else {
        let detail = String(cString: gsplat_last_error_message())
        throw GsplatKitError(
            code: code,
            operation: operation,
            detail: detail == "ok" ? nil : detail
        )
    }
}
