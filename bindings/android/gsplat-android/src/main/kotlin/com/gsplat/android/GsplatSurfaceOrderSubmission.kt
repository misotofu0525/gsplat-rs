package com.gsplat.android

enum class GsplatSurfaceOrderUnsampledReason {
    RING_BUSY,
    SURFACE_UNAVAILABLE
}

/** CPU/GPU measurement submission identity for one successfully rendered frame. */
data class GsplatSurfaceOrderSubmission(
    val ticket: Long?,
    val cameraRevision: Long,
    val requestedBackend: GsplatSurfaceOrderBackend,
    val actualBackend: GsplatSurfaceOrderBackend,
    val adaptiveState: GsplatSurfaceAdaptiveState,
    val gpuRefresh: Boolean,
    val cpuFrameCompletionSample: Boolean,
    val unsampledRingBusy: Boolean,
    val unsampledSurfaceUnavailable: Boolean,
    val flags: Int
) {
    val measurementBackend: GsplatSurfaceOrderBackend?
        get() = when {
            gpuRefresh -> GsplatSurfaceOrderBackend.GPU
            cpuFrameCompletionSample -> GsplatSurfaceOrderBackend.CPU
            else -> null
        }

    val unsampledReason: GsplatSurfaceOrderUnsampledReason?
        get() = when {
            unsampledRingBusy -> GsplatSurfaceOrderUnsampledReason.RING_BUSY
            unsampledSurfaceUnavailable -> GsplatSurfaceOrderUnsampledReason.SURFACE_UNAVAILABLE
            else -> null
        }

    internal companion object {
        const val RAW_VALUE_COUNT: Int = 6
        private const val GPU_REFRESH = 1 shl 0
        private const val TICKET_ISSUED = 1 shl 1
        private const val UNSAMPLED_RING_BUSY = 1 shl 2
        private const val CPU_FRAME_COMPLETION_SAMPLE = 1 shl 3
        private const val UNSAMPLED_SURFACE_UNAVAILABLE = 1 shl 4

        fun fromRaw(raw: LongArray): GsplatSurfaceOrderSubmission {
            require(raw.size >= RAW_VALUE_COUNT)
            val flags = raw[5].toInt()
            val ticket = raw[0].takeIf { flags and TICKET_ISSUED != 0 }
            check(ticket == null || ticket > 0L) { "native issued an invalid order ticket" }
            val submission = GsplatSurfaceOrderSubmission(
                ticket = ticket,
                cameraRevision = raw[1],
                requestedBackend = GsplatSurfaceOrderBackend.fromNative(raw[2].toInt()),
                actualBackend = GsplatSurfaceOrderBackend.fromNative(raw[3].toInt()),
                adaptiveState = GsplatSurfaceAdaptiveState.fromNative(raw[4].toInt()),
                gpuRefresh = flags and GPU_REFRESH != 0,
                cpuFrameCompletionSample = flags and CPU_FRAME_COMPLETION_SAMPLE != 0,
                unsampledRingBusy = flags and UNSAMPLED_RING_BUSY != 0,
                unsampledSurfaceUnavailable = flags and UNSAMPLED_SURFACE_UNAVAILABLE != 0,
                flags = flags
            )
            check(!(submission.gpuRefresh && submission.cpuFrameCompletionSample)) {
                "native order submission requested both CPU and GPU measurement"
            }
            check(!(submission.unsampledRingBusy && submission.unsampledSurfaceUnavailable)) {
                "native order submission reported multiple unsampled reasons"
            }
            if (submission.measurementBackend == null) {
                check(submission.ticket == null && submission.unsampledReason == null) {
                    "native order submission published state without a request"
                }
            } else if (submission.ticket == null) {
                check(submission.unsampledReason != null) {
                    "native order measurement request has neither ticket nor unsampled reason"
                }
            } else {
                check(submission.unsampledReason == null) {
                    "native order submission both issued and rejected a ticket"
                }
            }
            return submission
        }
    }
}
