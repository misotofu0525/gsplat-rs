package com.gsplat.android

enum class GsplatSurfaceCurrentStatsRequestStatus(
    internal val nativeValue: Int,
    val wireName: String
) {
    REQUESTED(1, "requested"),
    BUSY(2, "busy"),
    GPU_UNAVAILABLE(3, "gpu_unavailable"),
    RESOURCE_UNAVAILABLE(4, "resource_unavailable"),
    TICKET_EXHAUSTED(5, "ticket_exhausted");

    companion object {
        internal fun fromNative(value: Long): GsplatSurfaceCurrentStatsRequestStatus =
            entries.firstOrNull { it.nativeValue.toLong() == value }
                ?: error("unknown current-stats request status $value")
    }
}

enum class GsplatSurfaceCurrentStatsSubmissionStatus(internal val nativeValue: Int) {
    NOT_REQUESTED(1),
    ISSUED(2);

    companion object {
        internal fun fromNative(value: Long): GsplatSurfaceCurrentStatsSubmissionStatus =
            entries.firstOrNull { it.nativeValue.toLong() == value }
                ?: error("unknown current-stats submission status $value")
    }
}

enum class GsplatSurfaceCurrentStatsPlan(internal val nativeValue: Int) {
    CPU_POST_SORT(1),
    GPU_POST_SORT(2),
    GPU_PREPROJECT(3);

    companion object {
        internal fun fromNative(value: Long): GsplatSurfaceCurrentStatsPlan =
            entries.firstOrNull { it.nativeValue.toLong() == value }
                ?: error("unknown current-stats executed plan $value")
    }
}

enum class GsplatSurfaceCurrentStatsPollKind(internal val nativeValue: Int) {
    EMPTY(1),
    UNSAMPLED(2),
    READY(3),
    MAP_FAILURE(4),
    GENERATION_INVALIDATED(5),
    EXPIRED(6),
    DROPPED(7);

    val isTerminalFailure: Boolean
        get() = this == MAP_FAILURE || this == GENERATION_INVALIDATED ||
            this == EXPIRED || this == DROPPED

    companion object {
        internal fun fromNative(value: Long): GsplatSurfaceCurrentStatsPollKind =
            entries.firstOrNull { it.nativeValue.toLong() == value }
                ?: error("unknown current-stats poll kind $value")
    }
}

enum class GsplatSurfaceCurrentStatsCountSemantics(internal val nativeValue: Int) {
    DIRECT_DRAW_EQUALS_VISIBLE(1),
    INDIRECT_DRAW_EQUALS_VISIBLE(2),
    INDIRECT_DRAW_EQUALS_CONTRIBUTOR(3);

    companion object {
        internal fun fromNative(value: Long): GsplatSurfaceCurrentStatsCountSemantics =
            entries.firstOrNull { it.nativeValue.toLong() == value }
                ?: error("unknown current-stats count semantics $value")
    }
}

data class GsplatSurfaceCurrentStatsIdentity(
    val sceneGeneration: Long,
    val cameraRevision: Long,
    val viewportGeneration: Long,
    val contractGeneration: Long,
    val planSetGeneration: Long,
    val orderGeneration: Long,
    val rasterGeneration: Long,
    val encodeAttempt: Long,
    val presentationSequence: Long,
    val executedPlan: GsplatSurfaceCurrentStatsPlan
) {
    internal fun writeRaw(target: LongArray, offset: Int) {
        target[offset] = sceneGeneration
        target[offset + 1] = cameraRevision
        target[offset + 2] = viewportGeneration
        target[offset + 3] = contractGeneration
        target[offset + 4] = planSetGeneration
        target[offset + 5] = orderGeneration
        target[offset + 6] = rasterGeneration
        target[offset + 7] = encodeAttempt
        target[offset + 8] = presentationSequence
        target[offset + 9] = executedPlan.nativeValue.toLong()
    }

    companion object {
        internal const val RAW_VALUE_COUNT = 10

        internal fun fromRaw(raw: LongArray, offset: Int): GsplatSurfaceCurrentStatsIdentity {
            require(raw.size >= offset + RAW_VALUE_COUNT) {
                "current-stats identity requires $RAW_VALUE_COUNT values"
            }
            return GsplatSurfaceCurrentStatsIdentity(
                sceneGeneration = raw[offset],
                cameraRevision = raw[offset + 1],
                viewportGeneration = raw[offset + 2],
                contractGeneration = raw[offset + 3],
                planSetGeneration = raw[offset + 4],
                orderGeneration = raw[offset + 5],
                rasterGeneration = raw[offset + 6],
                encodeAttempt = raw[offset + 7],
                presentationSequence = raw[offset + 8],
                executedPlan = GsplatSurfaceCurrentStatsPlan.fromNative(raw[offset + 9])
            )
        }

        internal fun rawPayloadIsZero(raw: LongArray, offset: Int): Boolean =
            (offset until offset + RAW_VALUE_COUNT).all { raw[it] == 0L }
    }
}

