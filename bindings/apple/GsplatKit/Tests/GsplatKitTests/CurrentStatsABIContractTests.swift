import GsplatFFI
@testable import GsplatKit
import XCTest

final class CurrentStatsABIContractTests: XCTestCase {
    func testPackagedCurrentStatsV1LayoutAndVersionAreFrozen() {
        XCTAssertEqual(GSPLAT_SURFACE_CURRENT_STATS_ABI_VERSION_V1, 1)

        assertLayout(
            GsplatSurfaceCurrentStatsIdentityV1.self,
            size: 80,
            alignment: 8,
            offsets: [
                MemoryLayout.offset(of: \GsplatSurfaceCurrentStatsIdentityV1.scene_generation),
                MemoryLayout.offset(of: \GsplatSurfaceCurrentStatsIdentityV1.camera_revision),
                MemoryLayout.offset(of: \GsplatSurfaceCurrentStatsIdentityV1.viewport_generation),
                MemoryLayout.offset(of: \GsplatSurfaceCurrentStatsIdentityV1.contract_generation),
                MemoryLayout.offset(of: \GsplatSurfaceCurrentStatsIdentityV1.plan_set_generation),
                MemoryLayout.offset(of: \GsplatSurfaceCurrentStatsIdentityV1.order_generation),
                MemoryLayout.offset(of: \GsplatSurfaceCurrentStatsIdentityV1.raster_generation),
                MemoryLayout.offset(of: \GsplatSurfaceCurrentStatsIdentityV1.encode_attempt),
                MemoryLayout.offset(of: \GsplatSurfaceCurrentStatsIdentityV1.presentation_sequence),
                MemoryLayout.offset(of: \GsplatSurfaceCurrentStatsIdentityV1.executed_plan),
                MemoryLayout.offset(of: \GsplatSurfaceCurrentStatsIdentityV1.reserved),
            ],
            expectedOffsets: [0, 8, 16, 24, 32, 40, 48, 56, 64, 72, 76]
        )
        assertLayout(
            GsplatSurfaceCurrentStatsRequestV1.self,
            size: 32,
            alignment: 8,
            offsets: [
                MemoryLayout.offset(of: \GsplatSurfaceCurrentStatsRequestV1.struct_size),
                MemoryLayout.offset(of: \GsplatSurfaceCurrentStatsRequestV1.version),
                MemoryLayout.offset(of: \GsplatSurfaceCurrentStatsRequestV1.status),
                MemoryLayout.offset(of: \GsplatSurfaceCurrentStatsRequestV1.reserved),
                MemoryLayout.offset(of: \GsplatSurfaceCurrentStatsRequestV1.reserved_u64),
            ],
            expectedOffsets: [0, 4, 8, 12, 16]
        )
        assertLayout(
            GsplatSurfaceCurrentStatsSubmissionV1.self,
            size: 120,
            alignment: 8,
            offsets: [
                MemoryLayout.offset(of: \GsplatSurfaceCurrentStatsSubmissionV1.struct_size),
                MemoryLayout.offset(of: \GsplatSurfaceCurrentStatsSubmissionV1.version),
                MemoryLayout.offset(of: \GsplatSurfaceCurrentStatsSubmissionV1.status),
                MemoryLayout.offset(of: \GsplatSurfaceCurrentStatsSubmissionV1.reserved),
                MemoryLayout.offset(of: \GsplatSurfaceCurrentStatsSubmissionV1.ticket),
                MemoryLayout.offset(of: \GsplatSurfaceCurrentStatsSubmissionV1.identity),
                MemoryLayout.offset(of: \GsplatSurfaceCurrentStatsSubmissionV1.reserved_u64),
            ],
            expectedOffsets: [0, 4, 8, 12, 16, 24, 104]
        )
        assertLayout(
            GsplatSurfaceCurrentStatsPollV1.self,
            size: 144,
            alignment: 8,
            offsets: [
                MemoryLayout.offset(of: \GsplatSurfaceCurrentStatsPollV1.struct_size),
                MemoryLayout.offset(of: \GsplatSurfaceCurrentStatsPollV1.version),
                MemoryLayout.offset(of: \GsplatSurfaceCurrentStatsPollV1.kind),
                MemoryLayout.offset(of: \GsplatSurfaceCurrentStatsPollV1.request_status),
                MemoryLayout.offset(of: \GsplatSurfaceCurrentStatsPollV1.count_semantics),
                MemoryLayout.offset(of: \GsplatSurfaceCurrentStatsPollV1.reserved),
                MemoryLayout.offset(of: \GsplatSurfaceCurrentStatsPollV1.ticket),
                MemoryLayout.offset(of: \GsplatSurfaceCurrentStatsPollV1.identity),
                MemoryLayout.offset(of: \GsplatSurfaceCurrentStatsPollV1.source_count),
                MemoryLayout.offset(of: \GsplatSurfaceCurrentStatsPollV1.visible_count),
                MemoryLayout.offset(of: \GsplatSurfaceCurrentStatsPollV1.contributor_count),
                MemoryLayout.offset(of: \GsplatSurfaceCurrentStatsPollV1.drawn_count),
                MemoryLayout.offset(of: \GsplatSurfaceCurrentStatsPollV1.reserved_u64),
            ],
            expectedOffsets: [0, 4, 8, 12, 16, 20, 24, 32, 112, 116, 120, 124, 128]
        )
    }

