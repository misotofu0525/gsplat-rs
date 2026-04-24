#include <jni.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#if defined(__ANDROID__)
#include <android/log.h>
#include <android/native_window_jni.h>
#endif

#include "../../../crates/gsplat-ffi-c/include/gsplat.h"

JNIEXPORT jint JNICALL Java_com_gsplat_demo_GsplatJniSmoke_nativeVersionMajor(JNIEnv *env, jclass cls) {
  (void)env;
  (void)cls;
  return (jint)gsplat_version_major();
}

JNIEXPORT jint JNICALL Java_com_gsplat_demo_NativeBridge_versionMajor(JNIEnv *env, jclass cls) {
  return Java_com_gsplat_demo_GsplatJniSmoke_nativeVersionMajor(env, cls);
}

JNIEXPORT jint JNICALL Java_com_gsplat_demo_GsplatJniSmoke_nativeVersionMinor(JNIEnv *env, jclass cls) {
  (void)env;
  (void)cls;
  return (jint)gsplat_version_minor();
}

JNIEXPORT jint JNICALL Java_com_gsplat_demo_NativeBridge_versionMinor(JNIEnv *env, jclass cls) {
  return Java_com_gsplat_demo_GsplatJniSmoke_nativeVersionMinor(env, cls);
}

JNIEXPORT jint JNICALL Java_com_gsplat_demo_GsplatJniSmoke_nativeFfiSmoke(JNIEnv *env, jclass cls, jstring dataset_path) {
  (void)cls;

  const char *dataset = (*env)->GetStringUTFChars(env, dataset_path, NULL);
  if (dataset == NULL) {
    return 20;
  }

  GsplatConfig config = {1280, 720, 0};
  GsplatContext *ctx = NULL;
  int32_t rc = gsplat_context_create(config, &ctx);
  if (rc != 0 || ctx == NULL) {
    (*env)->ReleaseStringUTFChars(env, dataset_path, dataset);
    return rc == 0 ? 21 : rc;
  }

  GsplatCamera camera;
  memset(&camera, 0, sizeof(camera));
  camera.rotation_xyzw[3] = 1.0f;
  camera.vertical_fov_radians = 1.0471976f;
  camera.near_plane = 0.01f;
  camera.far_plane = 1000.0f;

  rc = gsplat_context_set_camera(ctx, camera);
  if (rc != 0) {
    gsplat_context_destroy(ctx);
    (*env)->ReleaseStringUTFChars(env, dataset_path, dataset);
    return rc;
  }

  rc = gsplat_context_load_scene_path(ctx, dataset);
  if (rc != 0) {
    gsplat_context_destroy(ctx);
    (*env)->ReleaseStringUTFChars(env, dataset_path, dataset);
    return rc;
  }

  rc = gsplat_context_render_frame(ctx);
  if (rc != 0) {
    gsplat_context_destroy(ctx);
    (*env)->ReleaseStringUTFChars(env, dataset_path, dataset);
    return rc;
  }

  GsplatStats stats;
  memset(&stats, 0, sizeof(stats));
  rc = gsplat_context_get_stats(ctx, &stats);

  gsplat_context_destroy(ctx);
  (*env)->ReleaseStringUTFChars(env, dataset_path, dataset);

  if (rc != 0) {
    return rc;
  }

  if (stats.drawn_count == 0 || stats.visible_count == 0) {
    return 22;
  }

  return 0;
}

JNIEXPORT jint JNICALL Java_com_gsplat_demo_NativeBridge_runFfiSmoke(
    JNIEnv *env,
    jclass cls,
    jstring dataset_path) {
  return Java_com_gsplat_demo_GsplatJniSmoke_nativeFfiSmoke(env, cls, dataset_path);
}

#if defined(__ANDROID__)

#define GSPLAT_LOG_TAG "gsplat_jni"

typedef struct AndroidSurfaceRendererHandle {
  GsplatSurfaceRenderer *renderer;
  ANativeWindow *window;
} AndroidSurfaceRendererHandle;

static AndroidSurfaceRendererHandle *android_handle_from_jlong(jlong native_handle) {
  return (AndroidSurfaceRendererHandle *)(intptr_t)native_handle;
}

