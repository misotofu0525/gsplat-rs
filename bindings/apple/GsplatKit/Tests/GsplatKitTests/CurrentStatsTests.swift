import GsplatFFI
@testable import GsplatKit
import XCTest

final class CurrentStatsTests: XCTestCase {
    func testRequestAdmissionStatesAreNonfatalValues() throws {
        let expected: [GsplatCurrentStatsRequestStatus] = [
            .requested,
            .busy,
            .gpuUnavailable,
            .resourceUnavailable,
            .ticketExhausted,
        ]
        for status in expected {
            var native = request(status.rawValue)
            XCTAssertEqual(try currentStatsRequestStatus(native: native), status)
            native.status = 99
            XCTAssertThrowsError(try currentStatsRequestStatus(native: native))
        }
    }

    func testNotRequestedUnsampledAndEmptyDoNotPublishCounts() throws {
        var consumer = GsplatCurrentStatsConsumer()
        XCTAssertEqual(
            consumer.observe(try GsplatCurrentStatsSubmission(native: notRequestedSubmission())),
            .notRequested
        )
        for status: GsplatCurrentStatsRequestStatus in [
            .busy,
            .gpuUnavailable,
            .resourceUnavailable,
            .ticketExhausted,
        ] {
            XCTAssertEqual(
                consumer.consume(try GsplatCurrentStatsPoll(native: unsampledPoll(status))),
                .unsampled(status)
            )
        }
        XCTAssertEqual(
            consumer.consume(try GsplatCurrentStatsPoll(native: emptyPoll())),
            .empty
        )
        XCTAssertEqual(consumer.pendingCount, 0)
    }

    func testRepeatedIssuedSnapshotIsIdempotentButIdentityDriftIsRejected() throws {
        let identity = makeIdentity(seed: 5)
        let submission = try GsplatCurrentStatsSubmission(
            native: issuedSubmission(ticket: 11, identity: identity)
        )
        var consumer = GsplatCurrentStatsConsumer()
        guard case .pending = consumer.observe(submission),
              case .pending = consumer.observe(submission) else {
            return XCTFail("repeated read-only submission snapshot was not idempotent")
        }
        var driftedIdentity = identity
        driftedIdentity.presentation_sequence += 1
        let drifted = try GsplatCurrentStatsSubmission(
            native: issuedSubmission(ticket: 11, identity: driftedIdentity)
        )
        guard case .rejected(.identityMismatch) = consumer.observe(drifted) else {
            return XCTFail("same ticket with different complete identity was not rejected")
        }
        XCTAssertEqual(consumer.pendingCount, 0)
        guard case .rejected(.identityMismatch) = consumer.observe(drifted) else {
            return XCTFail("repeated drifted snapshot was not idempotently rejected")
        }

        for terminalIdentity in [identity, driftedIdentity] {
            let terminal = try GsplatCurrentStatsPoll(native: readyPoll(
                ticket: 11,
                identity: terminalIdentity,
                semantics: 1,
                source: 8,
                visible: 6,
                contributor: 5,
                drawn: 6
            ))
            XCTAssertEqual(
                consumer.consume(terminal),
                .rejected(.unknownTerminal(ticket: 11))
            )
            XCTAssertEqual(consumer.pendingCount, 0)
        }
    }

    func testReadyThenSameIssuedSnapshotReplayStaysSettled() throws {
        let identity = makeIdentity(seed: 7)
        let submission = try GsplatCurrentStatsSubmission(
            native: issuedSubmission(ticket: 17, identity: identity)
        )
        guard case .issued(let issued) = submission else {
            return XCTFail("fixture did not produce Issued")
        }
        let terminal = try GsplatCurrentStatsPoll(native: readyPoll(
            ticket: 17,
            identity: identity,
            semantics: 2,
            source: 12,
            visible: 9,
            contributor: 7,
            drawn: 9
        ))
        var consumer = GsplatCurrentStatsConsumer()
        XCTAssertEqual(consumer.observe(submission), .pending(issued))
        guard case .ready = consumer.consume(terminal) else {
            return XCTFail("matching terminal was not published")
        }
        XCTAssertEqual(consumer.observe(submission), .settled(issued))
        XCTAssertEqual(consumer.observe(submission), .settled(issued))
        XCTAssertEqual(consumer.pendingCount, 0)
    }

