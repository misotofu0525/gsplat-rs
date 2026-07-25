package com.gsplat.example

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
        var yields = 0

        val completed = drainBenchmarkTerminalReceipts(
            maxPolls = 3,
            terminalsComplete = {
                pendingOrderReceipts == 0 && pendingCurrentStatsReceipts == 0
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
        assertEquals(1, receiptPolls)
        assertEquals(1, currentStatsPolls)
        assertEquals(0, yields)
    }

    @Test
    fun incompleteTerminalsRemainBoundedAndFailClosed() {
        var receiptPolls = 0
        var currentStatsPolls = 0
        var yields = 0

        val completed = drainBenchmarkTerminalReceipts(
            maxPolls = 3,
            terminalsComplete = { false },
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
        assertEquals(3, receiptPolls)
        assertEquals(3, currentStatsPolls)
        assertEquals(3, yields)
    }

    @Test
    fun receiptPollFailureStopsBeforeCurrentStatsOrPublication() {
        var currentStatsPolled = false

        val completed = drainBenchmarkTerminalReceipts(
            maxPolls = 3,
            terminalsComplete = { false },
            pollReceipts = { false },
            pollCurrentStats = {
                currentStatsPolled = true
                true
            }
        )

        assertFalse(completed)
        assertFalse(currentStatsPolled)
    }
}
