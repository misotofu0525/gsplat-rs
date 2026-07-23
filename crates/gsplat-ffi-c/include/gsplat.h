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
 * their matching destroy function. Destroy functions accept NULL. Handles are
 * single-thread confined: call render, resize, camera, stats, option, and
 * destroy functions from one serialized owner thread or queue.
 *
 * `gsplat_context_*` is the small stable v0.1 C ABI. `gsplat_surface_renderer_*`
 * exists to validate Android/iOS realtime Surface integration and is not a full
 * mobile product SDK. Surface A/B option setters are experimental benchmark
 * knobs and may change before a published mobile SDK.
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

/*
 * Surface geometry selection. Product wrappers default to the exact packed
 * resident path; direct remains the wide-float oracle, and paged is a
 * diagnostic path that is not full-resident rendering.
 */
typedef enum GsplatGeometryPath {
  GSPLAT_GEOMETRY_PATH_DIRECT = 0,
  GSPLAT_GEOMETRY_PATH_PACKED_ATLAS = 1,
  GSPLAT_GEOMETRY_PATH_PAGED_ACTIVE_ATLAS = 2,
} GsplatGeometryPath;

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

typedef enum GsplatSurfaceAdaptiveGpuFailureReason {
  GSPLAT_SURFACE_ADAPTIVE_GPU_FAILURE_UNSUPPORTED = 1,
  GSPLAT_SURFACE_ADAPTIVE_GPU_FAILURE_INITIALIZATION = 2,
  GSPLAT_SURFACE_ADAPTIVE_GPU_FAILURE_OUT_OF_MEMORY = 3,
  GSPLAT_SURFACE_ADAPTIVE_GPU_FAILURE_VALIDATION = 4,
} GsplatSurfaceAdaptiveGpuFailureReason;

/* Additive telemetry for the experimental bounded async Surface sort path. */
typedef struct GsplatSurfaceSortStats {
  uint64_t camera_revision;
  uint64_t applied_order_revision;
  uint64_t scheduled_revision;
  uint64_t completed_revision;
  uint32_t presented_order_revision_lag;
  uint32_t observed_result_revision_lag;
  /* bits: 0 refreshed, 1 uploaded, 2 scheduled, 3 scheduled revision valid,
   * 4 completed revision valid, 5 result applied, 6 stale dropped,
   * 7 sync fallback, 8 observed result lag valid, 9-10 actual order backend
   * (0 CPU, 1 GPU), 11 GPU-to-CPU fallback, 12-14 adaptive state
   * (0 disabled, 1 CPU learning, 2 CPU stable, 3 GPU probe, 4 GPU stable,
   * 5 CPU probe, 6 cooldown), 15 adaptive GPU failure reason valid, 16-18
   * GsplatSurfaceAdaptiveGpuFailureReason, 19 GPU measurement ticket issued. */
  uint32_t flags;
} GsplatSurfaceSortStats;

/* Where a required Surface order refresh is computed. */
typedef enum GsplatSurfaceOrderBackend {
  GSPLAT_SURFACE_ORDER_BACKEND_CPU = 0,
  GSPLAT_SURFACE_ORDER_BACKEND_GPU = 1,
  GSPLAT_SURFACE_ORDER_BACKEND_ADAPTIVE = 2,
} GsplatSurfaceOrderBackend;

/* Timing source for one completed GPU order receipt. */
typedef enum GsplatSurfaceTimingSource {
  GSPLAT_SURFACE_TIMING_SOURCE_UNKNOWN = 0,
  GSPLAT_SURFACE_TIMING_SOURCE_GPU_TIMESTAMP_QUERY = 1,
  GSPLAT_SURFACE_TIMING_SOURCE_GPU_COMPLETION = 2,
} GsplatSurfaceTimingSource;

/* Runtime adaptive-policy state observed when a receipt was harvested. */
typedef enum GsplatSurfaceAdaptiveState {
  GSPLAT_SURFACE_ADAPTIVE_STATE_DISABLED = 0,
  GSPLAT_SURFACE_ADAPTIVE_STATE_CPU_LEARNING = 1,
  GSPLAT_SURFACE_ADAPTIVE_STATE_CPU_STABLE = 2,
  GSPLAT_SURFACE_ADAPTIVE_STATE_GPU_PROBE = 3,
  GSPLAT_SURFACE_ADAPTIVE_STATE_GPU_STABLE = 4,
  GSPLAT_SURFACE_ADAPTIVE_STATE_CPU_PROBE = 5,
  GSPLAT_SURFACE_ADAPTIVE_STATE_COOLDOWN = 6,
} GsplatSurfaceAdaptiveState;