    func testReadyPublishesAtomicallyAfterCompleteIdentityMatch() throws {
        let identity = makeIdentity(seed: 10)
        let issued = try GsplatCurrentStatsSubmission(
            native: issuedSubmission(ticket: 41, identity: identity)
        )
        let ready = try GsplatCurrentStatsPoll(native: readyPoll(
            ticket: 41,
            identity: identity,
            semantics: 3,
            source: 20,
            visible: 15,
            contributor: 12,
            drawn: 12
        ))
        var consumer = GsplatCurrentStatsConsumer()
        guard case .pending = consumer.observe(issued) else {
            return XCTFail("Issued did not become pending")
        }
        guard case .ready(let receipt) = consumer.consume(ready) else {
            return XCTFail("matching Ready was not published")
        }
        XCTAssertEqual(receipt.ticket, 41)
        XCTAssertEqual(receipt.identity.executedPlan, .gpuPreproject)
        XCTAssertEqual(receipt.sourceCount, 20)
        XCTAssertEqual(receipt.visibleCount, 15)
        XCTAssertEqual(receipt.contributorCount, 12)
        XCTAssertEqual(receipt.drawnCount, 12)
        XCTAssertEqual(receipt.countSemantics, .indirectDrawEqualsContributor)
        XCTAssertEqual(consumer.pendingCount, 0)
    }

    func testZeroOpaqueRevisionGenerationAttemptAndSequenceValuesAreValid() throws {
        var identity = makeIdentity(seed: 0)
        identity.scene_generation = 0
        identity.camera_revision = 0
        identity.viewport_generation = 0
        identity.contract_generation = 0
        identity.plan_set_generation = 0
        identity.order_generation = 0
        identity.raster_generation = 0
        identity.encode_attempt = 0
        identity.presentation_sequence = 0
        let submission = try GsplatCurrentStatsSubmission(
            native: issuedSubmission(ticket: 7, identity: identity)
        )
        let terminal = try GsplatCurrentStatsPoll(native: readyPoll(
            ticket: 7,
            identity: identity,
            semantics: 1,
            source: 1,
            visible: 1,
            contributor: 1,
            drawn: 1
        ))
        var consumer = GsplatCurrentStatsConsumer()
        guard case .pending(let issued) = consumer.observe(submission) else {
            return XCTFail("zero-valued opaque identity was rejected")
        }
        XCTAssertEqual(issued.identity.cameraRevision, 0)
        XCTAssertEqual(issued.identity.viewportGeneration, 0)
        guard case .ready = consumer.consume(terminal) else {
            return XCTFail("matching zero-valued opaque identity was not accepted")
        }
    }

    func testEveryCompleteIdentityFieldMismatchIsRejectedAndTerminatesTicket() throws {
        let issuedIdentity = makeIdentity(seed: 20)
        let fields: [WritableKeyPath<GsplatSurfaceCurrentStatsIdentityV1, UInt64>] = [
            \.scene_generation,
            \.camera_revision,
            \.viewport_generation,
            \.contract_generation,
            \.plan_set_generation,
            \.order_generation,
            \.raster_generation,
            \.encode_attempt,
            \.presentation_sequence,
        ]
        var mismatches: [GsplatSurfaceCurrentStatsIdentityV1] = fields.map { field in
            var identity = issuedIdentity
            identity[keyPath: field] += 1
            return identity
        }
        var planMismatch = issuedIdentity
        planMismatch.executed_plan = 2
        mismatches.append(planMismatch)

        for (offset, terminalIdentity) in mismatches.enumerated() {
            let ticket = UInt64(42 + offset)
            let submission = try GsplatCurrentStatsSubmission(
                native: issuedSubmission(ticket: ticket, identity: issuedIdentity)
            )
            let terminal = try GsplatCurrentStatsPoll(native: readyPoll(
                ticket: ticket,
                identity: terminalIdentity,
                semantics: 2,
                source: 9,
                visible: 7,
                contributor: 6,
                drawn: 7
            ))
            var consumer = GsplatCurrentStatsConsumer()
            _ = consumer.observe(submission)
            guard case .rejected(.identityMismatch(
                let rejectedTicket,
                let expected,
                let actual
            )) = consumer.consume(terminal) else {
                return XCTFail("mismatched Ready was not rejected")
            }
            XCTAssertEqual(rejectedTicket, ticket)
            XCTAssertNotEqual(expected, actual)
            XCTAssertEqual(consumer.pendingCount, 0)
            guard case .rejected(.identityMismatch) = consumer.observe(submission) else {
                return XCTFail("terminal mismatch replay reopened pending")
            }
            XCTAssertEqual(consumer.pendingCount, 0)
        }
    }

