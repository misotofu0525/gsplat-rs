#include <jni.h>
#include <string.h>

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
