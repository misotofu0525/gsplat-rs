package com.gsplat.android

/**
 * Queue-completion receipt for one formally sampled CPU order refresh.
 *
 * [frameCompleteMs] covers frame start through graphics-queue completion, so
 * Adaptive can compare it with GPU completion timing without substituting CPU
 * submit-wall time.
 */
data class GsplatSurfaceCpuOrderMeasurement(
    val ticket: Long,
    val cameraRevision: Long,
    val preprocessMs: Float,
    val sortMs: Float,
    val frameCompleteMs: Float,
    val requestedBackend: GsplatSurfaceOrderBackend,
    val actualBackend: GsplatSurfaceOrderBackend,
    val adaptiveState: GsplatSurfaceAdaptiveState,
    /** Near/far candidate count V for this ticket and camera revision. */
    val visibleCount: Long,
    /** Exact post-projection contributor count C for this ticket and camera revision. */
    val contributorCount: Long,
    /** Actual issued/drawn count D for this ticket and camera revision. */
    val drawnCount: Long,
    /** True only when the issued draw count D is exactly the contributor count C. */
    val exactContributorCompaction: Boolean,
    /** True when this bounded queue receipt follows at least one dropped older receipt. */
    val droppedPriorMeasurements: Boolean,
    val flags: Int
) {
    internal companion object {
        const val RAW_VALUE_COUNT: Int = 15
        private const val DROPPED_PRIOR = 1 shl 0
        private const val EXACT_CONTRIBUTOR_DRAW = 1 shl 1
        private const val CONTRIBUTOR_COUNT_VALID = 1 shl 2
        private const val COUNTS_EXACT_CONTRIBUTOR_DRAW = 1 shl 0

        fun fromRaw(raw: LongArray): GsplatSurfaceCpuOrderMeasurement? {
            require(raw.size >= RAW_VALUE_COUNT)
            if (raw[0] == 0L) return null

            val actualBackend = GsplatSurfaceOrderBackend.fromNative(raw[7].toInt())
            check(actualBackend == GsplatSurfaceOrderBackend.CPU) {
                "native CPU order receipt reports $actualBackend"
            }
            val flags = raw[9].toInt()
            val countsFlags = raw[14].toInt()
            val exactContributorCompaction =
                countsFlags and COUNTS_EXACT_CONTRIBUTOR_DRAW != 0
            check(exactContributorCompaction == (flags and EXACT_CONTRIBUTOR_DRAW != 0)) {
                "native CPU order receipt and V/C/D receipt disagree on exact compaction"
            }
            check(flags and CONTRIBUTOR_COUNT_VALID != 0 && raw[10] == raw[12]) {
                "native CPU compatibility receipt disagrees with V/C/D contributor count"
            }
            check(raw[12] in 0..raw[11]) {
                "native CPU order receipt violates 0 <= contributor <= visible"
            }
            check(
                if (exactContributorCompaction) raw[13] == raw[12]
                else raw[13] == raw[11]
            ) {
                "native CPU order receipt violates issued-count semantics"
            }
            return GsplatSurfaceCpuOrderMeasurement(
                ticket = raw[1],
                cameraRevision = raw[2],
                preprocessMs = Float.fromBits(raw[3].toInt()),
                sortMs = Float.fromBits(raw[4].toInt()),
                frameCompleteMs = Float.fromBits(raw[5].toInt()),
                requestedBackend = GsplatSurfaceOrderBackend.fromNative(raw[6].toInt()),
                actualBackend = actualBackend,
                adaptiveState = GsplatSurfaceAdaptiveState.fromNative(raw[8].toInt()),
                visibleCount = raw[11],
                contributorCount = raw[12],
                drawnCount = raw[13],
                exactContributorCompaction = exactContributorCompaction,
                droppedPriorMeasurements = flags and DROPPED_PRIOR != 0,
                flags = flags
            )
        }
    }
}
