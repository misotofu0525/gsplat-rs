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
