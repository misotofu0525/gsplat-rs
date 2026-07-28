package com.gsplat.android

import android.view.Surface
import java.io.Closeable

class GsplatSurfaceRenderer private constructor(
    private var nativeHandle: Long
) : Closeable {
    private val lock = Any()
    private val currentStatsAdapter = GsplatSurfaceCurrentStatsAdapter()

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
            checkResult(
                NativeBridge.setSurfaceGpuProducerMeasurementEnabledV1(nativeHandle, false)
            )
            checkResult(
                NativeBridge.setSurfaceGpuOrderProducerV1(
                    nativeHandle,
                    GsplatSurfaceGpuOrderProducer.POST_SORT.nativeValue
                )
            )
            checkResult(NativeBridge.setSurfaceSortInterval(nativeHandle, options.sortInterval))
            if (options.asyncSort) {
                checkResult(
                    NativeBridge.setSurfaceOrderBackend(
                        nativeHandle,
                        GsplatSurfaceOrderBackend.CPU.nativeValue
                    )
                )
                checkResult(NativeBridge.setSurfaceAsyncSortEnabled(nativeHandle, true))
            } else {
                checkResult(NativeBridge.setSurfaceAsyncSortEnabled(nativeHandle, false))
                checkResult(
                    NativeBridge.setSurfaceOrderBackend(
                        nativeHandle,
                        options.orderBackend.nativeValue
                    )
                )
            }
            checkResult(NativeBridge.setSurfaceFrameLatency(nativeHandle, options.frameLatency))
            checkResult(
                NativeBridge.setSurfaceProjectedPolicyV1(
                    nativeHandle,
                    options.projectedPolicy.nativeValue
                )
            )
            options.gpuProducerDiagnostics?.let { diagnostics ->
                checkResult(
                    NativeBridge.setSurfaceGpuOrderProducerV1(
                        nativeHandle,
                        diagnostics.producer.nativeValue
                    )
                )
                checkResult(
                    NativeBridge.setSurfaceGpuProducerMeasurementEnabledV1(nativeHandle, true)
                )
            }
        }
    }

    /** Changes exact Candidate/Compact/Adaptive projected-draw execution. */
    fun setProjectedPolicy(policy: GsplatSurfaceProjectedPolicy) {
        synchronized(lock) {
            checkOpen()
            checkResult(NativeBridge.setSurfaceProjectedPolicyV1(nativeHandle, policy.nativeValue))
        }
    }

    /** Enables a strict producer diagnostic after the caller configures its exact context. */
    fun setGpuProducerDiagnostics(diagnostics: GsplatSurfaceGpuProducerDiagnostics?) {
        synchronized(lock) {
            checkOpen()
            checkResult(
                NativeBridge.setSurfaceGpuProducerMeasurementEnabledV1(nativeHandle, false)
            )
            checkResult(
                NativeBridge.setSurfaceGpuOrderProducerV1(
                    nativeHandle,
                    diagnostics?.producer?.nativeValue
                        ?: GsplatSurfaceGpuOrderProducer.POST_SORT.nativeValue
                )
            )
            if (diagnostics != null) {
                checkResult(
                    NativeBridge.setSurfaceGpuProducerMeasurementEnabledV1(nativeHandle, true)
                )
            }
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
            val renderCode = NativeBridge.renderSurfaceFrame(nativeHandle)
            checkResult(renderCode)
            if (currentStatsAdapter.requiresSubmissionReconciliation) {
                currentStatsAdapter.reconcileAfterOrdinaryRender(nativeHandle)
            } else {
                currentStatsAdapter.observeOrdinaryRender()
            }
        }
    }

    /** Explicit observer path: request, render, read submission, and single-poll. */
    fun renderFrameWithCurrentStats(): GsplatSurfaceCurrentStatsCycle {
        synchronized(lock) {
            checkOpen()
            val request = currentStatsAdapter.request(nativeHandle)
            val renderCode = NativeBridge.renderSurfaceFrame(nativeHandle)
            if (renderCode != 0) {
                currentStatsAdapter.abandonFrame()
                checkResult(renderCode)
            }
            return currentStatsAdapter.complete(nativeHandle, request)
        }
    }

    /** Advances one already-issued current-stats ticket without requesting or rendering. */
    fun pollCurrentStats(): GsplatSurfaceCurrentStatsState {
        synchronized(lock) {
            checkOpen()
            return currentStatsAdapter.poll(nativeHandle)
        }
    }

    /**
     * Destructively polls the additive timing-bearing V2 current-stats value.
     * This consumes the same native single-pop queue as [pollCurrentStats], so
     * a caller must choose one API for each expected terminal. The result is
     * also projected into the existing V1 adapter bookkeeping without timing,
     * preserving pending/tombstone and presentation-watermark behavior.
     */
    fun pollCurrentStatsV2(): GsplatSurfaceCurrentStatsPollV2 {
        synchronized(lock) {
            checkOpen()
            val raw = LongArray(GsplatSurfaceCurrentStatsPollV2.RAW_VALUE_COUNT)
            checkResult(NativeBridge.pollSurfaceCurrentStatsV2(nativeHandle, raw))
            val poll = GsplatSurfaceCurrentStatsPollV2.fromRaw(raw)
            currentStatsAdapter.consumePollResult(poll.withoutTiming())
            return poll
        }
    }

    /**
     * Returns this adapter's latest current-stats state. Counts exist only in
     * [GsplatSurfaceCurrentStatsState.Ready]; all other states intentionally
     * carry no fallback from [stats].
     */
    fun currentStats(): GsplatSurfaceCurrentStatsState = synchronized(lock) {
        checkOpen()
        currentStatsAdapter.state
    }

    fun stats(): GsplatSurfaceStats {
        synchronized(lock) {
            checkOpen()
            val raw = LongArray(6)
            checkResult(NativeBridge.getSurfaceStats(nativeHandle, raw))
            return GsplatSurfaceStats.fromRaw(raw)
        }
    }

    /** Returns the last frame's ordering backend, revisions, and Adaptive GPU status. */
    fun orderStatus(): GsplatSurfaceOrderStatus {
        synchronized(lock) {
            checkOpen()
            val raw = LongArray(GsplatSurfaceOrderStatus.RAW_VALUE_COUNT)
            checkResult(NativeBridge.getSurfaceSortStats(nativeHandle, raw))
            return GsplatSurfaceOrderStatus.fromRaw(raw)
        }
    }

    /** Returns the CPU/GPU measurement submission identity for the last successful frame. */
    fun orderSubmission(): GsplatSurfaceOrderSubmission {
        synchronized(lock) {
            checkOpen()
            val raw = LongArray(GsplatSurfaceOrderSubmission.RAW_VALUE_COUNT)
            checkResult(NativeBridge.getSurfaceOrderSubmission(nativeHandle, raw))
            return GsplatSurfaceOrderSubmission.fromRaw(raw)
        }
    }

    /** Returns projected-draw identity for the last successful frame. */
    fun projectedSubmission(): GsplatSurfaceProjectedSubmission {
        synchronized(lock) {
            checkOpen()
            val raw = LongArray(GsplatSurfaceProjectedSubmission.RAW_VALUE_COUNT)
            checkResult(NativeBridge.getSurfaceProjectedSubmissionV1(nativeHandle, raw))
            return GsplatSurfaceProjectedSubmission.fromRaw(raw)
        }
    }

    /** Returns one terminal projected-draw success with its exact V/C/D counts. */
    fun pollProjectedMeasurement(): GsplatSurfaceProjectedMeasurement? {
        synchronized(lock) {
            checkOpen()
            val raw = LongArray(GsplatSurfaceProjectedMeasurement.RAW_VALUE_COUNT)
            checkResult(NativeBridge.pollSurfaceProjectedMeasurementV1(nativeHandle, raw))
            return GsplatSurfaceProjectedMeasurement.fromRaw(raw)
        }
    }

    /** Drains currently available projected-draw successes in ticket order. */
    fun drainProjectedMeasurements(): List<GsplatSurfaceProjectedMeasurement> {
        synchronized(lock) {
            checkOpen()
            val completed = mutableListOf<GsplatSurfaceProjectedMeasurement>()
            while (true) {
                val raw = LongArray(GsplatSurfaceProjectedMeasurement.RAW_VALUE_COUNT)
                checkResult(NativeBridge.pollSurfaceProjectedMeasurementV1(nativeHandle, raw))
                val measurement = GsplatSurfaceProjectedMeasurement.fromRaw(raw) ?: break
                completed += measurement
            }
            return completed
        }
    }

    /** Returns one terminal projected-draw failure, or null without blocking. */
    fun pollProjectedMeasurementFailure(): GsplatSurfaceProjectedMeasurementFailure? {
        synchronized(lock) {
            checkOpen()
            val raw = LongArray(GsplatSurfaceProjectedMeasurementFailure.RAW_VALUE_COUNT)
            checkResult(NativeBridge.pollSurfaceProjectedFailureV1(nativeHandle, raw))
            return GsplatSurfaceProjectedMeasurementFailure.fromRaw(raw)
        }
    }

    /** Drains currently available projected-draw terminal failures. */
    fun drainProjectedMeasurementFailures(): List<GsplatSurfaceProjectedMeasurementFailure> {
        synchronized(lock) {
            checkOpen()
            val failures = mutableListOf<GsplatSurfaceProjectedMeasurementFailure>()
            while (true) {
                val raw = LongArray(GsplatSurfaceProjectedMeasurementFailure.RAW_VALUE_COUNT)
                checkResult(NativeBridge.pollSurfaceProjectedFailureV1(nativeHandle, raw))
                val failure = GsplatSurfaceProjectedMeasurementFailure.fromRaw(raw) ?: break
                failures += failure
            }
            return failures
        }
    }

    /** Returns producer identity for the last successfully rendered frame. */
    fun gpuProducerSubmission(): GsplatSurfaceGpuProducerSubmission {
        synchronized(lock) {
            checkOpen()
            val raw = LongArray(GsplatSurfaceGpuProducerSubmission.RAW_VALUE_COUNT)
            checkResult(NativeBridge.getSurfaceGpuProducerSubmissionV1(nativeHandle, raw))
            return GsplatSurfaceGpuProducerSubmission.fromRaw(raw)
        }
    }

    /** Returns one terminal producer success with exact S/C/D counts. */
    fun pollGpuProducerMeasurement(): GsplatSurfaceGpuProducerMeasurement? {
        synchronized(lock) {
            checkOpen()
            val raw = LongArray(GsplatSurfaceGpuProducerMeasurement.RAW_VALUE_COUNT)
            checkResult(NativeBridge.pollSurfaceGpuProducerMeasurementV1(nativeHandle, raw))
            return GsplatSurfaceGpuProducerMeasurement.fromRaw(raw)
        }
    }

    /** Drains producer successes currently available in native ticket order. */
    fun drainGpuProducerMeasurements(): List<GsplatSurfaceGpuProducerMeasurement> {
        synchronized(lock) {
            checkOpen()
            val completed = mutableListOf<GsplatSurfaceGpuProducerMeasurement>()
            while (true) {
                val raw = LongArray(GsplatSurfaceGpuProducerMeasurement.RAW_VALUE_COUNT)
                checkResult(NativeBridge.pollSurfaceGpuProducerMeasurementV1(nativeHandle, raw))
                val measurement = GsplatSurfaceGpuProducerMeasurement.fromRaw(raw) ?: break
                completed += measurement
            }
            return completed
        }
    }

    /** Returns one terminal producer failure, or null without blocking. */
    fun pollGpuProducerMeasurementFailure(): GsplatSurfaceGpuProducerMeasurementFailure? {
        synchronized(lock) {
            checkOpen()
            val raw = LongArray(GsplatSurfaceGpuProducerMeasurementFailure.RAW_VALUE_COUNT)
            checkResult(NativeBridge.pollSurfaceGpuProducerFailureV1(nativeHandle, raw))
            return GsplatSurfaceGpuProducerMeasurementFailure.fromRaw(raw)
        }
    }

    /** Drains terminal producer failures currently available. */
    fun drainGpuProducerMeasurementFailures(): List<GsplatSurfaceGpuProducerMeasurementFailure> {
        synchronized(lock) {
            checkOpen()
            val failures = mutableListOf<GsplatSurfaceGpuProducerMeasurementFailure>()
            while (true) {
                val raw = LongArray(GsplatSurfaceGpuProducerMeasurementFailure.RAW_VALUE_COUNT)
                checkResult(NativeBridge.pollSurfaceGpuProducerFailureV1(nativeHandle, raw))
                val failure = GsplatSurfaceGpuProducerMeasurementFailure.fromRaw(raw) ?: break
                failures += failure
            }
            return failures
        }
    }

    /** Returns the current exactness/capability receipt for this Surface. */
    fun exactness(): GsplatSurfaceExactness {
        synchronized(lock) {
            checkOpen()
            val raw = LongArray(GsplatSurfaceExactness.RAW_VALUE_COUNT)
            checkResult(NativeBridge.getSurfaceExactness(nativeHandle, raw))
            return GsplatSurfaceExactness.fromRaw(raw)
        }
    }

    /** Returns native requested/Surface/internal/presented pixel dimensions. */
    fun presentation(): GsplatSurfacePresentation {
        synchronized(lock) {
            checkOpen()
            val raw = LongArray(GsplatSurfacePresentation.RAW_VALUE_COUNT)
            checkResult(NativeBridge.getSurfacePresentation(nativeHandle, raw))
            return GsplatSurfacePresentation.fromRaw(raw)
        }
    }

    /** Returns one completed GPU order receipt, or null without blocking. */
    fun pollOrderMeasurement(): GsplatSurfaceOrderMeasurement? {
        synchronized(lock) {
            checkOpen()
            val raw = LongArray(GsplatSurfaceOrderMeasurement.RAW_VALUE_COUNT)
            checkResult(NativeBridge.pollSurfaceOrderMeasurement(nativeHandle, raw))
            return GsplatSurfaceOrderMeasurement.fromRaw(raw)
        }
    }

    /** Drains all GPU order receipts currently available in native ticket order. */
    fun drainOrderMeasurements(): List<GsplatSurfaceOrderMeasurement> {
        synchronized(lock) {
            checkOpen()
            val completed = mutableListOf<GsplatSurfaceOrderMeasurement>()
            while (true) {
                val raw = LongArray(GsplatSurfaceOrderMeasurement.RAW_VALUE_COUNT)
                checkResult(NativeBridge.pollSurfaceOrderMeasurement(nativeHandle, raw))
                val measurement = GsplatSurfaceOrderMeasurement.fromRaw(raw) ?: break
                completed += measurement
            }
            return completed
        }
    }

    /** Returns one CPU frame-start-to-queue-completion receipt, or null without blocking. */
    fun pollCpuOrderMeasurement(): GsplatSurfaceCpuOrderMeasurement? {
        synchronized(lock) {
            checkOpen()
            val raw = LongArray(GsplatSurfaceCpuOrderMeasurement.RAW_VALUE_COUNT)
            checkResult(NativeBridge.pollSurfaceCpuOrderMeasurement(nativeHandle, raw))
            return GsplatSurfaceCpuOrderMeasurement.fromRaw(raw)
        }
    }

    /** Drains all currently available CPU queue-completion receipts in ticket order. */
    fun drainCpuOrderMeasurements(): List<GsplatSurfaceCpuOrderMeasurement> {
        synchronized(lock) {
            checkOpen()
            val completed = mutableListOf<GsplatSurfaceCpuOrderMeasurement>()
            while (true) {
                val raw = LongArray(GsplatSurfaceCpuOrderMeasurement.RAW_VALUE_COUNT)
                checkResult(NativeBridge.pollSurfaceCpuOrderMeasurement(nativeHandle, raw))
                val measurement = GsplatSurfaceCpuOrderMeasurement.fromRaw(raw) ?: break
                completed += measurement
            }
            return completed
        }
    }

    /** Returns one terminal CPU/GPU measurement failure, or null without blocking. */
    fun pollOrderMeasurementFailure(): GsplatSurfaceOrderMeasurementFailure? {
        synchronized(lock) {
            checkOpen()
            val raw = LongArray(GsplatSurfaceOrderMeasurementFailure.RAW_VALUE_COUNT)
            checkResult(NativeBridge.pollSurfaceOrderMeasurementFailure(nativeHandle, raw))
            return GsplatSurfaceOrderMeasurementFailure.fromRaw(raw)
        }
    }

    /** Drains all terminal CPU/GPU measurement failures currently available. */
    fun drainOrderMeasurementFailures(): List<GsplatSurfaceOrderMeasurementFailure> {
        synchronized(lock) {
            checkOpen()
            val failures = mutableListOf<GsplatSurfaceOrderMeasurementFailure>()
            while (true) {
                val raw = LongArray(GsplatSurfaceOrderMeasurementFailure.RAW_VALUE_COUNT)
                checkResult(NativeBridge.pollSurfaceOrderMeasurementFailure(nativeHandle, raw))
                val failure = GsplatSurfaceOrderMeasurementFailure.fromRaw(raw) ?: break
                failures += failure
            }
            return failures
        }
    }

    override fun close() {
        synchronized(lock) {
            val handle = nativeHandle
            nativeHandle = 0L
            currentStatsAdapter.reset()
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