data class GsplatSurfaceCurrentStatsRequest(
    val status: GsplatSurfaceCurrentStatsRequestStatus
) {
    companion object {
        internal const val RAW_VALUE_COUNT = 1

        internal fun fromRaw(raw: LongArray): GsplatSurfaceCurrentStatsRequest {
            require(raw.size >= RAW_VALUE_COUNT)
            return GsplatSurfaceCurrentStatsRequest(
                GsplatSurfaceCurrentStatsRequestStatus.fromNative(raw[0])
            )
        }
    }
}

data class GsplatSurfaceCurrentStatsSubmission(
    val status: GsplatSurfaceCurrentStatsSubmissionStatus,
    val ticket: Long?,
    val identity: GsplatSurfaceCurrentStatsIdentity?
) {
    init {
        when (status) {
            GsplatSurfaceCurrentStatsSubmissionStatus.NOT_REQUESTED -> {
                check(ticket == null && identity == null) {
                    "NotRequested current-stats submission carried ticket identity"
                }
            }
            GsplatSurfaceCurrentStatsSubmissionStatus.ISSUED -> {
                check(ticket != null && ticket != 0L && identity != null) {
                    "Issued current-stats submission requires non-zero ticket and full identity"
                }
            }
        }
    }

    companion object {
        internal const val RAW_VALUE_COUNT = 12

        internal fun fromRaw(raw: LongArray): GsplatSurfaceCurrentStatsSubmission {
            require(raw.size >= RAW_VALUE_COUNT)
            val status = GsplatSurfaceCurrentStatsSubmissionStatus.fromNative(raw[0])
            return when (status) {
                GsplatSurfaceCurrentStatsSubmissionStatus.NOT_REQUESTED -> {
                    check(raw[1] == 0L && GsplatSurfaceCurrentStatsIdentity.rawPayloadIsZero(raw, 2)) {
                        "NotRequested current-stats submission exposed inapplicable payload"
                    }
                    GsplatSurfaceCurrentStatsSubmission(status, null, null)
                }
                GsplatSurfaceCurrentStatsSubmissionStatus.ISSUED ->
                    GsplatSurfaceCurrentStatsSubmission(
                        status = status,
                        ticket = raw[1],
                        identity = GsplatSurfaceCurrentStatsIdentity.fromRaw(raw, 2)
                    )
            }
        }
    }
}

data class GsplatSurfaceCurrentStatsReceipt(
    val ticket: Long,
    val identity: GsplatSurfaceCurrentStatsIdentity,
    val sourceCount: Long,
    val visibleCount: Long,
    val contributorCount: Long,
    val drawnCount: Long,
    val countSemantics: GsplatSurfaceCurrentStatsCountSemantics
) {
    init {
        check(ticket != 0L) { "current-stats Ready ticket must be non-zero" }
        check(sourceCount >= 0L && visibleCount in 0L..sourceCount) {
            "current-stats Ready violates 0 <= V <= S"
        }
        check(contributorCount in 0L..visibleCount && drawnCount in 0L..sourceCount) {
            "current-stats Ready violates 0 <= C <= V <= S or D <= S"
        }
        when (countSemantics) {
            GsplatSurfaceCurrentStatsCountSemantics.DIRECT_DRAW_EQUALS_VISIBLE,
            GsplatSurfaceCurrentStatsCountSemantics.INDIRECT_DRAW_EQUALS_VISIBLE ->
                check(drawnCount == visibleCount) {
                    "current-stats visible-draw semantics require D=V"
                }
            GsplatSurfaceCurrentStatsCountSemantics.INDIRECT_DRAW_EQUALS_CONTRIBUTOR ->
                check(drawnCount == contributorCount) {
                    "current-stats contributor-draw semantics require D=C"
                }
        }
    }
}