#define GSPLAT_SURFACE_ORDER_MEASUREMENT_PREPROCESS_VALID (1u << 0)
#define GSPLAT_SURFACE_ORDER_MEASUREMENT_RADIX_VALID (1u << 1)
#define GSPLAT_SURFACE_ORDER_MEASUREMENT_ORDER_VALID (1u << 2)
#define GSPLAT_SURFACE_ORDER_MEASUREMENT_TIMESTAMP_PERIOD_VALID (1u << 3)
#define GSPLAT_SURFACE_ORDER_MEASUREMENT_BELOW_TIMESTAMP_RESOLUTION (1u << 4)
#define GSPLAT_SURFACE_ORDER_MEASUREMENT_DROPPED_PRIOR (1u << 5)
/* visible_count is V and drawn_count is the exact stable compacted C draw. */
#define GSPLAT_SURFACE_ORDER_MEASUREMENT_EXACT_CONTRIBUTOR_DRAW (1u << 6)

/*
 * Additive, non-blocking GPU order receipt. Optional floats are zero unless
 * their validity bit is set. `gpu_complete_ms` and visible/drawn counts are
 * always valid. CPU phase timing remains available through GsplatStats.
 */
typedef struct GsplatSurfaceOrderMeasurement {
  uint64_t ticket;
  uint64_t camera_revision;
  float gpu_preprocess_ms;
  float gpu_radix_ms;
  float gpu_order_ms;
  float gpu_complete_ms;
  float timestamp_period_ns;
  uint32_t visible_count;
  uint32_t drawn_count;
  /* GsplatSurfaceTimingSource value. */
  uint32_t timing_source;
  /* GsplatSurfaceOrderBackend requested when this ticket was submitted. */
  uint32_t requested_backend;
  /* Actual backend selected when this ticket was submitted. */
  uint32_t actual_backend;
  /* GsplatSurfaceAdaptiveState observed for this ticket's submission. */
  uint32_t adaptive_state;
  /* GSPLAT_SURFACE_ORDER_MEASUREMENT_* validity/detail bits. */
  uint32_t flags;
} GsplatSurfaceOrderMeasurement;

#define GSPLAT_SURFACE_CPU_ORDER_MEASUREMENT_DROPPED_PRIOR (1u << 0)
/* Join this receipt to frame stats by ticket+camera_revision; then D=C. */
#define GSPLAT_SURFACE_CPU_ORDER_MEASUREMENT_EXACT_CONTRIBUTOR_DRAW (1u << 1)
/* The ABI-reserved word carries the post-projection contributor count C. */
#define GSPLAT_SURFACE_CPU_ORDER_MEASUREMENT_CONTRIBUTOR_COUNT_VALID (1u << 2)

/*
 * Queue-completion receipt for one formally sampled CPU order refresh.
 * `frame_complete_ms` measures frame start through graphics-queue completion;
 * it is not CPU submit-wall time.
 */
typedef struct GsplatSurfaceCpuOrderMeasurement {
  uint64_t ticket;
  uint64_t camera_revision;
  float preprocess_ms;
  float sort_ms;
  float frame_complete_ms;
  uint32_t requested_backend;
  uint32_t actual_backend;
  uint32_t adaptive_state;
  uint32_t flags;
  uint32_t reserved;
} GsplatSurfaceCpuOrderMeasurement;

#define GSPLAT_SURFACE_ORDER_COUNTS_EXACT_CONTRIBUTOR_DRAW (1u << 0)

/*
 * Additive, ticket-addressed V/C/D receipt for either CPU or GPU success.
 * Join by both ticket and camera_revision. Taking a receipt consumes it.
 */
typedef struct GsplatSurfaceOrderCounts {
  uint64_t ticket;
  uint64_t camera_revision;
  uint32_t visible_count;
  uint32_t contributor_count;
  uint32_t drawn_count;
  uint32_t flags;
} GsplatSurfaceOrderCounts;

typedef enum GsplatSurfaceOrderMeasurementFailureReason {
  GSPLAT_SURFACE_ORDER_MEASUREMENT_FAILURE_READBACK_MAP = 1,
  GSPLAT_SURFACE_ORDER_MEASUREMENT_FAILURE_GENERATION_INVALIDATED = 2,
} GsplatSurfaceOrderMeasurementFailureReason;

