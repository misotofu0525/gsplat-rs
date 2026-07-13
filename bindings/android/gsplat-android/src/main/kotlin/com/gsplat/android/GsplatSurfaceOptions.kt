package com.gsplat.android

enum class GsplatGeometryPath(internal val nativeValue: Int) {
    DIRECT(0),
    PACKED_ATLAS(1),
    PAGED_ACTIVE_ATLAS(2)
}

data class GsplatSurfaceOptions(
    val sortInterval: Int = 2,
    val asyncSort: Boolean = false,
    val frameLatency: Int = 2,
    val geometryPath: GsplatGeometryPath = GsplatGeometryPath.DIRECT
)