data class GsplatSurfaceCurrentStatsFailure(
    val kind: GsplatSurfaceCurrentStatsPollKind,
    val ticket: Long,
    val identity: GsplatSurfaceCurrentStatsIdentity
) {
    init {
        check(kind.isTerminalFailure)
        check(ticket != 0L)
    }
}

data class GsplatSurfaceCurrentStatsPoll(
    val kind: GsplatSurfaceCurrentStatsPollKind,
    val requestStatus: GsplatSurfaceCurrentStatsRequestStatus? = null,
    val receipt: GsplatSurfaceCurrentStatsReceipt? = null,
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
                kind.isTerminalFailure && requestStatus == null && receipt == null && failure != null
            )
        }
    }

    companion object {
        internal const val RAW_VALUE_COUNT = 18

        internal fun fromRaw(raw: LongArray): GsplatSurfaceCurrentStatsPoll {
            require(raw.size >= RAW_VALUE_COUNT)
            val kind = GsplatSurfaceCurrentStatsPollKind.fromNative(raw[0])
            return when (kind) {
                GsplatSurfaceCurrentStatsPollKind.EMPTY -> {
                    requireInapplicablePayload(raw)
                    GsplatSurfaceCurrentStatsPoll(kind)
                }
                GsplatSurfaceCurrentStatsPollKind.UNSAMPLED -> {
                    check(raw[2] == 0L && raw[3] == 0L)
                    check(GsplatSurfaceCurrentStatsIdentity.rawPayloadIsZero(raw, 4))
                    check(raw.sliceArray(14 until RAW_VALUE_COUNT).all { it == 0L })
                    GsplatSurfaceCurrentStatsPoll(
                        kind = kind,
                        requestStatus = GsplatSurfaceCurrentStatsRequestStatus.fromNative(raw[1])
                    )
                }
                GsplatSurfaceCurrentStatsPollKind.READY -> {
                    check(raw[1] == 0L)
                    val receipt = GsplatSurfaceCurrentStatsReceipt(
                        ticket = raw[3],
                        identity = GsplatSurfaceCurrentStatsIdentity.fromRaw(raw, 4),
                        sourceCount = raw[14],
                        visibleCount = raw[15],
                        contributorCount = raw[16],
                        drawnCount = raw[17],
                        countSemantics =
                            GsplatSurfaceCurrentStatsCountSemantics.fromNative(raw[2])
                    )
                    GsplatSurfaceCurrentStatsPoll(kind = kind, receipt = receipt)
                }
                else -> {
                    check(raw[1] == 0L && raw[2] == 0L && raw[3] != 0L)
                    check(raw.sliceArray(14 until RAW_VALUE_COUNT).all { it == 0L })
                    val identity = GsplatSurfaceCurrentStatsIdentity.fromRaw(raw, 4)
                    GsplatSurfaceCurrentStatsPoll(
                        kind = kind,
                        failure = GsplatSurfaceCurrentStatsFailure(kind, raw[3], identity)
                    )
                }
            }
        }

        private fun requireInapplicablePayload(raw: LongArray) {
            check(raw[1] == 0L && raw[2] == 0L && raw[3] == 0L)
            check(GsplatSurfaceCurrentStatsIdentity.rawPayloadIsZero(raw, 4))
            check(raw.sliceArray(14 until RAW_VALUE_COUNT).all { it == 0L })
        }
    }
}