#define GSPLAT_SURFACE_ORDER_MEASUREMENT_FAILURE_DROPPED_PRIOR (1u << 0)

/* Terminal failure for one issued CPU or GPU measurement ticket. */
typedef struct GsplatSurfaceOrderMeasurementFailure {
  uint64_t ticket;
  uint64_t camera_revision;
  /* GsplatSurfaceOrderMeasurementFailureReason value. */
  uint32_t reason;
  uint32_t requested_backend;
  uint32_t actual_backend;
  uint32_t adaptive_state;
  uint32_t flags;
  uint32_t reserved;
} GsplatSurfaceOrderMeasurementFailure;

#define GSPLAT_SURFACE_ORDER_SUBMISSION_GPU_REFRESH (1u << 0)
#define GSPLAT_SURFACE_ORDER_SUBMISSION_TICKET_ISSUED (1u << 1)
#define GSPLAT_SURFACE_ORDER_SUBMISSION_UNSAMPLED_RING_BUSY (1u << 2)
#define GSPLAT_SURFACE_ORDER_SUBMISSION_CPU_FRAME_COMPLETION_SAMPLE (1u << 3)
#define GSPLAT_SURFACE_ORDER_SUBMISSION_UNSAMPLED_SURFACE_UNAVAILABLE (1u << 4)

/* CPU/GPU measurement submission identity for the last successful render call. */
typedef struct GsplatSurfaceOrderSubmission {
  uint64_t ticket;
  uint64_t camera_revision;
  uint32_t requested_backend;
  uint32_t actual_backend;
  uint32_t adaptive_state;
  uint32_t flags;
} GsplatSurfaceOrderSubmission;

/*
 * Versioned projected-draw ABI. V1 layouts are frozen; any future extension
 * uses new V2 types/symbols. Zero is a setter-only alias for the legacy
 * Adaptive default; successful frame receipts canonicalize it to ADAPTIVE=3.
 */
#define GSPLAT_SURFACE_PROJECTED_ABI_VERSION_V1 1u

typedef enum GsplatSurfaceProjectedPolicyV1 {
  GSPLAT_SURFACE_PROJECTED_POLICY_LEGACY_DEFAULT = 0,
  GSPLAT_SURFACE_PROJECTED_POLICY_CANDIDATE = 1,
  GSPLAT_SURFACE_PROJECTED_POLICY_COMPACT = 2,
  GSPLAT_SURFACE_PROJECTED_POLICY_ADAPTIVE = 3,
} GsplatSurfaceProjectedPolicyV1;

typedef enum GsplatSurfaceProjectedExecutionV1 {
  GSPLAT_SURFACE_PROJECTED_EXECUTION_UNKNOWN = 0,
  GSPLAT_SURFACE_PROJECTED_EXECUTION_CANDIDATE = 1,
  GSPLAT_SURFACE_PROJECTED_EXECUTION_COMPACT = 2,
} GsplatSurfaceProjectedExecutionV1;

typedef enum GsplatSurfaceProjectedAdaptiveStateV1 {
  GSPLAT_SURFACE_PROJECTED_ADAPTIVE_STATE_DISABLED = 0,
  GSPLAT_SURFACE_PROJECTED_ADAPTIVE_STATE_CANDIDATE_LEARNING = 1,
  GSPLAT_SURFACE_PROJECTED_ADAPTIVE_STATE_CANDIDATE_STABLE = 2,
  GSPLAT_SURFACE_PROJECTED_ADAPTIVE_STATE_COMPACT_PROBE = 3,
  GSPLAT_SURFACE_PROJECTED_ADAPTIVE_STATE_COMPACT_STABLE = 4,
  GSPLAT_SURFACE_PROJECTED_ADAPTIVE_STATE_CANDIDATE_PROBE = 5,
  GSPLAT_SURFACE_PROJECTED_ADAPTIVE_STATE_COOLDOWN = 6,
  GSPLAT_SURFACE_PROJECTED_ADAPTIVE_STATE_CANDIDATE_ONLY = 7,
} GsplatSurfaceProjectedAdaptiveStateV1;

#define GSPLAT_SURFACE_PROJECTED_SUBMISSION_TICKET_ISSUED (1u << 0)
#define GSPLAT_SURFACE_PROJECTED_SUBMISSION_UNSAMPLED_RING_BUSY (1u << 1)
#define GSPLAT_SURFACE_PROJECTED_SUBMISSION_UNSAMPLED_SURFACE_UNAVAILABLE (1u << 2)