    func testEveryTerminalFailureEndsOnlyItsMatchingPendingTicket() throws {
        let terminals: [(UInt32, GsplatCurrentStatsFailureReason)] = [
            (4, .mapFailure),
            (5, .generationInvalidated),
            (6, .expired),
            (7, .dropped),
        ]
        for (kind, reason) in terminals {
            let identity = makeIdentity(seed: UInt64(kind) * 10)
            let ticket = UInt64(100 + kind)
            let submission = try GsplatCurrentStatsSubmission(
                native: issuedSubmission(ticket: ticket, identity: identity)
            )
            guard case .issued(let issued) = submission else {
                return XCTFail("fixture did not produce Issued")
            }
            var consumer = GsplatCurrentStatsConsumer()
            _ = consumer.observe(submission)
            let event = consumer.consume(try GsplatCurrentStatsPoll(
                native: failurePoll(kind: kind, ticket: ticket, identity: identity)
            ))
            guard case .failure(let failure) = event else {
                return XCTFail("failure kind \(kind) was not accepted")
            }
            XCTAssertEqual(failure.ticket, ticket)
            XCTAssertEqual(failure.reason, reason)
            XCTAssertEqual(consumer.pendingCount, 0)
            XCTAssertEqual(consumer.observe(submission), .settled(issued))
            XCTAssertEqual(consumer.observe(submission), .settled(issued))
            XCTAssertEqual(consumer.pendingCount, 0)
        }
    }

    func testConcurrentPendingAcceptOutOfOrderTerminalsWithoutRevival() throws {
        let firstIdentity = makeIdentity(seed: 30)
        let secondIdentity = makeIdentity(seed: 40)
        let first = try GsplatCurrentStatsSubmission(
            native: issuedSubmission(ticket: 31, identity: firstIdentity)
        )
        let second = try GsplatCurrentStatsSubmission(
            native: issuedSubmission(ticket: 32, identity: secondIdentity)
        )
        guard case .issued(let secondIssued) = second else {
            return XCTFail("fixture did not produce Issued")
        }
        var consumer = GsplatCurrentStatsConsumer()
        _ = consumer.observe(first)
        _ = consumer.observe(second)
        XCTAssertEqual(consumer.pendingCount, 2)

        guard case .ready(let secondReceipt) = consumer.consume(
            try GsplatCurrentStatsPoll(native: readyPoll(
                ticket: 32,
                identity: secondIdentity,
                semantics: 3,
                source: 20,
                visible: 15,
                contributor: 12,
                drawn: 12
            ))
        ) else {
            return XCTFail("newer pending terminal was not accepted first")
        }
        XCTAssertEqual(secondReceipt.ticket, 32)
        XCTAssertEqual(consumer.pendingCount, 1)
        XCTAssertEqual(consumer.observe(second), .settled(secondIssued))

        guard case .failure(let firstFailure) = consumer.consume(
            try GsplatCurrentStatsPoll(native: failurePoll(
                kind: 6,
                ticket: 31,
                identity: firstIdentity
            ))
        ) else {
            return XCTFail("older pending terminal was not accepted later")
        }
        XCTAssertEqual(firstFailure.ticket, 31)
        XCTAssertEqual(firstFailure.reason, .expired)
        XCTAssertEqual(consumer.pendingCount, 0)
        XCTAssertEqual(consumer.observe(second), .settled(secondIssued))
    }

    func testNotRequestedEmptyAndUnsampledCannotReviveReadyCounts() throws {
        let identity = makeIdentity(seed: 50)
        let submission = try GsplatCurrentStatsSubmission(
            native: issuedSubmission(ticket: 51, identity: identity)
        )
        guard case .issued(let issued) = submission else {
            return XCTFail("fixture did not produce Issued")
        }
        var consumer = GsplatCurrentStatsConsumer()
        _ = consumer.observe(submission)
        guard case .ready = consumer.consume(try GsplatCurrentStatsPoll(native: readyPoll(
            ticket: 51,
            identity: identity,
            semantics: 1,
            source: 6,
            visible: 5,
            contributor: 4,
            drawn: 5
        ))) else {
            return XCTFail("matching Ready was not accepted")
        }

        let settled = consumer.observe(submission)
        let notRequested = consumer.observe(.notRequested)
        let empty = consumer.consume(.empty)
        let unsampled = consumer.consume(.unsampled(.busy))
        XCTAssertEqual(settled, .settled(issued))
        XCTAssertEqual(notRequested, .notRequested)
        XCTAssertEqual(empty, .empty)
        XCTAssertEqual(unsampled, .unsampled(.busy))
        XCTAssertFalse(settled.isReady)
        XCTAssertFalse(notRequested.isReady)
        XCTAssertFalse(empty.isReady)
        XCTAssertFalse(unsampled.isReady)
        XCTAssertEqual(consumer.pendingCount, 0)
    }

