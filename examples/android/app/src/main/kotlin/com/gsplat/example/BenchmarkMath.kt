package com.gsplat.example

import kotlin.math.ceil

internal object BenchmarkMath {
    fun nearestRank(sorted: LongArray, count: Int, percentile: Double): Long {
        require(count in 1..sorted.size)
        require(percentile > 0.0 && percentile <= 1.0)
        val index = (ceil(percentile * count.toDouble()).toInt() - 1).coerceAtLeast(0)
        return sorted[index]
    }
}