/*
 * Initialize struct_size=sizeof(struct) and version=1 before every call. The
 * producer writes its canonical V1 size back into struct_size.
 */
typedef struct GsplatSurfaceProjectedSubmissionV1 {
  uint32_t struct_size;
  uint32_t version;
  uint64_t ticket;
  uint64_t camera_revision;
  /* GsplatSurfaceProjectedPolicyV1 value. */
  uint32_t requested_policy;
  /* GsplatSurfaceProjectedExecutionV1 value. */
  uint32_t actual_execution;
  /* Actual GsplatSurfaceOrderBackend used by this frame: CPU=0, GPU=1. */
  uint32_t order_backend;
  /* GsplatSurfaceProjectedAdaptiveStateV1 value for that order lane. */
  uint32_t adaptive_state;
  /* GSPLAT_SURFACE_PROJECTED_SUBMISSION_* bits. */
  uint32_t flags;
  uint32_t reserved;
} GsplatSurfaceProjectedSubmissionV1;

#define GSPLAT_SURFACE_PROJECTED_MEASUREMENT_PROJECTION_REBUILT (1u << 0)
#define GSPLAT_SURFACE_PROJECTED_MEASUREMENT_ORDER_REFRESHED (1u << 1)
#define GSPLAT_SURFACE_PROJECTED_MEASUREMENT_EXACT_CONTRIBUTOR_DRAW (1u << 2)
#define GSPLAT_SURFACE_PROJECTED_MEASUREMENT_DROPPED_PRIOR (1u << 3)

/* Terminal completion receipt; take V/C/D separately by ticket. */
typedef struct GsplatSurfaceProjectedMeasurementV1 {
  uint32_t struct_size;
  uint32_t version;
  uint64_t ticket;
  uint64_t camera_revision;
  uint64_t projection_generation;
  uint64_t probe_generation;
  float frame_complete_ms;
  /* GsplatSurfaceProjectedExecutionV1 value. */
  uint32_t execution;
  /* Actual GsplatSurfaceOrderBackend used by this frame: CPU=0, GPU=1. */
  uint32_t order_backend;
  /* GSPLAT_SURFACE_PROJECTED_MEASUREMENT_* bits. */
  uint32_t flags;
} GsplatSurfaceProjectedMeasurementV1;

#define GSPLAT_SURFACE_PROJECTED_COUNTS_EXACT_CONTRIBUTOR_DRAW (1u << 0)

/* Ticket-addressed exact V/C/D counts. Candidate requires D=V; Compact D=C. */
typedef struct GsplatSurfaceProjectedCountsV1 {
  uint32_t struct_size;
  uint32_t version;
  uint64_t ticket;
  uint64_t camera_revision;
  uint32_t visible_count;
  uint32_t contributor_count;
  uint32_t drawn_count;
  /* GSPLAT_SURFACE_PROJECTED_COUNTS_* bits. */
  uint32_t flags;
} GsplatSurfaceProjectedCountsV1;

typedef enum GsplatSurfaceProjectedFailureReasonV1 {
  GSPLAT_SURFACE_PROJECTED_FAILURE_READBACK_MAP = 1,
  GSPLAT_SURFACE_PROJECTED_FAILURE_GENERATION_INVALIDATED = 2,
  GSPLAT_SURFACE_PROJECTED_FAILURE_INVARIANT_VIOLATION = 3,
} GsplatSurfaceProjectedFailureReasonV1;

#define GSPLAT_SURFACE_PROJECTED_FAILURE_DROPPED_PRIOR (1u << 0)

typedef struct GsplatSurfaceProjectedFailureV1 {
  uint32_t struct_size;
  uint32_t version;
  uint64_t ticket;
  uint64_t camera_revision;
  uint64_t projection_generation;
  uint64_t probe_generation;
  /* GsplatSurfaceProjectedFailureReasonV1 value. */
  uint32_t reason;
  /* GsplatSurfaceProjectedExecutionV1 value. */
  uint32_t execution;
  /* Actual GsplatSurfaceOrderBackend used by this frame: CPU=0, GPU=1. */
  uint32_t order_backend;
  /* GSPLAT_SURFACE_PROJECTED_FAILURE_* bits. */
  uint32_t flags;
} GsplatSurfaceProjectedFailureV1;

