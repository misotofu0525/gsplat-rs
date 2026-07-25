package com.gsplat.example

import com.gsplat.android.GsplatSurfaceCurrentStatsAdapter
import com.gsplat.android.GsplatSurfaceCurrentStatsCycle
import com.gsplat.android.GsplatSurfaceCurrentStatsFailure
import com.gsplat.android.GsplatSurfaceCurrentStatsIdentity
import com.gsplat.android.GsplatSurfaceCurrentStatsPollKind
import com.gsplat.android.GsplatSurfaceCurrentStatsReceipt
import com.gsplat.android.GsplatSurfaceCurrentStatsRequest
import com.gsplat.android.GsplatSurfaceCurrentStatsRequestStatus
import com.gsplat.android.GsplatSurfaceCurrentStatsState
import com.gsplat.android.GsplatSurfaceCurrentStatsSubmissionStatus

internal data class SurfaceCurrentStatsFrameBinding(
    val frameId: Long,
    val sampleIndex: Int?,
    val traceFrameIndex: Int?,
    val traceTimestampNs: Long?
)

internal sealed interface SurfaceCurrentStatsTerminal {
    data class Ready(val receipt: GsplatSurfaceCurrentStatsReceipt) :
        SurfaceCurrentStatsTerminal

    data class Failure(val failure: GsplatSurfaceCurrentStatsFailure) :
        SurfaceCurrentStatsTerminal

    data class Unavailable(val status: GsplatSurfaceCurrentStatsRequestStatus) :
        SurfaceCurrentStatsTerminal

    data class Rejected(val reason: String, val ticket: Long?) :
        SurfaceCurrentStatsTerminal
}

internal data class SurfaceCurrentStatsSampleRecord(
    val binding: SurfaceCurrentStatsFrameBinding,
    val requestStatus: GsplatSurfaceCurrentStatsRequestStatus,
    val submissionIssued: Boolean,
    val ticket: Long?,
    val identity: GsplatSurfaceCurrentStatsIdentity?,
    val terminal: SurfaceCurrentStatsTerminal?
)

internal sealed interface SurfaceCurrentStatsDisplay {
    data class Ready(
        val binding: SurfaceCurrentStatsFrameBinding,
        val receipt: GsplatSurfaceCurrentStatsReceipt
    ) : SurfaceCurrentStatsDisplay

    data class Unavailable(
        val reason: String,
        val pendingCount: Int
    ) : SurfaceCurrentStatsDisplay
}

internal data class SurfaceCurrentStatsTrackedState(
    val outstandingRequestCount: Int,
    val issuedTicketCount: Int,
    val sampleRecordCount: Int,
    val rejectionRecordCount: Int
) {
    val totalRecordCount: Int
        get() =
            outstandingRequestCount + issuedTicketCount + sampleRecordCount +
                rejectionRecordCount
}

/**
 * Sample-owned correlation and evidence ledger over the accepted Android
 * current-stats adapter. This class never creates native tickets, generations,
 * plans, samples, or fallback counts.
 */
