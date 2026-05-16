package com.gsplat.android

object GsplatAndroidVersion {
    const val SUPPORTED_MAJOR: Int = 0
    const val SUPPORTED_MINOR: Int = 1

    val currentMajor: Int
        get() = NativeBridge.versionMajor()

    val currentMinor: Int
        get() = NativeBridge.versionMinor()

    fun requireSupported() {
        val major = currentMajor
        val minor = currentMinor
        check(major == SUPPORTED_MAJOR && minor == SUPPORTED_MINOR) {
            "unsupported gsplat native ABI version $major.$minor"
        }
    }
}
