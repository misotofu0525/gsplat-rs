import CryptoKit
import Foundation
import UIKit

private let benchmarkSchema = "gsplat-benchmark/v1"
private let countSemantics = "candidate_visible_contributor_issued_v1"
private let gpuExactContributorDraw: UInt32 = 1 << 6
private let cpuExactContributorDraw: UInt32 = 1 << 1
private let cpuContributorCountValid: UInt32 = 1 << 2
private let orderCountsExactContributorDraw: UInt32 = 1 << 0
private let projectedSubmissionTicketIssued: UInt32 = 1 << 0
private let projectedSubmissionRingBusy: UInt32 = 1 << 1
private let projectedSubmissionSurfaceUnavailable: UInt32 = 1 << 2
private let projectedMeasurementProjectionRebuilt: UInt32 = 1 << 0
private let projectedMeasurementOrderRefreshed: UInt32 = 1 << 1
private let projectedMeasurementExactContributorDraw: UInt32 = 1 << 2
private let projectedMeasurementDroppedPrior: UInt32 = 1 << 3
private let projectedCountsExactContributorDraw: UInt32 = 1 << 0
private let projectedFailureDroppedPrior: UInt32 = 1 << 0

struct BenchmarkSample {
    let elapsedNs: UInt64
    let callMs: Double
    let frameWallMs: Double
    let cameraRevision: UInt64
    let appliedOrderRevision: UInt64
    let sortRefreshed: Bool
    let actualOrderBackend: UInt32
    let adaptiveState: UInt32
    let gpuSortFallback: Bool
    let adaptiveGpuFailure: UInt32?
    let submittedMeasurementTicket: UInt64?
    let submissionFlags: UInt32
    let projectedPolicy: UInt32
    let projectedExecution: UInt32
    let projectedAdaptiveState: UInt32
    let projectedSubmittedMeasurementTicket: UInt64?
    let projectedSubmissionExecution: UInt32?
    let projectedUnsampledReason: String?
    let projectedSubmissionFlags: UInt32
    let currentStatsTicket: UInt64
    let traceFrameIndex: Int?
    let traceTimestampNs: UInt64?
    let traceLoopIndex: Int?
}

private struct IssuedCurrentStatsSample: Equatable {
    let sampleIndex: Int
    let sampleKey: String
    let traceKey: String
    let submission: GsplatCurrentStatsIssuedSubmission
}

private struct IssuedOrderTicket: Equatable {
    let cameraRevision: UInt64
    let backend: UInt32
}

private struct IssuedProjectedTicket: Equatable {
    let cameraRevision: UInt64
    let execution: UInt32
    let orderBackend: UInt32
}

private struct CompletedProjectedMeasurement {
    let measurement: GsplatSurfaceProjectedMeasurementV1
    let counts: GsplatSurfaceProjectedCountsV1
}

private struct ProjectedTerminalIdentity: Equatable {
    let cameraRevision: UInt64
    let projectionGeneration: UInt64
    let probeGeneration: UInt64
    let execution: UInt32
    let orderBackend: UInt32
}

final class SurfaceBenchmark {
    let config: BenchmarkConfig
    private(set) var complete = false
    private var observedFrames = 0
    private var samples: [BenchmarkSample]
    private var runStartedAt = Date()
    private var measurementStartedAt: Date?
    private var measurementEndedAt: Date?
    private var measurementStartNs: UInt64?
    private var previousFrameStartNs: UInt64?
    private let initialThermalState = ProcessInfo.processInfo.thermalState
    private var finalThermalState: ProcessInfo.ThermalState?
    private var issuedOrderTickets: [UInt64: IssuedOrderTicket] = [:]
    private var completedGpuTickets: [UInt64: GsplatSurfaceOrderMeasurement] = [:]
    private var completedCpuTickets: [UInt64: GsplatSurfaceCpuOrderMeasurement] = [:]
    private var completedCountsByTicket: [UInt64: GsplatSurfaceOrderCounts] = [:]
    private var failedOrderTickets: [UInt64: GsplatSurfaceOrderMeasurementFailure] = [:]
    private var unsampledOrderRequests: [String] = []
    private var orderLedgerErrors: [String] = []
    private var issuedProjectedTickets: [UInt64: IssuedProjectedTicket] = [:]
    private var completedProjectedTickets: [UInt64: CompletedProjectedMeasurement] = [:]
    private var failedProjectedTickets: [UInt64: GsplatSurfaceProjectedFailureV1] = [:]
    private var projectedTerminalIdentities: [UInt64: ProjectedTerminalIdentity] = [:]
    private var unsampledProjectedRequests: [String] = []
    private var projectedLedgerErrors: [String] = []
    private var issuedCurrentStatsSamples: [UInt64: IssuedCurrentStatsSample] = [:]
    private var completedCurrentStatsTickets: [UInt64: GsplatCurrentStatsReceipt] = [:]
    private var failedCurrentStatsTickets: [UInt64: GsplatCurrentStatsFailure] = [:]
    private var currentStatsLedgerErrors: [String] = []

    init(config: BenchmarkConfig) {
        self.config = config
        samples = []
        samples.reserveCapacity(config.measuredSampleCount)
    }

    func nextTraceStep() -> CameraTraceStep? {
        guard config.enabled, !complete, config.cameraTraceSequence,
              let metadata = config.cameraTraceMetadata else { return nil }
        let measuredIndex = observedFrames - config.warmupFrames
        let warmup = measuredIndex < 0
        let loopIndex = warmup ? 0 : measuredIndex / config.frames
        let phaseFrameIndex = warmup ? observedFrames : measuredIndex % config.frames
        let traceFrameIndex = config.cameraTraceFrameIndices[
            phaseFrameIndex % config.cameraTraceFrameIndices.count
        ]
        return CameraTraceStep(
            phase: warmup ? "warmup" : "measure",
            loopIndex: loopIndex,
            phaseFrameIndex: phaseFrameIndex,
            measuredSampleIndex: warmup ? nil : measuredIndex,
            traceFrameIndex: traceFrameIndex,
            timestampNs: metadata.timestamps[traceFrameIndex]
        )
    }

    var nextFrameRequiresCurrentStats: Bool {
        config.enabled && !complete && observedFrames >= config.warmupFrames
    }

    func recordWarmupFrame(frameStartNs: UInt64) {
        guard config.enabled, !complete, observedFrames < config.warmupFrames else {
            return
        }
        observedFrames += 1
        previousFrameStartNs = frameStartNs
    }

    func recordCurrentStatsError(_ error: String) {
        currentStatsLedgerErrors.append(error)
    }

    func recordCurrentStatsSubmission(
        _ submission: GsplatCurrentStatsIssuedSubmission,
        traceStep: CameraTraceStep?
    ) {
        let sampleIndex = samples.count
        if let traceStep {
            guard traceStep.phase == "measure",
                  traceStep.measuredSampleIndex == sampleIndex else {
                currentStatsLedgerErrors.append(
                    "current-stats submission does not match the measured trace sample"
                )
                return
            }
        }
        let sampleKey = "measure:\(sampleIndex)"
        let traceKey = currentStatsTraceKey(
            sampleIndex: sampleIndex,
            traceStep: traceStep
        )
        let issued = IssuedCurrentStatsSample(
            sampleIndex: sampleIndex,
            sampleKey: sampleKey,
            traceKey: traceKey,
            submission: submission
        )
        if issuedCurrentStatsSamples.updateValue(issued, forKey: submission.ticket) != nil {
            currentStatsLedgerErrors.append("current-stats ticket was issued more than once")
        }
    }

    func recordCurrentStatsEvent(_ event: GsplatCurrentStatsConsumerEvent) {
        switch event {
        case .empty:
            return
        case .unsampled(let status):
            currentStatsLedgerErrors.append(
                "current-stats request reached an unsampled terminal: \(requestStatusName(status))"
            )
        case .ready(let receipt):
            guard let issued = issuedCurrentStatsSamples[receipt.ticket],
                  issued.submission.identity == receipt.identity else {
                currentStatsLedgerErrors.append(
                    "current-stats Ready does not match an issued ticket and full identity"
                )
                return
            }
            if failedCurrentStatsTickets[receipt.ticket] != nil {
                currentStatsLedgerErrors.append(
                    "current-stats ticket produced both Ready and failure terminals"
                )
            }
            if completedCurrentStatsTickets.updateValue(
                receipt,
                forKey: receipt.ticket
            ) != nil {
                currentStatsLedgerErrors.append(
                    "current-stats ticket produced more than one Ready terminal"
                )
            }
            if let error = currentStatsCountContractError(receipt) {
                currentStatsLedgerErrors.append(error)
            }
        case .failure(let failure):
            guard let issued = issuedCurrentStatsSamples[failure.ticket],
                  issued.submission.identity == failure.identity else {
                currentStatsLedgerErrors.append(
                    "current-stats failure does not match an issued ticket and full identity"
                )
                return
            }
            if completedCurrentStatsTickets[failure.ticket] != nil {
                currentStatsLedgerErrors.append(
                    "current-stats ticket produced both Ready and failure terminals"
                )
            }
            if failedCurrentStatsTickets.updateValue(
                failure,
                forKey: failure.ticket
            ) != nil {
                currentStatsLedgerErrors.append(
                    "current-stats ticket produced more than one failure terminal"
                )
            }
            currentStatsLedgerErrors.append(
                "current-stats ticket \(failure.ticket) failed reason=" +
                    failureReasonName(failure.reason)
            )
        case .rejected(let rejection):
            switch rejection {
            case .unknownTerminal(let ticket):
                currentStatsLedgerErrors.append(
                    "current-stats consumer rejected unknown terminal ticket \(ticket)"
                )
            case .identityMismatch(let ticket, _, _):
                currentStatsLedgerErrors.append(
                    "current-stats consumer rejected full identity drift for ticket \(ticket)"
                )
            }
        case .notRequested, .pending, .settled:
            currentStatsLedgerErrors.append(
                "current-stats poll produced a non-terminal consumer event"
            )
        }
    }

