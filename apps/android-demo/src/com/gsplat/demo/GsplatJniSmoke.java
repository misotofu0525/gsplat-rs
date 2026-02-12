package com.gsplat.demo;

public final class GsplatJniSmoke {
  static {
    System.loadLibrary("gsplat_jni");
  }

  private static native int nativeVersionMajor();
  private static native int nativeVersionMinor();
  private static native int nativeFfiSmoke(String datasetPath);

  private GsplatJniSmoke() {}

  public static void main(String[] args) {
    String datasetPath = args.length > 0 ? args[0] : "tests/datasets/minimal_ascii.ply";

    int major = nativeVersionMajor();
    int minor = nativeVersionMinor();

    if (major != 0 || minor != 1) {
      System.err.printf("unexpected ABI version: %d.%d%n", major, minor);
      System.exit(30);
      return;
    }

    int rc = nativeFfiSmoke(datasetPath);
    if (rc != 0) {
      System.err.printf("JNI ffi smoke failed with code=%d%n", rc);
      System.exit(rc);
      return;
    }

    System.out.println("jni smoke ok");
  }
}