    func testPackagedV1FieldsTranslateEveryExactPlanAndReadyCount() throws {
        let expectedPlans: [(UInt32, GsplatCurrentStatsPlan)] = [
            (1, .cpuPostSort),
            (2, .gpuPostSort),
            (3, .gpuPreproject),
        ]

        for (nativePlan, swiftPlan) in expectedPlans {
            var identity = GsplatSurfaceCurrentStatsIdentityV1()
            identity.scene_generation = 11
            identity.camera_revision = 12
            identity.viewport_generation = 13
            identity.contract_generation = 14
            identity.plan_set_generation = 15
            identity.order_generation = 16
            identity.raster_generation = 17
            identity.encode_attempt = 18
            identity.presentation_sequence = 19
            identity.executed_plan = nativePlan

            var submission = GsplatSurfaceCurrentStatsSubmissionV1()
            submission.struct_size = UInt32(
                MemoryLayout<GsplatSurfaceCurrentStatsSubmissionV1>.size
            )
            submission.version = GSPLAT_SURFACE_CURRENT_STATS_ABI_VERSION_V1
            submission.status = 2
            submission.ticket = UInt64(100 + nativePlan)
            submission.identity = identity

            guard case .issued(let issued) = try GsplatCurrentStatsSubmission(
                native: submission
            ) else {
                return XCTFail("packaged V1 Issued value did not translate")
            }
            XCTAssertEqual(issued.ticket, UInt64(100 + nativePlan))
            XCTAssertEqual(issued.identity.sceneGeneration, 11)
            XCTAssertEqual(issued.identity.cameraRevision, 12)
            XCTAssertEqual(issued.identity.viewportGeneration, 13)
            XCTAssertEqual(issued.identity.contractGeneration, 14)
            XCTAssertEqual(issued.identity.planSetGeneration, 15)
            XCTAssertEqual(issued.identity.orderGeneration, 16)
            XCTAssertEqual(issued.identity.rasterGeneration, 17)
            XCTAssertEqual(issued.identity.encodeAttempt, 18)
            XCTAssertEqual(issued.identity.presentationSequence, 19)
            XCTAssertEqual(issued.identity.executedPlan, swiftPlan)

            var poll = GsplatSurfaceCurrentStatsPollV1()
            poll.struct_size = UInt32(MemoryLayout<GsplatSurfaceCurrentStatsPollV1>.size)
            poll.version = GSPLAT_SURFACE_CURRENT_STATS_ABI_VERSION_V1
            poll.kind = 3
            poll.count_semantics = nativePlan == 3 ? 3 : 2
            poll.ticket = submission.ticket
            poll.identity = identity
            poll.source_count = 20
            poll.visible_count = 15
            poll.contributor_count = 12
            poll.drawn_count = nativePlan == 3 ? 12 : 15

            guard case .ready(let receipt) = try GsplatCurrentStatsPoll(native: poll) else {
                return XCTFail("packaged V1 Ready value did not translate")
            }
            XCTAssertEqual(receipt.ticket, submission.ticket)
            XCTAssertEqual(receipt.identity, issued.identity)
            XCTAssertEqual(receipt.sourceCount, 20)
            XCTAssertEqual(receipt.visibleCount, 15)
            XCTAssertEqual(receipt.contributorCount, 12)
            XCTAssertEqual(receipt.drawnCount, nativePlan == 3 ? 12 : 15)
            XCTAssertEqual(
                receipt.countSemantics,
                nativePlan == 3
                    ? .indirectDrawEqualsContributor
                    : .indirectDrawEqualsVisible
            )
        }
    }
}

private func assertLayout<T>(
    _ type: T.Type,
    size: Int,
    alignment: Int,
    offsets: [Int?],
    expectedOffsets: [Int],
    file: StaticString = #filePath,
    line: UInt = #line
) {
    XCTAssertEqual(MemoryLayout<T>.size, size, file: file, line: line)
    XCTAssertEqual(MemoryLayout<T>.stride, size, file: file, line: line)
    XCTAssertEqual(MemoryLayout<T>.alignment, alignment, file: file, line: line)
    XCTAssertEqual(offsets.compactMap { $0 }, expectedOffsets, file: file, line: line)
    XCTAssertEqual(offsets.count, expectedOffsets.count, file: file, line: line)
}