    func testBusyAdmissionCanObserveIssuedSnapshotFromLaterSuccessfulFrame() throws {
        XCTAssertEqual(
            try currentStatsRequestStatus(native: request(
                GsplatCurrentStatsRequestStatus.busy.rawValue
            )),
            .busy
        )
        let identity = makeIdentity(seed: 70)
        let submission = try GsplatCurrentStatsSubmission(
            native: issuedSubmission(ticket: 71, identity: identity)
        )
        var consumer = GsplatCurrentStatsConsumer()
        guard case .pending = consumer.observe(submission),
              case .pending = consumer.observe(submission) else {
            return XCTFail("later successful-frame Issued snapshot was not idempotent")
        }
        guard case .ready = consumer.consume(try GsplatCurrentStatsPoll(native: readyPoll(
            ticket: 71,
            identity: identity,
            semantics: 2,
            source: 10,
            visible: 8,
            contributor: 6,
            drawn: 8
        ))) else {
            return XCTFail("later successful-frame terminal was not accepted")
        }
        XCTAssertEqual(consumer.pendingCount, 0)
    }

    func testCompletedTicketHistoryDoesNotGrowAcrossSnapshots() throws {
        var consumer = GsplatCurrentStatsConsumer()
        for ticket in UInt64(200)..<UInt64(264) {
            let identity = makeIdentity(seed: ticket)
            let submission = try GsplatCurrentStatsSubmission(
                native: issuedSubmission(ticket: ticket, identity: identity)
            )
            guard case .issued(let issued) = submission else {
                return XCTFail("fixture did not produce Issued")
            }
            XCTAssertEqual(consumer.observe(submission), .pending(issued))
            guard case .failure = consumer.consume(try GsplatCurrentStatsPoll(
                native: failurePoll(kind: 7, ticket: ticket, identity: identity)
            )) else {
                return XCTFail("matching terminal was not accepted")
            }
            XCTAssertEqual(consumer.observe(submission), .settled(issued))
            XCTAssertEqual(consumer.pendingCount, 0)
        }
    }

    func testEmptyAfterEarlierReadyCannotFallbackToOldCounts() throws {
        let firstIdentity = makeIdentity(seed: 60)
        let secondIdentity = makeIdentity(seed: 80)
        var consumer = GsplatCurrentStatsConsumer()
        _ = consumer.observe(try GsplatCurrentStatsSubmission(
            native: issuedSubmission(ticket: 61, identity: firstIdentity)
        ))
        guard case .ready = consumer.consume(try GsplatCurrentStatsPoll(native: readyPoll(
            ticket: 61,
            identity: firstIdentity,
            semantics: 1,
            source: 5,
            visible: 4,
            contributor: 3,
            drawn: 4
        ))) else {
            return XCTFail("first Ready was not accepted")
        }
        _ = consumer.observe(try GsplatCurrentStatsSubmission(
            native: issuedSubmission(ticket: 62, identity: secondIdentity)
        ))
        XCTAssertEqual(
            consumer.consume(try GsplatCurrentStatsPoll(native: emptyPoll())),
            .empty
        )
        XCTAssertEqual(consumer.pendingCount, 1)
    }

