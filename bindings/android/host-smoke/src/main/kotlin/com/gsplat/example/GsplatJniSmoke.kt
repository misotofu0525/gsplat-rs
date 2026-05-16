package com.gsplat.example

object GsplatJniSmoke {
    init {
        System.loadLibrary("gsplat_jni")
    }

    @JvmStatic
    private external fun nativeVersionMajor(): Int

    @JvmStatic
    private external fun nativeVersionMinor(): Int

    @JvmStatic
    private external fun nativeFfiSmoke(datasetPath: String): Int

    @JvmStatic
    fun main(args: Array<String>) {
        val datasetPath = args.firstOrNull() ?: "tests/datasets/minimal_ascii.ply"
        val major = nativeVersionMajor()
        val minor = nativeVersionMinor()

        if (major != 0 || minor != 1) {
            System.err.printf("unexpected ABI version: %d.%d%n", major, minor)
            kotlin.system.exitProcess(30)
        }

        val rc = nativeFfiSmoke(datasetPath)
        if (rc != 0) {
            System.err.printf("JNI ffi smoke failed with code=%d%n", rc)
            kotlin.system.exitProcess(rc)
        }

        println("jni smoke ok")
    }
}
