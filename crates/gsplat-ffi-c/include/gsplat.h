#ifndef GSPLAT_FFI_C_GSPLAT_H
#define GSPLAT_FFI_C_GSPLAT_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct GsplatConfig {
  uint32_t width;
  uint32_t height;
  uint32_t mode;
} GsplatConfig;

typedef struct GsplatStats {
  float frame_ms;
  float preprocess_ms;
  float sort_ms;
  float raster_ms;
  uint32_t visible_count;
  uint32_t drawn_count;
} GsplatStats;

typedef struct GsplatCamera {
  float position[3];
  float rotation_xyzw[4];
  float vertical_fov_radians;
  float near_plane;
  float far_plane;
} GsplatCamera;

typedef struct GsplatContext GsplatContext;

uint32_t gsplat_version_major(void);
uint32_t gsplat_version_minor(void);

int32_t gsplat_context_create(GsplatConfig config, GsplatContext **out_ctx);
void gsplat_context_destroy(GsplatContext *ctx);
int32_t gsplat_context_set_camera(GsplatContext *ctx, GsplatCamera camera);
int32_t gsplat_context_load_scene_path(GsplatContext *ctx, const char *path);
int32_t gsplat_context_render_frame(GsplatContext *ctx);
int32_t gsplat_context_get_stats(const GsplatContext *ctx, GsplatStats *out_stats);

#ifdef __cplusplus
}
#endif

#endif  // GSPLAT_FFI_C_GSPLAT_H