    func recordOrderSubmission(_ submission: GsplatSurfaceOrderSubmission) {
        let gpuRefresh = submission.flags & 1 != 0
        let ticketIssued = submission.flags & (1 << 1) != 0
        let ringBusy = submission.flags & (1 << 2) != 0
        let cpuSample = submission.flags & (1 << 3) != 0
        let surfaceUnavailable = submission.flags & (1 << 4) != 0
        guard submission.requested_backend == requestedBackendValue() else {
            orderLedgerErrors.append("submission requested backend mismatch")
            return
        }
        guard submission.adaptive_state <= 6 else {
            orderLedgerErrors.append("submission adaptive state is invalid")
            return
        }
        if gpuRefresh && cpuSample {
            orderLedgerErrors.append("submission requested both CPU and GPU measurement")
            return
        }
        if ringBusy && surfaceUnavailable {
            orderLedgerErrors.append("submission published multiple unsampled reasons")
            return
        }
        let backend: UInt32?
        if gpuRefresh {
            backend = 1
        } else if cpuSample {
            backend = 0
        } else {
            backend = nil
        }
        guard let backend else {
            if ticketIssued || ringBusy || surfaceUnavailable || submission.ticket != 0 {
                orderLedgerErrors.append("non-measured frame published measurement state")
            }
            return
        }
        guard submission.actual_backend == backend else {
            orderLedgerErrors.append("measurement submission backend does not match the frame")
            return
        }
        if !ticketIssued {
            if !ringBusy && !surfaceUnavailable {
                orderLedgerErrors.append("measurement request omitted ticket without unsampled evidence")
            }
            let reason = ringBusy ? "ring_busy" : "surface_unavailable"
            unsampledOrderRequests.append(
                "backend=\(backend) revision=\(submission.camera_revision) reason=\(reason)"
            )
            return
        }
        guard submission.ticket > 0,
              !ringBusy,
              !surfaceUnavailable else {
            orderLedgerErrors.append("issued order ticket has invalid submission flags")
            return
        }
        if issuedOrderTickets.updateValue(
            IssuedOrderTicket(cameraRevision: submission.camera_revision, backend: backend),
            forKey: submission.ticket
        ) != nil {
            orderLedgerErrors.append("order ticket was issued more than once")
        }
    }

    func recordProjectedSubmission(
        _ submission: GsplatSurfaceProjectedSubmissionV1,
        orderSubmission: GsplatSurfaceOrderSubmission
    ) {
        let ticketIssued = submission.flags & projectedSubmissionTicketIssued != 0
        let ringBusy = submission.flags & projectedSubmissionRingBusy != 0
        let surfaceUnavailable = submission.flags & projectedSubmissionSurfaceUnavailable != 0
        let requestedPolicy = requestedProjectedPolicyValue()
        guard submission.requested_policy == requestedPolicy else {
            projectedLedgerErrors.append("submission requested projected policy mismatch")
            return
        }
        guard (1...2).contains(submission.actual_execution),
              submission.order_backend <= 1,
              submission.adaptive_state <= 7,
              submission.flags & ~UInt32(0b111) == 0,
              submission.reserved == 0 else {
            projectedLedgerErrors.append("submission contains an invalid projected enum or flag")
            return
        }
        guard submission.camera_revision == orderSubmission.camera_revision,
              submission.order_backend == orderSubmission.actual_backend else {
            projectedLedgerErrors.append("projected submission disagrees with frame order identity")
            return
        }
        if ringBusy && surfaceUnavailable {
            projectedLedgerErrors.append("projected submission published multiple unsampled reasons")
            return
        }
        if requestedPolicy != 3 {
            guard submission.actual_execution == requestedPolicy,
                  submission.adaptive_state == 0,
                  !ticketIssued,
                  !ringBusy,
                  !surfaceUnavailable,
                  submission.ticket == 0 else {
                projectedLedgerErrors.append("forced projected policy fabricated or changed execution")
                return
            }
            return
        }

        if ticketIssued {
            guard isProjectedDrawTicket(submission.ticket), !ringBusy, !surfaceUnavailable else {
                projectedLedgerErrors.append("Adaptive projected submission issued an invalid ticket")
                return
            }
            if orderSubmission.flags & (1 << 1) != 0 {
                projectedLedgerErrors.append("one frame issued both order and projected formal tickets")
                return
            }
            if issuedOrderTickets[submission.ticket] != nil {
                projectedLedgerErrors.append("projected ticket crossed into the order ticket ledger")
                return
            }
            let identity = IssuedProjectedTicket(
                cameraRevision: submission.camera_revision,
                execution: submission.actual_execution,
                orderBackend: submission.order_backend
            )
            if issuedProjectedTickets.updateValue(identity, forKey: submission.ticket) != nil {
                projectedLedgerErrors.append("projected ticket was issued more than once")
            }
            return
        }

        guard submission.ticket == 0 else {
            projectedLedgerErrors.append("projected submission exposed an unissued ticket")
            return
        }
        if ringBusy || surfaceUnavailable {
            let reason = ringBusy ? "ring_busy" : "surface_unavailable"
            unsampledProjectedRequests.append(
                "execution=\(submission.actual_execution) " +
                    "revision=\(submission.camera_revision) reason=\(reason)"
            )
        }
    }

    func recordOrderMeasurement(
        _ measurement: GsplatSurfaceOrderMeasurement,
        counts: GsplatSurfaceOrderCounts
    ) {
        guard issuedOrderTickets[measurement.ticket] == IssuedOrderTicket(
            cameraRevision: measurement.camera_revision,
            backend: 1
        ) else {
            orderLedgerErrors.append("GPU success does not match an issued ticket/revision")
            return
        }
        if measurement.requested_backend != requestedBackendValue() ||
            measurement.actual_backend != 1 || measurement.adaptive_state > 6 {
            orderLedgerErrors.append("GPU success has invalid backend or adaptive context")
        }
        if failedOrderTickets[measurement.ticket] != nil || completedCpuTickets[measurement.ticket] != nil {
            orderLedgerErrors.append("GPU ticket produced both success and failure")
        }
        if completedGpuTickets.updateValue(measurement, forKey: measurement.ticket) != nil {
            orderLedgerErrors.append("GPU ticket produced multiple success receipts")
        }
        if measurement.flags & (1 << 5) != 0 {
            orderLedgerErrors.append("GPU success queue dropped an older terminal receipt")
        }
        let exactContributorCompaction = counts.flags & orderCountsExactContributorDraw != 0
        if counts.ticket != measurement.ticket ||
            counts.camera_revision != measurement.camera_revision ||
            counts.visible_count != measurement.visible_count ||
            counts.drawn_count != measurement.drawn_count ||
            exactContributorCompaction != (measurement.flags & gpuExactContributorDraw != 0) {
            orderLedgerErrors.append("GPU timing and V/C/D receipts disagree")
        }
        if let error = countContractError(
            visible: counts.visible_count,
            contributor: counts.contributor_count,
            drawn: counts.drawn_count,
            exactContributorCompaction: exactContributorCompaction
        ) {
            orderLedgerErrors.append("GPU success count contract: \(error)")
        }
        recordCounts(counts)
    }

    func recordCpuOrderMeasurement(
        _ measurement: GsplatSurfaceCpuOrderMeasurement,
        counts: GsplatSurfaceOrderCounts
    ) {
        guard issuedOrderTickets[measurement.ticket] == IssuedOrderTicket(
            cameraRevision: measurement.camera_revision,
            backend: 0
        ) else {
            orderLedgerErrors.append("CPU success does not match an issued ticket/revision")
            return
        }
        if measurement.requested_backend != requestedBackendValue() ||
            measurement.actual_backend != 0 || measurement.adaptive_state > 6 {
            orderLedgerErrors.append("CPU success has invalid backend or adaptive context")
        }
        if failedOrderTickets[measurement.ticket] != nil || completedGpuTickets[measurement.ticket] != nil {
            orderLedgerErrors.append("CPU ticket produced both success and another terminal")
        }
        if completedCpuTickets.updateValue(measurement, forKey: measurement.ticket) != nil {
            orderLedgerErrors.append("CPU ticket produced multiple success receipts")
        }
        if measurement.flags & 1 != 0 {
            orderLedgerErrors.append("CPU success queue dropped an older terminal receipt")
        }
        if measurement.flags & cpuExactContributorDraw != 0 &&
            measurement.flags & cpuContributorCountValid == 0 {
            orderLedgerErrors.append("CPU exact contributor success omitted C")
        }
        let exactContributorCompaction = counts.flags & orderCountsExactContributorDraw != 0
        if counts.ticket != measurement.ticket ||
            counts.camera_revision != measurement.camera_revision ||
            measurement.flags & cpuContributorCountValid == 0 ||
            measurement.reserved != counts.contributor_count ||
            exactContributorCompaction != (measurement.flags & cpuExactContributorDraw != 0) {
            orderLedgerErrors.append("CPU timing and V/C/D receipts disagree")
        }
        if let error = countContractError(
            visible: counts.visible_count,
            contributor: counts.contributor_count,
            drawn: counts.drawn_count,
            exactContributorCompaction: exactContributorCompaction
        ) {
            orderLedgerErrors.append("CPU success count contract: \(error)")
        }
        if !measurement.preprocess_ms.isFinite || measurement.preprocess_ms < 0 ||
            !measurement.sort_ms.isFinite || measurement.sort_ms < 0 ||
            !measurement.frame_complete_ms.isFinite || measurement.frame_complete_ms < 0 {
            orderLedgerErrors.append("CPU success contains invalid timing")
        }
        recordCounts(counts)
    }

    private func recordCounts(_ counts: GsplatSurfaceOrderCounts) {
        if completedCountsByTicket.updateValue(counts, forKey: counts.ticket) != nil {
            orderLedgerErrors.append("order ticket produced multiple V/C/D receipts")
        }
    }

