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

JNIEXPORT jstring JNICALL Java_com_gsplat_demo_NativeBridge_errorMessage(JNIEnv *env, jclass cls, jint code) {
  (void)cls;
  return (*env)->NewStringUTF(env, gsplat_error_message((int32_t)code));
}

JNIEXPORT jint JNICALL Java_com_gsplat_demo_GsplatJniSmoke_nativeFfiSmoke(JNIEnv *env, jclass cls, jstring dataset_path) {
  (void)cls;

  const char *dataset = (*env)->GetStringUTFChars(env, dataset_path, NULL);
  if (dataset == NULL) {
    return 20;
  }

  GsplatConfig config = gsplat_config_default();
  GsplatContext *ctx = NULL;
  int32_t rc = gsplat_context_create(config, &ctx);
  if (rc != 0 || ctx == NULL) {
    (*env)->ReleaseStringUTFChars(env, dataset_path, dataset);
    return rc == 0 ? 21 : rc;
  }

  GsplatCamera camera = gsplat_camera_default();

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

static void set_out_error(JNIEnv *env, jintArray out_error, int32_t rc) {
  if (out_error == NULL || (*env)->GetArrayLength(env, out_error) < 1) {
    return;
  }

  jint value = (jint)rc;
  (*env)->SetIntArrayRegion(env, out_error, 0, 1, &value);
}

static AndroidSurfaceRendererHandle *android_handle_from_jlong(jlong native_handle) {
  return (AndroidSurfaceRendererHandle *)(intptr_t)native_handle;
}

JNIEXPORT jlong JNICALL Java_com_gsplat_demo_NativeBridge_createSurfaceRenderer(
    JNIEnv *env,
    jclass cls,
    jobject surface,
    jstring dataset_path,
    jint width,
    jint height,
    jintArray out_error) {
  (void)cls;

  set_out_error(env, out_error, GSPLAT_OK);

  if (surface == NULL || dataset_path == NULL || width <= 0 || height <= 0) {
    set_out_error(env, out_error, GSPLAT_ERROR_INVALID_ARGUMENT);
    return 0;
  }

  const char *dataset = (*env)->GetStringUTFChars(env, dataset_path, NULL);
  if (dataset == NULL) {
    set_out_error(env, out_error, GSPLAT_ERROR_INTERNAL);
    return 0;
  }

  ANativeWindow *window = ANativeWindow_fromSurface(env, surface);
  if (window == NULL) {
    __android_log_print(ANDROID_LOG_ERROR, GSPLAT_LOG_TAG, "ANativeWindow_fromSurface failed");
    set_out_error(env, out_error, GSPLAT_ERROR_UNSUPPORTED);
    (*env)->ReleaseStringUTFChars(env, dataset_path, dataset);
    return 0;
  }

  AndroidSurfaceRendererHandle *handle =
      (AndroidSurfaceRendererHandle *)calloc(1, sizeof(AndroidSurfaceRendererHandle));
  if (handle == NULL) {
    __android_log_print(ANDROID_LOG_ERROR, GSPLAT_LOG_TAG, "surface renderer handle allocation failed");
    set_out_error(env, out_error, GSPLAT_ERROR_INTERNAL);
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
    set_out_error(env, out_error, rc == 0 ? GSPLAT_ERROR_INTERNAL : rc);
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
  set_out_error(env, out_error, GSPLAT_OK);
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

JNIEXPORT jint JNICALL Java_com_gsplat_demo_NativeBridge_setSurfaceSortInterval(
    JNIEnv *env,
    jclass cls,
    jlong native_handle,
    jint interval) {
  (void)env;
  (void)cls;

  AndroidSurfaceRendererHandle *handle = android_handle_from_jlong(native_handle);
  if (handle == NULL || handle->renderer == NULL || interval <= 0) {
    return GSPLAT_ERROR_INVALID_ARGUMENT;
  }

  return gsplat_surface_renderer_set_sort_interval(handle->renderer, (uint32_t)interval);
}

JNIEXPORT jint JNICALL Java_com_gsplat_demo_NativeBridge_setSurfaceGpuPreprojectEnabled(
    JNIEnv *env,
    jclass cls,
    jlong native_handle,
    jboolean enabled) {
  (void)env;
  (void)cls;

  AndroidSurfaceRendererHandle *handle = android_handle_from_jlong(native_handle);
  if (handle == NULL || handle->renderer == NULL) {
    return GSPLAT_ERROR_INVALID_ARGUMENT;
  }

  return gsplat_surface_renderer_set_gpu_preproject(
      handle->renderer,
      enabled == JNI_TRUE ? 1u : 0u);
}

JNIEXPORT jint JNICALL Java_com_gsplat_demo_NativeBridge_setSurfaceGpuPreprojectDoubleBufferEnabled(
    JNIEnv *env,
    jclass cls,
    jlong native_handle,
    jboolean enabled) {
  (void)env;
  (void)cls;

  AndroidSurfaceRendererHandle *handle = android_handle_from_jlong(native_handle);
  if (handle == NULL || handle->renderer == NULL) {
    return GSPLAT_ERROR_INVALID_ARGUMENT;
  }

  return gsplat_surface_renderer_set_gpu_preproject_double_buffer(
      handle->renderer,
      enabled == JNI_TRUE ? 1u : 0u);
}

JNIEXPORT jint JNICALL Java_com_gsplat_demo_NativeBridge_setSurfaceStaticDirectEnabled(
    JNIEnv *env,
    jclass cls,
    jlong native_handle,
    jboolean enabled) {
  (void)env;
  (void)cls;

  AndroidSurfaceRendererHandle *handle = android_handle_from_jlong(native_handle);
  if (handle == NULL || handle->renderer == NULL) {
    return GSPLAT_ERROR_INVALID_ARGUMENT;
  }

  return gsplat_surface_renderer_set_static_direct(
      handle->renderer,
      enabled == JNI_TRUE ? 1u : 0u);
}

JNIEXPORT jint JNICALL Java_com_gsplat_demo_NativeBridge_setSurfaceAsyncSortEnabled(
    JNIEnv *env,
    jclass cls,
    jlong native_handle,
    jboolean enabled) {
  (void)env;
  (void)cls;

  AndroidSurfaceRendererHandle *handle = android_handle_from_jlong(native_handle);
  if (handle == NULL || handle->renderer == NULL) {
    return GSPLAT_ERROR_INVALID_ARGUMENT;
  }

  return gsplat_surface_renderer_set_async_sort(
      handle->renderer,
      enabled == JNI_TRUE ? 1u : 0u);
}

JNIEXPORT jint JNICALL Java_com_gsplat_demo_NativeBridge_setSurfaceAsyncGeometryEnabled(
    JNIEnv *env,
    jclass cls,
    jlong native_handle,
    jboolean enabled) {
  (void)env;
  (void)cls;

  AndroidSurfaceRendererHandle *handle = android_handle_from_jlong(native_handle);
  if (handle == NULL || handle->renderer == NULL) {
    return GSPLAT_ERROR_INVALID_ARGUMENT;
  }

  return gsplat_surface_renderer_set_async_geometry(
      handle->renderer,
      enabled == JNI_TRUE ? 1u : 0u);
}

JNIEXPORT jint JNICALL Java_com_gsplat_demo_NativeBridge_setSurfaceInstanceBufferCount(
    JNIEnv *env,
    jclass cls,
    jlong native_handle,
    jint count) {
  (void)env;
  (void)cls;

  AndroidSurfaceRendererHandle *handle = android_handle_from_jlong(native_handle);
  if (handle == NULL || handle->renderer == NULL || count <= 0) {
    return GSPLAT_ERROR_INVALID_ARGUMENT;
  }

  return gsplat_surface_renderer_set_instance_buffer_count(
      handle->renderer,
      (uint32_t)count);
}

JNIEXPORT jint JNICALL Java_com_gsplat_demo_NativeBridge_setSurfaceFrameLatency(
    JNIEnv *env,
    jclass cls,
    jlong native_handle,
    jint latency) {
  (void)env;
  (void)cls;

  AndroidSurfaceRendererHandle *handle = android_handle_from_jlong(native_handle);
  if (handle == NULL || handle->renderer == NULL || latency <= 0) {
    return GSPLAT_ERROR_INVALID_ARGUMENT;
  }

  return gsplat_surface_renderer_set_frame_latency(
      handle->renderer,
      (uint32_t)latency);
}

JNIEXPORT jint JNICALL Java_com_gsplat_demo_NativeBridge_resetSurfaceCamera(
    JNIEnv *env,
    jclass cls,
    jlong native_handle) {
  (void)env;
  (void)cls;

  AndroidSurfaceRendererHandle *handle = android_handle_from_jlong(native_handle);
  if (handle == NULL || handle->renderer == NULL) {
    return GSPLAT_ERROR_INVALID_ARGUMENT;
  }

  return gsplat_surface_renderer_reset_camera(handle->renderer);
}

JNIEXPORT jint JNICALL Java_com_gsplat_demo_NativeBridge_orbitSurfaceRenderer(
    JNIEnv *env,
    jclass cls,
    jlong native_handle,
    jfloat delta_yaw_radians,
    jfloat delta_pitch_radians) {
  (void)env;
  (void)cls;

  AndroidSurfaceRendererHandle *handle = android_handle_from_jlong(native_handle);
  if (handle == NULL || handle->renderer == NULL) {
    return GSPLAT_ERROR_INVALID_ARGUMENT;
  }

  return gsplat_surface_renderer_orbit(
      handle->renderer,
      (float)delta_yaw_radians,
      (float)delta_pitch_radians);
}

JNIEXPORT jint JNICALL Java_com_gsplat_demo_NativeBridge_zoomSurfaceRenderer(
    JNIEnv *env,
    jclass cls,
    jlong native_handle,
    jfloat distance_scale) {
  (void)env;
  (void)cls;

  AndroidSurfaceRendererHandle *handle = android_handle_from_jlong(native_handle);
  if (handle == NULL || handle->renderer == NULL) {
    return GSPLAT_ERROR_INVALID_ARGUMENT;
  }

  return gsplat_surface_renderer_zoom(handle->renderer, (float)distance_scale);
}

JNIEXPORT jint JNICALL Java_com_gsplat_demo_NativeBridge_panSurfaceRenderer(
    JNIEnv *env,
    jclass cls,
    jlong native_handle,
    jfloat normalized_delta_x,
    jfloat normalized_delta_y) {
  (void)env;
  (void)cls;

  AndroidSurfaceRendererHandle *handle = android_handle_from_jlong(native_handle);
  if (handle == NULL || handle->renderer == NULL) {
    return GSPLAT_ERROR_INVALID_ARGUMENT;
  }

  return gsplat_surface_renderer_pan(
      handle->renderer,
      (float)normalized_delta_x,
      (float)normalized_delta_y);
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
