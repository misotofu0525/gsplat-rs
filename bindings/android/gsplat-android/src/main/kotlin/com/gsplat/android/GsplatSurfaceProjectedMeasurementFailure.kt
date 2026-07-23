package com.gsplat.android

enum class GsplatSurfaceProjectedMeasurementFailureReason(internal val nativeValue: Int) {
    READBACK_MAP(1),
    GENERATION_INVALIDATED(2),
    INVARIANT_VIOLATION(3);

    internal companion object {
        fun fromNative(value: Int): GsplatSurfaceProjectedMeasurementFailureReason =
            entries.firstOrNull { it.nativeValue == value }
                ?: error("unsupported native Surface projected failure reason: $value")
    }
}

/** Terminal failure for one projected-draw ticket issued by Adaptive probing. */
data class GsplatSurfaceProjectedMeasurementFailure(
    val ticket: Long,
    val cameraRevision: Long,
    val projectionGeneration: Long,
    val probeGeneration: Long,
    val reason: GsplatSurfaceProjectedMeasurementFailureReason,
    val execution: GsplatSurfaceProjectedExecution,
    val orderBackend: GsplatSurfaceOrderBackend,
    val droppedPriorFailures: Boolean,
    val flags: Int
) {
    internal companion object {
        const val RAW_VALUE_COUNT: Int = 9
        private const val DROPPED_PRIOR = 1 shl 0

        fun fromRaw(raw: LongArray): GsplatSurfaceProjectedMeasurementFailure? {
            require(raw.size >= RAW_VALUE_COUNT)
            if (raw[0] == 0L) return null
            val ticket = raw[1]
            val orderBackend = GsplatSurfaceOrderBackend.fromNative(raw[7].toInt())
            val flags = raw[8].toInt()
            check(ticket > 0L) { "native projected failure issued an invalid ticket" }
            check(orderBackend != GsplatSurfaceOrderBackend.ADAPTIVE) {
                "native projected failure exposed Adaptive as an actual order backend"
            }
            return GsplatSurfaceProjectedMeasurementFailure(
                ticket = ticket,
                cameraRevision = raw[2],
                projectionGeneration = raw[3],
                probeGeneration = raw[4],
                reason = GsplatSurfaceProjectedMeasurementFailureReason.fromNative(raw[5].toInt()),
                execution = GsplatSurfaceProjectedExecution.fromNative(raw[6].toInt()),
                orderBackend = orderBackend,
                droppedPriorFailures = flags and DROPPED_PRIOR != 0,
                flags = flags
            )
        }
    }
}