    func testHeadersReservedEnumsAndPayloadApplicabilityFailClosed() throws {
        var requestValue = request(GsplatCurrentStatsRequestStatus.requested.rawValue)
        requestValue.version = 2
        XCTAssertThrowsError(try currentStatsRequestStatus(native: requestValue))
        requestValue.version = 1
        requestValue.reserved = 1
        XCTAssertThrowsError(try currentStatsRequestStatus(native: requestValue))

        var submission = notRequestedSubmission()
        submission.ticket = 9
        XCTAssertThrowsError(try GsplatCurrentStatsSubmission(native: submission))
        submission = issuedSubmission(ticket: 9, identity: makeIdentity(seed: 90))
        submission.identity.reserved = 1
        XCTAssertThrowsError(try GsplatCurrentStatsSubmission(native: submission))

        var poll = emptyPoll()
        poll.visible_count = 1
        XCTAssertThrowsError(try GsplatCurrentStatsPoll(native: poll))

        poll = readyPoll(
            ticket: 9,
            identity: makeIdentity(seed: 90),
            semantics: 99,
            source: 4,
            visible: 3,
            contributor: 2,
            drawn: 3
        )
        XCTAssertThrowsError(try GsplatCurrentStatsPoll(native: poll))

        poll = readyPoll(
            ticket: 9,
            identity: makeIdentity(seed: 90),
            semantics: 2,
            source: 4,
            visible: 3,
            contributor: 2,
            drawn: 3
        )
        poll.identity.executed_plan = 99
        XCTAssertThrowsError(try GsplatCurrentStatsPoll(native: poll))

        poll = failurePoll(kind: 4, ticket: 9, identity: makeIdentity(seed: 90))
        poll.source_count = 4
        XCTAssertThrowsError(try GsplatCurrentStatsPoll(native: poll))
    }
}

private func request(_ status: UInt32) -> GsplatSurfaceCurrentStatsRequestV1 {
    var native = GsplatSurfaceCurrentStatsRequestV1()
    native.struct_size = UInt32(MemoryLayout<GsplatSurfaceCurrentStatsRequestV1>.size)
    native.version = 1
    native.status = status
    return native
}

private func makeIdentity(seed: UInt64) -> GsplatSurfaceCurrentStatsIdentityV1 {
    var native = GsplatSurfaceCurrentStatsIdentityV1()
    native.scene_generation = seed + 1
    native.camera_revision = seed + 2
    native.viewport_generation = seed + 3
    native.contract_generation = seed + 4
    native.plan_set_generation = seed + 5
    native.order_generation = seed + 6
    native.raster_generation = seed + 7
    native.encode_attempt = seed + 8
    native.presentation_sequence = seed + 9
    native.executed_plan = 3
    return native
}

private func notRequestedSubmission() -> GsplatSurfaceCurrentStatsSubmissionV1 {
    var native = GsplatSurfaceCurrentStatsSubmissionV1()
    native.struct_size = UInt32(MemoryLayout<GsplatSurfaceCurrentStatsSubmissionV1>.size)
    native.version = 1
    native.status = 1
    return native
}

private func issuedSubmission(
    ticket: UInt64,
    identity: GsplatSurfaceCurrentStatsIdentityV1
) -> GsplatSurfaceCurrentStatsSubmissionV1 {
    var native = notRequestedSubmission()
    native.status = 2
    native.ticket = ticket
    native.identity = identity
    return native
}

private func emptyPoll() -> GsplatSurfaceCurrentStatsPollV1 {
    var native = GsplatSurfaceCurrentStatsPollV1()
    native.struct_size = UInt32(MemoryLayout<GsplatSurfaceCurrentStatsPollV1>.size)
    native.version = 1
    native.kind = 1
    return native
}

private func unsampledPoll(
    _ status: GsplatCurrentStatsRequestStatus
) -> GsplatSurfaceCurrentStatsPollV1 {
    var native = emptyPoll()
    native.kind = 2
    native.request_status = status.rawValue
    return native
}

private func readyPoll(
    ticket: UInt64,
    identity: GsplatSurfaceCurrentStatsIdentityV1,
    semantics: UInt32,
    source: UInt32,
    visible: UInt32,
    contributor: UInt32,
    drawn: UInt32
) -> GsplatSurfaceCurrentStatsPollV1 {
    var native = emptyPoll()
    native.kind = 3
    native.count_semantics = semantics
    native.ticket = ticket
    native.identity = identity
    native.source_count = source
    native.visible_count = visible
    native.contributor_count = contributor
    native.drawn_count = drawn
    return native
}

private func failurePoll(
    kind: UInt32,
    ticket: UInt64,
    identity: GsplatSurfaceCurrentStatsIdentityV1
) -> GsplatSurfaceCurrentStatsPollV1 {
    var native = emptyPoll()
    native.kind = kind
    native.ticket = ticket
    native.identity = identity
    return native
}
