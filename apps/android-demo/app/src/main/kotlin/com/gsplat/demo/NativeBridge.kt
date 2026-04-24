package com.gsplat.demo

import android.view.Surface

object NativeBridge {
    init {
        System.loadLibrary("gsplat_jni")
    }

    @JvmStatic
    external fun versionMajor(): Int

    @JvmStatic
    external fun versionMinor(): Int

    @JvmStatic
    external fun errorMessage(code: Int): String

    @JvmStatic
    external fun runFfiSmoke(datasetPath: String): Int

    @JvmStatic
    external fun createSurfaceRenderer(
        surface: Surface,
        datasetPath: String,
        width: Int,
        height: Int,
        outError: IntArray
    ): Long

    @JvmStatic
    external fun resizeSurfaceRenderer(nativeHandle: Long, width: Int, height: Int): Int

    @JvmStatic
    external fun resetSurfaceCamera(nativeHandle: Long): Int

    @JvmStatic
    external fun orbitSurfaceRenderer(
        nativeHandle: Long,
        deltaYawRadians: Float,
        deltaPitchRadians: Float
    ): Int

    @JvmStatic
    external fun zoomSurfaceRenderer(nativeHandle: Long, distanceScale: Float): Int

    @JvmStatic
    external fun panSurfaceRenderer(
        nativeHandle: Long,
        normalizedDeltaX: Float,
        normalizedDeltaY: Float
    ): Int

    @JvmStatic
    external fun renderSurfaceFrame(nativeHandle: Long): Int

    @JvmStatic
    external fun getSurfaceStats(nativeHandle: Long, outStats: LongArray): Int

    @JvmStatic
    external fun destroySurfaceRenderer(nativeHandle: Long)
}
