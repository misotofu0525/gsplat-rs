# Findings: Packed Atlas Branch Closeout

## Branch Shape

- `origin/main` is the exact merge base; the branch is 0 behind and 50 commits ahead.
- The pre-closeout tree changes 113 files by about +23,530/-1,028 lines.
- Renderer Rust grew from 3,378 to 10,736 lines, mostly through Packed, SPZ,
  local paging, and evidence infrastructure.

## Retained Value

- Packed stores the draw hot record in 20 bytes per splat and keeps Direct as default.
- Resource preflight reports binding failures before allocation.
- Benchmark artifacts bind raw frames to dataset, trace, backend, and revision facts.
- The pinned PlayCanvas harness plus SSIM and paired statistics prevent broad,
  unsupported comparison claims.
- Surface/draw responsibility extraction reduces duplicate Direct/Packed/Paged orchestration.

## Problems Found

- Packed allocated and uploaded an RGBA8Uint SH texture at 48 bytes per splat,
  but its bind group and shader exposed only sorted IDs, hot records, and params.
  Its CPU pack also built the full sidecar transiently, and obsolete hot-atlas
  dimensions still caused false texture-dimension paging decisions after the
  texture was removed.
- Packed CPU records start with DC color. Surface rendering skipped SH refresh
  whenever the initial order was uploaded, so the first frame was degree-0 only.
- A banded color refresh stored only a position key while evaluating each band
  against the newest camera, allowing one completed color buffer to mix views.
- The Auto Surface constructors had no repository consumer and exposed product
  policy before the C/Web/mobile contract had chosen an Auto value.
- The local Paged runtime retains full `SceneBuffers` and page indices, performs
  synchronous schedule/decode/pack/sort/color work, and therefore is not
  end-to-end bounded streaming.
- Paging implementation types were public only to avoid dead-code warnings;
  they had no repository consumer outside the renderer crate.

## Evidence Boundary

- Historical Chrome/WebGPU Kitsune-static evidence reported five paired runs,
  p95 ratio 1.0200 and minimum SSIM 0.998657. It is not exact-closeout-HEAD
  evidence and does not imply broad browser or native leadership.
- Historical physical A065 evidence reported Direct 279,199 splats at 11.330
  ms/frame and Paged 225,784 active splats at 23.626 ms/frame. This is evidence
  of Paged execution and bounded GPU slots, not a performance win.
- Missing external Kitsune/Flowers datasets cause optional local parity tests to
  skip; committed minimal/synthetic fixtures and CI remain the reproducible gates.

## Next Competitive Priority

Keep the graphics draw and add capability-gated GPU visible compaction, a
portable radix sort, and indirect draw to the Direct path. Validate Bonsai,
Bicycle/Garden, and Kitsune at representative desktop/mobile resolutions before
restarting true streaming work.
