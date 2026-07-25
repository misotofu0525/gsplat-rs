package com.gsplat.example

import com.gsplat.android.NativeBridge

/** Sample-only JNI controls that are deliberately absent from the Android AAR API. */
internal object BenchmarkBridge {
    init {
        System.loadLibrary("gsplat_jni")
    }

    /** Selects 0=CPU, 1=GPU, or 2=adaptive ordering for benchmark runs. */
    @JvmStatic
    external fun setSurfaceOrderBackend(nativeHandle: Long, backend: Int): Int

    /** Applies one validated gsplat-camera-trace/v1 frame to the Surface benchmark. */
    @JvmStatic
    external fun setSurfaceCameraTraceFrame(
        nativeHandle: Long,
        tracePath: String,
        frameIndex: Int,
        requireTraceDisplayMatch: Boolean
    ): Int

    /** Reads the camera state that the native Surface session actually presented. */
    @JvmStatic
    external fun getSurfaceCameraReceiptV1(
        nativeHandle: Long,
        outReceipt: LongArray
    ): Int
}

/** Sample-only camera evidence. Float fields are native f32 values, not trace JSON copies. */
internal data class BenchmarkCameraReceipt(
    val cameraRevision: Long,
    val presentedCameraRevision: Long,
    val surfaceWidth: Int,
    val surfaceHeight: Int,
    val flags: Int,
    val position: FloatArray,
    val rotationXyzw: FloatArray,
    val verticalFovRadians: Float,
    val nearPlane: Float,
    val farPlane: Float,
    val viewMatrix: FloatArray,
    val projectionMatrix: FloatArray,
    val viewProjectionMatrix: FloatArray
) {
    val framePresented: Boolean
        get() = flags and FLAG_FRAME_PRESENTED != 0

    val currentRevisionPresented: Boolean
        get() = flags and FLAG_CURRENT_REVISION_PRESENTED != 0

    fun requirePresented(expectedRevision: Long) {
        check(framePresented) { "native camera receipt does not describe a presented frame" }
        check(currentRevisionPresented) {
            "native camera receipt current revision was not the presented revision"
        }
        check(cameraRevision == expectedRevision) {
            "native camera receipt revision $cameraRevision does not match frame $expectedRevision"
        }
        check(presentedCameraRevision == cameraRevision) {
            "native camera receipt current/presented revisions differ"
        }
        check(surfaceWidth > 0 && surfaceHeight > 0) {
            "native camera receipt has an invalid Surface size"
        }
    }

    companion object {
        const val RAW_VALUE_COUNT = 63
        private const val FLAG_FRAME_PRESENTED = 1 shl 0
        private const val FLAG_CURRENT_REVISION_PRESENTED = 1 shl 1

        fun query(nativeHandle: Long): Result<BenchmarkCameraReceipt> = runCatching {
            val raw = LongArray(RAW_VALUE_COUNT)
            val rc = BenchmarkBridge.getSurfaceCameraReceiptV1(nativeHandle, raw)
            check(rc == 0) {
                val detail = NativeBridge.lastErrorMessage()
                    .ifBlank { NativeBridge.errorMessage(rc) }
                "camera receipt query failed rc=$rc error=$detail"
            }
            fromRaw(raw)
        }

        fun fromRaw(raw: LongArray): BenchmarkCameraReceipt {
            require(raw.size >= RAW_VALUE_COUNT) { "camera receipt array is too small" }
            fun floatAt(index: Int): Float = Float.fromBits(raw[index].toInt())
            val receipt = BenchmarkCameraReceipt(
                cameraRevision = raw[0],
                presentedCameraRevision = raw[1],
                surfaceWidth = raw[2].toInt(),
                surfaceHeight = raw[3].toInt(),
                flags = raw[4].toInt(),
                position = FloatArray(3) { floatAt(5 + it) },
                rotationXyzw = FloatArray(4) { floatAt(8 + it) },
                verticalFovRadians = floatAt(12),
                nearPlane = floatAt(13),
                farPlane = floatAt(14),
                viewMatrix = FloatArray(16) { floatAt(15 + it) },
                projectionMatrix = FloatArray(16) { floatAt(31 + it) },
                viewProjectionMatrix = FloatArray(16) { floatAt(47 + it) }
            )
            val scalars = receipt.position.asSequence() +
                receipt.rotationXyzw.asSequence() +
                sequenceOf(
                    receipt.verticalFovRadians,
                    receipt.nearPlane,
                    receipt.farPlane
                ) +
                receipt.viewMatrix.asSequence() +
                receipt.projectionMatrix.asSequence() +
                receipt.viewProjectionMatrix.asSequence()
            check(scalars.all(Float::isFinite)) {
                "native camera receipt contains a non-finite value"
            }
            check(receipt.cameraRevision >= 0L && receipt.presentedCameraRevision >= 0L) {
                "native camera receipt revision overflowed the signed host representation"
            }
            return receipt
        }
    }
}

