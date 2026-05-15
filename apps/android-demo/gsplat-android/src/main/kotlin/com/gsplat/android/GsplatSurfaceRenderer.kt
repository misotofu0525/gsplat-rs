package com.gsplat.android

import android.view.Surface
import java.io.Closeable

class GsplatSurfaceRenderer private constructor(
    private var nativeHandle: Long
) : Closeable {
    val isClosed: Boolean
        get() = nativeHandle == 0L

    fun resize(width: Int, height: Int) {
        checkOpen()
        checkResult(NativeBridge.resizeSurfaceRenderer(nativeHandle, width, height))
    }

    fun configure(options: GsplatSurfaceOptions) {
        checkOpen()
        checkResult(NativeBridge.setSurfaceSortInterval(nativeHandle, options.sortInterval))
        checkResult(NativeBridge.setSurfaceGpuPreprojectEnabled(nativeHandle, options.gpuPreproject))
        checkResult(
            NativeBridge.setSurfaceGpuPreprojectDoubleBufferEnabled(
                nativeHandle,
                options.gpuPreprojectDoubleBuffer
            )
        )
        checkResult(NativeBridge.setSurfaceStaticDirectEnabled(nativeHandle, options.staticDirect))
        checkResult(NativeBridge.setSurfaceAsyncSortEnabled(nativeHandle, options.asyncSort))
        checkResult(NativeBridge.setSurfaceAsyncGeometryEnabled(nativeHandle, options.asyncGeometry))
        checkResult(NativeBridge.setSurfaceInstanceBufferCount(nativeHandle, options.instanceBufferCount))
        checkResult(NativeBridge.setSurfaceFrameLatency(nativeHandle, options.frameLatency))
    }

    fun resetCamera() {
        checkOpen()
        checkResult(NativeBridge.resetSurfaceCamera(nativeHandle))
    }

    fun orbit(deltaYawRadians: Float, deltaPitchRadians: Float) {
        checkOpen()
        checkResult(NativeBridge.orbitSurfaceRenderer(nativeHandle, deltaYawRadians, deltaPitchRadians))
    }

    fun zoom(distanceScale: Float) {
        checkOpen()
        checkResult(NativeBridge.zoomSurfaceRenderer(nativeHandle, distanceScale))
    }

    fun pan(normalizedDeltaX: Float, normalizedDeltaY: Float) {
        checkOpen()
        checkResult(NativeBridge.panSurfaceRenderer(nativeHandle, normalizedDeltaX, normalizedDeltaY))
    }

    fun renderFrame() {
        checkOpen()
        checkResult(NativeBridge.renderSurfaceFrame(nativeHandle))
    }

    fun stats(): GsplatSurfaceStats {
        checkOpen()
        val raw = LongArray(6)
        checkResult(NativeBridge.getSurfaceStats(nativeHandle, raw))
        return GsplatSurfaceStats.fromRaw(raw)
    }

    override fun close() {
        val handle = nativeHandle
        nativeHandle = 0L
        if (handle != 0L) {
            NativeBridge.destroySurfaceRenderer(handle)
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
            val outError = IntArray(1)
            val handle = NativeBridge.createSurfaceRenderer(
                surface,
                datasetPath,
                width,
                height,
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
