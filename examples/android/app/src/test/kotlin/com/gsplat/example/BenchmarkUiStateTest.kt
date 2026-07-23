package com.gsplat.example

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class BenchmarkUiStateTest {
    @Test
    fun completeStatusDistinguishesResidentAndDrawnCounts() {
        assertEquals(
            "COMPLETE  ·  2541226 LOADED  ·  1041477 DRAWN  ·  159.837 MS",
            BenchmarkUiState.completeSceneStatus(
                residentSplats = "2541226",
                drawnSplats = "1041477",
                avgFrameMs = "159.837"
            )
        )
    }

    @Test
    fun completedBenchmarkIsNotOverwrittenByDuePeriodicStatus() {
        assertFalse(
            BenchmarkUiState.shouldPublishPeriodicRenderStatus(
                running = false,
                elapsedSinceLastStatusNs = 501_000_000L,
                statusIntervalNs = 500_000_000L
            )
        )
    }

    @Test
    fun runningBenchmarkPublishesPeriodicStatusWhenDue() {
        assertTrue(
            BenchmarkUiState.shouldPublishPeriodicRenderStatus(
                running = true,
                elapsedSinceLastStatusNs = 501_000_000L,
                statusIntervalNs = 500_000_000L
            )
        )
    }
}
