package com.gsplat.example

import android.graphics.Bitmap
import android.os.Handler
import android.os.Looper
import android.view.PixelCopy
import android.view.SurfaceView
import java.io.FileOutputStream
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/** Copies the renderer's last presented Surface buffer into an app-private PNG. */
internal class BenchmarkSurfaceCapture(
    private val surfaceView: SurfaceView
) {
    fun capture(
        request: BenchmarkFinalFrameRequest,
        width: Int,
        height: Int
    ) {
        check(width == BENCHMARK_FINAL_FRAME_WIDTH && height == BENCHMARK_FINAL_FRAME_HEIGHT) {
            "formal final-frame capture requires " +
                "${BENCHMARK_FINAL_FRAME_WIDTH}x$BENCHMARK_FINAL_FRAME_HEIGHT, got ${width}x$height"
        }
        val surface = surfaceView.holder.surface
        check(surface.isValid) { "benchmark Surface is not valid for capture" }
        check(surfaceView.width == width && surfaceView.height == height) {
            "benchmark SurfaceView size does not match its native drawable"
        }
        val surfaceFrame = surfaceView.holder.surfaceFrame
        check(surfaceFrame.width() == width && surfaceFrame.height() == height) {
            "benchmark Surface buffer size does not match its native drawable"
        }
        if (request.outputFile.exists()) {
            check(request.outputFile.delete()) {
                "benchmark final frame appeared before capture and cannot be removed"
            }
            error("benchmark final frame appeared before capture")
        }

        val bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
        var copyCompleted = false
        try {
            val completed = CountDownLatch(1)
            var copyResult = PixelCopy.ERROR_UNKNOWN
            PixelCopy.request(
                surface,
                bitmap,
                { result ->
                    copyResult = result
                    completed.countDown()
                },
                Handler(Looper.getMainLooper())
            )
            copyCompleted = completed.await(CAPTURE_TIMEOUT_SECONDS, TimeUnit.SECONDS)
            check(copyCompleted) {
                "timed out waiting for benchmark Surface capture"
            }
            check(copyResult == PixelCopy.SUCCESS) {
                "benchmark Surface capture failed with PixelCopy result $copyResult"
            }
            check(bitmap.width == width && bitmap.height == height) {
                "benchmark Surface capture dimensions changed unexpectedly"
            }
            writeAtomically(request, bitmap)
        } catch (error: Throwable) {
            request.temporaryFile().delete()
            request.outputFile.delete()
            throw error
        } finally {
            // PixelCopy has no cancellation API. If it times out, retain the
            // Bitmap until the system's outstanding request releases it.
            if (copyCompleted) {
                bitmap.recycle()
            }
        }
    }

    private fun writeAtomically(request: BenchmarkFinalFrameRequest, bitmap: Bitmap) {
        val temporary = request.temporaryFile()
        check(!temporary.exists() || temporary.delete()) {
            "cannot clear benchmark final-frame temporary file"
        }
        try {
            FileOutputStream(temporary).use { output ->
                check(bitmap.compress(Bitmap.CompressFormat.PNG, 100, output)) {
                    "failed to encode benchmark final frame as PNG"
                }
                output.flush()
                output.fd.sync()
            }
            check(!request.outputFile.exists()) { "benchmark final frame appeared before publish" }
            check(temporary.renameTo(request.outputFile)) {
                "failed to atomically publish benchmark final frame"
            }
        } catch (error: Throwable) {
            temporary.delete()
            request.outputFile.delete()
            throw error
        }
    }

    private companion object {
        const val CAPTURE_TIMEOUT_SECONDS = 10L
    }
}