/*
 * Versioned diagnostic ABI for the Packed GPU producer graph. Product
 * defaults remain PostSort with measurement disabled. Zero is a setter-only
 * alias for PostSort; successful receipts always use canonical values 1/2.
 */
#define GSPLAT_SURFACE_GPU_PRODUCER_ABI_VERSION_V1 1u

typedef enum GsplatSurfaceGpuProducerV1 {
  GSPLAT_SURFACE_GPU_PRODUCER_LEGACY_DEFAULT = 0,
  GSPLAT_SURFACE_GPU_PRODUCER_POST_SORT = 1,
  GSPLAT_SURFACE_GPU_PRODUCER_PREPROJECT = 2,
} GsplatSurfaceGpuProducerV1;

typedef enum GsplatSurfaceGpuProducerDrawScopeV1 {
  GSPLAT_SURFACE_GPU_PRODUCER_DRAW_SCOPE_UNKNOWN = 0,
  GSPLAT_SURFACE_GPU_PRODUCER_DRAW_SCOPE_EXACT_CURRENT_CONTRIBUTORS = 1,
  GSPLAT_SURFACE_GPU_PRODUCER_DRAW_SCOPE_STALE_ORDER_CANDIDATES = 2,
} GsplatSurfaceGpuProducerDrawScopeV1;

#define GSPLAT_SURFACE_GPU_PRODUCER_SUBMISSION_TICKET_ISSUED (1u << 0)
#define GSPLAT_SURFACE_GPU_PRODUCER_SUBMISSION_UNSAMPLED_RING_BUSY (1u << 1)
#define GSPLAT_SURFACE_GPU_PRODUCER_SUBMISSION_UNSAMPLED_SURFACE_UNAVAILABLE (1u << 2)
#define GSPLAT_SURFACE_GPU_PRODUCER_SUBMISSION_MEASUREMENT_ENABLED (1u << 3)

typedef struct GsplatSurfaceGpuProducerSubmissionV1 {
  uint32_t struct_size;
  uint32_t version;
  uint64_t ticket;
  uint64_t camera_revision;
  /* Requested GsplatSurfaceGpuProducerV1 value. */
  uint32_t requested_producer;
  /* Actual GsplatSurfaceGpuProducerV1, or zero outside a Packed GPU frame. */
  uint32_t actual_producer;
  /* Actual GsplatSurfaceOrderBackend: producer evidence requires GPU=1. */
  uint32_t order_backend;
  /* Actual GsplatSurfaceProjectedExecutionV1: strict A/B requires COMPACT=2. */
  uint32_t projected_execution;
  /* GSPLAT_SURFACE_GPU_PRODUCER_SUBMISSION_* bits. */
  uint32_t flags;
  uint32_t reserved;
} GsplatSurfaceGpuProducerSubmissionV1;

#define GSPLAT_SURFACE_GPU_PRODUCER_MEASUREMENT_ORDER_REFRESHED (1u << 0)
#define GSPLAT_SURFACE_GPU_PRODUCER_MEASUREMENT_EXACT_CURRENT_DRAW (1u << 1)
#define GSPLAT_SURFACE_GPU_PRODUCER_MEASUREMENT_STALE_ORDER (1u << 2)
#define GSPLAT_SURFACE_GPU_PRODUCER_MEASUREMENT_DROPPED_PRIOR (1u << 3)

/* Terminal producer success with exact source/contributor/drawn (S/C/D). */
typedef struct GsplatSurfaceGpuProducerMeasurementV1 {
  uint32_t struct_size;
  uint32_t version;
  uint64_t ticket;
  uint64_t camera_revision;
  uint64_t order_generation;
  uint64_t projection_generation;
  float frame_complete_ms;
  /* GsplatSurfaceGpuProducerV1 value. */
  uint32_t producer;
  uint32_t source_count;
  uint32_t contributor_count;
  uint32_t drawn_count;
  /* GsplatSurfaceGpuProducerDrawScopeV1 value. */
  uint32_t draw_scope;
  /* GSPLAT_SURFACE_GPU_PRODUCER_MEASUREMENT_* bits. */
  uint32_t flags;
  uint32_t reserved;
} GsplatSurfaceGpuProducerMeasurementV1;

