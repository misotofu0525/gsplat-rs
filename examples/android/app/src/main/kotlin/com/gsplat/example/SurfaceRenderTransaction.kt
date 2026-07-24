package com.gsplat.example

internal data class SurfaceRenderTransactionResult(
    val commandRc: Int,
    val commandFailureClosedCurrentStats: Boolean,
    val requestAttempted: Boolean,
    val requestSucceeded: Boolean,
    val requestError: Throwable?,
    val renderRc: Int?,
    val reconciliationAttempted: Boolean,
    val reconciliationError: Throwable?
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
 * now-determined frame, then renders and reconciles its submission plus one
 * non-blocking poll. The same lock is shared with Surface resize/destroy.
 */
internal fun performSurfaceRenderTransaction(
    renderLock: Any,
    applyCommand: () -> Int,
    closeCurrentStatsOnCommandFailure: () -> Boolean,
    requestCurrentStats: (() -> Unit)?,
    stopOnRequestFailure: Boolean,
    render: () -> Int,
    observeRequestedRenderFailure: () -> Unit,
    reconcileAfterSuccessfulRender: () -> Unit
): SurfaceRenderTransactionResult = synchronized(renderLock) {
    val commandRc = applyCommand()
    if (commandRc != 0) {
        return@synchronized SurfaceRenderTransactionResult(
            commandRc = commandRc,
            commandFailureClosedCurrentStats = closeCurrentStatsOnCommandFailure(),
            requestAttempted = false,
            requestSucceeded = false,
            requestError = null,
            renderRc = null,
            reconciliationAttempted = false,
            reconciliationError = null
        )
    }

    var requestError: Throwable? = null
    var requestSucceeded = false
    if (requestCurrentStats != null) {
        runCatching(requestCurrentStats)
            .onSuccess { requestSucceeded = true }
            .onFailure { requestError = it }
        if (requestError != null && stopOnRequestFailure) {
            return@synchronized SurfaceRenderTransactionResult(
                commandRc = 0,
                commandFailureClosedCurrentStats = false,
                requestAttempted = true,
                requestSucceeded = false,
                requestError = requestError,
                renderRc = null,
                reconciliationAttempted = false,
                reconciliationError = null
            )
        }
    }

    val renderRc = render()
    if (renderRc != 0 && requestSucceeded) {
        observeRequestedRenderFailure()
    }
    var reconciliationError: Throwable? = null
    val reconciliationAttempted = renderRc == 0
    if (reconciliationAttempted) {
        runCatching(reconcileAfterSuccessfulRender)
            .onFailure { reconciliationError = it }
    }

    SurfaceRenderTransactionResult(
        commandRc = 0,
        commandFailureClosedCurrentStats = false,
        requestAttempted = requestCurrentStats != null,
        requestSucceeded = requestSucceeded,
        requestError = requestError,
        renderRc = renderRc,
        reconciliationAttempted = reconciliationAttempted,
        reconciliationError = reconciliationError
    )
}
