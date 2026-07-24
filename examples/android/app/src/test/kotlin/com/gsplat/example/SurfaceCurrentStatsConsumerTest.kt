package com.gsplat.example

import com.gsplat.android.GsplatSurfaceCurrentStatsCountSemantics
import com.gsplat.android.GsplatSurfaceCurrentStatsCycle
import com.gsplat.android.GsplatSurfaceCurrentStatsFailure
import com.gsplat.android.GsplatSurfaceCurrentStatsIdentity
import com.gsplat.android.GsplatSurfaceCurrentStatsPlan
import com.gsplat.android.GsplatSurfaceCurrentStatsPoll
import com.gsplat.android.GsplatSurfaceCurrentStatsPollKind
import com.gsplat.android.GsplatSurfaceCurrentStatsReceipt
import com.gsplat.android.GsplatSurfaceCurrentStatsRequest
import com.gsplat.android.GsplatSurfaceCurrentStatsRequestStatus
import com.gsplat.android.GsplatSurfaceCurrentStatsState
import com.gsplat.android.GsplatSurfaceCurrentStatsSubmission
import com.gsplat.android.GsplatSurfaceCurrentStatsSubmissionStatus
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class SurfaceCurrentStatsConsumerTest {
    @Test
    fun requestSubmissionAndLaterReadyCloseOneMeasuredSample() {
        val consumer = SurfaceCurrentStatsConsumer()
        val binding = binding(0)
        val request = consumer.beginRequest(binding) { requested() }
        val identity = identity(cameraRevision = 7, presentationSequence = 41)

        consumer.consumeCycle(pendingCycle(request, ticket = 11, identity = identity))
        assertFalse(consumer.benchmarkTerminalsComplete(1))
        consumer.consumePolledState(readyState(ticket = 11, identity = identity))

        val record = consumer.strictRecords(1).single()
        assertEquals(11L, record.ticket)
        assertEquals(identity, record.identity)
        assertTrue(record.terminal is SurfaceCurrentStatsTerminal.Ready)
    }

    @Test
    fun laterCycleCanIssueTicketTwoWhilePollingTicketOneTerminal() {
        val consumer = SurfaceCurrentStatsConsumer()
        val firstIdentity = identity(cameraRevision = 7, presentationSequence = 41)
        val firstRequest = consumer.beginRequest(binding(0)) { requested() }
        consumer.consumeCycle(pendingCycle(firstRequest, ticket = 11, identity = firstIdentity))
        assertEquals(1, consumer.trackedStateForTest().issuedTicketCount)

        val secondIdentity = identity(cameraRevision = 8, presentationSequence = 42)
        val secondRequest = consumer.beginRequest(binding(1)) { requested() }
        val firstReady = receipt(ticket = 11, identity = firstIdentity)
        consumer.consumeCycle(
            GsplatSurfaceCurrentStatsCycle(
                request = secondRequest,
                submission = GsplatSurfaceCurrentStatsSubmission(
                    GsplatSurfaceCurrentStatsSubmissionStatus.ISSUED,
                    12,
                    secondIdentity
                ),
                poll = GsplatSurfaceCurrentStatsPoll(
                    GsplatSurfaceCurrentStatsPollKind.READY,
                    receipt = firstReady
                ),
                state = GsplatSurfaceCurrentStatsState.Ready(
                    firstReady,
                    pendingCount = 1
                )
            )
        )
        assertFalse(consumer.benchmarkTerminalsComplete(2))
        assertEquals(1, consumer.trackedStateForTest().issuedTicketCount)

        consumer.consumePolledState(readyState(ticket = 12, identity = secondIdentity))
        assertEquals(0, consumer.trackedStateForTest().issuedTicketCount)
        val records = consumer.strictRecords(2)
        assertEquals(listOf(11L, 12L), records.map { it.ticket })
        assertEquals(firstIdentity, records[0].identity)
        assertEquals(secondIdentity, records[1].identity)
    }

    @Test
    fun twoPendingTicketsCanTerminateInReverseOrder() {
        val consumer = SurfaceCurrentStatsConsumer()
        val firstIdentity = identity(cameraRevision = 9, presentationSequence = 43)
        val firstRequest = consumer.beginRequest(binding(0)) { requested() }
        consumer.consumeCycle(pendingCycle(firstRequest, 13, firstIdentity))

        val secondIdentity = identity(cameraRevision = 10, presentationSequence = 44)
        val secondRequest = consumer.beginRequest(binding(1)) { requested() }
        consumer.consumeCycle(
            pendingCycle(
                secondRequest,
                ticket = 14,
                identity = secondIdentity,
                pendingCount = 2
            )
        )
        assertEquals(2, consumer.trackedStateForTest().issuedTicketCount)

        consumer.consumePolledState(
            readyState(ticket = 14, identity = secondIdentity, pendingCount = 1)
        )
        assertEquals(1, consumer.trackedStateForTest().issuedTicketCount)
        assertFalse(consumer.benchmarkTerminalsComplete(2))

        consumer.consumePolledState(readyState(ticket = 13, identity = firstIdentity))
        assertEquals(0, consumer.trackedStateForTest().issuedTicketCount)
        assertEquals(listOf(13L, 14L), consumer.strictRecords(2).map { it.ticket })
    }

    @Test
    fun failedRenderRetainsTheSamePreTicketIntent() {
        val consumer = SurfaceCurrentStatsConsumer()
        val binding = binding(0)
        val request = consumer.beginRequest(binding) { requested() }
        consumer.observeRenderFailure()
        var reread = false

        assertEquals(
            request,
            consumer.beginRequest(binding) {
                reread = true
                requested()
            }
        )
        assertFalse(reread)

        val identity = identity(cameraRevision = 8, presentationSequence = 42)
        consumer.consumeCycle(
            readyCycle(request, ticket = 12, identity = identity)
        )
        assertEquals(12L, consumer.strictRecords(1).single().ticket)
    }

    @Test
    fun failedRetryCommandClosesOldIntentAndInvalidatesStrictSample() {
        val consumer = SurfaceCurrentStatsConsumer()
        consumer.beginRequest(binding(0)) { requested() }
        consumer.observeRenderFailure()

        assertTrue(consumer.closeOutstandingAfterCommandFailure())
        assertFalse(consumer.hasInFlight)
        assertTrue(consumer.benchmarkTerminalsComplete(1))
        assertTrue(
            consumer.recordForSample(0)?.terminal is SurfaceCurrentStatsTerminal.Rejected
        )
        val unavailable = consumer.display as SurfaceCurrentStatsDisplay.Unavailable
        assertEquals("command_failed_current_stats_session_closed", unavailable.reason)
        assertThrows(IllegalStateException::class.java) { consumer.strictRecords(1) }
        assertFalse(consumer.closeOutstandingAfterCommandFailure())
    }

    @Test
    fun aSecondUnsubmittedPreTicketIntentIsRejected() {
        val consumer = SurfaceCurrentStatsConsumer()
        consumer.beginRequest(binding(0)) { requested() }

        assertThrows(IllegalStateException::class.java) {
            consumer.beginRequest(binding(1)) { requested() }
        }
    }

    @Test
    fun busyPreTicketNeverPublishesFallbackCounts() {
        val consumer = SurfaceCurrentStatsConsumer()
        consumer.beginRequest(binding(0)) {
            GsplatSurfaceCurrentStatsRequest(GsplatSurfaceCurrentStatsRequestStatus.BUSY)
        }

        assertTrue(consumer.display is SurfaceCurrentStatsDisplay.Unavailable)
        assertThrows(IllegalStateException::class.java) {
            consumer.strictRecords(1)
        }
    }

    @Test
    fun identityDriftAndTerminalFailureBothFailClosed() {
        val drift = SurfaceCurrentStatsConsumer()
        val driftRequest = drift.beginRequest(binding(0)) { requested() }
        val expected = identity(cameraRevision = 9, presentationSequence = 50)
        drift.consumeCycle(pendingCycle(driftRequest, 20, expected))
        drift.consumePolledState(
            readyState(
                ticket = 20,
                identity = identity(cameraRevision = 9, presentationSequence = 51)
            )
        )
        assertThrows(IllegalStateException::class.java) { drift.strictRecords(1) }

        val failed = SurfaceCurrentStatsConsumer()
        val failedRequest = failed.beginRequest(binding(0)) { requested() }
        failed.consumeCycle(pendingCycle(failedRequest, 21, expected))
        failed.consumePolledState(
            GsplatSurfaceCurrentStatsState.Failed(
                GsplatSurfaceCurrentStatsFailure(
                    GsplatSurfaceCurrentStatsPollKind.MAP_FAILURE,
                    21,
                    expected
                ),
                pendingCount = 0
            )
        )
        assertEquals(0, failed.trackedStateForTest().issuedTicketCount)
        assertThrows(IllegalStateException::class.java) { failed.strictRecords(1) }
    }

    @Test
    fun fixedCameraSamplesRequireDistinctTicketsAndPresentationSequences() {
        val consumer = SurfaceCurrentStatsConsumer()
        val firstIdentity = identity(cameraRevision = 12, presentationSequence = 70)
        val firstRequest = consumer.beginRequest(binding(0)) { requested() }
        consumer.consumeCycle(readyCycle(firstRequest, 30, firstIdentity))

        val secondIdentity = identity(cameraRevision = 12, presentationSequence = 71)
        val secondRequest = consumer.beginRequest(binding(1)) { requested() }
        consumer.consumeCycle(readyCycle(secondRequest, 31, secondIdentity))

        assertEquals(listOf(30L, 31L), consumer.strictRecords(2).map { it.ticket })

        val aliased = SurfaceCurrentStatsConsumer()
        val aliasFirst = aliased.beginRequest(binding(0)) { requested() }
        aliased.consumeCycle(readyCycle(aliasFirst, 40, firstIdentity))
        val aliasSecond = aliased.beginRequest(binding(1)) { requested() }
        aliased.consumeCycle(readyCycle(aliasSecond, 41, firstIdentity))
        assertThrows(IllegalStateException::class.java) { aliased.strictRecords(2) }
    }

    @Test
    fun duplicateTicketAndUnflushedPendingTicketFailClosed() {
        val duplicate = SurfaceCurrentStatsConsumer()
        val firstIdentity = identity(cameraRevision = 20, presentationSequence = 80)
        val firstRequest = duplicate.beginRequest(binding(0)) { requested() }
        duplicate.consumeCycle(readyCycle(firstRequest, 50, firstIdentity))
        val secondRequest = duplicate.beginRequest(binding(1)) { requested() }
        duplicate.consumeCycle(
            readyCycle(
                secondRequest,
                50,
                identity(cameraRevision = 21, presentationSequence = 81)
            )
        )
        assertThrows(IllegalStateException::class.java) { duplicate.strictRecords(2) }

        val pending = SurfaceCurrentStatsConsumer()
        val pendingRequest = pending.beginRequest(binding(0)) { requested() }
        pending.consumeCycle(
            pendingCycle(
                pendingRequest,
                60,
                identity(cameraRevision = 22, presentationSequence = 82)
            )
        )
        assertFalse(pending.benchmarkTerminalsComplete(1))
        assertThrows(IllegalStateException::class.java) { pending.strictRecords(1) }
    }

    @Test
    fun uiClearsReadyCountsAsSoonAsANewerSampleStarts() {
        val consumer = SurfaceCurrentStatsConsumer()
        val first = consumer.nextUiBinding()
        val request = consumer.beginRequest(first) { requested() }
        consumer.consumeCycle(
            readyCycle(
                request,
                70,
                identity(cameraRevision = 30, presentationSequence = 90)
            )
        )
        assertTrue(consumer.display is SurfaceCurrentStatsDisplay.Ready)

        consumer.beginRequest(consumer.nextUiBinding()) {
            GsplatSurfaceCurrentStatsRequest(
                GsplatSurfaceCurrentStatsRequestStatus.RESOURCE_UNAVAILABLE
            )
        }
        val unavailable = consumer.display as SurfaceCurrentStatsDisplay.Unavailable
        assertEquals("resource_unavailable", unavailable.reason)
    }

    @Test
    fun thousandsOfUiReadyCyclesKeepConsumerTrackingBounded() {
        val consumer = SurfaceCurrentStatsConsumer()

        repeat(5_000) { index ->
            val binding = consumer.nextUiBinding()
            val request = consumer.beginRequest(binding) { requested() }
            val ticket = index.toLong() + 1L
            val sampleIdentity = identity(
                cameraRevision = ticket,
                presentationSequence = ticket
            )
            val expectedReceipt = receipt(ticket, sampleIdentity)

            consumer.consumeCycle(readyCycle(request, ticket, sampleIdentity))

            val ready = consumer.display as SurfaceCurrentStatsDisplay.Ready
            assertEquals(binding, ready.binding)
            assertEquals(expectedReceipt, ready.receipt)
            assertEquals(0, consumer.trackedStateForTest().issuedTicketCount)
        }

        assertEquals(
            SurfaceCurrentStatsTrackedState(
                outstandingRequestCount = 0,
                issuedTicketCount = 0,
                sampleRecordCount = 0,
                rejectionRecordCount = 0
            ),
            consumer.trackedStateForTest()
        )
    }

    @Test
    fun newerPendingUiSampleNeverRedisplaysAnOlderReadyReceipt() {
        val consumer = SurfaceCurrentStatsConsumer()
        val firstBinding = consumer.nextUiBinding()
        val firstIdentity = identity(cameraRevision = 30, presentationSequence = 90)
        val firstRequest = consumer.beginRequest(firstBinding) { requested() }
        consumer.consumeCycle(readyCycle(firstRequest, 70, firstIdentity))
        assertEquals(0, consumer.trackedStateForTest().issuedTicketCount)

        val secondBinding = consumer.nextUiBinding()
        val secondIdentity = identity(cameraRevision = 31, presentationSequence = 91)
        val secondRequest = consumer.beginRequest(secondBinding) { requested() }
        consumer.consumeCycle(pendingCycle(secondRequest, 71, secondIdentity))
        assertTrue(consumer.display is SurfaceCurrentStatsDisplay.Unavailable)

        consumer.consumePolledState(readyState(70, firstIdentity))
        val stale = consumer.display as SurfaceCurrentStatsDisplay.Unavailable
        assertEquals("ready_without_matching_issued", stale.reason)
        assertEquals(1, consumer.trackedStateForTest().issuedTicketCount)

        consumer.consumePolledState(readyState(71, secondIdentity))
        val ready = consumer.display as SurfaceCurrentStatsDisplay.Ready
        assertEquals(secondBinding, ready.binding)
        assertEquals(71L, ready.receipt.ticket)
        assertEquals(0, consumer.trackedStateForTest().issuedTicketCount)
    }

    @Test
    fun duplicateAndStaleTerminalsKeepOnlyFirstFailureAndInvalidateStrictRecords() {
        val consumer = SurfaceCurrentStatsConsumer()
        val sampleIdentity = identity(cameraRevision = 40, presentationSequence = 100)
        val request = consumer.beginRequest(binding(0)) { requested() }
        consumer.consumeCycle(readyCycle(request, 80, sampleIdentity))

        repeat(2_000) { index ->
            val staleTicket = index.toLong() + 1_000L
            consumer.consumePolledState(
                readyState(
                    ticket = if (index % 2 == 0) 80 else staleTicket,
                    identity = if (index % 2 == 0) {
                        sampleIdentity
                    } else {
                        identity(staleTicket, staleTicket)
                    }
                )
            )
        }

        val tracked = consumer.trackedStateForTest()
        assertEquals(0, tracked.issuedTicketCount)
        assertEquals(1, tracked.sampleRecordCount)
        assertEquals(1, tracked.rejectionRecordCount)
        assertEquals(2, tracked.totalRecordCount)
        assertThrows(IllegalStateException::class.java) { consumer.strictRecords(1) }
    }

    @Test
    fun rejectionRecyclesItsPendingTicketAndClosesTheBenchmarkRecord() {
        val consumer = SurfaceCurrentStatsConsumer()
        val sampleIdentity = identity(cameraRevision = 50, presentationSequence = 110)
        val request = consumer.beginRequest(binding(0)) { requested() }
        consumer.consumeCycle(pendingCycle(request, 90, sampleIdentity))

        consumer.consumePolledState(
            GsplatSurfaceCurrentStatsState.Rejected(
                reason = "stale_ticket",
                ticket = 90,
                pendingCount = 0
            )
        )

        assertFalse(consumer.hasInFlight)
        assertTrue(consumer.benchmarkTerminalsComplete(1))
        assertTrue(
            consumer.recordForSample(0)?.terminal is SurfaceCurrentStatsTerminal.Rejected
        )
        assertEquals(0, consumer.trackedStateForTest().issuedTicketCount)
        assertEquals(1, consumer.trackedStateForTest().rejectionRecordCount)
        assertThrows(IllegalStateException::class.java) { consumer.strictRecords(1) }
    }

    private fun binding(index: Int) = SurfaceCurrentStatsFrameBinding(
        frameId = index.toLong() + 1,
        sampleIndex = index,
        traceFrameIndex = 0,
        traceTimestampNs = 0L
    )

    private fun requested() =
        GsplatSurfaceCurrentStatsRequest(GsplatSurfaceCurrentStatsRequestStatus.REQUESTED)

    private fun identity(
        cameraRevision: Long,
        presentationSequence: Long,
        plan: GsplatSurfaceCurrentStatsPlan = GsplatSurfaceCurrentStatsPlan.GPU_POST_SORT
    ) = GsplatSurfaceCurrentStatsIdentity(
        sceneGeneration = 1,
        cameraRevision = cameraRevision,
        viewportGeneration = 2,
        contractGeneration = 3,
        planSetGeneration = 4,
        orderGeneration = 5,
        rasterGeneration = 6,
        encodeAttempt = presentationSequence,
        presentationSequence = presentationSequence,
        executedPlan = plan
    )

    private fun pendingCycle(
        request: GsplatSurfaceCurrentStatsRequest,
        ticket: Long,
        identity: GsplatSurfaceCurrentStatsIdentity,
        pendingCount: Int = 1
    ) = GsplatSurfaceCurrentStatsCycle(
        request = request,
        submission = GsplatSurfaceCurrentStatsSubmission(
            GsplatSurfaceCurrentStatsSubmissionStatus.ISSUED,
            ticket,
            identity
        ),
        poll = GsplatSurfaceCurrentStatsPoll(GsplatSurfaceCurrentStatsPollKind.EMPTY),
        state = GsplatSurfaceCurrentStatsState.Pending(ticket, identity, pendingCount)
    )

    private fun readyCycle(
        request: GsplatSurfaceCurrentStatsRequest,
        ticket: Long,
        identity: GsplatSurfaceCurrentStatsIdentity
    ): GsplatSurfaceCurrentStatsCycle {
        val receipt = receipt(ticket, identity)
        return GsplatSurfaceCurrentStatsCycle(
            request = request,
            submission = GsplatSurfaceCurrentStatsSubmission(
                GsplatSurfaceCurrentStatsSubmissionStatus.ISSUED,
                ticket,
                identity
            ),
            poll = GsplatSurfaceCurrentStatsPoll(
                kind = GsplatSurfaceCurrentStatsPollKind.READY,
                receipt = receipt
            ),
            state = GsplatSurfaceCurrentStatsState.Ready(receipt, pendingCount = 0)
        )
    }

    private fun readyState(
        ticket: Long,
        identity: GsplatSurfaceCurrentStatsIdentity,
        pendingCount: Int = 0
    ): GsplatSurfaceCurrentStatsState =
        GsplatSurfaceCurrentStatsState.Ready(receipt(ticket, identity), pendingCount)

    private fun receipt(
        ticket: Long,
        identity: GsplatSurfaceCurrentStatsIdentity
    ): GsplatSurfaceCurrentStatsReceipt {
        val compact = identity.executedPlan == GsplatSurfaceCurrentStatsPlan.GPU_PREPROJECT
        return GsplatSurfaceCurrentStatsReceipt(
            ticket = ticket,
            identity = identity,
            sourceCount = 100,
            visibleCount = 80,
            contributorCount = 70,
            drawnCount = if (compact) 70 else 80,
            countSemantics = if (compact) {
                GsplatSurfaceCurrentStatsCountSemantics.INDIRECT_DRAW_EQUALS_CONTRIBUTOR
            } else {
                GsplatSurfaceCurrentStatsCountSemantics.INDIRECT_DRAW_EQUALS_VISIBLE
            }
        )
    }
}
