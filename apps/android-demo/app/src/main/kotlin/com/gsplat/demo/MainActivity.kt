package com.gsplat.demo

import android.app.Activity
import android.graphics.Color
import android.os.Bundle
import android.util.Log
import android.view.Surface
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.widget.FrameLayout
import android.widget.TextView
import java.io.File
import kotlin.math.max
import kotlin.math.roundToInt

class MainActivity : Activity(), SurfaceHolder.Callback {
    private val renderLock = Object()
    @Volatile private var running = false
    @Volatile private var renderThread: Thread? = null
    private var nativeRenderer = 0L
    private lateinit var datasetPath: String
    private lateinit var statusText: TextView
    private var latestStatus = "state=waiting_for_surface"
    private var surfaceSizeLabel = "pending"

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        val flowerDataset = File(filesDir, "flowers_1.ply")
        datasetPath = if (flowerDataset.exists()) {
            flowerDataset.absolutePath
        } else {
            File(filesDir, "minimal_ascii.ply").absolutePath.also(::writeDataset)
        }

        val renderSurfaceSize = preferredSurfaceSize()
        surfaceSizeLabel = "${renderSurfaceSize.first}x${renderSurfaceSize.second}"
        val surfaceView = SurfaceView(this).apply {
            holder.addCallback(this@MainActivity)
            holder.setFixedSize(renderSurfaceSize.first, renderSurfaceSize.second)
        }
        statusText = TextView(this).apply {
            setTextColor(Color.WHITE)
            setBackgroundColor(0x66000000)
            text = buildStatusText(latestStatus)
        }

