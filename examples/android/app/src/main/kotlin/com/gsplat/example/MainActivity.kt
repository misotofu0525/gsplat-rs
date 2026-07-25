package com.gsplat.example

import android.app.Activity
import android.content.Intent
import android.content.pm.ActivityInfo
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
import android.util.Base64
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
import com.gsplat.android.GsplatSurfaceCurrentStatsCountSemantics
import com.gsplat.android.GsplatSurfaceCurrentStatsIdentity
import com.gsplat.android.GsplatSurfaceCurrentStatsPlan
import com.gsplat.android.GsplatSurfaceCurrentStatsRequestStatus
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
private const val GSPLAT_PROJECTED_POLICY_COMPACT = 2
private const val GSPLAT_GPU_PRODUCER_POST_SORT = 1
private const val GSPLAT_GPU_PRODUCER_PREPROJECT = 2
private const val GPU_MEASUREMENT_EXACT_CONTRIBUTOR_DRAW = 1 shl 6
private const val CPU_MEASUREMENT_EXACT_CONTRIBUTOR_DRAW = 1 shl 1
private const val CPU_MEASUREMENT_CONTRIBUTOR_COUNT_VALID = 1 shl 2
private const val ORDER_COUNTS_EXACT_CONTRIBUTOR_DRAW = 1 shl 0
private const val COUNT_SEMANTICS = "candidate_visible_contributor_issued_v1"
private const val CURRENT_STATS_SCHEMA = "gsplat-surface-current-stats/v1"
private const val RENDER_SHUTDOWN_TIMEOUT_MS = 1_000L

private fun gpuProducerValue(label: String): Int =
    when (label) {
        "preproject" -> GSPLAT_GPU_PRODUCER_PREPROJECT
        else -> GSPLAT_GPU_PRODUCER_POST_SORT
    }

private fun gpuProducerName(value: Int): String =
    when (value) {
        GSPLAT_GPU_PRODUCER_POST_SORT -> "post_sort"
        GSPLAT_GPU_PRODUCER_PREPROJECT -> "preproject"
        else -> error("unsupported GPU producer value: $value")
    }

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

private fun currentStatsPlanName(plan: GsplatSurfaceCurrentStatsPlan): String =
    when (plan) {
        GsplatSurfaceCurrentStatsPlan.CPU_POST_SORT -> "cpu_post_sort"
        GsplatSurfaceCurrentStatsPlan.GPU_POST_SORT -> "gpu_post_sort"
        GsplatSurfaceCurrentStatsPlan.GPU_PREPROJECT -> "gpu_preproject"
    }

private fun currentStatsCountSemanticsName(
    semantics: GsplatSurfaceCurrentStatsCountSemantics
): String = when (semantics) {
    GsplatSurfaceCurrentStatsCountSemantics.DIRECT_DRAW_EQUALS_VISIBLE ->
        "direct_draw_equals_visible"
    GsplatSurfaceCurrentStatsCountSemantics.INDIRECT_DRAW_EQUALS_VISIBLE ->
        "indirect_draw_equals_visible"
    GsplatSurfaceCurrentStatsCountSemantics.INDIRECT_DRAW_EQUALS_CONTRIBUTOR ->
        "indirect_draw_equals_contributor"
}

