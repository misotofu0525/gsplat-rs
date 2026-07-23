package com.gsplat.android

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
    external fun lastErrorMessage(): String

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
    external fun createSurfaceRendererWithGeometryPath(
        surface: Surface,
        datasetPath: String,
        width: Int,
        height: Int,
        geometryPath: Int,
        outError: IntArray
    ): Long

    @JvmStatic
    external fun resizeSurfaceRenderer(nativeHandle: Long, width: Int, height: Int): Int

    @JvmStatic
    external fun setSurfaceSortInterval(nativeHandle: Long, interval: Int): Int

    @JvmStatic
    external fun setSurfaceOrderBackend(nativeHandle: Long, backend: Int): Int

    @JvmStatic
    external fun setSurfaceProjectedPolicyV1(nativeHandle: Long, policy: Int): Int

    @JvmStatic
    external fun setSurfaceAsyncSortEnabled(nativeHandle: Long, enabled: Boolean): Int

    @JvmStatic
    external fun setSurfaceGeometryPath(nativeHandle: Long, path: Int): Int

    @JvmStatic
    external fun setSurfaceFrameLatency(nativeHandle: Long, latency: Int): Int

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
    external fun getSurfaceExactness(nativeHandle: Long, outExactness: LongArray): Int

    @JvmStatic
    external fun getSurfacePresentation(nativeHandle: Long, outPresentation: LongArray): Int

    @JvmStatic
    external fun getSurfaceSortStats(nativeHandle: Long, outStats: LongArray): Int

    @JvmStatic
    external fun getSurfaceOrderSubmission(nativeHandle: Long, outSubmission: LongArray): Int

    @JvmStatic
    external fun getSurfaceProjectedSubmissionV1(nativeHandle: Long, outSubmission: LongArray): Int

    @JvmStatic
    external fun pollSurfaceProjectedMeasurementV1(nativeHandle: Long, outMeasurement: LongArray): Int

    @JvmStatic
    external fun pollSurfaceProjectedFailureV1(nativeHandle: Long, outFailure: LongArray): Int

    @JvmStatic
    external fun pollSurfaceOrderMeasurement(nativeHandle: Long, outMeasurement: LongArray): Int

    @JvmStatic
    external fun pollSurfaceCpuOrderMeasurement(nativeHandle: Long, outMeasurement: LongArray): Int

    @JvmStatic
    external fun pollSurfaceOrderMeasurementFailure(nativeHandle: Long, outFailure: LongArray): Int

    @JvmStatic
    external fun destroySurfaceRenderer(nativeHandle: Long)
}
