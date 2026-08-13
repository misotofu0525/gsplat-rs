package com.gsplat.example

/** Sample-only JNI controls that are deliberately absent from the Android AAR API. */
internal object BenchmarkBridge {
    init {
        System.loadLibrary("gsplat_jni")
    }

    /** Selects 0=CPU, 1=GPU, or 2=adaptive ordering for benchmark runs. */
    @JvmStatic
    external fun setSurfaceOrderBackend(nativeHandle: Long, backend: Int): Int

    /** Selects 0=full-f32 or 1=quantized resident storage for benchmark runs. */
    @JvmStatic
    external fun setSurfaceStorageProfile(nativeHandle: Long, profile: Int): Int
}