    func recordOrderMeasurementFailure(_ failure: GsplatSurfaceOrderMeasurementFailure) {
        guard let issued = issuedOrderTickets[failure.ticket],
              issued.cameraRevision == failure.camera_revision else {
            orderLedgerErrors.append("order failure does not match an issued ticket/revision")
            return
        }
        if failure.requested_backend != requestedBackendValue() ||
            failure.actual_backend != issued.backend || failure.adaptive_state > 6 {
            orderLedgerErrors.append("order failure has invalid backend or adaptive context")
        }
        if failure.reason != 1 && failure.reason != 2 {
            orderLedgerErrors.append("order failure has an unknown terminal reason")
        }
        if issued.backend == 0 && failure.reason == 1 {
            orderLedgerErrors.append("CPU ticket reported a GPU-only readback failure")
        }
        if completedGpuTickets[failure.ticket] != nil || completedCpuTickets[failure.ticket] != nil {
            orderLedgerErrors.append("order ticket produced both success and failure")
        }
        if failedOrderTickets.updateValue(failure, forKey: failure.ticket) != nil {
            orderLedgerErrors.append("order ticket produced multiple failure receipts")
        }
        if failure.flags & 1 != 0 {
            orderLedgerErrors.append("GPU failure queue dropped an older terminal receipt")
        }
    }

    func recordProjectedMeasurement(
        _ measurement: GsplatSurfaceProjectedMeasurementV1,
        counts: GsplatSurfaceProjectedCountsV1
    ) {
        let terminalIdentity = ProjectedTerminalIdentity(
            cameraRevision: measurement.camera_revision,
            projectionGeneration: measurement.projection_generation,
            probeGeneration: measurement.probe_generation,
            execution: measurement.execution,
            orderBackend: measurement.order_backend
        )
        guard recordProjectedTerminalIdentity(
            ticket: measurement.ticket,
            identity: terminalIdentity
        ) else { return }
        guard measurement.struct_size == UInt32(MemoryLayout<GsplatSurfaceProjectedMeasurementV1>.size),
              measurement.version == 1,
              counts.struct_size == UInt32(MemoryLayout<GsplatSurfaceProjectedCountsV1>.size),
              counts.version == 1,
              measurement.frame_complete_ms.isFinite,
              measurement.frame_complete_ms >= 0,
              measurement.flags & ~UInt32(0b1111) == 0,
              counts.flags & ~UInt32(0b1) == 0 else {
            projectedLedgerErrors.append("projected success contains an invalid V1 header, timing, or flag")
            return
        }
        let projectionRebuilt = measurement.flags & projectedMeasurementProjectionRebuilt != 0
        let orderRefreshed = measurement.flags & projectedMeasurementOrderRefreshed != 0
        let exactContributorDraw =
            measurement.flags & projectedMeasurementExactContributorDraw != 0
        let countsExactContributorDraw = counts.flags & projectedCountsExactContributorDraw != 0
        guard projectionRebuilt,
              !orderRefreshed,
              measurement.flags & projectedMeasurementDroppedPrior == 0,
              counts.ticket == measurement.ticket,
              counts.camera_revision == measurement.camera_revision,
              counts.contributor_count <= counts.visible_count,
              exactContributorDraw == countsExactContributorDraw,
              measurement.execution == 1
                ? (!exactContributorDraw && counts.drawn_count == counts.visible_count)
                : (exactContributorDraw && counts.drawn_count == counts.contributor_count) else {
            projectedLedgerErrors.append("projected success violates formal projection or S/V/C/D identity")
            return
        }
        if failedProjectedTickets[measurement.ticket] != nil {
            projectedLedgerErrors.append("projected ticket produced both success and failure")
            return
        }
        if completedProjectedTickets.updateValue(
            CompletedProjectedMeasurement(measurement: measurement, counts: counts),
            forKey: measurement.ticket
        ) != nil {
            projectedLedgerErrors.append("projected ticket produced multiple success receipts")
        }
    }

    func recordProjectedMeasurementFailure(_ failure: GsplatSurfaceProjectedFailureV1) {
        let terminalIdentity = ProjectedTerminalIdentity(
            cameraRevision: failure.camera_revision,
            projectionGeneration: failure.projection_generation,
            probeGeneration: failure.probe_generation,
            execution: failure.execution,
            orderBackend: failure.order_backend
        )
        guard recordProjectedTerminalIdentity(ticket: failure.ticket, identity: terminalIdentity)
        else { return }
        guard failure.struct_size == UInt32(MemoryLayout<GsplatSurfaceProjectedFailureV1>.size),
              failure.version == 1,
              (1...3).contains(failure.reason),
              failure.flags & ~UInt32(0b1) == 0 else {
            projectedLedgerErrors.append("projected failure contains an invalid V1 header or enum")
            return
        }
        if failure.flags & projectedFailureDroppedPrior != 0 {
            projectedLedgerErrors.append("projected failure queue dropped an older terminal receipt")
        }
        if completedProjectedTickets[failure.ticket] != nil {
            projectedLedgerErrors.append("projected ticket produced both success and failure")
        }
        if failedProjectedTickets.updateValue(failure, forKey: failure.ticket) != nil {
            projectedLedgerErrors.append("projected ticket produced multiple failure receipts")
        }
    }

    private func recordProjectedTerminalIdentity(
        ticket: UInt64,
        identity: ProjectedTerminalIdentity
    ) -> Bool {
        guard isProjectedDrawTicket(ticket),
              issuedProjectedTickets[ticket] == IssuedProjectedTicket(
                  cameraRevision: identity.cameraRevision,
                  execution: identity.execution,
                  orderBackend: identity.orderBackend
              ) else {
            projectedLedgerErrors.append(
                "projected terminal does not match an issued ticket/revision/execution/order lane"
            )
            return false
        }
        if let previous = projectedTerminalIdentities[ticket] {
            projectedLedgerErrors.append(
                previous == identity
                    ? "projected ticket produced more than one terminal"
                    : "projected terminal changed ticket/revision/generation identity"
            )
            return false
        }
        projectedTerminalIdentities[ticket] = identity
        return true
    }

    var orderLedgerWaitFinished: Bool {
        if !unsampledOrderRequests.isEmpty { return true }
        return issuedOrderTickets.keys.allSatisfy {
            ((completedGpuTickets[$0] != nil || completedCpuTickets[$0] != nil) &&
                completedCountsByTicket[$0] != nil) ||
                failedOrderTickets[$0] != nil
        }
    }

    var projectedLedgerWaitFinished: Bool {
        if !unsampledProjectedRequests.isEmpty { return true }
        return issuedProjectedTickets.keys.allSatisfy {
            completedProjectedTickets[$0] != nil || failedProjectedTickets[$0] != nil
        }
    }

    var currentStatsLedgerWaitFinished: Bool {
        issuedCurrentStatsSamples.keys.allSatisfy {
            completedCurrentStatsTickets[$0] != nil || failedCurrentStatsTickets[$0] != nil
        }
    }

    var currentStatsProtocolError: String? { currentStatsLedgerErrors.first }

    var terminalLedgersWaitFinished: Bool {
        orderLedgerWaitFinished && projectedLedgerWaitFinished && currentStatsLedgerWaitFinished
    }

    var currentStatsLedgerError: String? {
        if let first = currentStatsLedgerErrors.first { return first }
        if issuedCurrentStatsSamples.count != samples.count {
            return "every measured sample must own one unique current-stats submission"
        }
        var presentationSequences: Set<UInt64> = []
        for (sampleIndex, sample) in samples.enumerated() {
            guard let issued = issuedCurrentStatsSamples[sample.currentStatsTicket],
                  issued.sampleIndex == sampleIndex,
                  let receipt = completedCurrentStatsTickets[sample.currentStatsTicket] else {
                return "measured sample lacks a matching current-stats Ready terminal"
            }
            if failedCurrentStatsTickets[sample.currentStatsTicket] != nil {
                return "measured current-stats ticket has a terminal failure"
            }
            if issued.submission.identity != receipt.identity {
                return "measured current-stats ticket changed its full identity"
            }
            if receipt.identity.cameraRevision != sample.cameraRevision {
                return "current-stats identity does not match the sample camera revision"
            }
            if !presentationSequences.insert(receipt.identity.presentationSequence).inserted {
                return "measured current-stats samples reused a presentation sequence"
            }
            let expectedPlan: GsplatCurrentStatsPlan
            if sample.actualOrderBackend == 0 {
                expectedPlan = .cpuPostSort
            } else if sample.projectedExecution == 2 {
                expectedPlan = .gpuPreproject
            } else {
                expectedPlan = .gpuPostSort
            }
            if receipt.identity.executedPlan != expectedPlan {
                return "current-stats plan does not match the executed order/projected tuple"
            }
        }
        if let failure = failedCurrentStatsTickets.values.first {
            return "current-stats ticket \(failure.ticket) failed reason=\(failureReasonName(failure.reason))"
        }
        return nil
    }

    private func currentStatsTraceKey(
        sampleIndex: Int,
        traceStep: CameraTraceStep?
    ) -> String {
        if let traceStep {
            return "trace:\(traceStep.traceFrameIndex):\(traceStep.timestampNs):" +
                "\(traceStep.loopIndex)"
        }
        if let metadata = config.cameraTraceMetadata {
            return "fixed:\(metadata.sha256):\(config.cameraTraceFrame)"
        }
        return "orbit:\(sampleIndex)"
    }

    var orderLedgerError: String? {
        if let first = orderLedgerErrors.first { return first }
        if !unsampledOrderRequests.isEmpty {
            return "order measurement was unsampled: \(unsampledOrderRequests)"
        }
        for (ticket, issued) in issuedOrderTickets {
            let terminalCount = (completedGpuTickets[ticket] == nil ? 0 : 1) +
                (completedCpuTickets[ticket] == nil ? 0 : 1) +
                (failedOrderTickets[ticket] == nil ? 0 : 1)
            if terminalCount != 1 {
                return "issued order ticket \(ticket) revision \(issued.cameraRevision) " +
                    "lacks exactly one terminal"
            }
            if (completedGpuTickets[ticket] != nil || completedCpuTickets[ticket] != nil) &&
                completedCountsByTicket[ticket] == nil {
                return "successful order ticket \(ticket) lacks its V/C/D receipt"
            }
        }
        if let failure = failedOrderTickets.values.first {
            return "order ticket \(failure.ticket) failed reason=\(failure.reason)"
        }
        if samples.contains(where: { $0.gpuSortFallback }) {
            return "measured frame used GPU-to-CPU fallback"
        }
        for sample in samples where sample.submittedMeasurementTicket != nil {
            guard let counts = orderCounts(for: sample) else {
                return "issued order sample lacks its same-ticket V/C/D receipt"
            }
            if sample.actualOrderBackend == 1 {
                guard let measurement = orderMeasurement(for: sample),
                      counts.ticket == measurement.ticket else {
                    return "GPU order sample lacks its same-ticket terminal"
                }
            } else {
                guard let measurement = cpuOrderMeasurement(for: sample),
                      counts.ticket == measurement.ticket else {
                    return "CPU order sample lacks its same-ticket terminal"
                }
            }
            if let error = countContractError(
                visible: counts.visible_count,
                contributor: counts.contributor_count,
                drawn: counts.drawn_count,
                exactContributorCompaction:
                    counts.flags & orderCountsExactContributorDraw != 0
            ) {
                return "order sample contributor count contract: \(error)"
            }
        }
        return nil
    }

