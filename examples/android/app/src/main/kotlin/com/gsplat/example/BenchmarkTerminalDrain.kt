package com.gsplat.example

/**
 * Boundedly drains terminal benchmark receipts without issuing more render work.
 *
 * Rendering during this phase can create another measured order ticket and move
 * the completion goal indefinitely. Callers must therefore advance only the
 * existing receipt pumps from [pollReceipts] and [pollCurrentStats].
 */
internal fun drainBenchmarkTerminalReceipts(
    maxPolls: Int,
    terminalsComplete: () -> Boolean,
    pollReceipts: () -> Boolean,
    pollCurrentStats: () -> Boolean,
    yieldAfterIncompletePoll: () -> Unit = {}
): Boolean {
    require(maxPolls >= 0) { "benchmark terminal poll bound must be non-negative" }
    if (terminalsComplete()) return true

    repeat(maxPolls) {
        if (!pollReceipts() || !pollCurrentStats()) return false
        if (terminalsComplete()) return true
        yieldAfterIncompletePoll()
    }
    return terminalsComplete()
}
