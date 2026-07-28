package com.gsplat.android

/** Same-ticket timing carried by a current-stats V2 Ready terminal. */
data class GsplatSurfaceCurrentStatsTimingV2(
    val frameCompleteMs: Float,
    val cpuPreprocessMs: Float?,
    val cpuSortMs: Float?
) {
    init {
        check(frameCompleteMs.isFinite() && frameCompleteMs >= 0f)
        check(cpuPreprocessMs == null || cpuPreprocessMs.isFinite() && cpuPreprocessMs >= 0f)
        check(cpuSortMs == null || cpuSortMs.isFinite() && cpuSortMs >= 0f)
    }
}

/** Atomic V2 Ready receipt: one ticket, identity, S/V/C/D, semantics, and timing. */
data class GsplatSurfaceCurrentStatsReceiptV2(
    val ticket: Long,
    val identity: GsplatSurfaceCurrentStatsIdentity,
    val sourceCount: Long,
    val visibleCount: Long,
    val contributorCount: Long,
    val drawnCount: Long,
    val countSemantics: GsplatSurfaceCurrentStatsCountSemantics,
    val timing: GsplatSurfaceCurrentStatsTimingV2
) {
    init {
        check(ticket != 0L) { "current-stats V2 Ready ticket must be non-zero" }
        check(sourceCount >= 0L && visibleCount in 0L..sourceCount) {
            "current-stats V2 Ready violates 0 <= V <= S"
        }
        check(contributorCount in 0L..visibleCount && drawnCount in 0L..sourceCount) {
            "current-stats V2 Ready violates 0 <= C <= V <= S or D <= S"
        }
        when (countSemantics) {
            GsplatSurfaceCurrentStatsCountSemantics.DIRECT_DRAW_EQUALS_VISIBLE,
            GsplatSurfaceCurrentStatsCountSemantics.INDIRECT_DRAW_EQUALS_VISIBLE ->
                check(drawnCount == visibleCount) {
                    "current-stats V2 visible-draw semantics require D=V"
                }
            GsplatSurfaceCurrentStatsCountSemantics.INDIRECT_DRAW_EQUALS_CONTRIBUTOR ->
                check(drawnCount == contributorCount) {
                    "current-stats V2 contributor-draw semantics require D=C"
                }
        }
    }

    /** Count/identity projection used by the existing presentation-safe adapter. */
    fun withoutTiming() = GsplatSurfaceCurrentStatsReceipt(
        ticket = ticket,
        identity = identity,
        sourceCount = sourceCount,
        visibleCount = visibleCount,
        contributorCount = contributorCount,
        drawnCount = drawnCount,
        countSemantics = countSemantics
    )
}

