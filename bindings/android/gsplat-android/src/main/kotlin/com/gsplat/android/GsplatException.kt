package com.gsplat.android

class GsplatException(
    val code: Int,
    val detail: String = NativeBridge.lastErrorMessage().ifBlank { NativeBridge.errorMessage(code) }
) : RuntimeException("gsplat error $code: $detail")
