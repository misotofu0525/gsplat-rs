package com.gsplat.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class GsplatSurfaceProjectedReceiptsTest {
    @Test
    fun optionsDefaultToIndependentAdaptiveProjectedPolicy() {
        assertEquals(
            GsplatSurfaceProjectedPolicy.ADAPTIVE,
            GsplatSurfaceOptions().projectedPolicy
        )
    }

    @Test
    fun adaptiveSubmissionCarriesIssuedTicketIdentity() {
        val receipt = GsplatSurfaceProjectedSubmission.fromRaw(
            longArrayOf(
                7L,
                42L,
                GsplatSurfaceProjectedPolicy.ADAPTIVE.nativeValue.toLong(),
                GsplatSurfaceProjectedExecution.COMPACT.nativeValue.toLong(),
                GsplatSurfaceOrderBackend.GPU.nativeValue.toLong(),
                GsplatSurfaceProjectedAdaptiveState.COMPACT_PROBE.nativeValue.toLong(),
                1L
            )
        )

        assertEquals(7L, receipt.ticket)
        assertEquals(42L, receipt.cameraRevision)
        assertEquals(GsplatSurfaceProjectedPolicy.ADAPTIVE, receipt.requestedPolicy)
        assertEquals(GsplatSurfaceProjectedExecution.COMPACT, receipt.actualExecution)
        assertEquals(GsplatSurfaceProjectedAdaptiveState.COMPACT_PROBE, receipt.adaptiveState)
        assertNull(receipt.unsampledReason)
    }

    @Test
    fun forcedCandidateDoesNotFabricateProjectedTicket() {
        val receipt = GsplatSurfaceProjectedSubmission.fromRaw(
            longArrayOf(
                0L,
                9L,
                GsplatSurfaceProjectedPolicy.CANDIDATE.nativeValue.toLong(),
                GsplatSurfaceProjectedExecution.CANDIDATE.nativeValue.toLong(),
                GsplatSurfaceOrderBackend.CPU.nativeValue.toLong(),
                GsplatSurfaceProjectedAdaptiveState.DISABLED.nativeValue.toLong(),
                0L
            )
        )
        assertNull(receipt.ticket)
        assertNull(receipt.unsampledReason)

        assertThrows(IllegalStateException::class.java) {
            GsplatSurfaceProjectedSubmission.fromRaw(
                longArrayOf(
                    12L,
                    9L,
                    GsplatSurfaceProjectedPolicy.CANDIDATE.nativeValue.toLong(),
                    GsplatSurfaceProjectedExecution.CANDIDATE.nativeValue.toLong(),
                    GsplatSurfaceOrderBackend.CPU.nativeValue.toLong(),
                    GsplatSurfaceProjectedAdaptiveState.DISABLED.nativeValue.toLong(),
                    1L
                )
            )
        }
    }

    @Test
    fun candidateMeasurementRetainsExactContributorEvidenceWhileDrawingVisibleSet() {
        val receipt = GsplatSurfaceProjectedMeasurement.fromRaw(
            projectedMeasurementRaw(
                execution = GsplatSurfaceProjectedExecution.CANDIDATE,
                flags = 3,
                visible = 100L,
                contributor = 55L,
                drawn = 100L,
                countsFlags = 0
            )
        )!!

        assertEquals(100L, receipt.visibleCount)
        assertEquals(55L, receipt.contributorCount)
        assertEquals(100L, receipt.drawnCount)
        assertFalse(receipt.exactContributorCompaction)
        assertTrue(receipt.projectionRebuilt)
        assertTrue(receipt.orderRefreshed)
    }

    @Test
    fun compactMeasurementRequiresExactDrawnEqualsContributorInvariant() {
        val receipt = GsplatSurfaceProjectedMeasurement.fromRaw(
            projectedMeasurementRaw(
                execution = GsplatSurfaceProjectedExecution.COMPACT,
                flags = 4,
                visible = 100L,
                contributor = 55L,
                drawn = 55L,
                countsFlags = 1
            )
        )!!
        assertTrue(receipt.exactContributorCompaction)
        assertEquals(receipt.contributorCount, receipt.drawnCount)

        assertThrows(IllegalStateException::class.java) {
            GsplatSurfaceProjectedMeasurement.fromRaw(
                projectedMeasurementRaw(
                    execution = GsplatSurfaceProjectedExecution.COMPACT,
                    flags = 4,
                    visible = 100L,
                    contributor = 55L,
                    drawn = 56L,
                    countsFlags = 1
                )
            )
        }
    }

    @Test
    fun emptyPollAndTerminalFailureRemainDistinct() {
        assertNull(
            GsplatSurfaceProjectedMeasurement.fromRaw(
                LongArray(GsplatSurfaceProjectedMeasurement.RAW_VALUE_COUNT)
            )
        )
        assertNull(
            GsplatSurfaceProjectedMeasurementFailure.fromRaw(
                LongArray(GsplatSurfaceProjectedMeasurementFailure.RAW_VALUE_COUNT)
            )
        )

        val failure = GsplatSurfaceProjectedMeasurementFailure.fromRaw(
            longArrayOf(
                1L,
                91L,
                14L,
                6L,
                3L,
                GsplatSurfaceProjectedMeasurementFailureReason.INVARIANT_VIOLATION.nativeValue.toLong(),
                GsplatSurfaceProjectedExecution.COMPACT.nativeValue.toLong(),
                GsplatSurfaceOrderBackend.GPU.nativeValue.toLong(),
                1L
            )
        )!!
        assertEquals(91L, failure.ticket)
        assertEquals(GsplatSurfaceProjectedMeasurementFailureReason.INVARIANT_VIOLATION, failure.reason)
        assertTrue(failure.droppedPriorFailures)
    }

    private fun projectedMeasurementRaw(
        execution: GsplatSurfaceProjectedExecution,
        flags: Int,
        visible: Long,
        contributor: Long,
        drawn: Long,
        countsFlags: Int
    ): LongArray = longArrayOf(
        1L,
        11L,
        42L,
        9L,
        2L,
        12.5f.toRawBits().toLong(),
        execution.nativeValue.toLong(),
        GsplatSurfaceOrderBackend.GPU.nativeValue.toLong(),
        flags.toLong(),
        visible,
        contributor,
        drawn,
        countsFlags.toLong()
    )
}
