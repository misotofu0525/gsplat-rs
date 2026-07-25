import GsplatFFI
@testable import GsplatKit
import XCTest

final class ProjectedSubmissionContractTests: XCTestCase {
    func testIssuedProjectedTicketIsOpaqueNonzeroIdentity() throws {
        var native = validAdaptiveSubmission(ticket: 1)

        let submission = try GsplatProjectedDrawSubmission(native)

        XCTAssertEqual(submission.ticket, 1)
        XCTAssertEqual(submission.cameraRevision, 7)
        XCTAssertEqual(submission.requestedPolicy, .adaptive)
        XCTAssertEqual(submission.actualExecution, .candidate)
        XCTAssertEqual(submission.orderBackend, .cpu)
        XCTAssertNil(submission.unsampledReason)

        native.ticket = (1 << 53) + 1
        XCTAssertEqual(try GsplatProjectedDrawSubmission(native).ticket, native.ticket)
        XCTAssertThrowsError(
            try GsplatProjectedDrawSubmission(validAdaptiveSubmission(ticket: 0))
        )
    }

    private func validAdaptiveSubmission(
        ticket: UInt64
    ) -> GsplatSurfaceProjectedSubmissionV1 {
        var native = GsplatSurfaceProjectedSubmissionV1()
        native.struct_size = UInt32(MemoryLayout<GsplatSurfaceProjectedSubmissionV1>.size)
        native.version = GSPLAT_SURFACE_PROJECTED_ABI_VERSION_V1
        native.ticket = ticket
        native.camera_revision = 7
        native.requested_policy = 3
        native.actual_execution = 1
        native.order_backend = 0
        native.adaptive_state = 1
        native.flags = 1
        return native
    }
}