private fun currentStatsStatusDetail(display: SurfaceCurrentStatsDisplay): String =
    when (display) {
        is SurfaceCurrentStatsDisplay.Ready -> {
            val receipt = display.receipt
            "current_stats=ready source=${receipt.sourceCount} " +
                "visible=${receipt.visibleCount} contributor=${receipt.contributorCount} " +
                "drawn=${receipt.drawnCount} " +
                "plan=${currentStatsPlanName(receipt.identity.executedPlan)} " +
                "camera_revision=${receipt.identity.cameraRevision} " +
                "presentation_sequence=${receipt.identity.presentationSequence}"
        }
        is SurfaceCurrentStatsDisplay.Unavailable ->
            "current_stats=${display.reason} counts=unavailable " +
                "pending=${display.pendingCount}"
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

private fun adaptiveGpuFailureName(flags: Long): String? {
    if (flags and (1L shl 15) == 0L) return null
    return when ((flags shr 16) and 7L) {
        1L -> "unsupported"
        2L -> "initialization"
        3L -> "out_of_memory"
        4L -> "validation"
        else -> "unknown"
    }
}

private fun requireContributorCountContract(
    visible: Long,
    contributor: Long?,
    drawn: Long,
    exactContributorCompaction: Boolean,
    context: String
) {
    check(visible >= 0L && drawn >= 0L) { "$context reported a negative V/D count" }
    if (contributor != null) {
        check(contributor in 0L..visible) {
            "$context violates 0 <= C <= V: V=$visible C=$contributor"
        }
    }
    if (exactContributorCompaction) {
        checkNotNull(contributor) { "$context exact contributor draw omitted C" }
        check(drawn == contributor) {
            "$context exact contributor draw requires D=C: V=$visible C=$contributor D=$drawn"
        }
    } else {
        check(drawn == visible) {
            "$context legacy/downlevel draw requires D=V: V=$visible D=$drawn"
        }
    }
}

private data class BenchmarkOrderMeasurement(
    val ticket: Long,
    val cameraRevision: Long,
    val timingSource: String,
    val requestedBackend: Int,
    val actualBackend: Int,
    val adaptiveState: Int,
    val gpuPreprocessMs: Float?,
    val gpuRadixMs: Float?,
    val gpuOrderMs: Float?,
    val gpuCompleteMs: Float,
    val timestampPeriodNs: Float?,
    val visible: Long,
    val contributor: Long,
    val drawn: Long,
    val exactContributorCompaction: Boolean,
    val flags: Int
)

private data class BenchmarkOrderMeasurementFailure(
    val ticket: Long,
    val cameraRevision: Long,
    val reason: String,
    val requestedBackend: Int,
    val actualBackend: Int,
    val adaptiveState: Int,
    val flags: Int
)

private data class BenchmarkCpuOrderMeasurement(
    val ticket: Long,
    val cameraRevision: Long,
    val preprocessMs: Float,
    val sortMs: Float,
    val frameCompleteMs: Float,
    val requestedBackend: Int,
    val actualBackend: Int,
    val adaptiveState: Int,
    val visible: Long,
    val contributor: Long,
    val drawn: Long,
    val exactContributorCompaction: Boolean,
    val flags: Int
)

private data class IssuedOrderTicket(val cameraRevision: Long, val backend: Int)
private data class IssuedGpuProducerTicket(val cameraRevision: Long, val producer: Int)

private data class BenchmarkGpuProducerMeasurement(
    val ticket: Long,
    val cameraRevision: Long,
    val orderGeneration: Long,
    val projectionGeneration: Long,
    val frameCompleteMs: Float,
    val producer: Int,
    val source: Long,
    val contributor: Long,
    val drawn: Long,
    val drawScope: Int,
    val flags: Int
) {
    val orderRefreshed: Boolean get() = flags and 1 != 0
    val exactCurrentDraw: Boolean get() = flags and (1 shl 1) != 0
    val staleOrder: Boolean get() = flags and (1 shl 2) != 0
    val droppedPrior: Boolean get() = flags and (1 shl 3) != 0
}

private data class BenchmarkGpuProducerMeasurementFailure(
    val ticket: Long,
    val cameraRevision: Long,
    val orderGeneration: Long,
    val projectionGeneration: Long,
    val reason: Int,
    val producer: Int,
    val flags: Int
)

private data class BenchmarkGpuProducerSubmission(
    val ticket: Long?,
    val cameraRevision: Long,
    val requestedProducer: Int,
    val actualProducer: Int?,
    val orderBackend: Int,
    val projectedExecution: Int,
    val measurementEnabled: Boolean,
    val unsampledReason: String?,
    val flags: Int
) {
    companion object {
        fun query(handle: Long): Result<BenchmarkGpuProducerSubmission> {
            val raw = LongArray(7)
            val rc = NativeBridge.getSurfaceGpuProducerSubmissionV1(handle, raw)
            if (rc != 0) {
                return Result.failure(
                    IllegalStateException(
                        "getSurfaceGpuProducerSubmissionV1 failed rc=$rc " +
                            "error=${NativeBridge.lastErrorMessage()}"
                    )
                )
            }
            val flags = raw[6].toInt()
            val issued = flags and 1 != 0
            return Result.success(
                BenchmarkGpuProducerSubmission(
                    ticket = raw[0].takeIf { issued },
                    cameraRevision = raw[1],
                    requestedProducer = raw[2].toInt(),
                    actualProducer = raw[3].toInt().takeIf { it != 0 },
                    orderBackend = raw[4].toInt(),
                    projectedExecution = raw[5].toInt(),
                    measurementEnabled = flags and (1 shl 3) != 0,
                    unsampledReason = when {
                        flags and (1 shl 1) != 0 -> "ring_busy"
                        flags and (1 shl 2) != 0 -> "surface_unavailable"
                        else -> null
                    },
                    flags = flags
                )
            )
        }
    }
}

private data class BenchmarkOrderSubmission(
    val ticket: Long?,
    val cameraRevision: Long,
    val requestedBackend: Int,
    val actualBackend: Int,
    val adaptiveState: Int,
    val gpuRefresh: Boolean,
    val cpuFrameCompletionSample: Boolean,
    val unsampledReason: String?,
    val flags: Int
) {
    val measurementBackend: Int?
        get() = when {
            gpuRefresh -> GSPLAT_ORDER_BACKEND_GPU
            cpuFrameCompletionSample -> GSPLAT_ORDER_BACKEND_CPU
            else -> null
        }

    companion object {
        fun query(handle: Long): Result<BenchmarkOrderSubmission> {
            val raw = LongArray(6)
            val rc = NativeBridge.getSurfaceOrderSubmission(handle, raw)
            if (rc != 0) {
                return Result.failure(
                    IllegalStateException(
                        "getSurfaceOrderSubmission failed rc=$rc " +
                            "error=${NativeBridge.lastErrorMessage()}"
                    )
                )
            }
            val flags = raw[5].toInt()
            val issued = flags and (1 shl 1) != 0
            return Result.success(
                BenchmarkOrderSubmission(
                    ticket = raw[0].takeIf { issued },
                    cameraRevision = raw[1],
                    requestedBackend = raw[2].toInt(),
                    actualBackend = raw[3].toInt(),
                    adaptiveState = raw[4].toInt(),
                    gpuRefresh = flags and 1 != 0,
                    cpuFrameCompletionSample = flags and (1 shl 3) != 0,
                    unsampledReason = when {
                        flags and (1 shl 2) != 0 -> "ring_busy"
                        flags and (1 shl 4) != 0 -> "surface_unavailable"
                        else -> null
                    },
                    flags = flags
                )
            )
        }
    }
}

private data class BenchmarkExactnessReceipt(
    val source: Long,
    val decoded: Long,
    val encoded: Long,
    val resident: Long,
    val addressable: Long,
    val sourceShDegree: Int,
    val residentShDegree: Int,
    val qualityFlags: Int,
    val maxStorageBuffersPerShaderStage: Int,
    val maxStorageBufferBindingSize: Long
) {
    val fullQuality: Boolean
        get() = qualityFlags and 0b1_1111 == 0b1_1111 &&
            source == decoded && source == encoded && source == resident &&
            source == addressable && sourceShDegree == residentShDegree

    companion object {
        fun query(handle: Long): Result<BenchmarkExactnessReceipt> {
            val raw = LongArray(10)
            val rc = NativeBridge.getSurfaceExactness(handle, raw)
            if (rc != 0) {
                return Result.failure(
                    IllegalStateException(
                        "getSurfaceExactness failed rc=$rc error=${NativeBridge.lastErrorMessage()}"
                    )
                )
            }
            return Result.success(
                BenchmarkExactnessReceipt(
                    source = raw[0], decoded = raw[1], encoded = raw[2],
                    resident = raw[3], addressable = raw[4],
                    sourceShDegree = raw[5].toInt(), residentShDegree = raw[6].toInt(),
                    qualityFlags = raw[7].toInt(),
                    maxStorageBuffersPerShaderStage = raw[8].toInt(),
                    maxStorageBufferBindingSize = raw[9]
                )
            )
        }
    }
}

private data class BenchmarkPresentationReceipt(
    val requestedWidth: Int,
    val requestedHeight: Int,
    val surfaceWidth: Int,
    val surfaceHeight: Int,
    val internalRenderWidth: Int,
    val internalRenderHeight: Int,
    val presentedWidth: Int,
    val presentedHeight: Int,
    val presentedCameraRevision: Long,
    val lastFramePresented: Boolean,
    val everPresented: Boolean,
    val dynamicResolutionDisabled: Boolean,
    val upscalingDisabled: Boolean,
    val fullResolution: Boolean,
    val flags: Int
) {
    val dimensionsMatch: Boolean
        get() = requestedWidth == surfaceWidth && requestedHeight == surfaceHeight &&
            surfaceWidth == internalRenderWidth && surfaceHeight == internalRenderHeight &&
            internalRenderWidth == presentedWidth && internalRenderHeight == presentedHeight

    fun requireFormal(viewportWidth: Int, viewportHeight: Int) {
        check(requestedWidth > requestedHeight) {
            "formal Android benchmark requires a settled landscape Surface"
        }
        check(requestedWidth == viewportWidth && requestedHeight == viewportHeight) {
            "native requested size does not match the Activity Surface viewport"
        }
        check(lastFramePresented && everPresented) {
            "the terminal benchmark frame was not presented"
        }
        check(dynamicResolutionDisabled && upscalingDisabled) {
            "formal benchmark forbids dynamic resolution and upscaling"
        }
        check(fullResolution && dimensionsMatch) {
            "formal benchmark requires requested=Surface=internal=presented pixels"
        }
    }

    companion object {
        fun query(handle: Long): Result<BenchmarkPresentationReceipt> {
            val raw = LongArray(11)
            val rc = NativeBridge.getSurfacePresentation(handle, raw)
            if (rc != 0) {
                return Result.failure(
                    IllegalStateException(
                        "getSurfacePresentation failed rc=$rc error=${NativeBridge.lastErrorMessage()}"
                    )
                )
            }
            val flags = raw[9].toInt()
            return Result.success(
                BenchmarkPresentationReceipt(
                    requestedWidth = raw[0].toInt(),
                    requestedHeight = raw[1].toInt(),
                    surfaceWidth = raw[2].toInt(),
                    surfaceHeight = raw[3].toInt(),
                    internalRenderWidth = raw[4].toInt(),
                    internalRenderHeight = raw[5].toInt(),
                    presentedWidth = raw[6].toInt(),
                    presentedHeight = raw[7].toInt(),
                    presentedCameraRevision = raw[8],
                    lastFramePresented = flags and 1 != 0,
                    everPresented = flags and (1 shl 1) != 0,
                    dynamicResolutionDisabled = flags and (1 shl 2) != 0,
                    upscalingDisabled = flags and (1 shl 3) != 0,
                    fullResolution = flags and (1 shl 4) != 0,
                    flags = flags
                )
            ).onSuccess { receipt ->
                check(raw[10] == 0L) { "native presentation reserved field is non-zero" }
                check(receipt.requestedWidth > 0 && receipt.requestedHeight > 0)
                check(receipt.surfaceWidth > 0 && receipt.surfaceHeight > 0)
                check(receipt.internalRenderWidth > 0 && receipt.internalRenderHeight > 0)
            }
        }
    }
}

private fun logCompletedCpuOrderMeasurements(
    handle: Long,
    consume: (BenchmarkCpuOrderMeasurement) -> Unit = {}
): Int {
    var count = 0
    while (true) {
        val raw = LongArray(15)
        val rc = NativeBridge.pollSurfaceCpuOrderMeasurement(handle, raw)
        if (rc != 0) {
            Log.e(
                "GsplatExample",
                "pollSurfaceCpuOrderMeasurement failed rc=$rc error=${NativeBridge.lastErrorMessage()}"
            )
            return -1
        }
        if (raw[0] == 0L) return count
        val flags = raw[9].toInt()
        val countsFlags = raw[14].toInt()
        val exactContributorCompaction =
            countsFlags and ORDER_COUNTS_EXACT_CONTRIBUTOR_DRAW != 0
        check(exactContributorCompaction ==
            (flags and CPU_MEASUREMENT_EXACT_CONTRIBUTOR_DRAW != 0)) {
            "CPU timing and V/C/D receipts disagree on exact compaction"
        }
        check(flags and CPU_MEASUREMENT_CONTRIBUTOR_COUNT_VALID != 0 && raw[10] == raw[12]) {
            "CPU compatibility receipt disagrees with V/C/D contributor count"
        }
        requireContributorCountContract(
            raw[11], raw[12], raw[13], exactContributorCompaction, "CPU order receipt ${raw[1]}"
        )
        val measurement = BenchmarkCpuOrderMeasurement(
            ticket = raw[1],
            cameraRevision = raw[2],
            preprocessMs = Float.fromBits(raw[3].toInt()),
            sortMs = Float.fromBits(raw[4].toInt()),
            frameCompleteMs = Float.fromBits(raw[5].toInt()),
            requestedBackend = raw[6].toInt(),
            actualBackend = raw[7].toInt(),
            adaptiveState = raw[8].toInt(),
            visible = raw[11],
            contributor = raw[12],
            drawn = raw[13],
            exactContributorCompaction = exactContributorCompaction,
            flags = flags
        )
        Log.i(
            "GsplatExample",
            "CPU_ORDER_MEASUREMENT ticket=${measurement.ticket} " +
                "camera_revision=${measurement.cameraRevision} " +
                "requested_backend=${measurement.requestedBackend} " +
                "actual_backend=${measurement.actualBackend} " +
                "adaptive_state=${measurement.adaptiveState} " +
                "preprocess_ms=${measurement.preprocessMs} sort_ms=${measurement.sortMs} " +
                "frame_complete_ms=${measurement.frameCompleteMs} " +
                "visible=${measurement.visible} contributor=${measurement.contributor} " +
                "drawn=${measurement.drawn} " +
                "exact_contributor_compaction=${measurement.exactContributorCompaction} " +
                "flags=${measurement.flags}"
        )
        consume(measurement)
        count += 1
    }
}

private fun logCompletedOrderMeasurements(
    handle: Long,
    consume: (BenchmarkOrderMeasurement) -> Unit = {}
): Int {
    var count = 0
    val raw = LongArray(19)
    while (true) {
        val rc = NativeBridge.pollSurfaceOrderMeasurement(handle, raw)
        if (rc != 0) {
            Log.e("GsplatExample", "pollSurfaceOrderMeasurement failed rc=$rc error=${NativeBridge.lastErrorMessage()}")
            return -1
        }
        if (raw[0] == 0L) return count

        val flags = raw[14].toInt()
        fun optionalValue(index: Int, bit: Int): Float? =
            if ((flags and (1 shl bit)) != 0) Float.fromBits(raw[index].toInt()) else null
        val timingSource = when (raw[10].toInt()) {
            1 -> "timestamp_query"
            2 -> "completion"
            else -> "unknown"
        }
        val countsFlags = raw[18].toInt()
        val exactContributorCompaction =
            countsFlags and ORDER_COUNTS_EXACT_CONTRIBUTOR_DRAW != 0
        check(exactContributorCompaction ==
            (flags and GPU_MEASUREMENT_EXACT_CONTRIBUTOR_DRAW != 0)) {
            "GPU timing and V/C/D receipts disagree on exact compaction"
        }
        check(raw[8] == raw[15] && raw[9] == raw[17]) {
            "GPU timing and V/C/D receipts disagree on visible/drawn counts"
        }
        requireContributorCountContract(
            raw[15], raw[16], raw[17], exactContributorCompaction, "GPU order receipt ${raw[1]}"
        )
        val measurement = BenchmarkOrderMeasurement(
            ticket = raw[1],
            cameraRevision = raw[2],
            timingSource = timingSource,
            requestedBackend = raw[11].toInt(),
            actualBackend = raw[12].toInt(),
            adaptiveState = raw[13].toInt(),
            gpuPreprocessMs = optionalValue(3, 0),
            gpuRadixMs = optionalValue(4, 1),
            gpuOrderMs = optionalValue(5, 2),
            gpuCompleteMs = Float.fromBits(raw[6].toInt()),
            timestampPeriodNs = optionalValue(7, 3),
            visible = raw[15],
            contributor = raw[16],
            drawn = raw[17],
            exactContributorCompaction = exactContributorCompaction,
            flags = flags
        )
        Log.i(
            "GsplatExample",
            "ORDER_MEASUREMENT ticket=${measurement.ticket} camera_revision=${measurement.cameraRevision} " +
                "timing_source=${measurement.timingSource} requested_backend=${measurement.requestedBackend} " +
                "actual_backend=${measurement.actualBackend} adaptive_state=${measurement.adaptiveState} " +
                "gpu_preprocess_ms=${measurement.gpuPreprocessMs ?: "null"} " +
                "gpu_radix_ms=${measurement.gpuRadixMs ?: "null"} " +
                "gpu_order_ms=${measurement.gpuOrderMs ?: "null"} " +
                "gpu_complete_ms=${measurement.gpuCompleteMs} " +
                "timestamp_period_ns=${measurement.timestampPeriodNs ?: "null"} " +
                "visible=${measurement.visible} contributor=${measurement.contributor} " +
                "drawn=${measurement.drawn} " +
                "exact_contributor_compaction=${measurement.exactContributorCompaction} " +
                "flags=${measurement.flags}"
        )
        consume(measurement)
        count += 1
    }
}

private fun logCompletedOrderMeasurementFailures(
    handle: Long,
    consume: (BenchmarkOrderMeasurementFailure) -> Unit = {}
): Int {
    var count = 0
    val raw = LongArray(8)
    while (true) {
        val rc = NativeBridge.pollSurfaceOrderMeasurementFailure(handle, raw)
        if (rc != 0) {
            Log.e(
                "GsplatExample",
                "pollSurfaceOrderMeasurementFailure failed rc=$rc error=${NativeBridge.lastErrorMessage()}"
            )
            return -1
        }
        if (raw[0] == 0L) return count

        val failure = BenchmarkOrderMeasurementFailure(
            ticket = raw[1],
            cameraRevision = raw[2],
            reason = when (raw[3].toInt()) {
                1 -> "readback_map"
                2 -> "generation_invalidated"
                else -> "unknown_${raw[3]}"
            },
            requestedBackend = raw[4].toInt(),
            actualBackend = raw[5].toInt(),
            adaptiveState = raw[6].toInt(),
            flags = raw[7].toInt()
        )
        Log.e(
            "GsplatExample",
            "ORDER_MEASUREMENT_FAILURE ticket=${failure.ticket} " +
                "camera_revision=${failure.cameraRevision} reason=${failure.reason} " +
                "requested_backend=${failure.requestedBackend} " +
                "actual_backend=${failure.actualBackend} " +
                "adaptive_state=${failure.adaptiveState} flags=${failure.flags}"
        )
        consume(failure)
        count += 1
    }
}

private fun logCompletedGpuProducerMeasurements(
    handle: Long,
    consume: (BenchmarkGpuProducerMeasurement) -> Unit = {}
): Int {
    var count = 0
    while (true) {
        val raw = LongArray(12)
        val rc = NativeBridge.pollSurfaceGpuProducerMeasurementV1(handle, raw)
        if (rc != 0) {
            Log.e(
                "GsplatExample",
                "pollSurfaceGpuProducerMeasurementV1 failed rc=$rc " +
                    "error=${NativeBridge.lastErrorMessage()}"
            )
            return -1
        }
        if (raw[0] == 0L) return count
        val measurement = BenchmarkGpuProducerMeasurement(
            ticket = raw[1],
            cameraRevision = raw[2],
            orderGeneration = raw[3],
            projectionGeneration = raw[4],
            frameCompleteMs = Float.fromBits(raw[5].toInt()),
            producer = raw[6].toInt(),
            source = raw[7],
            contributor = raw[8],
            drawn = raw[9],
            drawScope = raw[10].toInt(),
            flags = raw[11].toInt()
        )
        Log.i(
            "GsplatExample",
            "GPU_PRODUCER_MEASUREMENT ticket=${measurement.ticket} " +
                "camera_revision=${measurement.cameraRevision} " +
                "producer=${gpuProducerName(measurement.producer)} " +
                "order_generation=${measurement.orderGeneration} " +
                "projection_generation=${measurement.projectionGeneration} " +
                "frame_complete_ms=${measurement.frameCompleteMs} " +
                "source=${measurement.source} contributor=${measurement.contributor} " +
                "drawn=${measurement.drawn} scope=${measurement.drawScope} " +
                "flags=${measurement.flags}"
        )
        consume(measurement)
        count += 1
    }
}

private fun logCompletedGpuProducerFailures(
    handle: Long,
    consume: (BenchmarkGpuProducerMeasurementFailure) -> Unit = {}
): Int {
    var count = 0
    while (true) {
        val raw = LongArray(8)
        val rc = NativeBridge.pollSurfaceGpuProducerFailureV1(handle, raw)
        if (rc != 0) {
            Log.e(
                "GsplatExample",
                "pollSurfaceGpuProducerFailureV1 failed rc=$rc " +
                    "error=${NativeBridge.lastErrorMessage()}"
            )
            return -1
        }
        if (raw[0] == 0L) return count
        val failure = BenchmarkGpuProducerMeasurementFailure(
            ticket = raw[1],
            cameraRevision = raw[2],
            orderGeneration = raw[3],
            projectionGeneration = raw[4],
            reason = raw[5].toInt(),
            producer = raw[6].toInt(),
            flags = raw[7].toInt()
        )
        Log.e(
            "GsplatExample",
            "GPU_PRODUCER_MEASUREMENT_FAILURE ticket=${failure.ticket} " +
                "camera_revision=${failure.cameraRevision} reason=${failure.reason} " +
                "producer=${gpuProducerName(failure.producer)} flags=${failure.flags}"
        )
        consume(failure)
        count += 1
    }
}

private fun flushCompletedOrderMeasurements(
    handle: Long,
    maxFrames: Int = 120,
    consume: (BenchmarkOrderMeasurement) -> Unit = {},
    consumeCpu: (BenchmarkCpuOrderMeasurement) -> Unit = {},
    consumeFailure: (BenchmarkOrderMeasurementFailure) -> Unit = {},
    consumeSubmission: (BenchmarkOrderSubmission) -> Unit,
    terminalsComplete: () -> Boolean,
    producerEnabled: Boolean = false,
    consumeProducer: (BenchmarkGpuProducerMeasurement) -> Unit = {},
    consumeProducerFailure: (BenchmarkGpuProducerMeasurementFailure) -> Unit = {},
    producerTerminalsComplete: () -> Boolean = { true },
    advanceCurrentStats: (renderedFrame: Boolean) -> Boolean,
    currentStatsTerminalsComplete: () -> Boolean
): Boolean {
    if (
        terminalsComplete() && producerTerminalsComplete() &&
        currentStatsTerminalsComplete()
    ) return true
    repeat(maxFrames) {
        var renderedFrame = false
        if (!producerEnabled) {
            val rc = NativeBridge.renderSurfaceFrame(handle)
            if (rc != 0) {
                Log.e("GsplatExample", "order measurement flush render failed rc=$rc")
                return false
            }
            renderedFrame = true
            val submission = BenchmarkOrderSubmission.query(handle).getOrElse { error ->
                Log.e("GsplatExample", "order submission flush query failed", error)
                return false
            }
            consumeSubmission(submission)
        }
        if (logCompletedCpuOrderMeasurements(handle, consumeCpu) < 0) return false
        if (logCompletedOrderMeasurements(handle, consume) < 0) return false
        if (logCompletedOrderMeasurementFailures(handle, consumeFailure) < 0) return false
        if (producerEnabled &&
            (logCompletedGpuProducerMeasurements(handle, consumeProducer) < 0 ||
                logCompletedGpuProducerFailures(handle, consumeProducerFailure) < 0)
        ) return false
        if (!advanceCurrentStats(renderedFrame)) return false
        if (
            terminalsComplete() && producerTerminalsComplete() &&
            currentStatsTerminalsComplete()
        ) return true
        if (producerEnabled) {
            // Producer qualification has already recorded every intended
            // frame/submission. Polling the native receipt pump advances queue
            // callbacks without issuing another ticket; a short bounded yield
            // prevents 120 immediate polls from expiring before a large-scene
            // GPU submission can complete.
            SystemClock.sleep(4)
        }
    }
    return terminalsComplete() && producerTerminalsComplete() &&
        currentStatsTerminalsComplete()
}

class MainActivity : Activity(), SurfaceHolder.Callback {
    private val renderLock = Object()
    private val renderSessionOwner = SurfaceRenderSessionOwner(renderLock)
    private val cameraCommandLock = Object()
    private lateinit var datasetPath: String
    private var datasetLabel = "pending"
    private lateinit var statusText: TextView
    private lateinit var surfaceView: SurfaceView
    private lateinit var sceneTitleText: TextView
    private lateinit var sceneMetaText: TextView
    private lateinit var studioPanel: LinearLayout
    private lateinit var studioButton: Button
    private var currentSurface: Surface? = null
    private var currentSurfaceWidth = 0
    private var currentSurfaceHeight = 0
    private var currentSurfaceRequest: SurfaceRenderSessionOwner.SurfaceRequest? = null
    private var restartAfterRendererRetires = false
    private var activityDestroying = false
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

        benchmarkConfig = BenchmarkConfig.fromIntent(intent, filesDir)
        benchmarkConfig.finalFrameRequest?.prepareForLaunch()
        if (benchmarkConfig.enabled) {
            requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE
        }
        setDataset(resolveInitialDataset())

        surfaceSizeLabel = "window"
        surfaceView = SurfaceView(this).apply {
            holder.addCallback(this@MainActivity)
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
        val request = renderSessionOwner.observeSurface(width, height)
        currentSurfaceRequest = request
        if (benchmarkConfig.enabled && width <= height) {
            restartAfterRendererRetires = false
            Log.i(TAG, "benchmark waiting for landscape Surface size=${width}x$height")
            updateStatus("state=waiting_for_landscape size=${width}x$height")
            return
        }
        synchronized(renderLock) {
            val handle = renderSessionOwner.activeHandle()
            if (handle != 0L) {
                val rc = NativeBridge.resizeSurfaceRenderer(handle, width, height)
                updateStatus("state=resized size=${width}x$height rc=$rc")
                return
            }
        }

        startRenderer(holder.surface, request)
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        Log.i(TAG, "surfaceDestroyed")
        currentSurface = null
        currentSurfaceWidth = 0
        currentSurfaceHeight = 0
        currentSurfaceRequest = null
        renderSessionOwner.forgetSurface()
        restartAfterRendererRetires = false
        updateStatus("state=surface_destroyed")
        stopRenderer()
    }

    override fun onDestroy() {
        activityDestroying = true
        restartAfterRendererRetires = false
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

    private fun startRenderer(
        surface: Surface,
        request: SurfaceRenderSessionOwner.SurfaceRequest
    ) {
        val width = request.width
        val height = request.height
        val session = when (val reservation = renderSessionOwner.reserve(request)) {
            is SurfaceRenderSessionOwner.ReserveResult.Acquired -> reservation.session
            is SurfaceRenderSessionOwner.ReserveResult.Busy -> {
                if (reservation.retiring) {
                    restartAfterRendererRetires = true
                    updateStatus(
                        "state=waiting_for_renderer_shutdown " +
                            "generation=${reservation.generation}"
                    )
                }
                return
            }
            is SurfaceRenderSessionOwner.ReserveResult.Stale -> {
                Log.i(
                    TAG,
                    "discarding stale Surface request generation=" +
                        "${reservation.requestGeneration} latest=" +
                        "${reservation.latestRequestGeneration}"
                )
                return
            }
        }
        restartAfterRendererRetires = false

        clearPendingCameraCommands()
        val thread = Thread(
            {
                var ownedHandle = 0L
                try {
                    Log.i(
                        TAG,
                        "createSurfaceRenderer start generation=${session.generation} " +
                            "surface_generation=${request.generation} " +
                            "size=${width}x$height geometry=${benchmarkConfig.geometryPath} " +
                            "dataset=$datasetPath"
                    )
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
                    ownedHandle = handle
                    if (handle == 0L) {
                        val rc = createError[0]
                        val detail = NativeBridge.lastErrorMessage()
                            .ifBlank { NativeBridge.errorMessage(rc) }
                        val message = detail.replace('\n', ' ').take(240)
                        Log.e(TAG, "createSurfaceRenderer failed rc=$rc error=$detail")
                        updateStatus("state=create_failed rc=$rc error=$message")
                        return@Thread
                    }

                    if (!renderSessionOwner.shouldRun(session)) {
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
                        updateStatus("state=create_failed rc=$sortIntervalRc error=$message")
                        return@Thread
                    }
                    val orderBackendRc = NativeBridge.setSurfaceOrderBackend(
                        handle,
                        orderBackendValue(benchmarkConfig.orderBackend)
                    )
                    if (orderBackendRc != 0) {
                        val message = NativeBridge.errorMessage(orderBackendRc)
                        Log.e(TAG, "setSurfaceOrderBackend failed rc=$orderBackendRc error=$message")
                        updateStatus("state=create_failed rc=$orderBackendRc error=$message")
                        return@Thread
                    }
                    val asyncSortRc = NativeBridge.setSurfaceAsyncSortEnabled(
                        handle,
                        benchmarkConfig.asyncSort
                    )
                    if (asyncSortRc != 0) {
                        val message = NativeBridge.errorMessage(asyncSortRc)
                        Log.e(TAG, "setSurfaceAsyncSortEnabled failed rc=$asyncSortRc error=$message")
                        updateStatus("state=create_failed rc=$asyncSortRc error=$message")
                        return@Thread
                    }
                    val frameLatencyRc = NativeBridge.setSurfaceFrameLatency(
                        handle,
                        benchmarkConfig.frameLatency
                    )
                    if (frameLatencyRc != 0) {
                        val message = NativeBridge.errorMessage(frameLatencyRc)
                        Log.e(TAG, "setSurfaceFrameLatency failed rc=$frameLatencyRc error=$message")
                        updateStatus("state=create_failed rc=$frameLatencyRc error=$message")
                        return@Thread
                    }
                    val gpuProducerRc = benchmarkConfig.gpuProducer?.let { producer ->
                        val projectedRc = NativeBridge.setSurfaceProjectedPolicyV1(
                            handle,
                            GSPLAT_PROJECTED_POLICY_COMPACT
                        )
                        if (projectedRc != 0) {
                            projectedRc
                        } else {
                            val producerRc = NativeBridge.setSurfaceGpuOrderProducerV1(
                                handle,
                                gpuProducerValue(producer)
                            )
                            if (producerRc != 0) {
                                producerRc
                            } else {
                                NativeBridge.setSurfaceGpuProducerMeasurementEnabledV1(handle, true)
                            }
                        }
                    } ?: 0
                    if (gpuProducerRc != 0) {
                        val detail = NativeBridge.lastErrorMessage()
                            .ifBlank { NativeBridge.errorMessage(gpuProducerRc) }
                        val message = detail.replace('\n', ' ').take(240)
                        Log.e(
                            TAG,
                            "configure GPU producer diagnostic failed " +
                                "rc=$gpuProducerRc error=$detail"
                        )
                        updateStatus("state=create_failed rc=$gpuProducerRc error=$message")
                        return@Thread
                    }
                    val cameraTraceRc = benchmarkConfig.cameraTracePath?.let { tracePath ->
                        val initialFrame = if (benchmarkConfig.cameraTraceSequence) {
                            benchmarkConfig.cameraTraceFrameIndices.last()
                        } else {
                            benchmarkConfig.cameraTraceFrame
                        }
                        BenchmarkBridge.setSurfaceCameraTraceFrame(
                            handle,
                            tracePath,
                            initialFrame,
                            benchmarkConfig.requireTraceDisplayMatch
                        )
                    } ?: 0
                    if (cameraTraceRc != 0) {
                        val detail = NativeBridge.lastErrorMessage()
                            .ifBlank { NativeBridge.errorMessage(cameraTraceRc) }
                        val message = detail.replace('\n', ' ').take(240)
                        Log.e(
                            TAG,
                            "setSurfaceCameraTraceFrame failed rc=$cameraTraceRc error=$detail"
                        )
                        updateStatus("state=create_failed rc=$cameraTraceRc error=$message")
                        return@Thread
                    }
                    if (benchmarkConfig.cameraTracePath != null) {
                        val metadata = checkNotNull(benchmarkConfig.cameraTraceMetadata)
                        val mode = if (benchmarkConfig.cameraTraceSequence) {
                            "trace_sequence"
                        } else {
                            "fixed_frame"
                        }
                        val selected = if (benchmarkConfig.cameraTraceSequence) {
                            benchmarkConfig.cameraTraceFrameIndices.joinToString(",")
                        } else {
                            benchmarkConfig.cameraTraceFrame.toString()
                        }
                        val receiptFrame = if (benchmarkConfig.cameraTraceSequence) {
                            benchmarkConfig.cameraTraceFrameIndices.first()
                        } else {
                            benchmarkConfig.cameraTraceFrame
                        }
                        cameraStatus = "camera=trace mode=$mode frames=$selected"
                        Log.i(
                            TAG,
                            "CAMERA_TRACE trace_id=${metadata.id} " +
                                "trace_sha256=${metadata.sha256} " +
                                "mode=$mode frame_indices=$selected frame_index=$receiptFrame " +
                                "timestamp_ns=${metadata.timestamps[receiptFrame]} " +
                                "requested_backend=${benchmarkConfig.orderBackend}"
                        )
                    }
                    if (!renderSessionOwner.publishHandle(session, handle)) {
                        Log.i(
                            TAG,
                            "discarding stopped renderer generation=${session.generation} " +
                                "handle=$handle"
                        )
                        return@Thread
                    }

                    var frameCount = 0L
                    var consecutiveErrors = 0L
                    var lastStatusAt = 0L
                    var lastPublishedCurrentStatsVersion = -1L
                    val sortStats = LongArray(7)
                    val exactness = BenchmarkExactnessReceipt.query(handle).getOrElse { error ->
                        Log.e(TAG, "SURFACE_EXACTNESS_FAILED ${error.message}")
                        updateStatus("state=exactness_error error=${error.message}")
                        renderSessionOwner.requestStop(session)
                        return@Thread
                    }
                    Log.i(
                        TAG,
                        "SURFACE_EXACTNESS source=${exactness.source} decoded=${exactness.decoded} " +
                            "encoded=${exactness.encoded} resident=${exactness.resident} " +
                            "addressable=${exactness.addressable} source_sh=${exactness.sourceShDegree} " +
                            "resident_sh=${exactness.residentShDegree} flags=${exactness.qualityFlags} " +
                            "max_storage_buffers_per_shader_stage=${exactness.maxStorageBuffersPerShaderStage} " +
                            "max_storage_buffer_binding_size=${exactness.maxStorageBufferBindingSize}"
                    )
                    if (benchmarkConfig.geometryPath != "paged" && !exactness.fullQuality) {
                        Log.e(TAG, "SURFACE_EXACTNESS_REJECTED full-quality invariant failed")
                        updateStatus("state=exactness_rejected")
                        renderSessionOwner.requestStop(session)
                        return@Thread
                    }
                    val benchmark = SurfaceBenchmark(
                        benchmarkConfig,
                        currentThermalStatus(),
                        exactness
                    )
                    val currentStats = SurfaceCurrentStatsConsumer()
                    while (
                        renderSessionOwner.shouldRun(session) &&
                        !Thread.currentThread().isInterrupted
                    ) {
                        val traceStep = benchmark.nextTraceStep()
                        val iterationStartNs = System.nanoTime()
                        val benchmarkStatsBinding = benchmark.currentStatsBinding(traceStep)
                        val uiSamplingDue = !benchmark.enabled &&
                            !currentStats.hasInFlight &&
                            iterationStartNs - lastStatusAt > STATUS_INTERVAL_NS
                        val currentStatsBinding = benchmarkStatsBinding ?: if (uiSamplingDue) {
                            currentStats.nextUiBinding()
                        } else {
                            null
                        }
                        var renderCallNs = 0L
                        val renderStartNs = System.nanoTime()
                        val transaction = performSurfaceRenderTransaction(
                            renderLock = renderLock,
                            applyCommand = {
                                if (traceStep != null && benchmark.config.cameraTraceSequence) {
                                    BenchmarkBridge.setSurfaceCameraTraceFrame(
                                        handle,
                                        checkNotNull(benchmark.config.cameraTracePath),
                                        traceStep.traceFrameIndex,
                                        benchmark.config.requireTraceDisplayMatch
                                    )
                                } else if (traceStep != null) {
                                    // Fixed-trace mode was applied transactionally before the
                                    // render loop. Do not force a fresh sort every frame merely
                                    // to attach the same trace identity to its runtime receipt.
                                    0
                                } else if (
                                    benchmark.enabled && benchmark.config.cameraTracePath == null
                                ) {
                                    NativeBridge.orbitSurfaceRenderer(
                                        handle,
                                        benchmark.config.yawStepRadians,
                                        0f
                                    )
                                } else {
                                    applyPendingCameraCommands(handle)
                                }
                            },
                            closeCurrentStatsOnCommandFailure =
                                currentStats::closeOutstandingAfterCommandFailure,
                            requestCurrentStats = currentStatsBinding?.let { binding ->
                                {
                                    currentStats.request(handle, binding)
                                    Unit
                                }
                            },
                            stopOnRequestFailure = benchmarkStatsBinding != null,
                            render = { NativeBridge.renderSurfaceFrame(handle) },
                            observeRequestedRenderFailure = currentStats::renderFailed,
                            reconcileAfterSuccessfulRender = {
                                currentStats.afterSuccessfulRender(handle)
                                Unit
                            }
                        ).also { renderCallNs = System.nanoTime() - renderStartNs }
                        if (transaction.commandFailureClosedCurrentStats) {
                            Log.e(
                                TAG,
                                "camera command failed with an outstanding current-stats intent; " +
                                    "closing renderer rc=${transaction.commandRc}"
                            )
                            updateStatus(
                                "state=current_stats_session_closed " +
                                    "reason=command_failed rc=${transaction.commandRc}"
                            )
                            renderSessionOwner.requestStop(session)
                            continue
                        }
                        val currentStatsRequestError = transaction.requestError
                        if (benchmarkStatsBinding != null && currentStatsRequestError != null) {
                            val error = currentStatsRequestError
                            Log.e(TAG, "strict current-stats request failed", error)
                            updateStatus(
                                "state=benchmark_current_stats_request_error " +
                                    "error=${compactMessage(error)}"
                            )
                            renderSessionOwner.requestStop(session)
                            continue
                        }
                        currentStatsRequestError?.let { error ->
                            Log.e(TAG, "UI current-stats request failed; rendering continues", error)
                        }
                        val currentStatsReconciliationError = transaction.reconciliationError
                        if (currentStatsReconciliationError != null) {
                            if (benchmark.enabled) {
                                Log.e(
                                    TAG,
                                    "strict current-stats reconciliation failed",
                                    currentStatsReconciliationError
                                )
                                updateStatus(
                                    "state=benchmark_current_stats_error " +
                                        "error=${compactMessage(currentStatsReconciliationError)}"
                                )
                                renderSessionOwner.requestStop(session)
                                continue
                            }
                            Log.e(
                                TAG,
                                "UI current-stats reconciliation failed; rendering continues",
                                currentStatsReconciliationError
                            )
                        }
                        val rc = transaction.rc
                        frameCount += 1
                        if (rc != 0) {
                            if (transaction.commandRc != 0) {
                                Log.e(TAG, "applyPendingCameraCommands failed rc=${transaction.commandRc}")
                            }
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
                                val cameraReceiptResult = BenchmarkCameraReceipt.query(handle)
                                if (cameraReceiptResult.isFailure) {
                                    Log.e(
                                        TAG,
                                        "benchmark camera receipt query failed",
                                        cameraReceiptResult.exceptionOrNull()
                                    )
                                    updateStatus("state=benchmark_camera_receipt_error")
                                    renderSessionOwner.requestStop(session)
                                    continue
                                }
                                val cameraReceipt = cameraReceiptResult.getOrThrow()
                                val submissionResult = BenchmarkOrderSubmission.query(handle)
                                if (submissionResult.isFailure) {
                                    Log.e(
                                        TAG,
                                        "benchmark order submission query failed",
                                        submissionResult.exceptionOrNull()
                                    )
                                    updateStatus("state=benchmark_submission_error")
                                    renderSessionOwner.requestStop(session)
                                    continue
                                }
                                val submission = submissionResult.getOrThrow()
                                benchmark.recordOrderSubmission(submission)
                                val producerSubmissionResult =
                                    benchmark.config.gpuProducer?.let {
                                        BenchmarkGpuProducerSubmission.query(handle)
                                    }
                                if (producerSubmissionResult?.isFailure == true) {
                                    Log.e(
                                        TAG,
                                        "benchmark GPU producer submission query failed",
                                        producerSubmissionResult.exceptionOrNull()
                                    )
                                    updateStatus("state=benchmark_gpu_producer_submission_error")
                                    renderSessionOwner.requestStop(session)
                                    continue
                                }
                                val producerSubmission = producerSubmissionResult?.getOrThrow()
                                producerSubmission?.let(benchmark::recordGpuProducerSubmission)
                                val sortStatsRc = NativeBridge.getSurfaceSortStats(handle, sortStats)
                                if (sortStatsRc == 0) {
                                    if (logCompletedCpuOrderMeasurements(
                                            handle,
                                            benchmark::recordCpuOrderMeasurement
                                        ) < 0
                                        || logCompletedOrderMeasurements(
                                            handle,
                                            benchmark::recordOrderMeasurement
                                        ) < 0
                                        || logCompletedOrderMeasurementFailures(
                                            handle,
                                            benchmark::recordOrderMeasurementFailure
                                        ) < 0
                                        || (benchmark.config.gpuProducer != null &&
                                            (logCompletedGpuProducerMeasurements(
                                                handle,
                                                benchmark::recordGpuProducerMeasurement
                                            ) < 0 || logCompletedGpuProducerFailures(
                                                handle,
                                                benchmark::recordGpuProducerMeasurementFailure
                                            ) < 0))
                                    ) {
                                        updateStatus("state=benchmark_measurement_error")
                                        renderSessionOwner.requestStop(session)
                                        continue
                                    }
                                    if (traceStep != null) {
                                        val metadata = checkNotNull(benchmark.config.cameraTraceMetadata)
                                        Log.i(
                                            TAG,
                                            "CAMERA_TRACE_FRAME trace_id=${metadata.id} " +
                                                "trace_sha256=${metadata.sha256} phase=${traceStep.phase} " +
                                                "loop=${traceStep.loopIndex} phase_frame=${traceStep.phaseFrameIndex} " +
                                                "frame_index=${traceStep.traceFrameIndex} " +
                                                "timestamp_ns=${traceStep.timestampNs} " +
                                                "requested_backend=${benchmark.config.orderBackend}"
                                        )
                                    }
                                    val recordResult = runCatching {
                                        benchmark.record(
                                            sortStats,
                                            submission,
                                            producerSubmission,
                                            renderCallNs,
                                            System.nanoTime() - iterationStartNs,
                                            traceStep,
                                            cameraReceipt,
                                            currentStats
                                        )
                                    }
                                    if (recordResult.isFailure) {
                                        val error = checkNotNull(recordResult.exceptionOrNull())
                                        Log.e(TAG, "strict benchmark frame rejected", error)
                                        updateStatus(
                                            "state=benchmark_frame_error " +
                                                "error=${compactMessage(error)}"
                                        )
                                        renderSessionOwner.requestStop(session)
                                        continue
                                    }
                                    if (benchmark.complete) {
                                        if (
                                            !flushCompletedOrderMeasurements(
                                                handle,
                                                consume = benchmark::recordOrderMeasurement,
                                                consumeCpu = benchmark::recordCpuOrderMeasurement,
                                                consumeFailure = benchmark::recordOrderMeasurementFailure,
                                                consumeSubmission = benchmark::recordOrderSubmission,
                                                terminalsComplete = benchmark::orderTerminalsComplete,
                                                producerEnabled = benchmark.config.gpuProducer != null,
                                                consumeProducer = benchmark::recordGpuProducerMeasurement,
                                                consumeProducerFailure =
                                                    benchmark::recordGpuProducerMeasurementFailure,
                                                producerTerminalsComplete =
                                                    benchmark::gpuProducerTerminalsComplete,
                                                advanceCurrentStats = { renderedFrame ->
                                                    runCatching {
                                                        synchronized(renderLock) {
                                                            if (renderedFrame) {
                                                                currentStats.afterSuccessfulRender(handle)
                                                            } else {
                                                                currentStats.pollPending(handle)
                                                            }
                                                        }
                                                    }.onFailure { error ->
                                                        Log.e(
                                                            TAG,
                                                            "current-stats terminal flush failed",
                                                            error
                                                        )
                                                    }.isSuccess
                                                },
                                                currentStatsTerminalsComplete = {
                                                    currentStats.benchmarkTerminalsComplete(
                                                        benchmark.measuredSampleCount
                                                    )
                                                }
                                            )
                                        ) {
                                            updateStatus("state=benchmark_measurement_flush_error")
                                            renderSessionOwner.requestStop(session)
                                            continue
                                        }
                                        val artifactResult = finalizeBenchmarkArtifact(
                                            prepareEvidence = {
                                                val presentation = BenchmarkPresentationReceipt
                                                    .query(handle)
                                                    .getOrThrow()
                                                presentation.requireFormal(
                                                    currentSurfaceWidth,
                                                    currentSurfaceHeight
                                                )
                                                PreparedBenchmarkArtifact(
                                                    resultLine = benchmark.resultLine(
                                                        datasetLabel,
                                                        currentStats
                                                    ),
                                                    records = benchmark.artifactLines(
                                                        datasetLabel = datasetLabel,
                                                        datasetPath = datasetPath,
                                                        presentation = presentation,
                                                        physicalDisplayWidth =
                                                            display?.mode?.physicalWidth,
                                                        physicalDisplayHeight =
                                                            display?.mode?.physicalHeight,
                                                        density = resources.displayMetrics.density,
                                                        refreshHz = display?.refreshRate?.toDouble(),
                                                        thermalStatusEnd = currentThermalStatus(),
                                                        currentStats = currentStats
                                                    )
                                                )
                                            },
                                            captureFinalFrame = benchmark.config.finalFrameRequest
                                                ?.let { request ->
                                                    {
                                                        BenchmarkSurfaceCapture(surfaceView).capture(
                                                            request,
                                                            currentSurfaceWidth,
                                                            currentSurfaceHeight
                                                        )
                                                    }
                                                },
                                            publishEvidence = { prepared ->
                                                Log.i(TAG, prepared.resultLine)
                                                prepared.records.forEach { (prefix, json) ->
                                                    logBenchmarkArtifact(prefix, json)
                                                }
                                            }
                                        )
                                        if (artifactResult.isFailure) {
                                            val error = artifactResult.exceptionOrNull()
                                            Log.e(TAG, "formal benchmark artifact rejected", error)
                                            updateStatus(
                                                "state=benchmark_artifact_error " +
                                                    "error=${compactMessage(checkNotNull(error))}"
                                            )
                                            renderSessionOwner.requestStop(session)
                                            continue
                                        }
                                        val result = artifactResult.getOrThrow()
                                        updateStatus("state=benchmark_complete $result")
                                        renderSessionOwner.requestStop(session)
                                    }
                                } else {
                                    Log.e(TAG, "benchmark sort stats failed rc=$sortStatsRc")
                                    updateStatus("state=benchmark_sort_stats_error rc=$sortStatsRc")
                                    renderSessionOwner.requestStop(session)
                                }
                            }
                            val currentStatsDisplayChanged =
                                currentStats.displayVersion != lastPublishedCurrentStatsVersion
                            if (
                                BenchmarkUiState.shouldPublishPeriodicRenderStatus(
                                    running = renderSessionOwner.shouldRun(session),
                                    elapsedSinceLastStatusNs = now - lastStatusAt,
                                    statusIntervalNs = STATUS_INTERVAL_NS
                                ) || (!benchmark.enabled && currentStatsDisplayChanged)
                            ) {
                                lastStatusAt = now
                                lastPublishedCurrentStatsVersion = currentStats.displayVersion
                                val detail = "state=rendering frames=$frameCount " +
                                    currentStatsStatusDetail(currentStats.display) +
                                    " call=${formatNanos(renderCallNs)}ms"
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
                    if (ownedHandle != 0L) {
                        synchronized(renderLock) {
                            renderSessionOwner.clearOwnedHandle(session, ownedHandle)
                        }
                        Log.i(
                            TAG,
                            "destroySurfaceRenderer generation=${session.generation} " +
                                "handle=$ownedHandle"
                        )
                        // The owner slot remains occupied until finish(), so a blocked
                        // native destroy cannot make a replacement session eligible or
                        // hold renderLock across a lifecycle callback.
                        NativeBridge.destroySurfaceRenderer(ownedHandle)
                    }
                    check(renderSessionOwner.finish(session, Thread.currentThread())) {
                        "render session generation ${session.generation} lost thread ownership"
                    }
                    resumeLatestSurfaceAfterRetirement(session.generation)
                }
            },
            "gsplat-surface-render-${session.generation}"
        )
        check(renderSessionOwner.attachThread(session, thread)) {
            "render session generation ${session.generation} lost its reserved slot"
        }
        thread.start()
    }

    private fun stopRenderer() {
        when (
            val result = renderSessionOwner.requestStopAndAwait(
                RENDER_SHUTDOWN_TIMEOUT_MS
            )
        ) {
            SurfaceRenderSessionOwner.StopResult.Idle,
            is SurfaceRenderSessionOwner.StopResult.Stopped -> Unit
            is SurfaceRenderSessionOwner.StopResult.Retiring -> {
                Log.w(
                    TAG,
                    "render generation ${result.generation} did not stop within " +
                        "${RENDER_SHUTDOWN_TIMEOUT_MS}ms; retaining owner slot fail-closed"
                )
                updateStatus(
                    "state=renderer_retiring generation=${result.generation} " +
                        "new_session=blocked"
                )
            }
        }
    }

    private fun resumeLatestSurfaceAfterRetirement(generation: Long) {
        runOnUiThread {
            if (!restartAfterRendererRetires || activityDestroying) return@runOnUiThread
            val surface = currentSurface
            val request = currentSurfaceRequest
            if (surface == null || !surface.isValid || request == null) {
                restartAfterRendererRetires = false
                updateStatus("state=renderer_retired waiting_for_surface")
                return@runOnUiThread
            }
            Log.i(
                TAG,
                "render generation $generation retired; starting latest Surface " +
                    "generation=${request.generation} ${request.width}x${request.height}"
            )
            startRenderer(surface, request)
        }
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
        val request = currentSurfaceRequest
        stopRenderer()

        if (surface != null && surface.isValid && request != null) {
            startRenderer(surface, request)
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

    /**
     * logd truncates a single entry at roughly 4 KiB. A complete terminal
     * ticket ledger is legitimately larger, so oversized artifact records are
     * emitted as indexed base64 chunks with one whole-record SHA-256 receipt.
     */
    private fun logBenchmarkArtifact(prefix: String, json: String) {
        val direct = "$prefix$json"
        if (direct.toByteArray(Charsets.UTF_8).size <= BENCHMARK_LOG_DIRECT_MAX_BYTES) {
            Log.i(TAG, direct)
            return
        }

        val record = when (prefix) {
            BENCHMARK_MANIFEST_PREFIX -> "manifest"
            BENCHMARK_FRAME_PREFIX -> "frame"
            BENCHMARK_SUMMARY_PREFIX -> "summary"
            else -> error("unknown benchmark artifact prefix")
        }
        val runId = JSONObject(json).getString("run_id")
        val bytes = json.toByteArray(Charsets.UTF_8)
        val digest = MessageDigest.getInstance("SHA-256")
            .digest(bytes)
            .joinToString("") { "%02x".format(it) }
        val total = (bytes.size + BENCHMARK_LOG_CHUNK_BYTES - 1) /
            BENCHMARK_LOG_CHUNK_BYTES
        for (index in 0 until total) {
            val start = index * BENCHMARK_LOG_CHUNK_BYTES
            val end = minOf(start + BENCHMARK_LOG_CHUNK_BYTES, bytes.size)
            val payload = Base64.encodeToString(
                bytes.copyOfRange(start, end),
                Base64.NO_WRAP
            )
            Log.i(
                TAG,
                "$BENCHMARK_CHUNK_PREFIX" +
                    "record=$record run_id=$runId index=$index total=$total " +
                    "encoding=base64 sha256=$digest payload=$payload"
            )
        }
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
        if (latestStatus.startsWith("state=benchmark_complete")) {
            return BenchmarkUiState.completeSceneStatus(
                residentSplats = statusValue("resident_splats"),
                drawnSplats = statusValue("avg_drawn"),
                avgFrameMs = statusValue("avg_frame_ms")
            )
        }
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
            val cameraMode = when {
                benchmarkConfig.cameraTraceSequence -> "trace_sequence"
                benchmarkConfig.cameraTracePath != null -> "fixed_camera"
                else -> "orbit"
            }
            appendLine(
                "benchmark=$cameraMode frames=${benchmarkConfig.frames} " +
                    "warmup=${benchmarkConfig.warmupFrames} loops=${benchmarkConfig.cameraTraceLoops}"
            )
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
        private const val EXTRA_CAMERA_TRACE_PATH = "gsplat_camera_trace_path"
        private const val EXTRA_CAMERA_TRACE_FRAME = "gsplat_camera_trace_frame"
        private const val EXTRA_CAMERA_TRACE_SEQUENCE = "gsplat_camera_trace_sequence"
        private const val EXTRA_CAMERA_TRACE_FRAME_INDICES = "gsplat_camera_frame_indices"
        private const val EXTRA_CAMERA_TRACE_LOOPS = "gsplat_camera_trace_loops"
        private const val EXTRA_REQUIRE_TRACE_DISPLAY_MATCH = "gsplat_require_trace_display_match"
        private const val EXTRA_SURFACE_SORT_INTERVAL = "gsplat_surface_sort_interval"
        private const val EXTRA_SURFACE_ASYNC_SORT = "gsplat_surface_async_sort"
        private const val EXTRA_SURFACE_FRAME_LATENCY = "gsplat_surface_frame_latency"
        private const val EXTRA_SURFACE_GEOMETRY_PATH = "gsplat_geometry_path"
        private const val EXTRA_SURFACE_ORDER_BACKEND = "gsplat_surface_order_backend"
        private const val EXTRA_SURFACE_GPU_PRODUCER = "gsplat_surface_gpu_producer"
        private const val EXTRA_SURFACE_GPU_PRODUCER_MEASUREMENT =
            "gsplat_surface_gpu_producer_measurement"
        private const val EXTRA_BENCHMARK_FINAL_PNG_PATH = "gsplat_benchmark_final_png_path"
        private const val DEFAULT_BENCHMARK_FRAMES = 120
        private const val DEFAULT_BENCHMARK_WARMUP_FRAMES = 10
        private const val DEFAULT_BENCHMARK_YAW_STEP = 0.001f
        private const val DEFAULT_SURFACE_SORT_INTERVAL = 1
        private const val DEFAULT_SURFACE_ASYNC_SORT = false
        private const val DEFAULT_SURFACE_FRAME_LATENCY = 2
        private const val DEFAULT_SURFACE_GEOMETRY_PATH = "packed"
        private const val DEFAULT_SURFACE_ORDER_BACKEND = "adaptive"
        private const val TARGET_FRAME_INTERVAL_NS = 16_666_667L
        private const val BENCHMARK_MANIFEST_PREFIX = "GSPLAT_BENCHMARK_MANIFEST "
        private const val BENCHMARK_FRAME_PREFIX = "GSPLAT_BENCHMARK_FRAME "
        private const val BENCHMARK_SUMMARY_PREFIX = "GSPLAT_BENCHMARK_SUMMARY "
        private const val BENCHMARK_CHUNK_PREFIX = "GSPLAT_BENCHMARK_CHUNK "
        private const val BENCHMARK_LOG_DIRECT_MAX_BYTES = 3_500
        private const val BENCHMARK_LOG_CHUNK_BYTES = 1_800
        private const val CAMERA_RECEIPT_TOLERANCE = 5.0e-5

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
        // Complete resident production path by default; Direct and Paged remain explicit A/B knobs.
        val geometryPath: String = DEFAULT_SURFACE_GEOMETRY_PATH,
        // Explicit strict diagnostic only; null keeps PostSort telemetry off.
        val gpuProducer: String? = null,
        val cameraTracePath: String? = null,
        val cameraTraceFrame: Int = 0,
        val cameraTraceSequence: Boolean = false,
        val cameraTraceFrameIndices: List<Int> = emptyList(),
        val cameraTraceLoops: Int = 1,
        val requireTraceDisplayMatch: Boolean = true,
        val cameraTraceMetadata: CameraTraceMetadata? = null,
        val finalFrameRequest: BenchmarkFinalFrameRequest? = null
    ) {
        companion object {
            fun fromIntent(intent: Intent, appFilesDir: File): BenchmarkConfig {
                val benchmarkEnabled = intent.getBooleanExtra(EXTRA_BENCHMARK, false)
                val finalFrameRequest = BenchmarkFinalFrameRequest.parse(
                    benchmarkEnabled = benchmarkEnabled,
                    requestedPath = intent.getStringExtra(EXTRA_BENCHMARK_FINAL_PNG_PATH),
                    appFilesDir = appFilesDir
                )
                val cameraTracePath = intent.getStringExtra(EXTRA_CAMERA_TRACE_PATH)
                    ?.trim()
                    ?.takeIf(String::isNotEmpty)
                val cameraTraceMetadata = cameraTracePath?.let(CameraTraceMetadata::load)
                val cameraTraceSequence = intent.getBooleanExtra(EXTRA_CAMERA_TRACE_SEQUENCE, false)
                check(!cameraTraceSequence || cameraTraceMetadata != null) {
                    "$EXTRA_CAMERA_TRACE_SEQUENCE requires $EXTRA_CAMERA_TRACE_PATH"
                }
                check(!cameraTraceSequence || !intent.hasExtra(EXTRA_CAMERA_TRACE_FRAME)) {
                    "$EXTRA_CAMERA_TRACE_FRAME cannot be combined with $EXTRA_CAMERA_TRACE_SEQUENCE"
                }
                val frameIndicesText = intent.getStringExtra(EXTRA_CAMERA_TRACE_FRAME_INDICES)
                val cameraTraceFrameIndices = if (cameraTraceSequence) {
                    frameIndicesText?.let(::parseFrameIndices)
                        ?: cameraTraceMetadata!!.timestamps.indices.toList()
                } else {
                    check(frameIndicesText == null && !intent.hasExtra(EXTRA_CAMERA_TRACE_LOOPS)) {
                        "camera trace sequence extras require $EXTRA_CAMERA_TRACE_SEQUENCE=true"
                    }
                    emptyList()
                }
                check(!cameraTraceSequence || cameraTraceFrameIndices.size >= 2) {
                    "camera trace sequence requires at least two frame indices"
                }
                cameraTraceMetadata?.validateFrameIndices(cameraTraceFrameIndices)

                val defaultFrames = if (cameraTraceSequence) {
                    cameraTraceFrameIndices.size
                } else {
                    DEFAULT_BENCHMARK_FRAMES
                }
                val frames = intent
                    .getIntExtra(EXTRA_BENCHMARK_FRAMES, defaultFrames)
                    .coerceAtLeast(1)
                val defaultWarmup = if (cameraTraceSequence) 0 else DEFAULT_BENCHMARK_WARMUP_FRAMES
                val warmupFrames = intent
                    .getIntExtra(EXTRA_BENCHMARK_WARMUP_FRAMES, defaultWarmup)
                    .coerceAtLeast(0)
                val yawStep = intent
                    .getFloatExtra(EXTRA_BENCHMARK_YAW_STEP, DEFAULT_BENCHMARK_YAW_STEP)
                    .takeIf { it.isFinite() }
                    ?: DEFAULT_BENCHMARK_YAW_STEP
                val defaultSortInterval = if (cameraTraceSequence) 1 else DEFAULT_SURFACE_SORT_INTERVAL
                val sortInterval = intent
                    .getIntExtra(EXTRA_SURFACE_SORT_INTERVAL, defaultSortInterval)
                    .coerceAtLeast(1)
                check(!cameraTraceSequence || sortInterval == 1) {
                    "camera trace sequence requires $EXTRA_SURFACE_SORT_INTERVAL=1"
                }
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
                val gpuProducer = intent.getStringExtra(EXTRA_SURFACE_GPU_PRODUCER)
                    ?.trim()
                    ?.lowercase(Locale.US)
                    ?.takeIf(String::isNotEmpty)
                check(gpuProducer == null || gpuProducer == "post_sort" || gpuProducer == "preproject") {
                    "$EXTRA_SURFACE_GPU_PRODUCER must be post_sort or preproject"
                }
                val gpuProducerMeasurement = intent.getBooleanExtra(
                    EXTRA_SURFACE_GPU_PRODUCER_MEASUREMENT,
                    false
                )
                check((gpuProducer != null) == gpuProducerMeasurement) {
                    "$EXTRA_SURFACE_GPU_PRODUCER and " +
                        "$EXTRA_SURFACE_GPU_PRODUCER_MEASUREMENT=true are required together"
                }
                if (gpuProducer != null) {
                    check(intent.getBooleanExtra(EXTRA_BENCHMARK, false)) {
                        "GPU producer diagnostics require benchmark mode"
                    }
                    check(geometryPath == "packed" && orderBackend == "gpu" && !asyncSort) {
                        "GPU producer diagnostics require packed + GPU + synchronous ordering"
                    }
                    check(sortInterval == 1 && cameraTraceSequence) {
                        "GPU producer diagnostics require sort_interval=1 and trace sequence playback"
                    }
                }
                val cameraTraceFrame = intent
                    .getIntExtra(EXTRA_CAMERA_TRACE_FRAME, 0)
                    .coerceAtLeast(0)
                if (!cameraTraceSequence && cameraTraceMetadata != null) {
                    cameraTraceMetadata.validateFrameIndices(listOf(cameraTraceFrame))
                }
                val cameraTraceLoops = intent
                    .getIntExtra(EXTRA_CAMERA_TRACE_LOOPS, 1)
                    .coerceAtLeast(1)
                val requireTraceDisplayMatch = intent.getBooleanExtra(
                    EXTRA_REQUIRE_TRACE_DISPLAY_MATCH,
                    true
                )
                requireFormalFinalFrameProtocol(
                    request = finalFrameRequest,
                    requireTraceDisplayMatch = requireTraceDisplayMatch,
                    traceWidth = cameraTraceMetadata?.width,
                    traceHeight = cameraTraceMetadata?.height,
                    geometryPath = geometryPath
                )
                return BenchmarkConfig(
                    enabled = benchmarkEnabled,
                    frames = frames,
                    warmupFrames = warmupFrames,
                    yawStepRadians = yawStep,
                    sortInterval = sortInterval,
                    asyncSort = asyncSort,
                    frameLatency = frameLatency,
                    orderBackend = orderBackend,
                    geometryPath = geometryPath,
                    gpuProducer = gpuProducer,
                    cameraTracePath = cameraTracePath,
                    cameraTraceFrame = cameraTraceFrame,
                    cameraTraceSequence = cameraTraceSequence,
                    cameraTraceFrameIndices = cameraTraceFrameIndices,
                    cameraTraceLoops = cameraTraceLoops,
                    requireTraceDisplayMatch = requireTraceDisplayMatch,
                    cameraTraceMetadata = cameraTraceMetadata,
                    finalFrameRequest = finalFrameRequest
                )
            }

            private fun parseFrameIndices(value: String): List<Int> {
                check(value.isNotBlank()) { "$EXTRA_CAMERA_TRACE_FRAME_INDICES must not be empty" }
                val indices = value.split(',').map { part ->
                    part.trim().toIntOrNull()?.takeIf { it >= 0 }
                        ?: error("$EXTRA_CAMERA_TRACE_FRAME_INDICES must contain non-negative integers")
                }
                check(indices.toSet().size == indices.size) {
                    "$EXTRA_CAMERA_TRACE_FRAME_INDICES contains duplicates"
                }
                return indices
            }
        }
    }

    private data class CameraTraceMetadata(
        val id: String,
        val sha256: String,
        val fileSha256: String,
        val width: Int,
        val height: Int,
        val timestamps: LongArray
    ) {
        fun validateFrameIndices(indices: List<Int>) {
            check(indices.all { it in timestamps.indices }) {
                "camera trace frame index is out of range for ${timestamps.size} frames"
            }
        }

        companion object {
            fun load(path: String): CameraTraceMetadata {
                val bytes = File(path).readBytes()
                val root = JSONObject(bytes.toString(Charsets.UTF_8))
                check(root.getString("schema") == "gsplat-camera-trace/v1")
                val id = root.getString("trace_id")
                val sha256 = root.getString("content_sha256")
                check(id.isNotEmpty() && sha256.matches(Regex("[0-9a-f]{64}")))
                val coordinateSystem = root.getJSONObject("coordinate_system")
                check(coordinateSystem.getString("handedness") == "right")
                check(coordinateSystem.getString("axes") == "RUF")
                check(coordinateSystem.getString("camera_forward") == "+Z")
                val convention = root.getJSONObject("matrix_convention")
                check(convention.getString("storage_order") == "row-major")
                check(convention.getString("vector_convention") == "column")
                check(
                    convention.getString("composition") ==
                        "projection * view * world_position"
                )
                check(convention.getString("ndc_xy") == "[-1,1]")
                check(convention.getString("ndc_z") == "[0,1]")
                check(convention.getString("clip_w") == "camera_z")
                val display = root.getJSONObject("display")
                val width = display.getInt("width")
                val height = display.getInt("height")
                check(width > 0 && height > 0)
                val frames = root.getJSONArray("frames")
                check(frames.length() > 0)
                val timestamps = LongArray(frames.length()) { index ->
                    val frame = frames.getJSONObject(index)
                    check(frame.getInt("frame_index") == index)
                    frame.getLong("timestamp_ns")
                }
                check(timestamps.indices.drop(1).all { timestamps[it] > timestamps[it - 1] })
                val fileSha256 = MessageDigest.getInstance("SHA-256")
                    .digest(bytes)
                    .joinToString("") { "%02x".format(it) }
                return CameraTraceMetadata(id, sha256, fileSha256, width, height, timestamps)
            }
        }
    }

    private data class CameraTraceStep(
        val phase: String,
        val loopIndex: Int,
        val phaseFrameIndex: Int,
        val measuredSampleIndex: Int?,
        val traceFrameIndex: Int,
        val timestampNs: Long
    )

    private class SurfaceBenchmark(
        val config: BenchmarkConfig,
        private val thermalStatusStart: Int?,
        private val exactness: BenchmarkExactnessReceipt
    ) {
        val enabled: Boolean = config.enabled
        var complete = false
            private set

        val measuredSampleCount = if (config.cameraTraceSequence) {
            Math.multiplyExact(config.frames, config.cameraTraceLoops)
        } else {
            config.frames
        }

        private val runId = UUID.randomUUID().toString()
        private val runStartedAtMs = System.currentTimeMillis()
        private val callNs = LongArray(measuredSampleCount)
        private val frameWallNs = LongArray(measuredSampleCount)
        private val elapsedNs = LongArray(measuredSampleCount)
        private val cameraRevision = LongArray(measuredSampleCount)
        private val appliedOrderRevision = LongArray(measuredSampleCount)
        private val scheduledRevision = LongArray(measuredSampleCount)
        private val completedRevision = LongArray(measuredSampleCount)
        private val presentedOrderLag = LongArray(measuredSampleCount)
        private val observedResultLag = LongArray(measuredSampleCount)
        private val sortFlags = LongArray(measuredSampleCount)
        private val orderSubmissionTicket = LongArray(measuredSampleCount)
        private val orderSubmissionFlags = LongArray(measuredSampleCount)
        private val gpuProducerSubmissionTicket = LongArray(measuredSampleCount)
        private val gpuProducerSubmissionFlags = LongArray(measuredSampleCount)
        private val traceFrameIndex = IntArray(measuredSampleCount) { -1 }
        private val traceTimestampNs = LongArray(measuredSampleCount) { -1L }
        private val traceLoopIndex = IntArray(measuredSampleCount) { -1 }
        private val cameraReceipts = arrayOfNulls<BenchmarkCameraReceipt>(measuredSampleCount)
        private var observedFrames = 0
        private var samples = 0
        private var measurementStartNs = 0L
        private var measurementStartedAtMs = 0L
        private var measurementEndedAtMs = 0L
        private var totalCallNs = 0L
        private var totalFrameWallNs = 0L
        private val orderMeasurementsByRevision = LinkedHashMap<Long, BenchmarkOrderMeasurement>()
        private val orderMeasurementsByTicket = LinkedHashMap<Long, BenchmarkOrderMeasurement>()
        private val cpuOrderMeasurementsByRevision =
            LinkedHashMap<Long, BenchmarkCpuOrderMeasurement>()
        private val cpuOrderMeasurementsByTicket = LinkedHashMap<Long, BenchmarkCpuOrderMeasurement>()
        private val orderFailuresByRevision = LinkedHashMap<Long, BenchmarkOrderMeasurementFailure>()
        private val orderFailuresByTicket = LinkedHashMap<Long, BenchmarkOrderMeasurementFailure>()
        private val issuedOrderTickets = LinkedHashMap<Long, IssuedOrderTicket>()
        private val unsampledOrderRequests = ArrayList<String>()
        private val gpuProducerMeasurementsByTicket =
            LinkedHashMap<Long, BenchmarkGpuProducerMeasurement>()
        private val gpuProducerFailuresByTicket =
            LinkedHashMap<Long, BenchmarkGpuProducerMeasurementFailure>()
        private val issuedGpuProducerTickets = LinkedHashMap<Long, IssuedGpuProducerTicket>()
        private val unsampledGpuProducerRequests = ArrayList<String>()

        fun recordOrderSubmission(submission: BenchmarkOrderSubmission) {
            check(submission.requestedBackend == orderBackendValue(config.orderBackend)) {
                "order submission requested backend does not match the benchmark"
            }
            check(submission.adaptiveState in 0..6) {
                "order submission reports unknown adaptive state ${submission.adaptiveState}"
            }
            check(!(submission.gpuRefresh && submission.cpuFrameCompletionSample)) {
                "order submission requested both CPU and GPU measurement"
            }
            val backend = submission.measurementBackend
            if (backend == null) {
                check(submission.ticket == null && submission.unsampledReason == null) {
                    "non-measured frame published measurement state"
                }
                return
            }
            check(submission.actualBackend == backend) {
                "order measurement backend $backend does not match frame ${submission.actualBackend}"
            }
            val ticket = submission.ticket
            if (ticket == null) {
                check(submission.unsampledReason != null) {
                    "order measurement request omitted a ticket without unsampled evidence"
                }
                unsampledOrderRequests +=
                    "backend=$backend revision=${submission.cameraRevision} reason=${submission.unsampledReason}"
                return
            }
            check(submission.unsampledReason == null) {
                "order measurement both issued a ticket and reported unsampled"
            }
            check(ticket > 0L) { "order measurement ticket must be positive" }
            val previous = issuedOrderTickets.put(
                ticket,
                IssuedOrderTicket(submission.cameraRevision, backend)
            )
            check(previous == null) {
                "order measurement ticket $ticket was issued more than once"
            }
        }

        fun recordGpuProducerSubmission(submission: BenchmarkGpuProducerSubmission) {
            val expectedLabel = checkNotNull(config.gpuProducer) {
                "GPU producer submission arrived while diagnostics are disabled"
            }
            val expectedProducer = gpuProducerValue(expectedLabel)
            check(submission.flags and 0b1111 == submission.flags) {
                "GPU producer submission reports unknown flags ${submission.flags}"
            }
            check(submission.measurementEnabled) {
                "GPU producer submission reports measurement disabled"
            }
            check(submission.requestedProducer == expectedProducer) {
                "GPU producer submission requested ${submission.requestedProducer}, " +
                    "expected $expectedProducer"
            }
            check(submission.actualProducer == expectedProducer) {
                "GPU producer submission executed ${submission.actualProducer}, " +
                    "expected $expectedProducer"
            }
            check(submission.orderBackend == GSPLAT_ORDER_BACKEND_GPU) {
                "GPU producer diagnostic executed a non-GPU order lane"
            }
            check(submission.projectedExecution == GSPLAT_PROJECTED_POLICY_COMPACT) {
                "GPU producer diagnostic did not execute Compact projected draw"
            }
            val ticket = submission.ticket
            if (ticket == null) {
                check(submission.unsampledReason != null) {
                    "GPU producer diagnostic omitted a ticket without an unsampled reason"
                }
                unsampledGpuProducerRequests +=
                    "producer=$expectedLabel revision=${submission.cameraRevision} " +
                    "reason=${submission.unsampledReason}"
                return
            }
            check(submission.unsampledReason == null) {
                "GPU producer submission both issued a ticket and reported unsampled"
            }
            check(ticket > 0L) { "GPU producer ticket must be positive" }
            val previous = issuedGpuProducerTickets.put(
                ticket,
                IssuedGpuProducerTicket(submission.cameraRevision, expectedProducer)
            )
            check(previous == null) { "GPU producer ticket $ticket was issued more than once" }
        }

        fun recordGpuProducerMeasurement(measurement: BenchmarkGpuProducerMeasurement) {
            val expectedProducer = gpuProducerValue(checkNotNull(config.gpuProducer))
            check(
                issuedGpuProducerTickets[measurement.ticket] ==
                    IssuedGpuProducerTicket(measurement.cameraRevision, expectedProducer)
            ) { "GPU producer success does not match an issued ticket/revision" }
            check(measurement.producer == expectedProducer) {
                "GPU producer receipt reports ${measurement.producer}, expected $expectedProducer"
            }
            check(measurement.frameCompleteMs.isFinite() && measurement.frameCompleteMs >= 0f) {
                "GPU producer receipt contains invalid completion timing"
            }
            check(measurement.orderGeneration > 0L && measurement.projectionGeneration > 0L) {
                "GPU producer receipt contains an uninitialized generation"
            }
            check(measurement.source == exactness.source) {
                "GPU producer receipt source ${measurement.source} != ${exactness.source}"
            }
            check(measurement.drawScope == 1 && measurement.exactCurrentDraw) {
                "GPU producer diagnostic requires exact-current contributor scope"
            }
            check(measurement.orderRefreshed && !measurement.staleOrder) {
                "GPU producer diagnostic retained stale order"
            }
            check(!measurement.droppedPrior) {
                "GPU producer receipt queue dropped an older result"
            }
            check(measurement.contributor in 0L..measurement.source) {
                "GPU producer receipt violates 0 <= C <= S"
            }
            check(measurement.drawn == measurement.contributor) {
                "GPU producer Compact receipt requires D=C"
            }
            check(gpuProducerFailuresByTicket[measurement.ticket] == null) {
                "GPU producer ticket ${measurement.ticket} produced success and failure"
            }
            check(gpuProducerMeasurementsByTicket.put(measurement.ticket, measurement) == null) {
                "GPU producer ticket ${measurement.ticket} produced duplicate successes"
            }
        }

        fun recordGpuProducerMeasurementFailure(
            failure: BenchmarkGpuProducerMeasurementFailure
        ) {
            val expectedProducer = gpuProducerValue(checkNotNull(config.gpuProducer))
            check(
                issuedGpuProducerTickets[failure.ticket] ==
                    IssuedGpuProducerTicket(failure.cameraRevision, expectedProducer)
            ) { "GPU producer failure does not match an issued ticket/revision" }
            check(failure.producer == expectedProducer) {
                "GPU producer failure reports the wrong producer"
            }
            check(failure.reason in 1..3 && failure.flags and 1 == failure.flags) {
                "GPU producer failure contains an invalid reason or flags"
            }
            check(gpuProducerMeasurementsByTicket[failure.ticket] == null) {
                "GPU producer ticket ${failure.ticket} produced success and failure"
            }
            check(gpuProducerFailuresByTicket.put(failure.ticket, failure) == null) {
                "GPU producer ticket ${failure.ticket} produced duplicate failures"
            }
        }

        fun gpuProducerTerminalsComplete(): Boolean {
            if (config.gpuProducer == null || unsampledGpuProducerRequests.isNotEmpty()) return true
            return issuedGpuProducerTickets.keys.all { ticket ->
                gpuProducerMeasurementsByTicket[ticket] != null ||
                    gpuProducerFailuresByTicket[ticket] != null
            }
        }

        private fun gpuProducerMeasurement(index: Int): BenchmarkGpuProducerMeasurement? {
            val ticket = gpuProducerSubmissionTicket[index]
            return ticket.takeIf { it > 0L }?.let(gpuProducerMeasurementsByTicket::get)
        }

        private fun requireGpuProducerMeasurements() {
            if (config.gpuProducer == null) {
                check(
                    issuedGpuProducerTickets.isEmpty() &&
                        gpuProducerMeasurementsByTicket.isEmpty() &&
                        gpuProducerFailuresByTicket.isEmpty() &&
                        unsampledGpuProducerRequests.isEmpty()
                ) { "GPU producer evidence appeared while diagnostics are disabled" }
                return
            }
            check(unsampledGpuProducerRequests.isEmpty()) {
                "GPU producer measurement was unsampled: $unsampledGpuProducerRequests"
            }
            for ((ticket, issued) in issuedGpuProducerTickets) {
                val success = gpuProducerMeasurementsByTicket[ticket]
                val failure = gpuProducerFailuresByTicket[ticket]
                check(listOf(success, failure).count { it != null } == 1) {
                    "GPU producer ticket $ticket revision ${issued.cameraRevision} " +
                        "did not produce exactly one terminal receipt"
                }
            }
            check(gpuProducerFailuresByTicket.isEmpty()) {
                val failure = gpuProducerFailuresByTicket.values.first()
                "GPU producer measurement failed: ticket=${failure.ticket} reason=${failure.reason}"
            }
            val measuredTickets = HashSet<Long>()
            for (index in 0 until samples) {
                val ticket = gpuProducerSubmissionTicket[index]
                check(ticket > 0L && measuredTickets.add(ticket)) {
                    "frame $index lacks a unique GPU producer ticket"
                }
                check(gpuProducerSubmissionFlags[index] and 1L != 0L) {
                    "frame $index GPU producer submission did not mark ticket issued"
                }
                val measurement = checkNotNull(gpuProducerMeasurementsByTicket[ticket]) {
                    "frame $index lacks a terminal GPU producer receipt"
                }
                check(measurement.cameraRevision == cameraRevision[index]) {
                    "frame $index producer receipt revision does not match presentation"
                }
            }
        }

        fun recordOrderMeasurement(measurement: BenchmarkOrderMeasurement) {
            check(
                issuedOrderTickets[measurement.ticket] ==
                    IssuedOrderTicket(measurement.cameraRevision, GSPLAT_ORDER_BACKEND_GPU)
            ) {
                "GPU success terminal does not match an issued ticket/revision"
            }
            check(measurement.actualBackend == GSPLAT_ORDER_BACKEND_GPU) {
                "GPU order receipt reports non-GPU backend ${measurement.actualBackend}"
            }
            check(measurement.requestedBackend == orderBackendValue(config.orderBackend)) {
                "GPU order receipt requested backend does not match the benchmark"
            }
            check(measurement.adaptiveState in 0..6) {
                "GPU order receipt reports unknown adaptive state ${measurement.adaptiveState}"
            }
            requireContributorCountContract(
                measurement.visible,
                measurement.contributor,
                measurement.drawn,
                measurement.exactContributorCompaction,
                "GPU order receipt ${measurement.ticket}"
            )
            check(measurement.visible <= exactness.source) {
                "GPU order receipt count exceeds source count"
            }
            check(measurement.flags and (1 shl 5) == 0) {
                "GPU order receipt queue dropped an older measurement"
            }
            check(orderFailuresByTicket[measurement.ticket] == null) {
                "GPU order ticket ${measurement.ticket} produced both success and failure"
            }
            val previousTicket = orderMeasurementsByTicket.put(measurement.ticket, measurement)
            check(previousTicket == null) {
                "GPU order ticket ${measurement.ticket} produced multiple success receipts"
            }
            val previous = orderMeasurementsByRevision.put(measurement.cameraRevision, measurement)
            check(previous == null || previous.ticket == measurement.ticket) {
                "multiple GPU order tickets reported camera revision ${measurement.cameraRevision}"
            }
        }

        fun recordCpuOrderMeasurement(measurement: BenchmarkCpuOrderMeasurement) {
            check(
                issuedOrderTickets[measurement.ticket] ==
                    IssuedOrderTicket(measurement.cameraRevision, GSPLAT_ORDER_BACKEND_CPU)
            ) {
                "CPU success terminal does not match an issued ticket/revision"
            }
            check(measurement.actualBackend == GSPLAT_ORDER_BACKEND_CPU) {
                "CPU order receipt reports non-CPU backend ${measurement.actualBackend}"
            }
            check(measurement.requestedBackend == orderBackendValue(config.orderBackend)) {
                "CPU order receipt requested backend does not match the benchmark"
            }
            check(measurement.adaptiveState in 0..6) {
                "CPU order receipt reports unknown adaptive state ${measurement.adaptiveState}"
            }
            check(
                measurement.preprocessMs.isFinite() && measurement.preprocessMs >= 0f &&
                    measurement.sortMs.isFinite() && measurement.sortMs >= 0f &&
                    measurement.frameCompleteMs.isFinite() && measurement.frameCompleteMs >= 0f
            ) { "CPU order receipt contains invalid timing" }
            requireContributorCountContract(
                measurement.visible,
                measurement.contributor,
                measurement.drawn,
                measurement.exactContributorCompaction,
                "CPU order receipt ${measurement.ticket}"
            )
            check(measurement.visible <= exactness.source) {
                "CPU order receipt count exceeds source count"
            }
            check(measurement.flags and 1 == 0) {
                "CPU order receipt queue dropped an older measurement"
            }
            check(orderFailuresByTicket[measurement.ticket] == null) {
                "CPU order ticket ${measurement.ticket} produced both success and failure"
            }
            check(orderMeasurementsByTicket[measurement.ticket] == null) {
                "CPU order ticket ${measurement.ticket} produced CPU and GPU success"
            }
            val previous = cpuOrderMeasurementsByTicket.put(measurement.ticket, measurement)
            check(previous == null) {
                "CPU order ticket ${measurement.ticket} produced multiple success receipts"
            }
            val previousRevision = cpuOrderMeasurementsByRevision.put(
                measurement.cameraRevision,
                measurement
            )
            check(previousRevision == null || previousRevision.ticket == measurement.ticket) {
                "multiple CPU order tickets reported camera revision ${measurement.cameraRevision}"
            }
        }

        fun recordOrderMeasurementFailure(failure: BenchmarkOrderMeasurementFailure) {
            val issued = checkNotNull(issuedOrderTickets[failure.ticket]) {
                "order failure terminal does not match an issued ticket"
            }
            check(issued.cameraRevision == failure.cameraRevision) {
                "order failure terminal revision does not match its issued ticket"
            }
            check(failure.actualBackend == issued.backend) {
                "order failure backend does not match its issued ticket"
            }
            check(failure.requestedBackend == orderBackendValue(config.orderBackend)) {
                "GPU order failure requested backend does not match the benchmark"
            }
            check(failure.adaptiveState in 0..6) {
                "GPU order failure reports unknown adaptive state ${failure.adaptiveState}"
            }
            check(failure.reason == "readback_map" || failure.reason == "generation_invalidated") {
                "order failure reports unknown reason ${failure.reason}"
            }
            check(issued.backend != GSPLAT_ORDER_BACKEND_CPU || failure.reason != "readback_map") {
                "CPU order ticket reported a GPU-only readback failure"
            }
            check(failure.flags and 1 == 0) {
                "GPU order failure queue dropped an older terminal receipt"
            }
            check(
                orderMeasurementsByTicket[failure.ticket] == null &&
                    cpuOrderMeasurementsByTicket[failure.ticket] == null
            ) {
                "order ticket ${failure.ticket} produced both success and failure"
            }
            val previousTicket = orderFailuresByTicket.put(failure.ticket, failure)
            check(previousTicket == null) {
                "GPU order ticket ${failure.ticket} produced multiple failure receipts"
            }
            val previous = orderFailuresByRevision.put(failure.cameraRevision, failure)
            check(previous == null || previous.ticket == failure.ticket) {
                "multiple GPU order failures reported camera revision ${failure.cameraRevision}"
            }
        }

        private fun orderMeasurementForFrame(index: Int): BenchmarkOrderMeasurement? =
            orderSubmissionTicket[index]
                .takeIf { it > 0L }
                ?.let(orderMeasurementsByTicket::get)

        private fun cpuOrderMeasurementForFrame(index: Int): BenchmarkCpuOrderMeasurement? =
            orderSubmissionTicket[index]
                .takeIf { it > 0L }
                ?.let(cpuOrderMeasurementsByTicket::get)

        fun orderTerminalsComplete(): Boolean {
            if (unsampledOrderRequests.isNotEmpty()) return true
            return issuedOrderTickets.keys.all { ticket ->
                orderMeasurementsByTicket[ticket] != null ||
                    cpuOrderMeasurementsByTicket[ticket] != null ||
                    orderFailuresByTicket[ticket] != null
            }
        }

        private fun currentStatsReady(
            index: Int,
            currentStats: SurfaceCurrentStatsConsumer
        ) = (currentStats.recordForSample(index)?.terminal as?
            SurfaceCurrentStatsTerminal.Ready)?.receipt

        private fun resolvedVisible(
            index: Int,
            currentStats: SurfaceCurrentStatsConsumer
        ): Long = checkNotNull(currentStatsReady(index, currentStats)).visibleCount

        private fun resolvedDrawn(
            index: Int,
            currentStats: SurfaceCurrentStatsConsumer
        ): Long = checkNotNull(currentStatsReady(index, currentStats)).drawnCount

        private fun resolvedContributor(
            index: Int,
            currentStats: SurfaceCurrentStatsConsumer
        ): Long = checkNotNull(currentStatsReady(index, currentStats)).contributorCount

        private fun resolvedExactContributorCompaction(
            index: Int,
            currentStats: SurfaceCurrentStatsConsumer
        ): Boolean = checkNotNull(currentStatsReady(index, currentStats)).countSemantics ==
            GsplatSurfaceCurrentStatsCountSemantics.INDIRECT_DRAW_EQUALS_CONTRIBUTOR

        private fun requireOrderMeasurements() {
            check(unsampledOrderRequests.isEmpty()) {
                "order measurement was unsampled: $unsampledOrderRequests"
            }
            for ((ticket, issued) in issuedOrderTickets) {
                val gpuSuccess = orderMeasurementsByTicket[ticket]
                val cpuSuccess = cpuOrderMeasurementsByTicket[ticket]
                val failure = orderFailuresByTicket[ticket]
                val terminalCount = listOf(gpuSuccess, cpuSuccess, failure).count { it != null }
                check(terminalCount == 1) {
                    "issued order ticket $ticket revision ${issued.cameraRevision} " +
                        "did not produce exactly one terminal receipt"
                }
                val terminalRevision = gpuSuccess?.cameraRevision
                    ?: cpuSuccess?.cameraRevision
                    ?: failure?.cameraRevision
                check(terminalRevision == issued.cameraRevision) {
                    "order ticket $ticket terminal revision does not match its submission"
                }
            }
            check(orderFailuresByTicket.isEmpty()) {
                val failure = orderFailuresByTicket.values.first()
                "order measurement failed: ticket=${failure.ticket} " +
                    "camera_revision=${failure.cameraRevision} reason=${failure.reason}"
            }
            val seenTickets = HashSet<Long>()
            for (index in 0 until samples) {
                val gpuFrame = ((sortFlags[index] shr 9) and 3L) == 1L
                val refreshed = sortFlags[index] and 1L != 0L
                val submittedTicket = orderSubmissionTicket[index].takeIf { it > 0L }
                if (submittedTicket == null) {
                    check(!refreshed || config.geometryPath == "paged") {
                        "refreshed measured frame $index lacks its own order ticket"
                    }
                    continue
                }
                if (!gpuFrame) {
                    if (refreshed && config.geometryPath != "paged") {
                        check(orderSubmissionFlags[index] and (1L shl 3) != 0L) {
                            "CPU order refresh did not request comparable queue-completion timing"
                        }
                        check(orderSubmissionFlags[index] and (1L shl 1) != 0L) {
                            "CPU order refresh did not issue a measurement ticket"
                        }
                    }
                    val measurement = checkNotNull(cpuOrderMeasurementForFrame(index)) {
                        "missing CPU completion receipt for measured frame $index"
                    }
                    check(submittedTicket == measurement.ticket) {
                        "CPU terminal ticket does not match measured frame submission"
                    }
                    check(seenTickets.add(measurement.ticket)) {
                        "duplicate CPU order receipt ticket ${measurement.ticket}"
                    }
                    requireContributorCountContract(
                        measurement.visible,
                        measurement.contributor,
                        measurement.drawn,
                        measurement.exactContributorCompaction,
                        "CPU frame $index ticket ${measurement.ticket}"
                    )
                    continue
                }
                if (refreshed) {
                    check(orderSubmissionFlags[index] and 1L != 0L) {
                        "GPU order refresh did not request GPU timing"
                    }
                    check(orderSubmissionFlags[index] and (1L shl 1) != 0L) {
                        "GPU order refresh did not issue a measurement ticket"
                    }
                    check(orderFailuresByRevision[cameraRevision[index]] == null) {
                        "GPU order ticket failed for camera revision ${cameraRevision[index]}"
                    }
                }
                val measurement = checkNotNull(orderMeasurementForFrame(index)) {
                    "missing GPU order receipt for measured frame $index"
                }
                check(submittedTicket == measurement.ticket) {
                    "GPU terminal ticket does not match the measured frame submission"
                }
                check(seenTickets.add(measurement.ticket)) {
                    "duplicate GPU order receipt ticket ${measurement.ticket}"
                }
                check(measurement.requestedBackend == orderBackendValue(config.orderBackend)) {
                    "GPU order receipt requested backend does not match the benchmark"
                }
            }
        }

        fun nextTraceStep(): CameraTraceStep? {
            if (!enabled || complete || config.cameraTraceMetadata == null) return null
            val metadata = checkNotNull(config.cameraTraceMetadata)
            val playbackIndex = observedFrames
            val measuredIndex = playbackIndex - config.warmupFrames
            val warmup = measuredIndex < 0
            val loopIndex = if (warmup || !config.cameraTraceSequence) {
                0
            } else {
                measuredIndex / config.frames
            }
            val phaseFrameIndex = if (warmup) playbackIndex else measuredIndex % config.frames
            val traceFrameIndex = if (config.cameraTraceSequence) {
                config.cameraTraceFrameIndices[
                    phaseFrameIndex % config.cameraTraceFrameIndices.size
                ]
            } else {
                config.cameraTraceFrame
            }
            return CameraTraceStep(
                phase = if (warmup) "warmup" else "measure",
                loopIndex = loopIndex,
                phaseFrameIndex = phaseFrameIndex,
                measuredSampleIndex = if (warmup) null else measuredIndex,
                traceFrameIndex = traceFrameIndex,
                timestampNs = metadata.timestamps[traceFrameIndex]
            )
        }

        fun currentStatsBinding(
            traceStep: CameraTraceStep?
        ): SurfaceCurrentStatsFrameBinding? {
            if (!enabled || complete) return null
            val sampleIndex = traceStep?.measuredSampleIndex
                ?: (observedFrames - config.warmupFrames).takeIf { it >= 0 }
                ?: return null
            if (sampleIndex !in 0 until measuredSampleCount) return null
            return SurfaceCurrentStatsFrameBinding(
                frameId = sampleIndex.toLong() + 1L,
                sampleIndex = sampleIndex,
                traceFrameIndex = traceStep?.traceFrameIndex,
                traceTimestampNs = traceStep?.timestampNs
            )
        }

        fun record(
            sortStats: LongArray,
            submission: BenchmarkOrderSubmission,
            producerSubmission: BenchmarkGpuProducerSubmission?,
            renderCallNs: Long,
            frameIterationNs: Long,
            traceStep: CameraTraceStep?,
            cameraReceipt: BenchmarkCameraReceipt,
            currentStats: SurfaceCurrentStatsConsumer
        ) {
            if (!enabled || complete) {
                return
            }
            cameraReceipt.requirePresented(sortStats[0])
            config.cameraTraceMetadata?.let { metadata ->
                if (config.requireTraceDisplayMatch) {
                    check(
                        cameraReceipt.surfaceWidth == metadata.width &&
                            cameraReceipt.surfaceHeight == metadata.height
                    ) {
                        "native camera receipt Surface does not match the trace display"
                    }
                }
                check(traceStep != null) {
                    "trace benchmark frame omitted its playback identity"
                }
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
            frameWallNs[index] = frameIterationNs
            elapsedNs[index] = nowNs - measurementStartNs
            cameraRevision[index] = sortStats[0]
            check(submission.cameraRevision == sortStats[0]) {
                "order submission revision does not match frame sort telemetry"
            }
            check(submission.actualBackend == ((sortStats[6] shr 9) and 3L).toInt()) {
                "order submission backend does not match frame sort telemetry"
            }
            appliedOrderRevision[index] = sortStats[1]
            scheduledRevision[index] = sortStats[2]
            completedRevision[index] = sortStats[3]
            presentedOrderLag[index] = sortStats[4]
            observedResultLag[index] = sortStats[5]
            sortFlags[index] = sortStats[6]
            orderSubmissionTicket[index] = submission.ticket ?: 0L
            orderSubmissionFlags[index] = submission.flags.toLong()
            if (config.gpuProducer != null) {
                val producer = checkNotNull(producerSubmission) {
                    "GPU producer diagnostic frame omitted its submission receipt"
                }
                check(producer.cameraRevision == sortStats[0]) {
                    "GPU producer submission revision does not match frame telemetry"
                }
                gpuProducerSubmissionTicket[index] = checkNotNull(producer.ticket)
                gpuProducerSubmissionFlags[index] = producer.flags.toLong()
            } else {
                check(producerSubmission == null) {
                    "non-diagnostic frame received a GPU producer submission"
                }
            }
            cameraReceipts[index] = cameraReceipt
            val currentRecord = checkNotNull(currentStats.recordForSample(index)) {
                "measured frame $index lacks a current-stats pre-ticket record"
            }
            check(
                currentRecord.requestStatus == GsplatSurfaceCurrentStatsRequestStatus.REQUESTED &&
                    currentRecord.submissionIssued && currentRecord.ticket != null &&
                    currentRecord.identity != null
            ) {
                "measured frame $index lacks a requested and Issued current-stats sample"
            }
            check(currentRecord.identity.cameraRevision == cameraReceipt.cameraRevision) {
                "measured frame $index current-stats camera identity drifted from presentation"
            }
            if (traceStep != null) {
                check(traceStep.phase == "measure" && traceStep.measuredSampleIndex == index)
                traceFrameIndex[index] = traceStep.traceFrameIndex
                traceTimestampNs[index] = traceStep.timestampNs
                traceLoopIndex[index] = traceStep.loopIndex
            }
            samples += 1
            totalFrameWallNs += frameIterationNs
            totalCallNs += renderCallNs
            complete = samples >= measuredSampleCount
            if (complete) {
                measurementEndedAtMs = System.currentTimeMillis()
            }
        }

        fun resultLine(
            datasetLabel: String,
            currentStats: SurfaceCurrentStatsConsumer
        ): String {
            requireOrderMeasurements()
            requireGpuProducerMeasurements()
            currentStats.strictRecords(measuredSampleCount)
            val safeSamples = samples.coerceAtLeast(1)
            var cpuTimingSamples = 0
            var cpuPreprocessMs = 0.0
            var cpuSortMs = 0.0
            var cpuQueueCompleteMs = 0.0
            var cpuQueueCompleteSamples = 0
            var gpuQueueCompleteMs = 0.0
            var gpuQueueCompleteSamples = 0
            for (index in 0 until samples) {
                cpuOrderMeasurementForFrame(index)?.let { measurement ->
                    cpuTimingSamples += 1
                    cpuPreprocessMs += measurement.preprocessMs
                    cpuSortMs += measurement.sortMs
                    cpuQueueCompleteMs += measurement.frameCompleteMs
                    cpuQueueCompleteSamples += 1
                }
                orderMeasurementForFrame(index)?.let { measurement ->
                    gpuQueueCompleteMs += measurement.gpuCompleteMs
                    gpuQueueCompleteSamples += 1
                }
            }
            val averageCpuPreprocess = if (cpuTimingSamples == 0) {
                "n/a"
            } else {
                "%.3f".format(Locale.US, cpuPreprocessMs / cpuTimingSamples)
            }
            val averageCpuSort = if (cpuTimingSamples == 0) {
                "n/a"
            } else {
                "%.3f".format(Locale.US, cpuSortMs / cpuTimingSamples)
            }
            val averageCpuQueueComplete = if (cpuQueueCompleteSamples == 0) {
                "n/a"
            } else {
                "%.3f".format(Locale.US, cpuQueueCompleteMs / cpuQueueCompleteSamples)
            }
            val averageGpuQueueComplete = if (gpuQueueCompleteSamples == 0) {
                "n/a"
            } else {
                "%.3f".format(Locale.US, gpuQueueCompleteMs / gpuQueueCompleteSamples)
            }
            val producerCompletion = (0 until samples)
                .mapNotNull(::gpuProducerMeasurement)
                .map { it.frameCompleteMs.toDouble() }
            val averageGpuProducerComplete = if (producerCompletion.isEmpty()) {
                "n/a"
            } else {
                "%.3f".format(Locale.US, producerCompletion.average())
            }
            var resolvedVisibleTotal = 0L
            var resolvedContributorTotal = 0L
            var resolvedContributorSamples = 0
            var resolvedDrawnTotal = 0L
            for (index in 0 until samples) {
                resolvedVisibleTotal += resolvedVisible(index, currentStats)
                resolvedContributorTotal += resolvedContributor(index, currentStats)
                resolvedContributorSamples += 1
                resolvedDrawnTotal += resolvedDrawn(index, currentStats)
            }
            val averageCount = "avg_visible=${resolvedVisibleTotal / safeSamples}"
            return "BENCHMARK_RESULT dataset=$datasetLabel " +
                "samples=$samples warmup=${config.warmupFrames} sort_interval=${config.sortInterval} " +
                "loops=${config.cameraTraceLoops} " +
                "async_sort=${config.asyncSort} " +
                "order_backend=${config.orderBackend} " +
                "geometry_pipeline=${geometryPipelineName(config.geometryPath)} " +
                "frame_latency=${config.frameLatency} " +
                "source_splats=${exactness.source} " +
                "resident_splats=${exactness.resident} " +
                "avg_call_ms=${avgNs(totalCallNs, safeSamples)} " +
                "avg_frame_ms=${avgNs(totalFrameWallNs, safeSamples)} " +
                "frame_wall_source=host_iteration " +
                "cpu_timing_samples=$cpuTimingSamples " +
                "avg_cpu_preprocess_ms=$averageCpuPreprocess " +
                "avg_cpu_sort_ms=$averageCpuSort " +
                "avg_cpu_raster_ms=n/a " +
                "avg_cpu_queue_complete_ms=$averageCpuQueueComplete " +
                "avg_gpu_queue_complete_ms=$averageGpuQueueComplete " +
                "gpu_order_producer=${config.gpuProducer ?: "disabled"} " +
                "avg_gpu_producer_frame_complete_ms=$averageGpuProducerComplete " +
                "$averageCount " +
                (if (resolvedContributorSamples == samples) {
                    "avg_contributor=${resolvedContributorTotal / safeSamples} "
                } else {
                    "avg_contributor=n/a "
                }) +
                "avg_drawn=${resolvedDrawnTotal / safeSamples}"
        }

        fun artifactLines(
            datasetLabel: String,
            datasetPath: String,
            presentation: BenchmarkPresentationReceipt,
            physicalDisplayWidth: Int?,
            physicalDisplayHeight: Int?,
            density: Float,
            refreshHz: Double?,
            thermalStatusEnd: Int?,
            currentStats: SurfaceCurrentStatsConsumer
        ): List<Pair<String, String>> {
            check(complete) { "benchmark artifacts require a complete measurement" }
            requireOrderMeasurements()
            requireGpuProducerMeasurements()
            val currentStatsRecords = currentStats.strictRecords(measuredSampleCount)
            presentation.requireFormal(
                presentation.requestedWidth,
                presentation.requestedHeight
            )
            val validRefreshHz = refreshHz?.takeIf { it.isFinite() && it > 0.0 }
            val frameBudgetMs = 1000.0 / (validRefreshHz ?: 60.0)
            val datasetMetadata = datasetMetadata(File(datasetPath))
            check(datasetMetadata.splatCount == exactness.source) {
                "native exactness source count does not match the selected dataset"
            }
            check(datasetMetadata.shDegree == exactness.sourceShDegree) {
                "native exactness source SH degree does not match the selected dataset"
            }
            val cameraTrace = config.cameraTraceMetadata
            val exactnessReceiptId = exactnessReceiptId()
            val traceId = cameraTrace?.id ?: "orbit-yaw-${config.yawStepRadians}"
            val traceSha256 = cameraTrace?.sha256
                ?: sha256(traceId.toByteArray(Charsets.UTF_8))
            val hasGpuFrames = (0 until samples).any { index ->
                ((sortFlags[index] shr 9) and 3L) == 1L
            }
            val unavailable = linkedSetOf(
                "environment.browser",
                "environment.adapter",
                "environment.driver",
                "frames[*].geometry_submit_ms",
                "frames[*].gpu_wait_ms",
                "summary.distributions.geometry_submit_ms",
                "summary.distributions.gpu_wait_ms",
                "frames[*].raster_ms",
                "summary.distributions.raster_ms"
            )
            if ((0 until samples).any { cpuOrderMeasurementForFrame(it) == null }) {
                unavailable += "frames[*].preprocess_ms"
                unavailable += "frames[*].sort_ms"
            }
            if ((0 until samples).any { orderMeasurementForFrame(it) == null }) {
                unavailable += "frames[*].gpu_complete_ms"
            }
            if (!hasGpuFrames) {
                unavailable += "summary.distributions.gpu_complete_ms"
            }
            if ((0 until samples).any { cpuOrderMeasurementForFrame(it) == null }) {
                unavailable += "frames[*].cpu_frame_complete_ms"
            }
            if (cpuOrderMeasurementsByTicket.isEmpty()) {
                unavailable += "summary.distributions.cpu_frame_complete_ms"
            }
            if (thermalStatusStart == null) unavailable += "environment.thermal_status_start"
            if (thermalStatusEnd == null) unavailable += "environment.thermal_status_end"
            for (index in 0 until samples) {
                requireCurrentStatsRecord(
                    index,
                    currentStatsRecords[index],
                    checkNotNull(cameraReceipts[index])
                )
                requireContributorCountContract(
                    resolvedVisible(index, currentStats),
                    resolvedContributor(index, currentStats),
                    resolvedDrawn(index, currentStats),
                    resolvedExactContributorCompaction(index, currentStats),
                    "artifact frame $index"
                )
            }

            val manifest = JSONObject()
                .put("schema", "gsplat-benchmark/v1")
                .put("record_type", "manifest")
                .put("run_id", runId)
                .put("identity", JSONObject()
                    .put("series_id", when {
                        config.cameraTraceSequence -> "android-native-camera-trace-sequence"
                        cameraTrace != null -> "android-native-fixed-camera"
                        else -> "android-native-orbit"
                    })
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
                .put("exactness", JSONObject()
                    .put("receipt_id", exactnessReceiptId)
                    .put("source_splat_count", exactness.source)
                    .put("decoded_splat_count", exactness.decoded)
                    .put("encoded_splat_count", exactness.encoded)
                    .put("resident_splat_count", exactness.resident)
                    .put("addressable_splat_count", exactness.addressable)
                    .put("source_sh_degree", exactness.sourceShDegree)
                    .put("resident_sh_degree", exactness.residentShDegree)
                    .put("source_membership", if (exactness.qualityFlags and 1 != 0) "all" else "partial")
                    .put("sampling", if (exactness.qualityFlags and (1 shl 1) != 0) "disabled" else "enabled")
                    .put("lod", if (exactness.qualityFlags and (1 shl 2) != 0) "disabled" else "enabled")
                    .put("sh_degree_policy", if (exactness.qualityFlags and (1 shl 3) != 0) "source" else "reduced")
                    .put("partial_scene_published", exactness.qualityFlags and (1 shl 4) == 0)
                    .put("full_quality", exactness.fullQuality))
                .put("trace", JSONObject()
                    .put("schema", cameraTrace?.let { "gsplat-camera-trace/v1" } ?: JSONObject.NULL)
                    .put("id", traceId)
                    .put("sha256", traceSha256)
                    .put("file_sha256", cameraTrace?.fileSha256 ?: JSONObject.NULL)
                    .put("reference_width", cameraTrace?.width ?: JSONObject.NULL)
                    .put("reference_height", cameraTrace?.height ?: JSONObject.NULL)
                    .put("playback_mode", when {
                        config.cameraTraceSequence -> "sequence"
                        cameraTrace != null -> "fixed"
                        else -> "endpoint_orbit"
                    })
                    .put(
                        "measured_frames_per_loop",
                        if (cameraTrace != null) config.frames else JSONObject.NULL
                    )
                    .put(
                        "loops",
                        if (cameraTrace != null) config.cameraTraceLoops else JSONObject.NULL
                    )
                    .put("require_display_match", cameraTrace != null && config.requireTraceDisplayMatch)
                    .put("display_policy", when {
                        cameraTrace == null -> "endpoint_orbit"
                        config.requireTraceDisplayMatch -> "trace_display_exact"
                        else -> "native_aspect_reprojection"
                    })
                    .put("quality_comparable", cameraTrace != null && config.requireTraceDisplayMatch)
                    .put("coordinate_system", if (cameraTrace == null) {
                        JSONObject.NULL
                    } else {
                        JSONObject()
                            .put("handedness", "right")
                            .put("axes", "RUF")
                            .put("camera_forward", "+Z")
                    })
                    .put("matrix_convention", if (cameraTrace == null) {
                        JSONObject.NULL
                    } else {
                        JSONObject()
                            .put("storage_order", "row-major")
                            .put("vector_convention", "column")
                            .put("composition", "projection * view * world_position")
                            .put("ndc_xy", "[-1,1]")
                            .put("ndc_z", "[0,1]")
                            .put("clip_w", "camera_z")
                    })
                    .put("runtime_camera_receipt", if (cameraTrace == null) {
                        JSONObject.NULL
                    } else {
                        JSONObject()
                            .put("schema", "gsplat-surface-camera-receipt/v1")
                            .put("source", "native_runtime_after_present")
                            .put("scalar_storage", "float32")
                            .put("absolute_tolerance", CAMERA_RECEIPT_TOLERANCE)
                            .put("relative_tolerance", CAMERA_RECEIPT_TOLERANCE)
                    })
                    .also { traceJson ->
                        when {
                            config.cameraTraceSequence -> traceJson.put(
                                "frame_indices",
                                JSONArray(config.cameraTraceFrameIndices)
                            )
                            cameraTrace != null -> traceJson.put("frame_index", config.cameraTraceFrame)
                            else -> traceJson.put("frame_index", JSONObject.NULL)
                        }
                    })
                .put("renderer", JSONObject()
                    .put("implementation", "gsplat-rs-native")
                    .put("path", geometryPipelineName(config.geometryPath))
                    .put("backend", "vulkan")
                    .put("max_storage_buffers_per_shader_stage", exactness.maxStorageBuffersPerShaderStage)
                    .put("max_storage_buffer_binding_size", exactness.maxStorageBufferBindingSize)
                    .put("order_backend_requested", config.orderBackend)
                    .put("sort_interval", config.sortInterval)
                    .put("sort_policy", if (config.asyncSort) {
                        "async_latest:${config.sortInterval}"
                    } else {
                        "interval:${config.sortInterval}"
                    })
                    .also { renderer ->
                        renderer
                            .put("count_semantics", COUNT_SEMANTICS)
                            .put("count_source", "matching_current_stats_ready")
                            .put("current_stats_schema", CURRENT_STATS_SCHEMA)
                            .put("current_stats_strict", true)
                        if (config.gpuProducer != null) {
                            renderer
                                .put("raster_plan", "projected_quads_exact")
                                .put("projected_policy_requested", "compact")
                                .put("gpu_order_producer_requested", config.gpuProducer)
                                .put("gpu_producer_measurement_enabled", true)
                        } else {
                            renderer
                                .put("gpu_order_producer_requested", JSONObject.NULL)
                                .put("gpu_producer_measurement_enabled", false)
                        }
                    })
                .put("resolution", JSONObject()
                    .put("requested_width", presentation.requestedWidth)
                    .put("requested_height", presentation.requestedHeight)
                    .put("surface_width", presentation.surfaceWidth)
                    .put("surface_height", presentation.surfaceHeight)
                    .put("internal_render_width", presentation.internalRenderWidth)
                    .put("internal_render_height", presentation.internalRenderHeight)
                    .put("presented_width", presentation.presentedWidth)
                    .put("presented_height", presentation.presentedHeight)
                    .put("presented_camera_revision", presentation.presentedCameraRevision)
                    .put("dynamic_resolution", "disabled")
                    .put("upscaling", "disabled")
                    .put("full_resolution", presentation.fullResolution)
                    .put("native_flags", presentation.flags))
                .put("display", JSONObject()
                    .put("width", presentation.presentedWidth)
                    .put("height", presentation.presentedHeight)
                    .put("dpr", density.toDouble())
                    .put("refresh_hz", validRefreshHz ?: 60.0)
                    .put("refresh_hz_source", if (validRefreshHz == null) "configured" else "observed")
                    .put("frame_budget_ms", frameBudgetMs)
                    .put("frame_budget_source", if (validRefreshHz == null) "configured" else "observed"))
                .put("timing_contract", JSONObject()
                    .put("call_ms", "host_camera_request_render_transaction_wall")
                    .put(
                        "frame_wall_ms",
                        "host_iteration_request_through_receipt_queries"
                    )
                    .put("preprocess_ms", "matching_cpu_order_terminal_only")
                    .put("sort_ms", "matching_cpu_order_terminal_only")
                    .put("raster_ms", JSONObject.NULL))
                .put("environment", JSONObject()
                    .put("platform", "android-native")
                    .put("os", "Android ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT})")
                    .put("device", "${Build.MANUFACTURER} ${Build.MODEL} (${Build.DEVICE})")
                    .put("browser", JSONObject.NULL)
                    .put("adapter", JSONObject.NULL)
                    .put("driver", JSONObject.NULL)
                    .put("hardware", Build.HARDWARE)
                    .put("orientation", "landscape")
                    .put("physical_display_width", physicalDisplayWidth ?: JSONObject.NULL)
                    .put("physical_display_height", physicalDisplayHeight ?: JSONObject.NULL)
                    .put("thermal_status_start", thermalStatusStart ?: JSONObject.NULL)
                    .put("thermal_status_end", thermalStatusEnd ?: JSONObject.NULL))
                .put("unavailable_fields", JSONArray(unavailable))

            val lines = ArrayList<Pair<String, String>>(samples + 2)
            lines += BENCHMARK_MANIFEST_PREFIX to manifest.toString()
            for (index in 0 until samples) {
                val flags = sortFlags[index]
                val gpuBackend = ((flags shr 9) and 3L) == 1L
                val orderMeasurement = orderMeasurementForFrame(index)
                val cpuMeasurement = cpuOrderMeasurementForFrame(index)
                val terminalTicket = orderMeasurement?.ticket ?: cpuMeasurement?.ticket
                val terminalRevision = orderMeasurement?.cameraRevision
                    ?: cpuMeasurement?.cameraRevision
                val contributor = resolvedContributor(index, currentStats)
                val exactContributorCompaction =
                    resolvedExactContributorCompaction(index, currentStats)
                val producerMeasurement = gpuProducerMeasurement(index)
                val currentRecord = currentStatsRecords[index]
                val currentIdentity = checkNotNull(currentRecord.identity)
                val frame = JSONObject()
                    .put("schema", "gsplat-benchmark/v1")
                    .put("record_type", "frame")
                    .put("run_id", runId)
                    .put("frame_index", index)
                    .put("elapsed_ns", elapsedNs[index])
                    .put("call_ms", callNs[index].toDouble() / 1_000_000.0)
                    .put("frame_wall_ms", frameWallNs[index].toDouble() / 1_000_000.0)
                    .put("preprocess_ms", cpuMeasurement?.preprocessMs?.toDouble() ?: JSONObject.NULL)
                    .put("sort_ms", cpuMeasurement?.sortMs?.toDouble() ?: JSONObject.NULL)
                    .put("geometry_submit_ms", JSONObject.NULL)
                    .put("gpu_wait_ms", JSONObject.NULL)
                    .put("gpu_complete_ms", orderMeasurement?.gpuCompleteMs?.toDouble() ?: JSONObject.NULL)
                    .put("cpu_frame_complete_ms", cpuMeasurement?.frameCompleteMs?.toDouble() ?: JSONObject.NULL)
                    .put("visible", resolvedVisible(index, currentStats))
                    .put("drawn", resolvedDrawn(index, currentStats))
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
                    .put("adaptive_gpu_failure", adaptiveGpuFailureName(flags) ?: JSONObject.NULL)
                    .put("trace_frame_index", if (traceFrameIndex[index] >= 0) traceFrameIndex[index] else JSONObject.NULL)
                    .put("trace_timestamp_ns", if (traceTimestampNs[index] >= 0L) traceTimestampNs[index] else JSONObject.NULL)
                    .put("trace_loop_index", if (traceLoopIndex[index] >= 0) traceLoopIndex[index] else JSONObject.NULL)
                    .put(
                        "camera_receipt",
                        cameraReceiptJson(checkNotNull(cameraReceipts[index]))
                    )
                    .put("order_backend_requested", config.orderBackend)
                    .put("order_measurement_ticket", terminalTicket ?: JSONObject.NULL)
                    .put("order_measurement_ticket_issued", orderSubmissionFlags[index] and (1L shl 1) != 0L)
                    .put("order_submission_ticket", orderSubmissionTicket[index].takeIf { it > 0L } ?: JSONObject.NULL)
                    .put("order_submission_flags", orderSubmissionFlags[index])
                    .put("order_measurement_camera_revision", terminalRevision ?: JSONObject.NULL)
                    .put("order_timing_source", orderMeasurement?.timingSource ?: JSONObject.NULL)
                    .put("gpu_preprocess_ms", orderMeasurement?.gpuPreprocessMs?.toDouble() ?: JSONObject.NULL)
                    .put("gpu_radix_ms", orderMeasurement?.gpuRadixMs?.toDouble() ?: JSONObject.NULL)
                    .put("gpu_order_ms", orderMeasurement?.gpuOrderMs?.toDouble() ?: JSONObject.NULL)
                    .put("gpu_timestamp_period_ns", orderMeasurement?.timestampPeriodNs?.toDouble() ?: JSONObject.NULL)
                    .put("order_measurement_flags", orderMeasurement?.flags ?: JSONObject.NULL)
                    .put("cpu_order_measurement_flags", cpuMeasurement?.flags ?: JSONObject.NULL)
                    .put("current_stats_ticket", checkNotNull(currentRecord.ticket))
                    .put(
                        "current_stats_presentation_sequence",
                        currentIdentity.presentationSequence
                    )
                    .put(
                        "current_stats_executed_plan",
                        currentStatsPlanName(currentIdentity.executedPlan)
                    )
                    .also { frameJson ->
                        frameJson
                            .put("contributor", contributor)
                            .put(
                                "exact_contributor_compaction",
                                exactContributorCompaction
                            )
                        if (config.gpuProducer != null) {
                            val producer = checkNotNull(producerMeasurement)
                            frameJson
                                .put("gpu_order_producer", gpuProducerName(producer.producer))
                                .put("gpu_producer_measurement_ticket", producer.ticket)
                                .put(
                                    "gpu_producer_measurement_camera_revision",
                                    producer.cameraRevision
                                )
                                .put("gpu_producer_order_generation", producer.orderGeneration)
                                .put(
                                    "gpu_producer_projection_generation",
                                    producer.projectionGeneration
                                )
                                .put(
                                    "gpu_producer_frame_complete_ms",
                                    producer.frameCompleteMs.toDouble()
                                )
                                .put("gpu_producer_source", producer.source)
                                .put("gpu_producer_contributor", producer.contributor)
                                .put("gpu_producer_drawn", producer.drawn)
                                .put("gpu_producer_draw_scope", "exact_current_contributors")
                                .put("gpu_producer_order_refreshed", producer.orderRefreshed)
                                .put("gpu_producer_exact_current_draw", producer.exactCurrentDraw)
                                .put("gpu_producer_stale_order", producer.staleOrder)
                                .put("gpu_producer_dropped_prior", producer.droppedPrior)
                                .put(
                                    "gpu_producer_submission_flags",
                                    gpuProducerSubmissionFlags[index]
                                )
                        }
                    }
                lines += BENCHMARK_FRAME_PREFIX to frame.toString()
            }

            var missedFrames = 0
            for (index in 0 until samples) {
                if (frameWallNs[index].toDouble() / 1_000_000.0 > frameBudgetMs) {
                    missedFrames += 1
                }
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
                    .put("frame_wall_ms", distributionJson(frameWallNs, samples, 1_000_000.0))
                    .put(
                        "preprocess_ms",
                        cpuMeasurementDistribution { it.preprocessMs.toDouble() }
                    )
                    .put(
                        "sort_ms",
                        cpuMeasurementDistribution { it.sortMs.toDouble() }
                    )
                    .put("geometry_submit_ms", JSONObject.NULL)
                    .put("gpu_wait_ms", JSONObject.NULL)
                    .put("gpu_complete_ms", gpuMeasurementDistribution { it.gpuCompleteMs.toDouble() })
                    .put("cpu_frame_complete_ms", cpuMeasurementDistribution())
                    .put("gpu_preprocess_ms", gpuMeasurementDistribution { it.gpuPreprocessMs?.toDouble() })
                    .put("gpu_radix_ms", gpuMeasurementDistribution { it.gpuRadixMs?.toDouble() })
                    .put("gpu_order_ms", gpuMeasurementDistribution { it.gpuOrderMs?.toDouble() }))
                .put("sort_telemetry", sortTelemetrySummary())
                .put("order_terminal_ledger", orderTerminalLedger(exactnessReceiptId))
                .put(
                    "current_stats_terminal_ledger",
                    currentStatsTerminalLedger(currentStatsRecords, exactnessReceiptId)
                )
                .put(
                    "gpu_producer_telemetry",
                    if (config.gpuProducer == null) {
                        JSONObject.NULL
                    } else {
                        gpuProducerTelemetrySummary()
                    }
                )
                .put(
                    "gpu_producer_terminal_ledger",
                    if (config.gpuProducer == null) {
                        JSONObject.NULL
                    } else {
                        gpuProducerTerminalLedger(exactnessReceiptId)
                    }
                )
            lines += BENCHMARK_SUMMARY_PREFIX to summary.toString()
            return lines
        }

        private fun requireCurrentStatsRecord(
            index: Int,
            record: SurfaceCurrentStatsSampleRecord,
            cameraReceipt: BenchmarkCameraReceipt
        ) {
            check(record.binding.sampleIndex == index) {
                "current-stats record is bound to the wrong measured frame"
            }
            check(record.requestStatus == GsplatSurfaceCurrentStatsRequestStatus.REQUESTED) {
                "frame $index current-stats pre-ticket was ${record.requestStatus.wireName}"
            }
            check(record.submissionIssued) {
                "frame $index current-stats request did not produce Issued"
            }
            val ticket = checkNotNull(record.ticket)
            val identity = checkNotNull(record.identity)
            val ready = (record.terminal as? SurfaceCurrentStatsTerminal.Ready)?.receipt
                ?: error("frame $index current-stats ticket $ticket did not become Ready")
            check(ready.ticket == ticket && ready.identity == identity) {
                "frame $index current-stats Ready identity drifted from submission"
            }
            check(identity.cameraRevision == cameraReceipt.cameraRevision) {
                "frame $index current-stats camera revision is stale"
            }
            check(identity.presentationSequence > 0L) {
                "frame $index current-stats presentation sequence is uninitialized"
            }
            check(ready.sourceCount == exactness.source) {
                "frame $index current-stats S does not match exactness"
            }
            val actualBackend = ((sortFlags[index] shr 9) and 3L).toInt()
            when (identity.executedPlan) {
                GsplatSurfaceCurrentStatsPlan.CPU_POST_SORT -> {
                    check(actualBackend == GSPLAT_ORDER_BACKEND_CPU) {
                        "frame $index executed CPU PostSort but reported a non-CPU lane"
                    }
                    check(ready.drawnCount == ready.visibleCount) {
                        "frame $index CPU PostSort requires D=V"
                    }
                }
                GsplatSurfaceCurrentStatsPlan.GPU_POST_SORT -> {
                    check(actualBackend == GSPLAT_ORDER_BACKEND_GPU) {
                        "frame $index executed GPU PostSort but reported a non-GPU lane"
                    }
                    check(ready.drawnCount == ready.visibleCount) {
                        "frame $index GPU PostSort requires D=V"
                    }
                }
                GsplatSurfaceCurrentStatsPlan.GPU_PREPROJECT -> {
                    check(actualBackend == GSPLAT_ORDER_BACKEND_GPU) {
                        "frame $index executed GPU Preproject but reported a non-GPU lane"
                    }
                    check(ready.drawnCount == ready.contributorCount) {
                        "frame $index GPU Preproject requires D=C"
                    }
                }
            }
            orderMeasurementForFrame(index)?.let { order ->
                check(
                    order.visible == ready.visibleCount &&
                        order.contributor == ready.contributorCount &&
                        order.drawn == ready.drawnCount
                ) { "frame $index GPU order/current-stats V/C/D disagree" }
            }
            cpuOrderMeasurementForFrame(index)?.let { order ->
                check(
                    order.visible == ready.visibleCount &&
                        order.contributor == ready.contributorCount &&
                        order.drawn == ready.drawnCount
                ) { "frame $index CPU order/current-stats V/C/D disagree" }
            }
            gpuProducerMeasurement(index)?.let { producer ->
                val (producerPlan, producerCountSemantics) = when (producer.producer) {
                    GSPLAT_GPU_PRODUCER_POST_SORT ->
                        GsplatSurfaceCurrentStatsPlan.GPU_POST_SORT to
                            GsplatSurfaceCurrentStatsCountSemantics
                                .INDIRECT_DRAW_EQUALS_VISIBLE
                    GSPLAT_GPU_PRODUCER_PREPROJECT ->
                        GsplatSurfaceCurrentStatsPlan.GPU_PREPROJECT to
                            GsplatSurfaceCurrentStatsCountSemantics
                                .INDIRECT_DRAW_EQUALS_CONTRIBUTOR
                    else -> error(
                        "frame $index producer receipt has unknown producer " +
                            producer.producer
                    )
                }
                check(
                    identity.executedPlan == producerPlan &&
                        ready.countSemantics == producerCountSemantics &&
                        producer.cameraRevision == identity.cameraRevision &&
                        producer.orderGeneration == identity.orderGeneration &&
                        producer.source == ready.sourceCount &&
                        producer.contributor == ready.contributorCount &&
                        producer.drawn == ready.drawnCount
                ) {
                    "frame $index producer/current-stats identity, plan, semantics, or S/C/D disagree"
                }
            }
        }

        private fun currentStatsTerminalLedger(
            records: List<SurfaceCurrentStatsSampleRecord>,
            exactnessReceiptId: String
        ): JSONArray = JSONArray().also { ledger ->
            records.forEachIndexed { index, record ->
                val identity = checkNotNull(record.identity)
                val ready = checkNotNull(
                    (record.terminal as? SurfaceCurrentStatsTerminal.Ready)?.receipt
                )
                ledger.put(
                    JSONObject()
                        .put("sample_index", index)
                        .put("trace_frame_index", record.binding.traceFrameIndex ?: JSONObject.NULL)
                        .put(
                            "trace_timestamp_ns",
                            record.binding.traceTimestampNs ?: JSONObject.NULL
                        )
                        .put("request_status", record.requestStatus.wireName)
                        .put("submission_status", "issued")
                        .put("ticket", checkNotNull(record.ticket))
                        .put("identity", currentStatsIdentityJson(identity))
                        .put("outcome", "ready")
                        .put("source", ready.sourceCount)
                        .put("visible", ready.visibleCount)
                        .put("contributor", ready.contributorCount)
                        .put("drawn", ready.drawnCount)
                        .put(
                            "count_semantics",
                            currentStatsCountSemanticsName(ready.countSemantics)
                        )
                        .put("exactness_receipt_id", exactnessReceiptId)
                )
            }
        }

        private fun currentStatsIdentityJson(
            identity: GsplatSurfaceCurrentStatsIdentity
        ): JSONObject = JSONObject()
            .put("scene_generation", identity.sceneGeneration)
            .put("camera_revision", identity.cameraRevision)
            .put("viewport_generation", identity.viewportGeneration)
            .put("contract_generation", identity.contractGeneration)
            .put("plan_set_generation", identity.planSetGeneration)
            .put("order_generation", identity.orderGeneration)
            .put("raster_generation", identity.rasterGeneration)
            .put("encode_attempt", identity.encodeAttempt)
            .put("presentation_sequence", identity.presentationSequence)
            .put("executed_plan", currentStatsPlanName(identity.executedPlan))

        private fun cameraReceiptJson(receipt: BenchmarkCameraReceipt): JSONObject =
            JSONObject()
                .put("schema", "gsplat-surface-camera-receipt/v1")
                .put("source", "native_runtime_after_present")
                .put("camera_revision", receipt.cameraRevision)
                .put("presented_camera_revision", receipt.presentedCameraRevision)
                .put("surface_width", receipt.surfaceWidth)
                .put("surface_height", receipt.surfaceHeight)
                .put("flags", receipt.flags)
                .put("pose", JSONObject()
                    .put("position", JSONArray(receipt.position.map { it.toDouble() }))
                    .put("rotation_xyzw", JSONArray(receipt.rotationXyzw.map { it.toDouble() })))
                .put("intrinsics", JSONObject()
                    .put("vertical_fov_radians", receipt.verticalFovRadians.toDouble())
                    .put("near_plane", receipt.nearPlane.toDouble())
                    .put("far_plane", receipt.farPlane.toDouble()))
                .put("view_matrix", JSONArray(receipt.viewMatrix.map { it.toDouble() }))
                .put(
                    "projection_matrix",
                    JSONArray(receipt.projectionMatrix.map { it.toDouble() })
                )
                .put(
                    "view_projection_matrix",
                    JSONArray(receipt.viewProjectionMatrix.map { it.toDouble() })
                )

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
            var adaptiveGpuFailureFrames = 0
            var previousBackend = -1L
            var maxPresentedLag = 0L
            var gpuReceipts = 0
            var gpuTimestampReceipts = 0
            var gpuCompletionReceipts = 0
            var gpuDroppedReceipts = 0
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
                if (adaptiveGpuFailureName(flags) != null) adaptiveGpuFailureFrames += 1
                maxPresentedLag = maxOf(maxPresentedLag, presentedOrderLag[index])
            }
            for ((ticket, issued) in issuedOrderTickets) {
                if (issued.backend != GSPLAT_ORDER_BACKEND_GPU) continue
                val measurement = orderMeasurementsByTicket[ticket] ?: continue
                gpuReceipts += 1
                if (measurement.timingSource == "timestamp_query") {
                    gpuTimestampReceipts += 1
                } else if (measurement.timingSource == "completion") {
                    gpuCompletionReceipts += 1
                }
                if (measurement.flags and (1 shl 5) != 0) gpuDroppedReceipts += 1
            }
            val issuedCpuCount = issuedOrderTickets.values.count {
                it.backend == GSPLAT_ORDER_BACKEND_CPU
            }
            val issuedGpuCount = issuedOrderTickets.size - issuedCpuCount
            return JSONObject()
                .put("scheduled_count", scheduled)
                .put("completed_count", completed)
                .put("applied_count", applied)
                .put("dropped_count", dropped)
                .put("sync_fallback_count", fallbacks)
                .put("cpu_frame_count", cpuFrames)
                .put("gpu_frame_count", gpuFrames)
                .put("gpu_sort_fallback_count", gpuFallbacks)
                .put("order_measurement_scheduled_count", issuedOrderTickets.size)
                .put("cpu_order_measurement_scheduled_count", issuedCpuCount)
                .put("cpu_order_measurement_completed_count", cpuOrderMeasurementsByTicket.size)
                .put("gpu_order_measurement_scheduled_count", issuedGpuCount)
                .put("gpu_order_measurement_completed_count", gpuReceipts)
                .put("gpu_order_measurement_timestamp_count", gpuTimestampReceipts)
                .put("gpu_order_measurement_completion_count", gpuCompletionReceipts)
                .put("gpu_order_measurement_dropped_count", gpuDroppedReceipts)
                .put("order_measurement_terminal_failure_count", orderFailuresByTicket.size)
                .put("order_measurement_unsampled_count", unsampledOrderRequests.size)
                .put("backend_switch_count", backendSwitches)
                .put("adaptive_gpu_failure_frame_count", adaptiveGpuFailureFrames)
                .put("adaptive_gpu_failure_final", if (samples > 0) {
                    adaptiveGpuFailureName(sortFlags[samples - 1]) ?: JSONObject.NULL
                } else {
                    JSONObject.NULL
                })
                .put("adaptive_final_state", if (samples > 0) {
                    adaptiveStateName(sortFlags[samples - 1])
                } else {
                    "disabled"
                })
                .put("max_presented_revision_lag", maxPresentedLag)
                .put("stale_applied_count", 0)
        }

        private fun orderTerminalLedger(exactnessReceiptId: String): JSONArray {
            val ledger = JSONArray()
            issuedOrderTickets.keys.sorted().forEach { ticket ->
                val issued = checkNotNull(issuedOrderTickets[ticket])
                val record = JSONObject()
                    .put("ticket", ticket)
                    .put("camera_revision", issued.cameraRevision)
                    .put("backend", if (issued.backend == GSPLAT_ORDER_BACKEND_GPU) "gpu" else "cpu")
                    .put("exactness_receipt_id", exactnessReceiptId)
                when {
                    orderMeasurementsByTicket[ticket] != null -> {
                        val measurement = checkNotNull(orderMeasurementsByTicket[ticket])
                        record
                            .put("outcome", "success")
                            .put("frame_complete_ms", measurement.gpuCompleteMs.toDouble())
                            .put("visible", measurement.visible)
                            .put("contributor", measurement.contributor)
                            .put("drawn", measurement.drawn)
                            .put(
                                "exact_contributor_compaction",
                                measurement.exactContributorCompaction
                            )
                    }
                    cpuOrderMeasurementsByTicket[ticket] != null -> {
                        val measurement = checkNotNull(cpuOrderMeasurementsByTicket[ticket])
                        record
                            .put("outcome", "success")
                            .put("frame_complete_ms", measurement.frameCompleteMs.toDouble())
                            .put("visible", measurement.visible)
                            .put("contributor", measurement.contributor)
                            .put("drawn", measurement.drawn)
                            .put(
                                "exact_contributor_compaction",
                                measurement.exactContributorCompaction
                            )
                    }
                    orderFailuresByTicket[ticket] != null -> {
                        val failure = checkNotNull(orderFailuresByTicket[ticket])
                        record
                            .put("outcome", "failure")
                            .put("failure_reason", failure.reason)
                    }
                    else -> record.put("outcome", "pending")
                }
                ledger.put(record)
            }
            return ledger
        }

        private fun gpuProducerTelemetrySummary(): JSONObject {
            val measurements = (0 until samples).map { index ->
                checkNotNull(gpuProducerMeasurement(index))
            }
            return JSONObject()
                .put("requested_producer", checkNotNull(config.gpuProducer))
                .put("scheduled_count", measurements.size)
                .put("completed_count", measurements.size)
                .put("failure_count", gpuProducerFailuresByTicket.size)
                .put("unsampled_count", unsampledGpuProducerRequests.size)
                .put("exact_current_count", measurements.count { it.exactCurrentDraw })
                .put("stale_count", measurements.count { it.staleOrder })
                .put("dropped_count", measurements.count { it.droppedPrior })
                .put("order_refreshed_count", measurements.count { it.orderRefreshed })
                .put(
                    "frame_complete_ms",
                    doubleDistributionJson(measurements.map { it.frameCompleteMs.toDouble() })
                )
        }

        private fun gpuProducerTerminalLedger(exactnessReceiptId: String): JSONArray {
            val ledger = JSONArray()
            issuedGpuProducerTickets.keys.sorted().forEach { ticket ->
                val issued = checkNotNull(issuedGpuProducerTickets[ticket])
                val record = JSONObject()
                    .put("ticket", ticket)
                    .put("camera_revision", issued.cameraRevision)
                    .put("producer", gpuProducerName(issued.producer))
                    .put("exactness_receipt_id", exactnessReceiptId)
                val success = gpuProducerMeasurementsByTicket[ticket]
                val failure = gpuProducerFailuresByTicket[ticket]
                when {
                    success != null -> record
                        .put("outcome", "success")
                        .put("order_generation", success.orderGeneration)
                        .put("projection_generation", success.projectionGeneration)
                        .put("frame_complete_ms", success.frameCompleteMs.toDouble())
                        .put("source", success.source)
                        .put("contributor", success.contributor)
                        .put("drawn", success.drawn)
                        .put("draw_scope", "exact_current_contributors")
                    failure != null -> record
                        .put("outcome", "failure")
                        .put("failure_reason", failure.reason)
                    else -> record.put("outcome", "pending")
                }
                ledger.put(record)
            }
            return ledger
        }

        private fun exactnessReceiptId(): String = listOf(
            "native-exactness-v1",
            exactness.source,
            exactness.decoded,
            exactness.encoded,
            exactness.resident,
            exactness.addressable,
            exactness.sourceShDegree,
            exactness.residentShDegree,
            exactness.qualityFlags
        ).joinToString(":")

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

        private fun gpuMeasurementDistribution(
            select: (BenchmarkOrderMeasurement) -> Double?
        ): Any {
            val values = ArrayList<Double>()
            for (index in 0 until samples) {
                val measurement = orderMeasurementForFrame(index) ?: continue
                select(measurement)?.let(values::add)
            }
            if (values.isEmpty()) return JSONObject.NULL
            values.sort()
            val sum = values.sum()
            fun nearestRank(fraction: Double): Double {
                val index = maxOf(kotlin.math.ceil(fraction * values.size).toInt() - 1, 0)
                return values[index]
            }
            return JSONObject()
                .put("count", values.size)
                .put("mean", sum / values.size.toDouble())
                .put("p50", nearestRank(0.50))
                .put("p90", nearestRank(0.90))
                .put("p95", nearestRank(0.95))
                .put("p99", nearestRank(0.99))
                .put("max", values.last())
        }

        private fun doubleDistributionJson(input: List<Double>): JSONObject {
            check(input.isNotEmpty()) { "distribution requires at least one sample" }
            val values = input.sorted()
            fun nearestRank(fraction: Double): Double {
                val index = maxOf(kotlin.math.ceil(fraction * values.size).toInt() - 1, 0)
                return values[index]
            }
            return JSONObject()
                .put("count", values.size)
                .put("mean", values.average())
                .put("p50", nearestRank(0.50))
                .put("p90", nearestRank(0.90))
                .put("p95", nearestRank(0.95))
                .put("p99", nearestRank(0.99))
                .put("max", values.last())
        }

        private fun cpuMeasurementDistribution(
            select: (BenchmarkCpuOrderMeasurement) -> Double = {
                it.frameCompleteMs.toDouble()
            }
        ): Any {
            val values = ArrayList<Double>()
            for (index in 0 until samples) {
                cpuOrderMeasurementForFrame(index)?.let { measurement ->
                    values += select(measurement)
                }
            }
            values.sort()
            if (values.isEmpty()) return JSONObject.NULL
            fun nearestRank(fraction: Double): Double {
                val index = maxOf(kotlin.math.ceil(fraction * values.size).toInt() - 1, 0)
                return values[index]
            }
            return JSONObject()
                .put("count", values.size)
                .put("mean", values.sum() / values.size.toDouble())
                .put("p50", nearestRank(0.50))
                .put("p90", nearestRank(0.90))
                .put("p95", nearestRank(0.95))
                .put("p99", nearestRank(0.99))
                .put("max", values.last())
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