/** One destructive value from the Renderer-owned current-stats single-pop queue. */
data class GsplatSurfaceCurrentStatsPollV2(
    val kind: GsplatSurfaceCurrentStatsPollKind,
    val requestStatus: GsplatSurfaceCurrentStatsRequestStatus? = null,
    val receipt: GsplatSurfaceCurrentStatsReceiptV2? = null,
    val failure: GsplatSurfaceCurrentStatsFailure? = null
) {
    init {
        when (kind) {
            GsplatSurfaceCurrentStatsPollKind.EMPTY ->
                check(requestStatus == null && receipt == null && failure == null)
            GsplatSurfaceCurrentStatsPollKind.UNSAMPLED ->
                check(
                    requestStatus != null &&
                        requestStatus != GsplatSurfaceCurrentStatsRequestStatus.REQUESTED &&
                        receipt == null && failure == null
                )
            GsplatSurfaceCurrentStatsPollKind.READY ->
                check(requestStatus == null && receipt != null && failure == null)
            else -> check(
                kind.isTerminalFailure && requestStatus == null && receipt == null &&
                    failure != null
            )
        }
    }

    fun withoutTiming(): GsplatSurfaceCurrentStatsPoll = when (kind) {
        GsplatSurfaceCurrentStatsPollKind.EMPTY -> GsplatSurfaceCurrentStatsPoll(kind)
        GsplatSurfaceCurrentStatsPollKind.UNSAMPLED ->
            GsplatSurfaceCurrentStatsPoll(kind = kind, requestStatus = requestStatus)
        GsplatSurfaceCurrentStatsPollKind.READY -> {
            val ready = checkNotNull(receipt)
            GsplatSurfaceCurrentStatsPoll(
                kind = kind,
                receipt = ready.withoutTiming()
            )
        }
        else -> GsplatSurfaceCurrentStatsPoll(kind = kind, failure = failure)
    }

    companion object {
        internal const val RAW_VALUE_COUNT = 22
        private const val FRAME_COMPLETE_VALID = 1
        private const val CPU_PREPROCESS_VALID = 1 shl 1
        private const val CPU_SORT_VALID = 1 shl 2
        private const val KNOWN_TIMING_FLAGS =
            FRAME_COMPLETE_VALID or CPU_PREPROCESS_VALID or CPU_SORT_VALID

        internal fun fromRaw(raw: LongArray): GsplatSurfaceCurrentStatsPollV2 {
            require(raw.size >= RAW_VALUE_COUNT)
            val kind = GsplatSurfaceCurrentStatsPollKind.fromNative(raw[0])
            return when (kind) {
                GsplatSurfaceCurrentStatsPollKind.EMPTY -> {
                    requireInapplicablePayload(raw)
                    GsplatSurfaceCurrentStatsPollV2(kind)
                }
                GsplatSurfaceCurrentStatsPollKind.UNSAMPLED -> {
                    check(raw[2] == 0L && raw[3] == 0L && raw[4] == 0L)
                    check(GsplatSurfaceCurrentStatsIdentity.rawPayloadIsZero(raw, 5))
                    check(raw.sliceArray(15 until RAW_VALUE_COUNT).all { it == 0L })
                    GsplatSurfaceCurrentStatsPollV2(
                        kind = kind,
                        requestStatus = GsplatSurfaceCurrentStatsRequestStatus.fromNative(raw[1])
                    )
                }
                GsplatSurfaceCurrentStatsPollKind.READY -> readyFromRaw(raw)
                else -> {
                    check(raw[1] == 0L && raw[2] == 0L && raw[3] == 0L && raw[4] != 0L)
                    check(raw.sliceArray(15 until RAW_VALUE_COUNT).all { it == 0L })
                    val identity = GsplatSurfaceCurrentStatsIdentity.fromRaw(raw, 5)
                    GsplatSurfaceCurrentStatsPollV2(
                        kind = kind,
                        failure = GsplatSurfaceCurrentStatsFailure(kind, raw[4], identity)
                    )
                }
            }
        }

        private fun readyFromRaw(raw: LongArray): GsplatSurfaceCurrentStatsPollV2 {
            check(raw[1] == 0L && raw[4] != 0L)
            val flags = raw[3].toInt()
            check(raw[3] >= 0L && flags and KNOWN_TIMING_FLAGS.inv() == 0)
            check(flags and FRAME_COMPLETE_VALID != 0) {
                "current-stats V2 Ready requires frame-complete timing"
            }
            val preprocess = optionalTiming(raw[20], flags and CPU_PREPROCESS_VALID != 0)
            val sort = optionalTiming(raw[21], flags and CPU_SORT_VALID != 0)
            val receipt = GsplatSurfaceCurrentStatsReceiptV2(
                ticket = raw[4],
                identity = GsplatSurfaceCurrentStatsIdentity.fromRaw(raw, 5),
                sourceCount = raw[15],
                visibleCount = raw[16],
                contributorCount = raw[17],
                drawnCount = raw[18],
                countSemantics = GsplatSurfaceCurrentStatsCountSemantics.fromNative(raw[2]),
                timing = GsplatSurfaceCurrentStatsTimingV2(
                    frameCompleteMs = requiredTiming(raw[19]),
                    cpuPreprocessMs = preprocess,
                    cpuSortMs = sort
                )
            )
            return GsplatSurfaceCurrentStatsPollV2(
                kind = GsplatSurfaceCurrentStatsPollKind.READY,
                receipt = receipt
            )
        }

        private fun requiredTiming(bits: Long): Float {
            check(bits in 0L..0xffff_ffffL) { "current-stats V2 timing bits overflow u32" }
            return Float.fromBits(bits.toInt()).also {
                check(it.isFinite() && it >= 0f) { "current-stats V2 timing is invalid" }
            }
        }

        private fun optionalTiming(bits: Long, valid: Boolean): Float? {
            if (!valid) {
                check(bits == 0L) { "current-stats V2 absent timing carried payload" }
                return null
            }
            return requiredTiming(bits)
        }

        private fun requireInapplicablePayload(raw: LongArray) {
            check(raw.sliceArray(1 until 5).all { it == 0L })
            check(GsplatSurfaceCurrentStatsIdentity.rawPayloadIsZero(raw, 5))
            check(raw.sliceArray(15 until RAW_VALUE_COUNT).all { it == 0L })
        }
    }
}

/** One V2 request/render/submission/poll transaction over the V1 ticket owner. */
data class GsplatSurfaceCurrentStatsCycleV2(
    val request: GsplatSurfaceCurrentStatsRequest,
    val submission: GsplatSurfaceCurrentStatsSubmission,
    val poll: GsplatSurfaceCurrentStatsPollV2,
    val state: GsplatSurfaceCurrentStatsState
)

/** One destructive V2 poll plus its presentation-safe adapter state. */
data class GsplatSurfaceCurrentStatsPollResultV2(
    val poll: GsplatSurfaceCurrentStatsPollV2,
    val state: GsplatSurfaceCurrentStatsState
)
