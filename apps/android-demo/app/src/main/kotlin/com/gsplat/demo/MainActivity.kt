package com.gsplat.demo

import android.app.Activity
import android.content.Intent
import android.graphics.Color
import android.net.Uri
import android.os.Bundle
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
import android.widget.TextView
import java.io.File
import kotlin.math.abs
import kotlin.math.hypot
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt

class MainActivity : Activity(), SurfaceHolder.Callback {
    private val renderLock = Object()
    private val cameraCommandLock = Object()
    @Volatile private var running = false
    @Volatile private var renderThread: Thread? = null
    private var nativeRenderer = 0L
    private lateinit var datasetPath: String
    private var datasetLabel = "pending"
    private lateinit var statusText: TextView
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
        statusText = TextView(this).apply {
            setTextColor(Color.WHITE)
            setBackgroundColor(0x66000000)
            isClickable = false
            text = buildStatusText(latestStatus)
        }
        val importButton = Button(this).apply {
            text = "Import PLY"
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
            addView(
                statusText,
                FrameLayout.LayoutParams(
                    FrameLayout.LayoutParams.MATCH_PARENT,
                    FrameLayout.LayoutParams.WRAP_CONTENT
                )
            )
            addView(
                importButton,
                FrameLayout.LayoutParams(
                    FrameLayout.LayoutParams.WRAP_CONTENT,
                    FrameLayout.LayoutParams.WRAP_CONTENT,
                    Gravity.BOTTOM or Gravity.END
                ).apply {
                    setMargins(24, 24, 24, 24)
                }
            )
        }
        setContentView(root)
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
                Log.i(TAG, "createSurfaceRenderer start size=${width}x$height dataset=$datasetPath")
                updateStatus("state=creating size=${width}x$height")
                val createError = IntArray(1)
                val handle = NativeBridge.createSurfaceRenderer(surface, datasetPath, width, height, createError)
                if (handle == 0L) {
                    val rc = createError[0]
                    val message = NativeBridge.errorMessage(rc)
                    Log.e(TAG, "createSurfaceRenderer failed rc=$rc error=$message")
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
                val gpuPreprojectRc = NativeBridge.setSurfaceGpuPreprojectEnabled(
                    handle,
                    benchmarkConfig.gpuPreproject
                )
                if (gpuPreprojectRc != 0) {
                    val message = NativeBridge.errorMessage(gpuPreprojectRc)
                    Log.e(TAG, "setSurfaceGpuPreprojectEnabled failed rc=$gpuPreprojectRc error=$message")
                    NativeBridge.destroySurfaceRenderer(handle)
                    running = false
                    updateStatus("state=create_failed rc=$gpuPreprojectRc error=$message")
                    return@Thread
                }
                val gpuPreprojectDoubleBufferRc =
                    NativeBridge.setSurfaceGpuPreprojectDoubleBufferEnabled(
                        handle,
                        benchmarkConfig.gpuPreprojectDoubleBuffer
                    )
                if (gpuPreprojectDoubleBufferRc != 0) {
                    val message = NativeBridge.errorMessage(gpuPreprojectDoubleBufferRc)
                    Log.e(TAG, "setSurfaceGpuPreprojectDoubleBufferEnabled failed rc=$gpuPreprojectDoubleBufferRc error=$message")
                    NativeBridge.destroySurfaceRenderer(handle)
                    running = false
                    updateStatus("state=create_failed rc=$gpuPreprojectDoubleBufferRc error=$message")
                    return@Thread
                }
                val staticDirectRc = NativeBridge.setSurfaceStaticDirectEnabled(
                    handle,
                    benchmarkConfig.staticDirect
                )
                if (staticDirectRc != 0) {
                    val message = NativeBridge.errorMessage(staticDirectRc)
                    Log.e(TAG, "setSurfaceStaticDirectEnabled failed rc=$staticDirectRc error=$message")
                    NativeBridge.destroySurfaceRenderer(handle)
                    running = false
                    updateStatus("state=create_failed rc=$staticDirectRc error=$message")
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
                val asyncGeometryRc = NativeBridge.setSurfaceAsyncGeometryEnabled(
                    handle,
                    benchmarkConfig.asyncGeometry
                )
                if (asyncGeometryRc != 0) {
                    val message = NativeBridge.errorMessage(asyncGeometryRc)
                    Log.e(TAG, "setSurfaceAsyncGeometryEnabled failed rc=$asyncGeometryRc error=$message")
                    NativeBridge.destroySurfaceRenderer(handle)
                    running = false
                    updateStatus("state=create_failed rc=$asyncGeometryRc error=$message")
                    return@Thread
                }
                val instanceBufferRc = NativeBridge.setSurfaceInstanceBufferCount(
                    handle,
                    benchmarkConfig.instanceBuffers
                )
                if (instanceBufferRc != 0) {
                    val message = NativeBridge.errorMessage(instanceBufferRc)
                    Log.e(TAG, "setSurfaceInstanceBufferCount failed rc=$instanceBufferRc error=$message")
                    NativeBridge.destroySurfaceRenderer(handle)
                    running = false
                    updateStatus("state=create_failed rc=$instanceBufferRc error=$message")
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
                    val benchmark = SurfaceBenchmark(benchmarkConfig)
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
                                if (statsRc == 0) {
                                    benchmark.record(stats, renderCallNs)
                                    if (benchmark.complete) {
                                        val result = benchmark.resultLine(datasetLabel)
                                        Log.i(TAG, result)
                                        updateStatus("state=benchmark_complete $result")
                                        running = false
                                    }
                                } else {
                                    Log.e(TAG, "benchmark getSurfaceStats failed rc=$statsRc")
                                    updateStatus("state=benchmark_stats_error rc=$statsRc")
                                    running = false
                                }
                            }
                            if (now - lastStatusAt > STATUS_INTERVAL_NS) {
                                lastStatusAt = now
                                val statsRc = NativeBridge.getSurfaceStats(handle, stats)
                                val detail = if (statsRc == 0) {
                                    "state=rendering frames=$frameCount " +
                                        "visible=${stats[0]} drawn=${stats[1]}/${stats[0]} " +
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
            statusText.text = buildStatusText(latestStatus)
        }
    }

    private fun buildStatusText(status: String): String = buildString {
        appendLine("gsplat android demo")
        appendLine("abi=${NativeBridge.versionMajor()}.${NativeBridge.versionMinor()}")
        appendLine("surface=wgpu realtime ${surfaceSizeLabel}")
        appendLine(status)
        appendLine(cameraStatus)
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
        private const val TAG = "GsplatDemo"
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
        private const val EXTRA_BENCHMARK = "gsplat_benchmark"
        private const val EXTRA_BENCHMARK_FRAMES = "gsplat_benchmark_frames"
        private const val EXTRA_BENCHMARK_WARMUP_FRAMES = "gsplat_benchmark_warmup_frames"
        private const val EXTRA_BENCHMARK_YAW_STEP = "gsplat_benchmark_yaw_step"
        private const val EXTRA_SURFACE_SORT_INTERVAL = "gsplat_surface_sort_interval"
        private const val EXTRA_SURFACE_GPU_PREPROJECT = "gsplat_surface_gpu_preproject"
        private const val EXTRA_SURFACE_GPU_PREPROJECT_DOUBLE_BUFFER =
            "gsplat_surface_gpu_preproject_double_buffer"
        private const val EXTRA_SURFACE_STATIC_DIRECT = "gsplat_surface_static_direct"
        private const val EXTRA_SURFACE_ASYNC_SORT = "gsplat_surface_async_sort"
        private const val EXTRA_SURFACE_ASYNC_GEOMETRY = "gsplat_surface_async_geometry"
        private const val EXTRA_SURFACE_INSTANCE_BUFFERS = "gsplat_surface_instance_buffers"
        private const val EXTRA_SURFACE_FRAME_LATENCY = "gsplat_surface_frame_latency"
        private const val DEFAULT_BENCHMARK_FRAMES = 120
        private const val DEFAULT_BENCHMARK_WARMUP_FRAMES = 10
        private const val DEFAULT_BENCHMARK_YAW_STEP = 0.001f
        private const val DEFAULT_SURFACE_SORT_INTERVAL = 2
        private const val DEFAULT_SURFACE_GPU_PREPROJECT = false
        private const val DEFAULT_SURFACE_GPU_PREPROJECT_DOUBLE_BUFFER = false
        private const val DEFAULT_SURFACE_STATIC_DIRECT = false
        private const val DEFAULT_SURFACE_ASYNC_SORT = false
        private const val DEFAULT_SURFACE_ASYNC_GEOMETRY = false
        private const val DEFAULT_SURFACE_INSTANCE_BUFFERS = 1
        private const val DEFAULT_SURFACE_FRAME_LATENCY = 2
        private const val TARGET_FRAME_INTERVAL_NS = 16_666_667L

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
        val gpuPreproject: Boolean = DEFAULT_SURFACE_GPU_PREPROJECT,
        val gpuPreprojectDoubleBuffer: Boolean = DEFAULT_SURFACE_GPU_PREPROJECT_DOUBLE_BUFFER,
        val staticDirect: Boolean = DEFAULT_SURFACE_STATIC_DIRECT,
        val asyncSort: Boolean = DEFAULT_SURFACE_ASYNC_SORT,
        val asyncGeometry: Boolean = DEFAULT_SURFACE_ASYNC_GEOMETRY,
        val instanceBuffers: Int = DEFAULT_SURFACE_INSTANCE_BUFFERS,
        val frameLatency: Int = DEFAULT_SURFACE_FRAME_LATENCY
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
                    .takeIf { it.isFinite() && it != 0f }
                    ?: DEFAULT_BENCHMARK_YAW_STEP
                val sortInterval = intent
                    .getIntExtra(EXTRA_SURFACE_SORT_INTERVAL, DEFAULT_SURFACE_SORT_INTERVAL)
                    .coerceAtLeast(1)
                val gpuPreproject = intent
                    .getBooleanExtra(
                        EXTRA_SURFACE_GPU_PREPROJECT,
                        DEFAULT_SURFACE_GPU_PREPROJECT
                    )
                val gpuPreprojectDoubleBuffer = intent.getBooleanExtra(
                    EXTRA_SURFACE_GPU_PREPROJECT_DOUBLE_BUFFER,
                    DEFAULT_SURFACE_GPU_PREPROJECT_DOUBLE_BUFFER
                )
                val staticDirect = intent
                    .getBooleanExtra(EXTRA_SURFACE_STATIC_DIRECT, DEFAULT_SURFACE_STATIC_DIRECT)
                val asyncSort = intent
                    .getBooleanExtra(EXTRA_SURFACE_ASYNC_SORT, DEFAULT_SURFACE_ASYNC_SORT)
                val asyncGeometry = intent
                    .getBooleanExtra(EXTRA_SURFACE_ASYNC_GEOMETRY, DEFAULT_SURFACE_ASYNC_GEOMETRY)
                val instanceBuffers = intent
                    .getIntExtra(EXTRA_SURFACE_INSTANCE_BUFFERS, DEFAULT_SURFACE_INSTANCE_BUFFERS)
                    .coerceIn(1, 3)
                val frameLatency = intent
                    .getIntExtra(EXTRA_SURFACE_FRAME_LATENCY, DEFAULT_SURFACE_FRAME_LATENCY)
                    .coerceIn(1, 4)
                return BenchmarkConfig(
                    enabled = intent.getBooleanExtra(EXTRA_BENCHMARK, false),
                    frames = frames,
                    warmupFrames = warmupFrames,
                    yawStepRadians = yawStep,
                    sortInterval = sortInterval,
                    gpuPreproject = gpuPreproject,
                    gpuPreprojectDoubleBuffer = gpuPreprojectDoubleBuffer,
                    staticDirect = staticDirect,
                    asyncSort = asyncSort,
                    asyncGeometry = asyncGeometry,
                    instanceBuffers = instanceBuffers,
                    frameLatency = frameLatency
                )
            }
        }
    }

