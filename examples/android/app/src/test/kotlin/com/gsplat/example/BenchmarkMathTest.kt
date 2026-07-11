package com.gsplat.example

import org.junit.Assert.assertEquals
import org.junit.Test

class BenchmarkMathTest {
    @Test
    fun nearestRankUsesCeilingAndOneBasedRank() {
        val sorted = longArrayOf(1, 2, 3, 4, 5)

        assertEquals(3L, BenchmarkMath.nearestRank(sorted, sorted.size, 0.50))
        assertEquals(5L, BenchmarkMath.nearestRank(sorted, sorted.size, 0.90))
        assertEquals(5L, BenchmarkMath.nearestRank(sorted, sorted.size, 0.95))
        assertEquals(5L, BenchmarkMath.nearestRank(sorted, sorted.size, 0.99))
    }

    @Test
    fun nearestRankHandlesSingleSample() {
        assertEquals(42L, BenchmarkMath.nearestRank(longArrayOf(42), 1, 0.99))
    }
}
