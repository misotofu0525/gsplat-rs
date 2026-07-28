package com.gsplat.example

import com.gsplat.android.GsplatSurfaceCurrentStatsCountSemantics
import com.gsplat.android.GsplatSurfaceCurrentStatsCycle
import com.gsplat.android.GsplatSurfaceCurrentStatsIdentity
import com.gsplat.android.GsplatSurfaceCurrentStatsPlan
import com.gsplat.android.GsplatSurfaceCurrentStatsPoll
import com.gsplat.android.GsplatSurfaceCurrentStatsPollKind
import com.gsplat.android.GsplatSurfaceCurrentStatsPollResult
import com.gsplat.android.GsplatSurfaceCurrentStatsReceipt
import com.gsplat.android.GsplatSurfaceCurrentStatsRequest
import com.gsplat.android.GsplatSurfaceCurrentStatsRequestStatus
import com.gsplat.android.GsplatSurfaceCurrentStatsState
import com.gsplat.android.GsplatSurfaceCurrentStatsSubmission
import com.gsplat.android.GsplatSurfaceCurrentStatsSubmissionStatus
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class BenchmarkTerminalDrainTest {
    @Test
    fun pendingTerminalsCompleteFromPollsWithoutCreatingNewWork() {
        var pendingOrderReceipts = 1
        var pendingCurrentStatsReceipts = 1
        var receiptPolls = 0
        var currentStatsPolls = 0
        var callbackPumps = 0
        var yields = 0

        val completed = drainBenchmarkTerminalReceipts(
            maxPolls = 3,
            terminalsComplete = {
                pendingOrderReceipts == 0 && pendingCurrentStatsReceipts == 0
            },
            pumpCallbacks = {
                callbackPumps += 1
                true
            },
            pollReceipts = {
                receiptPolls += 1
                pendingOrderReceipts = 0
                true
            },
            pollCurrentStats = {
                currentStatsPolls += 1
                pendingCurrentStatsReceipts = 0
                true
            },
            yieldAfterIncompletePoll = { yields += 1 }
        )

        assertTrue(completed)
        assertEquals(1, callbackPumps)
        assertEquals(1, receiptPolls)
        assertEquals(1, currentStatsPolls)
        assertEquals(0, yields)
    }

    @Test
    fun incompleteTerminalsRemainBoundedAndFailClosed() {
        var receiptPolls = 0
        var currentStatsPolls = 0
        var callbackPumps = 0
        var yields = 0

        val completed = drainBenchmarkTerminalReceipts(
            maxPolls = 3,
            terminalsComplete = { false },
            pumpCallbacks = {
                callbackPumps += 1
                true
            },
            pollReceipts = {
                receiptPolls += 1
                true
            },
            pollCurrentStats = {
                currentStatsPolls += 1
                true
            },
            yieldAfterIncompletePoll = { yields += 1 }
        )

        assertFalse(completed)
        assertEquals(3, callbackPumps)
        assertEquals(3, receiptPolls)
        assertEquals(3, currentStatsPolls)
        assertEquals(3, yields)
    }

    @Test
    fun callbackPumpFailureStopsBeforeReceiptPollsOrPublication() {
        var receiptsPolled = false
        var currentStatsPolled = false

        val completed = drainBenchmarkTerminalReceipts(
            maxPolls = 3,
            terminalsComplete = { false },
            pumpCallbacks = { false },
            pollReceipts = {
                receiptsPolled = true
                true
            },
            pollCurrentStats = {
                currentStatsPolled = true
                true
            }
        )

        assertFalse(completed)
        assertFalse(receiptsPolled)
        assertFalse(currentStatsPolled)
    }

    @Test
    fun drainRecordsHistoricalRawReadyBeforeNewerPendingUiProjection() {
        val consumer = SurfaceCurrentStatsConsumer()
        val historicalIdentity = identity(cameraRevision = 41, presentationSequence = 141)
        val historicalRequest = consumer.beginRequest(binding(0)) { requested() }
        consumer.consumeCycle(pendingCycle(historicalRequest, 101, historicalIdentity, 1))

        val currentIdentity = identity(cameraRevision = 42, presentationSequence = 142)
        val currentRequest = consumer.beginRequest(binding(1)) { requested() }
        consumer.consumeCycle(pendingCycle(currentRequest, 102, currentIdentity, 2))

        var pollIndex = 0
        val completed = drainBenchmarkTerminalReceipts(
            maxPolls = 2,
            terminalsComplete = { consumer.benchmarkTerminalsComplete(2) },
            pumpCallbacks = { true },
            pollReceipts = { true },
            pollCurrentStats = {
                val result = when (pollIndex++) {
                    0 -> GsplatSurfaceCurrentStatsPollResult(
                        poll = readyPoll(101, historicalIdentity),
                        state = GsplatSurfaceCurrentStatsState.Pending(
                            ticket = 102,
                            identity = currentIdentity,
                            pendingCount = 1
                        )
                    )
                    else -> GsplatSurfaceCurrentStatsPollResult(
                        poll = readyPoll(102, currentIdentity),
                        state = GsplatSurfaceCurrentStatsState.Ready(
                            receipt = readyReceipt(102, currentIdentity),
                            pendingCount = 0
                        )
                    )
                }
                consumer.pollPending { result }
                true
            }
        )

        assertTrue(completed)
        assertEquals(2, pollIndex)
        assertEquals(listOf(101L, 102L), consumer.strictRecords(2).map { it.ticket })
    }

    @Test
    fun strictTicketNamespacesAllowIndependentOrAbsentOrderTelemetry() {
        requireStrictFrameTicketNamespaces(
            frameIndex = 0,
            orderSubmissionTicket = 201,
            currentStatsTicket = 901
        )
        requireStrictFrameTicketNamespaces(
            frameIndex = 1,
            orderSubmissionTicket = null,
            currentStatsTicket = 202
        )

        val invalidCurrentStats = runCatching {
            requireStrictFrameTicketNamespaces(2, 203, 0)
        }.exceptionOrNull()
        assertTrue(invalidCurrentStats is IllegalStateException)
        assertTrue(invalidCurrentStats?.message?.contains("current-stats ticket") == true)

        val invalidOrder = runCatching {
            requireStrictFrameTicketNamespaces(3, 0, 205)
        }.exceptionOrNull()
        assertTrue(invalidOrder is IllegalStateException)
        assertTrue(invalidOrder?.message?.contains("issued order ticket") == true)
    }

    private fun binding(sampleIndex: Int) = SurfaceCurrentStatsFrameBinding(
        frameId = sampleIndex.toLong(),
        sampleIndex = sampleIndex,
        traceFrameIndex = sampleIndex,
        traceTimestampNs = sampleIndex.toLong()
    )

    private fun requested() = GsplatSurfaceCurrentStatsRequest(
        GsplatSurfaceCurrentStatsRequestStatus.REQUESTED
    )

    private fun identity(
        cameraRevision: Long,
        presentationSequence: Long
    ) = GsplatSurfaceCurrentStatsIdentity(
        sceneGeneration = 1,
        cameraRevision = cameraRevision,
        viewportGeneration = 2,
        contractGeneration = 3,
        planSetGeneration = 4,
        orderGeneration = cameraRevision,
        rasterGeneration = 5,
        encodeAttempt = cameraRevision,
        presentationSequence = presentationSequence,
        executedPlan = GsplatSurfaceCurrentStatsPlan.GPU_POST_SORT
    )

    private fun pendingCycle(
        request: GsplatSurfaceCurrentStatsRequest,
        ticket: Long,
        identity: GsplatSurfaceCurrentStatsIdentity,
        pendingCount: Int
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

    private fun readyPoll(
        ticket: Long,
        identity: GsplatSurfaceCurrentStatsIdentity
    ) = GsplatSurfaceCurrentStatsPoll(
        kind = GsplatSurfaceCurrentStatsPollKind.READY,
        receipt = readyReceipt(ticket, identity)
    )

    private fun readyReceipt(
        ticket: Long,
        identity: GsplatSurfaceCurrentStatsIdentity
    ) = GsplatSurfaceCurrentStatsReceipt(
        ticket = ticket,
        identity = identity,
        sourceCount = 10,
        visibleCount = 8,
        contributorCount = 6,
        drawnCount = 8,
        countSemantics =
            GsplatSurfaceCurrentStatsCountSemantics.INDIRECT_DRAW_EQUALS_VISIBLE
    )
}