internal enum class BenchmarkCameraPresentationDecision {
    RECORD,
    WAIT_FOR_CURRENT_REVISION
}

/**
 * Admits only the narrow post-present transition exposed by the native receipt.
 *
 * A Surface frame may successfully present the previously committed camera
 * while a newly selected trace camera is current but not presented yet. The
 * gate holds that exact target revision across the next warmup render. It does
 * not accept missing presentation, revision regression, or a different camera
 * appearing while the target is pending.
 */
internal class BenchmarkCameraPresentationGate {
    private data class AwaitingPresentation(
        val targetRevision: Long,
        val lastPresentedRevision: Long
    )

    private var awaiting: AwaitingPresentation? = null

    fun decide(
        receipt: BenchmarkCameraReceipt,
        expectedRenderedRevision: Long
    ): BenchmarkCameraPresentationDecision {
        check(expectedRenderedRevision >= 0L) {
            "benchmark frame reported a negative camera revision"
        }
        check(receipt.surfaceWidth > 0 && receipt.surfaceHeight > 0) {
            "native camera receipt has an invalid Surface size"
        }

        val pending = awaiting
        if (pending != null) {
            val target = pending.targetRevision
            check(receipt.cameraRevision == target) {
                "native camera revision changed from pending $target to " +
                    "${receipt.cameraRevision} before presentation"
            }
            if (receipt.currentRevisionPresented) {
                receipt.requirePresented(target)
                check(expectedRenderedRevision == target) {
                    "render telemetry revision $expectedRenderedRevision does not match " +
                        "presented pending revision $target"
                }
                awaiting = null
                return BenchmarkCameraPresentationDecision.RECORD
            }
            requireAwaitable(receipt, expectedRenderedRevision, target)
            check(receipt.presentedCameraRevision > pending.lastPresentedRevision) {
                "pending camera revision $target made no presentation progress from " +
                    "${pending.lastPresentedRevision}"
            }
            awaiting = AwaitingPresentation(target, receipt.presentedCameraRevision)
            return BenchmarkCameraPresentationDecision.WAIT_FOR_CURRENT_REVISION
        }

        if (receipt.currentRevisionPresented) {
            receipt.requirePresented(expectedRenderedRevision)
            return BenchmarkCameraPresentationDecision.RECORD
        }

        check(receipt.cameraRevision > receipt.presentedCameraRevision) {
            "native camera receipt is stale without a newer pending revision"
        }
        val pendingTarget = receipt.cameraRevision
        requireAwaitable(receipt, expectedRenderedRevision, pendingTarget)
        awaiting = AwaitingPresentation(pendingTarget, receipt.presentedCameraRevision)
        return BenchmarkCameraPresentationDecision.WAIT_FOR_CURRENT_REVISION
    }

    private fun requireAwaitable(
        receipt: BenchmarkCameraReceipt,
        expectedRenderedRevision: Long,
        target: Long
    ) {
        check(receipt.framePresented) {
            "native camera receipt does not describe a presented frame"
        }
        check(!receipt.currentRevisionPresented) {
            "native camera receipt unexpectedly marked the pending revision presented"
        }
        check(receipt.cameraRevision == target) {
            "native camera receipt does not retain pending revision $target"
        }
        check(receipt.presentedCameraRevision < target) {
            "native presented camera revision did not precede pending revision $target"
        }
        check(
            expectedRenderedRevision == receipt.presentedCameraRevision ||
                expectedRenderedRevision == target
        ) {
            "render telemetry revision $expectedRenderedRevision matches neither " +
                "presented ${receipt.presentedCameraRevision} nor pending $target"
        }
    }
}