        setContentView(
            FrameLayout(this).apply {
                addView(
                    surfaceView,
                    FrameLayout.LayoutParams(
                        FrameLayout.LayoutParams.MATCH_PARENT,
                        FrameLayout.LayoutParams.MATCH_PARENT
                    )
                )
                addView(
                    statusText,
                    FrameLayout.LayoutParams(
                        FrameLayout.LayoutParams.MATCH_PARENT,
                        FrameLayout.LayoutParams.WRAP_CONTENT
                    )
                )
            }
        )
    }

    override fun surfaceCreated(holder: SurfaceHolder) {
        Log.i(TAG, "surfaceCreated")
    }

    override fun surfaceChanged(
        holder: SurfaceHolder,
        format: Int,
        width: Int,
        height: Int
    ) {
        if (width <= 0 || height <= 0) {
            return
        }

        Log.i(TAG, "surfaceChanged width=$width height=$height format=$format")
        surfaceSizeLabel = "${width}x${height}"
        updateStatus("state=surface_changed size=${width}x$height")
        synchronized(renderLock) {
            if (nativeRenderer != 0L) {
                val rc = NativeBridge.resizeSurfaceRenderer(nativeRenderer, width, height)
                updateStatus("state=resized size=${width}x$height rc=$rc")
                return
            }
        }

        startRenderer(holder.surface, width, height)
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        Log.i(TAG, "surfaceDestroyed")
        updateStatus("state=surface_destroyed")
        stopRenderer()
    }

    override fun onDestroy() {
        stopRenderer()
        super.onDestroy()
    }

    private fun startRenderer(surface: Surface, width: Int, height: Int) {
        if (renderThread != null) {
            return
        }

        running = true
        renderThread = Thread(
            {
                Log.i(TAG, "createSurfaceRenderer start size=${width}x$height dataset=$datasetPath")
                updateStatus("state=creating size=${width}x$height")
                val handle = NativeBridge.createSurfaceRenderer(surface, datasetPath, width, height)
                if (handle == 0L) {
                    Log.e(TAG, "createSurfaceRenderer failed")
                    running = false
                    updateStatus("state=create_failed")
                    return@Thread
                }

                Log.i(TAG, "createSurfaceRenderer ok handle=$handle")
                synchronized(renderLock) {
                    nativeRenderer = handle
                }

                try {
                    var frameCount = 0L
                    var consecutiveErrors = 0L
                    var lastStatusAt = 0L
                    val stats = LongArray(6)
                    while (running && !Thread.currentThread().isInterrupted) {
                        val rc = synchronized(renderLock) {
                            NativeBridge.renderSurfaceFrame(handle)
                        }
                        frameCount += 1
                        if (rc != 0) {
                            consecutiveErrors += 1
                            if (consecutiveErrors == 1L || consecutiveErrors % ERROR_STATUS_INTERVAL == 0L) {
                                Log.e(TAG, "renderSurfaceFrame failed rc=$rc")
                                updateStatus("state=render_error rc=$rc frames=$frameCount")
                            }
                        } else {
                            consecutiveErrors = 0L
                            val now = System.nanoTime()
                            if (now - lastStatusAt > STATUS_INTERVAL_NS) {
                                lastStatusAt = now
                                val statsRc = NativeBridge.getSurfaceStats(handle, stats)
                                val detail = if (statsRc == 0) {
                                    "state=rendering frames=$frameCount " +
                                        "visible=${stats[0]} drawn=${stats[1]}/${stats[0]} " +
                                        "frame=${formatMicros(stats[2])}ms " +
                                        "sort=${formatMicros(stats[4])}ms"
                                } else {
                                    "state=rendering frames=$frameCount stats_rc=$statsRc"
                                }
                                Log.i(TAG, detail)
                                updateStatus(detail)
                            }
                        }
                        Thread.sleep(16)
                    }
                } finally {
                    synchronized(renderLock) {
                        if (nativeRenderer == handle) {
                            nativeRenderer = 0L
                        }
                    }
                    Log.i(TAG, "destroySurfaceRenderer handle=$handle")
                    NativeBridge.destroySurfaceRenderer(handle)
                }
            },
            "gsplat-surface-render"
        ).also { it.start() }
    }

    private fun stopRenderer() {
        running = false
        val thread = renderThread
        renderThread = null
        thread?.interrupt()
        thread?.join(1000)
    }

    private fun writeDataset(datasetPath: String) {
        runCatching {
            File(datasetPath).writeText(MINIMAL_PLY)
        }
    }

    private fun preferredSurfaceSize(): Pair<Int, Int> {
        val metrics = resources.displayMetrics
        val width = metrics.widthPixels.coerceAtLeast(1)
        val height = metrics.heightPixels.coerceAtLeast(1)
        val maxSide = MAX_SURFACE_SIDE
        if (width <= maxSide && height <= maxSide) {
            return width to height
        }

        val scale = maxSide.toDouble() / max(width, height).toDouble()
        return max(1, (width * scale).roundToInt()) to max(1, (height * scale).roundToInt())
    }

    private fun updateStatus(status: String) {
        latestStatus = status
        runOnUiThread {
            statusText.text = buildStatusText(latestStatus)
        }
    }

    private fun buildStatusText(status: String): String = buildString {
        appendLine("gsplat android demo")
        appendLine("abi=${NativeBridge.versionMajor()}.${NativeBridge.versionMinor()}")
        appendLine("surface=wgpu realtime ${surfaceSizeLabel}")
        appendLine(status)
        append("dataset=$datasetPath")
    }

    private fun formatMicros(value: Long): String =
        String.format("%.2f", value.toDouble() / 1000.0)

    private companion object {
        private const val TAG = "GsplatDemo"
        private const val STATUS_INTERVAL_NS = 500_000_000L
        private const val ERROR_STATUS_INTERVAL = 120L
        private const val MAX_SURFACE_SIDE = 1600

        private val MINIMAL_PLY = """
            ply
            format ascii 1.0
            element vertex 1
            property float x
            property float y
            property float z
            property float opacity
            property float scale_0
            property float scale_1
            property float scale_2
            property float rot_0
            property float rot_1
            property float rot_2
            property float rot_3
            property float f_dc_0
            property float f_dc_1
            property float f_dc_2
            end_header
            0.0 0.0 0.5 0.9 1.0 1.0 1.0 1.0 0.0 0.0 0.0 0.9 0.2 0.1
        """.trimIndent() + "\n"
    }
}