    var projectedLedgerError: String? {
        if let first = projectedLedgerErrors.first { return first }
        if !unsampledProjectedRequests.isEmpty {
            return "projected measurement was unsampled: \(unsampledProjectedRequests)"
        }
        for (ticket, issued) in issuedProjectedTickets {
            let terminalCount = (completedProjectedTickets[ticket] == nil ? 0 : 1) +
                (failedProjectedTickets[ticket] == nil ? 0 : 1)
            if terminalCount != 1 {
                return "issued projected ticket \(ticket) revision \(issued.cameraRevision) " +
                    "lacks exactly one terminal"
            }
        }
        if let failure = failedProjectedTickets.values.first {
            return "projected ticket \(failure.ticket) failed reason=\(failure.reason)"
        }
        for sample in samples {
            guard sample.projectedPolicy == requestedProjectedPolicyValue(),
                  (1...2).contains(sample.projectedExecution),
                  sample.projectedAdaptiveState <= 7 else {
                return "frame contains invalid requested/actual projected policy evidence"
            }
            if sample.projectedPolicy != 3 {
                if sample.projectedExecution != sample.projectedPolicy ||
                    sample.projectedAdaptiveState != 0 ||
                    sample.projectedSubmittedMeasurementTicket != nil ||
                    sample.projectedSubmissionExecution != nil ||
                    sample.projectedUnsampledReason != nil {
                    return "forced projected frame fabricated Adaptive telemetry"
                }
            } else if let ticket = sample.projectedSubmittedMeasurementTicket {
                guard let issued = issuedProjectedTickets[ticket],
                      issued.cameraRevision == sample.cameraRevision,
                      issued.execution == sample.projectedExecution,
                      issued.orderBackend == sample.actualOrderBackend,
                      sample.projectedSubmissionExecution == sample.projectedExecution,
                      sample.projectedUnsampledReason == nil else {
                    return "Adaptive projected frame submission identity is inconsistent"
                }
            } else if sample.projectedUnsampledReason != nil {
                return "Adaptive projected frame contains rejected unsampled evidence"
            } else if sample.projectedSubmissionExecution != nil {
                return "unmeasured projected frame exposed a measurement execution"
            }

            guard let receipt = currentStatsReceipt(for: sample) else {
                return "projected frame lacks its own current-stats Ready receipt"
            }
            if sample.projectedExecution == 1 {
                if receipt.drawnCount != receipt.visibleCount {
                    return "Candidate projected frame requires D=V without exact compaction"
                }
            } else if receipt.drawnCount != receipt.contributorCount {
                return "Compact projected frame requires D=C with exact compaction"
            }
        }
        return nil
    }

    private func orderMeasurement(
        for sample: BenchmarkSample
    ) -> GsplatSurfaceOrderMeasurement? {
        sample.submittedMeasurementTicket.flatMap { completedGpuTickets[$0] }
    }

    private func cpuOrderMeasurement(
        for sample: BenchmarkSample
    ) -> GsplatSurfaceCpuOrderMeasurement? {
        sample.submittedMeasurementTicket.flatMap { completedCpuTickets[$0] }
    }

    private func orderCounts(for sample: BenchmarkSample) -> GsplatSurfaceOrderCounts? {
        sample.submittedMeasurementTicket.flatMap { completedCountsByTicket[$0] }
    }

    private func currentStatsReceipt(for sample: BenchmarkSample) -> GsplatCurrentStatsReceipt? {
        completedCurrentStatsTickets[sample.currentStatsTicket]
    }

    private func countContractError(
        visible: UInt32,
        contributor: UInt32?,
        drawn: UInt32,
        exactContributorCompaction: Bool
    ) -> String? {
        if let contributor, contributor > visible {
            return "C=\(contributor) exceeds V=\(visible)"
        }
        if exactContributorCompaction {
            guard let contributor else { return "exact contributor draw omitted C" }
            if drawn != contributor {
                return "exact contributor draw requires D=C, got D=\(drawn) C=\(contributor)"
            }
        } else if drawn != visible {
            return "legacy/downlevel draw requires D=V, got D=\(drawn) V=\(visible)"
        }
        return nil
    }

    func record(
        sortStats: GsplatSurfaceSortStats,
        submission: GsplatSurfaceOrderSubmission,
        projectedSubmission: GsplatSurfaceProjectedSubmissionV1,
        currentStatsSubmission: GsplatCurrentStatsIssuedSubmission,
        renderCallNs: UInt64,
        frameStartNs: UInt64,
        traceStep: CameraTraceStep?
    ) {
        guard config.enabled, !complete else { return }
        if submission.camera_revision != sortStats.camera_revision {
            orderLedgerErrors.append("submission revision does not match sort stats")
        }
        if submission.actual_backend != ((sortStats.flags >> 9) & 0b11) {
            orderLedgerErrors.append("submission backend does not match sort stats")
        }
        observedFrames += 1
        if observedFrames <= config.warmupFrames {
            previousFrameStartNs = frameStartNs
            return
        }

        if measurementStartNs == nil {
            measurementStartNs = frameStartNs
            measurementStartedAt = Date()
        }
        guard let issuedCurrent = issuedCurrentStatsSamples[currentStatsSubmission.ticket],
              issuedCurrent.sampleIndex == samples.count,
              issuedCurrent.submission == currentStatsSubmission else {
            currentStatsLedgerErrors.append(
                "measured frame did not retain its unique current-stats ticket and identity"
            )
            return
        }
        let wallNs = previousFrameStartNs.map { frameStartNs - $0 } ?? renderCallNs
        samples.append(BenchmarkSample(
            elapsedNs: frameStartNs - (measurementStartNs ?? frameStartNs),
            callMs: Double(renderCallNs) / 1_000_000.0,
            frameWallMs: Double(wallNs) / 1_000_000.0,
            cameraRevision: sortStats.camera_revision,
            appliedOrderRevision: sortStats.applied_order_revision,
            sortRefreshed: sortStats.flags & 1 != 0,
            actualOrderBackend: (sortStats.flags >> 9) & 0b11,
            adaptiveState: (sortStats.flags >> 12) & 0b111,
            gpuSortFallback: sortStats.flags & (1 << 11) != 0,
            adaptiveGpuFailure: sortStats.flags & (1 << 15) != 0
                ? (sortStats.flags >> 16) & 0b111
                : nil,
            submittedMeasurementTicket: submission.flags & (1 << 1) != 0
                ? submission.ticket
                : nil,
            submissionFlags: submission.flags,
            projectedPolicy: projectedSubmission.requested_policy,
            projectedExecution: projectedSubmission.actual_execution,
            projectedAdaptiveState: projectedSubmission.adaptive_state,
            projectedSubmittedMeasurementTicket:
                projectedSubmission.flags & projectedSubmissionTicketIssued != 0
                    ? projectedSubmission.ticket
                    : nil,
            projectedSubmissionExecution:
                projectedSubmission.flags & projectedSubmissionTicketIssued != 0 ||
                    projectedSubmission.flags & (projectedSubmissionRingBusy |
                        projectedSubmissionSurfaceUnavailable) != 0
                    ? projectedSubmission.actual_execution
                    : nil,
            projectedUnsampledReason:
                projectedSubmission.flags & projectedSubmissionRingBusy != 0
                    ? "ring_busy"
                    : (projectedSubmission.flags & projectedSubmissionSurfaceUnavailable != 0
                        ? "surface_unavailable"
                        : nil),
            projectedSubmissionFlags: projectedSubmission.flags,
            currentStatsTicket: currentStatsSubmission.ticket,
            traceFrameIndex: traceStep?.traceFrameIndex,
            traceTimestampNs: traceStep?.timestampNs,
            traceLoopIndex: traceStep?.loopIndex
        ))
        if let traceStep {
            precondition(traceStep.phase == "measure")
            precondition(traceStep.measuredSampleIndex == samples.count - 1)
        }
        previousFrameStartNs = frameStartNs
        complete = samples.count >= config.measuredSampleCount
        if complete {
            measurementEndedAt = Date()
            finalThermalState = ProcessInfo.processInfo.thermalState
        }
    }

    private func requestedBackendValue() -> UInt32 {
        switch config.orderBackend {
        case "gpu": return 1
        case "adaptive": return 2
        default: return 0
        }
    }

    private func requestedProjectedPolicyValue() -> UInt32 {
        switch config.projectedPolicy {
        case "candidate": return 1
        case "compact": return 2
        default: return 3
        }
    }