typedef enum GsplatSurfaceGpuProducerFailureReasonV1 {
  GSPLAT_SURFACE_GPU_PRODUCER_FAILURE_READBACK_MAP = 1,
  GSPLAT_SURFACE_GPU_PRODUCER_FAILURE_GENERATION_INVALIDATED = 2,
  GSPLAT_SURFACE_GPU_PRODUCER_FAILURE_INVARIANT_VIOLATION = 3,
} GsplatSurfaceGpuProducerFailureReasonV1;

#define GSPLAT_SURFACE_GPU_PRODUCER_FAILURE_DROPPED_PRIOR (1u << 0)

typedef struct GsplatSurfaceGpuProducerFailureV1 {
  uint32_t struct_size;
  uint32_t version;
  uint64_t ticket;
  uint64_t camera_revision;
  uint64_t order_generation;
  uint64_t projection_generation;
  /* GsplatSurfaceGpuProducerFailureReasonV1 value. */
  uint32_t reason;
  /* GsplatSurfaceGpuProducerV1 value. */
  uint32_t producer;
  /* GSPLAT_SURFACE_GPU_PRODUCER_FAILURE_* bits. */
  uint32_t flags;
  uint32_t reserved;
} GsplatSurfaceGpuProducerFailureV1;

#define GSPLAT_SURFACE_EXACTNESS_SOURCE_MEMBERSHIP_ALL (1u << 0)
#define GSPLAT_SURFACE_EXACTNESS_SAMPLING_DISABLED (1u << 1)
#define GSPLAT_SURFACE_EXACTNESS_LOD_DISABLED (1u << 2)
#define GSPLAT_SURFACE_EXACTNESS_SH_DEGREE_SOURCE (1u << 3)
#define GSPLAT_SURFACE_EXACTNESS_PARTIAL_SCENE_NOT_PUBLISHED (1u << 4)

/*
 * Source-to-GPU exactness receipt for the Surface renderer's current geometry
 * path. Full-quality Direct/Packed handles set all five
 * GSPLAT_SURFACE_EXACTNESS_* bits. A successful geometry-path switch updates
 * the receipt. The final two fields expose the physical adapter limits used by
 * resident admission.
 */
typedef struct GsplatSurfaceExactness {
  uint64_t source_splat_count;
  uint64_t decoded_splat_count;
  uint64_t encoded_splat_count;
  uint64_t resident_splat_count;
  uint64_t addressable_splat_count;
  uint32_t source_sh_degree;
  uint32_t resident_sh_degree;
  uint32_t quality_flags;
  uint32_t max_storage_buffers_per_shader_stage;
  uint64_t max_storage_buffer_binding_size;
} GsplatSurfaceExactness;

#define GSPLAT_SURFACE_PRESENTATION_LAST_FRAME_PRESENTED (1u << 0)
#define GSPLAT_SURFACE_PRESENTATION_EVER_PRESENTED (1u << 1)
#define GSPLAT_SURFACE_PRESENTATION_DYNAMIC_RESOLUTION_DISABLED (1u << 2)
#define GSPLAT_SURFACE_PRESENTATION_UPSCALING_DISABLED (1u << 3)
#define GSPLAT_SURFACE_PRESENTATION_FULL_RESOLUTION (1u << 4)

/*
 * Native pixel-resolution receipt. The internal target is rendered directly
 * into the Surface path: there is no dynamic-resolution stage or upscaler.
 * FULL_RESOLUTION is set only when the last frame was actually presented and
 * requested, Surface, internal-render, and presented dimensions all match.
 */
typedef struct GsplatSurfacePresentation {
  uint32_t requested_width;
  uint32_t requested_height;
  uint32_t surface_width;
  uint32_t surface_height;
  uint32_t internal_render_width;
  uint32_t internal_render_height;
  uint32_t presented_width;
  uint32_t presented_height;
  uint64_t presented_camera_revision;
  uint32_t flags;
  uint32_t reserved;
} GsplatSurfacePresentation;

#define GSPLAT_SURFACE_CAMERA_RECEIPT_ABI_VERSION_V1 1u
#define GSPLAT_SURFACE_CAMERA_RECEIPT_FRAME_PRESENTED (1u << 0)
#define GSPLAT_SURFACE_CAMERA_RECEIPT_CURRENT_REVISION_PRESENTED (1u << 1)

/*
 * Read-only camera state used by strict native benchmark evidence.
 *
 * Matrices are canonical row-major arrays for column vectors, with
 * `view_projection = projection * view`, right-handed RUF world/camera axes,
 * camera forward +Z, NDC x/y in [-1,1], and NDC z in [0,1]. Values are
 * derived from the renderer's actual f32 camera state and current Surface
 * aspect; they are not copied from a benchmark trace.
 *
 * Callers initialize `struct_size` and `version`. Strict evidence requires
 * both flags, equal camera/presented revisions, and a successful query made
 * immediately after the matching render call.
 */
