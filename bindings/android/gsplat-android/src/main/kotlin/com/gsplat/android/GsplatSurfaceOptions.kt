package com.gsplat.android

enum class GsplatGeometryPath(internal val nativeValue: Int) {
    DIRECT(0),
    PACKED_ATLAS(1),
    PAGED_ACTIVE_ATLAS(2)
}

enum class GsplatSurfaceOrderBackend(internal val nativeValue: Int) {
    CPU(0),
    GPU(1),
    ADAPTIVE(2);

    internal companion object {
        fun fromNative(value: Int): GsplatSurfaceOrderBackend =
            entries.firstOrNull { it.nativeValue == value }
                ?: error("unsupported native Surface order backend: $value")
    }
}

data class GsplatSurfaceOptions(
    val sortInterval: Int = 1,
    val asyncSort: Boolean = false,
    val frameLatency: Int = 2,
    val geometryPath: GsplatGeometryPath = GsplatGeometryPath.PACKED_ATLAS,
    val orderBackend: GsplatSurfaceOrderBackend = GsplatSurfaceOrderBackend.ADAPTIVE,
    val projectedPolicy: GsplatSurfaceProjectedPolicy = GsplatSurfaceProjectedPolicy.ADAPTIVE
) {
    init {
        require(sortInterval > 0) { "sortInterval must be positive" }
        require(frameLatency in 1..4) { "frameLatency must be in 1..4" }
        require(!asyncSort || orderBackend == GsplatSurfaceOrderBackend.CPU) {
            "asyncSort is available only with the CPU order backend"
        }
        require(
            geometryPath != GsplatGeometryPath.PAGED_ACTIVE_ATLAS ||
                orderBackend == GsplatSurfaceOrderBackend.CPU
        ) {
            "PAGED_ACTIVE_ATLAS is diagnostic-only and supports the CPU order backend"
        }
    }
}