    func emitArtifacts(
        datasetPath: String,
        datasetLabel: String,
        width: Int,
        height: Int,
        exactness: GsplatSurfaceExactness,
        presentation: GsplatSurfacePresentation
    ) -> Bool {
        let runEndedAt = Date()
        let runID = "ios-\(UUID().uuidString.lowercased())"
        let seriesID: String
        if config.cameraTraceSequence {
            seriesID = "ios-native-camera-trace-sequence"
        } else if config.cameraTracePath == nil {
            seriesID = "ios-native-surface"
        } else {
            seriesID = "ios-native-fixed-camera"
        }
        guard let dataset = inspectDataset(path: datasetPath) else {
            print("BENCHMARK_ARTIFACT_ERROR dataset metadata unavailable path=\(datasetPath)")
            return false
        }
        let fullQualityFlags: UInt32 = 0b1_1111
        guard exactness.quality_flags & fullQualityFlags == fullQualityFlags,
              exactness.source_splat_count == UInt64(dataset.splatCount),
              exactness.decoded_splat_count == UInt64(dataset.splatCount),
              exactness.encoded_splat_count == UInt64(dataset.splatCount),
              exactness.resident_splat_count == UInt64(dataset.splatCount),
              exactness.addressable_splat_count == UInt64(dataset.splatCount),
              exactness.source_sh_degree == UInt32(dataset.shDegree),
              exactness.resident_sh_degree == UInt32(dataset.shDegree) else {
            print(
                "BENCHMARK_ARTIFACT_ERROR native exactness receipt does not match " +
                "dataset count=\(dataset.splatCount) sh=\(dataset.shDegree)"
            )
            return false
        }
        let requiredPresentationFlags: UInt32 = 0b1_1111
        guard width > height,
              width > 0,
              height > 0,
              presentation.requested_width == UInt32(width),
              presentation.requested_height == UInt32(height),
              presentation.surface_width == presentation.requested_width,
              presentation.surface_height == presentation.requested_height,
              presentation.internal_render_width == presentation.surface_width,
              presentation.internal_render_height == presentation.surface_height,
              presentation.presented_width == presentation.internal_render_width,
              presentation.presented_height == presentation.internal_render_height,
              presentation.flags & requiredPresentationFlags == requiredPresentationFlags,
              presentation.reserved == 0 else {
            print(
                "BENCHMARK_ARTIFACT_ERROR native presentation is not settled full-resolution " +
                "requested=\(presentation.requested_width)x\(presentation.requested_height) " +
                "surface=\(presentation.surface_width)x\(presentation.surface_height) " +
                "internal=\(presentation.internal_render_width)x" +
                "\(presentation.internal_render_height) presented=" +
                "\(presentation.presented_width)x\(presentation.presented_height) " +
                "flags=\(presentation.flags)"
            )
            return false
        }
        if let ledgerError = currentStatsLedgerError {
            print("BENCHMARK_ARTIFACT_ERROR current-stats terminal ledger: \(ledgerError)")
            return false
        }
        if let ledgerError = orderLedgerError {
            print("BENCHMARK_ARTIFACT_ERROR order terminal ledger: \(ledgerError)")
            return false
        }
        if let ledgerError = projectedLedgerError {
            print("BENCHMARK_ARTIFACT_ERROR projected terminal ledger: \(ledgerError)")
            return false
        }
        for sample in samples {
            guard let receipt = currentStatsReceipt(for: sample),
                  UInt64(receipt.sourceCount) == exactness.source_splat_count,
                  receipt.contributorCount <= receipt.visibleCount,
                  receipt.visibleCount <= receipt.sourceCount,
                  sample.projectedExecution == 1
                    ? receipt.drawnCount == receipt.visibleCount
                    : receipt.drawnCount == receipt.contributorCount else {
                print("BENCHMARK_ARTIFACT_ERROR frame lacks valid ticket-matched current S/V/C/D")
                return false
            }
        }
        let exactnessReceiptID = exactnessReceiptID(exactness)
        let frameBudgetMs = 1000.0 / 60.0
        let hasMeasuredGpuCompletion = samples.contains { sample in
            sample.submittedMeasurementTicket.flatMap { completedGpuTickets[$0] } != nil
        }
        let hasMeasuredCpuCompletion = samples.contains { sample in
            sample.submittedMeasurementTicket.flatMap { completedCpuTickets[$0] } != nil
        }
        let hasMeasuredProjectedCompletion = samples.contains { sample in
            sample.projectedSubmittedMeasurementTicket.flatMap {
                completedProjectedTickets[$0]
            } != nil
        }
        let repositoryCommit = Bundle.main.object(
            forInfoDictionaryKey: "GsplatRepositoryCommit"
        ) as? String
        let repositoryDirty = Bundle.main.object(
            forInfoDictionaryKey: "GsplatRepositoryDirty"
        ) as? Bool
        let buildProfile = Bundle.main.object(
            forInfoDictionaryKey: "GsplatBuildProfile"
        ) as? String ?? "unknown"
        let repositoryCommitValue: Any = repositoryCommit.map { $0 as Any } ?? NSNull()
        let repositoryDirtyValue: Any = repositoryDirty.map { $0 as Any } ?? NSNull()
        var unavailable = [
            "environment.browser", "environment.driver", "frames[*].gpu_wait_ms",
            "frames[*].geometry_submit_ms",
            "summary.distributions.geometry_submit_ms",
        ]
        if repositoryCommit == nil { unavailable.append("build.repository_commit") }
        if repositoryDirty == nil { unavailable.append("build.dirty") }
        if !hasMeasuredGpuCompletion {
            unavailable.append("frames[*].gpu_complete_ms")
            unavailable.append("summary.distributions.gpu_complete_ms")
        }
        if !hasMeasuredCpuCompletion {
            unavailable.append("frames[*].cpu_frame_complete_ms")
            unavailable.append("summary.distributions.cpu_frame_complete_ms")
        }
        if !hasMeasuredProjectedCompletion {
            unavailable.append("summary.distributions.projected_frame_complete_ms")
        }
        let hasUnavailableCpuOrderPhases = samples.contains {
            $0.submittedMeasurementTicket.flatMap { completedCpuTickets[$0] } == nil
        }
        if hasUnavailableCpuOrderPhases {
            unavailable.append("frames[*].preprocess_ms")
            unavailable.append("frames[*].sort_ms")
        }
        if !hasMeasuredCpuCompletion {
            unavailable.append("summary.distributions.preprocess_ms")
            unavailable.append("summary.distributions.sort_ms")
        }
        let manifest: [String: Any] = [
            "schema": benchmarkSchema, "record_type": "manifest", "run_id": runID,
            "identity": [
                "series_id": seriesID,
                "started_at_utc": utc(runStartedAt), "ended_at_utc": utc(runEndedAt),
                "measurement_started_at_utc": utc(measurementStartedAt ?? runStartedAt),
                "measurement_ended_at_utc": utc(measurementEndedAt ?? runEndedAt),
            ],
            "build": [
                "repository_commit": repositoryCommitValue,
                "dirty": repositoryDirtyValue, "profile": buildProfile,
                "package_version": Bundle.main.object(
                    forInfoDictionaryKey: "CFBundleShortVersionString"
                ) as? String ?? "\(gsplat_version_major()).\(gsplat_version_minor())",
            ],
            "dataset": [
                "id": datasetLabel, "sha256": dataset.sha256, "bytes": dataset.bytes,
                "splat_count": dataset.splatCount, "sh_degree": dataset.shDegree,
            ],
            "exactness": [
                "receipt_id": exactnessReceiptID,
                "source_splat_count": exactness.source_splat_count,
                "decoded_splat_count": exactness.decoded_splat_count,
                "encoded_splat_count": exactness.encoded_splat_count,
                "resident_splat_count": exactness.resident_splat_count,
                "addressable_splat_count": exactness.addressable_splat_count,
                "source_sh_degree": exactness.source_sh_degree,
                "resident_sh_degree": exactness.resident_sh_degree,
                "source_membership": "all", "sampling": "disabled", "lod": "disabled",
                "sh_degree_policy": "source", "partial_scene_published": false,
                "full_quality": true,
            ],
            "trace": traceIdentity(),
            "renderer": [
                "implementation": "gsplat-rs", "path": geometryPipelineName(config.geometryPath), "backend": "metal",
                "order_backend_requested": config.orderBackend,
                "current_stats_evidence_version": 1,
                "projected_evidence_version": 1,
                "projected_policy_requested": config.projectedPolicy,
                "sort_interval": config.sortInterval,
                "sort_policy": "\(config.orderBackend)_interval_\(config.sortInterval)",
                "count_semantics": countSemantics,
                "max_storage_buffers_per_shader_stage":
                    exactness.max_storage_buffers_per_shader_stage,
                "max_storage_buffer_binding_size": exactness.max_storage_buffer_binding_size,
            ],
            "resolution": [
                "requested_width": presentation.requested_width,
                "requested_height": presentation.requested_height,
                "surface_width": presentation.surface_width,
                "surface_height": presentation.surface_height,
                "internal_render_width": presentation.internal_render_width,
                "internal_render_height": presentation.internal_render_height,
                "presented_width": presentation.presented_width,
                "presented_height": presentation.presented_height,
                "presented_camera_revision": presentation.presented_camera_revision,
                "dynamic_resolution": "disabled",
                "upscaling": "disabled",
                "full_resolution": true,
                "native_flags": presentation.flags,
            ],
            "display": [
                "width": presentation.presented_width,
                "height": presentation.presented_height,
                "dpr": Double(UIScreen.main.scale),
                "refresh_hz": 60.0, "frame_budget_ms": frameBudgetMs,
                "refresh_hz_source": "configured", "frame_budget_source": "configured",
            ],
            "environment": [
                "platform": "ios", "os": UIDevice.current.systemVersion,
                "device": machineIdentifier(), "browser": NSNull(), "adapter": "Apple GPU",
                "driver": NSNull(), "thermal_state_start": thermalName(initialThermalState),
                "thermal_state_end": thermalName(finalThermalState ?? initialThermalState),
                "orientation": "landscape",
                "physical_display_width": Int(UIScreen.main.nativeBounds.width.rounded()),
                "physical_display_height": Int(UIScreen.main.nativeBounds.height.rounded()),
            ],
            "unavailable_fields": unavailable,
        ]

        emit(kind: "manifest", object: manifest)
        for (index, sample) in samples.enumerated() {
            let orderMeasurementReceipt = sample.actualOrderBackend == 1
                ? orderMeasurement(for: sample)
                : nil
            let cpuOrderMeasurementReceipt = sample.actualOrderBackend == 0
                ? cpuOrderMeasurement(for: sample)
                : nil
            let submittedGpuMeasurement = sample.submittedMeasurementTicket.flatMap {
                completedGpuTickets[$0]
            }
            let submittedCpuMeasurement = sample.submittedMeasurementTicket.flatMap {
                completedCpuTickets[$0]
            }
            guard let issuedCurrent = issuedCurrentStatsSamples[sample.currentStatsTicket],
                  let currentReceipt = currentStatsReceipt(for: sample) else {
                print("BENCHMARK_ARTIFACT_ERROR frame \(index) lost current-stats identity")
                return false
            }
            let exactContributorCompaction =
                currentReceipt.countSemantics.exactContributorCompaction
            let timingSource: Any
            if let submittedGpuMeasurement {
                timingSource = submittedGpuMeasurement.timing_source == 1
                    ? "timestamp_query"
                    : "completion"
            } else {
                timingSource = NSNull()
            }
            let adaptiveFailure: Any = sample.adaptiveGpuFailure.map {
                adaptiveGpuFailureName($0) as Any
            } ?? NSNull()
            let gpuComplete: Any = submittedGpuMeasurement.map {
                Double($0.gpu_complete_ms) as Any
            } ?? NSNull()
            let cpuFrameComplete: Any = submittedCpuMeasurement.map {
                Double($0.frame_complete_ms) as Any
            } ?? NSNull()
            let terminalTicket: Any = orderMeasurementReceipt.map { $0.ticket as Any }
                ?? cpuOrderMeasurementReceipt.map { $0.ticket as Any }
                ?? NSNull()
            let terminalRevision: Any = orderMeasurementReceipt.map { $0.camera_revision as Any }
                ?? cpuOrderMeasurementReceipt.map { $0.camera_revision as Any }
                ?? NSNull()
            let currentIdentity = currentStatsIdentityObject(currentReceipt.identity)
            emit(kind: "frame", object: [
                "schema": benchmarkSchema, "record_type": "frame", "run_id": runID,
                "frame_index": index, "elapsed_ns": sample.elapsedNs, "call_ms": sample.callMs,
                "frame_wall_ms": sample.frameWallMs,
                "preprocess_ms": submittedCpuMeasurement.map {
                    Double($0.preprocess_ms) as Any
                } ?? NSNull(),
                "sort_ms": submittedCpuMeasurement.map {
                    Double($0.sort_ms) as Any
                } ?? NSNull(),
                "geometry_submit_ms": NSNull(),
                "gpu_wait_ms": NSNull(),
                "gpu_complete_ms": gpuComplete,
                "cpu_frame_complete_ms": cpuFrameComplete,
                "source": currentReceipt.sourceCount,
                "visible": currentReceipt.visibleCount,
                "contributor": currentReceipt.contributorCount,
                "drawn": currentReceipt.drawnCount,
                "exact_contributor_compaction": exactContributorCompaction,
                "sort_refreshed": sample.sortRefreshed,
                "camera_revision": sample.cameraRevision,
                "applied_order_revision": sample.appliedOrderRevision,
                "order_backend": sample.actualOrderBackend == 1 ? "gpu" : "cpu",
                "adaptive_state": adaptiveStateName(sample.adaptiveState),
                "adaptive_gpu_failure": adaptiveFailure,
                "gpu_sort_fallback": sample.gpuSortFallback,
                "order_submission_ticket": sample.submittedMeasurementTicket.map { $0 as Any } ?? NSNull(),
                "order_submission_flags": sample.submissionFlags,
                "order_measurement_ticket": terminalTicket,
                "order_measurement_camera_revision": terminalRevision,
                "order_timing_source": timingSource,
                "gpu_preprocess_ms": optionalGpuValue(submittedGpuMeasurement, field: 0),
                "gpu_radix_ms": optionalGpuValue(submittedGpuMeasurement, field: 1),
                "gpu_order_ms": optionalGpuValue(submittedGpuMeasurement, field: 2),
                "gpu_timestamp_period_ns": optionalGpuValue(submittedGpuMeasurement, field: 3),
                "order_measurement_flags": orderMeasurementReceipt.map {
                    $0.flags as Any
                } ?? NSNull(),
                "cpu_order_measurement_flags": cpuOrderMeasurementReceipt.map {
                    $0.flags as Any
                } ?? NSNull(),
                "projected_policy": projectedPolicyName(sample.projectedPolicy),
                "projected_execution": projectedExecutionName(sample.projectedExecution),
                "projected_adaptive_state":
                    projectedAdaptiveStateName(sample.projectedAdaptiveState),
                "projected_measurement_submission":
                    sample.projectedSubmittedMeasurementTicket != nil
                        ? "issued"
                        : (sample.projectedUnsampledReason != nil ? "unsampled" : "not_requested"),
                "projected_measurement_ticket":
                    sample.projectedSubmittedMeasurementTicket.map { $0 as Any } ?? NSNull(),
                "projected_measurement_execution": sample.projectedSubmissionExecution.map {
                    projectedExecutionName($0) as Any
                } ?? NSNull(),
                "projected_measurement_unsampled_reason":
                    sample.projectedUnsampledReason.map { $0 as Any } ?? NSNull(),
                "projected_submission_flags": sample.projectedSubmissionFlags,
                "current_stats_ticket": sample.currentStatsTicket,
                "current_stats_sample_key": issuedCurrent.sampleKey,
                "current_stats_trace_key": issuedCurrent.traceKey,
                "current_stats_identity": currentIdentity,
                "current_stats_count_semantics":
                    currentStatsCountSemanticsName(currentReceipt.countSemantics),
                "trace_frame_index": sample.traceFrameIndex.map { $0 as Any } ?? NSNull(),
                "trace_timestamp_ns": sample.traceTimestampNs.map { $0 as Any } ?? NSNull(),
                "trace_loop_index": sample.traceLoopIndex.map { $0 as Any } ?? NSNull(),
                "order_backend_requested": config.orderBackend,
            ])
        }
        emit(
            kind: "summary",
            object: summary(
                runID: runID,
                frameBudgetMs: frameBudgetMs,
                exactnessReceiptID: exactnessReceiptID
            )
        )
        return true
    }

