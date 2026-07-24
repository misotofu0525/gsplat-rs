package com.gsplat.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class GsplatSurfaceCurrentStatsTest {
    @Test
    fun mapsEveryExplicitRequestAndPollStatus() {
        GsplatSurfaceCurrentStatsRequestStatus.entries.forEach { status ->
            assertEquals(
                status,
                GsplatSurfaceCurrentStatsRequest.fromRaw(
                    longArrayOf(status.nativeValue.toLong())
                ).status
            )
        }
        assertEquals(
            GsplatSurfaceCurrentStatsPollKind.EMPTY,
            GsplatSurfaceCurrentStatsPoll.fromRaw(emptyPollRaw()).kind
        )
        GsplatSurfaceCurrentStatsRequestStatus.entries
            .filterNot { it == GsplatSurfaceCurrentStatsRequestStatus.REQUESTED }
            .forEach { status ->
                val raw = emptyPollRaw().also {
                    it[0] = GsplatSurfaceCurrentStatsPollKind.UNSAMPLED.nativeValue.toLong()
                    it[1] = status.nativeValue.toLong()
                }
                assertEquals(status, GsplatSurfaceCurrentStatsPoll.fromRaw(raw).requestStatus)
            }
    }

    @Test
    fun completeIdentityIncludesEveryJoinField() {
        val identity = identity()
        val raw = LongArray(GsplatSurfaceCurrentStatsIdentity.RAW_VALUE_COUNT)
        identity.writeRaw(raw, 0)
        assertEquals(identity, GsplatSurfaceCurrentStatsIdentity.fromRaw(raw, 0))

        val variants = listOf(
            identity.copy(sceneGeneration = 100),
            identity.copy(cameraRevision = 100),
            identity.copy(viewportGeneration = 100),
            identity.copy(contractGeneration = 100),
            identity.copy(planSetGeneration = 100),
            identity.copy(orderGeneration = 100),
            identity.copy(rasterGeneration = 100),
            identity.copy(encodeAttempt = 100),
            identity.copy(presentationSequence = 100),
            identity.copy(executedPlan = GsplatSurfaceCurrentStatsPlan.GPU_PREPROJECT)
        )

        assertTrue(variants.all { it != identity })
    }

    @Test
    fun mapsNotRequestedAndIssuedSubmissionPayloads() {
        val notRequestedRaw = LongArray(GsplatSurfaceCurrentStatsSubmission.RAW_VALUE_COUNT)
        notRequestedRaw[0] =
            GsplatSurfaceCurrentStatsSubmissionStatus.NOT_REQUESTED.nativeValue.toLong()
        assertEquals(notRequested(), GsplatSurfaceCurrentStatsSubmission.fromRaw(notRequestedRaw))

        val identity = identity()
        val issuedRaw = LongArray(GsplatSurfaceCurrentStatsSubmission.RAW_VALUE_COUNT)
        issuedRaw[0] = GsplatSurfaceCurrentStatsSubmissionStatus.ISSUED.nativeValue.toLong()
        issuedRaw[1] = 37
        identity.writeRaw(issuedRaw, 2)
        assertEquals(issued(37, identity), GsplatSurfaceCurrentStatsSubmission.fromRaw(issuedRaw))
    }

    @Test
    fun issuedThenEmptyIsPendingWithoutCounts() {
        val adapter = GsplatSurfaceCurrentStatsAdapter()
        val cycle = adapter.consume(
            requested(),
            issued(ticket = 41, identity = identity()),
            emptyPoll()
        )

        assertTrue(cycle.state is GsplatSurfaceCurrentStatsState.Pending)
        assertEquals(1, adapter.pendingCount)
        assertNull(adapter.currentReceipt)
    }

    @Test
    fun issuedThenEmptyCanCompleteThroughIndependentPoll() {
        val adapter = GsplatSurfaceCurrentStatsAdapter()
        val identity = identity()
        adapter.consume(requested(), issued(ticket = 43, identity = identity), emptyPoll())

        val state = adapter.consumePoll(
            readyPoll(
                ticket = 43,
                identity = identity,
                source = 100,
                visible = 80,
                contributor = 55,
                drawn = 55,
                semantics =
                    GsplatSurfaceCurrentStatsCountSemantics.INDIRECT_DRAW_EQUALS_CONTRIBUTOR
            )
        )

        val ready = state as GsplatSurfaceCurrentStatsState.Ready
        assertEquals(43L, ready.receipt.ticket)
        assertEquals(0, adapter.pendingCount)
        assertEquals(ready.receipt, adapter.currentReceipt)
    }

    @Test
    fun failedFrameIntentCanLandAsBusyThenIssuedOnNextSuccessfulFrame() {
        val adapter = GsplatSurfaceCurrentStatsAdapter()
        adapter.consume(requested(), notRequested(), emptyPoll())

        val cycle = adapter.consume(
            request(GsplatSurfaceCurrentStatsRequestStatus.BUSY),
            issued(ticket = 45, identity = identity()),
            emptyPoll()
        )

        assertTrue(cycle.state is GsplatSurfaceCurrentStatsState.Pending)
        assertEquals(45L, (cycle.state as GsplatSurfaceCurrentStatsState.Pending).ticket)
        assertEquals(1, adapter.pendingCount)
    }

    @Test
    fun busyAndLegacyUnavailableAreNonFatalAndNeverInventCounts() {
        val statuses = listOf(
            GsplatSurfaceCurrentStatsRequestStatus.BUSY,
            GsplatSurfaceCurrentStatsRequestStatus.GPU_UNAVAILABLE,
            GsplatSurfaceCurrentStatsRequestStatus.RESOURCE_UNAVAILABLE,
            GsplatSurfaceCurrentStatsRequestStatus.TICKET_EXHAUSTED
        )
        statuses.forEach { status ->
            val adapter = GsplatSurfaceCurrentStatsAdapter()
            val cycle = adapter.consume(request(status), notRequested(), emptyPoll())
            val unavailable = cycle.state as GsplatSurfaceCurrentStatsState.Unavailable
            assertEquals(status, unavailable.status)
            assertNull(adapter.currentReceipt)
            assertEquals(0, adapter.pendingCount)
        }
    }

    @Test
    fun matchingReadyPublishesAllCountsAtomically() {
        val adapter = GsplatSurfaceCurrentStatsAdapter()
        val identity = identity()
        adapter.consume(requested(), issued(51, identity), emptyPoll())

        val cycle = adapter.consume(
            request(GsplatSurfaceCurrentStatsRequestStatus.BUSY),
            notRequested(),
            readyPoll(
                ticket = 51,
                identity = identity,
                source = 100,
                visible = 80,
                contributor = 55,
                drawn = 55,
                semantics =
                    GsplatSurfaceCurrentStatsCountSemantics.INDIRECT_DRAW_EQUALS_CONTRIBUTOR
            )
        )

        val ready = cycle.state as GsplatSurfaceCurrentStatsState.Ready
        assertEquals(100, ready.receipt.sourceCount)
        assertEquals(80, ready.receipt.visibleCount)
        assertEquals(55, ready.receipt.contributorCount)
        assertEquals(55, ready.receipt.drawnCount)
        assertEquals(0, adapter.pendingCount)
        assertEquals(ready.receipt, adapter.currentReceipt)
    }

    @Test
    fun terminalFailureAndExpiryClearOnlyMatchingPending() {
        GsplatSurfaceCurrentStatsPollKind.entries
            .filter { it.isTerminalFailure }
            .forEachIndexed { index, kind ->
                val adapter = GsplatSurfaceCurrentStatsAdapter()
                val ticket = 60L + index
                val identity = identity().copy(encodeAttempt = ticket)
                adapter.consume(requested(), issued(ticket, identity), emptyPoll())

                val failed = adapter.consume(
                    request(GsplatSurfaceCurrentStatsRequestStatus.BUSY),
                    notRequested(),
                    failurePoll(kind, ticket, identity)
                ).state as GsplatSurfaceCurrentStatsState.Failed

                assertEquals(kind, failed.failure.kind)
                assertEquals(0, adapter.pendingCount)
                assertNull(adapter.currentReceipt)
            }
    }

    @Test
    fun ticketMatchWithoutFullIdentityIsRejected() {
        val adapter = GsplatSurfaceCurrentStatsAdapter()
        val identity = identity()
        adapter.consume(requested(), issued(71, identity), emptyPoll())

        val rejected = adapter.consume(
            request(GsplatSurfaceCurrentStatsRequestStatus.BUSY),
            notRequested(),
            readyPoll(
                ticket = 71,
                identity = identity.copy(contractGeneration = 999),
                source = 100,
                visible = 80,
                contributor = 55,
                drawn = 80,
                semantics = GsplatSurfaceCurrentStatsCountSemantics.INDIRECT_DRAW_EQUALS_VISIBLE
            )
        ).state as GsplatSurfaceCurrentStatsState.Rejected

        assertEquals("ready_identity_mismatch", rejected.reason)
        assertEquals(0, adapter.pendingCount)
        assertNull(adapter.currentReceipt)
    }

    @Test
    fun mismatchedFailureAlsoConsumesAndClearsPendingTicket() {
        val adapter = GsplatSurfaceCurrentStatsAdapter()
        val identity = identity()
        adapter.consume(requested(), issued(75, identity), emptyPoll())

        val rejected = adapter.consume(
            request(GsplatSurfaceCurrentStatsRequestStatus.BUSY),
            notRequested(),
            failurePoll(
                GsplatSurfaceCurrentStatsPollKind.EXPIRED,
                75,
                identity.copy(rasterGeneration = 999)
            )
        ).state as GsplatSurfaceCurrentStatsState.Rejected

        assertEquals("failure_identity_mismatch", rejected.reason)
        assertEquals(0, adapter.pendingCount)
        assertNull(adapter.currentReceipt)
    }

    @Test
    fun unavailableCycleClearsPreviouslyPublishedReadyInsteadOfFallingBack() {
        val adapter = GsplatSurfaceCurrentStatsAdapter()
        val identity = identity()
        adapter.consume(requested(), issued(81, identity), emptyPoll())
        adapter.consume(
            request(GsplatSurfaceCurrentStatsRequestStatus.BUSY),
            notRequested(),
            readyPoll(
                ticket = 81,
                identity = identity,
                source = 10,
                visible = 8,
                contributor = 6,
                drawn = 8,
                semantics = GsplatSurfaceCurrentStatsCountSemantics.DIRECT_DRAW_EQUALS_VISIBLE
            )
        )
        assertTrue(adapter.currentReceipt != null)

        adapter.consume(
            request(GsplatSurfaceCurrentStatsRequestStatus.GPU_UNAVAILABLE),
            notRequested(),
            emptyPoll()
        )

        assertNull(adapter.currentReceipt)
        assertTrue(adapter.state is GsplatSurfaceCurrentStatsState.Unavailable)
    }

    @Test
    fun readyParserRejectsZeroTicketAndInvalidCountSemantics() {
        val zeroTicket = readyPollRaw(
            ticket = 0,
            identity = identity(),
            source = 10,
            visible = 8,
            contributor = 6,
            drawn = 8,
            semantics = GsplatSurfaceCurrentStatsCountSemantics.DIRECT_DRAW_EQUALS_VISIBLE
        )
        assertThrows(IllegalStateException::class.java) {
            GsplatSurfaceCurrentStatsPoll.fromRaw(zeroTicket)
        }

        val invalidDrawn = readyPollRaw(
            ticket = 91,
            identity = identity(),
            source = 10,
            visible = 8,
            contributor = 6,
            drawn = 7,
            semantics = GsplatSurfaceCurrentStatsCountSemantics.DIRECT_DRAW_EQUALS_VISIBLE
        )
        assertThrows(IllegalStateException::class.java) {
            GsplatSurfaceCurrentStatsPoll.fromRaw(invalidDrawn)
        }
    }

    private fun identity() = GsplatSurfaceCurrentStatsIdentity(
        sceneGeneration = 1,
        cameraRevision = 2,
        viewportGeneration = 3,
        contractGeneration = 4,
        planSetGeneration = 5,
        orderGeneration = 6,
        rasterGeneration = 7,
        encodeAttempt = 8,
        presentationSequence = 9,
        executedPlan = GsplatSurfaceCurrentStatsPlan.GPU_POST_SORT
    )

    private fun request(status: GsplatSurfaceCurrentStatsRequestStatus) =
        GsplatSurfaceCurrentStatsRequest(status)

    private fun requested() = request(GsplatSurfaceCurrentStatsRequestStatus.REQUESTED)

    private fun issued(ticket: Long, identity: GsplatSurfaceCurrentStatsIdentity) =
        GsplatSurfaceCurrentStatsSubmission(
            GsplatSurfaceCurrentStatsSubmissionStatus.ISSUED,
            ticket,
            identity
        )

    private fun notRequested() = GsplatSurfaceCurrentStatsSubmission(
        GsplatSurfaceCurrentStatsSubmissionStatus.NOT_REQUESTED,
        null,
        null
    )

    private fun emptyPoll() = GsplatSurfaceCurrentStatsPoll(
        GsplatSurfaceCurrentStatsPollKind.EMPTY
    )

    private fun readyPoll(
        ticket: Long,
        identity: GsplatSurfaceCurrentStatsIdentity,
        source: Long,
        visible: Long,
        contributor: Long,
        drawn: Long,
        semantics: GsplatSurfaceCurrentStatsCountSemantics
    ) = GsplatSurfaceCurrentStatsPoll.fromRaw(
        readyPollRaw(ticket, identity, source, visible, contributor, drawn, semantics)
    )

    private fun failurePoll(
        kind: GsplatSurfaceCurrentStatsPollKind,
        ticket: Long,
        identity: GsplatSurfaceCurrentStatsIdentity
    ): GsplatSurfaceCurrentStatsPoll {
        val raw = emptyPollRaw()
        raw[0] = kind.nativeValue.toLong()
        raw[3] = ticket
        identity.writeRaw(raw, 4)
        return GsplatSurfaceCurrentStatsPoll.fromRaw(raw)
    }

    private fun emptyPollRaw() = LongArray(GsplatSurfaceCurrentStatsPoll.RAW_VALUE_COUNT).also {
        it[0] = GsplatSurfaceCurrentStatsPollKind.EMPTY.nativeValue.toLong()
    }

    private fun readyPollRaw(
        ticket: Long,
        identity: GsplatSurfaceCurrentStatsIdentity,
        source: Long,
        visible: Long,
        contributor: Long,
        drawn: Long,
        semantics: GsplatSurfaceCurrentStatsCountSemantics
    ) = emptyPollRaw().also {
        it[0] = GsplatSurfaceCurrentStatsPollKind.READY.nativeValue.toLong()
        it[2] = semantics.nativeValue.toLong()
        it[3] = ticket
        identity.writeRaw(it, 4)
        it[14] = source
        it[15] = visible
        it[16] = contributor
        it[17] = drawn
    }
}
