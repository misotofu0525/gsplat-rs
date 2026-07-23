#include <jni.h>
#include <math.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#if defined(__ANDROID__)
#include <android/log.h>
#include <android/native_window_jni.h>
#endif

#include "../../../crates/gsplat-ffi-c/include/gsplat.h"

/* Camera traces remain qualification-only; order controls and receipts are
 * part of the additive public Surface ABI above. */
extern int32_t gsplat_benchmark_set_surface_camera_trace_frame_with_display_policy(
    GsplatSurfaceRenderer *renderer,
    const char *trace_path,
    uint32_t frame_index,
    uint32_t require_trace_display_match);

JNIEXPORT jint JNICALL Java_com_gsplat_example_GsplatJniSmoke_nativeVersionMajor(JNIEnv *env, jclass cls) {
  (void)env;
  (void)cls;
  return (jint)gsplat_version_major();
}

JNIEXPORT jint JNICALL Java_com_gsplat_android_NativeBridge_versionMajor(JNIEnv *env, jclass cls) {
  return Java_com_gsplat_example_GsplatJniSmoke_nativeVersionMajor(env, cls);
}

JNIEXPORT jint JNICALL Java_com_gsplat_example_GsplatJniSmoke_nativeVersionMinor(JNIEnv *env, jclass cls) {
  (void)env;
  (void)cls;
  return (jint)gsplat_version_minor();
}

JNIEXPORT jint JNICALL Java_com_gsplat_android_NativeBridge_versionMinor(JNIEnv *env, jclass cls) {
  return Java_com_gsplat_example_GsplatJniSmoke_nativeVersionMinor(env, cls);
}

JNIEXPORT jstring JNICALL Java_com_gsplat_android_NativeBridge_errorMessage(JNIEnv *env, jclass cls, jint code) {
  (void)cls;
  return (*env)->NewStringUTF(env, gsplat_error_message((int32_t)code));
}

JNIEXPORT jstring JNICALL Java_com_gsplat_android_NativeBridge_lastErrorMessage(JNIEnv *env, jclass cls) {
  (void)cls;
  return (*env)->NewStringUTF(env, gsplat_last_error_message());
}