    func resultLine(datasetLabel: String) -> String {
        let count = max(samples.count, 1)
        func mean(_ value: (BenchmarkSample) -> Double) -> Double {
            samples.reduce(0.0) { $0 + value($1) } / Double(count)
        }
        let visible = samples.reduce(UInt64(0)) { total, sample in
            total + UInt64(currentStatsReceipt(for: sample)?.visibleCount ?? 0)
        } / UInt64(count)
        let contributor = samples.reduce(UInt64(0)) { total, sample in
            total + UInt64(currentStatsReceipt(for: sample)?.contributorCount ?? 0)
        } / UInt64(count)
        let drawn = samples.reduce(UInt64(0)) { total, sample in
            total + UInt64(currentStatsReceipt(for: sample)?.drawnCount ?? 0)
        } / UInt64(count)
        let cpuCompletion = samples.compactMap { sample in
            sample.submittedMeasurementTicket.flatMap { completedCpuTickets[$0] }
        }
        let gpuCompletion = samples.compactMap { sample in
            sample.submittedMeasurementTicket.flatMap { completedGpuTickets[$0] }
        }
        let projectedCompletion = samples.compactMap { sample in
            sample.projectedSubmittedMeasurementTicket.flatMap {
                completedProjectedTickets[$0]
            }
        }
        let meanCpuCompletion = cpuCompletion.isEmpty ? "n/a" : format(
            cpuCompletion.reduce(0.0) { $0 + Double($1.frame_complete_ms) } /
                Double(cpuCompletion.count)
        )
        let meanGpuCompletion = gpuCompletion.isEmpty ? "n/a" : format(
            gpuCompletion.reduce(0.0) { $0 + Double($1.gpu_complete_ms) } /
                Double(gpuCompletion.count)
        )
        let meanProjectedCompletion = projectedCompletion.isEmpty ? "n/a" : format(
            projectedCompletion.reduce(0.0) {
                $0 + Double($1.measurement.frame_complete_ms)
            } / Double(projectedCompletion.count)
        )
        let meanCpuPreprocess = cpuCompletion.isEmpty ? "n/a" : format(
            cpuCompletion.reduce(0.0) { $0 + Double($1.preprocess_ms) } /
                Double(cpuCompletion.count)
        )
        let meanCpuSort = cpuCompletion.isEmpty ? "n/a" : format(
            cpuCompletion.reduce(0.0) { $0 + Double($1.sort_ms) } /
                Double(cpuCompletion.count)
        )
        return [
            "BENCHMARK_RESULT", "dataset=\(datasetLabel)", "samples=\(samples.count)",
            "warmup=\(config.warmupFrames)", "sort_interval=\(config.sortInterval)",
            "loops=\(config.cameraTraceLoops)", "requested_backend=\(config.orderBackend)",
            "projected_policy=\(config.projectedPolicy)",
            "async_sort=\(config.asyncSort)", "geometry_pipeline=\(geometryPipelineName(config.geometryPath))",
            "frame_latency=\(config.frameLatency)", "avg_call_ms=\(format(mean { $0.callMs }))",
            "avg_frame_ms=n/a",
            "avg_preprocess_ms=\(meanCpuPreprocess)",
            "avg_sort_ms=\(meanCpuSort)",
            "avg_raster_ms=n/a",
            "avg_cpu_queue_complete_ms=\(meanCpuCompletion)",
            "avg_gpu_queue_complete_ms=\(meanGpuCompletion)",
            "avg_projected_queue_complete_ms=\(meanProjectedCompletion)",
            "avg_visible=\(visible)", "avg_contributor=\(contributor)", "avg_drawn=\(drawn)",
        ].joined(separator: " ")
    }

