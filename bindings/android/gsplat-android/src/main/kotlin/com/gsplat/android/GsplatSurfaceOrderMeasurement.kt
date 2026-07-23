package com.gsplat.android

enum class GsplatSurfaceTimingSource(internal val nativeValue: Int) {
    GPU_TIMESTAMP_QUERY(1),
    GPU_COMPLETION(2);

    internal companion object {
        fun fromNative(value: Int): GsplatSurfaceTimingSource =
            entries.firstOrNull { it.nativeValue == value }
                ?: error("unsupported native Surface timing source: $value")
    }
}

enum class GsplatSurfaceAdaptiveState(internal val nativeValue: Int) {
    DISABLED(0),
    CPU_LEARNING(1),
    CPU_STABLE(2),
    GPU_PROBE(3),
    GPU_STABLE(4),
    CPU_PROBE(5),
    COOLDOWN(6);

    internal companion object {
        fun fromNative(value: Int): GsplatSurfaceAdaptiveState =
            entries.firstOrNull { it.nativeValue == value }
                ?: error("unsupported native Surface adaptive state: $value")
    }
}

data class GsplatSurfaceOrderMeasurement(
    val ticket: Long,
    val cameraRevision: Long,
    val timingSource: GsplatSurfaceTimingSource,
    val requestedBackend: GsplatSurfaceOrderBackend,
    val actualBackend: GsplatSurfaceOrderBackend,
    val adaptiveState: GsplatSurfaceAdaptiveState,
    val gpuPreprocessMs: Float?,
    val gpuRadixMs: Float?,
    val gpuOrderMs: Float?,
    val gpuCompleteMs: Float,
    val timestampPeriodNs: Float?,
    val belowTimestampResolution: Boolean,
    /** True when this bounded queue receipt follows at least one dropped older receipt. */
    val droppedPriorMeasurements: Boolean,
    val visibleCount: Long,
    /** Exact post-projection contributor count C for this ticket and camera revision. */
    val contributorCount: Long,
    val drawnCount: Long,
    val exactContributorCompaction: Boolean,
    val flags: Int
) {
    internal companion object {
        const val RAW_VALUE_COUNT: Int = 19

        private const val PREPROCESS_VALID = 1 shl 0
        private const val RADIX_VALID = 1 shl 1
        private const val ORDER_VALID = 1 shl 2
        private const val TIMESTAMP_PERIOD_VALID = 1 shl 3
        private const val BELOW_TIMESTAMP_RESOLUTION = 1 shl 4
        private const val DROPPED_PRIOR = 1 shl 5
        private const val EXACT_CONTRIBUTOR_DRAW = 1 shl 6
        private const val COUNTS_EXACT_CONTRIBUTOR_DRAW = 1 shl 0

        fun fromRaw(raw: LongArray): GsplatSurfaceOrderMeasurement? {
            require(raw.size >= RAW_VALUE_COUNT)
            if (raw[0] == 0L) return null

            val flags = raw[14].toInt()
            val countsFlags = raw[18].toInt()
            val exactContributorCompaction =
                countsFlags and COUNTS_EXACT_CONTRIBUTOR_DRAW != 0
            check(exactContributorCompaction == (flags and EXACT_CONTRIBUTOR_DRAW != 0)) {
                "native GPU order receipt and V/C/D receipt disagree on exact compaction"
            }
            check(raw[8] == raw[15] && raw[9] == raw[17]) {
                "native GPU order receipt and V/C/D receipt disagree on visible/drawn counts"
            }
            check(raw[16] in 0..raw[15]) {
                "native GPU order receipt violates 0 <= contributor <= visible"
            }
            check(
                if (exactContributorCompaction) raw[17] == raw[16]
                else raw[17] == raw[15]
            ) {
                "native GPU order receipt violates issued-count semantics"
            }
            fun optionalFloat(index: Int, validBit: Int): Float? =
                if ((flags and validBit) != 0) Float.fromBits(raw[index].toInt()) else null

            return GsplatSurfaceOrderMeasurement(
                ticket = raw[1],
                cameraRevision = raw[2],
                gpuPreprocessMs = optionalFloat(3, PREPROCESS_VALID),
                gpuRadixMs = optionalFloat(4, RADIX_VALID),
                gpuOrderMs = optionalFloat(5, ORDER_VALID),
                gpuCompleteMs = Float.fromBits(raw[6].toInt()),
                timestampPeriodNs = optionalFloat(7, TIMESTAMP_PERIOD_VALID),
                visibleCount = raw[15],
                contributorCount = raw[16],
                drawnCount = raw[17],
                exactContributorCompaction = exactContributorCompaction,
                timingSource = GsplatSurfaceTimingSource.fromNative(raw[10].toInt()),
                requestedBackend = GsplatSurfaceOrderBackend.fromNative(raw[11].toInt()),
                actualBackend = GsplatSurfaceOrderBackend.fromNative(raw[12].toInt()),
                adaptiveState = GsplatSurfaceAdaptiveState.fromNative(raw[13].toInt()),
                belowTimestampResolution = (flags and BELOW_TIMESTAMP_RESOLUTION) != 0,
                droppedPriorMeasurements = (flags and DROPPED_PRIOR) != 0,
                flags = flags
            )
        }
    }
}
