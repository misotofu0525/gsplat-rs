package com.gsplat.android

/** Projected-draw identity for one successfully rendered frame. */
data class GsplatSurfaceProjectedSubmission(
    val ticket: Long?,
    val cameraRevision: Long,
    val requestedPolicy: GsplatSurfaceProjectedPolicy,
    val actualExecution: GsplatSurfaceProjectedExecution,
    val orderBackend: GsplatSurfaceOrderBackend,
    val adaptiveState: GsplatSurfaceProjectedAdaptiveState,
    val unsampledRingBusy: Boolean,
    val unsampledSurfaceUnavailable: Boolean,
    val flags: Int
) {
    val unsampledReason: GsplatSurfaceProjectedUnsampledReason?
        get() = when {
            unsampledRingBusy -> GsplatSurfaceProjectedUnsampledReason.RING_BUSY
            unsampledSurfaceUnavailable -> GsplatSurfaceProjectedUnsampledReason.SURFACE_UNAVAILABLE
            else -> null
        }

    internal companion object {
        const val RAW_VALUE_COUNT: Int = 7
        private const val TICKET_ISSUED = 1 shl 0
        private const val UNSAMPLED_RING_BUSY = 1 shl 1
        private const val UNSAMPLED_SURFACE_UNAVAILABLE = 1 shl 2

        fun fromRaw(raw: LongArray): GsplatSurfaceProjectedSubmission {
            require(raw.size >= RAW_VALUE_COUNT)
            val flags = raw[6].toInt()
            val ticketIssued = flags and TICKET_ISSUED != 0
            val ticket = raw[0].takeIf { ticketIssued }
            val submission = GsplatSurfaceProjectedSubmission(
                ticket = ticket,
                cameraRevision = raw[1],
                requestedPolicy = GsplatSurfaceProjectedPolicy.fromNative(raw[2].toInt()),
                actualExecution = GsplatSurfaceProjectedExecution.fromNative(raw[3].toInt()),
                orderBackend = GsplatSurfaceOrderBackend.fromNative(raw[4].toInt()),
                adaptiveState = GsplatSurfaceProjectedAdaptiveState.fromNative(raw[5].toInt()),
                unsampledRingBusy = flags and UNSAMPLED_RING_BUSY != 0,
                unsampledSurfaceUnavailable = flags and UNSAMPLED_SURFACE_UNAVAILABLE != 0,
                flags = flags
            )
            check(submission.orderBackend != GsplatSurfaceOrderBackend.ADAPTIVE) {
                "native projected submission exposed Adaptive as an actual order backend"
            }
            check(!(submission.unsampledRingBusy && submission.unsampledSurfaceUnavailable)) {
                "native projected submission reported multiple unsampled reasons"
            }
            if (ticketIssued) {
                check(ticket != null && ticket > 0L && submission.unsampledReason == null) {
                    "native projected submission published an invalid issued ticket"
                }
            } else {
                check(raw[0] == 0L) {
                    "native projected submission exposed a ticket without TICKET_ISSUED"
                }
            }
            if (submission.requestedPolicy != GsplatSurfaceProjectedPolicy.ADAPTIVE) {
                check(submission.ticket == null && submission.unsampledReason == null) {
                    "forced Candidate/Compact must not fabricate a projected ticket"
                }
            } else if (submission.ticket != null) {
                check(submission.unsampledReason == null) {
                    "native projected submission both issued and rejected a ticket"
                }
            }
            return submission
        }
    }
}
