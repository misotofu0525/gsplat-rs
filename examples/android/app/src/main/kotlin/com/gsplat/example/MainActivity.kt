package com.gsplat.example

import android.app.Activity
import android.content.Intent
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.net.Uri
import android.os.Bundle
import android.os.Build
import android.os.PowerManager
import android.os.SystemClock
import android.provider.OpenableColumns
import android.util.Log
import android.view.Gravity
import android.view.MotionEvent
import android.view.Surface
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.View
import android.widget.Button
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView
import com.gsplat.android.NativeBridge
import java.io.File
import java.text.SimpleDateFormat
import java.security.MessageDigest
import java.util.Date
import java.util.Locale
import java.util.TimeZone
import java.util.UUID
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.abs
import kotlin.math.hypot
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt

private const val GSPLAT_GEOMETRY_PATH_DIRECT = 0
private const val GSPLAT_GEOMETRY_PATH_PACKED_ATLAS = 1
private const val GSPLAT_GEOMETRY_PATH_PAGED_ACTIVE_ATLAS = 2
private const val GSPLAT_ORDER_BACKEND_CPU = 0
private const val GSPLAT_ORDER_BACKEND_GPU = 1
private const val GSPLAT_ORDER_BACKEND_ADAPTIVE = 2

private fun geometryPathValue(label: String): Int =
    when (label) {
        "packed" -> GSPLAT_GEOMETRY_PATH_PACKED_ATLAS
        "paged" -> GSPLAT_GEOMETRY_PATH_PAGED_ACTIVE_ATLAS
        else -> GSPLAT_GEOMETRY_PATH_DIRECT
    }

private fun geometryPipelineName(label: String): String =
    when (label) {
        "packed" -> "packed_atlas"
        "paged" -> "paged_active_atlas"
        else -> "sorted_index_direct"
    }

private fun orderBackendValue(label: String): Int =
    when (label) {
        "gpu" -> GSPLAT_ORDER_BACKEND_GPU
        "adaptive" -> GSPLAT_ORDER_BACKEND_ADAPTIVE
        else -> GSPLAT_ORDER_BACKEND_CPU
    }

private fun adaptiveStateName(flags: Long): String =
    when ((flags shr 12) and 7L) {
        1L -> "cpu_learning"
        2L -> "cpu_stable"
        3L -> "gpu_probe"
        4L -> "gpu_stable"
        5L -> "cpu_probe"
        6L -> "cooldown"
        else -> "disabled"
    }

class MainActivity : Activity(), SurfaceHolder.Callback {
    private val renderLock = Object()
    private val cameraCommandLock = Object()
    @Volatile private var running = false
    @Volatile private var renderThread: Thread? = null
    private var nativeRenderer = 0L
    private lateinit var datasetPath: String
    private var datasetLabel = "pending"
    private lateinit var statusText: TextView
    private lateinit var sceneTitleText: TextView
    private lateinit var sceneMetaText: TextView
    private lateinit var studioPanel: LinearLayout
    private lateinit var studioButton: Button
    private var currentSurface: Surface? = null
    private var currentSurfaceWidth = 0
    private var currentSurfaceHeight = 0
    private var latestStatus = "state=waiting_for_surface"
    private var surfaceSizeLabel = "pending"
    @Volatile private var cameraStatus = "camera=auto"
    private var gestureMode = GestureMode.None
    private var lastTouchX = 0f
    private var lastTouchY = 0f
    private var lastSpan = 0f
    private var lastFocusX = 0f
    private var lastFocusY = 0f
    private var lastTapAt = 0L
    private var lastTapX = 0f
    private var lastTapY = 0f
    private var pendingResetCamera = false
    private var pendingOrbitYaw = 0f
    private var pendingOrbitPitch = 0f
    private var pendingZoomScale = 1f
    private var pendingPanX = 0f
    private var pendingPanY = 0f
    private var benchmarkConfig = BenchmarkConfig()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        benchmarkConfig = BenchmarkConfig.fromIntent(intent)
        setDataset(resolveInitialDataset())

        val renderSurfaceSize = preferredSurfaceSize()
        surfaceSizeLabel = "${renderSurfaceSize.first}x${renderSurfaceSize.second}"
        val surfaceView = SurfaceView(this).apply {
            holder.addCallback(this@MainActivity)
            holder.setFixedSize(renderSurfaceSize.first, renderSurfaceSize.second)
            setOnTouchListener(::handleTouch)
        }
        val brandText = TextView(this).apply {
            text = "gsplat.rs   /   RUST + WGPU"
            setTextColor(SHOWCASE_ACCENT)
            textSize = 11f
            typeface = Typeface.create("sans-serif", Typeface.BOLD)
            letterSpacing = 0.12f
        }
        val heroText = TextView(this).apply {
            text = "Captured light.\nStill alive."
            setTextColor(SHOWCASE_TEXT)
            textSize = 38f
            typeface = Typeface.create("sans-serif-condensed", Typeface.BOLD)
            setLineSpacing(-dp(4).toFloat(), 0.92f)
        }
        val subtitleText = TextView(this).apply {
            text = "A living Gaussian splat, rendered natively\nby Rust on your phone."
            setTextColor(SHOWCASE_MUTED)
            textSize = 14f
            typeface = Typeface.create("sans-serif", Typeface.NORMAL)
            setLineSpacing(dp(3).toFloat(), 1f)
        }