    private class SurfaceBenchmark(val config: BenchmarkConfig) {
        val enabled: Boolean = config.enabled
        var complete = false
            private set

        private var observedFrames = 0
        private var samples = 0
        private var totalCallNs = 0L
        private var totalFrameMicros = 0L
        private var totalPreprocessMicros = 0L
        private var totalSortMicros = 0L
        private var totalRasterMicros = 0L
        private var totalVisible = 0L
        private var totalDrawn = 0L

        fun record(stats: LongArray, renderCallNs: Long) {
            if (!enabled || complete) {
                return
            }
            observedFrames += 1
            if (observedFrames <= config.warmupFrames) {
                return
            }

            samples += 1
            totalVisible += stats[0]
            totalDrawn += stats[1]
            totalFrameMicros += stats[2]
            totalPreprocessMicros += stats[3]
            totalSortMicros += stats[4]
            totalRasterMicros += stats[5]
            totalCallNs += renderCallNs
            complete = samples >= config.frames
        }

        fun resultLine(datasetLabel: String): String {
            val safeSamples = samples.coerceAtLeast(1)
            return "BENCHMARK_RESULT dataset=$datasetLabel " +
                "samples=$samples warmup=${config.warmupFrames} sort_interval=${config.sortInterval} " +
                "gpu_preproject=${config.gpuPreproject} " +
                "gpu_preproject_double_buffer=${config.gpuPreprojectDoubleBuffer} " +
                "static_direct=${config.staticDirect} " +
                "async_sort=${config.asyncSort} " +
                "async_geometry=${config.asyncGeometry} " +
                "instance_buffers=${config.instanceBuffers} " +
                "frame_latency=${config.frameLatency} " +
                "avg_call_ms=${avgNs(totalCallNs, safeSamples)} " +
                "avg_frame_ms=${avgMicros(totalFrameMicros, safeSamples)} " +
                "avg_preprocess_ms=${avgMicros(totalPreprocessMicros, safeSamples)} " +
                "avg_sort_ms=${avgMicros(totalSortMicros, safeSamples)} " +
                "avg_raster_ms=${avgMicros(totalRasterMicros, safeSamples)} " +
                "avg_visible=${totalVisible / safeSamples} " +
                "avg_drawn=${totalDrawn / safeSamples}"
        }

        private fun avgMicros(total: Long, samples: Int): String =
            String.format("%.3f", total.toDouble() / samples.toDouble() / 1000.0)

        private fun avgNs(total: Long, samples: Int): String =
            String.format("%.3f", total.toDouble() / samples.toDouble() / 1_000_000.0)
    }
}
