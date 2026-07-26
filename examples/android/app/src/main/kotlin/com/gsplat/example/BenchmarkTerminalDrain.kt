package com.gsplat.example

/**
 * Boundedly drains terminal benchmark receipts without issuing more render work.
 *
 * Rendering during this phase can create another measured order ticket and move
 * the completion goal indefinitely. Callers must first advance callbacks for
 * already-submitted work through [pumpCallbacks], then consume only existing
 * terminal lanes through [pollReceipts] and [pollCurrentStats].
 */
internal fun drainBenchmarkTerminalReceipts(
    maxPolls: Int,
    terminalsComplete: () -> Boolean,
    pumpCallbacks: () -> Boolean,
    pollReceipts: () -> Boolean,
    pollCurrentStats: () -> Boolean,
    yieldAfterIncompletePoll: () -> Unit = {}
): Boolean {
    require(maxPolls >= 0) { "benchmark terminal poll bound must be non-negative" }
    if (terminalsComplete()) return true

    repeat(maxPolls) {
        if (!pumpCallbacks() || !pollReceipts() || !pollCurrentStats()) return false
        if (terminalsComplete()) return true
        yieldAfterIncompletePoll()
    }
    return terminalsComplete()
}

/**
 * Requires the strict order/current-stats receipt join before artifact output.
 *
 * Exact refreshed frames project the renderer-owned current-stats ticket into
 * the immutable order terminal lane. Frames without an order refresh must keep
 * the order lane explicitly empty; they may not borrow another frame's ticket.
 */
internal fun requireStrictFrameTicketJoin(
    frameIndex: Int,
    orderRefreshed: Boolean,
    orderSubmissionTicket: Long?,
    currentStatsTicket: Long
) {
    require(frameIndex >= 0) { "strict frame index must be non-negative" }
    check(currentStatsTicket > 0L) {
        "strict frame $frameIndex has an invalid current-stats ticket"
    }
    if (orderRefreshed) {
        check(orderSubmissionTicket == currentStatsTicket) {
            "strict frame $frameIndex refreshed order/current-stats ticket identity drifted"
        }
    } else {
        check(orderSubmissionTicket == null) {
            "strict frame $frameIndex without an order refresh must use the explicit no-ticket state"
        }
    }
}
