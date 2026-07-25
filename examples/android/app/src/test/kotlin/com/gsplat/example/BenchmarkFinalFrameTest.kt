package com.gsplat.example

import java.io.File
import java.nio.file.Files
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class BenchmarkFinalFrameTest {
    @Test
    fun absentRequestPreservesOrdinaryBenchmarkPath() {
        val directory = Files.createTempDirectory("gsplat-final-frame").toFile()
        try {
            assertNull(BenchmarkFinalFrameRequest.parse(true, null, directory))
            assertNull(BenchmarkFinalFrameRequest.parse(true, "  ", directory))
        } finally {
            directory.deleteRecursively()
        }
    }

    @Test
    fun requestRequiresBenchmarkAndFixedAppPrivatePath() {
        val directory = Files.createTempDirectory("gsplat-final-frame").toFile()
        val expected = File(directory, BENCHMARK_FINAL_FRAME_NAME)
        try {
            val request = BenchmarkFinalFrameRequest.parse(true, expected.path, directory)
            assertEquals(expected.canonicalFile, request?.outputFile)
            assertFails("requires benchmark mode") {
                BenchmarkFinalFrameRequest.parse(false, expected.path, directory)
            }
            assertFails("fixed app-private destination") {
                BenchmarkFinalFrameRequest.parse(true, File(directory, "other.png").path, directory)
            }
        } finally {
            directory.deleteRecursively()
        }
    }

    @Test
    fun launchPreparationRemovesStalePublishedAndTemporaryFiles() {
        val directory = Files.createTempDirectory("gsplat-final-frame").toFile()
        val output = File(directory, BENCHMARK_FINAL_FRAME_NAME)
        try {
            val request = checkNotNull(
                BenchmarkFinalFrameRequest.parse(true, output.path, directory)
            )
            output.writeText("stale")
            request.temporaryFile().writeText("partial")
            request.prepareForLaunch()
            assertFalse(output.exists())
            assertFalse(request.temporaryFile().exists())
        } finally {
            directory.deleteRecursively()
        }
    }

    @Test
    fun formalProtocolRejectsReprojectionWrongSizeAndPagedGeometry() {
        val directory = Files.createTempDirectory("gsplat-final-frame").toFile()
        val output = File(directory, BENCHMARK_FINAL_FRAME_NAME)
        try {
            val request = checkNotNull(
                BenchmarkFinalFrameRequest.parse(true, output.path, directory)
            )
            requireFormalFinalFrameProtocol(
                request,
                requireTraceDisplayMatch = true,
                traceWidth = BENCHMARK_FINAL_FRAME_WIDTH,
                traceHeight = BENCHMARK_FINAL_FRAME_HEIGHT,
                geometryPath = "packed"
            )
            assertFails("exact trace/display") {
                requireFormalFinalFrameProtocol(
                    request,
                    false,
                    BENCHMARK_FINAL_FRAME_WIDTH,
                    BENCHMARK_FINAL_FRAME_HEIGHT,
                    "packed"
                )
            }
            assertFails("2412x1080 trace") {
                requireFormalFinalFrameProtocol(request, true, 640, 480, "packed")
            }
            assertFails("full-resident") {
                requireFormalFinalFrameProtocol(
                    request,
                    true,
                    BENCHMARK_FINAL_FRAME_WIDTH,
                    BENCHMARK_FINAL_FRAME_HEIGHT,
                    "paged"
                )
            }
        } finally {
            directory.deleteRecursively()
        }
    }

    @Test
    fun terminalEvidenceCaptureAndPublicationHaveFixedOrder() {
        val calls = ArrayList<String>()
        val result = finalizeBenchmarkArtifact(
            prepareEvidence = {
                calls += "validate-terminal-evidence"
                PreparedBenchmarkArtifact("BENCHMARK_RESULT ok", listOf("manifest" to "{}"))
            },
            captureFinalFrame = { calls += "capture-presented-surface" },
            publishEvidence = { calls += "publish-logs" }
        )
        assertEquals("BENCHMARK_RESULT ok", result.getOrThrow())
        assertEquals(
            listOf("validate-terminal-evidence", "capture-presented-surface", "publish-logs"),
            calls
        )
    }

    @Test
    fun captureFailurePublishesNoCompletionEvidence() {
        val calls = ArrayList<String>()
        val result = finalizeBenchmarkArtifact(
            prepareEvidence = {
                calls += "validate-terminal-evidence"
                PreparedBenchmarkArtifact("BENCHMARK_RESULT ok", emptyList())
            },
            captureFinalFrame = {
                calls += "capture-presented-surface"
                error("copy failed")
            },
            publishEvidence = { calls += "publish-logs" }
        )
        assertTrue(result.isFailure)
        assertEquals(
            listOf("validate-terminal-evidence", "capture-presented-surface"),
            calls
        )
    }

    private fun assertFails(message: String, block: () -> Unit) {
        val error = runCatching(block).exceptionOrNull()
        assertTrue(error is IllegalStateException)
        assertTrue(error?.message?.contains(message) == true)
    }
}
