package com.gsplat.demo;

public final class NativeBridge {
  static {
    System.loadLibrary("gsplat_jni");
  }

  private NativeBridge() {}

  public static native int versionMajor();

  public static native int versionMinor();

  public static native int runFfiSmoke(String datasetPath);
}