JNIEXPORT jlong JNICALL Java_com_gsplat_demo_NativeBridge_createSurfaceRenderer(
    JNIEnv *env,
    jclass cls,
    jobject surface,
    jstring dataset_path,
    jint width,
    jint height) {
  (void)cls;

  if (surface == NULL || width <= 0 || height <= 0) {
    return 0;
  }

  const char *dataset = (*env)->GetStringUTFChars(env, dataset_path, NULL);
  if (dataset == NULL) {
    return 0;
  }

  ANativeWindow *window = ANativeWindow_fromSurface(env, surface);
  if (window == NULL) {
    __android_log_print(ANDROID_LOG_ERROR, GSPLAT_LOG_TAG, "ANativeWindow_fromSurface failed");
    (*env)->ReleaseStringUTFChars(env, dataset_path, dataset);
    return 0;
  }

  AndroidSurfaceRendererHandle *handle =
      (AndroidSurfaceRendererHandle *)calloc(1, sizeof(AndroidSurfaceRendererHandle));
  if (handle == NULL) {
    __android_log_print(ANDROID_LOG_ERROR, GSPLAT_LOG_TAG, "surface renderer handle allocation failed");
    ANativeWindow_release(window);
    (*env)->ReleaseStringUTFChars(env, dataset_path, dataset);
    return 0;
  }

  __android_log_print(
      ANDROID_LOG_INFO,
      GSPLAT_LOG_TAG,
      "creating surface renderer width=%d height=%d dataset=%s",
      width,
      height,
      dataset);

  GsplatSurfaceRenderer *renderer = NULL;
  int32_t rc = gsplat_surface_renderer_create_android(
      (void *)window,
      dataset,
      (uint32_t)width,
      (uint32_t)height,
      &renderer);

  (*env)->ReleaseStringUTFChars(env, dataset_path, dataset);

  if (rc != 0 || renderer == NULL) {
    __android_log_print(
        ANDROID_LOG_ERROR,
        GSPLAT_LOG_TAG,
        "gsplat_surface_renderer_create_android failed rc=%d renderer=%p",
        rc,
        (void *)renderer);
    ANativeWindow_release(window);
    free(handle);
    return 0;
  }

  __android_log_print(ANDROID_LOG_INFO, GSPLAT_LOG_TAG, "surface renderer created");
  handle->renderer = renderer;
  handle->window = window;
  return (jlong)(intptr_t)handle;
}

JNIEXPORT jint JNICALL Java_com_gsplat_demo_NativeBridge_resizeSurfaceRenderer(
    JNIEnv *env,
    jclass cls,
    jlong native_handle,
    jint width,
    jint height) {
  (void)env;
  (void)cls;

  AndroidSurfaceRendererHandle *handle = android_handle_from_jlong(native_handle);
  if (handle == NULL || handle->renderer == NULL || width <= 0 || height <= 0) {
    return 1;
  }

  return gsplat_surface_renderer_resize(
      handle->renderer,
      (uint32_t)width,
      (uint32_t)height);
}

JNIEXPORT jint JNICALL Java_com_gsplat_demo_NativeBridge_renderSurfaceFrame(
    JNIEnv *env,
    jclass cls,
    jlong native_handle) {
  (void)env;
  (void)cls;

  AndroidSurfaceRendererHandle *handle = android_handle_from_jlong(native_handle);
  if (handle == NULL || handle->renderer == NULL) {
    return 1;
  }

  return gsplat_surface_renderer_render_frame(handle->renderer);
}

JNIEXPORT jint JNICALL Java_com_gsplat_demo_NativeBridge_getSurfaceStats(
    JNIEnv *env,
    jclass cls,
    jlong native_handle,
    jlongArray out_stats) {
  (void)cls;

  AndroidSurfaceRendererHandle *handle = android_handle_from_jlong(native_handle);
  if (handle == NULL || handle->renderer == NULL || out_stats == NULL) {
    return 1;
  }
  if ((*env)->GetArrayLength(env, out_stats) < 6) {
    return 1;
  }

  GsplatStats stats;
  memset(&stats, 0, sizeof(stats));
  int32_t rc = gsplat_surface_renderer_get_stats(handle->renderer, &stats);
  if (rc != 0) {
    return rc;
  }

  jlong values[6];
  values[0] = (jlong)stats.visible_count;
  values[1] = (jlong)stats.drawn_count;
  values[2] = (jlong)(stats.frame_ms * 1000.0f);
  values[3] = (jlong)(stats.preprocess_ms * 1000.0f);
  values[4] = (jlong)(stats.sort_ms * 1000.0f);
  values[5] = (jlong)(stats.raster_ms * 1000.0f);
  (*env)->SetLongArrayRegion(env, out_stats, 0, 6, values);
  return 0;
}

JNIEXPORT void JNICALL Java_com_gsplat_demo_NativeBridge_destroySurfaceRenderer(
    JNIEnv *env,
    jclass cls,
    jlong native_handle) {
  (void)env;
  (void)cls;

  AndroidSurfaceRendererHandle *handle = android_handle_from_jlong(native_handle);
  if (handle == NULL) {
    return;
  }

  if (handle->renderer != NULL) {
    gsplat_surface_renderer_destroy(handle->renderer);
  }
  if (handle->window != NULL) {
    ANativeWindow_release(handle->window);
  }
  free(handle);
}

#endif
