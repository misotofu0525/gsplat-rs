package com.gsplat.android

import android.view.Surface
import java.io.Closeable

class GsplatSurfaceRenderer private constructor(
    private var nativeHandle: Long
) : Closeable {
    private val lock = Any()

    val isClosed: Boolean
        get() = synchronized(lock) { nativeHandle == 0L }

    fun resize(width: Int, height: Int) {
        synchronized(lock) {
            checkOpen()
            checkResult(NativeBridge.resizeSurfaceRenderer(nativeHandle, width, height))
        }
    }

    fun configure(options: GsplatSurfaceOptions) {
        synchronized(lock) {
            checkOpen()
            checkResult(NativeBridge.setSurfaceSortInterval(nativeHandle, options.sortInterval))
            checkResult(NativeBridge.setSurfaceAsyncSortEnabled(nativeHandle, options.asyncSort))
            checkResult(NativeBridge.setSurfaceFrameLatency(nativeHandle, options.frameLatency))
        }
    }

    fun resetCamera() {
        synchronized(lock) {
            checkOpen()
            checkResult(NativeBridge.resetSurfaceCamera(nativeHandle))
        }
    }

    fun orbit(deltaYawRadians: Float, deltaPitchRadians: Float) {
        synchronized(lock) {
            checkOpen()
            checkResult(NativeBridge.orbitSurfaceRenderer(nativeHandle, deltaYawRadians, deltaPitchRadians))
        }
    }

    fun zoom(distanceScale: Float) {
        synchronized(lock) {
            checkOpen()
            checkResult(NativeBridge.zoomSurfaceRenderer(nativeHandle, distanceScale))
        }
    }

    fun pan(normalizedDeltaX: Float, normalizedDeltaY: Float) {
        synchronized(lock) {
            checkOpen()
            checkResult(NativeBridge.panSurfaceRenderer(nativeHandle, normalizedDeltaX, normalizedDeltaY))
        }
    }

    fun renderFrame() {
        synchronized(lock) {
            checkOpen()
            checkResult(NativeBridge.renderSurfaceFrame(nativeHandle))
        }
    }

    fun stats(): GsplatSurfaceStats {
        synchronized(lock) {
            checkOpen()
            val raw = LongArray(6)
            checkResult(NativeBridge.getSurfaceStats(nativeHandle, raw))
            return GsplatSurfaceStats.fromRaw(raw)
        }
    }

    override fun close() {
        synchronized(lock) {
            val handle = nativeHandle
            nativeHandle = 0L
            if (handle != 0L) {
                NativeBridge.destroySurfaceRenderer(handle)
            }
        }
    }

    private fun checkOpen() {
        check(nativeHandle != 0L) { "GsplatSurfaceRenderer is closed" }
    }

    companion object {
        fun create(
            surface: Surface,
            datasetPath: String,
            width: Int,
            height: Int,
            options: GsplatSurfaceOptions = GsplatSurfaceOptions()
        ): GsplatSurfaceRenderer {
            GsplatAndroidVersion.requireSupported()

            val outError = IntArray(1)
            val handle = NativeBridge.createSurfaceRendererWithGeometryPath(
                surface,
                datasetPath,
                width,
                height,
                options.geometryPath.nativeValue,
                outError
            )
            if (handle == 0L) {
                throw GsplatException(outError[0])
            }

            return GsplatSurfaceRenderer(handle).also { renderer ->
                try {
                    renderer.configure(options)
                } catch (error: Throwable) {
                    renderer.close()
                    throw error
                }
            }
        }

        internal fun checkResult(code: Int) {
            if (code != 0) {
                throw GsplatException(code)
            }
        }
    }
}
