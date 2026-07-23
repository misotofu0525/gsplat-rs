package com.gsplat.example

internal object BenchmarkUiState {
    fun completeSceneStatus(
        residentSplats: String?,
        drawnSplats: String?,
        avgFrameMs: String?
    ): String = listOfNotNull(
        "COMPLETE",
        residentSplats?.let { "$it LOADED" },
        drawnSplats?.let { "$it DRAWN" },
        avgFrameMs?.let { "$it MS" }
    ).joinToString("  ·  ")

    fun shouldPublishPeriodicRenderStatus(
        running: Boolean,
        elapsedSinceLastStatusNs: Long,
        statusIntervalNs: Long
    ): Boolean {
        require(statusIntervalNs >= 0L)
        return running && elapsedSinceLastStatusNs > statusIntervalNs
    }
}
