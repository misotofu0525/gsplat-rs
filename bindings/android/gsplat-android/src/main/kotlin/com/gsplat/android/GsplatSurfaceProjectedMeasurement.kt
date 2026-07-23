package com.gsplat.android

/** Terminal projected-draw success, joined atomically by JNI with exact V/C/D counts. */
data class GsplatSurfaceProjectedMeasurement(
    val ticket: Long,
    val cameraRevision: Long,
    val projectionGeneration: Long,
    val probeGeneration: Long,
    val frameCompleteMs: Float,
    val execution: GsplatSurfaceProjectedExecution,
    val orderBackend: GsplatSurfaceOrderBackend,
    val projectionRebuilt: Boolean,
    val orderRefreshed: Boolean,
    val exactContributorCompaction: Boolean,
    val droppedPriorMeasurements: Boolean,
    val visibleCount: Long,
    val contributorCount: Long,
    val drawnCount: Long,
    val flags: Int
) {
    internal companion object {
        const val RAW_VALUE_COUNT: Int = 13
        private const val PROJECTION_REBUILT = 1 shl 0
        private const val ORDER_REFRESHED = 1 shl 1
        private const val EXACT_CONTRIBUTOR_DRAW = 1 shl 2
        private const val DROPPED_PRIOR = 1 shl 3
        private const val COUNTS_EXACT_CONTRIBUTOR_DRAW = 1 shl 0

        fun fromRaw(raw: LongArray): GsplatSurfaceProjectedMeasurement? {
            require(raw.size >= RAW_VALUE_COUNT)
            if (raw[0] == 0L) return null

            val ticket = raw[1]
            val frameCompleteMs = Float.fromBits(raw[5].toInt())
            val execution = GsplatSurfaceProjectedExecution.fromNative(raw[6].toInt())
            val orderBackend = GsplatSurfaceOrderBackend.fromNative(raw[7].toInt())
            val flags = raw[8].toInt()
            val countsFlags = raw[12].toInt()
            val exactContributorCompaction =
                countsFlags and COUNTS_EXACT_CONTRIBUTOR_DRAW != 0
            val visibleCount = raw[9]
            val contributorCount = raw[10]
            val drawnCount = raw[11]

            check(ticket > 0L) { "native projected measurement issued an invalid ticket" }
            check(frameCompleteMs.isFinite() && frameCompleteMs >= 0.0f) {
                "native projected measurement exposed invalid completion time"
            }
            check(orderBackend != GsplatSurfaceOrderBackend.ADAPTIVE) {
                "native projected measurement exposed Adaptive as an actual order backend"
            }
            check(exactContributorCompaction == (flags and EXACT_CONTRIBUTOR_DRAW != 0)) {
                "native projected timing and V/C/D receipts disagree on exact compaction"
            }
            check(contributorCount in 0..visibleCount) {
                "native projected receipt violates 0 <= contributor <= visible"
            }
            when (execution) {
                GsplatSurfaceProjectedExecution.CANDIDATE -> {
                    check(!exactContributorCompaction && drawnCount == visibleCount) {
                        "Candidate projected execution requires D=V without compaction"
                    }
                }
                GsplatSurfaceProjectedExecution.COMPACT -> {
                    check(exactContributorCompaction && drawnCount == contributorCount) {
                        "Compact projected execution requires D=C with exact compaction"
                    }
                }
            }

            return GsplatSurfaceProjectedMeasurement(
                ticket = ticket,
                cameraRevision = raw[2],
                projectionGeneration = raw[3],
                probeGeneration = raw[4],
                frameCompleteMs = frameCompleteMs,
                execution = execution,
                orderBackend = orderBackend,
                projectionRebuilt = flags and PROJECTION_REBUILT != 0,
                orderRefreshed = flags and ORDER_REFRESHED != 0,
                exactContributorCompaction = exactContributorCompaction,
                droppedPriorMeasurements = flags and DROPPED_PRIOR != 0,
                visibleCount = visibleCount,
                contributorCount = contributorCount,
                drawnCount = drawnCount,
                flags = flags
            )
        }
    }
}