        sceneTitleText = TextView(this).apply {
            setTextColor(SHOWCASE_TEXT)
            textSize = 15f
            typeface = Typeface.create("sans-serif", Typeface.BOLD)
        }
        sceneMetaText = TextView(this).apply {
            setTextColor(SHOWCASE_MUTED)
            textSize = 10f
            typeface = Typeface.MONOSPACE
            letterSpacing = 0.08f
        }
        val sceneCard = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(16), dp(13), dp(16), dp(13))
            background = roundedBackground(SHOWCASE_GLASS, SHOWCASE_BORDER, 14f)
            addView(sceneTitleText)
            addView(sceneMetaText, LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.WRAP_CONTENT,
                LinearLayout.LayoutParams.WRAP_CONTENT
            ).apply { topMargin = dp(5) })
        }

        statusText = TextView(this).apply {
            setTextColor(SHOWCASE_TEXT)
            textSize = 11f
            typeface = Typeface.MONOSPACE
            isClickable = false
            text = buildStatusText(latestStatus)
        }
        val studioLabel = TextView(this).apply {
            text = "STUDIO / LIVE DIAGNOSTICS"
            setTextColor(SHOWCASE_ACCENT)
            textSize = 10f
            typeface = Typeface.create("sans-serif", Typeface.BOLD)
            letterSpacing = 0.12f
        }
        studioPanel = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            visibility = View.GONE
            setPadding(dp(16), dp(15), dp(16), dp(16))
            background = roundedBackground(Color.argb(235, 11, 13, 12), SHOWCASE_BORDER, 14f)
            addView(studioLabel)
            addView(statusText, LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT
            ).apply { topMargin = dp(10) })
        }

        studioButton = showcaseButton("Studio").apply {
            contentDescription = "Toggle live diagnostics"
            setOnClickListener { toggleStudioPanel() }
        }
        val importButton = Button(this).apply {
            text = "Open PLY  +"
            setTextColor(Color.BLACK)
            textSize = 12f
            typeface = Typeface.create("sans-serif", Typeface.BOLD)
            isAllCaps = false
            minWidth = 0
            minHeight = 0
            setPadding(dp(17), dp(11), dp(17), dp(11))
            background = roundedBackground(SHOWCASE_TEXT, null, 24f)
            setOnClickListener { openPlyPicker() }
        }

        val root = FrameLayout(this).apply {
            setOnTouchListener(::handleTouch)
            addView(
                surfaceView,
                FrameLayout.LayoutParams(
                    FrameLayout.LayoutParams.MATCH_PARENT,
                    FrameLayout.LayoutParams.MATCH_PARENT
                )
            )
            setBackgroundColor(Color.BLACK)
            addView(brandText, FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.WRAP_CONTENT,
                FrameLayout.LayoutParams.WRAP_CONTENT,
                Gravity.TOP or Gravity.START
            ).apply { setMargins(dp(24), dp(25), dp(96), 0) })
            addView(studioButton, FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.WRAP_CONTENT,
                FrameLayout.LayoutParams.WRAP_CONTENT,
                Gravity.TOP or Gravity.END
            ).apply { setMargins(0, dp(14), dp(18), 0) })
            addView(heroText, FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.MATCH_PARENT,
                FrameLayout.LayoutParams.WRAP_CONTENT,
                Gravity.TOP or Gravity.START
            ).apply { setMargins(dp(24), dp(76), dp(42), 0) })
            addView(subtitleText, FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.MATCH_PARENT,
                FrameLayout.LayoutParams.WRAP_CONTENT,
                Gravity.TOP or Gravity.START
            ).apply { setMargins(dp(26), dp(170), dp(42), 0) })
            addView(studioPanel, FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.MATCH_PARENT,
                FrameLayout.LayoutParams.WRAP_CONTENT,
                Gravity.TOP
            ).apply { setMargins(dp(18), dp(68), dp(18), 0) })
            addView(sceneCard, FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.WRAP_CONTENT,
                FrameLayout.LayoutParams.WRAP_CONTENT,
                Gravity.BOTTOM or Gravity.START
            ).apply { setMargins(dp(18), 0, dp(148), dp(20)) })
            addView(
                importButton,
                FrameLayout.LayoutParams(
                    FrameLayout.LayoutParams.WRAP_CONTENT,
                    FrameLayout.LayoutParams.WRAP_CONTENT,
                    Gravity.BOTTOM or Gravity.END
                ).apply {
                    setMargins(0, 0, dp(18), dp(22))
                }
            )
        }
        setContentView(root)
        updateShowcaseOverlay()
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
        currentSurface = holder.surface
        currentSurfaceWidth = width
        currentSurfaceHeight = height
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
        currentSurface = null
        currentSurfaceWidth = 0
        currentSurfaceHeight = 0
        updateStatus("state=surface_destroyed")
        stopRenderer()
    }

    override fun onDestroy() {
        stopRenderer()
        super.onDestroy()
    }

    @Suppress("OVERRIDE_DEPRECATION")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != REQUEST_IMPORT_PLY) {
            return
        }

        val uri = data?.data
        if (resultCode != RESULT_OK || uri == null) {
            updateStatus("state=import_cancelled")
            return
        }

        importPlyFromUri(uri)
    }

    private fun openPlyPicker() {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            type = "*/*"
        }

        @Suppress("DEPRECATION")
        runCatching {
            startActivityForResult(intent, REQUEST_IMPORT_PLY)
        }.onFailure { error ->
            updateStatus("state=import_picker_failed error=${compactMessage(error)}")
        }
    }

    private fun startRenderer(surface: Surface, width: Int, height: Int) {
        if (renderThread != null) {
            return
        }

        clearPendingCameraCommands()
        running = true
        renderThread = Thread(
            {
                Log.i(TAG, "createSurfaceRenderer start size=${width}x$height geometry=${benchmarkConfig.geometryPath} dataset=$datasetPath")
                updateStatus("state=creating size=${width}x$height")
                val createError = IntArray(1)
                val handle = NativeBridge.createSurfaceRendererWithGeometryPath(
                    surface,
                    datasetPath,
                    width,
                    height,
                    geometryPathValue(benchmarkConfig.geometryPath),
                    createError
                )
                if (handle == 0L) {
                    val rc = createError[0]
                    val detail = NativeBridge.lastErrorMessage()
                        .ifBlank { NativeBridge.errorMessage(rc) }
                    val message = detail.replace('\n', ' ').take(240)
                    Log.e(TAG, "createSurfaceRenderer failed rc=$rc error=$detail")
                    running = false
                    updateStatus("state=create_failed rc=$rc error=$message")
                    return@Thread
                }

                Log.i(TAG, "createSurfaceRenderer ok handle=$handle")
                val sortIntervalRc = NativeBridge.setSurfaceSortInterval(
                    handle,
                    benchmarkConfig.sortInterval
                )
                if (sortIntervalRc != 0) {
                    val message = NativeBridge.errorMessage(sortIntervalRc)
                    Log.e(TAG, "setSurfaceSortInterval failed rc=$sortIntervalRc error=$message")
                    NativeBridge.destroySurfaceRenderer(handle)
                    running = false
                    updateStatus("state=create_failed rc=$sortIntervalRc error=$message")
                    return@Thread
                }
                val asyncSortRc = NativeBridge.setSurfaceAsyncSortEnabled(
                    handle,
                    benchmarkConfig.asyncSort
                )
                if (asyncSortRc != 0) {
                    val message = NativeBridge.errorMessage(asyncSortRc)
                    Log.e(TAG, "setSurfaceAsyncSortEnabled failed rc=$asyncSortRc error=$message")
                    NativeBridge.destroySurfaceRenderer(handle)
                    running = false
                    updateStatus("state=create_failed rc=$asyncSortRc error=$message")
                    return@Thread
                }
                val orderBackendRc = BenchmarkBridge.setSurfaceOrderBackend(
                    handle,
                    orderBackendValue(benchmarkConfig.orderBackend)
                )
                if (orderBackendRc != 0) {
                    val message = NativeBridge.errorMessage(orderBackendRc)
                    Log.e(TAG, "setSurfaceOrderBackend failed rc=$orderBackendRc error=$message")
                    NativeBridge.destroySurfaceRenderer(handle)
                    running = false
                    updateStatus("state=create_failed rc=$orderBackendRc error=$message")
                    return@Thread
                }
                val frameLatencyRc = NativeBridge.setSurfaceFrameLatency(
                    handle,
                    benchmarkConfig.frameLatency
                )
                if (frameLatencyRc != 0) {
                    val message = NativeBridge.errorMessage(frameLatencyRc)
                    Log.e(TAG, "setSurfaceFrameLatency failed rc=$frameLatencyRc error=$message")
                    NativeBridge.destroySurfaceRenderer(handle)
                    running = false
                    updateStatus("state=create_failed rc=$frameLatencyRc error=$message")
                    return@Thread
                }
                synchronized(renderLock) {
                    nativeRenderer = handle
                }

                try {
                    var frameCount = 0L
                    var consecutiveErrors = 0L
                    var lastStatusAt = 0L
                    val stats = LongArray(6)
                    val sortStats = LongArray(7)
                    val benchmark = SurfaceBenchmark(benchmarkConfig, currentThermalStatus())
                    while (running && !Thread.currentThread().isInterrupted) {
                        val renderStartNs = System.nanoTime()
                        val rc = synchronized(renderLock) {
                            val cameraRc = if (benchmark.enabled) {
                                NativeBridge.orbitSurfaceRenderer(
                                    handle,
                                    benchmark.config.yawStepRadians,
                                    0f
                                )
                            } else {
                                applyPendingCameraCommands(handle)
                            }
                            if (cameraRc != 0) {
                                Log.e(TAG, "applyPendingCameraCommands failed rc=$cameraRc")
                                cameraRc
                            } else {
                                NativeBridge.renderSurfaceFrame(handle)
                            }
                        }
                        val renderCallNs = System.nanoTime() - renderStartNs
                        frameCount += 1
                        if (rc != 0) {
                            consecutiveErrors += 1
                            if (consecutiveErrors == 1L || consecutiveErrors % ERROR_STATUS_INTERVAL == 0L) {
                                val message = NativeBridge.errorMessage(rc)
                                Log.e(TAG, "renderSurfaceFrame failed rc=$rc error=$message")
                                updateStatus("state=render_error rc=$rc error=$message frames=$frameCount")
                            }
                        } else {
                            consecutiveErrors = 0L
                            val now = System.nanoTime()
                            if (benchmark.enabled) {
                                val statsRc = NativeBridge.getSurfaceStats(handle, stats)
                                val sortStatsRc = NativeBridge.getSurfaceSortStats(handle, sortStats)
                                if (statsRc == 0 && sortStatsRc == 0) {
                                    benchmark.record(stats, sortStats, renderCallNs)
                                    if (benchmark.complete) {
                                        val result = benchmark.resultLine(datasetLabel)
                                        Log.i(TAG, result)
                                        benchmark.artifactLines(
                                            datasetLabel = datasetLabel,
                                            datasetPath = datasetPath,
                                            surfaceWidth = currentSurfaceWidth,
                                            surfaceHeight = currentSurfaceHeight,
                                            density = resources.displayMetrics.density,
                                            refreshHz = display?.refreshRate?.toDouble(),
                                            thermalStatusEnd = currentThermalStatus()
                                        ).forEach { (prefix, json) ->
                                            Log.i(TAG, "$prefix$json")
                                        }
                                        updateStatus("state=benchmark_complete $result")
                                        running = false
                                    }
                                } else {
                                    Log.e(TAG, "benchmark stats failed stats_rc=$statsRc sort_stats_rc=$sortStatsRc")
                                    updateStatus("state=benchmark_stats_error stats_rc=$statsRc sort_stats_rc=$sortStatsRc")
                                    running = false
                                }
                            }
                            if (now - lastStatusAt > STATUS_INTERVAL_NS) {
                                lastStatusAt = now
                                val statsRc = NativeBridge.getSurfaceStats(handle, stats)
                                val detail = if (statsRc == 0) {
                                    val counts = if (benchmarkConfig.geometryPath == "paged") {
                                        "loaded=${stats[0]} drawn=${stats[1]}/${stats[0]}"
                                    } else {
                                        "visible=${stats[0]} drawn=${stats[1]}/${stats[0]}"
                                    }
                                    "state=rendering frames=$frameCount " +
                                        "$counts " +
                                        "frame=${formatMicros(stats[2])}ms " +
                                        "preprocess=${formatMicros(stats[3])}ms " +
                                        "sort=${formatMicros(stats[4])}ms " +
                                        "raster=${formatMicros(stats[5])}ms " +
                                        "call=${formatNanos(renderCallNs)}ms"
                                } else {
                                    "state=rendering frames=$frameCount stats_rc=$statsRc"
                                }
                                Log.i(TAG, detail)
                                updateStatus(detail)
                            }
                        }
                        if (!benchmark.enabled) {
                            val sleepNs = (TARGET_FRAME_INTERVAL_NS - renderCallNs).coerceAtLeast(0L)
                            if (sleepNs > 0L) {
                                Thread.sleep(sleepNs / 1_000_000L, (sleepNs % 1_000_000L).toInt())
                            }
                        }
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

    private fun importPlyFromUri(uri: Uri) {
        updateStatus("state=importing")
        Thread(
            {
                val result = runCatching {
                    val (file, displayName) = copyPlyIntoAppStorage(uri)
                    DatasetSelection(file.absolutePath, "imported:$displayName")
                }

                runOnUiThread {
                    result
                        .onSuccess { selection ->
                            setDataset(selection)
                            updateStatus("state=imported")
                            restartRendererForDataset()
                        }
                        .onFailure { error ->
                            updateStatus("state=import_failed error=${compactMessage(error)}")
                        }
                }
            },
            "gsplat-import-ply"
        ).start()
    }

    private fun copyPlyIntoAppStorage(uri: Uri): Pair<File, String> {
        val displayName = displayNameForUri(uri)
        val destination = File(filesDir, IMPORTED_PLY_NAME)
        val temp = File(filesDir, "$IMPORTED_PLY_NAME.tmp")

        runCatching { temp.delete() }
        val input = contentResolver.openInputStream(uri)
            ?: error("Unable to open selected file")
        input.use { source ->
            temp.outputStream().use { target ->
                source.copyTo(target)
            }
        }

        if (destination.exists() && !destination.delete()) {
            error("Unable to replace previous import")
        }
        if (!temp.renameTo(destination)) {
            temp.copyTo(destination, overwrite = true)
            if (!temp.delete()) {
                Log.w(TAG, "failed to remove temp import ${temp.absolutePath}")
            }
        }

        return destination to displayName
    }

    private fun displayNameForUri(uri: Uri): String {
        var displayName: String? = null
        contentResolver.query(
            uri,
            arrayOf(OpenableColumns.DISPLAY_NAME),
            null,
            null,
            null
        )?.use { cursor ->
            val nameIndex = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
            if (nameIndex >= 0 && cursor.moveToFirst()) {
                displayName = cursor.getString(nameIndex)
            }
        }

        return displayName
            ?.takeIf { it.isNotBlank() }
            ?: uri.lastPathSegment?.substringAfterLast('/')?.takeIf { it.isNotBlank() }
            ?: IMPORTED_PLY_NAME
    }

    private fun restartRendererForDataset() {
        clearPendingCameraCommands()
        cameraStatus = "camera=auto"

        val surface = currentSurface
        val width = currentSurfaceWidth
        val height = currentSurfaceHeight
        stopRenderer()

        if (surface != null && surface.isValid && width > 0 && height > 0) {
            startRenderer(surface, width, height)
        } else {
            updateStatus("state=dataset_ready waiting_for_surface")
        }
    }

    private fun resolveInitialDataset(): DatasetSelection {
        val importedDataset = File(filesDir, IMPORTED_PLY_NAME)
        if (importedDataset.exists()) {
            return DatasetSelection(importedDataset.absolutePath, importedDataset.name)
        }

        val showcaseDataset = File(filesDir, SHOWCASE_PLY_NAME)
        val bundledShowcaseReady = runCatching {
            assets.openFd(SHOWCASE_PLY_NAME).use { descriptor ->
                if (!showcaseDataset.exists() || showcaseDataset.length() != descriptor.length) {
                    val temp = File(filesDir, "$SHOWCASE_PLY_NAME.tmp")
                    temp.delete()
                    assets.open(SHOWCASE_PLY_NAME).use { input ->
                        temp.outputStream().use { output -> input.copyTo(output) }
                    }
                    temp.copyTo(showcaseDataset, overwrite = true)
                    temp.delete()
                }
            }
            true
        }.onFailure {
            Log.i(TAG, "bundled showcase not available; checking local fallbacks")
        }.getOrDefault(false)
        if (bundledShowcaseReady && showcaseDataset.exists()) {
            val bundledLabel = runCatching {
                assets.open(SHOWCASE_LABEL_NAME)
                    .bufferedReader()
                    .use { it.readLine()?.trim() }
            }.getOrNull()?.takeIf { !it.isNullOrBlank() }
            return DatasetSelection(
                showcaseDataset.absolutePath,
                bundledLabel ?: showcaseDataset.name
            )
        }

        val flowerDataset = File(filesDir, "flowers_1.ply")
        if (flowerDataset.exists()) {
            return DatasetSelection(flowerDataset.absolutePath, flowerDataset.name)
        }

        val minimalDataset = File(filesDir, "minimal_ascii.ply")
        if (!minimalDataset.exists()) {
            writeDataset(minimalDataset.absolutePath)
        }
        return DatasetSelection(minimalDataset.absolutePath, minimalDataset.name)
    }

    private fun setDataset(selection: DatasetSelection) {
        datasetPath = selection.path
        datasetLabel = selection.label
    }

    private fun writeDataset(datasetPath: String) {
        runCatching {
            File(datasetPath).writeText(MINIMAL_PLY)
        }
    }

    private fun compactMessage(error: Throwable): String =
        (error.message ?: error::class.java.simpleName)
            .replace('\n', ' ')
            .take(160)

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

    private fun handleTouch(view: View, event: MotionEvent): Boolean {
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                maybeResetCameraFromDoubleTap(event.x, event.y)
                gestureMode = GestureMode.Orbit
                lastTouchX = event.x
                lastTouchY = event.y
            }
            MotionEvent.ACTION_POINTER_DOWN -> {
                if (event.pointerCount >= 2) {
                    gestureMode = GestureMode.Transform
                    lastSpan = pointerSpan(event)
                    lastFocusX = pointerFocusX(event)
                    lastFocusY = pointerFocusY(event)
                }
            }
            MotionEvent.ACTION_MOVE -> {
                if (event.pointerCount >= 2) {
                    handleTransformGesture(view, event)
                } else if (gestureMode == GestureMode.Orbit) {
                    handleOrbitGesture(view, event)
                }
            }
            MotionEvent.ACTION_POINTER_UP -> {
                if (event.pointerCount <= 2) {
                    gestureMode = GestureMode.None
                    lastSpan = 0f
                }
            }
            MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                gestureMode = GestureMode.None
                lastSpan = 0f
            }
        }
        return true
    }

    private fun maybeResetCameraFromDoubleTap(x: Float, y: Float) {
        val now = SystemClock.uptimeMillis()
        val slop = DOUBLE_TAP_SLOP_DP * resources.displayMetrics.density
        val isDoubleTap = now - lastTapAt <= DOUBLE_TAP_TIMEOUT_MS &&
            hypot(x - lastTapX, y - lastTapY) <= slop

        if (isDoubleTap) {
            queueCameraReset()
            lastTapAt = 0L
        } else {
            lastTapAt = now
            lastTapX = x
            lastTapY = y
        }
    }

    private fun handleOrbitGesture(view: View, event: MotionEvent) {
        val size = min(view.width, view.height).coerceAtLeast(1).toFloat()
        val dx = (event.x - lastTouchX) / size
        val dy = (event.y - lastTouchY) / size
        lastTouchX = event.x
        lastTouchY = event.y

        if (abs(dx) < TOUCH_EPSILON && abs(dy) < TOUCH_EPSILON) {
            return
        }

        queueCameraOrbit(-dx * ORBIT_RADIANS_PER_SCREEN, -dy * ORBIT_RADIANS_PER_SCREEN)
    }

    private fun handleTransformGesture(view: View, event: MotionEvent) {
        val span = pointerSpan(event)
        val focusX = pointerFocusX(event)
        val focusY = pointerFocusY(event)
        val width = view.width.coerceAtLeast(1).toFloat()
        val height = view.height.coerceAtLeast(1).toFloat()

        if (lastSpan > PINCH_MIN_SPAN && span > PINCH_MIN_SPAN) {
            val scale = (lastSpan / span).coerceIn(MIN_ZOOM_STEP, MAX_ZOOM_STEP)
            if (abs(scale - 1.0f) > ZOOM_EPSILON) {
                queueCameraZoom(scale)
            }
        }

        val dx = (focusX - lastFocusX) / width
        val dy = (focusY - lastFocusY) / height
        if (abs(dx) > TOUCH_EPSILON || abs(dy) > TOUCH_EPSILON) {
            queueCameraPan(dx, dy)
        }

        lastSpan = span
        lastFocusX = focusX
        lastFocusY = focusY
    }

    private fun queueCameraReset() {
        synchronized(cameraCommandLock) {
            pendingResetCamera = true
            pendingOrbitYaw = 0f
            pendingOrbitPitch = 0f
            pendingZoomScale = 1f
            pendingPanX = 0f
            pendingPanY = 0f
        }
        cameraStatus = "camera=reset"
    }

    private fun queueCameraOrbit(deltaYawRadians: Float, deltaPitchRadians: Float) {
        synchronized(cameraCommandLock) {
            pendingOrbitYaw += deltaYawRadians
            pendingOrbitPitch += deltaPitchRadians
        }
        cameraStatus = "camera=orbit"
    }

    private fun queueCameraZoom(distanceScale: Float) {
        synchronized(cameraCommandLock) {
            pendingZoomScale = (pendingZoomScale * distanceScale).coerceIn(MIN_PENDING_ZOOM, MAX_PENDING_ZOOM)
        }
        cameraStatus = "camera=zoom"
    }

    private fun queueCameraPan(normalizedDeltaX: Float, normalizedDeltaY: Float) {
        synchronized(cameraCommandLock) {
            pendingPanX += normalizedDeltaX
            pendingPanY += normalizedDeltaY
        }
        if (cameraStatus != "camera=zoom") {
            cameraStatus = "camera=pan"
        }
    }

    private fun clearPendingCameraCommands() {
        synchronized(cameraCommandLock) {
            pendingResetCamera = false
            pendingOrbitYaw = 0f
            pendingOrbitPitch = 0f
            pendingZoomScale = 1f
            pendingPanX = 0f
            pendingPanY = 0f
        }
    }

    private fun applyPendingCameraCommands(nativeHandle: Long): Int {
        val command = synchronized(cameraCommandLock) {
            val hasCommand = pendingResetCamera ||
                abs(pendingOrbitYaw) > TOUCH_EPSILON ||
                abs(pendingOrbitPitch) > TOUCH_EPSILON ||
                abs(pendingZoomScale - 1f) > ZOOM_EPSILON ||
                abs(pendingPanX) > TOUCH_EPSILON ||
                abs(pendingPanY) > TOUCH_EPSILON
            if (!hasCommand) {
                null
            } else {
                CameraCommand(
                    reset = pendingResetCamera,
                    orbitYaw = pendingOrbitYaw,
                    orbitPitch = pendingOrbitPitch,
                    zoomScale = pendingZoomScale,
                    panX = pendingPanX,
                    panY = pendingPanY
                ).also {
                    pendingResetCamera = false
                    pendingOrbitYaw = 0f
                    pendingOrbitPitch = 0f
                    pendingZoomScale = 1f
                    pendingPanX = 0f
                    pendingPanY = 0f
                }
            }
        } ?: return 0

        var rc = 0
        if (command.reset) {
            rc = NativeBridge.resetSurfaceCamera(nativeHandle)
            if (rc != 0) {
                cameraStatus = "camera=reset_error rc=$rc"
                return rc
            }
        }
        if (abs(command.orbitYaw) > TOUCH_EPSILON || abs(command.orbitPitch) > TOUCH_EPSILON) {
            rc = NativeBridge.orbitSurfaceRenderer(nativeHandle, command.orbitYaw, command.orbitPitch)
            if (rc != 0) {
                cameraStatus = "camera=orbit_error rc=$rc"
                return rc
            }
        }
        if (abs(command.zoomScale - 1f) > ZOOM_EPSILON) {
            rc = NativeBridge.zoomSurfaceRenderer(nativeHandle, command.zoomScale)
            if (rc != 0) {
                cameraStatus = "camera=zoom_error rc=$rc"
                return rc
            }
        }
        if (abs(command.panX) > TOUCH_EPSILON || abs(command.panY) > TOUCH_EPSILON) {
            rc = NativeBridge.panSurfaceRenderer(nativeHandle, command.panX, command.panY)
            if (rc != 0) {
                cameraStatus = "camera=pan_error rc=$rc"
                return rc
            }
        }

        return 0
    }

    private fun pointerSpan(event: MotionEvent): Float {
        if (event.pointerCount < 2) {
            return 0f
        }
        return hypot(event.getX(0) - event.getX(1), event.getY(0) - event.getY(1))
    }

    private fun pointerFocusX(event: MotionEvent): Float {
        var total = 0f
        for (index in 0 until event.pointerCount) {
            total += event.getX(index)
        }
        return total / event.pointerCount.toFloat()
    }

    private fun pointerFocusY(event: MotionEvent): Float {
        var total = 0f
        for (index in 0 until event.pointerCount) {
            total += event.getY(index)
        }
        return total / event.pointerCount.toFloat()
    }

    private fun updateStatus(status: String) {
        latestStatus = status
        runOnUiThread {
            updateShowcaseOverlay()
        }
    }

    private fun updateShowcaseOverlay() {
        statusText.text = buildStatusText(latestStatus)
        sceneTitleText.text = sceneTitle()
        sceneMetaText.text = compactSceneStatus()
    }

    private fun sceneTitle(): String = when {
        datasetLabel.startsWith("imported:") -> "Imported memory"
        datasetLabel.contains("showcase", ignoreCase = true) ||
            datasetLabel.contains("kitune", ignoreCase = true) -> "Kitsune shrine"
        datasetLabel.contains("flower", ignoreCase = true) -> "Flowers / NVIDIA"
        else -> "Gaussian scene"
    }

    private fun compactSceneStatus(): String {
        if (latestStatus.startsWith("state=rendering")) {
            val drawn = statusValue("drawn")
            val frame = statusValue("frame")
            val splats = if (benchmarkConfig.geometryPath == "paged") {
                drawn?.replace("/", " / ")
            } else {
                drawn?.substringBefore('/')
            }
            return listOfNotNull(
                "LIVE",
                splats?.let { "$it SPLATS" },
                frame?.let { "$it MS" }
            ).joinToString("  ·  ")
        }
        if (latestStatus.contains("failed") || latestStatus.contains("error")) {
            return "ATTENTION  ·  OPEN STUDIO"
        }
        return "LOADING  ·  DRAG TO ORBIT"
    }

    private fun statusValue(key: String): String? =
        Regex("(?:^| )${Regex.escape(key)}=([^ ]+)")
            .find(latestStatus)
            ?.groupValues
            ?.getOrNull(1)

    private fun toggleStudioPanel() {
        val opening = studioPanel.visibility != View.VISIBLE
        studioPanel.visibility = if (opening) View.VISIBLE else View.GONE
        studioButton.text = if (opening) "Close" else "Studio"
    }

    private fun showcaseButton(label: String): Button = Button(this).apply {
        text = label
        setTextColor(SHOWCASE_TEXT)
        textSize = 11f
        typeface = Typeface.create("sans-serif", Typeface.BOLD)
        isAllCaps = false
        minWidth = 0
        minHeight = 0
        setPadding(dp(15), dp(9), dp(15), dp(9))
        background = roundedBackground(SHOWCASE_GLASS, SHOWCASE_BORDER, 22f)
    }

    private fun roundedBackground(fillColor: Int, strokeColor: Int?, radiusDp: Float): GradientDrawable =
        GradientDrawable().apply {
            shape = GradientDrawable.RECTANGLE
            setColor(fillColor)
            cornerRadius = dp(radiusDp.toInt()).toFloat()
            strokeColor?.let { setStroke(dp(1), it) }
        }

    private fun dp(value: Int): Int = (value * resources.displayMetrics.density).roundToInt()

    private fun currentThermalStatus(): Int? =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            getSystemService(PowerManager::class.java)?.currentThermalStatus
        } else {
            null
        }

    private fun buildStatusText(status: String): String = buildString {
        appendLine("gsplat android example")
        appendLine("abi=${NativeBridge.versionMajor()}.${NativeBridge.versionMinor()}")
        appendLine("surface=wgpu realtime ${surfaceSizeLabel}")
        appendLine(status)
        appendLine(cameraStatus)
        appendLine("geometry_pipeline=${geometryPipelineName(benchmarkConfig.geometryPath)}")
        appendLine("order_backend=${benchmarkConfig.orderBackend}")
        if (benchmarkConfig.enabled) {
            appendLine("benchmark=orbit frames=${benchmarkConfig.frames} warmup=${benchmarkConfig.warmupFrames}")
        }
        appendLine("dataset=$datasetLabel")
        append("path=$datasetPath")
    }

    private fun formatMicros(value: Long): String =
        String.format("%.2f", value.toDouble() / 1000.0)

    private fun formatNanos(value: Long): String =
        String.format("%.2f", value.toDouble() / 1_000_000.0)

    private companion object {
        private const val TAG = "GsplatExample"
        private const val STATUS_INTERVAL_NS = 500_000_000L
        private const val ERROR_STATUS_INTERVAL = 120L
        private const val MAX_SURFACE_SIDE = 1600
        private const val ORBIT_RADIANS_PER_SCREEN = 3.2f
        private const val PINCH_MIN_SPAN = 24f
        private const val MIN_ZOOM_STEP = 0.5f
        private const val MAX_ZOOM_STEP = 2.0f
        private const val MIN_PENDING_ZOOM = 0.001f
        private const val MAX_PENDING_ZOOM = 1000.0f
        private const val ZOOM_EPSILON = 0.003f
        private const val TOUCH_EPSILON = 0.0001f
        private const val DOUBLE_TAP_TIMEOUT_MS = 300L
        private const val DOUBLE_TAP_SLOP_DP = 48f
        private const val REQUEST_IMPORT_PLY = 42
        private const val IMPORTED_PLY_NAME = "imported_scene.ply"
        private const val SHOWCASE_PLY_NAME = "showcase.ply"
        private const val SHOWCASE_LABEL_NAME = "showcase.name"
        private const val EXTRA_BENCHMARK = "gsplat_benchmark"
        private const val EXTRA_BENCHMARK_FRAMES = "gsplat_benchmark_frames"
        private const val EXTRA_BENCHMARK_WARMUP_FRAMES = "gsplat_benchmark_warmup_frames"
        private const val EXTRA_BENCHMARK_YAW_STEP = "gsplat_benchmark_yaw_step"
        private const val EXTRA_SURFACE_SORT_INTERVAL = "gsplat_surface_sort_interval"
        private const val EXTRA_SURFACE_ASYNC_SORT = "gsplat_surface_async_sort"
        private const val EXTRA_SURFACE_FRAME_LATENCY = "gsplat_surface_frame_latency"
        private const val EXTRA_SURFACE_GEOMETRY_PATH = "gsplat_geometry_path"
        private const val EXTRA_SURFACE_ORDER_BACKEND = "gsplat_surface_order_backend"
        private const val DEFAULT_BENCHMARK_FRAMES = 120
        private const val DEFAULT_BENCHMARK_WARMUP_FRAMES = 10
        private const val DEFAULT_BENCHMARK_YAW_STEP = 0.001f
        private const val DEFAULT_SURFACE_SORT_INTERVAL = 2
        private const val DEFAULT_SURFACE_ASYNC_SORT = false
        private const val DEFAULT_SURFACE_FRAME_LATENCY = 2
        private const val DEFAULT_SURFACE_GEOMETRY_PATH = "direct"
        private const val DEFAULT_SURFACE_ORDER_BACKEND = "cpu"
        private const val TARGET_FRAME_INTERVAL_NS = 16_666_667L
        private const val BENCHMARK_MANIFEST_PREFIX = "GSPLAT_BENCHMARK_MANIFEST "
        private const val BENCHMARK_FRAME_PREFIX = "GSPLAT_BENCHMARK_FRAME "
        private const val BENCHMARK_SUMMARY_PREFIX = "GSPLAT_BENCHMARK_SUMMARY "

        private val SHOWCASE_TEXT = Color.rgb(245, 241, 232)
        private val SHOWCASE_MUTED = Color.rgb(181, 178, 169)
        private val SHOWCASE_ACCENT = Color.rgb(211, 246, 113)
        private val SHOWCASE_GLASS = Color.argb(200, 14, 16, 15)
        private val SHOWCASE_BORDER = Color.argb(90, 245, 241, 232)

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

    private enum class GestureMode {
        None,
        Orbit,
        Transform
    }

    private data class DatasetSelection(
        val path: String,
        val label: String
    )

    private data class CameraCommand(
        val reset: Boolean,
        val orbitYaw: Float,
        val orbitPitch: Float,
        val zoomScale: Float,
        val panX: Float,
        val panY: Float
    )

    private data class BenchmarkConfig(
        val enabled: Boolean = false,
        val frames: Int = DEFAULT_BENCHMARK_FRAMES,
        val warmupFrames: Int = DEFAULT_BENCHMARK_WARMUP_FRAMES,
        val yawStepRadians: Float = DEFAULT_BENCHMARK_YAW_STEP,
        val sortInterval: Int = DEFAULT_SURFACE_SORT_INTERVAL,
        val asyncSort: Boolean = DEFAULT_SURFACE_ASYNC_SORT,
        val frameLatency: Int = DEFAULT_SURFACE_FRAME_LATENCY,
        // Sample-only A/B knob: "cpu", "gpu", or "adaptive".
        val orderBackend: String = DEFAULT_SURFACE_ORDER_BACKEND,
        // Experimental A/B benchmark knob: "direct" (default), "packed", or "paged".
        val geometryPath: String = DEFAULT_SURFACE_GEOMETRY_PATH
    ) {
        companion object {
            fun fromIntent(intent: Intent): BenchmarkConfig {
                val frames = intent
                    .getIntExtra(EXTRA_BENCHMARK_FRAMES, DEFAULT_BENCHMARK_FRAMES)
                    .coerceAtLeast(1)
                val warmupFrames = intent
                    .getIntExtra(EXTRA_BENCHMARK_WARMUP_FRAMES, DEFAULT_BENCHMARK_WARMUP_FRAMES)
                    .coerceAtLeast(0)
                val yawStep = intent
                    .getFloatExtra(EXTRA_BENCHMARK_YAW_STEP, DEFAULT_BENCHMARK_YAW_STEP)
                    .takeIf { it.isFinite() }
                    ?: DEFAULT_BENCHMARK_YAW_STEP
                val sortInterval = intent
                    .getIntExtra(EXTRA_SURFACE_SORT_INTERVAL, DEFAULT_SURFACE_SORT_INTERVAL)
                    .coerceAtLeast(1)
                val asyncSort = intent
                    .getBooleanExtra(EXTRA_SURFACE_ASYNC_SORT, DEFAULT_SURFACE_ASYNC_SORT)
                val frameLatency = intent
                    .getIntExtra(EXTRA_SURFACE_FRAME_LATENCY, DEFAULT_SURFACE_FRAME_LATENCY)
                    .coerceIn(1, 4)
                val geometryPath = intent.getStringExtra(EXTRA_SURFACE_GEOMETRY_PATH)
                    ?.trim()
                    ?.lowercase(Locale.US)
                    ?.takeIf { it == "direct" || it == "packed" || it == "paged" }
                    ?: DEFAULT_SURFACE_GEOMETRY_PATH
                val orderBackend = intent.getStringExtra(EXTRA_SURFACE_ORDER_BACKEND)
                    ?.trim()
                    ?.lowercase(Locale.US)
                    ?.takeIf { it == "cpu" || it == "gpu" || it == "adaptive" }
                    ?: DEFAULT_SURFACE_ORDER_BACKEND
                return BenchmarkConfig(
                    enabled = intent.getBooleanExtra(EXTRA_BENCHMARK, false),
                    frames = frames,
                    warmupFrames = warmupFrames,
                    yawStepRadians = yawStep,
                    sortInterval = sortInterval,
                    asyncSort = asyncSort,
                    frameLatency = frameLatency,
                    orderBackend = orderBackend,
                    geometryPath = geometryPath
                )
            }
        }
    }

    private class SurfaceBenchmark(
        val config: BenchmarkConfig,
        private val thermalStatusStart: Int?
    ) {
        val enabled: Boolean = config.enabled
        var complete = false
            private set

        private val runId = UUID.randomUUID().toString()
        private val runStartedAtMs = System.currentTimeMillis()
        private val callNs = LongArray(config.frames)
        private val frameMicros = LongArray(config.frames)
        private val preprocessMicros = LongArray(config.frames)
        private val sortMicros = LongArray(config.frames)
        private val rasterMicros = LongArray(config.frames)
        private val visible = LongArray(config.frames)
        private val drawn = LongArray(config.frames)
        private val elapsedNs = LongArray(config.frames)
        private val cameraRevision = LongArray(config.frames)
        private val appliedOrderRevision = LongArray(config.frames)
        private val scheduledRevision = LongArray(config.frames)
        private val completedRevision = LongArray(config.frames)
        private val presentedOrderLag = LongArray(config.frames)
        private val observedResultLag = LongArray(config.frames)
        private val sortFlags = LongArray(config.frames)
        private var observedFrames = 0
        private var samples = 0
        private var measurementStartNs = 0L
        private var measurementStartedAtMs = 0L
        private var measurementEndedAtMs = 0L
        private var totalCallNs = 0L
        private var totalFrameMicros = 0L
        private var totalVisible = 0L
        private var totalDrawn = 0L

        fun record(stats: LongArray, sortStats: LongArray, renderCallNs: Long) {
            if (!enabled || complete) {
                return
            }
            observedFrames += 1
            if (observedFrames <= config.warmupFrames) {
                return
            }

            val nowNs = System.nanoTime()
            if (samples == 0) {
                measurementStartNs = nowNs
                measurementStartedAtMs = System.currentTimeMillis()
            }
            val index = samples
            callNs[index] = renderCallNs
            frameMicros[index] = stats[2]
            preprocessMicros[index] = stats[3]
            sortMicros[index] = stats[4]
            rasterMicros[index] = stats[5]
            visible[index] = stats[0]
            drawn[index] = stats[1]
            elapsedNs[index] = nowNs - measurementStartNs
            cameraRevision[index] = sortStats[0]
            appliedOrderRevision[index] = sortStats[1]
            scheduledRevision[index] = sortStats[2]
            completedRevision[index] = sortStats[3]
            presentedOrderLag[index] = sortStats[4]
            observedResultLag[index] = sortStats[5]
            sortFlags[index] = sortStats[6]
            samples += 1
            totalVisible += stats[0]
            totalDrawn += stats[1]
            totalFrameMicros += stats[2]
            totalCallNs += renderCallNs
            complete = samples >= config.frames
            if (complete) {
                measurementEndedAtMs = System.currentTimeMillis()
            }
        }

        fun resultLine(datasetLabel: String): String {
            val safeSamples = samples.coerceAtLeast(1)
            var cpuTimingSamples = 0
            var cpuPreprocessMicros = 0L
            var cpuSortMicros = 0L
            var cpuRasterMicros = 0L
            for (index in 0 until samples) {
                if (((sortFlags[index] shr 9) and 3L) != 1L) {
                    cpuTimingSamples += 1
                    cpuPreprocessMicros += preprocessMicros[index]
                    cpuSortMicros += sortMicros[index]
                    cpuRasterMicros += rasterMicros[index]
                }
            }
            val cpuDivisor = cpuTimingSamples.coerceAtLeast(1)
            val averageCpuPreprocess = if (cpuTimingSamples == 0) "n/a" else avgMicros(cpuPreprocessMicros, cpuDivisor)
            val averageCpuSort = if (cpuTimingSamples == 0) "n/a" else avgMicros(cpuSortMicros, cpuDivisor)
            val averageCpuRaster = if (cpuTimingSamples == 0) "n/a" else avgMicros(cpuRasterMicros, cpuDivisor)
            val averageCount = if (config.geometryPath == "paged") {
                "avg_loaded_source=${totalVisible / safeSamples}"
            } else {
                "avg_visible=${totalVisible / safeSamples}"
            }
            return "BENCHMARK_RESULT dataset=$datasetLabel " +
                "samples=$samples warmup=${config.warmupFrames} sort_interval=${config.sortInterval} " +
                "async_sort=${config.asyncSort} " +
                "order_backend=${config.orderBackend} " +
                "geometry_pipeline=${geometryPipelineName(config.geometryPath)} " +
                "frame_latency=${config.frameLatency} " +
                "avg_call_ms=${avgNs(totalCallNs, safeSamples)} " +
                "avg_frame_ms=${avgMicros(totalFrameMicros, safeSamples)} " +
                "cpu_timing_samples=$cpuTimingSamples " +
                "avg_cpu_preprocess_ms=$averageCpuPreprocess " +
                "avg_cpu_sort_ms=$averageCpuSort " +
                "avg_cpu_raster_ms=$averageCpuRaster " +
                "$averageCount " +
                "avg_drawn=${totalDrawn / safeSamples}"
        }

        fun artifactLines(
            datasetLabel: String,
            datasetPath: String,
            surfaceWidth: Int,
            surfaceHeight: Int,
            density: Float,
            refreshHz: Double?,
            thermalStatusEnd: Int?
        ): List<Pair<String, String>> {
            check(complete) { "benchmark artifacts require a complete measurement" }
            val validRefreshHz = refreshHz?.takeIf { it.isFinite() && it > 0.0 }
            val frameBudgetMs = 1000.0 / (validRefreshHz ?: 60.0)
            val datasetMetadata = datasetMetadata(File(datasetPath))
            val traceId = "orbit-yaw-${config.yawStepRadians}"
            val hasGpuFrames = (0 until samples).any { index ->
                ((sortFlags[index] shr 9) and 3L) == 1L
            }
            val unavailable = mutableListOf(
                "environment.browser",
                "environment.adapter",
                "environment.driver",
                "frames[*].geometry_submit_ms",
                "frames[*].gpu_wait_ms",
                "frames[*].gpu_complete_ms",
                "summary.distributions.geometry_submit_ms",
                "summary.distributions.gpu_wait_ms",
                "summary.distributions.gpu_complete_ms"
            )
            if (hasGpuFrames) {
                unavailable += "frames[*].preprocess_ms"
                unavailable += "frames[*].sort_ms"
            }
            if (thermalStatusStart == null) unavailable += "environment.thermal_status_start"
            if (thermalStatusEnd == null) unavailable += "environment.thermal_status_end"

            val manifest = JSONObject()
                .put("schema", "gsplat-benchmark/v1")
                .put("record_type", "manifest")
                .put("run_id", runId)
                .put("identity", JSONObject()
                    .put("series_id", "android-native-orbit")
                    .put("started_at_utc", utcTimestamp(runStartedAtMs))
                    .put("ended_at_utc", utcTimestamp(System.currentTimeMillis()))
                    .put("measurement_started_at_utc", utcTimestamp(measurementStartedAtMs))
                    .put("measurement_ended_at_utc", utcTimestamp(measurementEndedAtMs)))
                .put("build", JSONObject()
                    .put("repository_commit", BuildConfig.REPOSITORY_COMMIT)
                    .put("dirty", BuildConfig.REPOSITORY_DIRTY)
                    .put("profile", "android-${if (BuildConfig.DEBUG) "debug" else "release"}+rust-${BuildConfig.NATIVE_RUST_PROFILE}")
                    .put("android_variant", if (BuildConfig.DEBUG) "debug" else "release")
                    .put("native_rust_profile", BuildConfig.NATIVE_RUST_PROFILE)
                    .put("package_version", BuildConfig.VERSION_NAME))
                .put("dataset", JSONObject()
                    .put("id", datasetLabel)
                    .put("sha256", datasetMetadata.sha256)
                    .put("bytes", datasetMetadata.bytes)
                    .put("splat_count", datasetMetadata.splatCount)
                    .put("sh_degree", datasetMetadata.shDegree))
                .put("trace", JSONObject()
                    .put("id", traceId)
                    .put("sha256", sha256(traceId.toByteArray(Charsets.UTF_8))))
                .put("renderer", JSONObject()
                    .put("implementation", "gsplat-rs-native")
                    .put("path", geometryPipelineName(config.geometryPath))
                    .put("backend", "vulkan")
                    .put("order_backend_requested", config.orderBackend)
                    .put("gpu_count_semantics", "source_count_upper_bound; sort-all/draw-all")
                    .put("sort_policy", if (config.asyncSort) {
                        "async_latest:${config.sortInterval}"
                    } else {
                        "interval:${config.sortInterval}"
                    }))
                .put("display", JSONObject()
                    .put("width", surfaceWidth)
                    .put("height", surfaceHeight)
                    .put("dpr", density.toDouble())
                    .put("refresh_hz", validRefreshHz ?: 60.0)
                    .put("refresh_hz_source", if (validRefreshHz == null) "configured" else "observed")
                    .put("frame_budget_ms", frameBudgetMs)
                    .put("frame_budget_source", if (validRefreshHz == null) "configured" else "observed"))
                .put("environment", JSONObject()
                    .put("platform", "android-native")
                    .put("os", "Android ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT})")
                    .put("device", "${Build.MANUFACTURER} ${Build.MODEL} (${Build.DEVICE})")
                    .put("browser", JSONObject.NULL)
                    .put("adapter", JSONObject.NULL)
                    .put("driver", JSONObject.NULL)
                    .put("hardware", Build.HARDWARE)
                    .put("thermal_status_start", thermalStatusStart ?: JSONObject.NULL)
                    .put("thermal_status_end", thermalStatusEnd ?: JSONObject.NULL))
                .put("unavailable_fields", JSONArray(unavailable))

            val lines = ArrayList<Pair<String, String>>(samples + 2)
            lines += BENCHMARK_MANIFEST_PREFIX to manifest.toString()
            for (index in 0 until samples) {
                val flags = sortFlags[index]
                val gpuBackend = ((flags shr 9) and 3L) == 1L
                val frame = JSONObject()
                    .put("schema", "gsplat-benchmark/v1")
                    .put("record_type", "frame")
                    .put("run_id", runId)
                    .put("frame_index", index)
                    .put("elapsed_ns", elapsedNs[index])
                    .put("call_ms", callNs[index].toDouble() / 1_000_000.0)
                    .put("frame_wall_ms", frameMicros[index].toDouble() / 1000.0)
                    .put("preprocess_ms", if (gpuBackend) JSONObject.NULL else preprocessMicros[index].toDouble() / 1000.0)
                    .put("sort_ms", if (gpuBackend) JSONObject.NULL else sortMicros[index].toDouble() / 1000.0)
                    .put("geometry_submit_ms", JSONObject.NULL)
                    .put("gpu_wait_ms", JSONObject.NULL)
                    .put("gpu_complete_ms", JSONObject.NULL)
                    .put("visible", visible[index])
                    .put("drawn", drawn[index])
                    .put("sort_refreshed", flags and 1L != 0L)
                    .put("camera_revision", cameraRevision[index])
                    .put("applied_order_revision", appliedOrderRevision[index])
                    .put("presented_order_revision_lag", presentedOrderLag[index])
                    .put("async_sort_scheduled_revision", if (flags and (1L shl 3) != 0L) scheduledRevision[index] else JSONObject.NULL)
                    .put("async_sort_completed_revision", if (flags and (1L shl 4) != 0L) completedRevision[index] else JSONObject.NULL)
                    .put("async_sort_observed_result_lag", if (flags and (1L shl 8) != 0L) observedResultLag[index] else JSONObject.NULL)
                    .put("async_sort_scheduled", flags and (1L shl 2) != 0L)
                    .put("async_sort_result_applied", flags and (1L shl 5) != 0L)
                    .put("stale_async_sort_dropped", flags and (1L shl 6) != 0L)
                    .put("sync_sort_fallback", flags and (1L shl 7) != 0L)
                    .put("order_backend", if (((flags shr 9) and 3L) == 1L) "gpu" else "cpu")
                    .put("gpu_sort_fallback", flags and (1L shl 11) != 0L)
                    .put("adaptive_state", adaptiveStateName(flags))
                lines += BENCHMARK_FRAME_PREFIX to frame.toString()
            }

            var missedFrames = 0
            for (index in 0 until samples) {
                if (frameMicros[index].toDouble() / 1000.0 > frameBudgetMs) missedFrames += 1
            }
            val summary = JSONObject()
                .put("schema", "gsplat-benchmark/v1")
                .put("record_type", "summary")
                .put("run_id", runId)
                .put("sample_count", samples)
                .put("warmup_count", config.warmupFrames)
                .put("frame_budget_ms", frameBudgetMs)
                .put("missed_frame_count", missedFrames)
                .put("distributions", JSONObject()
                    .put("call_ms", distributionJson(callNs, samples, 1_000_000.0))
                    .put("frame_wall_ms", distributionJson(frameMicros, samples, 1000.0))
                    .put("preprocess_ms", cpuTimingDistribution(preprocessMicros))
                    .put("sort_ms", cpuTimingDistribution(sortMicros))
                    .put("geometry_submit_ms", JSONObject.NULL)
                    .put("gpu_wait_ms", JSONObject.NULL)
                    .put("gpu_complete_ms", JSONObject.NULL))
                .put("sort_telemetry", sortTelemetrySummary())
            lines += BENCHMARK_SUMMARY_PREFIX to summary.toString()
            return lines
        }

        private fun sortTelemetrySummary(): JSONObject {
            var scheduled = 0
            var completed = 0
            var applied = 0
            var dropped = 0
            var fallbacks = 0
            var cpuFrames = 0
            var gpuFrames = 0
            var gpuFallbacks = 0
            var backendSwitches = 0
            var previousBackend = -1L
            var maxPresentedLag = 0L
            for (index in 0 until samples) {
                val flags = sortFlags[index]
                if (flags and (1L shl 2) != 0L) scheduled += 1
                if (flags and (1L shl 4) != 0L) completed += 1
                if (flags and (1L shl 5) != 0L) applied += 1
                if (flags and (1L shl 6) != 0L) dropped += 1
                if (flags and (1L shl 7) != 0L) fallbacks += 1
                val backend = (flags shr 9) and 3L
                if (backend == 1L) gpuFrames += 1 else cpuFrames += 1
                if (previousBackend >= 0L && backend != previousBackend) backendSwitches += 1
                previousBackend = backend
                if (flags and (1L shl 11) != 0L) gpuFallbacks += 1
                maxPresentedLag = maxOf(maxPresentedLag, presentedOrderLag[index])
            }
            return JSONObject()
                .put("scheduled_count", scheduled)
                .put("completed_count", completed)
                .put("applied_count", applied)
                .put("dropped_count", dropped)
                .put("sync_fallback_count", fallbacks)
                .put("cpu_frame_count", cpuFrames)
                .put("gpu_frame_count", gpuFrames)
                .put("gpu_sort_fallback_count", gpuFallbacks)
                .put("backend_switch_count", backendSwitches)
                .put("adaptive_final_state", if (samples > 0) {
                    adaptiveStateName(sortFlags[samples - 1])
                } else {
                    "disabled"
                })
                .put("max_presented_revision_lag", maxPresentedLag)
                .put("stale_applied_count", 0)
        }

        private fun distributionJson(values: LongArray, count: Int, divisor: Double): JSONObject {
            val sorted = values.copyOf(count).also(LongArray::sort)
            var sum = 0.0
            for (index in 0 until count) sum += values[index].toDouble() / divisor
            return JSONObject()
                .put("count", count)
                .put("mean", sum / count.toDouble())
                .put("p50", BenchmarkMath.nearestRank(sorted, count, 0.50).toDouble() / divisor)
                .put("p90", BenchmarkMath.nearestRank(sorted, count, 0.90).toDouble() / divisor)
                .put("p95", BenchmarkMath.nearestRank(sorted, count, 0.95).toDouble() / divisor)
                .put("p99", BenchmarkMath.nearestRank(sorted, count, 0.99).toDouble() / divisor)
                .put("max", sorted[count - 1].toDouble() / divisor)
        }

        private fun cpuTimingDistribution(values: LongArray): Any {
            val filtered = LongArray(samples)
            var count = 0
            for (index in 0 until samples) {
                if (((sortFlags[index] shr 9) and 3L) != 1L) {
                    filtered[count] = values[index]
                    count += 1
                }
            }
            return if (count == 0) {
                JSONObject.NULL
            } else {
                distributionJson(filtered, count, 1000.0)
            }
        }

        private fun utcTimestamp(epochMs: Long): String =
            SimpleDateFormat("yyyy-MM-dd'T'HH:mm:ss.SSS'Z'", Locale.US).apply {
                timeZone = TimeZone.getTimeZone("UTC")
            }.format(Date(epochMs))

        private fun datasetMetadata(file: File): DatasetArtifactMetadata {
            check(file.isFile) { "benchmark dataset is unavailable: ${file.absolutePath}" }
            val digest = MessageDigest.getInstance("SHA-256")
            file.inputStream().buffered().use { input ->
                val buffer = ByteArray(64 * 1024)
                while (true) {
                    val count = input.read(buffer)
                    if (count < 0) break
                    digest.update(buffer, 0, count)
                }
            }
            var splatCount = 0L
            var restProperties = 0
            file.bufferedReader(Charsets.US_ASCII).use { reader ->
                repeat(512) {
                    val line = reader.readLine() ?: return@repeat
                    if (line.startsWith("element vertex ")) {
                        splatCount = line.substringAfterLast(' ').toLongOrNull() ?: 0L
                    } else if (line.startsWith("property ") && line.substringAfterLast(' ').startsWith("f_rest_")) {
                        restProperties += 1
                    } else if (line == "end_header") {
                        return@use
                    }
                }
            }
            val shDegree = (0..4).firstOrNull { degree ->
                3 * (((degree + 1) * (degree + 1)) - 1) == restProperties
            } ?: error("unsupported SH property count in benchmark dataset: $restProperties")
            check(splatCount > 0L) { "benchmark dataset has no declared vertices" }
            return DatasetArtifactMetadata(
                sha256 = digest.digest().joinToString("") { "%02x".format(it) },
                bytes = file.length(),
                splatCount = splatCount,
                shDegree = shDegree
            )
        }

        private fun sha256(bytes: ByteArray): String =
            MessageDigest.getInstance("SHA-256")
                .digest(bytes)
                .joinToString("") { "%02x".format(it) }

        private data class DatasetArtifactMetadata(
            val sha256: String,
            val bytes: Long,
            val splatCount: Long,
            val shDegree: Int
        )

        private fun avgMicros(total: Long, samples: Int): String =
            String.format("%.3f", total.toDouble() / samples.toDouble() / 1000.0)

        private fun avgNs(total: Long, samples: Int): String =
            String.format("%.3f", total.toDouble() / samples.toDouble() / 1_000_000.0)
    }
}
