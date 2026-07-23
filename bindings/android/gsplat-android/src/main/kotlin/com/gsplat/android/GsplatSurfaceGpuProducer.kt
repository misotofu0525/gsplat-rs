package com.gsplat.android

/** Diagnostic Packed GPU graph selector; independent from CPU/GPU ordering. */
enum class GsplatSurfaceGpuOrderProducer(internal val nativeValue: Int) {
    POST_SORT(1),
    PREPROJECT(2);

    internal companion object {
        fun fromNative(value: Int): GsplatSurfaceGpuOrderProducer =
            entries.firstOrNull { it.nativeValue == value }
                ?: error("unsupported native Surface GPU producer: $value")
    }
}

enum class GsplatSurfaceGpuProducerDrawScope(internal val nativeValue: Int) {
    EXACT_CURRENT_CONTRIBUTORS(1),
    STALE_ORDER_CANDIDATES(2);

    internal companion object {
        fun fromNative(value: Int): GsplatSurfaceGpuProducerDrawScope =
            entries.firstOrNull { it.nativeValue == value }
                ?: error("unsupported native Surface GPU producer draw scope: $value")
    }
}

enum class GsplatSurfaceGpuProducerUnsampledReason {
    RING_BUSY,
    SURFACE_UNAVAILABLE
}

/** Explicit opt-in to the strict producer benchmark lane. */
data class GsplatSurfaceGpuProducerDiagnostics(
    val producer: GsplatSurfaceGpuOrderProducer
)