JNIEXPORT jint JNICALL Java_com_gsplat_example_GsplatJniSmoke_nativeFfiSmoke(JNIEnv *env, jclass cls, jstring dataset_path) {
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

JNIEXPORT jint JNICALL Java_com_gsplat_android_NativeBridge_runFfiSmoke(
    JNIEnv *env,
    jclass cls,
    jstring dataset_path) {
  return Java_com_gsplat_example_GsplatJniSmoke_nativeFfiSmoke(env, cls, dataset_path);
}

JNIEXPORT jint JNICALL Java_com_gsplat_example_GsplatJniSmoke_nativeProjectedAbiSmoke(
    JNIEnv *env,
    jclass cls) {
  (void)env;
  (void)cls;

  GsplatSurfaceProjectedSubmissionV1 submission;
  memset(&submission, 0, sizeof(submission));
  submission.struct_size = (uint32_t)sizeof(submission);
  submission.version = GSPLAT_SURFACE_PROJECTED_ABI_VERSION_V1;
  if (gsplat_surface_renderer_set_projected_policy_v1(
          NULL,
          GSPLAT_SURFACE_PROJECTED_POLICY_ADAPTIVE) != GSPLAT_ERROR_INVALID_ARGUMENT) {
    return 40;
  }
  if (gsplat_surface_renderer_get_projected_submission_v1(NULL, &submission) !=
      GSPLAT_ERROR_INVALID_ARGUMENT) {
    return 41;
  }
  if (submission.struct_size != sizeof(submission) ||
      submission.version != GSPLAT_SURFACE_PROJECTED_ABI_VERSION_V1) {
    return 42;
  }
  return 0;
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

static jlong float_bits_to_jlong(float value) {
  uint32_t bits = 0;
  memcpy(&bits, &value, sizeof(bits));
  return (jlong)bits;
}

static void initialize_projected_submission_v1(
    GsplatSurfaceProjectedSubmissionV1 *submission) {
  memset(submission, 0, sizeof(*submission));
  submission->struct_size = (uint32_t)sizeof(*submission);
  submission->version = GSPLAT_SURFACE_PROJECTED_ABI_VERSION_V1;
}

static void initialize_projected_measurement_v1(
    GsplatSurfaceProjectedMeasurementV1 *measurement) {
  memset(measurement, 0, sizeof(*measurement));
  measurement->struct_size = (uint32_t)sizeof(*measurement);
  measurement->version = GSPLAT_SURFACE_PROJECTED_ABI_VERSION_V1;
}

static void initialize_projected_counts_v1(GsplatSurfaceProjectedCountsV1 *counts) {
  memset(counts, 0, sizeof(*counts));
  counts->struct_size = (uint32_t)sizeof(*counts);
  counts->version = GSPLAT_SURFACE_PROJECTED_ABI_VERSION_V1;
}

static void initialize_projected_failure_v1(GsplatSurfaceProjectedFailureV1 *failure) {
  memset(failure, 0, sizeof(*failure));
  failure->struct_size = (uint32_t)sizeof(*failure);
  failure->version = GSPLAT_SURFACE_PROJECTED_ABI_VERSION_V1;
}

static int projected_v1_header_matches(
    uint32_t struct_size,
    uint32_t version,
    size_t expected_size) {
  return struct_size == expected_size &&
      version == GSPLAT_SURFACE_PROJECTED_ABI_VERSION_V1;
}

static jlong create_surface_renderer(
    JNIEnv *env,
    jobject surface,
    jstring dataset_path,
    jint width,
    jint height,
    jint geometry_path,
    jintArray out_error) {
  set_out_error(env, out_error, GSPLAT_OK);

  if (surface == NULL || dataset_path == NULL || width <= 0 || height <= 0 ||
      geometry_path < GSPLAT_GEOMETRY_PATH_DIRECT ||
      geometry_path > GSPLAT_GEOMETRY_PATH_PAGED_ACTIVE_ATLAS) {
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
      "creating surface renderer width=%d height=%d geometry_path=%d dataset=%s",
      width,
      height,
      geometry_path,
      dataset);

  GsplatSurfaceRenderer *renderer = NULL;
  int32_t rc = gsplat_surface_renderer_create_android_with_geometry_path(
      (void *)window,
      dataset,
      (uint32_t)width,
      (uint32_t)height,
      (uint32_t)geometry_path,
      &renderer);

  (*env)->ReleaseStringUTFChars(env, dataset_path, dataset);

  if (rc != 0 || renderer == NULL) {
    set_out_error(env, out_error, rc == 0 ? GSPLAT_ERROR_INTERNAL : rc);
    __android_log_print(
        ANDROID_LOG_ERROR,
        GSPLAT_LOG_TAG,
        "gsplat_surface_renderer_create_android_with_geometry_path failed rc=%d renderer=%p",
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

JNIEXPORT jlong JNICALL Java_com_gsplat_android_NativeBridge_createSurfaceRenderer(
    JNIEnv *env,
    jclass cls,
    jobject surface,
    jstring dataset_path,
    jint width,
    jint height,
    jintArray out_error) {
  (void)cls;
  return create_surface_renderer(
      env,
      surface,
      dataset_path,
      width,
      height,
      GSPLAT_GEOMETRY_PATH_PACKED_ATLAS,
      out_error);
}

JNIEXPORT jlong JNICALL Java_com_gsplat_android_NativeBridge_createSurfaceRendererWithGeometryPath(
    JNIEnv *env,
    jclass cls,
    jobject surface,
    jstring dataset_path,
    jint width,
    jint height,
    jint geometry_path,
    jintArray out_error) {
  (void)cls;
  return create_surface_renderer(
      env,
      surface,
      dataset_path,
      width,
      height,
      geometry_path,
      out_error);
}

JNIEXPORT jint JNICALL Java_com_gsplat_android_NativeBridge_resizeSurfaceRenderer(
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

JNIEXPORT jint JNICALL Java_com_gsplat_android_NativeBridge_setSurfaceSortInterval(
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

JNIEXPORT jint JNICALL Java_com_gsplat_android_NativeBridge_setSurfaceOrderBackend(
    JNIEnv *env,
    jclass cls,
    jlong native_handle,
    jint backend) {
  (void)env;
  (void)cls;

  AndroidSurfaceRendererHandle *handle = android_handle_from_jlong(native_handle);
  if (handle == NULL || handle->renderer == NULL ||
      backend < GSPLAT_SURFACE_ORDER_BACKEND_CPU ||
      backend > GSPLAT_SURFACE_ORDER_BACKEND_ADAPTIVE) {
    return GSPLAT_ERROR_INVALID_ARGUMENT;
  }

  return gsplat_surface_renderer_set_order_backend(
      handle->renderer,
      (uint32_t)backend);
}

JNIEXPORT jint JNICALL Java_com_gsplat_android_NativeBridge_setSurfaceProjectedPolicyV1(
    JNIEnv *env,
    jclass cls,
    jlong native_handle,
    jint policy) {
  (void)env;
  (void)cls;

  AndroidSurfaceRendererHandle *handle = android_handle_from_jlong(native_handle);
  if (handle == NULL || handle->renderer == NULL ||
      policy < GSPLAT_SURFACE_PROJECTED_POLICY_CANDIDATE ||
      policy > GSPLAT_SURFACE_PROJECTED_POLICY_ADAPTIVE) {
    return GSPLAT_ERROR_INVALID_ARGUMENT;
  }

  return gsplat_surface_renderer_set_projected_policy_v1(
      handle->renderer,
      (uint32_t)policy);
}

JNIEXPORT jint JNICALL Java_com_gsplat_android_NativeBridge_setSurfaceGeometryPath(
    JNIEnv *env,
    jclass cls,
    jlong native_handle,
    jint path) {
  (void)env;
  (void)cls;

  AndroidSurfaceRendererHandle *handle = android_handle_from_jlong(native_handle);
  if (handle == NULL || handle->renderer == NULL) {
    return GSPLAT_ERROR_INVALID_ARGUMENT;
  }

  return gsplat_surface_renderer_set_geometry_path(handle->renderer, (uint32_t)path);
}

JNIEXPORT jint JNICALL Java_com_gsplat_android_NativeBridge_setSurfaceAsyncSortEnabled(
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

JNIEXPORT jint JNICALL Java_com_gsplat_example_BenchmarkBridge_setSurfaceOrderBackend(
    JNIEnv *env,
    jclass cls,
    jlong native_handle,
    jint backend) {
  (void)env;
  (void)cls;

  AndroidSurfaceRendererHandle *handle = android_handle_from_jlong(native_handle);
  if (handle == NULL || handle->renderer == NULL || backend < 0 || backend > 2) {
    return GSPLAT_ERROR_INVALID_ARGUMENT;
  }
  return gsplat_surface_renderer_set_order_backend(
      handle->renderer,
      (uint32_t)backend);
}

JNIEXPORT jint JNICALL Java_com_gsplat_example_BenchmarkBridge_setSurfaceCameraTraceFrame(
    JNIEnv *env,
    jclass cls,
    jlong native_handle,
    jstring trace_path,
    jint frame_index,
    jboolean require_trace_display_match) {
  (void)cls;

  AndroidSurfaceRendererHandle *handle = android_handle_from_jlong(native_handle);
  if (handle == NULL || handle->renderer == NULL || trace_path == NULL || frame_index < 0) {
    return GSPLAT_ERROR_INVALID_ARGUMENT;
  }
  const char *path = (*env)->GetStringUTFChars(env, trace_path, NULL);
  if (path == NULL) {
    return GSPLAT_ERROR_INTERNAL;
  }
  int32_t rc = gsplat_benchmark_set_surface_camera_trace_frame_with_display_policy(
      handle->renderer,
      path,
      (uint32_t)frame_index,
      require_trace_display_match == JNI_TRUE ? 1u : 0u);
  (*env)->ReleaseStringUTFChars(env, trace_path, path);
  return rc;
}

JNIEXPORT jint JNICALL Java_com_gsplat_example_BenchmarkBridge_getSurfaceCameraReceiptV1(
    JNIEnv *env,
    jclass cls,
    jlong native_handle,
    jlongArray out_receipt) {
  (void)cls;

  enum { CAMERA_RECEIPT_RAW_VALUE_COUNT = 63 };
  AndroidSurfaceRendererHandle *handle = android_handle_from_jlong(native_handle);
  if (handle == NULL || handle->renderer == NULL || out_receipt == NULL ||
      (*env)->GetArrayLength(env, out_receipt) < CAMERA_RECEIPT_RAW_VALUE_COUNT) {
    return GSPLAT_ERROR_INVALID_ARGUMENT;
  }

  GsplatSurfaceCameraReceiptV1 receipt;
  memset(&receipt, 0, sizeof(receipt));
  receipt.struct_size = (uint32_t)sizeof(receipt);
  receipt.version = GSPLAT_SURFACE_CAMERA_RECEIPT_ABI_VERSION_V1;
  int32_t rc = gsplat_surface_renderer_get_camera_receipt_v1(
      handle->renderer,
      &receipt);
  if (rc != GSPLAT_OK) {
    return rc;
  }
  if (receipt.struct_size != sizeof(receipt) ||
      receipt.version != GSPLAT_SURFACE_CAMERA_RECEIPT_ABI_VERSION_V1) {
    return GSPLAT_ERROR_INTERNAL;
  }

  jlong values[CAMERA_RECEIPT_RAW_VALUE_COUNT] = {0};
  values[0] = (jlong)receipt.camera_revision;
  values[1] = (jlong)receipt.presented_camera_revision;
  values[2] = (jlong)receipt.surface_width;
  values[3] = (jlong)receipt.surface_height;
  values[4] = (jlong)receipt.flags;
  size_t cursor = 5;
  for (size_t index = 0; index < 3; ++index) {
    values[cursor++] = float_bits_to_jlong(receipt.position[index]);
  }
  for (size_t index = 0; index < 4; ++index) {
    values[cursor++] = float_bits_to_jlong(receipt.rotation_xyzw[index]);
  }
  values[cursor++] = float_bits_to_jlong(receipt.vertical_fov_radians);
  values[cursor++] = float_bits_to_jlong(receipt.near_plane);
  values[cursor++] = float_bits_to_jlong(receipt.far_plane);
  for (size_t index = 0; index < 16; ++index) {
    values[cursor++] = float_bits_to_jlong(receipt.view_matrix[index]);
  }
  for (size_t index = 0; index < 16; ++index) {
    values[cursor++] = float_bits_to_jlong(receipt.projection_matrix[index]);
  }
  for (size_t index = 0; index < 16; ++index) {
    values[cursor++] = float_bits_to_jlong(receipt.view_projection_matrix[index]);
  }
  if (cursor != CAMERA_RECEIPT_RAW_VALUE_COUNT) {
    return GSPLAT_ERROR_INTERNAL;
  }
  (*env)->SetLongArrayRegion(
      env,
      out_receipt,
      0,
      CAMERA_RECEIPT_RAW_VALUE_COUNT,
      values);
  return (*env)->ExceptionCheck(env) ? GSPLAT_ERROR_INTERNAL : GSPLAT_OK;
}

JNIEXPORT jint JNICALL Java_com_gsplat_android_NativeBridge_setSurfaceFrameLatency(
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

JNIEXPORT jint JNICALL Java_com_gsplat_android_NativeBridge_resetSurfaceCamera(
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

JNIEXPORT jint JNICALL Java_com_gsplat_android_NativeBridge_orbitSurfaceRenderer(
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

JNIEXPORT jint JNICALL Java_com_gsplat_android_NativeBridge_zoomSurfaceRenderer(
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

JNIEXPORT jint JNICALL Java_com_gsplat_android_NativeBridge_panSurfaceRenderer(
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

JNIEXPORT jint JNICALL Java_com_gsplat_android_NativeBridge_renderSurfaceFrame(
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

JNIEXPORT jint JNICALL Java_com_gsplat_android_NativeBridge_getSurfaceStats(
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

JNIEXPORT jint JNICALL Java_com_gsplat_android_NativeBridge_getSurfaceExactness(
    JNIEnv *env,
    jclass cls,
    jlong native_handle,
    jlongArray out_exactness) {
  (void)cls;

  AndroidSurfaceRendererHandle *handle = android_handle_from_jlong(native_handle);
  if (handle == NULL || handle->renderer == NULL || out_exactness == NULL) {
    return GSPLAT_ERROR_INVALID_ARGUMENT;
  }
  if ((*env)->GetArrayLength(env, out_exactness) < 10) {
    return GSPLAT_ERROR_INVALID_ARGUMENT;
  }

  GsplatSurfaceExactness exactness;
  memset(&exactness, 0, sizeof(exactness));
  int32_t rc = gsplat_surface_renderer_get_exactness(handle->renderer, &exactness);
  if (rc != GSPLAT_OK) {
    return rc;
  }

  jlong values[10];
  values[0] = (jlong)exactness.source_splat_count;
  values[1] = (jlong)exactness.decoded_splat_count;
  values[2] = (jlong)exactness.encoded_splat_count;
  values[3] = (jlong)exactness.resident_splat_count;
  values[4] = (jlong)exactness.addressable_splat_count;
  values[5] = (jlong)exactness.source_sh_degree;
  values[6] = (jlong)exactness.resident_sh_degree;
  values[7] = (jlong)exactness.quality_flags;
  values[8] = (jlong)exactness.max_storage_buffers_per_shader_stage;
  values[9] = (jlong)exactness.max_storage_buffer_binding_size;
  (*env)->SetLongArrayRegion(env, out_exactness, 0, 10, values);
  return GSPLAT_OK;
}

JNIEXPORT jint JNICALL Java_com_gsplat_android_NativeBridge_getSurfacePresentation(
    JNIEnv *env,
    jclass cls,
    jlong native_handle,
    jlongArray out_presentation) {
  (void)cls;

  AndroidSurfaceRendererHandle *handle = android_handle_from_jlong(native_handle);
  if (handle == NULL || handle->renderer == NULL || out_presentation == NULL) {
    return GSPLAT_ERROR_INVALID_ARGUMENT;
  }
  if ((*env)->GetArrayLength(env, out_presentation) < 11) {
    return GSPLAT_ERROR_INVALID_ARGUMENT;
  }

  GsplatSurfacePresentation presentation;
  memset(&presentation, 0, sizeof(presentation));
  int32_t rc = gsplat_surface_renderer_get_presentation(handle->renderer, &presentation);
  if (rc != GSPLAT_OK) {
    return rc;
  }

  jlong values[11];
  values[0] = (jlong)presentation.requested_width;
  values[1] = (jlong)presentation.requested_height;
  values[2] = (jlong)presentation.surface_width;
  values[3] = (jlong)presentation.surface_height;
  values[4] = (jlong)presentation.internal_render_width;
  values[5] = (jlong)presentation.internal_render_height;
  values[6] = (jlong)presentation.presented_width;
  values[7] = (jlong)presentation.presented_height;
  values[8] = (jlong)presentation.presented_camera_revision;
  values[9] = (jlong)presentation.flags;
  values[10] = (jlong)presentation.reserved;
  (*env)->SetLongArrayRegion(env, out_presentation, 0, 11, values);
  return GSPLAT_OK;
}

JNIEXPORT jint JNICALL Java_com_gsplat_android_NativeBridge_getSurfaceSortStats(
    JNIEnv *env,
    jclass cls,
    jlong native_handle,
    jlongArray out_stats) {
  (void)cls;

  AndroidSurfaceRendererHandle *handle = android_handle_from_jlong(native_handle);
  if (handle == NULL || handle->renderer == NULL || out_stats == NULL) {
    return 1;
  }
  if ((*env)->GetArrayLength(env, out_stats) < 7) {
    return 1;
  }

  GsplatSurfaceSortStats stats;
  memset(&stats, 0, sizeof(stats));
  int32_t rc = gsplat_surface_renderer_get_sort_stats(handle->renderer, &stats);
  if (rc != 0) {
    return rc;
  }

  jlong values[7];
  values[0] = (jlong)stats.camera_revision;
  values[1] = (jlong)stats.applied_order_revision;
  values[2] = (jlong)stats.scheduled_revision;
  values[3] = (jlong)stats.completed_revision;
  values[4] = (jlong)stats.presented_order_revision_lag;
  values[5] = (jlong)stats.observed_result_revision_lag;
  values[6] = (jlong)stats.flags;
  (*env)->SetLongArrayRegion(env, out_stats, 0, 7, values);
  return 0;
}

JNIEXPORT jint JNICALL Java_com_gsplat_android_NativeBridge_getSurfaceOrderSubmission(
    JNIEnv *env,
    jclass cls,
    jlong native_handle,
    jlongArray out_submission) {
  (void)cls;

  AndroidSurfaceRendererHandle *handle = android_handle_from_jlong(native_handle);
  if (handle == NULL || handle->renderer == NULL || out_submission == NULL) {
    return GSPLAT_ERROR_INVALID_ARGUMENT;
  }
  if ((*env)->GetArrayLength(env, out_submission) < 6) {
    return GSPLAT_ERROR_INVALID_ARGUMENT;
  }

  GsplatSurfaceOrderSubmission submission;
  memset(&submission, 0, sizeof(submission));
  int32_t rc = gsplat_surface_renderer_get_order_submission(handle->renderer, &submission);
  if (rc != GSPLAT_OK) {
    return rc;
  }

  jlong values[6];
  values[0] = (jlong)submission.ticket;
  values[1] = (jlong)submission.camera_revision;
  values[2] = (jlong)submission.requested_backend;
  values[3] = (jlong)submission.actual_backend;
  values[4] = (jlong)submission.adaptive_state;
  values[5] = (jlong)submission.flags;
  (*env)->SetLongArrayRegion(env, out_submission, 0, 6, values);
  return GSPLAT_OK;
}

JNIEXPORT jint JNICALL Java_com_gsplat_android_NativeBridge_getSurfaceProjectedSubmissionV1(
    JNIEnv *env,
    jclass cls,
    jlong native_handle,
    jlongArray out_submission) {
  (void)cls;

  AndroidSurfaceRendererHandle *handle = android_handle_from_jlong(native_handle);
  if (handle == NULL || handle->renderer == NULL || out_submission == NULL ||
      (*env)->GetArrayLength(env, out_submission) < 7) {
    return GSPLAT_ERROR_INVALID_ARGUMENT;
  }

  GsplatSurfaceProjectedSubmissionV1 submission;
  initialize_projected_submission_v1(&submission);
  int32_t rc = gsplat_surface_renderer_get_projected_submission_v1(
      handle->renderer,
      &submission);
  if (rc != GSPLAT_OK) {
    return rc;
  }
  if (!projected_v1_header_matches(
          submission.struct_size,
          submission.version,
          sizeof(submission)) ||
      submission.requested_policy < GSPLAT_SURFACE_PROJECTED_POLICY_CANDIDATE ||
      submission.requested_policy > GSPLAT_SURFACE_PROJECTED_POLICY_ADAPTIVE ||
      submission.actual_execution < GSPLAT_SURFACE_PROJECTED_EXECUTION_CANDIDATE ||
      submission.actual_execution > GSPLAT_SURFACE_PROJECTED_EXECUTION_COMPACT ||
      submission.order_backend > GSPLAT_SURFACE_ORDER_BACKEND_GPU ||
      submission.adaptive_state > GSPLAT_SURFACE_PROJECTED_ADAPTIVE_STATE_CANDIDATE_ONLY ||
      submission.reserved != 0) {
    __android_log_print(
        ANDROID_LOG_ERROR,
        GSPLAT_LOG_TAG,
        "invalid projected submission v1 receipt");
    return GSPLAT_ERROR_INTERNAL;
  }

  const uint32_t ticket_issued =
      submission.flags & GSPLAT_SURFACE_PROJECTED_SUBMISSION_TICKET_ISSUED;
  const uint32_t unsampled_ring =
      submission.flags & GSPLAT_SURFACE_PROJECTED_SUBMISSION_UNSAMPLED_RING_BUSY;
  const uint32_t unsampled_surface =
      submission.flags & GSPLAT_SURFACE_PROJECTED_SUBMISSION_UNSAMPLED_SURFACE_UNAVAILABLE;
  const uint32_t known_flags =
      GSPLAT_SURFACE_PROJECTED_SUBMISSION_TICKET_ISSUED |
      GSPLAT_SURFACE_PROJECTED_SUBMISSION_UNSAMPLED_RING_BUSY |
      GSPLAT_SURFACE_PROJECTED_SUBMISSION_UNSAMPLED_SURFACE_UNAVAILABLE;
  if ((submission.flags & ~known_flags) != 0 ||
      (unsampled_ring != 0 && unsampled_surface != 0) ||
      (ticket_issued != 0 &&
       (submission.ticket == 0 || unsampled_ring != 0 || unsampled_surface != 0)) ||
      (ticket_issued == 0 && submission.ticket != 0) ||
      (submission.requested_policy != GSPLAT_SURFACE_PROJECTED_POLICY_ADAPTIVE &&
       (ticket_issued != 0 || unsampled_ring != 0 || unsampled_surface != 0))) {
    __android_log_print(
        ANDROID_LOG_ERROR,
        GSPLAT_LOG_TAG,
        "invalid projected submission ticket semantics policy=%u ticket=%llu flags=%u",
        submission.requested_policy,
        (unsigned long long)submission.ticket,
        submission.flags);
    return GSPLAT_ERROR_INTERNAL;
  }

  jlong values[7];
  values[0] = (jlong)submission.ticket;
  values[1] = (jlong)submission.camera_revision;
  values[2] = (jlong)submission.requested_policy;
  values[3] = (jlong)submission.actual_execution;
  values[4] = (jlong)submission.order_backend;
  values[5] = (jlong)submission.adaptive_state;
  values[6] = (jlong)submission.flags;
  (*env)->SetLongArrayRegion(env, out_submission, 0, 7, values);
  return GSPLAT_OK;
}

JNIEXPORT jint JNICALL Java_com_gsplat_android_NativeBridge_pollSurfaceProjectedMeasurementV1(
    JNIEnv *env,
    jclass cls,
    jlong native_handle,
    jlongArray out_measurement) {
  (void)cls;

  AndroidSurfaceRendererHandle *handle = android_handle_from_jlong(native_handle);
  if (handle == NULL || handle->renderer == NULL || out_measurement == NULL ||
      (*env)->GetArrayLength(env, out_measurement) < 13) {
    return GSPLAT_ERROR_INVALID_ARGUMENT;
  }

  GsplatSurfaceProjectedMeasurementV1 measurement;
  initialize_projected_measurement_v1(&measurement);
  uint32_t available = 0;
  int32_t rc = gsplat_surface_renderer_poll_projected_measurement_v1(
      handle->renderer,
      &measurement,
      &available);
  if (rc != GSPLAT_OK) {
    return rc;
  }
  if (available > 1 || !projected_v1_header_matches(
          measurement.struct_size,
          measurement.version,
          sizeof(measurement))) {
    return GSPLAT_ERROR_INTERNAL;
  }

  GsplatSurfaceProjectedCountsV1 counts;
  initialize_projected_counts_v1(&counts);
  if (available != 0) {
    if (measurement.ticket == 0 || !isfinite(measurement.frame_complete_ms) ||
        measurement.frame_complete_ms < 0.0f ||
        measurement.execution < GSPLAT_SURFACE_PROJECTED_EXECUTION_CANDIDATE ||
        measurement.execution > GSPLAT_SURFACE_PROJECTED_EXECUTION_COMPACT ||
        measurement.order_backend > GSPLAT_SURFACE_ORDER_BACKEND_GPU) {
      return GSPLAT_ERROR_INTERNAL;
    }

    uint32_t counts_available = 0;
    rc = gsplat_surface_renderer_take_projected_counts_v1(
        handle->renderer,
        measurement.ticket,
        &counts,
        &counts_available);
    if (rc != GSPLAT_OK) {
      return rc;
    }
    if (counts_available != 1 || !projected_v1_header_matches(
            counts.struct_size,
            counts.version,
            sizeof(counts)) ||
        counts.ticket != measurement.ticket ||
        counts.camera_revision != measurement.camera_revision) {
      __android_log_print(
          ANDROID_LOG_ERROR,
          GSPLAT_LOG_TAG,
          "projected success lacks matching V/C/D counts ticket=%llu revision=%llu",
          (unsigned long long)measurement.ticket,
          (unsigned long long)measurement.camera_revision);
      return GSPLAT_ERROR_INTERNAL;
    }

    const uint32_t measurement_exact =
        measurement.flags & GSPLAT_SURFACE_PROJECTED_MEASUREMENT_EXACT_CONTRIBUTOR_DRAW;
    const uint32_t counts_exact =
        counts.flags & GSPLAT_SURFACE_PROJECTED_COUNTS_EXACT_CONTRIBUTOR_DRAW;
    const uint32_t known_measurement_flags =
        GSPLAT_SURFACE_PROJECTED_MEASUREMENT_PROJECTION_REBUILT |
        GSPLAT_SURFACE_PROJECTED_MEASUREMENT_ORDER_REFRESHED |
        GSPLAT_SURFACE_PROJECTED_MEASUREMENT_EXACT_CONTRIBUTOR_DRAW |
        GSPLAT_SURFACE_PROJECTED_MEASUREMENT_DROPPED_PRIOR;
    if ((measurement.flags & ~known_measurement_flags) != 0 ||
        (counts.flags & ~GSPLAT_SURFACE_PROJECTED_COUNTS_EXACT_CONTRIBUTOR_DRAW) != 0 ||
        (measurement_exact != 0) != (counts_exact != 0) ||
        counts.contributor_count > counts.visible_count ||
        (measurement.execution == GSPLAT_SURFACE_PROJECTED_EXECUTION_CANDIDATE &&
         (measurement_exact != 0 || counts.drawn_count != counts.visible_count)) ||
        (measurement.execution == GSPLAT_SURFACE_PROJECTED_EXECUTION_COMPACT &&
         (measurement_exact == 0 || counts.drawn_count != counts.contributor_count))) {
      __android_log_print(
          ANDROID_LOG_ERROR,
          GSPLAT_LOG_TAG,
          "projected V/C/D invariant failed ticket=%llu V=%u C=%u D=%u execution=%u",
          (unsigned long long)measurement.ticket,
          counts.visible_count,
          counts.contributor_count,
          counts.drawn_count,
          measurement.execution);
      return GSPLAT_ERROR_INTERNAL;
    }
  }

  jlong values[13];
  values[0] = (jlong)available;
  values[1] = (jlong)measurement.ticket;
  values[2] = (jlong)measurement.camera_revision;
  values[3] = (jlong)measurement.projection_generation;
  values[4] = (jlong)measurement.probe_generation;
  values[5] = float_bits_to_jlong(measurement.frame_complete_ms);
  values[6] = (jlong)measurement.execution;
  values[7] = (jlong)measurement.order_backend;
  values[8] = (jlong)measurement.flags;
  values[9] = (jlong)counts.visible_count;
  values[10] = (jlong)counts.contributor_count;
  values[11] = (jlong)counts.drawn_count;
  values[12] = (jlong)counts.flags;
  (*env)->SetLongArrayRegion(env, out_measurement, 0, 13, values);
  return GSPLAT_OK;
}

JNIEXPORT jint JNICALL Java_com_gsplat_android_NativeBridge_pollSurfaceProjectedFailureV1(
    JNIEnv *env,
    jclass cls,
    jlong native_handle,
    jlongArray out_failure) {
  (void)cls;

  AndroidSurfaceRendererHandle *handle = android_handle_from_jlong(native_handle);
  if (handle == NULL || handle->renderer == NULL || out_failure == NULL ||
      (*env)->GetArrayLength(env, out_failure) < 9) {
    return GSPLAT_ERROR_INVALID_ARGUMENT;
  }

  GsplatSurfaceProjectedFailureV1 failure;
  initialize_projected_failure_v1(&failure);
  uint32_t available = 0;
  int32_t rc = gsplat_surface_renderer_poll_projected_failure_v1(
      handle->renderer,
      &failure,
      &available);
  if (rc != GSPLAT_OK) {
    return rc;
  }
  if (available > 1 || !projected_v1_header_matches(
          failure.struct_size,
          failure.version,
          sizeof(failure)) ||
      (available != 0 &&
       (failure.ticket == 0 ||
        failure.reason < GSPLAT_SURFACE_PROJECTED_FAILURE_READBACK_MAP ||
        failure.reason > GSPLAT_SURFACE_PROJECTED_FAILURE_INVARIANT_VIOLATION ||
        failure.execution < GSPLAT_SURFACE_PROJECTED_EXECUTION_CANDIDATE ||
        failure.execution > GSPLAT_SURFACE_PROJECTED_EXECUTION_COMPACT ||
        failure.order_backend > GSPLAT_SURFACE_ORDER_BACKEND_GPU ||
        (failure.flags & ~GSPLAT_SURFACE_PROJECTED_FAILURE_DROPPED_PRIOR) != 0))) {
    return GSPLAT_ERROR_INTERNAL;
  }

  jlong values[9];
  values[0] = (jlong)available;
  values[1] = (jlong)failure.ticket;
  values[2] = (jlong)failure.camera_revision;
  values[3] = (jlong)failure.projection_generation;
  values[4] = (jlong)failure.probe_generation;
  values[5] = (jlong)failure.reason;
  values[6] = (jlong)failure.execution;
  values[7] = (jlong)failure.order_backend;
  values[8] = (jlong)failure.flags;
  (*env)->SetLongArrayRegion(env, out_failure, 0, 9, values);
  return GSPLAT_OK;
}

JNIEXPORT jint JNICALL Java_com_gsplat_android_NativeBridge_pollSurfaceOrderMeasurement(
    JNIEnv *env,
    jclass cls,
    jlong native_handle,
    jlongArray out_measurement) {
  (void)cls;

  AndroidSurfaceRendererHandle *handle = android_handle_from_jlong(native_handle);
  if (handle == NULL || handle->renderer == NULL || out_measurement == NULL) {
    return GSPLAT_ERROR_INVALID_ARGUMENT;
  }
  if ((*env)->GetArrayLength(env, out_measurement) < 19) {
    return GSPLAT_ERROR_INVALID_ARGUMENT;
  }

  GsplatSurfaceOrderMeasurement measurement;
  memset(&measurement, 0, sizeof(measurement));
  uint32_t available = 0;
  int32_t rc = gsplat_surface_renderer_poll_order_measurement(
      handle->renderer,
      &measurement,
      &available);
  if (rc != GSPLAT_OK) {
    return rc;
  }

  GsplatSurfaceOrderCounts counts;
  memset(&counts, 0, sizeof(counts));
  if (available != 0) {
    uint32_t counts_available = 0;
    rc = gsplat_surface_renderer_take_order_counts(
        handle->renderer,
        measurement.ticket,
        &counts,
        &counts_available);
    if (rc != GSPLAT_OK) {
      return rc;
    }
    if (counts_available == 0 || counts.ticket != measurement.ticket ||
        counts.camera_revision != measurement.camera_revision) {
      __android_log_print(
          ANDROID_LOG_ERROR,
          GSPLAT_LOG_TAG,
          "GPU order receipt lacks matching V/C/D counts ticket=%llu revision=%llu",
          (unsigned long long)measurement.ticket,
          (unsigned long long)measurement.camera_revision);
      return GSPLAT_ERROR_INTERNAL;
    }
  }

  jlong values[19];
  values[0] = (jlong)available;
  values[1] = (jlong)measurement.ticket;
  values[2] = (jlong)measurement.camera_revision;
  values[3] = float_bits_to_jlong(measurement.gpu_preprocess_ms);
  values[4] = float_bits_to_jlong(measurement.gpu_radix_ms);
  values[5] = float_bits_to_jlong(measurement.gpu_order_ms);
  values[6] = float_bits_to_jlong(measurement.gpu_complete_ms);
  values[7] = float_bits_to_jlong(measurement.timestamp_period_ns);
  values[8] = (jlong)measurement.visible_count;
  values[9] = (jlong)measurement.drawn_count;
  values[10] = (jlong)measurement.timing_source;
  values[11] = (jlong)measurement.requested_backend;
  values[12] = (jlong)measurement.actual_backend;
  values[13] = (jlong)measurement.adaptive_state;
  values[14] = (jlong)measurement.flags;
  values[15] = (jlong)counts.visible_count;
  values[16] = (jlong)counts.contributor_count;
  values[17] = (jlong)counts.drawn_count;
  values[18] = (jlong)counts.flags;
  (*env)->SetLongArrayRegion(env, out_measurement, 0, 19, values);
  return GSPLAT_OK;
}

JNIEXPORT jint JNICALL Java_com_gsplat_android_NativeBridge_pollSurfaceCpuOrderMeasurement(
    JNIEnv *env,
    jclass cls,
    jlong native_handle,
    jlongArray out_measurement) {
  (void)cls;

  AndroidSurfaceRendererHandle *handle = android_handle_from_jlong(native_handle);
  if (handle == NULL || handle->renderer == NULL || out_measurement == NULL) {
    return GSPLAT_ERROR_INVALID_ARGUMENT;
  }
  if ((*env)->GetArrayLength(env, out_measurement) < 15) {
    return GSPLAT_ERROR_INVALID_ARGUMENT;
  }

  GsplatSurfaceCpuOrderMeasurement measurement;
  memset(&measurement, 0, sizeof(measurement));
  uint32_t available = 0;
  int32_t rc = gsplat_surface_renderer_poll_cpu_order_measurement(
      handle->renderer,
      &measurement,
      &available);
  if (rc != GSPLAT_OK) {
    return rc;
  }

  GsplatSurfaceOrderCounts counts;
  memset(&counts, 0, sizeof(counts));
  if (available != 0) {
    uint32_t counts_available = 0;
    rc = gsplat_surface_renderer_take_order_counts(
        handle->renderer,
        measurement.ticket,
        &counts,
        &counts_available);
    if (rc != GSPLAT_OK) {
      return rc;
    }
    if (counts_available == 0 || counts.ticket != measurement.ticket ||
        counts.camera_revision != measurement.camera_revision) {
      __android_log_print(
          ANDROID_LOG_ERROR,
          GSPLAT_LOG_TAG,
          "CPU order receipt lacks matching V/C/D counts ticket=%llu revision=%llu",
          (unsigned long long)measurement.ticket,
          (unsigned long long)measurement.camera_revision);
      return GSPLAT_ERROR_INTERNAL;
    }
  }

  jlong values[15];
  values[0] = (jlong)available;
  values[1] = (jlong)measurement.ticket;
  values[2] = (jlong)measurement.camera_revision;
  values[3] = float_bits_to_jlong(measurement.preprocess_ms);
  values[4] = float_bits_to_jlong(measurement.sort_ms);
  values[5] = float_bits_to_jlong(measurement.frame_complete_ms);
  values[6] = (jlong)measurement.requested_backend;
  values[7] = (jlong)measurement.actual_backend;
  values[8] = (jlong)measurement.adaptive_state;
  values[9] = (jlong)measurement.flags;
  values[10] = (jlong)measurement.reserved;
  values[11] = (jlong)counts.visible_count;
  values[12] = (jlong)counts.contributor_count;
  values[13] = (jlong)counts.drawn_count;
  values[14] = (jlong)counts.flags;
  (*env)->SetLongArrayRegion(env, out_measurement, 0, 15, values);
  return GSPLAT_OK;
}

JNIEXPORT jint JNICALL Java_com_gsplat_android_NativeBridge_pollSurfaceOrderMeasurementFailure(
    JNIEnv *env,
    jclass cls,
    jlong native_handle,
    jlongArray out_failure) {
  (void)cls;

  AndroidSurfaceRendererHandle *handle = android_handle_from_jlong(native_handle);
  if (handle == NULL || handle->renderer == NULL || out_failure == NULL) {
    return GSPLAT_ERROR_INVALID_ARGUMENT;
  }
  if ((*env)->GetArrayLength(env, out_failure) < 8) {
    return GSPLAT_ERROR_INVALID_ARGUMENT;
  }

  GsplatSurfaceOrderMeasurementFailure failure;
  memset(&failure, 0, sizeof(failure));
  uint32_t available = 0;
  int32_t rc = gsplat_surface_renderer_poll_order_measurement_failure(
      handle->renderer,
      &failure,
      &available);
  if (rc != GSPLAT_OK) {
    return rc;
  }

  jlong values[8];
  values[0] = (jlong)available;
  values[1] = (jlong)failure.ticket;
  values[2] = (jlong)failure.camera_revision;
  values[3] = (jlong)failure.reason;
  values[4] = (jlong)failure.requested_backend;
  values[5] = (jlong)failure.actual_backend;
  values[6] = (jlong)failure.adaptive_state;
  values[7] = (jlong)failure.flags;
  (*env)->SetLongArrayRegion(env, out_failure, 0, 8, values);
  return GSPLAT_OK;
}

JNIEXPORT void JNICALL Java_com_gsplat_android_NativeBridge_destroySurfaceRenderer(
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