sealed interface GsplatSurfaceCurrentStatsState {
    data class NotRequested(val pendingCount: Int) : GsplatSurfaceCurrentStatsState
    data class AwaitingSubmission(
        val pendingCount: Int
    ) : GsplatSurfaceCurrentStatsState
    data class Pending(
        val ticket: Long,
        val identity: GsplatSurfaceCurrentStatsIdentity,
        val pendingCount: Int
    ) : GsplatSurfaceCurrentStatsState
    data class Unavailable(
        val status: GsplatSurfaceCurrentStatsRequestStatus,
        val pendingCount: Int
    ) : GsplatSurfaceCurrentStatsState
    data class Ready(
        val receipt: GsplatSurfaceCurrentStatsReceipt,
        val pendingCount: Int
    ) : GsplatSurfaceCurrentStatsState
    data class Failed(
        val failure: GsplatSurfaceCurrentStatsFailure,
        val pendingCount: Int
    ) : GsplatSurfaceCurrentStatsState
    data class Rejected(
        val reason: String,
        val ticket: Long?,
        val pendingCount: Int
    ) : GsplatSurfaceCurrentStatsState
}

data class GsplatSurfaceCurrentStatsCycle(
    val request: GsplatSurfaceCurrentStatsRequest,
    val submission: GsplatSurfaceCurrentStatsSubmission,
    val poll: GsplatSurfaceCurrentStatsPoll,
    val state: GsplatSurfaceCurrentStatsState
)

/**
 * Thin Android consumer for the Renderer-owned current-stats v1 values.
 *
 * Renderer admission bounds pending tickets. This consumer retains their full
 * identities plus a finite terminal/rejection replay window so repeated getter
 * snapshots stay idempotent. It never creates tickets, generations, samples,
 * plans, or fallback counts.
 */
class GsplatSurfaceCurrentStatsAdapter {
    private class PresentationWatermark

    // The opaque token avoids assuming native ticket/sequence ordering. Old
    // tokens remain referenced only by renderer-bounded pending tickets.
    private data class PendingTicket(
        val identity: GsplatSurfaceCurrentStatsIdentity,
        val presentationWatermark: PresentationWatermark
    )

    private enum class TicketPhase {
        TERMINAL,
        REJECTED
    }

    private data class TicketRecord(
        val identity: GsplatSurfaceCurrentStatsIdentity,
        val phase: TicketPhase
    )

    // Renderer admission bounds issued-but-unresolved tickets. Android adds
    // only a finite replay window for completed/rejected submission snapshots.
    private val pending = LinkedHashMap<Long, PendingTicket>()
    private val tombstones = LinkedHashMap<Long, TicketRecord>()
    private var outstandingRequest: GsplatSurfaceCurrentStatsRequest? = null
    private var currentPresentationWatermark = PresentationWatermark()

    var state: GsplatSurfaceCurrentStatsState =
        GsplatSurfaceCurrentStatsState.NotRequested(pendingCount = 0)
        private set

    val currentReceipt: GsplatSurfaceCurrentStatsReceipt?
        get() = (state as? GsplatSurfaceCurrentStatsState.Ready)?.receipt

    val pendingCount: Int
        get() = pending.size

    internal val requiresSubmissionReconciliation: Boolean
        get() = outstandingRequest != null

    internal val trackedTombstoneCount: Int
        get() = tombstones.size

    fun request(nativeHandle: Long): GsplatSurfaceCurrentStatsRequest {
        val raw = LongArray(GsplatSurfaceCurrentStatsRequest.RAW_VALUE_COUNT)
        checkNative(NativeBridge.requestSurfaceCurrentStatsV1(nativeHandle, raw))
        return observeRequest(GsplatSurfaceCurrentStatsRequest.fromRaw(raw))
    }

    internal fun observeRequest(
        request: GsplatSurfaceCurrentStatsRequest
    ): GsplatSurfaceCurrentStatsRequest {
        state = when (request.status) {
            GsplatSurfaceCurrentStatsRequestStatus.REQUESTED -> {
                outstandingRequest = request
                GsplatSurfaceCurrentStatsState.AwaitingSubmission(pendingCount)
            }
            else -> GsplatSurfaceCurrentStatsState.Unavailable(request.status, pendingCount)
        }
        return request
    }