typedef struct GsplatSurfaceCameraReceiptV1 {
  uint32_t struct_size;
  uint32_t version;
  uint64_t camera_revision;
  uint64_t presented_camera_revision;
  uint32_t surface_width;
  uint32_t surface_height;
  uint32_t flags;
  uint32_t reserved;
  float position[3];
  float rotation_xyzw[4];
  float vertical_fov_radians;
  float near_plane;
  float far_plane;
  float view_matrix[16];
  float projection_matrix[16];
  float view_projection_matrix[16];
} GsplatSurfaceCameraReceiptV1;

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
const char *gsplat_last_error_message(void);

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
int32_t gsplat_surface_renderer_create_android_with_geometry_path(
    void *native_window,
    const char *path,
    uint32_t width,
    uint32_t height,
    uint32_t geometry_path,
    GsplatSurfaceRenderer **out_renderer);
int32_t gsplat_surface_renderer_create_uikit(
    void *ui_view,
    void *ui_view_controller,
    const char *path,
    uint32_t width,
    uint32_t height,
    GsplatSurfaceRenderer **out_renderer);
int32_t gsplat_surface_renderer_create_uikit_with_geometry_path(
    void *ui_view,
    void *ui_view_controller,
    const char *path,
    uint32_t width,
    uint32_t height,
    uint32_t geometry_path,
    GsplatSurfaceRenderer **out_renderer);
void gsplat_surface_renderer_destroy(GsplatSurfaceRenderer *renderer);
int32_t gsplat_surface_renderer_resize(
    GsplatSurfaceRenderer *renderer,
    uint32_t width,
    uint32_t height);
int32_t gsplat_surface_renderer_set_sort_interval(
    GsplatSurfaceRenderer *renderer,
    uint32_t interval);
int32_t gsplat_surface_renderer_set_order_backend(
    GsplatSurfaceRenderer *renderer,
    uint32_t backend);
/*
 * Experimental A/B benchmark knob: switch between the exact packed resident
 * path (the mobile-constructor default), direct wide-float oracle, and
 * diagnostic local-source paged active atlas. `path` is a GsplatGeometryPath
 * value. May change before a published mobile SDK.
 */
int32_t gsplat_surface_renderer_set_geometry_path(
    GsplatSurfaceRenderer *renderer,
    uint32_t path);
/*
 * v0.1 ABI compatibility no-ops. Rendering always uses the resident-scene
 * sorted-index pipeline selected by gsplat_surface_renderer_set_geometry_path;
 * new integrations should not call these.
 */
int32_t gsplat_surface_renderer_set_gpu_preproject(
    GsplatSurfaceRenderer *renderer,
    uint32_t enabled);
int32_t gsplat_surface_renderer_set_gpu_preproject_double_buffer(
    GsplatSurfaceRenderer *renderer,
    uint32_t enabled);
int32_t gsplat_surface_renderer_set_async_sort(
    GsplatSurfaceRenderer *renderer,
    uint32_t enabled);
/* v0.1 ABI compatibility no-op; CPU geometry expansion was removed. */
int32_t gsplat_surface_renderer_set_async_geometry(
    GsplatSurfaceRenderer *renderer,
    uint32_t enabled);
/* v0.1 ABI compatibility no-op; direct rendering has no instance-buffer ring. */
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
int32_t gsplat_surface_renderer_get_exactness(
    const GsplatSurfaceRenderer *renderer,
    GsplatSurfaceExactness *out_exactness);
int32_t gsplat_surface_renderer_get_presentation(
    const GsplatSurfaceRenderer *renderer,
    GsplatSurfacePresentation *out_presentation);
int32_t gsplat_surface_renderer_get_camera_receipt_v1(
    const GsplatSurfaceRenderer *renderer,
    GsplatSurfaceCameraReceiptV1 *out_receipt);
int32_t gsplat_surface_renderer_get_sort_stats(
    const GsplatSurfaceRenderer *renderer,
    GsplatSurfaceSortStats *out_stats);
int32_t gsplat_surface_renderer_get_order_submission(
    const GsplatSurfaceRenderer *renderer,
    GsplatSurfaceOrderSubmission *out_submission);
