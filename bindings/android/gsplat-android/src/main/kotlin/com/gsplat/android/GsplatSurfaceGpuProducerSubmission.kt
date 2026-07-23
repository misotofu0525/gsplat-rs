package com.gsplat.android

/** Producer identity for the last successful Surface render call. */
data class GsplatSurfaceGpuProducerSubmission(
    val ticket: Long?,
    val cameraRevision: Long,
    val requestedProducer: GsplatSurfaceGpuOrderProducer,
    val actualProducer: GsplatSurfaceGpuOrderProducer?,
    val orderBackend: GsplatSurfaceOrderBackend,
    val projectedExecution: GsplatSurfaceProjectedExecution,
    val measurementEnabled: Boolean,
    val unsampledReason: GsplatSurfaceGpuProducerUnsampledReason?,
    val flags: Int
) {
    internal companion object {
        const val RAW_VALUE_COUNT: Int = 7
        private const val TICKET_ISSUED = 1 shl 0
        private const val UNSAMPLED_RING_BUSY = 1 shl 1
        private const val UNSAMPLED_SURFACE_UNAVAILABLE = 1 shl 2
        private const val MEASUREMENT_ENABLED = 1 shl 3
        private const val KNOWN_FLAGS = TICKET_ISSUED or UNSAMPLED_RING_BUSY or
            UNSAMPLED_SURFACE_UNAVAILABLE or MEASUREMENT_ENABLED

        fun fromRaw(raw: LongArray): GsplatSurfaceGpuProducerSubmission {
            require(raw.size >= RAW_VALUE_COUNT)
            val flags = raw[6].toInt()
            check(flags and KNOWN_FLAGS.inv() == 0) {
                "native GPU producer submission exposed unknown flags"
            }
            val issued = flags and TICKET_ISSUED != 0
            val ringBusy = flags and UNSAMPLED_RING_BUSY != 0
            val surfaceUnavailable = flags and UNSAMPLED_SURFACE_UNAVAILABLE != 0
            check(!(ringBusy && surfaceUnavailable)) {
                "native GPU producer submission reported multiple unsampled reasons"
            }
            val ticket = raw[0].takeIf { issued }
            if (issued) {
                check(ticket != null && ticket > 0L && !ringBusy && !surfaceUnavailable) {
                    "native GPU producer submission published an invalid ticket"
                }
            } else {
                check(raw[0] == 0L) {
                    "native GPU producer submission exposed a ticket without TICKET_ISSUED"
                }
            }
            val measurementEnabled = flags and MEASUREMENT_ENABLED != 0
            check(measurementEnabled || (!issued && !ringBusy && !surfaceUnavailable)) {
                "disabled GPU producer telemetry exposed measurement state"
            }
            return GsplatSurfaceGpuProducerSubmission(
                ticket = ticket,
                cameraRevision = raw[1],
                requestedProducer = GsplatSurfaceGpuOrderProducer.fromNative(raw[2].toInt()),
                actualProducer = raw[3].toInt().takeIf { it != 0 }
                    ?.let(GsplatSurfaceGpuOrderProducer::fromNative),
                orderBackend = GsplatSurfaceOrderBackend.fromNative(raw[4].toInt()),
                projectedExecution = GsplatSurfaceProjectedExecution.fromNative(raw[5].toInt()),
                measurementEnabled = measurementEnabled,
                unsampledReason = when {
                    ringBusy -> GsplatSurfaceGpuProducerUnsampledReason.RING_BUSY
                    surfaceUnavailable -> GsplatSurfaceGpuProducerUnsampledReason.SURFACE_UNAVAILABLE
                    else -> null
                },
                flags = flags
            )
        }
    }
}
