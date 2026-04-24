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
    external fun runFfiSmoke(datasetPath: String): Int

    @JvmStatic
    external fun createSurfaceRenderer(
        surface: Surface,
        datasetPath: String,
        width: Int,
        height: Int
    ): Long

    @JvmStatic
    external fun resizeSurfaceRenderer(nativeHandle: Long, width: Int, height: Int): Int

    @JvmStatic
    external fun renderSurfaceFrame(nativeHandle: Long): Int

    @JvmStatic
    external fun getSurfaceStats(nativeHandle: Long, outStats: LongArray): Int

    @JvmStatic
    external fun destroySurfaceRenderer(nativeHandle: Long)
}
