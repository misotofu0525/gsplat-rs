package com.gsplat.android

class GsplatException(
    val code: Int,
    message: String = NativeBridge.errorMessage(code)
) : RuntimeException("gsplat error $code: $message")