    fun complete(
        nativeHandle: Long,
        request: GsplatSurfaceCurrentStatsRequest
    ): GsplatSurfaceCurrentStatsCycle {
        return completeAfterSuccessfulPresentation(
            request = request,
            readSubmission = { readSubmission(nativeHandle) },
            readPoll = { readPoll(nativeHandle) }
        )
    }

    internal fun reconcileAfterSuccessfulRender(
        readSubmission: () -> GsplatSurfaceCurrentStatsSubmission,
        readPoll: () -> GsplatSurfaceCurrentStatsPoll
    ): GsplatSurfaceCurrentStatsCycle? {
        val request = outstandingRequest ?: return null
        return completeAfterSuccessfulPresentation(request, readSubmission, readPoll)
    }

    internal fun reconcileAfterSuccessfulRender(
        nativeHandle: Long
    ): GsplatSurfaceCurrentStatsCycle? = reconcileAfterSuccessfulRender(
        readSubmission = { readSubmission(nativeHandle) },
        readPoll = { readPoll(nativeHandle) }
    )

    internal fun reconcileAfterOrdinaryRender(
        readSubmission: () -> GsplatSurfaceCurrentStatsSubmission,
        readPoll: () -> GsplatSurfaceCurrentStatsPoll
    ): GsplatSurfaceCurrentStatsCycle? = try {
        reconcileAfterSuccessfulRender(readSubmission, readPoll)
    } catch (_: RuntimeException) {
        recoveryReadFailed()
        null
    }

    internal fun reconcileAfterOrdinaryRender(
        nativeHandle: Long
    ): GsplatSurfaceCurrentStatsCycle? = reconcileAfterOrdinaryRender(
        readSubmission = { readSubmission(nativeHandle) },
        readPoll = { readPoll(nativeHandle) }
    )

    internal fun completeAfterSuccessfulPresentation(
        request: GsplatSurfaceCurrentStatsRequest,
        readSubmission: () -> GsplatSurfaceCurrentStatsSubmission,
        readPoll: () -> GsplatSurfaceCurrentStatsPoll
    ): GsplatSurfaceCurrentStatsCycle {
        advancePresentationWatermark()
        return complete(request, readSubmission, readPoll)
    }

    private fun complete(
        request: GsplatSurfaceCurrentStatsRequest,
        readSubmission: () -> GsplatSurfaceCurrentStatsSubmission,
        readPoll: () -> GsplatSurfaceCurrentStatsPoll
    ): GsplatSurfaceCurrentStatsCycle {
        if (request.status == GsplatSurfaceCurrentStatsRequestStatus.REQUESTED &&
            outstandingRequest == null
        ) {
            outstandingRequest = request
        }
        val submission = readSubmission()
        val submissionRejection = observeSubmission(submission)
        val poll = readPoll()
        val pollState = consumePollValue(poll, request, submission)
        state = submissionRejection ?: pollState
        return GsplatSurfaceCurrentStatsCycle(request, submission, poll, state)
    }

    /** Single non-blocking poll used while flushing already-issued tickets. */
    fun poll(nativeHandle: Long): GsplatSurfaceCurrentStatsState {
        return consumePoll(readPoll(nativeHandle))
    }

    internal fun consumePoll(
        poll: GsplatSurfaceCurrentStatsPoll
    ): GsplatSurfaceCurrentStatsState {
        state = consumePollValue(poll, request = null, submission = null)
        return state
    }

    fun consume(
        request: GsplatSurfaceCurrentStatsRequest,
        submission: GsplatSurfaceCurrentStatsSubmission,
        poll: GsplatSurfaceCurrentStatsPoll
    ): GsplatSurfaceCurrentStatsCycle {
        if (request.status == GsplatSurfaceCurrentStatsRequestStatus.REQUESTED &&
            outstandingRequest == null
        ) {
            outstandingRequest = request
        }
        val submissionRejection = observeSubmission(submission)
        val pollState = consumePollValue(poll, request, submission)
        state = submissionRejection ?: pollState
        return GsplatSurfaceCurrentStatsCycle(request, submission, poll, state)
    }

    fun abandonFrame() {
        state = if (outstandingRequest != null) {
            GsplatSurfaceCurrentStatsState.AwaitingSubmission(pendingCount)
        } else {
            rejected("render_failed_before_submission", ticket = null)
        }
    }

