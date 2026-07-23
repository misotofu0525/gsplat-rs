package com.gsplat.android

enum class GsplatSurfaceAdaptiveGpuFailureReason(internal val nativeValue: Int) {
    UNSUPPORTED(1),
    INITIALIZATION(2),
    OUT_OF_MEMORY(3),
    VALIDATION(4);

    internal companion object {
        fun fromNative(value: Int): GsplatSurfaceAdaptiveGpuFailureReason =
            entries.firstOrNull { it.nativeValue == value }
                ?: error("unsupported native adaptive GPU failure reason: $value")
    }
}

/** Per-frame ordering status, including Adaptive GPU availability. */
data class GsplatSurfaceOrderStatus(
    val cameraRevision: Long,
    val appliedOrderRevision: Long,
    val scheduledRevision: Long?,
    val completedRevision: Long?,
    val presentedOrderRevisionLag: Long,
    val observedResultRevisionLag: Long?,
    val sortRefreshed: Boolean,
    val orderUploaded: Boolean,
    val actualBackend: GsplatSurfaceOrderBackend,
    val gpuSortFallback: Boolean,
    val adaptiveState: GsplatSurfaceAdaptiveState,
    val adaptiveGpuFailure: GsplatSurfaceAdaptiveGpuFailureReason?,
    val measurementTicketIssued: Boolean,
    val flags: Int
) {
    internal companion object {
        const val RAW_VALUE_COUNT: Int = 7

        fun fromRaw(raw: LongArray): GsplatSurfaceOrderStatus {
            require(raw.size >= RAW_VALUE_COUNT)
            val flags = raw[6].toInt()
            val failure = if (flags and (1 shl 15) != 0) {
                GsplatSurfaceAdaptiveGpuFailureReason.fromNative((flags ushr 16) and 0b111)
            } else {
                null
            }
            return GsplatSurfaceOrderStatus(
                cameraRevision = raw[0],
                appliedOrderRevision = raw[1],
                scheduledRevision = raw[2].takeIf { flags and (1 shl 3) != 0 },
                completedRevision = raw[3].takeIf { flags and (1 shl 4) != 0 },
                presentedOrderRevisionLag = raw[4],
                observedResultRevisionLag = raw[5].takeIf { flags and (1 shl 8) != 0 },
                sortRefreshed = flags and 1 != 0,
                orderUploaded = flags and (1 shl 1) != 0,
                actualBackend = GsplatSurfaceOrderBackend.fromNative((flags ushr 9) and 0b11),
                gpuSortFallback = flags and (1 shl 11) != 0,
                adaptiveState = GsplatSurfaceAdaptiveState.fromNative((flags ushr 12) and 0b111),
                adaptiveGpuFailure = failure,
                measurementTicketIssued = flags and (1 shl 19) != 0,
                flags = flags
            )
        }
    }
}
