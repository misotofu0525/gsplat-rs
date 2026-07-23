#include <stdio.h>
#include <stddef.h>
#include <string.h>

#include "../../crates/gsplat-ffi-c/include/gsplat.h"

_Static_assert(sizeof(GsplatSurfaceSortStats) == 48, "legacy sort stats ABI changed");
_Static_assert(sizeof(GsplatSurfaceOrderMeasurement) == 64, "order receipt ABI changed");
_Static_assert(sizeof(GsplatSurfaceCpuOrderMeasurement) == 48, "CPU order receipt ABI changed");
_Static_assert(sizeof(GsplatSurfaceOrderCounts) == 32, "order counts receipt ABI changed");
_Static_assert(sizeof(GsplatSurfaceOrderMeasurementFailure) == 40, "order failure ABI changed");
_Static_assert(sizeof(GsplatSurfaceOrderSubmission) == 32, "order submission ABI changed");
_Static_assert(sizeof(GsplatSurfaceProjectedSubmissionV1) == 48, "projected submission v1 ABI changed");
_Static_assert(sizeof(GsplatSurfaceProjectedMeasurementV1) == 56, "projected measurement v1 ABI changed");
_Static_assert(sizeof(GsplatSurfaceProjectedCountsV1) == 40, "projected counts v1 ABI changed");
_Static_assert(sizeof(GsplatSurfaceProjectedFailureV1) == 56, "projected failure v1 ABI changed");
_Static_assert(sizeof(GsplatSurfaceGpuProducerSubmissionV1) == 48, "GPU producer submission v1 ABI changed");
_Static_assert(sizeof(GsplatSurfaceGpuProducerMeasurementV1) == 72, "GPU producer measurement v1 ABI changed");
_Static_assert(sizeof(GsplatSurfaceGpuProducerFailureV1) == 56, "GPU producer failure v1 ABI changed");
_Static_assert(offsetof(GsplatSurfaceProjectedSubmissionV1, ticket) == 8, "projected submission ticket offset changed");
_Static_assert(offsetof(GsplatSurfaceProjectedSubmissionV1, flags) == 40, "projected submission flags offset changed");
_Static_assert(offsetof(GsplatSurfaceProjectedMeasurementV1, frame_complete_ms) == 40, "projected measurement time offset changed");
_Static_assert(offsetof(GsplatSurfaceProjectedCountsV1, visible_count) == 24, "projected counts offset changed");
_Static_assert(offsetof(GsplatSurfaceProjectedFailureV1, reason) == 40, "projected failure reason offset changed");
_Static_assert(offsetof(GsplatSurfaceGpuProducerSubmissionV1, ticket) == 8, "GPU producer submission ticket offset changed");
_Static_assert(offsetof(GsplatSurfaceGpuProducerMeasurementV1, frame_complete_ms) == 40, "GPU producer completion offset changed");
_Static_assert(offsetof(GsplatSurfaceGpuProducerFailureV1, reason) == 40, "GPU producer failure reason offset changed");
_Static_assert(_Alignof(GsplatSurfaceProjectedSubmissionV1) == _Alignof(uint64_t), "projected submission alignment changed");
_Static_assert(_Alignof(GsplatSurfaceProjectedMeasurementV1) == _Alignof(uint64_t), "projected measurement alignment changed");
_Static_assert(_Alignof(GsplatSurfaceProjectedCountsV1) == _Alignof(uint64_t), "projected counts alignment changed");
_Static_assert(_Alignof(GsplatSurfaceProjectedFailureV1) == _Alignof(uint64_t), "projected failure alignment changed");
_Static_assert(_Alignof(GsplatSurfaceGpuProducerSubmissionV1) == _Alignof(uint64_t), "GPU producer submission alignment changed");
_Static_assert(_Alignof(GsplatSurfaceGpuProducerMeasurementV1) == _Alignof(uint64_t), "GPU producer measurement alignment changed");
_Static_assert(_Alignof(GsplatSurfaceGpuProducerFailureV1) == _Alignof(uint64_t), "GPU producer failure alignment changed");
_Static_assert(sizeof(GsplatSurfaceExactness) == 64, "exactness receipt ABI changed");
_Static_assert(sizeof(GsplatSurfacePresentation) == 48, "presentation receipt ABI changed");
_Static_assert(sizeof(GsplatSurfaceCameraReceiptV1) == 272, "camera receipt v1 ABI changed");
_Static_assert(offsetof(GsplatSurfaceCameraReceiptV1, camera_revision) == 8, "camera receipt revision offset changed");
_Static_assert(offsetof(GsplatSurfaceCameraReceiptV1, view_matrix) == 80, "camera receipt matrix offset changed");
_Static_assert(_Alignof(GsplatSurfaceCameraReceiptV1) == _Alignof(uint64_t), "camera receipt alignment changed");

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

  if (GSPLAT_SURFACE_ORDER_BACKEND_CPU != 0 ||
      GSPLAT_SURFACE_ORDER_BACKEND_GPU != 1 ||
      GSPLAT_SURFACE_ORDER_BACKEND_ADAPTIVE != 2 ||
      GSPLAT_SURFACE_TIMING_SOURCE_GPU_TIMESTAMP_QUERY != 1 ||
      GSPLAT_SURFACE_TIMING_SOURCE_GPU_COMPLETION != 2 ||
      GSPLAT_SURFACE_ADAPTIVE_GPU_FAILURE_UNSUPPORTED != 1 ||
      GSPLAT_SURFACE_ADAPTIVE_GPU_FAILURE_INITIALIZATION != 2 ||
      GSPLAT_SURFACE_ADAPTIVE_GPU_FAILURE_OUT_OF_MEMORY != 3 ||
      GSPLAT_SURFACE_ADAPTIVE_GPU_FAILURE_VALIDATION != 4 ||
      GSPLAT_SURFACE_ORDER_MEASUREMENT_FAILURE_READBACK_MAP != 1 ||
      GSPLAT_SURFACE_ORDER_MEASUREMENT_FAILURE_GENERATION_INVALIDATED != 2 ||
      GSPLAT_SURFACE_PROJECTED_ABI_VERSION_V1 != 1 ||
      GSPLAT_SURFACE_CAMERA_RECEIPT_ABI_VERSION_V1 != 1 ||
      GSPLAT_SURFACE_PROJECTED_POLICY_LEGACY_DEFAULT != 0 ||
      GSPLAT_SURFACE_PROJECTED_POLICY_CANDIDATE != 1 ||
      GSPLAT_SURFACE_PROJECTED_POLICY_COMPACT != 2 ||
      GSPLAT_SURFACE_PROJECTED_POLICY_ADAPTIVE != 3 ||
      GSPLAT_SURFACE_PROJECTED_EXECUTION_CANDIDATE != 1 ||
      GSPLAT_SURFACE_PROJECTED_EXECUTION_COMPACT != 2 ||
      GSPLAT_SURFACE_PROJECTED_FAILURE_READBACK_MAP != 1 ||
      GSPLAT_SURFACE_PROJECTED_FAILURE_GENERATION_INVALIDATED != 2 ||
      GSPLAT_SURFACE_PROJECTED_FAILURE_INVARIANT_VIOLATION != 3 ||
      (GSPLAT_SURFACE_PROJECTED_SUBMISSION_TICKET_ISSUED |
       GSPLAT_SURFACE_PROJECTED_SUBMISSION_UNSAMPLED_RING_BUSY |
       GSPLAT_SURFACE_PROJECTED_SUBMISSION_UNSAMPLED_SURFACE_UNAVAILABLE) != 7u ||
      GSPLAT_SURFACE_GPU_PRODUCER_ABI_VERSION_V1 != 1 ||
      GSPLAT_SURFACE_GPU_PRODUCER_LEGACY_DEFAULT != 0 ||
      GSPLAT_SURFACE_GPU_PRODUCER_POST_SORT != 1 ||
      GSPLAT_SURFACE_GPU_PRODUCER_PREPROJECT != 2 ||
      GSPLAT_SURFACE_GPU_PRODUCER_DRAW_SCOPE_EXACT_CURRENT_CONTRIBUTORS != 1 ||
      GSPLAT_SURFACE_GPU_PRODUCER_DRAW_SCOPE_STALE_ORDER_CANDIDATES != 2 ||
      GSPLAT_SURFACE_GPU_PRODUCER_FAILURE_INVARIANT_VIOLATION != 3 ||
      (GSPLAT_SURFACE_GPU_PRODUCER_SUBMISSION_TICKET_ISSUED |
       GSPLAT_SURFACE_GPU_PRODUCER_SUBMISSION_UNSAMPLED_RING_BUSY |
       GSPLAT_SURFACE_GPU_PRODUCER_SUBMISSION_UNSAMPLED_SURFACE_UNAVAILABLE |
       GSPLAT_SURFACE_GPU_PRODUCER_SUBMISSION_MEASUREMENT_ENABLED) != 15u ||
      (GSPLAT_SURFACE_EXACTNESS_SOURCE_MEMBERSHIP_ALL |
       GSPLAT_SURFACE_EXACTNESS_SAMPLING_DISABLED |
       GSPLAT_SURFACE_EXACTNESS_LOD_DISABLED |
       GSPLAT_SURFACE_EXACTNESS_SH_DEGREE_SOURCE |
       GSPLAT_SURFACE_EXACTNESS_PARTIAL_SCENE_NOT_PUBLISHED) != 31u ||
      (GSPLAT_SURFACE_PRESENTATION_DYNAMIC_RESOLUTION_DISABLED |
       GSPLAT_SURFACE_PRESENTATION_UPSCALING_DISABLED) != 12u) {
    fprintf(stderr, "unexpected Surface order ABI constants\n");
    return 10;
  }

  GsplatSurfaceOrderMeasurement measurement;
  memset(&measurement, 0, sizeof(measurement));
  GsplatSurfaceCameraReceiptV1 camera_receipt = {
      .struct_size = sizeof(GsplatSurfaceCameraReceiptV1),
      .version = GSPLAT_SURFACE_CAMERA_RECEIPT_ABI_VERSION_V1,
  };
  uint32_t measurement_available = 99;
  int32_t rc = gsplat_surface_renderer_set_order_backend(
      NULL,
      GSPLAT_SURFACE_ORDER_BACKEND_ADAPTIVE);
  if (rc != GSPLAT_ERROR_INVALID_ARGUMENT) {
    fprintf(stderr, "expected null Surface backend setter to fail, got: %d\n", rc);
    return 11;
  }
  rc = gsplat_surface_renderer_get_camera_receipt_v1(NULL, &camera_receipt);
  if (rc != GSPLAT_ERROR_INVALID_ARGUMENT) {
    fprintf(stderr, "expected null Surface camera receipt query to fail, got: %d\n", rc);
    return 17;
  }
  rc = gsplat_surface_renderer_poll_order_measurement(
      NULL,
      &measurement,
      &measurement_available);
  if (rc != GSPLAT_ERROR_INVALID_ARGUMENT) {
    fprintf(stderr, "expected null Surface measurement poll to fail, got: %d\n", rc);
    return 12;
  }
  GsplatSurfaceOrderMeasurementFailure failure;
  memset(&failure, 0, sizeof(failure));
  rc = gsplat_surface_renderer_poll_order_measurement_failure(
      NULL,
      &failure,
      &measurement_available);
  if (rc != GSPLAT_ERROR_INVALID_ARGUMENT) {
    fprintf(stderr, "expected null Surface failure poll to fail, got: %d\n", rc);
    return 14;
  }
  GsplatSurfaceCpuOrderMeasurement cpu_measurement;
  memset(&cpu_measurement, 0, sizeof(cpu_measurement));
  rc = gsplat_surface_renderer_poll_cpu_order_measurement(
      NULL,
      &cpu_measurement,
      &measurement_available);
  if (rc != GSPLAT_ERROR_INVALID_ARGUMENT) {
    fprintf(stderr, "expected null CPU Surface measurement poll to fail, got: %d\n", rc);
    return 16;
  }
  GsplatSurfaceOrderSubmission submission;
  memset(&submission, 0, sizeof(submission));
  rc = gsplat_surface_renderer_get_order_submission(NULL, &submission);
  if (rc != GSPLAT_ERROR_INVALID_ARGUMENT) {
    fprintf(stderr, "expected null Surface submission query to fail, got: %d\n", rc);
    return 15;
  }
  GsplatSurfaceOrderCounts counts;
  memset(&counts, 0, sizeof(counts));
  rc = gsplat_surface_renderer_take_order_counts(
      NULL,
      1,
      &counts,
      &measurement_available);
  if (rc != GSPLAT_ERROR_INVALID_ARGUMENT) {
    fprintf(stderr, "expected null Surface order-count query to fail, got: %d\n", rc);
    return 18;
  }
  GsplatSurfaceProjectedSubmissionV1 projected_submission = {
      .struct_size = sizeof(GsplatSurfaceProjectedSubmissionV1),
      .version = GSPLAT_SURFACE_PROJECTED_ABI_VERSION_V1,
  };
  rc = gsplat_surface_renderer_set_projected_policy_v1(
      NULL,
      GSPLAT_SURFACE_PROJECTED_POLICY_ADAPTIVE);
  if (rc != GSPLAT_ERROR_INVALID_ARGUMENT) {
    fprintf(stderr, "expected null projected policy setter to fail, got: %d\n", rc);
    return 19;
  }
  rc = gsplat_surface_renderer_get_projected_submission_v1(NULL, &projected_submission);
  if (rc != GSPLAT_ERROR_INVALID_ARGUMENT) {
    fprintf(stderr, "expected null projected submission query to fail, got: %d\n", rc);
    return 20;
  }
  GsplatSurfaceProjectedMeasurementV1 projected_measurement = {
      .struct_size = sizeof(GsplatSurfaceProjectedMeasurementV1),
      .version = GSPLAT_SURFACE_PROJECTED_ABI_VERSION_V1,
  };
  rc = gsplat_surface_renderer_poll_projected_measurement_v1(
      NULL,
      &projected_measurement,
      &measurement_available);
  if (rc != GSPLAT_ERROR_INVALID_ARGUMENT) {
    fprintf(stderr, "expected null projected measurement poll to fail, got: %d\n", rc);
    return 21;
  }
  GsplatSurfaceProjectedCountsV1 projected_counts = {
      .struct_size = sizeof(GsplatSurfaceProjectedCountsV1),
      .version = GSPLAT_SURFACE_PROJECTED_ABI_VERSION_V1,
  };
  rc = gsplat_surface_renderer_take_projected_counts_v1(
      NULL,
      1,
      &projected_counts,
      &measurement_available);
  if (rc != GSPLAT_ERROR_INVALID_ARGUMENT) {
    fprintf(stderr, "expected null projected counts query to fail, got: %d\n", rc);
    return 22;
  }
  GsplatSurfaceProjectedFailureV1 projected_failure = {
      .struct_size = sizeof(GsplatSurfaceProjectedFailureV1),
      .version = GSPLAT_SURFACE_PROJECTED_ABI_VERSION_V1,
  };
  rc = gsplat_surface_renderer_poll_projected_failure_v1(
      NULL,
      &projected_failure,
      &measurement_available);
  if (rc != GSPLAT_ERROR_INVALID_ARGUMENT) {
    fprintf(stderr, "expected null projected failure poll to fail, got: %d\n", rc);
    return 23;
  }
  GsplatSurfaceGpuProducerSubmissionV1 producer_submission = {
      .struct_size = sizeof(GsplatSurfaceGpuProducerSubmissionV1),
      .version = GSPLAT_SURFACE_GPU_PRODUCER_ABI_VERSION_V1,
  };
  rc = gsplat_surface_renderer_set_gpu_order_producer_v1(
      NULL,
      GSPLAT_SURFACE_GPU_PRODUCER_POST_SORT);
  if (rc != GSPLAT_ERROR_INVALID_ARGUMENT) {
    fprintf(stderr, "expected null GPU producer setter to fail, got: %d\n", rc);
    return 24;
  }
  rc = gsplat_surface_renderer_set_gpu_producer_measurement_enabled_v1(NULL, 1);
  if (rc != GSPLAT_ERROR_INVALID_ARGUMENT) {
    fprintf(stderr, "expected null GPU producer measurement setter to fail, got: %d\n", rc);
    return 25;
  }
  rc = gsplat_surface_renderer_get_gpu_producer_submission_v1(NULL, &producer_submission);
  if (rc != GSPLAT_ERROR_INVALID_ARGUMENT) {
    fprintf(stderr, "expected null GPU producer submission query to fail, got: %d\n", rc);
    return 26;
  }
  GsplatSurfaceGpuProducerMeasurementV1 producer_measurement = {
      .struct_size = sizeof(GsplatSurfaceGpuProducerMeasurementV1),
      .version = GSPLAT_SURFACE_GPU_PRODUCER_ABI_VERSION_V1,
  };
  rc = gsplat_surface_renderer_poll_gpu_producer_measurement_v1(
      NULL,
      &producer_measurement,
      &measurement_available);
  if (rc != GSPLAT_ERROR_INVALID_ARGUMENT) {
    fprintf(stderr, "expected null GPU producer measurement poll to fail, got: %d\n", rc);
    return 27;
  }
  GsplatSurfaceGpuProducerFailureV1 producer_failure = {
      .struct_size = sizeof(GsplatSurfaceGpuProducerFailureV1),
      .version = GSPLAT_SURFACE_GPU_PRODUCER_ABI_VERSION_V1,
  };
  rc = gsplat_surface_renderer_poll_gpu_producer_failure_v1(
      NULL,
      &producer_failure,
      &measurement_available);
  if (rc != GSPLAT_ERROR_INVALID_ARGUMENT) {
    fprintf(stderr, "expected null GPU producer failure poll to fail, got: %d\n", rc);
    return 28;
  }
  GsplatSurfaceExactness exactness;
  memset(&exactness, 0, sizeof(exactness));
  rc = gsplat_surface_renderer_get_exactness(NULL, &exactness);
  if (rc != GSPLAT_ERROR_INVALID_ARGUMENT) {
    fprintf(stderr, "expected null Surface exactness query to fail, got: %d\n", rc);
    return 13;
  }
  GsplatSurfacePresentation presentation;
  memset(&presentation, 0, sizeof(presentation));
  rc = gsplat_surface_renderer_get_presentation(NULL, &presentation);
  if (rc != GSPLAT_ERROR_INVALID_ARGUMENT) {
    fprintf(stderr, "expected null Surface presentation query to fail, got: %d\n", rc);
    return 17;
  }

  GsplatConfig unsupported_config = gsplat_config_default();
  unsupported_config.mode = GSPLAT_RENDER_MODE_SORTED_ALPHA + 1;
  GsplatContext *unsupported_ctx = NULL;
  rc = gsplat_context_create(unsupported_config, &unsupported_ctx);
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
