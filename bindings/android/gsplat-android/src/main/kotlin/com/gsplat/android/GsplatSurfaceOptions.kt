package com.gsplat.android

data class GsplatSurfaceOptions(
    val sortInterval: Int = 2,
    val asyncSort: Boolean = false,
    val frameLatency: Int = 2
)
