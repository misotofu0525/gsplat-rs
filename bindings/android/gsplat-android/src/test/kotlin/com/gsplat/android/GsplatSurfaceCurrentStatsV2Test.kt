package com.gsplat.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Test

class GsplatSurfaceCurrentStatsV2Test {
    @Test
    fun emptyHasNoTicketCountsOrTiming() {
        val raw = LongArray(GsplatSurfaceCurrentStatsPollV2.RAW_VALUE_COUNT)
        raw[0] = GsplatSurfaceCurrentStatsPollKind.EMPTY.nativeValue.toLong()

        val poll = GsplatSurfaceCurrentStatsPollV2.fromRaw(raw)
        assertEquals(GsplatSurfaceCurrentStatsPollKind.EMPTY, poll.kind)
        assertNull(poll.receipt)
        assertNull(poll.failure)
    }

    @Test
    fun failureRetainsOneTicketAndCompleteIdentityWithoutTiming() {
        val raw = baseTerminalRaw(GsplatSurfaceCurrentStatsPollKind.MAP_FAILURE, ticket = 41)

        val poll = GsplatSurfaceCurrentStatsPollV2.fromRaw(raw)
        assertEquals(41L, poll.failure?.ticket)
        assertEquals(identity(), poll.failure?.identity)
        assertNull(poll.receipt)
    }

    @Test
    fun readyCarriesSameTicketCountsSemanticsAndValidTiming() {
        val raw = baseTerminalRaw(GsplatSurfaceCurrentStatsPollKind.READY, ticket = 73)
        raw[2] = GsplatSurfaceCurrentStatsCountSemantics.INDIRECT_DRAW_EQUALS_VISIBLE
            .nativeValue.toLong()
        raw[3] = 0b111
        raw[15] = 10
        raw[16] = 8
        raw[17] = 6
        raw[18] = 8
        raw[19] = 6.5f.toRawBits().toLong() and 0xffff_ffffL
        raw[20] = 1.25f.toRawBits().toLong() and 0xffff_ffffL
        raw[21] = 2.75f.toRawBits().toLong() and 0xffff_ffffL

        val receipt = GsplatSurfaceCurrentStatsPollV2.fromRaw(raw).receipt
        assertEquals(73L, receipt?.ticket)
        assertEquals(identity(), receipt?.identity)
        assertEquals(10L, receipt?.sourceCount)
        assertEquals(8L, receipt?.visibleCount)
        assertEquals(6L, receipt?.contributorCount)
        assertEquals(8L, receipt?.drawnCount)
        assertEquals(6.5f, receipt?.timing?.frameCompleteMs)
        assertEquals(1.25f, receipt?.timing?.cpuPreprocessMs)
        assertEquals(2.75f, receipt?.timing?.cpuSortMs)
        val v1 = GsplatSurfaceCurrentStatsPollV2.fromRaw(raw).withoutTiming()
        assertEquals(73L, v1.receipt?.ticket)
        assertEquals(8L, v1.receipt?.drawnCount)
    }

    @Test
    fun readyRejectsMissingFrameFlagOrPayloadBehindAbsentOptionalFlag() {
        val missingFrame = baseTerminalRaw(GsplatSurfaceCurrentStatsPollKind.READY, ticket = 1)
        missingFrame[2] = GsplatSurfaceCurrentStatsCountSemantics.DIRECT_DRAW_EQUALS_VISIBLE
            .nativeValue.toLong()
        assertThrows(IllegalStateException::class.java) {
            GsplatSurfaceCurrentStatsPollV2.fromRaw(missingFrame)
        }

        val hiddenPayload = missingFrame.copyOf()
        hiddenPayload[3] = 0b001
        hiddenPayload[19] = 1f.toRawBits().toLong() and 0xffff_ffffL
        hiddenPayload[20] = 2f.toRawBits().toLong() and 0xffff_ffffL
        assertThrows(IllegalStateException::class.java) {
            GsplatSurfaceCurrentStatsPollV2.fromRaw(hiddenPayload)
        }
    }

    private fun baseTerminalRaw(
        kind: GsplatSurfaceCurrentStatsPollKind,
        ticket: Long
    ) = LongArray(GsplatSurfaceCurrentStatsPollV2.RAW_VALUE_COUNT).also {
        it[0] = kind.nativeValue.toLong()
        it[4] = ticket
        identity().writeRaw(it, 5)
    }

    private fun identity() = GsplatSurfaceCurrentStatsIdentity(
        sceneGeneration = 3,
        cameraRevision = 5,
        viewportGeneration = 7,
        contractGeneration = 11,
        planSetGeneration = 13,
        orderGeneration = 17,
        rasterGeneration = 19,
        encodeAttempt = 23,
        presentationSequence = 29,
        executedPlan = GsplatSurfaceCurrentStatsPlan.CPU_POST_SORT
    )
}