    fun recoveryReadFailed() {
        state = rejected("submission_reconciliation_failed", ticket = null)
    }

    /** Invalidates a previously Ready snapshot after a presentation without a new request. */
    internal fun observeOrdinaryRender() {
        check(outstandingRequest == null) {
            "ordinary render cannot bypass current-stats submission reconciliation"
        }
        advancePresentationWatermark()
        state = emptyState(request = null, submission = null)
    }

    fun reset() {
        pending.clear()
        tombstones.clear()
        outstandingRequest = null
        currentPresentationWatermark = PresentationWatermark()
        state = GsplatSurfaceCurrentStatsState.NotRequested(0)
    }

    private fun advancePresentationWatermark() {
        currentPresentationWatermark = PresentationWatermark()
    }

    private fun observeSubmission(
        submission: GsplatSurfaceCurrentStatsSubmission
    ): GsplatSurfaceCurrentStatsState.Rejected? {
        if (submission.status == GsplatSurfaceCurrentStatsSubmissionStatus.NOT_REQUESTED) {
            return null
        }
        outstandingRequest = null
        val ticket = checkNotNull(submission.ticket)
        val identity = checkNotNull(submission.identity)
        val pendingTicket = pending[ticket]
        if (pendingTicket != null) {
            if (pendingTicket.identity != identity) {
                pending.remove(ticket)
                rememberTombstone(ticket, pendingTicket.identity, TicketPhase.REJECTED)
                return rejected("submission_ticket_identity_mismatch", ticket)
            }
            return null
        }
        val completed = tombstones[ticket]
        if (completed != null) {
            if (completed.phase == TicketPhase.REJECTED) {
                return rejected("submission_ticket_already_rejected", ticket)
            }
            if (completed.identity != identity) {
                rememberTombstone(ticket, completed.identity, TicketPhase.REJECTED)
                return rejected("submission_ticket_identity_mismatch", ticket)
            }
            return null
        }
        pending[ticket] = PendingTicket(identity, currentPresentationWatermark)
        return null
    }

    private fun consumePollValue(
        poll: GsplatSurfaceCurrentStatsPoll,
        request: GsplatSurfaceCurrentStatsRequest?,
        submission: GsplatSurfaceCurrentStatsSubmission?
    ): GsplatSurfaceCurrentStatsState = when (poll.kind) {
        GsplatSurfaceCurrentStatsPollKind.EMPTY -> emptyState(request, submission)
        GsplatSurfaceCurrentStatsPollKind.UNSAMPLED -> {
            outstandingRequest = null
            GsplatSurfaceCurrentStatsState.Unavailable(
                status = checkNotNull(poll.requestStatus),
                pendingCount = pendingCount
            )
        }
        GsplatSurfaceCurrentStatsPollKind.READY -> consumeReady(checkNotNull(poll.receipt))
        else -> consumeFailure(checkNotNull(poll.failure))
    }

    private fun emptyState(
        request: GsplatSurfaceCurrentStatsRequest?,
        submission: GsplatSurfaceCurrentStatsSubmission?
    ): GsplatSurfaceCurrentStatsState {
        val oldest = pending.entries.firstOrNull()
        if (oldest != null) {
            return GsplatSurfaceCurrentStatsState.Pending(
                ticket = oldest.key,
                identity = oldest.value.identity,
                pendingCount = pendingCount
            )
        }
        if (outstandingRequest != null) {
            return GsplatSurfaceCurrentStatsState.AwaitingSubmission(pendingCount)
        }
        return if (submission?.status == GsplatSurfaceCurrentStatsSubmissionStatus.ISSUED) {
            GsplatSurfaceCurrentStatsState.NotRequested(pendingCount)
        } else if (request != null &&
            request.status != GsplatSurfaceCurrentStatsRequestStatus.REQUESTED
        ) {
            GsplatSurfaceCurrentStatsState.Unavailable(request.status, pendingCount)
        } else {
            GsplatSurfaceCurrentStatsState.NotRequested(pendingCount)
        }
    }

