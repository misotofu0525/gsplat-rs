package com.gsplat.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class GsplatSurfaceGpuProducerReceiptsTest {
    @Test
    fun productDefaultsDoNotEnableProducerDiagnostics() {
        val options = GsplatSurfaceOptions()
        assertNull(options.gpuProducerDiagnostics)
        assertEquals(GsplatSurfaceOrderBackend.ADAPTIVE, options.orderBackend)
        assertEquals(GsplatSurfaceProjectedPolicy.ADAPTIVE, options.projectedPolicy)
    }

    @Test
    fun diagnosticsRequireExactPackedCompactGpuContext() {
        val diagnostics = GsplatSurfaceGpuProducerDiagnostics(
            GsplatSurfaceGpuOrderProducer.PREPROJECT
        )
        val options = GsplatSurfaceOptions(
            orderBackend = GsplatSurfaceOrderBackend.GPU,
            projectedPolicy = GsplatSurfaceProjectedPolicy.COMPACT,
            gpuProducerDiagnostics = diagnostics
        )
        assertEquals(diagnostics, options.gpuProducerDiagnostics)

        assertThrows(IllegalArgumentException::class.java) {
            GsplatSurfaceOptions(gpuProducerDiagnostics = diagnostics)
        }
        assertThrows(IllegalArgumentException::class.java) {
            GsplatSurfaceOptions(
                geometryPath = GsplatGeometryPath.DIRECT,
                orderBackend = GsplatSurfaceOrderBackend.GPU,
                projectedPolicy = GsplatSurfaceProjectedPolicy.COMPACT,
                gpuProducerDiagnostics = diagnostics
            )
        }
        assertThrows(IllegalArgumentException::class.java) {
            GsplatSurfaceOptions(
                sortInterval = 2,
                orderBackend = GsplatSurfaceOrderBackend.GPU,
                projectedPolicy = GsplatSurfaceProjectedPolicy.COMPACT,
                gpuProducerDiagnostics = diagnostics
            )
        }
    }

    @Test
    fun issuedSubmissionKeepsProducerAndStrictExecutionIdentity() {
        val submission = GsplatSurfaceGpuProducerSubmission.fromRaw(
            longArrayOf(
                1L shl 51,
                9L,
                GsplatSurfaceGpuOrderProducer.PREPROJECT.nativeValue.toLong(),
                GsplatSurfaceGpuOrderProducer.PREPROJECT.nativeValue.toLong(),
                GsplatSurfaceOrderBackend.GPU.nativeValue.toLong(),
                GsplatSurfaceProjectedExecution.COMPACT.nativeValue.toLong(),
                0b1001L
            )
        )
        assertEquals(1L shl 51, submission.ticket)
        assertEquals(GsplatSurfaceGpuOrderProducer.PREPROJECT, submission.actualProducer)
        assertTrue(submission.measurementEnabled)
        assertNull(submission.unsampledReason)
    }

    @Test
    fun exactMeasurementRequiresRefreshedDrawnEqualsContributor() {
        val receipt = GsplatSurfaceGpuProducerMeasurement.fromRaw(
            measurementRaw(flags = 0b0011, scope = 1, contributor = 55, drawn = 55)
        )!!
        assertTrue(receipt.exactCurrentContributorDraw)
        assertFalse(receipt.staleOrder)
        assertEquals(100L, receipt.sourceCount)
        assertEquals(55L, receipt.drawnCount)

        assertThrows(IllegalStateException::class.java) {
            GsplatSurfaceGpuProducerMeasurement.fromRaw(
                measurementRaw(flags = 0b0011, scope = 1, contributor = 55, drawn = 56)
            )
        }
    }

    @Test
    fun staleAndFailureReceiptsStayExplicit() {
        val stale = GsplatSurfaceGpuProducerMeasurement.fromRaw(
            measurementRaw(flags = 0b0100, scope = 2, contributor = 55, drawn = 80)
        )!!
        assertTrue(stale.staleOrder)
        assertFalse(stale.exactCurrentContributorDraw)

        val failure = GsplatSurfaceGpuProducerMeasurementFailure.fromRaw(
            longArrayOf(
                1L,
                (1L shl 51) + 1,
                9L,
                4L,
                7L,
                GsplatSurfaceGpuProducerMeasurementFailureReason.INVARIANT_VIOLATION
                    .nativeValue.toLong(),
                GsplatSurfaceGpuOrderProducer.POST_SORT.nativeValue.toLong(),
                1L
            )
        )!!
        assertTrue(failure.droppedPriorFailures)
    }

    private fun measurementRaw(
        flags: Int,
        scope: Int,
        contributor: Long,
        drawn: Long
    ): LongArray = longArrayOf(
        1L,
        1L shl 51,
        9L,
        4L,
        7L,
        12.5f.toRawBits().toLong(),
        GsplatSurfaceGpuOrderProducer.POST_SORT.nativeValue.toLong(),
        100L,
        contributor,
        drawn,
        scope.toLong(),
        flags.toLong()
    )
}
