package com.gsplat.android

data class GsplatSurfaceStats(
    val visibleCount: Long,
    val drawnCount: Long,
    val frameMs: Float,
    val preprocessMs: Float,
    val sortMs: Float,
    val rasterMs: Float
) {
    companion object {
        internal fun fromRaw(raw: LongArray): GsplatSurfaceStats =
            GsplatSurfaceStats(
                visibleCount = raw[0],
                drawnCount = raw[1],
                frameMs = raw[2] / 1000.0f,
                preprocessMs = raw[3] / 1000.0f,
                sortMs = raw[4] / 1000.0f,
                rasterMs = raw[5] / 1000.0f
            )
    }
}
