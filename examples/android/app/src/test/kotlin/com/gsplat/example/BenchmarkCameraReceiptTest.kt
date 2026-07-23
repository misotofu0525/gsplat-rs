package com.gsplat.example

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class BenchmarkCameraReceiptTest {
    @Test
    fun rawReceiptPreservesNativeFloatBitsAndPresentedRevision() {
        val raw = validRawReceipt()
        val receipt = BenchmarkCameraReceipt.fromRaw(raw)

        receipt.requirePresented(expectedRevision = 9L)
        assertEquals(2412, receipt.surfaceWidth)
        assertEquals(1080, receipt.surfaceHeight)
        assertArrayEquals(floatArrayOf(1.25f, -0.5f, 3.0f), receipt.position, 0.0f)
        assertArrayEquals(floatArrayOf(0f, 0f, 0f, 1f), receipt.rotationXyzw, 0.0f)
        assertEquals(1.0f, receipt.viewMatrix[0], 0.0f)
        assertEquals(2.0f, receipt.projectionMatrix[0], 0.0f)
        assertEquals(3.0f, receipt.viewProjectionMatrix[0], 0.0f)
    }

    @Test
    fun missingPresentedFlagAndNonFiniteMatrixFailClosed() {
        val stale = validRawReceipt().also { it[4] = 1L }
        assertThrows(IllegalStateException::class.java) {
            BenchmarkCameraReceipt.fromRaw(stale).requirePresented(9L)
        }

        val nonFinite = validRawReceipt().also {
            it[15] = Float.NaN.toRawBits().toLong()
        }
        assertThrows(IllegalStateException::class.java) {
            BenchmarkCameraReceipt.fromRaw(nonFinite)
        }
    }

    private fun validRawReceipt(): LongArray {
        val raw = LongArray(BenchmarkCameraReceipt.RAW_VALUE_COUNT)
        raw[0] = 9L
        raw[1] = 9L
        raw[2] = 2412L
        raw[3] = 1080L
        raw[4] = 3L
        fun put(index: Int, value: Float) {
            raw[index] = value.toRawBits().toLong()
        }
        floatArrayOf(1.25f, -0.5f, 3.0f).forEachIndexed { index, value ->
            put(5 + index, value)
        }
        floatArrayOf(0f, 0f, 0f, 1f).forEachIndexed { index, value ->
            put(8 + index, value)
        }
        put(12, 0.9f)
        put(13, 0.01f)
        put(14, 100f)
        repeat(16) { index ->
            put(15 + index, if (index == 0) 1f else 0f)
            put(31 + index, if (index == 0) 2f else 0f)
            put(47 + index, if (index == 0) 3f else 0f)
        }
        return raw
    }
}