/*
 * Independent exact projected-draw policy. Forced Candidate/Compact controls
 * execution but does not automatically issue projected measurement tickets;
 * Adaptive formal probes do. After polling a success, immediately take its
 * counts by ticket. available=0 then means the bounded evidence expired and a
 * strict retained run must be rejected.
 */
int32_t gsplat_surface_renderer_set_projected_policy_v1(
    GsplatSurfaceRenderer *renderer,
    uint32_t policy);
int32_t gsplat_surface_renderer_get_projected_submission_v1(
    const GsplatSurfaceRenderer *renderer,
    GsplatSurfaceProjectedSubmissionV1 *out_submission);
int32_t gsplat_surface_renderer_poll_projected_measurement_v1(
    GsplatSurfaceRenderer *renderer,
    GsplatSurfaceProjectedMeasurementV1 *out_measurement,
    uint32_t *out_available);
int32_t gsplat_surface_renderer_take_projected_counts_v1(
    GsplatSurfaceRenderer *renderer,
    uint64_t ticket,
    GsplatSurfaceProjectedCountsV1 *out_counts,
    uint32_t *out_available);
int32_t gsplat_surface_renderer_poll_projected_failure_v1(
    GsplatSurfaceRenderer *renderer,
    GsplatSurfaceProjectedFailureV1 *out_failure,
    uint32_t *out_available);
/*
 * Strict diagnostic producer lane. Select the graph while measurement is
 * disabled, then enable receipts only under Packed + ProjectedQuadsExact +
 * forced Compact. CPU/GPU ordering remains independent; retained Android A/B
 * evidence additionally requires the forced GPU backend. Disabling stops new
 * tickets but preserves the last successful-render submission and every
 * already-issued terminal success/failure. Drain both terminal queues before
 * treating a later enabled period as a fresh strict experiment.
 */
int32_t gsplat_surface_renderer_set_gpu_order_producer_v1(
    GsplatSurfaceRenderer *renderer,
    uint32_t producer);
int32_t gsplat_surface_renderer_set_gpu_producer_measurement_enabled_v1(
    GsplatSurfaceRenderer *renderer,
    uint32_t enabled);
int32_t gsplat_surface_renderer_get_gpu_producer_submission_v1(
    const GsplatSurfaceRenderer *renderer,
    GsplatSurfaceGpuProducerSubmissionV1 *out_submission);
int32_t gsplat_surface_renderer_poll_gpu_producer_measurement_v1(
    GsplatSurfaceRenderer *renderer,
    GsplatSurfaceGpuProducerMeasurementV1 *out_measurement,
    uint32_t *out_available);
int32_t gsplat_surface_renderer_poll_gpu_producer_failure_v1(
    GsplatSurfaceRenderer *renderer,
    GsplatSurfaceGpuProducerFailureV1 *out_failure,
    uint32_t *out_available);
/* Drain one completed CPU frame-start -> queue-done order receipt. */
int32_t gsplat_surface_renderer_poll_cpu_order_measurement(
    GsplatSurfaceRenderer *renderer,
    GsplatSurfaceCpuOrderMeasurement *out_measurement,
    uint32_t *out_available);
/*
 * Drain one completed GPU order receipt. On success, `out_available` is 1
 * when a receipt was written or 0 when the non-blocking queue is empty.
 * Repeat until `out_available` is 0 to drain the current queue.
 */
int32_t gsplat_surface_renderer_poll_order_measurement(
    GsplatSurfaceRenderer *renderer,
    GsplatSurfaceOrderMeasurement *out_measurement,
    uint32_t *out_available);
/* Take one successful CPU/GPU V/C/D receipt by non-zero ticket. */
int32_t gsplat_surface_renderer_take_order_counts(
    GsplatSurfaceRenderer *renderer,
    uint64_t ticket,
    GsplatSurfaceOrderCounts *out_counts,
    uint32_t *out_available);
/*
 * Drain one terminal failure for an issued CPU or GPU order-measurement ticket.
 * Repeat until `out_available` is 0. A ticket appears in exactly one of the
 * success/failure queues while the live renderer continues to be pumped.
 */
int32_t gsplat_surface_renderer_poll_order_measurement_failure(
    GsplatSurfaceRenderer *renderer,
    GsplatSurfaceOrderMeasurementFailure *out_failure,
    uint32_t *out_available);

#ifdef __cplusplus
}
#endif

#endif  // GSPLAT_FFI_C_GSPLAT_H