internal class SurfaceCurrentStatsConsumer(
    private val adapter: GsplatSurfaceCurrentStatsAdapter =
        GsplatSurfaceCurrentStatsAdapter()
) {
    private class PresentationWatermark

    private data class OutstandingRequest(
        val binding: SurfaceCurrentStatsFrameBinding,
        val request: GsplatSurfaceCurrentStatsRequest
    )

    private data class IssuedTicket(
        val binding: SurfaceCurrentStatsFrameBinding,
        val identity: GsplatSurfaceCurrentStatsIdentity,
        val presentationWatermark: PresentationWatermark
    )

    private data class MutableSampleRecord(
        val binding: SurfaceCurrentStatsFrameBinding,
        val requestStatus: GsplatSurfaceCurrentStatsRequestStatus,
        var submissionIssued: Boolean = false,
        var ticket: Long? = null,
        var identity: GsplatSurfaceCurrentStatsIdentity? = null,
        var terminal: SurfaceCurrentStatsTerminal? = null
    ) {
        fun snapshot() = SurfaceCurrentStatsSampleRecord(
            binding = binding,
            requestStatus = requestStatus,
            submissionIssued = submissionIssued,
            ticket = ticket,
            identity = identity,
            terminal = terminal
        )
    }

    private val samples = LinkedHashMap<Int, MutableSampleRecord>()
    // Renderer admission bounds this unresolved-only set.
    private val issued = LinkedHashMap<Long, IssuedTicket>()
    // One rejection is enough to invalidate every later strict benchmark read.
    private var firstRejection: String? = null
    private var outstanding: OutstandingRequest? = null
    private var latestBinding: SurfaceCurrentStatsFrameBinding? = null
    // Old tokens remain referenced only by renderer-bounded issued tickets.
    private var currentPresentationWatermark = PresentationWatermark()
    private var nextUiFrameId = -1L

    var display: SurfaceCurrentStatsDisplay =
        SurfaceCurrentStatsDisplay.Unavailable("not_requested", 0)
        private set

    var displayVersion: Long = 0L
        private set

    val hasInFlight: Boolean
        get() = outstanding != null || issued.isNotEmpty()

    internal fun trackedStateForTest() = SurfaceCurrentStatsTrackedState(
        outstandingRequestCount = if (outstanding == null) 0 else 1,
        issuedTicketCount = issued.size,
        sampleRecordCount = samples.size,
        rejectionRecordCount = if (firstRejection == null) 0 else 1
    )

    fun nextUiBinding(): SurfaceCurrentStatsFrameBinding =
        SurfaceCurrentStatsFrameBinding(
            frameId = nextUiFrameId--,
            sampleIndex = null,
            traceFrameIndex = null,
            traceTimestampNs = null
        )

    fun request(
        nativeHandle: Long,
        binding: SurfaceCurrentStatsFrameBinding
    ): GsplatSurfaceCurrentStatsRequest = beginRequest(binding) {
        adapter.request(nativeHandle)
    }

    internal fun beginRequest(
        binding: SurfaceCurrentStatsFrameBinding,
        readRequest: () -> GsplatSurfaceCurrentStatsRequest
    ): GsplatSurfaceCurrentStatsRequest {
        outstanding?.let { current ->
            check(current.binding == binding) {
                "current-stats pre-ticket intent drifted to another frame"
            }
            return current.request
        }
        val request = readRequest()
        latestBinding = binding
        setDisplay(
            SurfaceCurrentStatsDisplay.Unavailable(
                reason = if (request.status == GsplatSurfaceCurrentStatsRequestStatus.REQUESTED) {
                    "awaiting_submission"
                } else {
                    request.status.wireName
                },
                pendingCount = pendingTicketCount()
            )
        )
        binding.sampleIndex?.let { sampleIndex ->
            check(samples.put(
                sampleIndex,
                MutableSampleRecord(binding, request.status)
            ) == null) {
                "current-stats sample $sampleIndex requested more than once"
            }
        }
        if (request.status == GsplatSurfaceCurrentStatsRequestStatus.REQUESTED) {
            outstanding = OutstandingRequest(binding, request)
        }
        return request
    }

    fun renderFailed() {
        adapter.abandonFrame()
        observeRenderFailure()
    }

    internal fun observeRenderFailure() {
        setDisplay(
            SurfaceCurrentStatsDisplay.Unavailable(
                reason = if (outstanding == null) {
                    "render_failed_without_request"
                } else {
                    "awaiting_submission_after_render_failure"
                },
                pendingCount = pendingTicketCount()
            )
        )
    }

    /**
     * Closes a pre-ticket intent whose same-frame retry command failed.
     *
     * There is no native cancellation operation for an accepted request, so
     * the caller must destroy the native renderer before another render. This
     * prevents that intent from issuing against a different camera/trace frame.
     */
    fun closeOutstandingAfterCommandFailure(): Boolean {
        val current = outstanding ?: return false
        adapter.abandonFrame()
        outstanding = null
        recordRejection(
            reason = "command_failed_before_same_frame_retry",
            ticket = null,
            binding = current.binding
        )
        setDisplay(
            SurfaceCurrentStatsDisplay.Unavailable(
                reason = "command_failed_current_stats_session_closed",
                pendingCount = pendingTicketCount()
            )
        )
        return true
    }

    fun afterSuccessfulRender(nativeHandle: Long): SurfaceCurrentStatsDisplay =
        advanceAfterSuccessfulRender(
            completeRequest = { request -> adapter.complete(nativeHandle, request) },
            pollPending = { adapter.poll(nativeHandle) }
        )

    internal fun advanceAfterSuccessfulRender(
        completeRequest: (GsplatSurfaceCurrentStatsRequest) -> GsplatSurfaceCurrentStatsCycle,
        pollPending: () -> GsplatSurfaceCurrentStatsState
    ): SurfaceCurrentStatsDisplay {
        currentPresentationWatermark = PresentationWatermark()
        val current = outstanding
        if (current != null) {
            consumeCycle(completeRequest(current.request))
        } else if (issued.isNotEmpty()) {
            consumePolledState(pollPending(), readyIsCurrent = false)
        } else {
            publishState(GsplatSurfaceCurrentStatsState.NotRequested(pendingCount = 0))
        }
        return display
    }

    fun pollPending(nativeHandle: Long): SurfaceCurrentStatsDisplay =
        pollPending { adapter.poll(nativeHandle) }

    internal fun pollPending(
        readPoll: () -> GsplatSurfaceCurrentStatsState
    ): SurfaceCurrentStatsDisplay {
        if (issued.isNotEmpty()) {
            consumePolledState(readPoll())
        }
        return display
    }

    internal fun consumeCycle(cycle: GsplatSurfaceCurrentStatsCycle) {
        val intent = checkNotNull(outstanding) {
            "current-stats cycle arrived without an outstanding intent"
        }
        check(outstanding == intent) { "current-stats cycle does not match outstanding intent" }
        check(cycle.request == intent.request) { "current-stats cycle changed its request status" }

        if (cycle.submission.status == GsplatSurfaceCurrentStatsSubmissionStatus.ISSUED) {
            val ticket = checkNotNull(cycle.submission.ticket)
            val identity = checkNotNull(cycle.submission.identity)
            recordSubmission(intent.binding, ticket, identity)
            outstanding = null
        }

        var readyTicket: IssuedTicket? = null
        when (cycle.poll.kind) {
            GsplatSurfaceCurrentStatsPollKind.EMPTY -> Unit
            GsplatSurfaceCurrentStatsPollKind.UNSAMPLED -> {
                val status = checkNotNull(cycle.poll.requestStatus)
                val terminal = SurfaceCurrentStatsTerminal.Unavailable(status)
                recordPreTicketTerminal(intent.binding, terminal)
                outstanding = null
            }
            GsplatSurfaceCurrentStatsPollKind.READY ->
                readyTicket = recordReady(checkNotNull(cycle.poll.receipt))
            else -> recordFailure(checkNotNull(cycle.poll.failure))
        }
        val cycleState = cycle.state
        if (cycleState is GsplatSurfaceCurrentStatsState.Rejected) {
            recordRejection(cycleState.reason, cycleState.ticket, intent.binding)
        }
        publishState(cycleState, readyTicket)
    }

    internal fun consumePolledState(
        state: GsplatSurfaceCurrentStatsState,
        readyIsCurrent: Boolean = true
    ) {
        var readyTicket: IssuedTicket? = null
        when (state) {
            is GsplatSurfaceCurrentStatsState.Ready ->
                readyTicket = recordReady(state.receipt)
            is GsplatSurfaceCurrentStatsState.Failed -> recordFailure(state.failure)
            is GsplatSurfaceCurrentStatsState.Rejected ->
                recordRejection(state.reason, state.ticket, null)
            else -> Unit
        }
        publishState(state, readyTicket, readyIsCurrent)
    }

    fun benchmarkTerminalsComplete(expectedSamples: Int): Boolean =
        (0 until expectedSamples).all { index ->
            val record = samples[index]
            record == null || record.ticket == null || record.terminal != null
        }

    fun benchmarkDiagnostics(expectedSamples: Int): String {
        val records = (0 until expectedSamples).mapNotNull(samples::get)
        val issued = records.count { it.ticket != null }
        val terminals = records.count { it.terminal != null }
        val firstMissingSubmission = (0 until expectedSamples).firstOrNull { index ->
            samples[index]?.ticket == null
        }
        val firstPendingTerminal = (0 until expectedSamples).firstOrNull { index ->
            val record = samples[index]
            record?.ticket != null && record.terminal == null
        }
        return "current_stats_records=${records.size} current_stats_issued=$issued " +
            "current_stats_callback_armed=$issued " +
            "current_stats_terminal=$terminals current_stats_pending=${issued - terminals} " +
            "current_stats_rejections=${if (firstRejection == null) 0 else 1} " +
            "first_missing_current_stats_submission=${firstMissingSubmission ?: -1} " +
            "first_pending_current_stats_terminal=${firstPendingTerminal ?: -1}"
    }

    fun strictRecords(expectedSamples: Int): List<SurfaceCurrentStatsSampleRecord> {
        check(firstRejection == null) {
            "current-stats consumer rejected evidence: $firstRejection"
        }
        return (0 until expectedSamples).map { index ->
            val record = checkNotNull(samples[index]) {
                "measured frame $index lacks a current-stats pre-ticket record"
            }.snapshot()
            check(record.requestStatus == GsplatSurfaceCurrentStatsRequestStatus.REQUESTED) {
                "measured frame $index current-stats request was ${record.requestStatus.wireName}"
            }
            check(record.submissionIssued && record.ticket != null && record.identity != null) {
                "measured frame $index lacks an Issued current-stats submission"
            }
            check(record.terminal is SurfaceCurrentStatsTerminal.Ready) {
                "measured frame $index lacks a matching Ready current-stats terminal"
            }
            record
        }.also { records ->
            check(records.map { checkNotNull(it.ticket) }.toSet().size == records.size) {
                "measured current-stats tickets are not unique"
            }
            check(
                records.map { checkNotNull(it.identity).presentationSequence }.toSet().size ==
                    records.size
            ) {
                "measured current-stats presentation sequences are not unique"
            }
        }
    }

    fun recordForSample(sampleIndex: Int): SurfaceCurrentStatsSampleRecord? =
        samples[sampleIndex]?.snapshot()

    private fun recordSubmission(
        binding: SurfaceCurrentStatsFrameBinding,
        ticket: Long,
        identity: GsplatSurfaceCurrentStatsIdentity
    ) {
        check(ticket != 0L)
        val previous = issued[ticket]
        if (previous != null) {
            val reason = if (previous.identity == identity) {
                "duplicate_issued_ticket"
            } else {
                "issued_ticket_identity_drift"
            }
            recordRejection(reason, ticket, binding)
            return
        }
        issued[ticket] = IssuedTicket(binding, identity, currentPresentationWatermark)
        binding.sampleIndex?.let { sampleIndex ->
            val record = checkNotNull(samples[sampleIndex])
            record.submissionIssued = true
            record.ticket = ticket
            record.identity = identity
        }
    }

    private fun recordReady(
        receipt: GsplatSurfaceCurrentStatsReceipt
    ): IssuedTicket? {
        val issuedTicket = issued[receipt.ticket]
        if (issuedTicket == null) {
            recordRejection("ready_without_issued_ticket", receipt.ticket, null)
            return null
        }
        if (issuedTicket.identity != receipt.identity) {
            recordRejection("ready_identity_drift", receipt.ticket, issuedTicket.binding)
            return null
        }
        val terminal = SurfaceCurrentStatsTerminal.Ready(receipt)
        issuedTicket.binding.sampleIndex?.let { sampleIndex ->
            checkNotNull(samples[sampleIndex]).terminal = terminal
        }
        issued.remove(receipt.ticket)
        return issuedTicket
    }

    private fun recordFailure(failure: GsplatSurfaceCurrentStatsFailure) {
        val issuedTicket = issued[failure.ticket]
        if (issuedTicket == null) {
            recordRejection("failure_without_issued_ticket", failure.ticket, null)
            return
        }
        if (issuedTicket.identity != failure.identity) {
            recordRejection("failure_identity_drift", failure.ticket, issuedTicket.binding)
            return
        }
        val terminal = SurfaceCurrentStatsTerminal.Failure(failure)
        issuedTicket.binding.sampleIndex?.let { sampleIndex ->
            checkNotNull(samples[sampleIndex]).terminal = terminal
        }
        issued.remove(failure.ticket)
    }

    private fun recordPreTicketTerminal(
        binding: SurfaceCurrentStatsFrameBinding,
        terminal: SurfaceCurrentStatsTerminal
    ) {
        binding.sampleIndex?.let { sampleIndex ->
            checkNotNull(samples[sampleIndex]).terminal = terminal
        }
    }

    private fun recordRejection(
        reason: String,
        ticket: Long?,
        binding: SurfaceCurrentStatsFrameBinding?
    ) {
        val detail = "$reason${ticket?.let { ":ticket=$it" } ?: ""}"
        if (firstRejection == null) {
            firstRejection = detail
        }
        val issuedBinding = ticket?.let { issued.remove(it)?.binding }
        val terminal = SurfaceCurrentStatsTerminal.Rejected(reason, ticket)
        listOfNotNull(issuedBinding, binding).distinct().forEach { resolvedBinding ->
            resolvedBinding.sampleIndex?.let { sampleIndex ->
                checkNotNull(samples[sampleIndex]).terminal = terminal
            }
        }
    }

    private fun publishState(
        state: GsplatSurfaceCurrentStatsState,
        readyTicket: IssuedTicket? = null,
        readyIsCurrent: Boolean = true
    ) {
        val next = when (state) {
            is GsplatSurfaceCurrentStatsState.Ready -> {
                val readyBinding = readyTicket?.binding
                val matchesCurrentPresentation = readyTicket?.presentationWatermark ===
                    currentPresentationWatermark
                if (readyIsCurrent &&
                    matchesCurrentPresentation &&
                    readyBinding != null &&
                    readyBinding == latestBinding &&
                    outstanding == null
                ) {
                    SurfaceCurrentStatsDisplay.Ready(readyBinding, state.receipt)
                } else {
                    SurfaceCurrentStatsDisplay.Unavailable(
                        if (!readyIsCurrent) {
                            "ready_for_prior_presentation"
                        } else if (readyTicket == null) {
                            "ready_without_matching_issued"
                        } else if (!matchesCurrentPresentation) {
                            "ready_for_prior_presentation"
                        } else {
                            "newer_sample_pending"
                        },
                        pendingTicketCount()
                    )
                }
            }
            is GsplatSurfaceCurrentStatsState.Pending ->
                SurfaceCurrentStatsDisplay.Unavailable("pending", state.pendingCount)
            is GsplatSurfaceCurrentStatsState.AwaitingSubmission ->
                SurfaceCurrentStatsDisplay.Unavailable(
                    "awaiting_submission",
                    state.pendingCount
                )
            is GsplatSurfaceCurrentStatsState.Unavailable ->
                SurfaceCurrentStatsDisplay.Unavailable(
                    state.status.wireName,
                    state.pendingCount
                )
            is GsplatSurfaceCurrentStatsState.Failed ->
                SurfaceCurrentStatsDisplay.Unavailable(
                    state.failure.kind.name.lowercase(),
                    state.pendingCount
                )
            is GsplatSurfaceCurrentStatsState.Rejected ->
                SurfaceCurrentStatsDisplay.Unavailable(state.reason, state.pendingCount)
            is GsplatSurfaceCurrentStatsState.NotRequested ->
                SurfaceCurrentStatsDisplay.Unavailable("not_requested", state.pendingCount)
        }
        setDisplay(next)
    }

    private fun setDisplay(next: SurfaceCurrentStatsDisplay) {
        if (display != next) {
            display = next
            displayVersion += 1L
        }
    }

    private fun pendingTicketCount(): Int = issued.size
}
