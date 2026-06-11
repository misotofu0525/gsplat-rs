package com.gsplat.android

data class GsplatSurfaceOptions(
    val sortInterval: Int = 2,
    // Experimental benchmark knobs; keep defaults for stable integrations.
    val gpuPreproject: Boolean = false,
    val gpuPreprojectDoubleBuffer: Boolean = false,
    // Static-direct is the default render path (fastest on device benchmarks).
    val staticDirect: Boolean = true,
    val asyncSort: Boolean = false,
    val asyncGeometry: Boolean = false,
    val instanceBufferCount: Int = 1,
    val frameLatency: Int = 2
)
