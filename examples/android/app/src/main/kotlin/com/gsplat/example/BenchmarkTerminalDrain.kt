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
 * Requires independent strict ticket namespaces before artifact output.
 *
 * Current-stats owns the presented-frame identity and S/V/C/D receipt. Order
 * telemetry is optional compatibility evidence with its own ticket namespace;
 * its ticket is validated only when the renderer actually issued one. Neither
 * order refresh state nor numeric ticket equality can manufacture a join.
 */
internal fun requireStrictFrameTicketNamespaces(
    frameIndex: Int,
    orderSubmissionTicket: Long?,
    currentStatsTicket: Long
) {
    require(frameIndex >= 0) { "strict frame index must be non-negative" }
    check(currentStatsTicket > 0L) {
        "strict frame $frameIndex has an invalid current-stats ticket"
    }
    if (orderSubmissionTicket != null) {
        check(orderSubmissionTicket > 0L) {
            "strict frame $frameIndex has an invalid issued order ticket"
        }
    }
}
