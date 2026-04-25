#ifndef GSPLAT_FFI_C_GSPLAT_H
#define GSPLAT_FFI_C_GSPLAT_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/*
 * gsplat-rs v0.1 native integration surface.
 *
 * All functions returning int32_t use GsplatErrorCode values. Handles are
 * opaque, owned by the caller after successful create, and must be released by
 * their matching destroy function. Destroy functions accept NULL.
 */

#define GSPLAT_API_VERSION_MAJOR_VALUE 0
#define GSPLAT_API_VERSION_MINOR_VALUE 1

typedef enum GsplatErrorCode {
  GSPLAT_OK = 0,
  GSPLAT_ERROR_INVALID_ARGUMENT = 1,
  GSPLAT_ERROR_NOT_FOUND = 2,
  GSPLAT_ERROR_PARSE_FAILED = 3,
  GSPLAT_ERROR_UNSUPPORTED = 4,
  GSPLAT_ERROR_SCENE_NOT_LOADED = 5,
  GSPLAT_ERROR_INTERNAL = 100,
} GsplatErrorCode;

typedef enum GsplatRenderMode {
  /* The only release-gated render path in v0.1. */
  GSPLAT_RENDER_MODE_SORTED_ALPHA = 0,
} GsplatRenderMode;

typedef struct GsplatConfig {
  uint32_t width;
  uint32_t height;
  /* GsplatRenderMode value. Only GSPLAT_RENDER_MODE_SORTED_ALPHA is stable. */
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
typedef struct GsplatSurfaceRenderer GsplatSurfaceRenderer;

uint32_t gsplat_version_major(void);
uint32_t gsplat_version_minor(void);
const char *gsplat_error_message(int32_t code);

GsplatConfig gsplat_config_default(void);
GsplatCamera gsplat_camera_default(void);

int32_t gsplat_context_create(GsplatConfig config, GsplatContext **out_ctx);
void gsplat_context_destroy(GsplatContext *ctx);
int32_t gsplat_context_set_camera(GsplatContext *ctx, GsplatCamera camera);
int32_t gsplat_context_set_auto_camera(GsplatContext *ctx);
int32_t gsplat_context_load_scene_path(GsplatContext *ctx, const char *path);
int32_t gsplat_context_render_frame(GsplatContext *ctx);
int32_t gsplat_context_get_stats(const GsplatContext *ctx, GsplatStats *out_stats);

int32_t gsplat_surface_renderer_create_android(
    void *native_window,
    const char *path,
    uint32_t width,
    uint32_t height,
    GsplatSurfaceRenderer **out_renderer);
void gsplat_surface_renderer_destroy(GsplatSurfaceRenderer *renderer);
int32_t gsplat_surface_renderer_resize(
    GsplatSurfaceRenderer *renderer,
    uint32_t width,
    uint32_t height);
int32_t gsplat_surface_renderer_set_sort_interval(
    GsplatSurfaceRenderer *renderer,
    uint32_t interval);
int32_t gsplat_surface_renderer_set_gpu_preproject(
    GsplatSurfaceRenderer *renderer,
    uint32_t enabled);
int32_t gsplat_surface_renderer_set_gpu_preproject_double_buffer(
    GsplatSurfaceRenderer *renderer,
    uint32_t enabled);
int32_t gsplat_surface_renderer_set_async_sort(
    GsplatSurfaceRenderer *renderer,
    uint32_t enabled);
int32_t gsplat_surface_renderer_set_async_geometry(
    GsplatSurfaceRenderer *renderer,
    uint32_t enabled);
int32_t gsplat_surface_renderer_set_instance_buffer_count(
    GsplatSurfaceRenderer *renderer,
    uint32_t count);
int32_t gsplat_surface_renderer_set_frame_latency(
    GsplatSurfaceRenderer *renderer,
    uint32_t latency);
int32_t gsplat_surface_renderer_reset_camera(GsplatSurfaceRenderer *renderer);
int32_t gsplat_surface_renderer_orbit(
    GsplatSurfaceRenderer *renderer,
    float delta_yaw_radians,
    float delta_pitch_radians);
int32_t gsplat_surface_renderer_zoom(
    GsplatSurfaceRenderer *renderer,
    float distance_scale);
int32_t gsplat_surface_renderer_pan(
    GsplatSurfaceRenderer *renderer,
    float normalized_delta_x,
    float normalized_delta_y);
int32_t gsplat_surface_renderer_render_frame(GsplatSurfaceRenderer *renderer);
int32_t gsplat_surface_renderer_get_stats(
    const GsplatSurfaceRenderer *renderer,
    GsplatStats *out_stats);

#ifdef __cplusplus
}
#endif

#endif  // GSPLAT_FFI_C_GSPLAT_H
