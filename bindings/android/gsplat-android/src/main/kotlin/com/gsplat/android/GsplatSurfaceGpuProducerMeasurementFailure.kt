package com.gsplat.android

enum class GsplatSurfaceGpuProducerMeasurementFailureReason(internal val nativeValue: Int) {
    READBACK_MAP(1),
    GENERATION_INVALIDATED(2),
    INVARIANT_VIOLATION(3);

    internal companion object {
        fun fromNative(value: Int): GsplatSurfaceGpuProducerMeasurementFailureReason =
            entries.firstOrNull { it.nativeValue == value }
                ?: error("unsupported native Surface GPU producer failure reason: $value")
    }
}

/** Terminal failure for one issued producer measurement ticket. */
data class GsplatSurfaceGpuProducerMeasurementFailure(
    val ticket: Long,
    val cameraRevision: Long,
    val orderGeneration: Long,
    val projectionGeneration: Long,
    val reason: GsplatSurfaceGpuProducerMeasurementFailureReason,
    val producer: GsplatSurfaceGpuOrderProducer,
    val droppedPriorFailures: Boolean,
    val flags: Int
) {
    internal companion object {
        const val RAW_VALUE_COUNT: Int = 8
        private const val DROPPED_PRIOR = 1 shl 0

        fun fromRaw(raw: LongArray): GsplatSurfaceGpuProducerMeasurementFailure? {
            require(raw.size >= RAW_VALUE_COUNT)
            if (raw[0] == 0L) return null
            val flags = raw[7].toInt()
            check(flags and DROPPED_PRIOR.inv() == 0) {
                "native GPU producer failure exposed unknown flags"
            }
            check(raw[1] > 0L) { "native GPU producer failure issued an invalid ticket" }
            return GsplatSurfaceGpuProducerMeasurementFailure(
                ticket = raw[1],
                cameraRevision = raw[2],
                orderGeneration = raw[3],
                projectionGeneration = raw[4],
                reason = GsplatSurfaceGpuProducerMeasurementFailureReason.fromNative(
                    raw[5].toInt()
                ),
                producer = GsplatSurfaceGpuOrderProducer.fromNative(raw[6].toInt()),
                droppedPriorFailures = flags and DROPPED_PRIOR != 0,
                flags = flags
            )
        }
    }
}
