package com.gsplat.android

enum class GsplatSurfaceOrderMeasurementFailureReason(internal val nativeValue: Int) {
    READBACK_MAP(1),
    GENERATION_INVALIDATED(2);

    internal companion object {
        fun fromNative(value: Int): GsplatSurfaceOrderMeasurementFailureReason =
            entries.firstOrNull { it.nativeValue == value }
                ?: error("unsupported native Surface order failure reason: $value")
    }
}

/** Terminal failure for one CPU or GPU measurement ticket that native code issued. */
data class GsplatSurfaceOrderMeasurementFailure(
    val ticket: Long,
    val cameraRevision: Long,
    val reason: GsplatSurfaceOrderMeasurementFailureReason,
    val requestedBackend: GsplatSurfaceOrderBackend,
    val actualBackend: GsplatSurfaceOrderBackend,
    val adaptiveState: GsplatSurfaceAdaptiveState,
    /** True when this bounded queue receipt follows at least one dropped failure. */
    val droppedPriorFailures: Boolean,
    val flags: Int
) {
    internal companion object {
        const val RAW_VALUE_COUNT: Int = 8
        private const val DROPPED_PRIOR = 1 shl 0

        fun fromRaw(raw: LongArray): GsplatSurfaceOrderMeasurementFailure? {
            require(raw.size >= RAW_VALUE_COUNT)
            if (raw[0] == 0L) return null
            val flags = raw[7].toInt()
            return GsplatSurfaceOrderMeasurementFailure(
                ticket = raw[1],
                cameraRevision = raw[2],
                reason = GsplatSurfaceOrderMeasurementFailureReason.fromNative(raw[3].toInt()),
                requestedBackend = GsplatSurfaceOrderBackend.fromNative(raw[4].toInt()),
                actualBackend = GsplatSurfaceOrderBackend.fromNative(raw[5].toInt()),
                adaptiveState = GsplatSurfaceAdaptiveState.fromNative(raw[6].toInt()),
                droppedPriorFailures = flags and DROPPED_PRIOR != 0,
                flags = flags
            )
        }
    }
}
