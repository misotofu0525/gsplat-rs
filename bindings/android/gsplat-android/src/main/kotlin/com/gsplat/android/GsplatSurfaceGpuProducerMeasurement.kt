package com.gsplat.android

/** Terminal producer success with exact source/contributor/drawn (S/C/D). */
data class GsplatSurfaceGpuProducerMeasurement(
    val ticket: Long,
    val cameraRevision: Long,
    val orderGeneration: Long,
    val projectionGeneration: Long,
    val frameCompleteMs: Float,
    val producer: GsplatSurfaceGpuOrderProducer,
    val sourceCount: Long,
    val contributorCount: Long,
    val drawnCount: Long,
    val drawScope: GsplatSurfaceGpuProducerDrawScope,
    val orderRefreshed: Boolean,
    val droppedPriorMeasurements: Boolean,
    val flags: Int
) {
    val exactCurrentContributorDraw: Boolean
        get() = drawScope == GsplatSurfaceGpuProducerDrawScope.EXACT_CURRENT_CONTRIBUTORS

    val staleOrder: Boolean
        get() = drawScope == GsplatSurfaceGpuProducerDrawScope.STALE_ORDER_CANDIDATES

    internal companion object {
        const val RAW_VALUE_COUNT: Int = 12
        private const val ORDER_REFRESHED = 1 shl 0
        private const val EXACT_CURRENT_DRAW = 1 shl 1
        private const val STALE_ORDER = 1 shl 2
        private const val DROPPED_PRIOR = 1 shl 3
        private const val KNOWN_FLAGS = ORDER_REFRESHED or EXACT_CURRENT_DRAW or
            STALE_ORDER or DROPPED_PRIOR

        fun fromRaw(raw: LongArray): GsplatSurfaceGpuProducerMeasurement? {
            require(raw.size >= RAW_VALUE_COUNT)
            if (raw[0] == 0L) return null
            val ticket = raw[1]
            val frameCompleteMs = Float.fromBits(raw[5].toInt())
            val source = raw[7]
            val contributor = raw[8]
            val drawn = raw[9]
            val scope = GsplatSurfaceGpuProducerDrawScope.fromNative(raw[10].toInt())
            val flags = raw[11].toInt()
            check(flags and KNOWN_FLAGS.inv() == 0) {
                "native GPU producer measurement exposed unknown flags"
            }
            val refreshed = flags and ORDER_REFRESHED != 0
            val exact = flags and EXACT_CURRENT_DRAW != 0
            val stale = flags and STALE_ORDER != 0
            check(ticket > 0L) { "native GPU producer measurement issued an invalid ticket" }
            check(frameCompleteMs.isFinite() && frameCompleteMs >= 0.0f) {
                "native GPU producer measurement exposed invalid completion time"
            }
            check(source >= 0L && contributor in 0L..source && drawn in 0L..source) {
                "native GPU producer receipt violates exact S/C/D bounds"
            }
            when (scope) {
                GsplatSurfaceGpuProducerDrawScope.EXACT_CURRENT_CONTRIBUTORS -> check(
                    exact && !stale && refreshed && drawn == contributor
                ) { "exact producer draw requires refreshed D=C" }
                GsplatSurfaceGpuProducerDrawScope.STALE_ORDER_CANDIDATES -> check(
                    !exact && stale
                ) { "stale producer draw must be labelled stale" }
            }
            return GsplatSurfaceGpuProducerMeasurement(
                ticket = ticket,
                cameraRevision = raw[2],
                orderGeneration = raw[3],
                projectionGeneration = raw[4],
                frameCompleteMs = frameCompleteMs,
                producer = GsplatSurfaceGpuOrderProducer.fromNative(raw[6].toInt()),
                sourceCount = source,
                contributorCount = contributor,
                drawnCount = drawn,
                drawScope = scope,
                orderRefreshed = refreshed,
                droppedPriorMeasurements = flags and DROPPED_PRIOR != 0,
                flags = flags
            )
        }
    }
}