    private func summary(
        runID: String,
        frameBudgetMs: Double,
        exactnessReceiptID: String
    ) -> [String: Any] {
        let wall = samples.map(\.frameWallMs)
        let gpuMeasurements = samples.compactMap { sample in
            sample.submittedMeasurementTicket.flatMap { completedGpuTickets[$0] }
        }
        let cpuMeasurements = samples.compactMap { sample in
            sample.submittedMeasurementTicket.flatMap { completedCpuTickets[$0] }
        }
        let projectedMeasurements = samples.compactMap { sample in
            sample.projectedSubmittedMeasurementTicket.flatMap {
                completedProjectedTickets[$0]
            }
        }
        let gpuCompleteDistribution: Any = gpuMeasurements.isEmpty
            ? NSNull()
            : distribution(gpuMeasurements.map { Double($0.gpu_complete_ms) })
        let cpuCompleteDistribution: Any = cpuMeasurements.isEmpty
            ? NSNull()
            : distribution(cpuMeasurements.map { Double($0.frame_complete_ms) })
        let projectedCompleteDistribution: Any = projectedMeasurements.isEmpty
            ? NSNull()
            : distribution(projectedMeasurements.map {
                Double($0.measurement.frame_complete_ms)
            })
        let cpuPreprocessDistribution: Any = cpuMeasurements.isEmpty
            ? NSNull()
            : distribution(cpuMeasurements.map { Double($0.preprocess_ms) })
        let cpuSortDistribution: Any = cpuMeasurements.isEmpty
            ? NSNull()
            : distribution(cpuMeasurements.map { Double($0.sort_ms) })
        let adaptiveGpuFailureFinal: Any = samples.last
            .flatMap { $0.adaptiveGpuFailure }
            .map { adaptiveGpuFailureName($0) as Any } ?? NSNull()
        let distributions: [String: Any] = [
            "call_ms": distribution(samples.map(\.callMs)),
            "frame_wall_ms": distribution(wall),
            "preprocess_ms": cpuPreprocessDistribution,
            "sort_ms": cpuSortDistribution,
            "geometry_submit_ms": NSNull(),
            "gpu_wait_ms": NSNull(),
            "gpu_complete_ms": gpuCompleteDistribution,
            "cpu_frame_complete_ms": cpuCompleteDistribution,
            "projected_frame_complete_ms": projectedCompleteDistribution,
        ]
        let issuedCpuCount = issuedOrderTickets.values.filter { $0.backend == 0 }.count
        let issuedGpuCount = issuedOrderTickets.values.filter { $0.backend == 1 }.count
        let sortTelemetry: [String: Any] = [
            "cpu_frame_count": samples.filter { $0.actualOrderBackend == 0 }.count,
            "gpu_frame_count": samples.filter { $0.actualOrderBackend == 1 }.count,
            "gpu_sort_fallback_count": samples.filter { $0.gpuSortFallback }.count,
            "order_measurement_scheduled_count": issuedOrderTickets.count,
            "cpu_order_measurement_scheduled_count": issuedCpuCount,
            "cpu_order_measurement_completed_count": completedCpuTickets.count,
            "gpu_order_measurement_scheduled_count": issuedGpuCount,
            "gpu_order_measurement_completed_count": completedGpuTickets.count,
            "order_measurement_terminal_failure_count": failedOrderTickets.count,
            "order_measurement_unsampled_count": unsampledOrderRequests.count,
            "adaptive_final_state": samples.last.map {
                adaptiveStateName($0.adaptiveState)
            } ?? "disabled",
            "adaptive_gpu_failure_final": adaptiveGpuFailureFinal,
        ]
        let projectedTelemetry: [String: Any] = [
            "policy_requested": config.projectedPolicy,
            "candidate_frame_count": samples.filter { $0.projectedExecution == 1 }.count,
            "compact_frame_count": samples.filter { $0.projectedExecution == 2 }.count,
            "measurement_scheduled_count": issuedProjectedTickets.count,
            "measurement_completed_count": completedProjectedTickets.count,
            "measurement_terminal_failure_count": failedProjectedTickets.count,
            "measurement_unsampled_count": unsampledProjectedRequests.count,
            "adaptive_final_state": samples.last.map {
                projectedAdaptiveStateName($0.projectedAdaptiveState)
            } ?? "disabled",
            "execution_final": samples.last.map {
                projectedExecutionName($0.projectedExecution)
            } ?? "candidate",
        ]
        let result: [String: Any] = [
            "schema": benchmarkSchema, "record_type": "summary", "run_id": runID,
            "sample_count": samples.count, "warmup_count": config.warmupFrames,
            "frame_budget_ms": frameBudgetMs,
            "missed_frame_count": wall.filter { $0 > frameBudgetMs }.count,
            "distributions": distributions,
            "sort_telemetry": sortTelemetry,
            "current_stats_terminal_ledger": currentStatsTerminalLedger(),
            "order_terminal_ledger": terminalLedger(exactnessReceiptID: exactnessReceiptID),
            "projected_draw_telemetry": projectedTelemetry,
            "projected_terminal_ledger": projectedTerminalLedger(
                exactnessReceiptID: exactnessReceiptID
            ),
        ]
        return result
    }

    private func currentStatsTerminalLedger() -> [String: Any] {
        let issued = issuedCurrentStatsSamples.values.sorted {
            $0.sampleIndex < $1.sampleIndex
        }
        let submissions: [[String: Any]] = issued.map { sample in
            [
                "ticket": sample.submission.ticket,
                "sample_index": sample.sampleIndex,
                "sample_key": sample.sampleKey,
                "trace_key": sample.traceKey,
                "identity": currentStatsIdentityObject(sample.submission.identity),
            ]
        }
        let successes: [[String: Any]] = issued.compactMap { sample in
            guard let receipt = completedCurrentStatsTickets[sample.submission.ticket]
            else { return nil }
            return [
                "ticket": receipt.ticket,
                "sample_index": sample.sampleIndex,
                "sample_key": sample.sampleKey,
                "trace_key": sample.traceKey,
                "identity": currentStatsIdentityObject(receipt.identity),
                "outcome": "ready",
                "count_semantics": currentStatsCountSemanticsName(
                    receipt.countSemantics
                ),
                "source": receipt.sourceCount,
                "visible": receipt.visibleCount,
                "contributor": receipt.contributorCount,
                "drawn": receipt.drawnCount,
            ]
        }
        let failures: [[String: Any]] = issued.compactMap { sample in
            guard let failure = failedCurrentStatsTickets[sample.submission.ticket]
            else { return nil }
            return [
                "ticket": failure.ticket,
                "sample_index": sample.sampleIndex,
                "sample_key": sample.sampleKey,
                "trace_key": sample.traceKey,
                "identity": currentStatsIdentityObject(failure.identity),
                "outcome": "failure",
                "failure_reason": failureReasonName(failure.reason),
            ]
        }
        return [
            "submissions": submissions,
            "successes": successes,
            "failures": failures,
            "issued_count": submissions.count,
            "success_count": successes.count,
            "failure_count": failures.count,
        ]
    }

    private func currentStatsCountContractError(
        _ receipt: GsplatCurrentStatsReceipt
    ) -> String? {
        if receipt.contributorCount > receipt.visibleCount ||
            receipt.visibleCount > receipt.sourceCount {
            return "current-stats Ready violates 0 <= C <= V <= S"
        }
        switch (receipt.identity.executedPlan, receipt.countSemantics) {
        case (.cpuPostSort, .directDrawEqualsVisible),
             (.gpuPostSort, .indirectDrawEqualsVisible):
            if receipt.drawnCount != receipt.visibleCount {
                return "PostSort current-stats Ready requires D=V"
            }
        case (.gpuPreproject, .indirectDrawEqualsContributor):
            if receipt.drawnCount != receipt.contributorCount {
                return "GPU Preproject current-stats Ready requires D=C"
            }
        default:
            return "current-stats plan and count semantics disagree"
        }
        return nil
    }

    private func currentStatsIdentityObject(
        _ identity: GsplatCurrentStatsIdentity
    ) -> [String: Any] {
        [
            "scene_generation": identity.sceneGeneration,
            "camera_revision": identity.cameraRevision,
            "viewport_generation": identity.viewportGeneration,
            "contract_generation": identity.contractGeneration,
            "plan_set_generation": identity.planSetGeneration,
            "executed_plan": currentStatsPlanName(identity.executedPlan),
            "order_generation": identity.orderGeneration,
            "raster_generation": identity.rasterGeneration,
            "encode_attempt": identity.encodeAttempt,
            "presentation_sequence": identity.presentationSequence,
        ]
    }

    private func currentStatsPlanName(_ plan: GsplatCurrentStatsPlan) -> String {
        switch plan {
        case .cpuPostSort: return "cpu_post_sort"
        case .gpuPostSort: return "gpu_post_sort"
        case .gpuPreproject: return "gpu_preproject"
        }
    }

    private func currentStatsCountSemanticsName(
        _ semantics: GsplatCurrentStatsCountSemantics
    ) -> String {
        switch semantics {
        case .directDrawEqualsVisible: return "direct_draw_equals_visible"
        case .indirectDrawEqualsVisible: return "indirect_draw_equals_visible"
        case .indirectDrawEqualsContributor: return "indirect_draw_equals_contributor"
        }
    }

    private func failureReasonName(_ reason: GsplatCurrentStatsFailureReason) -> String {
        switch reason {
        case .mapFailure: return "map_failure"
        case .generationInvalidated: return "generation_invalidated"
        case .expired: return "expired"
        case .dropped: return "dropped"
        }
    }

    private func requestStatusName(_ status: GsplatCurrentStatsRequestStatus) -> String {
        switch status {
        case .requested: return "requested"
        case .busy: return "busy"
        case .gpuUnavailable: return "gpu_unavailable"
        case .resourceUnavailable: return "resource_unavailable"
        case .ticketExhausted: return "ticket_exhausted"
        }
    }

    private func terminalLedger(exactnessReceiptID: String) -> [[String: Any]] {
        issuedOrderTickets.keys.sorted().map { ticket in
            let issued = issuedOrderTickets[ticket]!
            var record: [String: Any] = [
                "ticket": ticket,
                "camera_revision": issued.cameraRevision,
                "backend": issued.backend == 1 ? "gpu" : "cpu",
                "exactness_receipt_id": exactnessReceiptID,
            ]
            if let measurement = completedGpuTickets[ticket] {
                record["outcome"] = "success"
                record["frame_complete_ms"] = Double(measurement.gpu_complete_ms)
                if let counts = completedCountsByTicket[ticket] {
                    record["visible"] = counts.visible_count
                    record["contributor"] = counts.contributor_count
                    record["drawn"] = counts.drawn_count
                    record["exact_contributor_compaction"] =
                        counts.flags & orderCountsExactContributorDraw != 0
                }
            } else if let measurement = completedCpuTickets[ticket] {
                record["outcome"] = "success"
                record["frame_complete_ms"] = Double(measurement.frame_complete_ms)
                if let counts = completedCountsByTicket[ticket] {
                    record["visible"] = counts.visible_count
                    record["contributor"] = counts.contributor_count
                    record["drawn"] = counts.drawn_count
                    record["exact_contributor_compaction"] =
                        counts.flags & orderCountsExactContributorDraw != 0
                }
            } else if let failure = failedOrderTickets[ticket] {
                record["outcome"] = "failure"
                record["failure_reason"] = failure.reason
            } else {
                record["outcome"] = "pending"
            }
            return record
        }
    }

