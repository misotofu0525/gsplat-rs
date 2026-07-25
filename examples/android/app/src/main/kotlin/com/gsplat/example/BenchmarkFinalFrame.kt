package com.gsplat.example

import java.io.File

internal const val BENCHMARK_FINAL_FRAME_NAME = "benchmark-final-frame.png"
internal const val BENCHMARK_FINAL_FRAME_WIDTH = 2412
internal const val BENCHMARK_FINAL_FRAME_HEIGHT = 1080

/** Fixed, app-private destination for an opt-in formal benchmark capture. */
internal class BenchmarkFinalFrameRequest private constructor(
    val outputFile: File
) {
    fun prepareForLaunch() {
        val temporary = temporaryFile()
        check(!temporary.exists() || temporary.delete()) {
            "cannot remove stale benchmark final-frame temporary file"
        }
        check(!outputFile.exists() || outputFile.delete()) {
            "cannot remove stale benchmark final frame"
        }
        check(!outputFile.exists()) { "stale benchmark final frame remains after cleanup" }
    }

    fun temporaryFile(): File = File(outputFile.parentFile, "${outputFile.name}.pending")

    companion object {
        fun parse(
            benchmarkEnabled: Boolean,
            requestedPath: String?,
            appFilesDir: File
        ): BenchmarkFinalFrameRequest? {
            val value = requestedPath?.trim()?.takeIf(String::isNotEmpty) ?: return null
            check(benchmarkEnabled) { "final-frame capture requires benchmark mode" }
            val expected = File(appFilesDir, BENCHMARK_FINAL_FRAME_NAME).canonicalFile
            check(File(value).canonicalFile == expected) {
                "benchmark final-frame path must be the fixed app-private destination"
            }
            return BenchmarkFinalFrameRequest(expected)
        }
    }
}

internal data class PreparedBenchmarkArtifact(
    val resultLine: String,
    val records: List<Pair<String, String>>
)

internal fun requireFormalFinalFrameProtocol(
    request: BenchmarkFinalFrameRequest?,
    requireTraceDisplayMatch: Boolean,
    traceWidth: Int?,
    traceHeight: Int?,
    geometryPath: String
) {
    if (request == null) return
    check(requireTraceDisplayMatch) {
        "final-frame capture requires exact trace/display matching"
    }
    check(
        traceWidth == BENCHMARK_FINAL_FRAME_WIDTH &&
            traceHeight == BENCHMARK_FINAL_FRAME_HEIGHT
    ) {
        "final-frame capture requires the formal " +
            "${BENCHMARK_FINAL_FRAME_WIDTH}x$BENCHMARK_FINAL_FRAME_HEIGHT trace"
    }
    check(geometryPath != "paged") {
        "final-frame capture requires full-resident Direct or Packed geometry"
    }
}

/**
 * Publishes terminal benchmark evidence only after an optional final-frame
 * capture succeeds. A failed capture therefore cannot leave completion logs.
 */
internal fun finalizeBenchmarkArtifact(
    prepareEvidence: () -> PreparedBenchmarkArtifact,
    captureFinalFrame: (() -> Unit)?,
    publishEvidence: (PreparedBenchmarkArtifact) -> Unit
): Result<String> = runCatching {
    val prepared = prepareEvidence()
    captureFinalFrame?.invoke()
    publishEvidence(prepared)
    prepared.resultLine
}
