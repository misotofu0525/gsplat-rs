#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "../../crates/gsplat-ffi-c/include/gsplat.h"

static int32_t load_scene_bytes_from_path(GsplatContext *ctx, const char *path) {
  FILE *file = fopen(path, "rb");
  if (file == NULL) {
    return -1;
  }
  if (fseek(file, 0, SEEK_END) != 0) {
    fclose(file);
    return -1;
  }
  long nbytes = ftell(file);
  if (nbytes <= 0) {
    fclose(file);
    return -1;
  }
  rewind(file);
  uint8_t *bytes = (uint8_t *)malloc((size_t)nbytes);
  if (bytes == NULL) {
    fclose(file);
    return -1;
  }
  size_t readn = fread(bytes, 1, (size_t)nbytes, file);
  fclose(file);
  if (readn != (size_t)nbytes) {
    free(bytes);
    return -1;
  }
  int32_t rc = gsplat_context_load_scene_bytes(ctx, bytes, (uint64_t)nbytes);
  free(bytes);
  return rc;
}

int main(int argc, char **argv) {
  const char *dataset = "tests/datasets/minimal_ascii.ply";
  if (argc > 1) {
    dataset = argv[1];
  }

  if (gsplat_version_major() != GSPLAT_API_VERSION_MAJOR_VALUE ||
      gsplat_version_minor() != GSPLAT_API_VERSION_MINOR_VALUE) {
    fprintf(stderr, "unexpected gsplat ABI version: %u.%u\n", gsplat_version_major(), gsplat_version_minor());
    return 2;
  }

  GsplatConfig unsupported_config = gsplat_config_default();
  unsupported_config.mode = GSPLAT_RENDER_MODE_SORTED_ALPHA + 1;
  GsplatContext *unsupported_ctx = NULL;
  int32_t rc = gsplat_context_create(unsupported_config, &unsupported_ctx);
  if (rc != GSPLAT_ERROR_INVALID_ARGUMENT || unsupported_ctx != NULL) {
    fprintf(stderr, "expected unsupported render mode to fail with InvalidArgument, got rc=%d ctx=%p\n", rc, (void *)unsupported_ctx);
    if (unsupported_ctx != NULL) {
      gsplat_context_destroy(unsupported_ctx);
    }
    return 8;
  }

  GsplatConfig config = gsplat_config_default();
  GsplatContext *ctx = NULL;

  rc = gsplat_context_create(config, &ctx);
  if (rc != 0 || ctx == NULL) {
    fprintf(stderr, "gsplat_context_create failed: %s (%d)\n", gsplat_error_message(rc), rc);
    return 3;
  }

  GsplatCamera camera = gsplat_camera_default();

  GsplatCamera invalid_camera = camera;
  invalid_camera.near_plane = 10.0f;
  invalid_camera.far_plane = 1.0f;
  rc = gsplat_context_set_camera(ctx, invalid_camera);
  if (rc != GSPLAT_ERROR_INVALID_ARGUMENT) {
    fprintf(stderr, "expected invalid camera to fail with InvalidArgument, got: %d\n", rc);
    gsplat_context_destroy(ctx);
    return 9;
  }

  rc = gsplat_context_set_camera(ctx, camera);
  if (rc != 0) {
    fprintf(stderr, "gsplat_context_set_camera failed: %s (%d)\n", gsplat_error_message(rc), rc);
    gsplat_context_destroy(ctx);
    return 4;
  }

  rc = gsplat_context_load_scene_path(ctx, dataset);
  if (rc != 0) {
    fprintf(stderr, "gsplat_context_load_scene_path failed (%s): %s (%d)\n", dataset, gsplat_error_message(rc), rc);
    gsplat_context_destroy(ctx);
    return 5;
  }

  rc = load_scene_bytes_from_path(ctx, dataset);
  if (rc != 0) {
    fprintf(stderr, "gsplat_context_load_scene_bytes failed (%s): %s (%d)\n", dataset, gsplat_error_message(rc), rc);
    gsplat_context_destroy(ctx);
    return 10;
  }

  rc = gsplat_context_render_frame(ctx);
  if (rc != 0) {
    fprintf(stderr, "gsplat_context_render_frame failed: %s (%d)\n", gsplat_error_message(rc), rc);
    gsplat_context_destroy(ctx);
    return 6;
  }

  GsplatStats stats;
  memset(&stats, 0, sizeof(stats));
  rc = gsplat_context_get_stats(ctx, &stats);
  if (rc != 0) {
    fprintf(stderr, "gsplat_context_get_stats failed: %s (%d)\n", gsplat_error_message(rc), rc);
    gsplat_context_destroy(ctx);
    return 7;
  }

  printf("ffi smoke ok\n");
  printf("drawn=%u visible=%u frame_ms=%.4f\n", stats.drawn_count, stats.visible_count, stats.frame_ms);

  gsplat_context_destroy(ctx);
  return 0;
}
