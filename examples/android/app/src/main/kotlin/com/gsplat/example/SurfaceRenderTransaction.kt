package com.gsplat.example

internal data class SurfaceRenderTransactionResult(
    val commandRc: Int,
    val requestAttempted: Boolean,
    val requestSucceeded: Boolean,
    val requestError: Throwable?,
    val renderRc: Int?
) {
    val rc: Int
        get() = when {
            commandRc != 0 -> commandRc
            renderRc != null -> renderRc
            requestError != null -> Int.MIN_VALUE
            else -> error("surface render transaction ended without a terminal result")
        }
}

/**
 * Runs the fallible camera/resize command before binding current-stats to the
 * now-determined frame, then renders it. The caller holds the renderer lock for
 * this entire transaction.
 */
internal fun performSurfaceRenderTransaction(
    applyCommand: () -> Int,
    requestCurrentStats: (() -> Unit)?,
    stopOnRequestFailure: Boolean,
    render: () -> Int
): SurfaceRenderTransactionResult {
    val commandRc = applyCommand()
    if (commandRc != 0) {
        return SurfaceRenderTransactionResult(
            commandRc = commandRc,
            requestAttempted = false,
            requestSucceeded = false,
            requestError = null,
            renderRc = null
        )
    }

    var requestError: Throwable? = null
    var requestSucceeded = false
    if (requestCurrentStats != null) {
        runCatching(requestCurrentStats)
            .onSuccess { requestSucceeded = true }
            .onFailure { requestError = it }
        if (requestError != null && stopOnRequestFailure) {
            return SurfaceRenderTransactionResult(
                commandRc = 0,
                requestAttempted = true,
                requestSucceeded = false,
                requestError = requestError,
                renderRc = null
            )
        }
    }

    return SurfaceRenderTransactionResult(
        commandRc = 0,
        requestAttempted = requestCurrentStats != null,
        requestSucceeded = requestSucceeded,
        requestError = requestError,
        renderRc = render()
    )
}