    private fun consumeReady(
        receipt: GsplatSurfaceCurrentStatsReceipt
    ): GsplatSurfaceCurrentStatsState {
        val pendingTicket = pending.remove(receipt.ticket)
        if (pendingTicket == null) {
            val completed = tombstones[receipt.ticket]
            if (completed?.phase == TicketPhase.REJECTED) {
                return rejected("ready_for_rejected_ticket", receipt.ticket)
            }
            if (completed?.phase == TicketPhase.TERMINAL) {
                return rejected("duplicate_ready_terminal", receipt.ticket)
            }
            rememberTombstone(receipt.ticket, receipt.identity, TicketPhase.REJECTED)
            return rejected("ready_without_issued_submission", receipt.ticket)
        }
        if (pendingTicket.identity != receipt.identity) {
            rememberTombstone(receipt.ticket, pendingTicket.identity, TicketPhase.REJECTED)
            return rejected("ready_identity_mismatch", receipt.ticket)
        }
        rememberTombstone(receipt.ticket, pendingTicket.identity, TicketPhase.TERMINAL)
        val readyIsCurrent = pendingTicket.presentationWatermark ===
            currentPresentationWatermark
        return if (readyIsCurrent) {
            GsplatSurfaceCurrentStatsState.Ready(receipt, pendingCount)
        } else {
            emptyState(request = null, submission = null)
        }
    }

    private fun consumeFailure(
        failure: GsplatSurfaceCurrentStatsFailure
    ): GsplatSurfaceCurrentStatsState {
        val pendingTicket = pending.remove(failure.ticket)
        if (pendingTicket == null) {
            val completed = tombstones[failure.ticket]
            if (completed?.phase == TicketPhase.REJECTED) {
                return rejected("failure_for_rejected_ticket", failure.ticket)
            }
            if (completed?.phase == TicketPhase.TERMINAL) {
                return rejected("duplicate_failure_terminal", failure.ticket)
            }
            rememberTombstone(failure.ticket, failure.identity, TicketPhase.REJECTED)
            return rejected("failure_without_issued_submission", failure.ticket)
        }
        if (pendingTicket.identity != failure.identity) {
            rememberTombstone(failure.ticket, pendingTicket.identity, TicketPhase.REJECTED)
            return rejected("failure_identity_mismatch", failure.ticket)
        }
        rememberTombstone(failure.ticket, pendingTicket.identity, TicketPhase.TERMINAL)
        return GsplatSurfaceCurrentStatsState.Failed(failure, pendingCount)
    }

    private fun rememberTombstone(
        ticket: Long,
        identity: GsplatSurfaceCurrentStatsIdentity,
        phase: TicketPhase
    ) {
        tombstones.remove(ticket)
        tombstones[ticket] = TicketRecord(identity, phase)
        while (tombstones.size > REPLAY_TOMBSTONE_CAPACITY) {
            tombstones.remove(tombstones.entries.first().key)
        }
    }

    private fun rejected(
        reason: String,
        ticket: Long?
    ) = GsplatSurfaceCurrentStatsState.Rejected(
        reason = reason,
        ticket = ticket,
        pendingCount = pendingCount
    )

    private fun readSubmission(nativeHandle: Long): GsplatSurfaceCurrentStatsSubmission {
        val raw = LongArray(GsplatSurfaceCurrentStatsSubmission.RAW_VALUE_COUNT)
        checkNative(NativeBridge.getSurfaceCurrentStatsSubmissionV1(nativeHandle, raw))
        return GsplatSurfaceCurrentStatsSubmission.fromRaw(raw)
    }

    private fun readPoll(nativeHandle: Long): GsplatSurfaceCurrentStatsPoll {
        val raw = LongArray(GsplatSurfaceCurrentStatsPoll.RAW_VALUE_COUNT)
        checkNative(NativeBridge.pollSurfaceCurrentStatsV1(nativeHandle, raw))
        return GsplatSurfaceCurrentStatsPoll.fromRaw(raw)
    }

    private fun checkNative(code: Int) {
        if (code != 0) throw GsplatException(code)
    }

    private companion object {
        const val REPLAY_TOMBSTONE_CAPACITY = 8
    }
}
