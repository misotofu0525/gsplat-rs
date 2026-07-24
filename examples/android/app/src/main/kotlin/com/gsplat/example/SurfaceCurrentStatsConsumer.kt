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

/**
 * Sample-owned correlation and evidence ledger over the accepted Android
 * current-stats adapter. This class never creates native tickets, generations,
 * plans, samples, or fallback counts.
 */
internal class SurfaceCurrentStatsConsumer(
    private val adapter: GsplatSurfaceCurrentStatsAdapter =
        GsplatSurfaceCurrentStatsAdapter()
) {
    private data class OutstandingRequest(
        val binding: SurfaceCurrentStatsFrameBinding,
        val request: GsplatSurfaceCurrentStatsRequest
    )

    private data class IssuedTicket(
        val binding: SurfaceCurrentStatsFrameBinding,
        val identity: GsplatSurfaceCurrentStatsIdentity,
        var terminal: SurfaceCurrentStatsTerminal? = null
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
    private val issued = LinkedHashMap<Long, IssuedTicket>()
    private val rejections = ArrayList<String>()
    private var outstanding: OutstandingRequest? = null
    private var latestBinding: SurfaceCurrentStatsFrameBinding? = null
    private var nextUiFrameId = -1L

    var display: SurfaceCurrentStatsDisplay =
        SurfaceCurrentStatsDisplay.Unavailable("not_requested", 0)
        private set

    var displayVersion: Long = 0L
        private set

    val hasInFlight: Boolean
        get() = outstanding != null || issued.values.any { it.terminal == null }

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

    fun afterSuccessfulRender(nativeHandle: Long): SurfaceCurrentStatsDisplay =
        advanceAfterSuccessfulRender(
            completeRequest = { request -> adapter.complete(nativeHandle, request) },
            pollPending = { adapter.poll(nativeHandle) }
        )

    internal fun advanceAfterSuccessfulRender(
        completeRequest: (GsplatSurfaceCurrentStatsRequest) -> GsplatSurfaceCurrentStatsCycle,
        pollPending: () -> GsplatSurfaceCurrentStatsState
    ): SurfaceCurrentStatsDisplay {
        val current = outstanding
        if (current != null) {
            consumeCycle(completeRequest(current.request))
        } else if (issued.values.any { it.terminal == null }) {
            consumePolledState(pollPending())
        }
        return display
    }

    fun pollPending(nativeHandle: Long): SurfaceCurrentStatsDisplay {
        if (issued.values.any { it.terminal == null }) {
            consumePolledState(adapter.poll(nativeHandle))
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

        when (cycle.poll.kind) {
            GsplatSurfaceCurrentStatsPollKind.EMPTY -> Unit
            GsplatSurfaceCurrentStatsPollKind.UNSAMPLED -> {
                val status = checkNotNull(cycle.poll.requestStatus)
                val terminal = SurfaceCurrentStatsTerminal.Unavailable(status)
                recordPreTicketTerminal(intent.binding, terminal)
                outstanding = null
            }
            GsplatSurfaceCurrentStatsPollKind.READY ->
                recordReady(checkNotNull(cycle.poll.receipt))
            else -> recordFailure(checkNotNull(cycle.poll.failure))
        }
        val cycleState = cycle.state
        if (cycleState is GsplatSurfaceCurrentStatsState.Rejected) {
            recordRejection(cycleState.reason, cycleState.ticket, intent.binding)
        }
        publishState(cycleState)
    }

    internal fun consumePolledState(state: GsplatSurfaceCurrentStatsState) {
        when (state) {
            is GsplatSurfaceCurrentStatsState.Ready -> recordReady(state.receipt)
            is GsplatSurfaceCurrentStatsState.Failed -> recordFailure(state.failure)
            is GsplatSurfaceCurrentStatsState.Rejected ->
                recordRejection(state.reason, state.ticket, null)
            else -> Unit
        }
        publishState(state)
    }

    fun benchmarkTerminalsComplete(expectedSamples: Int): Boolean =
        (0 until expectedSamples).all { index ->
            val record = samples[index]
            record == null || record.ticket == null || record.terminal != null
        }

    fun strictRecords(expectedSamples: Int): List<SurfaceCurrentStatsSampleRecord> {
        check(rejections.isEmpty()) {
            "current-stats consumer rejected evidence: ${rejections.joinToString()}"
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
        issued[ticket] = IssuedTicket(binding, identity)
        binding.sampleIndex?.let { sampleIndex ->
            val record = checkNotNull(samples[sampleIndex])
            record.submissionIssued = true
            record.ticket = ticket
            record.identity = identity
        }
    }

    private fun recordReady(receipt: GsplatSurfaceCurrentStatsReceipt) {
        val issuedTicket = issued[receipt.ticket]
        if (issuedTicket == null) {
            recordRejection("ready_without_issued_ticket", receipt.ticket, null)
            return
        }
        if (issuedTicket.identity != receipt.identity) {
            recordRejection("ready_identity_drift", receipt.ticket, issuedTicket.binding)
            return
        }
        if (issuedTicket.terminal != null) {
            recordRejection("duplicate_ready_terminal", receipt.ticket, issuedTicket.binding)
            return
        }
        val terminal = SurfaceCurrentStatsTerminal.Ready(receipt)
        issuedTicket.terminal = terminal
        issuedTicket.binding.sampleIndex?.let { sampleIndex ->
            checkNotNull(samples[sampleIndex]).terminal = terminal
        }
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
        if (issuedTicket.terminal != null) {
            recordRejection("duplicate_failure_terminal", failure.ticket, issuedTicket.binding)
            return
        }
        val terminal = SurfaceCurrentStatsTerminal.Failure(failure)
        issuedTicket.terminal = terminal
        issuedTicket.binding.sampleIndex?.let { sampleIndex ->
            checkNotNull(samples[sampleIndex]).terminal = terminal
        }
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
        rejections += detail
        val resolvedBinding = binding ?: ticket?.let { issued[it]?.binding }
        resolvedBinding?.sampleIndex?.let { sampleIndex ->
            checkNotNull(samples[sampleIndex]).terminal =
                SurfaceCurrentStatsTerminal.Rejected(reason, ticket)
        }
    }

    private fun publishState(state: GsplatSurfaceCurrentStatsState) {
        val next = when (state) {
            is GsplatSurfaceCurrentStatsState.Ready -> {
                val binding = issued[state.receipt.ticket]?.binding
                if (binding != null && binding == latestBinding && outstanding == null) {
                    SurfaceCurrentStatsDisplay.Ready(binding, state.receipt)
                } else {
                    SurfaceCurrentStatsDisplay.Unavailable(
                        "newer_sample_pending",
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

    private fun pendingTicketCount(): Int =
        issued.values.count { it.terminal == null }
}