    private func projectedTerminalLedger(exactnessReceiptID: String) -> [String: Any] {
        let submissions: [[String: Any]] = issuedProjectedTickets.keys.sorted().map { ticket in
            let issued = issuedProjectedTickets[ticket]!
            return [
                "ticket": ticket,
                "camera_revision": issued.cameraRevision,
                "execution": projectedExecutionName(issued.execution),
                "order_backend": issued.orderBackend == 1 ? "gpu" : "cpu",
            ]
        }
        let successes: [[String: Any]] = completedProjectedTickets.keys.sorted().map { ticket in
            let completed = completedProjectedTickets[ticket]!
            let measurement = completed.measurement
            let counts = completed.counts
            return [
                "ticket": ticket,
                "camera_revision": measurement.camera_revision,
                "projection_generation": measurement.projection_generation,
                "probe_generation": measurement.probe_generation,
                "execution": projectedExecutionName(measurement.execution),
                "order_backend": measurement.order_backend == 1 ? "gpu" : "cpu",
                "outcome": "success",
                "frame_complete_ms": Double(measurement.frame_complete_ms),
                "projection_rebuilt":
                    measurement.flags & projectedMeasurementProjectionRebuilt != 0,
                "order_refreshed":
                    measurement.flags & projectedMeasurementOrderRefreshed != 0,
                "visible": counts.visible_count,
                "contributor": counts.contributor_count,
                "drawn": counts.drawn_count,
                "exact_contributor_compaction":
                    counts.flags & projectedCountsExactContributorDraw != 0,
                "exactness_receipt_id": exactnessReceiptID,
                "measurement_flags": measurement.flags,
                "counts_flags": counts.flags,
            ]
        }
        let failures: [[String: Any]] = failedProjectedTickets.keys.sorted().map { ticket in
            let failure = failedProjectedTickets[ticket]!
            return [
                "ticket": ticket,
                "camera_revision": failure.camera_revision,
                "projection_generation": failure.projection_generation,
                "probe_generation": failure.probe_generation,
                "execution": projectedExecutionName(failure.execution),
                "order_backend": failure.order_backend == 1 ? "gpu" : "cpu",
                "outcome": "failure",
                "failure_reason": projectedFailureReasonName(failure.reason),
                "exactness_receipt_id": exactnessReceiptID,
                "failure_flags": failure.flags,
            ]
        }
        return [
            "submissions": submissions,
            "successes": successes,
            "failures": failures,
            "issued_count": submissions.count,
            "success_count": successes.count,
            "failure_count": failures.count,
        ]
    }

    private func exactnessReceiptID(_ exactness: GsplatSurfaceExactness) -> String {
        [
            "native-exactness-v1",
            String(exactness.source_splat_count),
            String(exactness.decoded_splat_count),
            String(exactness.encoded_splat_count),
            String(exactness.resident_splat_count),
            String(exactness.addressable_splat_count),
            String(exactness.source_sh_degree),
            String(exactness.resident_sh_degree),
            String(exactness.quality_flags),
        ].joined(separator: ":")
    }

    private func optionalGpuValue(
        _ measurement: GsplatSurfaceOrderMeasurement?,
        field: UInt32
    ) -> Any {
        guard let measurement, measurement.flags & (1 << field) != 0 else {
            return NSNull()
        }
        switch field {
        case 0: return Double(measurement.gpu_preprocess_ms)
        case 1: return Double(measurement.gpu_radix_ms)
        case 2: return Double(measurement.gpu_order_ms)
        default: return Double(measurement.timestamp_period_ns)
        }
    }

    private func adaptiveStateName(_ value: UInt32) -> String {
        switch value {
        case 1: return "cpu_learning"
        case 2: return "cpu_stable"
        case 3: return "gpu_probe"
        case 4: return "gpu_stable"
        case 5: return "cpu_probe"
        case 6: return "cooldown"
        default: return "disabled"
        }
    }

    private func adaptiveGpuFailureName(_ value: UInt32) -> String {
        switch value {
        case 1: return "unsupported"
        case 2: return "initialization"
        case 3: return "out_of_memory"
        case 4: return "validation"
        default: return "unknown"
        }
    }

    private func projectedPolicyName(_ value: UInt32) -> String {
        switch value {
        case 1: return "candidate"
        case 2: return "compact"
        default: return "adaptive"
        }
    }

    private func projectedExecutionName(_ value: UInt32) -> String {
        value == 2 ? "compact" : "candidate"
    }

    private func projectedAdaptiveStateName(_ value: UInt32) -> String {
        switch value {
        case 1: return "candidate_learning"
        case 2: return "candidate_stable"
        case 3: return "compact_probe"
        case 4: return "compact_stable"
        case 5: return "candidate_probe"
        case 6: return "cooldown"
        case 7: return "candidate_only"
        default: return "disabled"
        }
    }

    private func projectedFailureReasonName(_ value: UInt32) -> String {
        switch value {
        case 1: return "readback_map"
        case 2: return "generation_invalidated"
        case 3: return "invariant_violation"
        default: return "unknown"
        }
    }

    private func distribution(_ values: [Double]) -> [String: Any] {
        let sorted = values.sorted()
        func percentile(_ p: Double) -> Double {
            sorted[max(Int(ceil(p * Double(sorted.count))) - 1, 0)]
        }
        var total = 0.0
        for value in values { total += value }
        return [
            "count": values.count, "mean": total / Double(values.count), "p50": percentile(0.50),
            "p90": percentile(0.90), "p95": percentile(0.95), "p99": percentile(0.99),
            "max": sorted.last ?? 0.0,
        ]
    }

    private func emit(kind: String, object: [String: Any]) {
        guard let data = try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]) else { return }
        print("BENCHMARK_ARTIFACT \(kind) \(data.base64EncodedString())")
    }

    private func traceIdentity() -> [String: Any] {
        guard let metadata = config.cameraTraceMetadata else {
            return [
                "id": "orbit-yaw-step-\(config.yawStepRadians)",
                "sha256": traceHash(),
                "reference_width": NSNull(),
                "reference_height": NSNull(),
            ]
        }
        if config.cameraTraceSequence {
            return [
                "id": metadata.id,
                "sha256": metadata.sha256,
                "reference_width": metadata.width,
                "reference_height": metadata.height,
                "require_display_match": config.requireTraceDisplayMatch,
                "display_policy": config.requireTraceDisplayMatch
                    ? "trace_display_exact"
                    : "native_aspect_reprojection",
                "quality_comparable": config.requireTraceDisplayMatch,
                "frame_indices": config.cameraTraceFrameIndices,
            ]
        }
        return [
            "id": metadata.id,
            "sha256": metadata.sha256,
            "reference_width": metadata.width,
            "reference_height": metadata.height,
            "require_display_match": config.requireTraceDisplayMatch,
            "display_policy": config.requireTraceDisplayMatch
                ? "trace_display_exact"
                : "native_aspect_reprojection",
            "quality_comparable": config.requireTraceDisplayMatch,
            "frame_index": config.cameraTraceFrame,
        ]
    }

    private func format(_ value: Double) -> String { String(format: "%.3f", value) }
}

private func utc(_ date: Date) -> String {
    ISO8601DateFormatter().string(from: date)
}

private func thermalName(_ state: ProcessInfo.ThermalState) -> String {
    switch state {
    case .nominal: return "nominal"
    case .fair: return "fair"
    case .serious: return "serious"
    case .critical: return "critical"
    @unknown default: return "unknown"
    }
}

private func machineIdentifier() -> String {
    var info = utsname()
    uname(&info)
    return withUnsafePointer(to: &info.machine) {
        $0.withMemoryRebound(to: CChar.self, capacity: 1) { String(cString: $0) }
    }
}

private struct DatasetFacts {
    var sha256: String
    var bytes: UInt64
    var splatCount: Int
    var shDegree: Int
}

private func inspectDataset(path: String) -> DatasetFacts? {
    guard let handle = try? FileHandle(forReadingFrom: URL(fileURLWithPath: path)),
          let headerData = try? handle.read(upToCount: 65_536),
          !headerData.isEmpty else {
        return nil
    }
    try? handle.close()
    let marker = Data("end_header\n".utf8)
    let headerEnd = headerData.range(of: marker)?.upperBound ?? headerData.endIndex
    let header = String(data: headerData[..<headerEnd], encoding: .ascii) ?? ""
    let vertexLine = header.split(separator: "\n").first { $0.hasPrefix("element vertex ") }
    let count = vertexLine.flatMap { Int($0.split(separator: " ").last ?? "") } ?? 0
    guard count > 0 else { return nil }
    let restCount = header.split(separator: "\n").filter { $0.hasPrefix("property float f_rest_") }.count
    let degree = restCount >= 45 ? 3 : (restCount >= 24 ? 2 : (restCount >= 9 ? 1 : 0))
    let bytes = (try? FileManager.default.attributesOfItem(atPath: path)[.size] as? NSNumber)?.uint64Value ?? 0
    guard bytes > 0, let hash = sha256File(path) else { return nil }
    return DatasetFacts(sha256: hash, bytes: bytes, splatCount: count, shDegree: degree)
}

private func sha256File(_ path: String) -> String? {
    guard let handle = try? FileHandle(forReadingFrom: URL(fileURLWithPath: path)) else {
        return nil
    }
    defer { try? handle.close() }
    var hasher = SHA256()
    while let chunk = try? handle.read(upToCount: 1_048_576), !chunk.isEmpty {
        hasher.update(data: chunk)
    }
    return hasher.finalize().map { String(format: "%02x", $0) }.joined()
}

private func traceHash() -> String {
    SHA256.hash(data: Data("gsplat-ios-orbit-trace-v1".utf8)).map { String(format: "%02x", $0) }.joined()
}
