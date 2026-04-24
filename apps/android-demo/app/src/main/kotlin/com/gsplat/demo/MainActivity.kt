package com.gsplat.demo

import android.app.Activity
import android.graphics.Color
import android.os.Bundle
import android.os.SystemClock
import android.util.Log
import android.view.MotionEvent
import android.view.Surface
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.View
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
    private lateinit var statusText: TextView
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
            setOnTouchListener(::handleTouch)
        }
        statusText = TextView(this).apply {
            setTextColor(Color.WHITE)
            setBackgroundColor(0x66000000)
            isClickable = false
            text = buildStatusText(latestStatus)
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
                            val cameraRc = applyPendingCameraCommands(handle)
                            if (cameraRc != 0) {
                                Log.e(TAG, "applyPendingCameraCommands failed rc=$cameraRc")
                                cameraRc
                            } else {
                                NativeBridge.renderSurfaceFrame(handle)
                            }
                        }
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
        append("dataset=$datasetPath")
    }

    private fun formatMicros(value: Long): String =
        String.format("%.2f", value.toDouble() / 1000.0)

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

    private data class CameraCommand(
        val reset: Boolean,
        val orbitYaw: Float,
        val orbitPitch: Float,
        val zoomScale: Float,
        val panX: Float,
        val panY: Float
    )
}
